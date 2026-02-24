use std::sync::Arc;

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
    Connection,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{catalog_delete, init_catalog, CatalogState};

/// Shared state for `drop_semantic_view`.
///
/// Stores the catalog `HashMap` and the database file path for opening a fresh
/// `Connection` inside `invoke` (see [`crate::ddl::define::DefineState`] for the
/// full explanation of why a path is stored instead of a `Connection`).
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

            // Open a fresh connection to the same database file for catalog writes.
            // `init_catalog` is called first to ensure schema and table exist on
            // the fresh connection before attempting the delete.
            let con = Connection::open(state.db_path.as_ref())?;
            init_catalog(&con)?;
            catalog_delete(&con, &state.catalog, &name)?;

            let msg = format!("Semantic view '{name}' removed successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
