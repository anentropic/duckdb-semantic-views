use std::ffi::CString;

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{catalog_delete, CatalogState};

/// Shared state for `drop_semantic_view`.
/// See [`crate::ddl::define::DefineState`] for the persist_conn pattern.
#[derive(Clone)]
pub struct DropState {
    pub catalog: CatalogState,
    pub persist_conn: Option<libduckdb_sys::duckdb_connection>,
}

// SAFETY: duckdb_connection is an opaque pointer managed by DuckDB.
// DuckDB handles concurrent access internally.
unsafe impl Send for DropState {}
unsafe impl Sync for DropState {}

pub struct DropSemanticView;

impl VScalar for DropSemanticView {
    type State = DropState;

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // view name
            ],
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // confirmation message
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

            // 1. Delete from DuckDB table FIRST (write-first for consistency).
            //    Uses a separate connection — no deadlock with invoke's execution lock.
            #[cfg(feature = "extension")]
            if let Some(conn) = state.persist_conn {
                let c_name = CString::new(name.as_str())
                    .map_err(|_| format!("semantic view name '{}' contains null byte", name))?;
                let rc =
                    unsafe { crate::shim::ffi::semantic_views_pragma_drop(conn, c_name.as_ptr()) };
                if rc != 0 {
                    return Err(format!(
                        "failed to remove semantic view '{}' from persistent storage",
                        name
                    )
                    .into());
                }
            }

            // 2. Remove from in-memory catalog AFTER successful table delete.
            catalog_delete(&state.catalog, &name)?;

            let msg = format!("Semantic view '{name}' removed successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
