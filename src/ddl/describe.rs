use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogState;

/// Bind-time data for `describe_semantic_view`: the parsed fields of one view.
///
/// The JSON array fields (`dimensions`, `metrics`, `filters`, `joins`) are
/// stored as their serialized JSON strings so they can be returned as VARCHAR
/// columns without re-serializing at emit time.
pub struct DescribeBindData {
    name: String,
    base_table: String,
    dimensions: String,
    metrics: String,
    filters: String,
    joins: String,
}

// SAFETY: all fields are `String`, which is `Send + Sync`.
unsafe impl Send for DescribeBindData {}
unsafe impl Sync for DescribeBindData {}

/// Init data for `describe_semantic_view`: tracks whether the single row has
/// been emitted.
pub struct DescribeInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for DescribeInitData {}
unsafe impl Sync for DescribeInitData {}

/// Table function that returns one row describing a named semantic view.
///
/// Output schema:
///   `(name VARCHAR, base_table VARCHAR, dimensions VARCHAR, metrics VARCHAR,
///     filters VARCHAR, joins VARCHAR)`
///
/// The `dimensions`, `metrics`, `filters`, and `joins` columns contain the
/// JSON-serialized arrays from the definition.
///
/// Takes one positional VARCHAR parameter: the view name.
pub struct DescribeSemanticViewVTab;

impl VTab for DescribeSemanticViewVTab {
    type BindData = DescribeBindData;
    type InitData = DescribeInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Declare output columns — all VARCHAR (RESEARCH.md Pitfall 6).
        bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column(
            "base_table",
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        );
        bind.add_result_column(
            "dimensions",
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        );
        bind.add_result_column("metrics", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column("filters", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column("joins", LogicalTypeHandle::from(LogicalTypeId::Varchar));

        // Read the name parameter.
        let name = bind.get_parameter(0).to_string();

        // Access the shared catalog state injected via extra_info.
        let state_ptr = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state_ptr).read().expect("catalog RwLock poisoned") };

        let json_str = guard
            .get(&name)
            .ok_or_else(|| format!("semantic view '{name}' does not exist"))?;

        // Parse the stored JSON to extract individual fields.
        let def: serde_json::Value = serde_json::from_str(json_str)?;

        let base_table = def["base_table"].as_str().unwrap_or("").to_string();

        // Re-serialize the array fields back to JSON strings for the VARCHAR columns.
        let dimensions =
            serde_json::to_string(&def["dimensions"]).unwrap_or_else(|_| "[]".to_string());
        let metrics = serde_json::to_string(&def["metrics"]).unwrap_or_else(|_| "[]".to_string());
        let filters = serde_json::to_string(&def["filters"]).unwrap_or_else(|_| "[]".to_string());
        let joins = serde_json::to_string(&def["joins"]).unwrap_or_else(|_| "[]".to_string());

        Ok(DescribeBindData {
            name,
            base_table,
            dimensions,
            metrics,
            filters,
            joins,
        })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(DescribeInitData {
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

        let name_vec = output.flat_vector(0);
        let base_table_vec = output.flat_vector(1);
        let dimensions_vec = output.flat_vector(2);
        let metrics_vec = output.flat_vector(3);
        let filters_vec = output.flat_vector(4);
        let joins_vec = output.flat_vector(5);

        name_vec.insert(0, bind_data.name.as_str());
        base_table_vec.insert(0, bind_data.base_table.as_str());
        dimensions_vec.insert(0, bind_data.dimensions.as_str());
        metrics_vec.insert(0, bind_data.metrics.as_str());
        filters_vec.insert(0, bind_data.filters.as_str());
        joins_vec.insert(0, bind_data.joins.as_str());

        output.set_len(1);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}
