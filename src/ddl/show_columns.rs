use crate::catalog::CatalogReader;
use crate::model::{AccessModifier, SemanticViewDefinition};

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 3 (Wave 2) — sv_show_columns_in_semantic_view_bind_rust
// ---------------------------------------------------------------------------
// FFI dispatcher for the migrated show_columns_in_semantic_view(view_name) TF.
// 8-column VARCHAR (database_name, schema_name, semantic_view_name,
// column_name, data_type, kind, expression, comment).
//
// Per-call Connection from the C++ bind opens an `information_schema.tables`
// probe to determine catalog-table-present; same borrow contract as
// list_semantic_views (Wave 0 spike).

/// # Safety
///
/// `conn` is a borrowed handle (see `src/ddl/list.rs` file-level docs).
/// `name_ptr` must point to `name_len` UTF-8 bytes (not NUL-terminated).
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_show_columns_in_semantic_view_bind_rust(
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
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let Ok(raw_name) = std::str::from_utf8(name_bytes) else {
            write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
            return 1_u8;
        };
        // FF-4: normalize so quoted-identifier inputs resolve like `semantic_view()`.
        let view_name = match crate::ident::normalize_view_name(raw_name) {
            Ok(s) => s,
            Err(e) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("Invalid view name '{raw_name}': {e}"),
                );
                return 1_u8;
            }
        };
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
        let json = match reader.lookup(&view_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                // C-4 (code-review 2026-07-11): canonical wording via
                // view_not_found_msg — SHOW COLUMNS was the one read path
                // with divergent "not found" phrasing.
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
        let def = match SemanticViewDefinition::from_json(&view_name, &json) {
            Ok(d) => d,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let internal_rows = collect_column_rows(&def, &view_name);
        let mut rows: Vec<Vec<String>> = Vec::with_capacity(internal_rows.len());
        for r in internal_rows {
            rows.push(vec![
                r.database_name,
                r.schema_name,
                r.semantic_view_name,
                r.column_name,
                r.data_type,
                r.kind,
                r.expression,
                r.comment,
            ]);
        }
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
    if let Ok(rc) = result {
        rc
    } else {
        use crate::ddl::read_ffi::write_err;
        write_err(
            error_buf,
            error_buf_len,
            "internal error: panic inside sv_show_columns_in_semantic_view_bind_rust",
        );
        2
    }
}

/// A single row in the SHOW COLUMNS IN SEMANTIC VIEW output.
struct ShowColumnRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    column_name: String,
    data_type: String,
    kind: String,
    expression: String,
    comment: String,
}

// Phase 65 Plan 05 Batch 3: legacy `ShowColumnsBindData` +
// `ShowColumnsInitData` retired with the H2 query_conn allocation.
// `ShowColumnRow` + `collect_column_rows` remain because the new
// `sv_show_columns_in_semantic_view_bind_rust` dispatcher still calls
// them to assemble the wire format.

/// Collect column rows from a semantic view definition.
/// Includes all dimensions, public facts, and public metrics.
/// Derived metrics (no `source_table`) get kind "`DERIVED_METRIC`".
fn collect_column_rows(def: &SemanticViewDefinition, view_name: &str) -> Vec<ShowColumnRow> {
    let database_name = def.database_name.clone().unwrap_or_default();
    let schema_name = def.schema_name.clone().unwrap_or_default();
    let mut rows = Vec::new();

    for dim in &def.dimensions {
        rows.push(ShowColumnRow {
            database_name: database_name.clone(),
            schema_name: schema_name.clone(),
            semantic_view_name: view_name.to_string(),
            column_name: dim.name.clone(),
            data_type: dim.output_type.clone().unwrap_or_default(),
            kind: "DIMENSION".to_string(),
            expression: dim.expr.clone(),
            comment: dim.comment.clone().unwrap_or_default(),
        });
    }

    for fact in &def.facts {
        if fact.access == AccessModifier::Private {
            continue;
        }
        rows.push(ShowColumnRow {
            database_name: database_name.clone(),
            schema_name: schema_name.clone(),
            semantic_view_name: view_name.to_string(),
            column_name: fact.name.clone(),
            data_type: fact.output_type.clone().unwrap_or_default(),
            kind: "FACT".to_string(),
            expression: fact.expr.clone(),
            comment: fact.comment.clone().unwrap_or_default(),
        });
    }

    for metric in &def.metrics {
        if metric.access == AccessModifier::Private {
            continue;
        }
        let kind = if metric.source_table.is_none() {
            "DERIVED_METRIC"
        } else {
            "METRIC"
        };
        rows.push(ShowColumnRow {
            database_name: database_name.clone(),
            schema_name: schema_name.clone(),
            semantic_view_name: view_name.to_string(),
            column_name: metric.name.clone(),
            data_type: metric.output_type.clone().unwrap_or_default(),
            kind: kind.to_string(),
            expression: metric.expr.clone(),
            comment: metric.comment.clone().unwrap_or_default(),
        });
    }

    rows.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.column_name.cmp(&b.column_name))
    });
    rows
}
