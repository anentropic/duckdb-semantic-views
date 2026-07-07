use std::ffi::{CStr, CString};

use libduckdb_sys as ffi;

use crate::catalog::CatalogReader;
use crate::expand::wildcard::{expand_wildcards, WildcardItemType};
use crate::expand::{expand, QueryRequest};
use crate::model::SemanticViewDefinition;
use crate::util::suggest_closest;

use super::error::QueryError;
use super::wire::{build_execution_sql, parse_varchar_list, serialize_register_payload};

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 6 (Wave 6) — sv_semantic_view_bind_rust
// ---------------------------------------------------------------------------
//
// FFI dispatcher for the migrated
// `semantic_view(view_name, dimensions := [...], metrics := [...], facts := [...])`
// table function. The C++ bind callback (`sv_semantic_view_bind` in
// `cpp/src/shim.cpp`) opens a per-call `Connection probe(*context.db)`,
// flattens the three optional LIST(VARCHAR) named parameters into the
// length-prefixed wire format (same encoding as the Wave 5 explain
// migration), and invokes this dispatcher. Same `reinterpret_cast` bridge
// + BORROW contract as the 15 prior migrations.
//
// Responsibilities of the Rust side:
//   - Catalog lookup (name normalisation, view-not-found suggestion).
//   - Wildcard expansion + `QueryRequest` construction.
//   - `expand::expand()` → expanded SQL.
//   - Column-type inference at read-side bind time per D-16/D-17:
//     * For fact queries: always probe via LIMIT 0 on the per-call conn.
//     * For dim+metric queries: prefer DDL-time persisted types if present
//       (back-compat for v0.7.1-era catalog rows); fall back to LIMIT 0
//       probe on per-call conn otherwise.
//   - `build_execution_sql()` wrapping for HUGEINT→BIGINT casts etc.
//
// The dispatcher returns a flat binary buffer to the C++ side encoding
// the schema + execution_sql for bind to declare output columns and the
// init_global callback to run the query. Wire format:
//
//   u32 n_cols (little-endian)
//   for each col:
//     u32 byte_len + bytes (column name, UTF-8)
//     u32 type_id (little-endian; already passed through normalize_type_id)
//   u32 byte_len + bytes (execution_sql, UTF-8)
//
// Return codes mirror the Wave 5 dispatcher:
//   0 — success; (out_ptr, out_len) populated.
//   1 — user-visible error; error_buf populated (raised as BinderException).
//   2 — internal error (panic across FFI); error_buf populated.

