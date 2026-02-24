use std::sync::Arc;

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{catalog_delete, write_sidecar, CatalogState};

/// Shared state for `drop_semantic_view`.
/// See [`crate::ddl::define::DefineState`] for the full explanation of the
/// sidecar persistence pattern.
#[derive(Clone)]
pub struct DropState {
    pub catalog: CatalogState,
    pub db_path: Arc<str>,
}

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

            // 1. Remove from in-memory catalog (checks existence).
            catalog_delete(&state.catalog, &name)?;

            // 2. Persist to sidecar file (file-backed databases only).
            if state.db_path.as_ref() != ":memory:" {
                write_sidecar(&state.db_path, &state.catalog)?;
            }

            let msg = format!("Semantic view '{name}' removed successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
