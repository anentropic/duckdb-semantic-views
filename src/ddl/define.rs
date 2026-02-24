use std::sync::Arc;

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
    Connection,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{catalog_insert, init_catalog, CatalogState};

/// Shared state for `define_semantic_view`: the in-memory catalog plus the
/// database file path needed to open a connection for catalog writes.
///
/// `Connection` is not `Send + Sync`, so it cannot be stored in `VScalar::State`.
/// Instead, we store the file path and open a fresh `Connection` per invocation.
///
/// For in-memory databases (the test sentinel path `":memory:"`), a fresh
/// `Connection::open(":memory:")` creates a *separate* database — catalog writes
/// from inside `invoke` will therefore be to a different DB than the one that
/// registered the function.  The `catalog_insert` call will still succeed
/// (because `init_catalog` is called on the new connection too), but the write
/// will not be visible to the original connection.  This is an accepted v0.1
/// limitation: integration tests that exercise `define_semantic_view` must use a
/// file-backed database.
#[derive(Clone)]
pub struct DefineState {
    pub catalog: CatalogState,
    pub db_path: Arc<str>,
}

pub struct DefineSemanticView;

impl VScalar for DefineSemanticView {
    type State = DefineState;

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // view name
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // definition JSON
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
        let json_col = input.flat_vector(1);
        let jsons = json_col.as_slice_with_len::<duckdb_string_t>(input.len());

        let out = output.flat_vector();
        for i in 0..input.len() {
            let name = duckdb::types::DuckString::new(&mut { names[i] })
                .as_str()
                .to_string();
            let json = duckdb::types::DuckString::new(&mut { jsons[i] })
                .as_str()
                .to_string();

            // Open a fresh connection to the same database file for catalog writes.
            // `Connection` is not `Send`, so it cannot be stored in state.
            //
            // The fresh connection starts with an empty catalog, so we call
            // `init_catalog` to ensure the schema and table exist before writing.
            // `init_catalog` is idempotent (`CREATE IF NOT EXISTS`) and also
            // loads existing rows — the return value (the HashMap) is discarded
            // here because the shared `CatalogState` is the source of truth.
            let con = Connection::open(state.db_path.as_ref())?;
            init_catalog(&con)?;
            catalog_insert(&con, &state.catalog, &name, &json)?;

            let msg = format!("Semantic view '{name}' registered successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
