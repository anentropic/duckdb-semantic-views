use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;
use std::ffi::CString;

use crate::catalog::{catalog_insert, catalog_upsert, CatalogState};

/// Shared state for `create_semantic_view` and `create_or_replace_semantic_view`.
///
/// `persist_conn` is `Some` for file-backed databases -- it is a separate
/// `duckdb_connection` created at init time and used to execute INSERT into
/// `semantic_layer._definitions` from within bind (avoids deadlock with
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

/// Bind-time data for the DDL define table function.
///
/// Holds the view name (returned as the single result row).
pub struct DefineBindData {
    name: String,
}

// SAFETY: String is Send + Sync.
unsafe impl Send for DefineBindData {}
unsafe impl Sync for DefineBindData {}

/// Init data for the DDL define table function.
pub struct DefineInitData {
    done: AtomicBool,
}

// SAFETY: AtomicBool is Send + Sync.
unsafe impl Send for DefineInitData {}
unsafe impl Sync for DefineInitData {}

/// `create_semantic_view_from_json(name, json)` table function.
///
/// Accepts a pre-parsed JSON-serialized `SemanticViewDefinition` (produced by
/// the AS-body DDL rewriter in parse.rs). Deserializes and stores the definition
/// using the shared DefineState/persist_define logic.
///
/// This is the execution target for `CREATE SEMANTIC VIEW name AS TABLES (...) ...`.
/// Three variants are registered:
/// - `create_semantic_view_from_json` (or_replace=false, if_not_exists=false)
/// - `create_or_replace_semantic_view_from_json` (or_replace=true)
/// - `create_semantic_view_if_not_exists_from_json` (if_not_exists=true)
pub struct DefineFromJsonVTab;

impl VTab for DefineFromJsonVTab {
    type BindData = DefineBindData;
    type InitData = DefineInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Declare output schema: single VARCHAR column with the view name.
        bind.add_result_column("view_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));

        let name = bind.get_parameter(0).to_string();
        let json = bind.get_parameter(1).to_string();

        // Deserialize the JSON into a SemanticViewDefinition.
        let mut def = crate::model::SemanticViewDefinition::from_json(&name, &json)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        // Phase 33: Reject old-format JSON (pre-v0.5.4) that has FK columns but no ref_columns.
        for join in &def.joins {
            if !join.fk_columns.is_empty() && join.ref_columns.is_empty() {
                return Err(Box::<dyn std::error::Error>::from(
                    "This semantic view was created with an older version. \
                     Please recreate it with the new DDL syntax.",
                ));
            }
        }

        // Validate relationship graph before persisting (Phase 26).
        // Catches cycles, diamonds, self-references, orphans, FK reference validation.
        crate::graph::validate_graph(&def).map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        // Validate facts: source table reachability, cycles, unknown refs (Phase 29).
        crate::graph::validate_facts(&def).map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        // Validate derived metrics: cycles, unknown refs, aggregate prohibition (Phase 30).
        crate::graph::validate_derived_metrics(&def)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        // Validate USING relationship references on metrics (Phase 32).
        crate::graph::validate_using_relationships(&def)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        // Access the DefineState from extra_info.
        let state_ptr = bind.get_extra_info::<DefineState>();
        let state = unsafe { &*state_ptr };

        // DDL-time type inference (file-backed databases only).
        // Runs LIMIT 0 on the expanded SQL via the persist connection.
        if let Some(conn) = state.persist_conn {
            let req_all = crate::expand::QueryRequest {
                dimensions: def.dimensions.iter().map(|d| d.name.clone()).collect(),
                metrics: def.metrics.iter().map(|m| m.name.clone()).collect(),
            };
            if let Ok(expanded_for_inference) = crate::expand::expand(&name, &def, &req_all) {
                let limit0_sql = format!("{expanded_for_inference} LIMIT 0");
                if let Some((names, types)) =
                    unsafe { crate::query::table_function::try_infer_schema(conn, &limit0_sql) }
                {
                    def.column_type_names = names;
                    def.column_types_inferred = types
                        .iter()
                        .map(|t| crate::query::table_function::normalize_type_id(*t as u32))
                        .collect();
                }
            }
        }

        let json_out = serde_json::to_string(&def)
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;

        // Persist to DuckDB table first (file-backed databases only).
        if let Some(conn) = state.persist_conn {
            persist_define(conn, &name, &json_out)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        }

        // Update in-memory catalog.
        if state.or_replace {
            catalog_upsert(&state.catalog, &name, &json_out)?;
        } else if state.if_not_exists {
            match catalog_insert(&state.catalog, &name, &json_out) {
                Ok(()) => {}
                Err(e) if e.to_string().contains("already exists") => {
                    // Silently succeed -- view exists, no replacement needed.
                }
                Err(e) => return Err(e),
            }
        } else {
            catalog_insert(&state.catalog, &name, &json_out)?;
        }

        Ok(DefineBindData { name })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(DefineInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        if init_data.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }
        let bind_data = func.get_bind_data();
        let name_vec = output.flat_vector(0);
        name_vec.insert(0, bind_data.name.as_str());
        output.set_len(1);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        // Both name and json are positional VARCHAR parameters.
        Some(vec![
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // name
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // json
        ])
    }

    fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
        None // No named parameters -- both are positional
    }
}
