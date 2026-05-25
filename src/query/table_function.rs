use std::collections::HashMap;
use std::ffi::{CStr, CString};

use duckdb::vtab::Value;
use libduckdb_sys as ffi;

use crate::catalog::CatalogReader;
use crate::expand::wildcard::{expand_wildcards, WildcardItemType};
use crate::expand::{expand, QueryRequest};
use crate::model::SemanticViewDefinition;
use crate::util::suggest_closest;

use super::error::QueryError;

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

unsafe fn sv_parse_string_list(buf: *const u8, len: usize) -> Option<Vec<String>> {
    if buf.is_null() {
        return if len == 0 { Some(Vec::new()) } else { None };
    }
    if len < 4 {
        return None;
    }
    let slice = std::slice::from_raw_parts(buf, len);
    let mut off = 0usize;
    let read_u32 = |slice: &[u8], off: &mut usize| -> Option<u32> {
        if *off + 4 > slice.len() {
            return None;
        }
        let v = u32::from_le_bytes(slice[*off..*off + 4].try_into().ok()?);
        *off += 4;
        Some(v)
    };
    let count = read_u32(slice, &mut off)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let n = read_u32(slice, &mut off)? as usize;
        if off + n > slice.len() {
            return None;
        }
        out.push(String::from_utf8_lossy(&slice[off..off + n]).into_owned());
        off += n;
    }
    if off != len {
        return None;
    }
    Some(out)
}

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

        let dimensions = match sv_parse_string_list(dims_ptr, dims_len) {
            Some(v) => v,
            None => {
                write_err(error_buf, error_buf_len, "malformed `dimensions` payload");
                return 1_u8;
            }
        };
        let metrics = match sv_parse_string_list(metrics_ptr, metrics_len) {
            Some(v) => v,
            None => {
                write_err(error_buf, error_buf_len, "malformed `metrics` payload");
                return 1_u8;
            }
        };
        let facts = match sv_parse_string_list(facts_ptr, facts_len) {
            Some(v) => v,
            None => {
                write_err(error_buf, error_buf_len, "malformed `facts` payload");
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

        let reader = CatalogReader::new(&borrowed, probe_catalog_table_present(&borrowed));
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

        // Type inference — same fall-back ladder as the legacy bind, but
        // every LIMIT-0 probe now runs on the per-call connection (`conn`)
        // instead of the long-lived `state.conn` (H2). The DDL-time
        // persisted-types branch stays as a back-compat fast path for
        // v0.7.1-era catalog rows where `column_type_names` may still be
        // populated; new definitions land with empty vecs (Plan 03 D-16).
        let type_map: HashMap<String, u32> = def
            .inferred_types()
            .map(|(name, t)| (name.to_ascii_lowercase(), normalize_type_id(t)))
            .collect();

        let (column_names, column_type_ids): (Vec<String>, Vec<u32>) = if !facts.is_empty() {
            let limit0_sql = format!("{expanded_sql} LIMIT 0");
            if let Some((names, types)) = try_infer_schema(&borrowed, &limit0_sql) {
                let type_ids: Vec<u32> =
                    types.iter().map(|t| normalize_type_id(*t as u32)).collect();
                (names, type_ids)
            } else {
                let mut names = Vec::new();
                for dim_name in &dimensions {
                    let canonical = def
                        .dimensions
                        .iter()
                        .find(|d| d.name.eq_ignore_ascii_case(dim_name))
                        .map_or_else(|| dim_name.clone(), |d| d.name.clone());
                    names.push(canonical);
                }
                for fact_name in &facts {
                    let canonical = def
                        .facts
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case(fact_name))
                        .map_or_else(|| fact_name.clone(), |f| f.name.clone());
                    names.push(canonical);
                }
                let type_ids = vec![0u32; names.len()];
                (names, type_ids)
            }
        } else if !type_map.is_empty() {
            let mut names = Vec::new();
            let mut type_ids = Vec::new();
            for dim_name in &dimensions {
                let canonical = def
                    .dimensions
                    .iter()
                    .find(|d| d.name.eq_ignore_ascii_case(dim_name))
                    .map_or_else(|| dim_name.clone(), |d| d.name.clone());
                let t = *type_map
                    .get(&canonical.to_ascii_lowercase())
                    .unwrap_or(&0u32);
                names.push(canonical);
                type_ids.push(t);
            }
            for met_name in &metrics {
                let canonical = def
                    .metrics
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(met_name))
                    .map_or_else(|| met_name.clone(), |m| m.name.clone());
                let t = *type_map
                    .get(&canonical.to_ascii_lowercase())
                    .unwrap_or(&0u32);
                names.push(canonical);
                type_ids.push(t);
            }
            (names, type_ids)
        } else {
            let limit0_sql = format!("{expanded_sql} LIMIT 0");
            if let Some((names, types)) = try_infer_schema(&borrowed, &limit0_sql) {
                let type_ids: Vec<u32> =
                    types.iter().map(|t| normalize_type_id(*t as u32)).collect();
                (names, type_ids)
            } else {
                let mut names = Vec::new();
                for dim_name in &dimensions {
                    let canonical = def
                        .dimensions
                        .iter()
                        .find(|d| d.name.eq_ignore_ascii_case(dim_name))
                        .map_or_else(|| dim_name.clone(), |d| d.name.clone());
                    names.push(canonical);
                }
                for met_name in &metrics {
                    let canonical = def
                        .metrics
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(met_name))
                        .map_or_else(|| met_name.clone(), |m| m.name.clone());
                    names.push(canonical);
                }
                let type_ids = vec![0u32; names.len()];
                (names, type_ids)
            }
        };

        // Build execution SQL with casts where needed (HUGEINT→BIGINT etc).
        let execution_sql = build_execution_sql(&expanded_sql, &column_names, &column_type_ids);

        // Serialise schema + execution_sql into a flat binary buffer.
        let n_cols = column_names.len() as u32;
        let cap = 4 // n_cols
            + column_names.iter().map(|n| 4 + n.len()).sum::<usize>()
            + column_type_ids.len() * 4
            + 4 + execution_sql.len();
        let mut buf: Vec<u8> = Vec::with_capacity(cap);
        buf.extend_from_slice(&n_cols.to_le_bytes());
        for (name, tid) in column_names.iter().zip(column_type_ids.iter()) {
            let nl = name.len() as u32;
            buf.extend_from_slice(&nl.to_le_bytes());
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(&tid.to_le_bytes());
        }
        let sql_len = execution_sql.len() as u32;
        buf.extend_from_slice(&sql_len.to_le_bytes());
        buf.extend_from_slice(execution_sql.as_bytes());

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
// Legacy `QueryState` + `SemanticViewBindData` + `StreamingState` +
// `SemanticViewInitData` RETIRED — Phase 65 Plan 05 Batch 3. The C++
// Catalog API path (`sv_register_semantic_view`) carries its own
// BindData / GlobalState struct on the C++ side; the Rust dispatcher
// `sv_semantic_view_bind_rust` only serialises the wire format and
// returns. The `MaterializedQueryResult` lives in `SemanticViewGlobalState`
// on the C++ side (see `cpp/src/shim.cpp`) and outlives the per-call
// Connection that produced it.
// ---------------------------------------------------------------------------

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

