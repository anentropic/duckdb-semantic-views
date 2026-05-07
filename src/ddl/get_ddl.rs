//! GET_DDL scalar function: wraps [`crate::render_ddl::render_create_ddl`] as a
//! `VScalar` so that `SELECT GET_DDL('SEMANTIC_VIEW', 'name')` works inside DuckDB.
//!
//! The render logic itself lives in [`crate::render_ddl`] (always compiled, unit-tested
//! under `cargo test`). This module adds the extension-only VScalar registration.

use duckdb::core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId};
use duckdb::types::DuckString;
use duckdb::vscalar::{ScalarFunctionSignature, VScalar};
use duckdb::vtab::arrow::WritableVector;
use libduckdb_sys::duckdb_string_t;

use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;
use crate::render_ddl::render_create_ddl;

pub struct GetDdlScalar;

impl VScalar for GetDdlScalar {
    type State = CatalogReader;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            let len = input.len();
            let type_vec = input.flat_vector(0);
            let name_vec = input.flat_vector(1);
            let types = type_vec.as_slice_with_len::<duckdb_string_t>(len);
            let names = name_vec.as_slice_with_len::<duckdb_string_t>(len);
            let out_vec = output.flat_vector();

            for i in 0..len {
                let obj_type = DuckString::new(&mut { types[i] }).as_str().to_string();
                let name = DuckString::new(&mut { names[i] }).as_str().to_string();

                if !obj_type.eq_ignore_ascii_case("SEMANTIC_VIEW") {
                    return Err(format!(
                        "GET_DDL: unsupported object type '{}'. Only 'SEMANTIC_VIEW' is supported.",
                        obj_type
                    )
                    .into());
                }

                let json = state
                    .lookup(&name)
                    .map_err(Box::<dyn std::error::Error>::from)?
                    .ok_or_else(|| format!("semantic view '{}' does not exist", name))?;
                let def: SemanticViewDefinition = serde_json::from_str(&json)?;
                let ddl =
                    render_create_ddl(&name, &def).map_err(|e| -> Box<dyn std::error::Error> {
                        format!("GET_DDL error: {e}").into()
                    })?;
                out_vec.insert(i, ddl.as_str());
            }
            Ok(())
        }))
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // object_type
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // name
            ],
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // return: DDL string
        )]
    }
}