/// # Safety
///
/// `conn` is a borrowed handle (do NOT disconnect). Wire-format payloads
/// described above; `name_ptr` must point to `name_len` UTF-8 bytes.
#[cfg(feature = "extension")]
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sv_semantic_view_bind_rust(
    conn: ffi::duckdb_connection,
    name_ptr: *const u8,
    name_len: usize,
    dims_ptr: *const u8,
    dims_len: usize,
    metrics_ptr: *const u8,
    metrics_len: usize,
    facts_ptr: *const u8,
    facts_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use crate::ddl::read_ffi::{
        probe_catalog_table_present, publish_owned_buffer, write_err, BorrowedConnection,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Wrap-on-entry (Phase 65.1 D-10 / WR-05). The raw `conn` parameter
        // is shadowed; everything downstream goes through `&borrowed` or
        // `borrowed.as_raw()`. `ffi::duckdb_disconnect` does not type-check
        // against `&mut BorrowedConnection`, enforcing the BORROW contract.
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        if name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "view name pointer is null");
            return 1_u8;
        }
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let view_name_raw = match std::str::from_utf8(name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
                return 1_u8;
            }
        };
        let view_name = match crate::ident::normalize_view_name(&view_name_raw) {
            Ok(s) => s,
            Err(e) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("Invalid view name '{view_name_raw}': {e}"),
                );
                return 1_u8;
            }
        };

        let dimensions = match parse_varchar_list(dims_ptr, dims_len) {
            Ok(v) => v,
            Err(detail) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("malformed `dimensions` payload: {detail}"),
                );
                return 1_u8;
            }
        };
        let metrics = match parse_varchar_list(metrics_ptr, metrics_len) {
            Ok(v) => v,
            Err(detail) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("malformed `metrics` payload: {detail}"),
                );
                return 1_u8;
            }
        };
        let facts = match parse_varchar_list(facts_ptr, facts_len) {
            Ok(v) => v,
            Err(detail) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("malformed `facts` payload: {detail}"),
                );
                return 1_u8;
            }
        };

        if dimensions.is_empty() && metrics.is_empty() && facts.is_empty() {
            write_err(
                error_buf,
                error_buf_len,
                &QueryError::EmptyRequest {
                    view_name: view_name.clone(),
                }
                .to_string(),
            );
            return 1_u8;
        }

        // FF-9: surface a probe-query failure as an error distinct from "no
        // views" instead of silently folding it into absence.
        let present = match probe_catalog_table_present(&borrowed) {
            Ok(p) => p,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let reader = CatalogReader::new(&borrowed, present);
        let json_str = match reader.lookup(&view_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                let available = reader.list_names().unwrap_or_default();
                let suggestion = suggest_closest(&view_name, &available);
                write_err(
                    error_buf,
                    error_buf_len,
                    &QueryError::ViewNotFound {
                        name: view_name,
                        suggestion,
                        available,
                    }
                    .to_string(),
                );
                return 1_u8;
            }
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };

        let def = match SemanticViewDefinition::from_json(&view_name, &json_str) {
            Ok(d) => d,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };

        let dimensions = match expand_wildcards(&dimensions, &def, &WildcardItemType::Dimension) {
            Ok(v) => v,
            Err(e) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &QueryError::ExpandFailed {
                        source: crate::expand::ExpandError::EmptyRequest {
                            view_name: format!("{view_name}: {e}"),
                        },
                    }
                    .to_string(),
                );
                return 1_u8;
            }
        };
        let metrics = match expand_wildcards(&metrics, &def, &WildcardItemType::Metric) {
            Ok(v) => v,
            Err(e) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &QueryError::ExpandFailed {
                        source: crate::expand::ExpandError::EmptyRequest {
                            view_name: format!("{view_name}: {e}"),
                        },
                    }
                    .to_string(),
                );
                return 1_u8;
            }
        };
        let facts = match expand_wildcards(&facts, &def, &WildcardItemType::Fact) {
            Ok(v) => v,
            Err(e) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &QueryError::ExpandFailed {
                        source: crate::expand::ExpandError::EmptyRequest {
                            view_name: format!("{view_name}: {e}"),
                        },
                    }
                    .to_string(),
                );
                return 1_u8;
            }
        };

        let req = QueryRequest {
            dimensions: dimensions
                .iter()
                .map(|s| crate::expand::DimensionName::new(s.clone()))
                .collect(),
            metrics: metrics
                .iter()
                .map(|s| crate::expand::MetricName::new(s.clone()))
                .collect(),
            facts: facts.clone(),
        };
        let expanded_sql = match expand(&view_name, &def, &req) {
            Ok(s) => s,
            Err(e) => {
                write_err(error_buf, error_buf_len, &QueryError::from(e).to_string());
                return 1_u8;
            }
        };

        // Type inference: a LIMIT-0 probe on the per-call connection yields
        // the output column names + types. The probe runs on `conn`, not a
        // long-lived handle (H2). AR-4 (PR-2) removed the DDL-time
        // persisted-types fast path (`column_type_names` / `column_types_inferred`
        // were dead for post-v0.10 rows) — every row now infers at read time,
        // matching Plan 03 D-16, so this is a single unconditional probe.
        let (column_names, column_type_ids): (Vec<String>, Vec<u32>) = {
            let limit0_sql = format!("{expanded_sql} LIMIT 0");
            // Phase 65.1 Plan 11 / WR-08 / D-15: surface probe failures via
            // the error_buf cascade. No silent vec![0u32; names.len()]
            // fallback to DUCKDB_TYPE_INVALID — that masked broken FACTS
            // expressions behind a VARCHAR placeholder at query time.
            match try_infer_schema(&borrowed, &limit0_sql) {
                Ok((names, types)) => {
                    let type_ids: Vec<u32> =
                        types.iter().map(|t| normalize_type_id(*t as u32)).collect();
                    (names, type_ids)
                }
                Err(msg) => {
                    write_err(
                        error_buf,
                        error_buf_len,
                        &format!(
                            "semantic_view: type inference failed for query \
                             `{limit0_sql}`: {msg}"
                        ),
                    );
                    return 1_u8;
                }
            }
        };

        // Build execution SQL with casts where needed (HUGEINT→BIGINT etc).
        let execution_sql = build_execution_sql(&expanded_sql, &column_names, &column_type_ids);

        // Serialise schema + execution_sql into a flat binary buffer.
        let buf = match serialize_register_payload(&column_names, &column_type_ids, &execution_sql)
        {
            Ok(b) => b,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        publish_owned_buffer(buf, out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            use crate::ddl::read_ffi::write_err;
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_semantic_view_bind_rust",
            );
            2
        }
    }
}

// ---------------------------------------------------------------------------
// FFI helpers
// ---------------------------------------------------------------------------

/// Execute a SQL string via the DuckDB C API and return the result.
///
/// The caller is responsible for calling `duckdb_destroy_result` on the returned
/// result when done.
///
/// # Safety
///
/// `conn` must be a valid, non-null `duckdb_connection` handle.
pub(crate) unsafe fn execute_sql_raw(
    conn: ffi::duckdb_connection,
    sql: &str,
) -> Result<ffi::duckdb_result, String> {
    let sql_cstr = CString::new(sql).map_err(|e| e.to_string())?;
    let mut result: ffi::duckdb_result = std::mem::zeroed();
    let rc = ffi::duckdb_query(conn, sql_cstr.as_ptr(), &mut result);
    if rc != ffi::DuckDBSuccess {
        let err_ptr = ffi::duckdb_result_error(&mut result);
        let err_msg = if err_ptr.is_null() {
            "unknown error".to_string()
        } else {
            CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
        };
        ffi::duckdb_destroy_result(&mut result);
        return Err(err_msg);
    }
    Ok(result)
}