// `value_raw_ptr`, `extract_list_strings`, `LogicalTypeOwned`,
// `type_from_duckdb_type_u32`, `declare_output_type` RETIRED — Phase 65
// Plan 05 Batch 3. All five helpers belonged to the legacy
// `SemanticViewVTab` Rust path. The C++ Catalog API path
// (`sv_register_semantic_view`) flattens LIST(VARCHAR) named params
// inside `cpp/src/shim.cpp::sv_serialise_string_list` and declares
// output logical types via the C++ helper
// `sv_logical_type_from_c_type_id` (see BATCH2-SUMMARY for the C-API ↔
// C++ enum-value mismatch story). Their removal also retires the
// duckdb-rs `Value`-pointer transmute that depended on
// `repr(Rust)` layout assumptions of `duckdb::vtab::Value`.

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
// Execution SQL generation
// ---------------------------------------------------------------------------

/// Map a DuckDB type ID to its SQL cast name.
///
/// Returns `Some("TYPE")` for types that can be cast via SQL text, `None` for
/// types whose precision/metadata cannot be expressed in a bare cast (DECIMAL,
/// LIST) -- those are handled via logical type metadata at bind time.
fn type_id_to_cast_sql(type_id: u32) -> Option<&'static str> {
    const BOOLEAN: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_BOOLEAN;
    const TINYINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TINYINT;
    const SMALLINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_SMALLINT;
    const INTEGER: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_INTEGER;
    const BIGINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_BIGINT;
    const UTINYINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_UTINYINT;
    const USMALLINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_USMALLINT;
    const UINTEGER: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_UINTEGER;
    const UBIGINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_UBIGINT;
    const FLOAT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_FLOAT;
    const DOUBLE: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_DOUBLE;
    const TIMESTAMP: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP;
    const DATE: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_DATE;
    const TIME: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIME;
    const HUGEINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_HUGEINT;
    const UHUGEINT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_UHUGEINT;
    const VARCHAR: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR;
    const TIMESTAMP_S: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_S;
    const TIMESTAMP_MS: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_MS;
    const TIMESTAMP_NS: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_NS;
    const TIMESTAMP_TZ: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_TZ;

    // Complex types that are declared as VARCHAR at bind time.
    const STRUCT: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT;
    const MAP: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_MAP;
    const INVALID: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_INVALID;

    // Note: DECIMAL is intentionally NOT cast via SQL text here -- DECIMAL
    // precision/scale is declared from the logical type handle at bind time,
    // and a bare "DECIMAL" cast would lose width/scale. Instead, DECIMAL
    // columns pass through without a text cast; the bind-time declaration
    // already ensures the correct output type.
    const DECIMAL: u32 = ffi::DUCKDB_TYPE_DUCKDB_TYPE_DECIMAL;

    match type_id {
        BOOLEAN => Some("BOOLEAN"),
        TINYINT => Some("TINYINT"),
        SMALLINT => Some("SMALLINT"),
        INTEGER => Some("INTEGER"),
        BIGINT | HUGEINT => Some("BIGINT"),
        UTINYINT => Some("UTINYINT"),
        USMALLINT => Some("USMALLINT"),
        UINTEGER => Some("UINTEGER"),
        UBIGINT | UHUGEINT => Some("UBIGINT"),
        FLOAT => Some("FLOAT"),
        DOUBLE => Some("DOUBLE"),
        DATE => Some("DATE"),
        TIME => Some("TIME"),
        TIMESTAMP => Some("TIMESTAMP"),
        TIMESTAMP_S => Some("TIMESTAMP_S"),
        TIMESTAMP_MS => Some("TIMESTAMP_MS"),
        TIMESTAMP_NS => Some("TIMESTAMP_NS"),
        TIMESTAMP_TZ => Some("TIMESTAMPTZ"),
        VARCHAR => Some("VARCHAR"),
        STRUCT | MAP | INVALID => Some("VARCHAR"),
        // DECIMAL and LIST columns cannot be cast via bare SQL type name --
        // DECIMAL requires precision/scale (bare "DECIMAL" defaults to (18,3)
        // which changes the value), LIST requires child type. These types are
        // handled via logical type metadata at bind time, so pass through
        // unmodified in the execution SQL wrapper.
        DECIMAL => None,
        // Unknown types: pass through rather than risk a lossy VARCHAR cast.
        // The runtime type check in func() will catch any real mismatch.
        _ => None,
    }
}

