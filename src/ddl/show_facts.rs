use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogReader;
use crate::ddl::describe::format_json_array;
use crate::model::SemanticViewDefinition;

/// A single row in the SHOW SEMANTIC FACTS output.
///
/// 8 Snowflake-aligned columns: database_name, schema_name, semantic_view_name,
/// table_name, name, data_type, synonyms, comment.
struct ShowFactRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    table_name: String,
    name: String,
    data_type: String,
    synonyms: String,
    comment: String,
}

/// Bind-time data: pre-collected fact rows.
pub struct ShowFactsBindData {
    rows: Vec<ShowFactRow>,
}

// SAFETY: all fields are owned `Vec<ShowFactRow>` (String fields), which is `Send + Sync`.
unsafe impl Send for ShowFactsBindData {}
unsafe impl Sync for ShowFactsBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct ShowFactsInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ShowFactsInitData {}
unsafe impl Sync for ShowFactsInitData {}

/// Helper: declare the 8-column output schema for facts.
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
    bind.add_result_column(
        "table_name",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("data_type", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("synonyms", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("comment", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}

/// Helper: collect fact rows for a single view.
fn collect_facts(view_name: &str, json: &str) -> Vec<ShowFactRow> {
    let Ok(def) = SemanticViewDefinition::from_json(view_name, json) else {
        return Vec::new();
    };
    let db_name = def.database_name.clone().unwrap_or_default();
    let sch_name = def.schema_name.clone().unwrap_or_default();
    let alias_map = def.alias_to_table_map();
    def.facts
        .iter()
        .map(|f| {
            let table_name = f
                .source_table
                .as_ref()
                .and_then(|a| alias_map.get(a).cloned())
                .unwrap_or_default();
            ShowFactRow {
                database_name: db_name.clone(),
                schema_name: sch_name.clone(),
                semantic_view_name: view_name.to_string(),
                table_name,
                name: f.name.clone(),
                data_type: f.output_type.clone().unwrap_or_default(),
                synonyms: format_json_array(&f.synonyms),
                comment: f.comment.clone().unwrap_or_default(),
            }
        })
        .collect()
}

/// Helper: emit rows from bind data into the output chunk.
fn emit_rows(
    func: &TableFunctionInfo<impl VTab<BindData = ShowFactsBindData, InitData = ShowFactsInitData>>,
    output: &mut DataChunkHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let init_data = func.get_init_data();
    if init_data.done.swap(true, Ordering::Relaxed) {
        output.set_len(0);
        return Ok(());
    }

    let bind_data = func.get_bind_data();
    let n = bind_data.rows.len();

    let db_name_vec = output.flat_vector(0);
    let sch_name_vec = output.flat_vector(1);
    let sv_vec = output.flat_vector(2);
    let table_vec = output.flat_vector(3);
    let name_vec = output.flat_vector(4);
    let type_vec = output.flat_vector(5);
    let syn_vec = output.flat_vector(6);
    let cmt_vec = output.flat_vector(7);

    for (i, row) in bind_data.rows.iter().enumerate() {
        db_name_vec.insert(i, row.database_name.as_str());
        sch_name_vec.insert(i, row.schema_name.as_str());
        sv_vec.insert(i, row.semantic_view_name.as_str());
        table_vec.insert(i, row.table_name.as_str());
        name_vec.insert(i, row.name.as_str());
        type_vec.insert(i, row.data_type.as_str());
        syn_vec.insert(i, row.synonyms.as_str());
        cmt_vec.insert(i, row.comment.as_str());
    }
    output.set_len(n);
    Ok(())
}

// ---------------------------------------------------------------------------
// Single-view form: show_semantic_facts('view_name')
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC FACTS IN view_name`
///
/// Takes one VARCHAR parameter (view name). Returns facts for that view.
pub struct ShowSemanticFactsVTab;

impl VTab for ShowSemanticFactsVTab {
    type BindData = ShowFactsBindData;
    type InitData = ShowFactsInitData;

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

            let mut rows = collect_facts(&view_name, &json);
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ShowFactsBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowFactsInitData {
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
// Cross-view form: show_semantic_facts_all()
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC FACTS` (no IN clause)
///
/// Takes no parameters. Returns facts across all registered semantic views.
pub struct ShowSemanticFactsAllVTab;

impl VTab for ShowSemanticFactsAllVTab {
    type BindData = ShowFactsBindData;
    type InitData = ShowFactsInitData;

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
                rows.extend(collect_facts(name, json));
            }
            rows.sort_by(|a, b| {
                a.semantic_view_name
                    .cmp(&b.semantic_view_name)
                    .then_with(|| a.name.cmp(&b.name))
            });

            Ok(ShowFactsBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowFactsInitData {
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
