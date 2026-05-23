//! CREATE-time enrichment shared by the parser_override CREATE path.
//!
//! Pre-v0.8.0 this module also hosted `DefineFromJsonVTab` — a table function
//! that the legacy parse_function fallback rewrote DDL into. v0.8.0's full
//! unification deleted that path; `parser_override` now emits native INSERT
//! against `semantic_layer._definitions` directly. Only the enrichment helper
//! remains — called by `crate::parse::rewrite_create` and
//! `crate::parse::rewrite_yaml_file_create`.
//!
//! Phase 65 (v0.10.0): removed `resolve_pk_from_catalog` (D-05). Auto-fallback
//! to `duckdb_constraints()` for tables without explicit PRIMARY KEY in the
//! TABLES clause is gone. Snowflake-aligned: PKs in semantic views are
//! LOGICAL user assertions, not physical-catalog imports. Step 3 below now
//! returns the D-06 hard error template when a FK references a table without
//! a PRIMARY KEY (or UNIQUE) declared in the TABLES clause.

use libduckdb_sys as ffi;
use std::ffi::CStr;

/// Run all CREATE-time enrichment + validation against `conn` (a connection
/// with read access to the user catalog and committed user tables) and return
/// the enriched JSON string ready for storage in `_definitions`.
///
/// Steps performed in order:
/// 1. Re-run cardinality inference (catches FK→PK mismatches once PKs are
///    declared explicitly in the TABLES clause).
/// 2. Catch joins that still have FK columns but no resolved ref columns,
///    and surface the D-06 hard error pointing the user at the missing
///    PRIMARY KEY / UNIQUE declaration in the TABLES clause.
/// 3. Run graph / facts / derived-metric / using-relationship validations.
/// 4. Capture metadata (`created_on`, `database_name`, `schema_name`) by
///    querying `now()` / `current_database()` / `current_schema()`.
/// 5. Run `LIMIT 0` against the expanded query to populate
///    `column_type_names` / `column_types_inferred` and back-fill
///    dimension/metric `output_type` fields.
/// 6. Run `typeof(expr)` against the source table for each fact to populate
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
    // 1. Re-run cardinality inference. Phase 65: no longer preceded by
    //    `resolve_pk_from_catalog` (D-05). Tables without explicit PRIMARY
    //    KEY in the TABLES clause that are FK-referenced by another table
    //    surface as the D-06 hard error in step 2.
    crate::parse::infer_cardinality(&def.tables, &mut def.joins).map_err(|e| e.message)?;

    // 2. Catch joins that reference a target without a PRIMARY KEY (or
    //    UNIQUE constraint) declared in the TABLES clause.
    //    Phase 65 (D-06): hard-error path. v0.9.0's resolve_pk_from_catalog
    //    auto-fallback to duckdb_constraints() is gone; the error is
    //    actionable and tells the user exactly what to add.
    //
    //    Two sub-cases:
    //      (a) `REFERENCES target` (no col list) — `infer_cardinality` left
    //          `ref_columns` empty because target has no `pk_columns`.
    //      (b) `REFERENCES target(cols)` — `ref_columns` was set explicitly
    //          but target has no `pk_columns` and no UNIQUE constraint
    //          matching `ref_columns`. (Without the D-06 wrapping this
    //          would surface as the more generic CARD-03 "FK ... does not
    //          match any PRIMARY KEY or UNIQUE constraint" error in
    //          `validate_fk_references`. The D-06 message is more
    //          actionable because it names the fix verbatim.)
    for join in &def.joins {
        if join.fk_columns.is_empty() {
            continue;
        }
        let to_alias_lower = join.table.to_ascii_lowercase();
        let fk_source = join.from_alias.as_str();
        let target = def
            .tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower);
        let Some(t) = target else {
            // Target alias unresolved — let graph validation surface that
            // with its dedicated message (more specific than D-06).
            continue;
        };

        let target_has_pk = !t.pk_columns.is_empty();
        let target_has_any_unique = !t.unique_constraints.is_empty();
        if target_has_pk || target_has_any_unique {
            // Target has some declared key — either ref_columns matches
            // (handled in step 3 below by graph::validate_graph), or it
            // doesn't (CARD-03 surfaces a column-mismatch error, which is
            // the right shape for that failure mode).
            continue;
        }

        // Target has NEITHER pk_columns NOR any unique_constraints
        // declared in the TABLES clause. This is unambiguously the D-06
        // case regardless of whether ref_columns is empty (implicit
        // REFERENCES) or set (explicit REFERENCES with cols).
        return Err(format!(
            "Table '{target}' has no PRIMARY KEY declared but is \
             referenced by FK in '{fk_source}'. Add PRIMARY KEY \
             (cols) or UNIQUE (cols) to the TABLES clause for \
             {target}. (v0.10.0: physical-catalog PK auto-inference \
             removed -- see CHANGELOG.)",
            target = t.alias,
            fk_source = fk_source,
        ));
    }

    // 3. Graph validations.
    crate::graph::validate_graph(&def)?;
    crate::graph::validate_facts(&def)?;
    crate::graph::validate_derived_metrics(&def)?;
    crate::graph::validate_using_relationships(&def)?;

    // 4. Capture metadata: timestamp + database_name + schema_name.
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

    // 5. DDL-time type inference via LIMIT 0 probe (file-backed only —
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

    // 6. Fact type inference via typeof(expr).
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
