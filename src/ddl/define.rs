use std::ffi::CString;

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys::duckdb_string_t;

use crate::catalog::{catalog_insert, CatalogState};

/// Shared state for `define_semantic_view`.
///
/// `persist_conn` is `Some` for file-backed databases — it is a separate
/// `duckdb_connection` created at init time and used to execute INSERT into
/// `semantic_layer._definitions` from within invoke (avoids deadlock with
/// the main connection's execution lock). For in-memory databases, `persist_conn`
/// is `None` and the `HashMap` is the sole source of truth for the session.
#[derive(Clone)]
pub struct DefineState {
    pub catalog: CatalogState,
    pub persist_conn: Option<libduckdb_sys::duckdb_connection>,
}

// SAFETY: duckdb_connection is an opaque pointer managed by DuckDB.
// DuckDB handles concurrent access internally.
unsafe impl Send for DefineState {}
unsafe impl Sync for DefineState {}

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

            // 1. Persist to DuckDB table FIRST (file-backed databases only).
            //    Uses a separate connection — no deadlock with invoke's execution lock.
            //    Write-first ordering: if this fails, HashMap is unchanged (PERSIST-02).
            #[cfg(feature = "extension")]
            if let Some(conn) = state.persist_conn {
                let c_name = CString::new(name.as_str())
                    .map_err(|_| format!("semantic view name '{}' contains null byte", name))?;
                let c_json = CString::new(json.as_str())
                    .map_err(|_| "definition JSON contains null byte".to_string())?;
                let rc = unsafe {
                    crate::shim::ffi::semantic_views_pragma_define(
                        conn,
                        c_name.as_ptr(),
                        c_json.as_ptr(),
                    )
                };
                if rc != 0 {
                    return Err(format!(
                        "failed to persist semantic view '{}': table write failed",
                        name
                    )
                    .into());
                }
            }

            // 2. Update in-memory catalog AFTER successful persist.
            //    catalog_insert validates JSON and checks for duplicates.
            catalog_insert(&state.catalog, &name, &json)?;

            let msg = format!("Semantic view '{name}' registered successfully");
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}
