use crate::catalog::CatalogReader;
use crate::ddl::describe::format_json_array;
use crate::model::SemanticViewDefinition;

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 2 (Wave 1) — sv_show_semantic_dimensions_all_bind_rust
// ---------------------------------------------------------------------------
// FFI dispatcher for the migrated show_semantic_dimensions_all() TF. Same
// bridge mechanism + borrow contract as the Wave 0 spike. 8-column VARCHAR
// schema: (database_name, schema_name, semantic_view_name, table_name, name,
// data_type, synonyms, comment).
//
// data_type is whatever was persisted in the JSON definition (empty on
// v0.10.0 fresh CREATEs because Plan 03 removed CREATE-time type inference).

/// # Safety
///
/// `conn` is a borrowed handle (see `src/ddl/list.rs` file-level docs for
/// the borrow contract). Caller releases the returned buffer via
/// `sv_free_buffer(*out_ptr, *out_len)`.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_show_semantic_dimensions_all_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
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
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        let reader = CatalogReader::new(&borrowed, probe_catalog_table_present(&borrowed));
        let entries = match reader.list_all() {
            Ok(e) => e,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let mut rows: Vec<Vec<String>> = Vec::new();
        for (name, json) in &entries {
            for r in collect_dims(name, json) {
                rows.push(vec![
                    r.database_name,
                    r.schema_name,
                    r.semantic_view_name,
                    r.table_name,
                    r.name,
                    r.data_type,
                    r.synonyms,
                    r.comment,
                ]);
            }
        }
        rows.sort_by(|a, b| a[2].cmp(&b[2]).then_with(|| a[4].cmp(&b[4])));
        let buf = serialize_varchar_rows(&rows);
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
                "internal error: panic inside sv_show_semantic_dimensions_all_bind_rust",
            );
            2
        }
    }
}

/// A single row in the SHOW SEMANTIC DIMENSIONS output.
///
/// 8 Snowflake-aligned columns: database_name, schema_name, semantic_view_name,
/// table_name, name, data_type, synonyms, comment.
struct ShowDimRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    table_name: String,
    name: String,
    data_type: String,
    synonyms: String,
    comment: String,
}

// Phase 65 Plan 05 Batch 3: legacy `ShowDimsBindData` + `ShowDimsInitData`
// + `bind_output_columns` + `emit_rows` retired with the H2 query_conn
// allocation. `ShowDimRow` + `collect_dims` remain because the new
// `sv_show_semantic_dimensions_bind_rust` / `_all_bind_rust` dispatchers
// still call them to assemble the wire format.

/// Helper: collect dimension rows for a single view.
fn collect_dims(view_name: &str, json: &str) -> Vec<ShowDimRow> {
    let Ok(def) = SemanticViewDefinition::from_json(view_name, json) else {
        return Vec::new();
    };
    let db_name = def.database_name.clone().unwrap_or_default();
    let sch_name = def.schema_name.clone().unwrap_or_default();
    let alias_map = def.alias_to_table_map();
    def.dimensions
        .iter()
        .map(|d| {
            let table_name = d
                .source_table
                .as_ref()
                .and_then(|a| alias_map.get(a).cloned())
                .unwrap_or_default();
            ShowDimRow {
                database_name: db_name.clone(),
                schema_name: sch_name.clone(),
                semantic_view_name: view_name.to_string(),
                table_name,
                name: d.name.clone(),
                data_type: d.output_type.clone().unwrap_or_default(),
                synonyms: format_json_array(&d.synonyms),
                comment: d.comment.clone().unwrap_or_default(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Single-view form: show_semantic_dimensions('view_name')
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 3 (Wave 2) — sv_show_semantic_dimensions_bind_rust
// ---------------------------------------------------------------------------
// Single-view variant. 8 VARCHAR cols (same shape as the _all variant).

/// # Safety
///
/// `conn` is a borrowed handle; `name_ptr` must point to `name_len` UTF-8 bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_show_semantic_dimensions_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    name_ptr: *const u8,
    name_len: usize,
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
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        if name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "view name pointer is null");
            return 1_u8;
        }
        let view_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s.to_string(),
            Err(_) => {
                write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
                return 1_u8;
            }
        };
        let reader = CatalogReader::new(&borrowed, probe_catalog_table_present(&borrowed));
        let json = match reader.lookup(&view_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &crate::catalog::view_not_found_msg(&view_name),
                );
                return 1_u8;
            }
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let mut internal = collect_dims(&view_name, &json);
        internal.sort_by(|a, b| a.name.cmp(&b.name));
        let mut rows: Vec<Vec<String>> = Vec::with_capacity(internal.len());
        for r in internal {
            rows.push(vec![
                r.database_name,
                r.schema_name,
                r.semantic_view_name,
                r.table_name,
                r.name,
                r.data_type,
                r.synonyms,
                r.comment,
            ]);
        }
        let buf = serialize_varchar_rows(&rows);
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
                "internal error: panic inside sv_show_semantic_dimensions_bind_rust",
            );
            2
        }
    }
}

// Legacy `ShowSemanticDimensionsVTab` + `ShowSemanticDimensionsAllVTab`
// (duckdb-rs VTab impls) RETIRED — Phase 65 Plan 05 Batch 3. The C++
// Catalog API paths (`sv_register_show_semantic_dimensions` /
// `sv_register_show_semantic_dimensions_all`) dispatch via the
// `sv_show_semantic_dimensions_bind_rust` / `_all_bind_rust` Rust
// dispatchers above.

// ---------------------------------------------------------------------------
// Cross-view form: show_semantic_dimensions_all()
// ---------------------------------------------------------------------------
// (Legacy VTab block retired in Plan 05 Batch 3 — see comment above.)
