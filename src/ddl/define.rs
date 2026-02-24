use std::sync::Arc;

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{catalog_insert, write_sidecar, CatalogState};

/// Shared state for `define_semantic_view`: the in-memory catalog plus the
/// database file path for sidecar persistence.
///
/// `db_path` is `":memory:"` for in-memory databases — the [`CatalogState`]
/// `HashMap` is the sole source of truth for the session, which is correct
/// (in-memory DBs cannot survive restart anyway).
///
/// For file-backed databases, after updating the `HashMap`, `invoke` writes the
/// full state to a sidecar file (`<db_path>.semantic_views`) using plain
/// filesystem I/O.  On next extension load, `init_catalog` reads the sidecar
/// and syncs it into the `DuckDB` table.
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

            // 1. Validate JSON and update the in-memory catalog.
            //    catalog_insert checks for duplicates and validates the JSON.
            catalog_insert(&state.catalog, &name, &json)?;

            // 2. Persist to sidecar file (file-backed databases only).
            //    Uses plain filesystem I/O — no DuckDB SQL needed, so no
            //    deadlock from within invoke.
            if state.db_path.as_ref() != ":memory:" {
                write_sidecar(&state.db_path, &state.catalog)?;
            }

            let msg = format!("Semantic view '{name}' registered successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
