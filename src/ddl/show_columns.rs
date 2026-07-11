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
    crate::ddl::read_ffi::run_dispatcher(
        conn,
        out_ptr,
        out_len,
        error_buf,
        error_buf_len,
        "sv_show_columns_in_semantic_view_bind_rust",
        |borrowed| unsafe { show_columns_rows(borrowed, name_ptr, name_len) },
    )
}

/// Body for [`sv_show_columns_in_semantic_view_bind_rust`]: resolve the view
/// and serialize its column rows over the shared varchar wire format.
///
/// # Safety
///
/// `name_ptr` must be null or point to `name_len` readable bytes.
#[cfg(feature = "extension")]
unsafe fn show_columns_rows(
    borrowed: &crate::ddl::read_ffi::BorrowedConnection,
    name_ptr: *const u8,
    name_len: usize,
) -> Result<Vec<u8>, String> {
    use crate::ddl::read_ffi::{probe_catalog_table_present, read_str_arg, serialize_varchar_rows};

    let raw_name = read_str_arg(name_ptr, name_len, "view name")?;
    // FF-4: normalize so quoted-identifier inputs resolve like `semantic_view()`.
    let view_name = crate::ident::normalize_view_name(&raw_name)
        .map_err(|e| format!("Invalid view name '{raw_name}': {e}"))?;
    // FF-9: a probe-query failure is distinct from "no views" (propagated).
    let present = probe_catalog_table_present(borrowed)?;
    let reader = CatalogReader::new(borrowed, present);
    // C-4 (code-review 2026-07-11): canonical wording via view_not_found_msg.
    let json = reader
        .lookup(&view_name)?
        .ok_or_else(|| crate::catalog::view_not_found_msg(&view_name))?;
    let def = SemanticViewDefinition::from_json(&view_name, &json)?;
    let rows: Vec<Vec<String>> = collect_column_rows(&def, &view_name)
        .into_iter()
        .map(|r| {
            vec![
                r.database_name,
                r.schema_name,
                r.semantic_view_name,
                r.column_name,
                r.data_type,
                r.kind,
                r.expression,
                r.comment,
            ]
        })
        .collect();
    serialize_varchar_rows(&rows)
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
