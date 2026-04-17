use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;

use crate::catalog::{catalog_delete, catalog_delete_if_exists, CatalogState};

/// Shared state for `drop_semantic_view` and `drop_semantic_view_if_exists`.
/// See [`crate::ddl::define::DefineState`] for the persist_conn pattern.
#[derive(Clone)]
pub struct DropState {
    pub catalog: CatalogState,
    pub persist_conn: Option<ffi::duckdb_connection>,
    /// When true, silently succeeds if the view does not exist.
    pub if_exists: bool,
}

// SAFETY: duckdb_connection is an opaque pointer managed by DuckDB.
unsafe impl Send for DropState {}
unsafe impl Sync for DropState {}

/// Persist the removal of a view from `semantic_layer._definitions` using
/// the separate persist_conn.
///
/// Returns Ok(()) always -- silently ignores SQL errors (the row may not exist
/// in the table for in-memory sessions where it was never persisted).
fn persist_drop(conn: ffi::duckdb_connection, name: &str) {
    unsafe {
        let _ = super::persist::execute_parameterized(
            conn,
            "DELETE FROM semantic_layer._definitions WHERE name = $1",
            &[name],
        );
    }
}

/// Bind-time data for the DDL drop table function.
pub struct DropBindData {
    name: String,
}

// SAFETY: String is Send + Sync.
unsafe impl Send for DropBindData {}
unsafe impl Sync for DropBindData {}

/// Init data for the DDL drop table function.
pub struct DropInitData {
    done: AtomicBool,
}

// SAFETY: AtomicBool is Send + Sync.
unsafe impl Send for DropInitData {}
unsafe impl Sync for DropInitData {}

/// `drop_semantic_view(name)` table function.
///
/// Removes a semantic view definition. Errors if the view does not exist.
/// Use `drop_semantic_view_if_exists` for silent no-op when absent.
pub struct DropSemanticViewVTab;

impl VTab for DropSemanticViewVTab {
    type BindData = DropBindData;
    type InitData = DropInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            // Declare output schema: single VARCHAR column with the view name.
            bind.add_result_column("view_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));

            // Read view name (positional parameter 0).
            let name = bind.get_parameter(0).to_string();

            // Access the DropState from extra_info.
            let state_ptr = bind.get_extra_info::<DropState>();
            let state = unsafe { &*state_ptr };

            // Check catalog first -- gives better error messages than the SQL DELETE.
            let exists = {
                let guard = state
                    .catalog
                    .read()
                    .map_err(|_| Box::<dyn std::error::Error>::from("catalog lock poisoned"))?;
                guard.contains_key(&name)
            };

            if !exists && !state.if_exists {
                return Err(format!("semantic view '{name}' does not exist").into());
            }

            if exists {
                // 1. Delete from DuckDB table FIRST (write-first for consistency).
                if let Some(conn) = state.persist_conn {
                    persist_drop(conn, &name);
                }

                // 2. Remove from in-memory catalog.
                if state.if_exists {
                    catalog_delete_if_exists(&state.catalog, &name);
                } else {
                    catalog_delete(&state.catalog, &name)?;
                }
            }

            Ok(DropBindData { name })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(DropInitData {
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
        name_vec.insert(0, bind_data.name.as_str());
        output.set_len(1);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        // Positional parameter: view name (VARCHAR)
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}
