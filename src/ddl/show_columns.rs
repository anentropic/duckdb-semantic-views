use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

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
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if conn.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        if name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "view name pointer is null");
            return 1_u8;
        }
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let view_name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
                return 1_u8;
            }
        };
        let reader = CatalogReader::new(conn, probe_catalog_table_present(conn));
        let json = match reader.lookup(&view_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("Semantic view '{view_name}' not found"),
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
                write_err(error_buf, error_buf_len, &e.to_string());
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
                "internal error: panic inside sv_show_columns_in_semantic_view_bind_rust",
            );
            2
        }
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

/// Bind-time data: pre-collected column rows.
pub struct ShowColumnsBindData {
    rows: Vec<ShowColumnRow>,
}

// SAFETY: all fields are owned `Vec<ShowColumnRow>` (String fields), which is `Send + Sync`.
unsafe impl Send for ShowColumnsBindData {}
unsafe impl Sync for ShowColumnsBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct ShowColumnsInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ShowColumnsInitData {}
unsafe impl Sync for ShowColumnsInitData {}

/// Collect column rows from a semantic view definition.
/// Includes all dimensions, public facts, and public metrics.
/// Derived metrics (no source_table) get kind "DERIVED_METRIC".
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

/// Table function that returns 8 columns for SHOW COLUMNS IN SEMANTIC VIEW:
/// (database_name, schema_name, semantic_view_name, column_name, data_type, kind, expression, comment)
///
/// Takes one VARCHAR parameter: the semantic view name.
/// Excludes PRIVATE facts and metrics from output.
///
/// Phase 65 Plan 05 Task 3 (Wave 2): registration retired in favor of the
/// C++ Catalog API path.
#[allow(dead_code)]
pub struct ShowColumnsInSemanticViewVTab;

impl VTab for ShowColumnsInSemanticViewVTab {
    type BindData = ShowColumnsBindData;
    type InitData = ShowColumnsInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind.add_result_column(
                "database_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "schema_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "semantic_view_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "column_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("data_type", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column("kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column(
                "expression",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("comment", LogicalTypeHandle::from(LogicalTypeId::Varchar));

            let view_name = bind.get_parameter(0).to_string();

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let json = reader
                .lookup(&view_name)
                .map_err(Box::<dyn std::error::Error>::from)?
                .ok_or_else(|| format!("Semantic view '{view_name}' not found"))?;

            let def = SemanticViewDefinition::from_json(&view_name, &json)?;
            let rows = collect_column_rows(&def, &view_name);

            Ok(ShowColumnsBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowColumnsInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        if init_data.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let bind_data = func.get_bind_data();
        let n = bind_data.rows.len();

        let db_vec = output.flat_vector(0);
        let schema_vec = output.flat_vector(1);
        let view_vec = output.flat_vector(2);
        let col_vec = output.flat_vector(3);
        let type_vec = output.flat_vector(4);
        let kind_vec = output.flat_vector(5);
        let expr_vec = output.flat_vector(6);
        let comment_vec = output.flat_vector(7);

        for (i, row) in bind_data.rows.iter().enumerate() {
            db_vec.insert(i, row.database_name.as_str());
            schema_vec.insert(i, row.schema_name.as_str());
            view_vec.insert(i, row.semantic_view_name.as_str());
            col_vec.insert(i, row.column_name.as_str());
            type_vec.insert(i, row.data_type.as_str());
            kind_vec.insert(i, row.kind.as_str());
            expr_vec.insert(i, row.expression.as_str());
            comment_vec.insert(i, row.comment.as_str());
        }
        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}
