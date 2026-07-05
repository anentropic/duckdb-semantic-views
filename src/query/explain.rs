use libduckdb_sys as ffi;

use crate::catalog::CatalogReader;
use crate::expand::find_routing_materialization_name;
use crate::expand::{expand, QueryRequest};
use crate::model::SemanticViewDefinition;
use crate::util::suggest_closest;

use super::error::QueryError;
use crate::expand::wildcard::{expand_wildcards, WildcardItemType};

use super::table_function::{execute_sql_raw, read_varchar_from_vector};

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 5 (Wave 5) — sv_explain_semantic_view_bind_rust
// ---------------------------------------------------------------------------
//
// FFI dispatcher for the migrated `explain_semantic_view(view_name,
// dimensions := [...], metrics := [...], facts := [...])` table function.
//
// The C++ bind callback (`sv_explain_semantic_view_bind` in
// `cpp/src/shim.cpp`) opens a per-call `Connection probe(*context.db)`,
// pulls the positional view-name from `input.inputs[0]` and the optional
// LIST(VARCHAR) named parameters from `input.named_parameters`, serialises
// the three string lists into the standard length-prefixed wire format,
// and invokes this dispatcher. Same `reinterpret_cast` bridge mechanism +
// BORROW contract as the 14 migrations in Batch 1 of Plan 05.
//
// Wire format for the three list arguments (`dims_buf`, `metrics_buf`,
// `facts_buf`), each independently encoded as:
//
//   u32 count (little-endian)
//   for each entry:
//     u32 byte_len (little-endian)
//     byte_len bytes (UTF-8, NOT NUL-terminated)
//
// Output wire format (matches Wave 0/1 VARCHAR-rows encoding):
//
//   u32 row_count (little-endian)
//   for each row:
//     u32 byte_len (little-endian)
//     byte_len bytes (UTF-8) — one explain-output line per row, single VARCHAR column
//
// Return codes:
//   0 — success; `(out_ptr, out_len)` populated.
//   1 — user-visible error (catalog miss, validation, expand failure);
//       `error_buf` populated. The C++ side raises `BinderException`.
//   2 — internal error (panic across FFI, allocation failure); `error_buf`
//       populated.

/// Parse a length-prefixed list-of-VARCHAR wire-format buffer back into a
/// `Vec<String>`. Returns an `Err(diagnostic)` on truncation / overflow /
/// trailing bytes; the C++ side surfaces this as rc=1 via the dispatcher.
///
/// Phase 65.1 WR-05: returns `Result<_, String>` so the dispatcher can
/// surface "expected u32 at offset N of M" or "trailing N bytes after
/// count C" details, matching the diagnostic shape of the C++
/// `sv_parse_varchar_payload`. The previous `Option<Vec<String>>` shape
/// only let the dispatcher report a flat "malformed X payload" error
/// with no detail — unactionable for an FFI-shape regression.
unsafe fn parse_string_list(buf: *const u8, len: usize) -> Result<Vec<String>, String> {
    // Handle null buffer explicitly before len-check so a pathological
    // (null, len > 0) FFI call cannot fall through to from_raw_parts(null, len),
    // which is UB. Mirrors src/query/table_function.rs::sv_parse_string_list.
    if buf.is_null() {
        return if len == 0 {
            Ok(Vec::new())
        } else {
            Err(format!("null buffer but len={len} (FFI shape drift)"))
        };
    }
    if len < 4 {
        return Err(format!(
            "buffer too short for count prefix: len={len} (expected >= 4)"
        ));
    }
    let slice = std::slice::from_raw_parts(buf, len);
    let mut off = 0usize;
    let read_u32 = |slice: &[u8], off: &mut usize| -> Result<u32, String> {
        if *off + 4 > slice.len() {
            return Err(format!(
                "expected u32 at offset {} of {} (truncated)",
                *off,
                slice.len()
            ));
        }
        let v = u32::from_le_bytes(slice[*off..*off + 4].try_into().map_err(
            |e: std::array::TryFromSliceError| format!("u32 decode failed at offset {}: {e}", *off),
        )?);
        *off += 4;
        Ok(v)
    };
    let count = read_u32(slice, &mut off)? as usize;
    // FF-6: cap the pre-allocation at the largest element count the buffer
    // could actually hold. The 4-byte count prefix has already been consumed,
    // and each remaining element carries at least a 4-byte length prefix, so
    // the ceiling is `(len - 4) / 4`. A corrupt `count` near u32::MAX would
    // otherwise request a ~100 GB allocation up front; the per-element bounds
    // check below still rejects a genuinely truncated payload.
    let mut out = Vec::with_capacity(count.min(len.saturating_sub(4) / 4));
    for i in 0..count {
        let n = read_u32(slice, &mut off)
            .map_err(|e| format!("reading length for element {i} of {count}: {e}"))?
            as usize;
        if off + n > slice.len() {
            return Err(format!(
                "element {i} of {count} declares length {n} but only {} bytes remain at offset {off}",
                slice.len().saturating_sub(off)
            ));
        }
        out.push(String::from_utf8_lossy(&slice[off..off + n]).into_owned());
        off += n;
    }
    if off != len {
        return Err(format!(
            "trailing {} bytes after count {count} (consumed {off} of {len})",
            len - off
        ));
    }
    Ok(out)
}

