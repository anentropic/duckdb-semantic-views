use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogState;

/// Bind-time snapshot of all registered semantic views.
///
/// Populated once at bind time by reading the in-memory catalog.
/// Stored as a `Vec<(name, base_table)>` — one entry per view.
pub struct ListBindData {
    rows: Vec<(String, String)>,
}

// SAFETY: `Vec<(String, String)>` is `Send + Sync`.
unsafe impl Send for ListBindData {}
unsafe impl Sync for ListBindData {}

/// Init data for `list_semantic_views`: tracks whether rows have been emitted.
pub struct ListInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ListInitData {}
unsafe impl Sync for ListInitData {}

/// Table function that returns `(name VARCHAR, base_table VARCHAR)` — one row
/// per registered semantic view.
///
/// Takes no parameters.  State is injected via
/// `register_table_function_with_extra_info`.
pub struct ListSemanticViewsVTab;

impl VTab for ListSemanticViewsVTab {
    type BindData = ListBindData;
    type InitData = ListInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column(
            "base_table",
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        );

        // Access the shared catalog state injected via extra_info.
        // `get_extra_info` returns a raw pointer; we dereference it to clone
        // the `Arc` — we do NOT take ownership.
        let state_ptr = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state_ptr).read().expect("catalog RwLock poisoned") };

        let mut rows = Vec::with_capacity(guard.len());
        for (name, json) in guard.iter() {
            let base_table = serde_json::from_str::<serde_json::Value>(json)
                .ok()
                .and_then(|v| v["base_table"].as_str().map(str::to_string))
                .unwrap_or_default();
            rows.push((name.clone(), base_table));
        }
        // Sort for deterministic output order.
        rows.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(ListBindData { rows })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ListInitData {
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

        let name_vec = output.flat_vector(0);
        let base_table_vec = output.flat_vector(1);
        for (i, (name, base_table)) in bind_data.rows.iter().enumerate() {
            name_vec.insert(i, name.as_str());
            base_table_vec.insert(i, base_table.as_str());
        }
        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}