/// Normalize HUGEINT/UHUGEINT type IDs to BIGINT/UBIGINT.
///
/// DuckDB's query planner infers `sum()` as HUGEINT at LIMIT-0 time, but the
/// runtime optimizer may substitute `sum_no_overflow` → BIGINT (8 bytes/value).
/// Normalizing stored type IDs prevents stale HUGEINT values from propagating
/// into output declarations.
pub(crate) fn normalize_type_id(t: u32) -> u32 {
    const HUGEINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_HUGEINT;
    const UHUGEINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_UHUGEINT;
    const BIGINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_BIGINT;
    const UBIGINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_UBIGINT;
    match t {
        HUGEINT => BIGINT,
        UHUGEINT => UBIGINT,
        _ => t,
    }
}

// ---------------------------------------------------------------------------
// Schema inference
// ---------------------------------------------------------------------------

/// Attempt to infer column names and types by executing a LIMIT 0 query.
///
/// Returns `Err(msg)` with the underlying `execute_sql_raw` error text when
/// the LIMIT 0 probe fails. Phase 65.1 Plan 11 / WR-08 / D-15: callers
/// surface the error via `write_err` + return rc=1, which the C++ binder
/// re-raises as `BinderException`. The previous `Option` return type
/// swallowed the diagnostic via `.ok()?`, masking broken DDL behind a
/// silent VARCHAR placeholder for `DUCKDB_TYPE_INVALID`.
///
/// # Safety
///
/// The underlying `duckdb_connection` accessed via `borrowed.as_raw()` must
/// be valid for the lifetime of the borrow.
pub(crate) unsafe fn try_infer_schema(
    borrowed: &crate::ddl::read_ffi::BorrowedConnection,
    sql: &str,
) -> Result<(Vec<String>, Vec<ffi::duckdb_type>), String> {
    let mut result = execute_sql_raw(borrowed.as_raw(), sql)?;

    let col_count = ffi::duckdb_column_count(&mut result) as usize;
    let mut names = Vec::with_capacity(col_count);
    let mut types = Vec::with_capacity(col_count);

    for i in 0..col_count {
        let name_ptr = ffi::duckdb_column_name(&mut result, i as ffi::idx_t);
        let name = if name_ptr.is_null() {
            format!("column{i}")
        } else {
            CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
        };
        names.push(name);

        let ty = ffi::duckdb_column_type(&mut result, i as ffi::idx_t);
        types.push(ty);
    }

    ffi::duckdb_destroy_result(&mut result);
    Ok((names, types))
}

// ---------------------------------------------------------------------------
// VARCHAR reading (used by explain.rs)
// ---------------------------------------------------------------------------

/// Read a VARCHAR value from a data chunk vector at the given column and row.
///
/// The vector must contain VARCHAR (`duckdb_string_t`) data. Returns an empty
/// string for NULL values.
///
/// Decodes the `duckdb_string_t` layout directly from vector memory to avoid
/// reliance on C API helper functions that may not be available in loadable
/// extension mode.
///
/// # Safety
///
/// `chunk` must be a valid, non-null `duckdb_data_chunk` handle.
/// `col_idx` and `row_idx` must be within bounds.
#[allow(clippy::cast_possible_truncation)]
pub(crate) unsafe fn read_varchar_from_vector(
    chunk: ffi::duckdb_data_chunk,
    col_idx: usize,
    row_idx: usize,
) -> String {
    let vector = ffi::duckdb_data_chunk_get_vector(chunk, col_idx as ffi::idx_t);

    // Check for NULL using the validity mask.
    let validity = ffi::duckdb_vector_get_validity(vector);
    if !validity.is_null() {
        let entry_idx = row_idx / 64;
        let bit_idx = row_idx % 64;
        let entry = *validity.add(entry_idx);
        if entry & (1u64 << bit_idx) == 0 {
            return String::new(); // NULL
        }
    }

    // Read the duckdb_string_t from the vector data.
    // Layout is a 16-byte union:
    //   Inline  (len <= 12): { length: u32, inlined: [c_char; 12] }
    //   Pointer (len > 12):  { length: u32, prefix: [c_char; 4], ptr: *mut c_char }
    let data_ptr = ffi::duckdb_vector_get_data(vector);
    let string_t_ptr = data_ptr.cast::<ffi::duckdb_string_t>().add(row_idx);
    let string_t = &*string_t_ptr;

    // The length field is at the same offset for both union variants.
    let len = string_t.value.inlined.length as usize;
    if len == 0 {
        return String::new();
    }

    let bytes = if len <= 12 {
        // Inline: string data follows the length field directly.
        let inline_ptr = string_t.value.inlined.inlined.as_ptr().cast::<u8>();
        std::slice::from_raw_parts(inline_ptr, len)
    } else {
        // Pointer: data is at the heap-allocated ptr.
        let heap_ptr = string_t.value.pointer.ptr.cast::<u8>();
        if heap_ptr.is_null() {
            return String::new();
        }
        std::slice::from_raw_parts(heap_ptr, len)
    };

    String::from_utf8_lossy(bytes).into_owned()
}