/// # Safety
///
/// `conn` is a borrowed handle (do NOT disconnect). The three `*_buf` /
/// `*_len` pairs encode LIST(VARCHAR) arguments using the wire format
/// documented above. `name_ptr` must point to `name_len` UTF-8 bytes.
#[cfg(feature = "extension")]
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sv_explain_semantic_view_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
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
        probe_catalog_table_present, publish_owned_buffer, serialize_varchar_rows, write_err,
        BorrowedConnection,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Wrap-on-entry (Phase 65.1 D-10 / WR-05). The raw `conn` parameter
        // is shadowed below; everything downstream goes through `&borrowed`
        // (for helpers that accept `&BorrowedConnection`) or
        // `borrowed.as_raw()` (for raw FFI calls like `duckdb_query`).
        // `ffi::duckdb_disconnect` does not type-check against
        // `&mut BorrowedConnection`, enforcing the BORROW contract.
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

        let dimensions = match parse_string_list(dims_ptr, dims_len) {
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
        let metrics = match parse_string_list(metrics_ptr, metrics_len) {
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
        let facts = match parse_string_list(facts_ptr, facts_len) {
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
            // Match the QueryError::EmptyRequest message rendered by the
            // legacy VTab so phase57_introspection assertions stay
            // byte-identical.
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
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };
        let metrics = match expand_wildcards(&metrics, &def, &WildcardItemType::Metric) {
            Ok(v) => v,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };
        let facts = match expand_wildcards(&facts, &def, &WildcardItemType::Fact) {
            Ok(v) => v,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };

        let mat_name = {
            let dim_refs: Vec<&crate::model::Dimension> = dimensions
                .iter()
                .filter_map(|name| {
                    def.dimensions
                        .iter()
                        .find(|d| d.name.eq_ignore_ascii_case(name))
                })
                .collect();
            let met_refs: Vec<&crate::model::Metric> = metrics
                .iter()
                .filter_map(|name| {
                    def.metrics
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(name))
                })
                .collect();
            find_routing_materialization_name(&def, &dim_refs, &met_refs).map(String::from)
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

        // Build the three-part output, identical to the legacy VTab so
        // phase28_e2e / phase46_* / phase57_introspection / phase64
        // assertions stay byte-identical.
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("-- Semantic View: {view_name}"));
        lines.push(format!("-- Dimensions: {}", dimensions.join(", ")));
        lines.push(format!("-- Metrics: {}", metrics.join(", ")));
        if !facts.is_empty() {
            lines.push(format!("-- Facts: {}", facts.join(", ")));
        }
        match mat_name {
            Some(ref n) => lines.push(format!("-- Materialization: {n}")),
            None => lines.push("-- Materialization: none".to_string()),
        }
        lines.push(String::new());
        lines.push("-- Expanded SQL:".to_string());
        for sql_line in expanded_sql.lines() {
            lines.push(sql_line.to_string());
        }
        lines.push(String::new());
        lines.push("-- DuckDB Plan:".to_string());
        let explain_lines = collect_explain_lines(&borrowed, &expanded_sql);
        lines.extend(explain_lines);

        // Serialise as 1-column VARCHAR rows.
        let rows: Vec<Vec<String>> = lines.into_iter().map(|l| vec![l]).collect();
        let buf = match serialize_varchar_rows(&rows) {
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
                "internal error: panic inside sv_explain_semantic_view_bind_rust",
            );
            2
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy `ExplainBindData` + `ExplainInitData` RETIRED — Phase 65 Plan 05
// Batch 3. The C++ Catalog API path's bind callback materialises lines
// into a length-prefixed wire format directly; no shared Rust bind-data
// struct is needed.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// EXPLAIN plan extraction
// ---------------------------------------------------------------------------

/// Execute `EXPLAIN {sql}` and return the plan as lines of text.
///
/// If the EXPLAIN fails (e.g., referenced tables do not exist), returns
/// a single fallback line with the error message.
///
/// # Safety
///
/// The underlying `duckdb_connection` accessed via `borrowed.as_raw()` must
/// be valid for the lifetime of the borrow.
#[allow(clippy::cast_possible_truncation)]
unsafe fn collect_explain_lines(
    borrowed: &crate::ddl::read_ffi::BorrowedConnection,
    sql: &str,
) -> Vec<String> {
    let explain_sql = format!("EXPLAIN {sql}");
    let mut lines = Vec::new();

    match execute_sql_raw(borrowed.as_raw(), &explain_sql) {
        Ok(mut result) => {
            let col_count = ffi::duckdb_column_count(&raw mut result) as usize;
            let chunk_count = ffi::duckdb_result_chunk_count(result) as usize;

            for chunk_idx in 0..chunk_count {
                let chunk = ffi::duckdb_result_get_chunk(result, chunk_idx as ffi::idx_t);
                if chunk.is_null() {
                    continue;
                }
                let row_count = ffi::duckdb_data_chunk_get_size(chunk) as usize;

                for row_idx in 0..row_count {
                    for col_idx in 0..col_count {
                        let s = read_varchar_from_vector(chunk, col_idx, row_idx);
                        if !s.is_empty() {
                            for plan_line in s.lines() {
                                lines.push(plan_line.to_string());
                            }
                        }
                    }
                }

                ffi::duckdb_destroy_data_chunk(&mut { chunk });
            }

            ffi::duckdb_destroy_result(&raw mut result);
        }
        Err(err) => {
            lines.push(format!("-- (not available -- {err})"));
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// Legacy `ExplainSemanticViewVTab` (duckdb-rs `VTab` impl) RETIRED — Phase 65
// Plan 05 Batch 3. The C++ Catalog API path
// (`sv_register_explain_semantic_view` → `sv_explain_semantic_view_bind_rust`
// above) is the sole registration target.
// ---------------------------------------------------------------------------
