//! CREATE-time enrichment shared by the parser_override CREATE path.
//!
//! Pre-v0.8.0 this module also hosted `DefineFromJsonVTab` — a table function
//! that the legacy parse_function fallback rewrote DDL into. v0.8.0's full
//! unification deleted that path; `parser_override` now emits native INSERT
//! against `semantic_layer._definitions` directly. Only the enrichment +
//! PK-resolution helpers remain — both called by `crate::parse::rewrite_create`
//! and `crate::parse::rewrite_yaml_file_create`.

use libduckdb_sys as ffi;
use std::ffi::CStr;

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
/// Called by both `parse::rewrite_create` (inline AS-body) and
/// `parse::rewrite_yaml_file_create` (FROM YAML FILE) under parser_override.
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