/// Build the SQL used at execution time, wrapping the expanded SQL with explicit
/// type casts for EVERY output column.
///
/// This ensures that runtime column types always match the bind-time schema
/// declaration, preventing type mismatches in `duckdb_vector_reference_vector`.
/// DuckDB optimizes away no-op casts (e.g., `col::BIGINT` when `col` is already
/// BIGINT), so the wrapper has negligible performance overhead.
///
/// Key type mappings:
/// - HUGEINT/UHUGEINT → BIGINT/UBIGINT (optimizer substitution)
/// - STRUCT/MAP/INVALID → VARCHAR (complex types declared as VARCHAR at bind)
/// - All scalar types → explicit cast matching bind declaration
fn build_execution_sql(
    expanded_sql: &str,
    column_names: &[String],
    column_type_ids: &[u32],
) -> String {
    // If there are no columns, return the original SQL (edge case).
    if column_names.is_empty() {
        return expanded_sql.to_string();
    }

    let clauses: Vec<String> = column_names
        .iter()
        .zip(column_type_ids.iter())
        .map(|(name, &tid)| match type_id_to_cast_sql(tid) {
            Some(cast_type) => format!("\"{name}\"::{cast_type} AS \"{name}\""),
            None => format!("\"{name}\""),
        })
        .collect();

    format!(
        "SELECT {} FROM ({expanded_sql}) __sv_inner",
        clauses.join(", ")
    )
}

// ---------------------------------------------------------------------------
// Legacy `SemanticViewVTab` (duckdb-rs `VTab` impl) RETIRED — Phase 65 Plan 05
// Batch 3. The C++ Catalog API path (`sv_register_semantic_view` →
// `sv_semantic_view_bind_rust` above) is the sole registration target.
// The MaterializedQueryResult lives in `SemanticViewGlobalState` on the C++
// side and outlives the per-call Connection that produced it (see
// `cpp/src/shim.cpp` and 65-05-BATCH2-SUMMARY.md for the streaming model).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Schema inference
// ---------------------------------------------------------------------------

/// Attempt to infer column names and types by executing a LIMIT 0 query.
///
/// Returns `None` if the query fails for any reason.
///
/// # Safety
///
/// The underlying `duckdb_connection` accessed via `borrowed.as_raw()` must
/// be valid for the lifetime of the borrow.
pub(crate) unsafe fn try_infer_schema(
    borrowed: &crate::ddl::read_ffi::BorrowedConnection,
    sql: &str,
) -> Option<(Vec<String>, Vec<ffi::duckdb_type>)> {
    let mut result = execute_sql_raw(borrowed.as_raw(), sql).ok()?;

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
    Some((names, types))
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

// Compile-time layout guard: fails `just build` (extension feature) if
// duckdb::vtab::Value size diverges from ffi::duckdb_value. This catches
// transmute breakage in value_raw_ptr at compile time rather than runtime.
// A runtime test is not possible because `cargo test` uses the `bundled`
// feature (no vtab module) and `cargo test --features extension` conflicts
// with loadable-extension stubs.
const _: () = {
    assert!(
        std::mem::size_of::<Value>() == std::mem::size_of::<ffi::duckdb_value>(),
        "Value size changed -- value_raw_ptr transmute is broken"
    );
    assert!(
        std::mem::align_of::<Value>() == std::mem::align_of::<ffi::duckdb_value>(),
        "Value alignment changed -- value_raw_ptr transmute is broken"
    );
};
