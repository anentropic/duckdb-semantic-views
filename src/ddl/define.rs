use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vscalar::{ScalarFunctionSignature, VScalar},
    vtab::arrow::WritableVector,
};
use libduckdb_sys as ffi;
use std::ffi::CString;

use crate::catalog::{catalog_insert, catalog_upsert, CatalogState};
use crate::ddl::parse_args::parse_define_args;
use crate::expand::{expand, QueryRequest};

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
    /// When true, silently succeeds (no-op) if the view already exists.
    /// Mutually exclusive with `or_replace` (or_replace takes precedence if both set).
    pub if_not_exists: bool,
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
            let mut parsed =
                parse_define_args(input, i).map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // DDL-time type inference (file-backed databases only).
            // Runs LIMIT 0 on the expanded SQL via the persist connection.
            // The persist connection has its own context — safe from invoke's execution lock.
            if let Some(conn) = state.persist_conn {
                let req_all = QueryRequest {
                    dimensions: parsed
                        .def
                        .dimensions
                        .iter()
                        .map(|d| d.name.clone())
                        .collect(),
                    metrics: parsed.def.metrics.iter().map(|m| m.name.clone()).collect(),
                    granularity_overrides: std::collections::HashMap::new(),
                };
                if let Ok(expanded_for_inference) = expand(&parsed.name, &parsed.def, &req_all) {
                    let limit0_sql = format!("{expanded_for_inference} LIMIT 0");
                    if let Some((names, types)) =
                        crate::query::table_function::try_infer_schema(conn, &limit0_sql)
                    {
                        // Store BOTH names and types so bind() can look up by name (not position).
                        // Essential because query-time requests may only include a subset of
                        // all dims+metrics, so positional indexing would not match DDL column order.
                        parsed.def.column_type_names = names;
                        parsed.def.column_types_inferred = types
                            .iter()
                            .map(|t| crate::query::table_function::normalize_type_id(*t as u32))
                            .collect();
                    }
                }
            }
            // In-memory: both vecs stay empty — VARCHAR fallback in bind().

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
            } else if state.if_not_exists {
                match catalog_insert(&state.catalog, &parsed.name, &json) {
                    Ok(()) => {}
                    Err(e) if e.to_string().contains("already exists") => {
                        // Silently succeed — view exists, no replacement needed.
                    }
                    Err(e) => return Err(e),
                }
            } else {
                catalog_insert(&state.catalog, &parsed.name, &json)?;
            }

            out.insert(i, parsed.name.as_str());
        }
        Ok(())
    }
}
