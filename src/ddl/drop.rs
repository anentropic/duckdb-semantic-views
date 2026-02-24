use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{CatalogState, CatalogWriterHandle};

/// Shared state for `drop_semantic_view`.
/// See [`crate::ddl::define::DefineState`] for the full explanation of the
/// `Option<CatalogWriterHandle>` pattern.
#[derive(Clone)]
pub struct DropState {
    pub catalog: CatalogState,
    pub writer: Option<CatalogWriterHandle>,
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

            // 1. Check the view exists in the in-memory catalog.
            {
                let guard = state.catalog.read().unwrap();
                if !guard.contains_key(&name) {
                    return Err(
                        format!("semantic view '{name}' does not exist").into()
                    );
                }
            }

            // 2. Persist the DELETE via background writer thread (file-backed only).
            if let Some(ref writer) = state.writer {
                writer.delete(&name)?;
            }

            // 3. Remove from in-memory catalog after successful persist.
            state.catalog.write().unwrap().remove(&name);

            let msg = format!("Semantic view '{name}' removed successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
