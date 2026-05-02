use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;
use std::ffi::CStr;

/// Shared state for the legacy `create_semantic_view_from_json` table-function
/// fallback (still reachable for direct user calls; the v0.8.0 native CREATE
/// path runs via `parser_override` and writes directly on the caller's
/// connection).
///
/// `persist_conn` is `Some` for file-backed databases — it is a separate
/// `duckdb_connection` created at init time and used to execute INSERT into
/// `semantic_layer._definitions` from within bind (avoids deadlock with
/// the main connection's execution lock). For in-memory databases via the
/// legacy fallback there is no separate write connection: writes go through
/// `catalog_conn` instead.
#[derive(Clone)]
pub struct DefineState {
    pub persist_conn: Option<ffi::duckdb_connection>,
    /// Connection for catalog queries (e.g., `duckdb_constraints()` lookups)
    /// and for legacy in-memory writes when `persist_conn` is `None`.
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

/// Whether a semantic view already exists in `semantic_layer._definitions`.
fn view_exists(conn: ffi::duckdb_connection, name: &str) -> Result<bool, String> {
    crate::catalog::CatalogReader::new(conn).exists(name)
}

/// Persist a view definition to `semantic_layer._definitions`.
///
/// `or_replace=true` → `INSERT OR REPLACE` (overwrite existing row).
/// `or_replace=false` → plain `INSERT`; caller is expected to have already
/// checked for existing rows so that the failure mode of a duplicate row is
/// surfaced as the friendly "already exists" error rather than a raw
/// constraint violation.
fn persist_define(
    conn: ffi::duckdb_connection,
    name: &str,
    json: &str,
    or_replace: bool,
) -> Result<(), String> {
    let sql = if or_replace {
        "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES ($1, $2)"
    } else {
        "INSERT INTO semantic_layer._definitions (name, definition) VALUES ($1, $2)"
    };
    unsafe {
        super::persist::execute_parameterized(conn, sql, &[name, json])
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
fn resolve_pk_from_catalog(
    conn: ffi::duckdb_connection,
    def: &mut crate::model::SemanticViewDefinition,
) {
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

/// Run all CREATE-time enrichment + validation against `conn` (a connection
/// with read access to the user catalog and committed user tables) and return
/// the enriched JSON string ready for storage in `_definitions`.
///
/// Steps performed in order:
/// 1. Resolve PRIMARY KEY columns from `duckdb_constraints()` for tables
///    declared without an explicit PK.
/// 2. Re-run cardinality inference now that catalog PKs are populated.
/// 3. Validate that no join still has FK columns without resolved ref columns.
/// 4. Run graph / facts / derived-metric / using-relationship validations.
/// 5. Capture metadata (`created_on`, `database_name`, `schema_name`) by
///    querying `now()` / `current_database()` / `current_schema()`.
/// 6. Run `LIMIT 0` against the expanded query to populate
///    `column_type_names` / `column_types_inferred` and back-fill
///    dimension/metric `output_type` fields.
/// 7. Run `typeof(expr)` against the source table for each fact to populate
///    fact `output_type` (best-effort).
///
/// Used by both the legacy `DefineFromJsonVTab::bind` path (CREATE FROM YAML
/// FILE still routes through it) and the v0.8.0 transactional CREATE path
/// emitted by `parser_override`.
pub fn enrich_definition_for_create(
    name: &str,
    mut def: crate::model::SemanticViewDefinition,
    conn: ffi::duckdb_connection,
    infer_types: bool,
) -> Result<String, String> {
    // 1. Catalog PK resolution.
    resolve_pk_from_catalog(conn, &mut def);

    // 2. Re-run cardinality inference now that catalog PKs are resolved.
    crate::parse::infer_cardinality(&def.tables, &mut def.joins).map_err(|e| e.message)?;

    // 3. Catch joins that still have FK columns but no resolved ref_columns.
    for join in &def.joins {
        if !join.fk_columns.is_empty() && join.ref_columns.is_empty() {
            let to_alias_lower = join.table.to_ascii_lowercase();
            let target = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower);
            if let Some(t) = target {
                return Err(format!(
                    "Table '{}' has no PRIMARY KEY. \
                     Specify referenced columns explicitly: REFERENCES {}(col).",
                    t.alias, t.alias
                ));
            }
            return Err("This semantic view was created with an older version. \
                 Please recreate it with the new DDL syntax."
                .to_string());
        }
    }

    // 4. Graph validations.
    crate::graph::validate_graph(&def)?;
    crate::graph::validate_facts(&def)?;
    crate::graph::validate_derived_metrics(&def)?;
    crate::graph::validate_using_relationships(&def)?;

    // 5. Capture metadata: timestamp + database_name + schema_name.
    let metadata_sql = "SELECT strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), \
                        current_database(), current_schema()";
    if let Ok(mut result) =
        unsafe { crate::query::table_function::execute_sql_raw(conn, metadata_sql) }
    {
        unsafe {
            let ts_ptr = ffi::duckdb_value_varchar(&mut result, 0, 0);
            if !ts_ptr.is_null() {
                def.created_on = Some(CStr::from_ptr(ts_ptr).to_string_lossy().into_owned());
                ffi::duckdb_free(ts_ptr as *mut std::ffi::c_void);
            }
            let db_ptr = ffi::duckdb_value_varchar(&mut result, 1, 0);
            if !db_ptr.is_null() {
                def.database_name = Some(CStr::from_ptr(db_ptr).to_string_lossy().into_owned());
                ffi::duckdb_free(db_ptr as *mut std::ffi::c_void);
            }
            let schema_ptr = ffi::duckdb_value_varchar(&mut result, 2, 0);
            if !schema_ptr.is_null() {
                def.schema_name = Some(CStr::from_ptr(schema_ptr).to_string_lossy().into_owned());
                ffi::duckdb_free(schema_ptr as *mut std::ffi::c_void);
            }
            ffi::duckdb_destroy_result(&mut result);
        }
    }

    // 6. DDL-time type inference via LIMIT 0 probe (file-backed only —
    // matches v0.7.1 design; in-memory DBs skip inference).
    if infer_types {
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
        if let Ok(expanded_for_inference) = crate::expand::expand(name, &def, &req_all) {
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

        if !def.column_type_names.is_empty() {
            let type_map: std::collections::HashMap<String, u32> = def
                .column_type_names
                .iter()
                .zip(def.column_types_inferred.iter())
                .map(|(name, &tid)| (name.to_ascii_lowercase(), tid))
                .collect();

            for dim in &mut def.dimensions {
                if dim.output_type.is_none() {
                    if let Some(&tid) = type_map.get(&dim.name.to_ascii_lowercase()) {
                        dim.output_type =
                            crate::query::table_function::type_id_to_display_name(tid)
                                .map(|s| s.to_string());
                    }
                }
            }
            for met in &mut def.metrics {
                if met.output_type.is_none() {
                    if let Some(&tid) = type_map.get(&met.name.to_ascii_lowercase()) {
                        met.output_type =
                            crate::query::table_function::type_id_to_display_name(tid)
                                .map(|s| s.to_string());
                    }
                }
            }
        }
    }

    // 7. Fact type inference via typeof(expr).
    if !def.facts.is_empty() {
        let alias_to_table: std::collections::HashMap<String, String> = def
            .tables
            .iter()
            .map(|t| (t.alias.to_ascii_lowercase(), t.table.clone()))
            .collect();
        let fallback_table = def.base_table().to_string();

        for fact in &mut def.facts {
            let alias = fact.source_table.as_deref().unwrap_or("");
            let table_name = fact
                .source_table
                .as_ref()
                .and_then(|a| alias_to_table.get(&a.to_ascii_lowercase()).cloned())
                .unwrap_or_else(|| fallback_table.clone());

            let from_clause = if alias.is_empty() {
                format!("\"{}\"", table_name.replace('"', "\"\""))
            } else {
                format!("\"{}\" AS {}", table_name.replace('"', "\"\""), alias)
            };
            let sql = format!("SELECT typeof({}) FROM {} LIMIT 1", fact.expr, from_clause);
            if let Ok(mut result) =
                unsafe { crate::query::table_function::execute_sql_raw(conn, &sql) }
            {
                let row_count = unsafe { ffi::duckdb_row_count(&mut result) };
                if row_count > 0 {
                    let val_ptr = unsafe { ffi::duckdb_value_varchar(&mut result, 0, 0) };
                    if !val_ptr.is_null() {
                        let type_name =
                            unsafe { CStr::from_ptr(val_ptr).to_string_lossy().into_owned() };
                        unsafe { ffi::duckdb_free(val_ptr as *mut std::ffi::c_void) };
                        fact.output_type = Some(type_name);
                    }
                }
                unsafe { ffi::duckdb_destroy_result(&mut result) };
            }
        }
    }

    serde_json::to_string(&def).map_err(|e| e.to_string())
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
            let def = crate::model::SemanticViewDefinition::from_json(&name, &json)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            // Access the DefineState from extra_info (moved up for catalog PK resolution).
            let state_ptr = bind.get_extra_info::<DefineState>();
            let state = unsafe { &*state_ptr };

            // Run shared enrichment (PK resolution → cardinality → graph
            // validations → metadata capture → type inference → fact typing).
            // `persist_conn.is_some()` mirrors the v0.7.1 file-backed gate for
            // LIMIT 0 type inference; in-memory DBs skip it.
            let infer_types = state.persist_conn.is_some();
            let json_out =
                enrich_definition_for_create(&name, def, state.catalog_conn, infer_types)
                    .map_err(Box::<dyn std::error::Error>::from)?;

            // Persist to `_definitions`. File-backed DBs use the dedicated
            // persist_conn; in-memory falls back to catalog_conn.
            let write_conn = state.persist_conn.unwrap_or(state.catalog_conn);

            if state.or_replace {
                persist_define(write_conn, &name, &json_out, true)
                    .map_err(Box::<dyn std::error::Error>::from)?;
            } else {
                let already =
                    view_exists(write_conn, &name).map_err(Box::<dyn std::error::Error>::from)?;
                if already {
                    if state.if_not_exists {
                        // Silently succeed -- view exists, no replacement needed.
                    } else {
                        return Err(format!(
                            "semantic view '{name}' already exists; use CREATE OR REPLACE SEMANTIC VIEW to overwrite"
                        )
                        .into());
                    }
                } else {
                    persist_define(write_conn, &name, &json_out, false)
                        .map_err(Box::<dyn std::error::Error>::from)?;
                }
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
