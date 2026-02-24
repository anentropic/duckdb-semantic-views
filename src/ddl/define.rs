use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{CatalogState, CatalogWriterHandle};
use crate::model::SemanticViewDefinition;

/// Shared state for `define_semantic_view`: the in-memory catalog plus an optional
/// handle to the background catalog writer thread.
///
/// `writer` is `None` for in-memory databases — the [`CatalogState`] `HashMap` is the
/// sole source of truth for the session, which is correct (in-memory DBs cannot
/// survive restart anyway).
///
/// For file-backed databases, `writer` holds a [`CatalogWriterHandle`] that sends
/// INSERT ops to a background thread owning its own `Connection::open(db_path)`.
/// This avoids executing SQL from within `invoke` while `DuckDB` holds internal locks.
#[derive(Clone)]
pub struct DefineState {
    pub catalog: CatalogState,
    pub writer: Option<CatalogWriterHandle>,
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

            // 1. Validate JSON — fail fast before touching any state.
            SemanticViewDefinition::from_json(&name, &json)
                .map_err(Box::<dyn std::error::Error>::from)?;

            // 2. Check for duplicate in the in-memory catalog.
            {
                let guard = state.catalog.read().unwrap();
                if guard.contains_key(&name) {
                    return Err(format!(
                        "semantic view '{name}' already exists; \
                         call drop_semantic_view first"
                    )
                    .into());
                }
            }

            // 3. Persist to `semantic_layer._definitions` via background writer thread.
            //    For in-memory databases (writer is None) we skip persistence — the
            //    HashMap below is the sole source of truth for the session.
            //
            //    The writer blocks until the background thread confirms the INSERT is
            //    committed, so the row is durable before we update the HashMap.
            if let Some(ref writer) = state.writer {
                writer.insert(&name, &json)?;
            }

            // 4. Update the in-memory catalog after successful persist.
            state
                .catalog
                .write()
                .unwrap()
                .insert(name.clone(), json.clone());

            let msg = format!("Semantic view '{name}' registered successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
