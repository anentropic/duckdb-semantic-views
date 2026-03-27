use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogState;
use crate::model::SemanticViewDefinition;

/// A single row in the SHOW SEMANTIC METRICS output.
struct ShowMetricRow {
    semantic_view_name: String,
    name: String,
    expr: String,
    source_table: String,
    data_type: String,
}

/// Bind-time data: pre-collected metric rows.
pub struct ShowMetricsBindData {
    rows: Vec<ShowMetricRow>,
}

// SAFETY: all fields are owned `Vec<ShowMetricRow>` (String fields), which is `Send + Sync`.
unsafe impl Send for ShowMetricsBindData {}
unsafe impl Sync for ShowMetricsBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct ShowMetricsInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ShowMetricsInitData {}
unsafe impl Sync for ShowMetricsInitData {}

/// Helper: declare the 5-column output schema for metrics.
fn bind_output_columns(bind: &BindInfo) {
    bind.add_result_column(
        "semantic_view_name",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("expr", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column(
        "source_table",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column("data_type", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}

/// Helper: collect metric rows for a single view.
fn collect_metrics(view_name: &str, json: &str) -> Vec<ShowMetricRow> {
    let Ok(def) = SemanticViewDefinition::from_json(view_name, json) else {
        return Vec::new();
    };
    def.metrics
        .iter()
        .map(|m| ShowMetricRow {
            semantic_view_name: view_name.to_string(),
            name: m.name.clone(),
            expr: m.expr.clone(),
            source_table: m.source_table.clone().unwrap_or_default(),
            data_type: m.output_type.clone().unwrap_or_default(),
        })
        .collect()
}

/// Helper: emit rows from bind data into the output chunk.
fn emit_rows(
    func: &TableFunctionInfo<
        impl VTab<BindData = ShowMetricsBindData, InitData = ShowMetricsInitData>,
    >,
    output: &mut DataChunkHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let init_data = func.get_init_data();
    if init_data.done.swap(true, Ordering::Relaxed) {
        output.set_len(0);
        return Ok(());
    }

    let bind_data = func.get_bind_data();
    let n = bind_data.rows.len();

    let sv_vec = output.flat_vector(0);
    let name_vec = output.flat_vector(1);
    let expr_vec = output.flat_vector(2);
    let source_vec = output.flat_vector(3);
    let type_vec = output.flat_vector(4);

    for (i, row) in bind_data.rows.iter().enumerate() {
        sv_vec.insert(i, row.semantic_view_name.as_str());
        name_vec.insert(i, row.name.as_str());
        expr_vec.insert(i, row.expr.as_str());
        source_vec.insert(i, row.source_table.as_str());
        type_vec.insert(i, row.data_type.as_str());
    }
    output.set_len(n);
    Ok(())
}

// ---------------------------------------------------------------------------
// Single-view form: show_semantic_metrics('view_name')
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC METRICS IN view_name`
///
/// Takes one VARCHAR parameter (view name). Returns metrics for that view.
pub struct ShowSemanticMetricsVTab;

impl VTab for ShowSemanticMetricsVTab {
    type BindData = ShowMetricsBindData;
    type InitData = ShowMetricsInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind_output_columns(bind);

        let view_name = bind.get_parameter(0).to_string();
        let state_ptr = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state_ptr).read().expect("catalog RwLock poisoned") };

        let json = guard
            .get(&view_name)
            .ok_or_else(|| format!("semantic view '{view_name}' does not exist"))?;

        let mut rows = collect_metrics(&view_name, json);
        rows.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(ShowMetricsBindData { rows })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowMetricsInitData {
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
// Cross-view form: show_semantic_metrics_all()
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC METRICS` (no IN clause)
///
/// Takes no parameters. Returns metrics across all registered semantic views.
pub struct ShowSemanticMetricsAllVTab;

impl VTab for ShowSemanticMetricsAllVTab {
    type BindData = ShowMetricsBindData;
    type InitData = ShowMetricsInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind_output_columns(bind);

        let state_ptr = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state_ptr).read().expect("catalog RwLock poisoned") };

        let mut rows = Vec::new();
        for (name, json) in guard.iter() {
            rows.extend(collect_metrics(name, json));
        }
        // Sort by (semantic_view_name, name) for deterministic output.
        rows.sort_by(|a, b| {
            a.semantic_view_name
                .cmp(&b.semantic_view_name)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(ShowMetricsBindData { rows })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowMetricsInitData {
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
