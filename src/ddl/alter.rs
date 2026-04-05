use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;

use crate::catalog::{catalog_rename, CatalogState};

/// Shared state for `alter_semantic_view_rename` and
/// `alter_semantic_view_rename_if_exists`.
///
/// See [`crate::ddl::define::DefineState`] for the persist_conn pattern.
#[derive(Clone)]
pub struct AlterRenameState {
    pub catalog: CatalogState,
    pub persist_conn: Option<ffi::duckdb_connection>,
    /// When true, silently succeeds if the view does not exist.
    pub if_exists: bool,
}

// SAFETY: duckdb_connection is an opaque pointer managed by DuckDB.
unsafe impl Send for AlterRenameState {}
unsafe impl Sync for AlterRenameState {}

/// Persist a rename in `semantic_layer._definitions` using the separate
/// persist_conn: DELETE old row, INSERT new row with updated name.
fn persist_rename(conn: ffi::duckdb_connection, old_name: &str, new_name: &str, json: &str) {
    unsafe {
        // Delete old row
        let _ = super::persist::execute_parameterized(
            conn,
            "DELETE FROM semantic_layer._definitions WHERE name = $1",
            &[old_name],
        );
        // Insert new row
        let _ = super::persist::execute_parameterized(
            conn,
            "INSERT INTO semantic_layer._definitions (name, definition) VALUES ($1, $2)",
            &[new_name, json],
        );
    }
}

/// Bind-time data for the ALTER RENAME table function.
pub struct AlterRenameBindData {
    old_name: String,
    new_name: String,
}

// SAFETY: String is Send + Sync.
unsafe impl Send for AlterRenameBindData {}
unsafe impl Sync for AlterRenameBindData {}

/// Init data for the ALTER RENAME table function.
pub struct AlterRenameInitData {
    done: AtomicBool,
}

// SAFETY: AtomicBool is Send + Sync.
unsafe impl Send for AlterRenameInitData {}
unsafe impl Sync for AlterRenameInitData {}

/// `alter_semantic_view_rename(old_name, new_name)` table function.
///
/// Renames a semantic view definition. Errors if the old view does not exist
/// (unless `if_exists` is true) or the new name already exists.
pub struct AlterRenameVTab;

impl VTab for AlterRenameVTab {
    type BindData = AlterRenameBindData;
    type InitData = AlterRenameInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Declare output schema: two VARCHAR columns.
        bind.add_result_column("old_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column("new_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));

        // Read parameters: old_name (0), new_name (1).
        let old_name = bind.get_parameter(0).to_string();
        let new_name = bind.get_parameter(1).to_string();

        // Access the AlterRenameState from extra_info.
        let state_ptr = bind.get_extra_info::<AlterRenameState>();
        let state = unsafe { &*state_ptr };

        // Check if old_name exists in catalog.
        let json = {
            let guard = state.catalog.read().unwrap();
            guard.get(&old_name).cloned()
        };

        match json {
            None => {
                if state.if_exists {
                    // Silent no-op: return the names but don't modify anything
                    return Ok(AlterRenameBindData { old_name, new_name });
                }
                return Err(format!("semantic view '{old_name}' does not exist").into());
            }
            Some(json_str) => {
                // Check if new_name already exists
                {
                    let guard = state.catalog.read().unwrap();
                    if guard.contains_key(&new_name) {
                        return Err(format!("semantic view '{new_name}' already exists").into());
                    }
                }

                // 1. Persist first (write-first for consistency).
                if let Some(conn) = state.persist_conn {
                    persist_rename(conn, &old_name, &new_name, &json_str);
                }

                // 2. Update in-memory catalog.
                catalog_rename(&state.catalog, &old_name, &new_name)?;
            }
        }

        Ok(AlterRenameBindData { old_name, new_name })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(AlterRenameInitData {
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
        let old_vec = output.flat_vector(0);
        old_vec.insert(0, bind_data.old_name.as_str());
        let new_vec = output.flat_vector(1);
        new_vec.insert(0, bind_data.new_name.as_str());
        output.set_len(1);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        // Two positional parameters: old_name and new_name (both VARCHAR)
        Some(vec![
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        ])
    }
}
