use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys as ffi;
use std::ffi::CString;

use crate::catalog::{catalog_insert, catalog_upsert, CatalogState};
use crate::ddl::parse_args::parse_define_args;

/// Shared state for `define_semantic_view` and `define_or_replace_semantic_view`.
///
/// `persist_conn` is `Some` for file-backed databases — it is a separate
/// `duckdb_connection` created at init time and used to execute INSERT into
/// `semantic_layer._definitions` from within invoke (avoids deadlock with
/// the main connection's execution lock). For in-memory databases,
/// `persist_conn` is `None` and the HashMap is the sole source of truth.
#[derive(Clone)]
pub struct DefineState {
    pub catalog: CatalogState,
    pub persist_conn: Option<ffi::duckdb_connection>,
    /// When true, uses INSERT OR REPLACE (upsert); when false, errors on duplicate.
    pub or_replace: bool,
}

// SAFETY: duckdb_connection is an opaque pointer managed by DuckDB.
// DuckDB handles concurrent access internally per connection.
unsafe impl Send for DefineState {}
unsafe impl Sync for DefineState {}

/// Persist a view definition to `semantic_layer._definitions` using the separate
/// persist_conn. This avoids deadlocking with the main connection's execution lock
/// (context_lock is non-reentrant; a second duckdb_connection has its own context).
///
/// Uses the Rust ffi::duckdb_query which goes through function pointers (loadable-
/// extension compatible) rather than direct symbol references.
///
/// Returns Ok(()) on success, Err on failure.
fn persist_define(conn: ffi::duckdb_connection, name: &str, json: &str) -> Result<(), String> {
    // Escape single quotes to prevent SQL injection / breakage
    let safe_name = name.replace('\'', "''");
    let safe_json = json.replace('\'', "''");
    let sql = format!(
        "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES ('{}', '{}')",
        safe_name, safe_json
    );
    let c_sql = CString::new(sql).map_err(|_| "SQL contains null byte".to_string())?;
    unsafe {
        let mut result: ffi::duckdb_result = std::mem::zeroed();
        let state = ffi::duckdb_query(conn, c_sql.as_ptr(), &mut result);
        let success = state == ffi::DuckDBSuccess;
        ffi::duckdb_destroy_result(&mut result);
        if success {
            Ok(())
        } else {
            Err(format!(
                "failed to persist semantic view '{name}' to catalog table"
            ))
        }
    }
}

/// `define_semantic_view(name, tables, relationships, dimensions, time_dimensions, metrics)`
/// scalar function.
///
/// Inserts a new semantic view definition. Errors if the view already exists.
/// Use `define_or_replace_semantic_view` to overwrite an existing view.
pub struct DefineSemanticView;

impl VScalar for DefineSemanticView {
    type State = DefineState;

    fn signatures() -> Vec<ScalarFunctionSignature> {
        let varchar = || LogicalTypeHandle::from(LogicalTypeId::Varchar);

        let tables_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("alias", varchar()),
            ("table", varchar()),
        ]));
        let col_pair_struct =
            LogicalTypeHandle::struct_type(&[("from", varchar()), ("to", varchar())]);
        let join_columns_type = LogicalTypeHandle::list(&col_pair_struct);
        let relationships_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("from_table", varchar()),
            ("to_table", varchar()),
            ("join_columns", join_columns_type),
        ]));
        let dimensions_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("name", varchar()),
            ("expr", varchar()),
            ("source_table", varchar()),
        ]));
        let time_dimensions_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("name", varchar()),
            ("expr", varchar()),
            ("granularity", varchar()),
        ]));
        let metrics_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("name", varchar()),
            ("expr", varchar()),
            ("source_table", varchar()),
        ]));

        vec![ScalarFunctionSignature::exact(
            vec![
                varchar(),            // 0: view name
                tables_type,          // 1: tables LIST(STRUCT(alias, table))
                relationships_type,   // 2: relationships LIST(STRUCT(..., join_columns LIST(...)))
                dimensions_type,      // 3: dimensions LIST(STRUCT(name, expr, source_table))
                time_dimensions_type, // 4: time_dimensions LIST(STRUCT(name, expr, granularity))
                metrics_type,         // 5: metrics LIST(STRUCT(name, expr, source_table))
            ],
            varchar(), // returns view name on success
        )]
    }

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let out = output.flat_vector();
        for i in 0..input.len() {
            let parsed =
                parse_define_args(input, i).map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            let json = serde_json::to_string(&parsed.def)
                .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;

            // 1. Persist to DuckDB table FIRST (file-backed databases only).
            //    Uses a separate connection — no deadlock with invoke's execution lock.
            //    Write-first ordering: if persist fails, HashMap is unchanged.
            if let Some(conn) = state.persist_conn {
                persist_define(conn, &parsed.name, &json)
                    .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            }

            // 2. Update in-memory catalog.
            if state.or_replace {
                catalog_upsert(&state.catalog, &parsed.name, &json)?;
            } else {
                catalog_insert(&state.catalog, &parsed.name, &json)?;
            }

            out.insert(i, parsed.name.as_str());
        }
        Ok(())
    }
}
