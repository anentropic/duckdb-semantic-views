use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogReader;
use crate::ddl::describe::format_json_array;
use crate::model::SemanticViewDefinition;

/// A single row in the SHOW SEMANTIC MATERIALIZATIONS output.
///
/// 7 columns: database_name, schema_name, semantic_view_name,
/// name, table, dimensions, metrics.
struct ShowMatRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    name: String,
    table: String,
    dimensions: String,
    metrics: String,
}

/// Bind-time data: pre-collected materialization rows.
pub struct ShowMatBindData {
    rows: Vec<ShowMatRow>,
}

// SAFETY: all fields are owned `Vec<ShowMatRow>` (String fields), which is `Send + Sync`.
unsafe impl Send for ShowMatBindData {}
unsafe impl Sync for ShowMatBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct ShowMatInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ShowMatInitData {}
unsafe impl Sync for ShowMatInitData {}

/// Helper: declare the 7-column output schema for materializations.
fn bind_output_columns(bind: &BindInfo) {
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
    bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("table", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column(
        "dimensions",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column("metrics", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}

/// Helper: collect materialization rows for a single view.
fn collect_mats(view_name: &str, json: &str) -> Vec<ShowMatRow> {
    let Ok(def) = SemanticViewDefinition::from_json(view_name, json) else {
        return Vec::new();
    };
    let db_name = def.database_name.clone().unwrap_or_default();
    let sch_name = def.schema_name.clone().unwrap_or_default();
    def.materializations
        .iter()
        .map(|m| ShowMatRow {
            database_name: db_name.clone(),
            schema_name: sch_name.clone(),
            semantic_view_name: view_name.to_string(),
            name: m.name.clone(),
            table: m.table.clone(),
            dimensions: format_json_array(&m.dimensions),
            metrics: format_json_array(&m.metrics),
        })
        .collect()
}

/// Helper: emit rows from bind data into the output chunk.
fn emit_rows(
    func: &TableFunctionInfo<impl VTab<BindData = ShowMatBindData, InitData = ShowMatInitData>>,
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
    let sch_vec = output.flat_vector(1);
    let sv_vec = output.flat_vector(2);
    let name_vec = output.flat_vector(3);
    let table_vec = output.flat_vector(4);
    let dims_vec = output.flat_vector(5);
    let mets_vec = output.flat_vector(6);

    for (i, row) in bind_data.rows.iter().enumerate() {
        db_vec.insert(i, row.database_name.as_str());
        sch_vec.insert(i, row.schema_name.as_str());
        sv_vec.insert(i, row.semantic_view_name.as_str());
        name_vec.insert(i, row.name.as_str());
        table_vec.insert(i, row.table.as_str());
        dims_vec.insert(i, row.dimensions.as_str());
        mets_vec.insert(i, row.metrics.as_str());
    }
    output.set_len(n);
    Ok(())
}

// ---------------------------------------------------------------------------
// Single-view form: show_semantic_materializations('view_name')
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC MATERIALIZATIONS IN view_name`
///
/// Takes one VARCHAR parameter (view name). Returns materializations for that view.
pub struct ShowSemanticMaterializationsVTab;

impl VTab for ShowSemanticMaterializationsVTab {
    type BindData = ShowMatBindData;
    type InitData = ShowMatInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind_output_columns(bind);

            let view_name = bind.get_parameter(0).to_string();
            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let json = reader
                .lookup(&view_name)
                .map_err(Box::<dyn std::error::Error>::from)?
                .ok_or_else(|| format!("semantic view '{view_name}' does not exist"))?;

            let mut rows = collect_mats(&view_name, &json);
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ShowMatBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowMatInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        emit_rows(func, output)
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}

// ---------------------------------------------------------------------------
// Cross-view form: show_semantic_materializations_all()
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC MATERIALIZATIONS` (no IN clause)
///
/// Takes no parameters. Returns materializations across all registered semantic views.
pub struct ShowSemanticMaterializationsAllVTab;

impl VTab for ShowSemanticMaterializationsAllVTab {
    type BindData = ShowMatBindData;
    type InitData = ShowMatInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind_output_columns(bind);

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader
                .list_all()
                .map_err(Box::<dyn std::error::Error>::from)?;

            let mut rows = Vec::new();
            for (name, json) in &entries {
                rows.extend(collect_mats(name, json));
            }
            rows.sort_by(|a, b| {
                a.semantic_view_name
                    .cmp(&b.semantic_view_name)
                    .then_with(|| a.name.cmp(&b.name))
            });

            Ok(ShowMatBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowMatInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        emit_rows(func, output)
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}
