use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;
use std::ffi::CStr;

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
    /// Connection for catalog queries (e.g., `duckdb_constraints()` lookups).
    /// Always set -- created at init time from the database handle.
    /// Distinct from `persist_conn` (which is file-backed only) and the main
    /// connection (which holds execution locks during bind).
    pub catalog_conn: ffi::duckdb_connection,
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
    unsafe {
        super::persist::execute_parameterized(
            conn,
            "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES ($1, $2)",
            &[name, json],
        )
        .map_err(|e| format!("failed to persist semantic view '{name}' to catalog table: {e}"))
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

/// Look up PRIMARY KEY constraints from the DuckDB catalog for tables
/// that were declared without an explicit PRIMARY KEY in the TABLES clause.
///
/// For each table with empty `pk_columns`, queries `duckdb_constraints()`
/// and fills in the PK columns if found. Tables that have no catalog PK
/// are left with empty `pk_columns` -- downstream validation will catch them.
fn resolve_pk_from_catalog(state: &DefineState, def: &mut crate::model::SemanticViewDefinition) {
    let conn = state.catalog_conn;
    if conn.is_null() {
        return; // No catalog connection available -- skip lookup.
    }
    for table in &mut def.tables {
        if !table.pk_columns.is_empty() {
            continue; // Explicit PK declared in DDL -- skip catalog lookup.
        }

        // Use UNNEST to flatten the VARCHAR[] into individual rows.
        // This avoids needing to parse the LIST type from duckdb_value_varchar
        // (duckdb_value_varchar returns NULL for LIST columns in DuckDB 1.5.0).
        let sql = format!(
            "SELECT UNNEST(constraint_column_names) AS col FROM duckdb_constraints() \
             WHERE database_name = current_database() \
             AND schema_name = 'main' \
             AND table_name = '{}' \
             AND constraint_type = 'PRIMARY KEY'",
            table.table.replace('\'', "''")
        );

        let result = unsafe { crate::query::table_function::execute_sql_raw(conn, &sql) };
        let mut db_result = match result {
            Ok(r) => r,
            Err(_) => continue, // Query failed -- leave pk_columns empty.
        };

        let row_count = unsafe { ffi::duckdb_row_count(&mut db_result) };
        if row_count == 0 {
            unsafe { ffi::duckdb_destroy_result(&mut db_result) };
            continue; // No PK constraint found.
        }

        // Read each unnested column name as a VARCHAR.
        let mut pk_cols = Vec::new();
        for row_idx in 0..row_count {
            let val_ptr = unsafe { ffi::duckdb_value_varchar(&mut db_result, 0, row_idx as u64) };
            if !val_ptr.is_null() {
                let col_name = unsafe { CStr::from_ptr(val_ptr) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { ffi::duckdb_free(val_ptr as *mut std::ffi::c_void) };
                if !col_name.is_empty() {
                    pk_cols.push(col_name);
                }
            }
        }

        unsafe { ffi::duckdb_destroy_result(&mut db_result) };

        if !pk_cols.is_empty() {
            table.pk_columns = pk_cols;
        }
    }
}

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
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            // Declare output schema: single VARCHAR column with the view name.
            bind.add_result_column("view_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));

            let name = bind.get_parameter(0).to_string();
            let json = bind.get_parameter(1).to_string();

            // Deserialize the JSON into a SemanticViewDefinition.
            let mut def = crate::model::SemanticViewDefinition::from_json(&name, &json)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // Access the DefineState from extra_info (moved up for catalog PK resolution).
            let state_ptr = bind.get_extra_info::<DefineState>();
            let state = unsafe { &*state_ptr };

            // Resolve PKs from catalog for tables without explicit PRIMARY KEY.
            // Works for both file-backed and in-memory databases.
            resolve_pk_from_catalog(state, &mut def);

            // Re-run cardinality inference now that catalog PKs are resolved.
            // Any joins with still-empty ref_columns (no catalog PK found) will
            // be caught by the Phase 33 guard or validate_fk_references below.
            crate::parse::infer_cardinality(&def.tables, &mut def.joins)
                .map_err(|e| Box::<dyn std::error::Error>::from(e.message))?;

            // After catalog PK resolution and re-inference, check for joins that
            // still have FK columns but no resolved ref_columns.
            for join in &def.joins {
                if !join.fk_columns.is_empty() && join.ref_columns.is_empty() {
                    // Find the target table to produce the right error message.
                    let to_alias_lower = join.table.to_ascii_lowercase();
                    let target = def
                        .tables
                        .iter()
                        .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower);
                    if let Some(t) = target {
                        // Target table exists but has no PK (neither DDL nor catalog).
                        return Err(Box::<dyn std::error::Error>::from(format!(
                            "Table '{}' has no PRIMARY KEY. \
                         Specify referenced columns explicitly: REFERENCES {}(col).",
                            t.alias, t.alias
                        )));
                    }
                    // Target not found -- old-format JSON or graph issue.
                    return Err(Box::<dyn std::error::Error>::from(
                        "This semantic view was created with an older version. \
                     Please recreate it with the new DDL syntax.",
                    ));
                }
            }

            // Validate relationship graph before persisting (Phase 26).
            // Catches cycles, diamonds, self-references, orphans, FK reference validation.
            crate::graph::validate_graph(&def)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // Validate facts: source table reachability, cycles, unknown refs (Phase 29).
            crate::graph::validate_facts(&def)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // Validate derived metrics: cycles, unknown refs, aggregate prohibition (Phase 30).
            crate::graph::validate_derived_metrics(&def)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // Validate USING relationship references on metrics (Phase 32).
            crate::graph::validate_using_relationships(&def)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // Capture metadata: timestamp, database_name, schema_name.
            // Uses catalog_conn which is always available (file-backed AND in-memory).
            let metadata_sql = "SELECT strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), \
                            current_database(), current_schema()";
            let metadata_result = unsafe {
                crate::query::table_function::execute_sql_raw(state.catalog_conn, metadata_sql)
            };
            if let Ok(mut result) = metadata_result {
                unsafe {
                    let ts_ptr = ffi::duckdb_value_varchar(&mut result, 0, 0);
                    if !ts_ptr.is_null() {
                        def.created_on =
                            Some(CStr::from_ptr(ts_ptr).to_string_lossy().into_owned());
                        ffi::duckdb_free(ts_ptr as *mut std::ffi::c_void);
                    }
                    let db_ptr = ffi::duckdb_value_varchar(&mut result, 1, 0);
                    if !db_ptr.is_null() {
                        def.database_name =
                            Some(CStr::from_ptr(db_ptr).to_string_lossy().into_owned());
                        ffi::duckdb_free(db_ptr as *mut std::ffi::c_void);
                    }
                    let schema_ptr = ffi::duckdb_value_varchar(&mut result, 2, 0);
                    if !schema_ptr.is_null() {
                        def.schema_name =
                            Some(CStr::from_ptr(schema_ptr).to_string_lossy().into_owned());
                        ffi::duckdb_free(schema_ptr as *mut std::ffi::c_void);
                    }
                    ffi::duckdb_destroy_result(&mut result);
                }
            }

            // DDL-time type inference (file-backed databases only).
            // Runs LIMIT 0 on the expanded SQL via the persist connection.
            if let Some(conn) = state.persist_conn {
                let req_all = crate::expand::QueryRequest {
                    dimensions: def
                        .dimensions
                        .iter()
                        .map(|d| crate::expand::DimensionName::new(d.name.clone()))
                        .collect(),
                    metrics: def
                        .metrics
                        .iter()
                        .map(|m| crate::expand::MetricName::new(m.name.clone()))
                        .collect(),
                    facts: vec![],
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

            // Fact type inference: use typeof(expr) to determine fact output types.
            // Best-effort -- if typeof() fails, output_type stays None (graceful degradation).
            if !def.facts.is_empty() {
                let alias_to_table: std::collections::HashMap<String, String> = def
                    .tables
                    .iter()
                    .map(|t| (t.alias.to_ascii_lowercase(), t.table.clone()))
                    .collect();

                let type_conn = state.persist_conn.unwrap_or(state.catalog_conn);
                let fallback_table = def.base_table().to_string();

                for fact in &mut def.facts {
                    let alias = fact.source_table.as_deref().unwrap_or("");
                    let table_name = fact
                        .source_table
                        .as_ref()
                        .and_then(|a| alias_to_table.get(&a.to_ascii_lowercase()).cloned())
                        .unwrap_or_else(|| fallback_table.clone());

                    // Include AS alias so expressions like `li.price` resolve correctly.
                    let from_clause = if alias.is_empty() {
                        format!("\"{}\"", table_name.replace('"', "\"\""))
                    } else {
                        format!("\"{}\" AS {}", table_name.replace('"', "\"\""), alias)
                    };
                    let sql = format!("SELECT typeof({}) FROM {} LIMIT 1", fact.expr, from_clause);
                    if let Ok(mut result) =
                        unsafe { crate::query::table_function::execute_sql_raw(type_conn, &sql) }
                    {
                        let row_count = unsafe { ffi::duckdb_row_count(&mut result) };
                        if row_count > 0 {
                            let val_ptr = unsafe { ffi::duckdb_value_varchar(&mut result, 0, 0) };
                            if !val_ptr.is_null() {
                                let type_name = unsafe {
                                    CStr::from_ptr(val_ptr).to_string_lossy().into_owned()
                                };
                                unsafe { ffi::duckdb_free(val_ptr as *mut std::ffi::c_void) };
                                fact.output_type = Some(type_name);
                            }
                        }
                        unsafe { ffi::duckdb_destroy_result(&mut result) };
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
        }))
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
