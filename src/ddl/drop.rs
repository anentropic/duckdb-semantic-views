use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys as ffi;
use libduckdb_sys::duckdb_string_t;
use std::ffi::CString;

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
/// Returns Ok(()) always — silently ignores SQL errors (the row may not exist
/// in the table for in-memory sessions where it was never persisted).
fn persist_drop(conn: ffi::duckdb_connection, name: &str) {
    let safe_name = name.replace('\'', "''");
    let sql = format!(
        "DELETE FROM semantic_layer._definitions WHERE name = '{}'",
        safe_name
    );
    if let Ok(c_sql) = CString::new(sql) {
        unsafe {
            let mut result: ffi::duckdb_result = std::mem::zeroed();
            ffi::duckdb_query(conn, c_sql.as_ptr(), &mut result);
            ffi::duckdb_destroy_result(&mut result);
        }
    }
}

/// `drop_semantic_view(name)` scalar function.
///
/// Removes a semantic view definition. Errors if the view does not exist.
/// Use `drop_semantic_view_if_exists` for silent no-op when absent.
pub struct DropSemanticView;

impl VScalar for DropSemanticView {
    type State = DropState;

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)],
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // returns view name on success
        )]
    }

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let name_col = input.flat_vector(0);
        let names = name_col.as_slice_with_len::<duckdb_string_t>(input.len());

        let out = output.flat_vector();
        for (i, raw) in names.iter().enumerate().take(input.len()) {
            let name = duckdb::types::DuckString::new(&mut { *raw })
                .as_str()
                .to_string();

            // Check catalog first — gives better error messages than the SQL DELETE.
            let exists = {
                let guard = state.catalog.read().unwrap();
                guard.contains_key(&name)
            };

            if !exists && !state.if_exists {
                return Err(format!("semantic view '{name}' does not exist").into());
            }

            if exists {
                // 1. Delete from DuckDB table FIRST (write-first for consistency).
                //    Uses a separate connection — no deadlock with invoke's execution lock.
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

            out.insert(i, name.as_str());
        }
        Ok(())
    }
}
