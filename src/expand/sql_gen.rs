use crate::model::SemanticViewDefinition;
use crate::util::{replace_word_boundary, suggest_closest};

use super::facts::{inline_derived_metrics, toposort_facts};
use super::fan_trap::check_fan_traps;
use super::join_resolver::{resolve_joins_pkfk, synthesize_on_clause, synthesize_on_clause_scoped};
use super::resolution::{find_dimension, find_metric, quote_ident, quote_table_ref};
use super::role_playing::find_using_context;
use super::types::{ExpandError, QueryRequest};

/// Expand a semantic view definition into a SQL query string.
///
/// Takes a view name (for error messages), its definition, and a query request
/// specifying which dimensions and metrics to include. Returns the generated SQL
/// or an `ExpandError` if the request is invalid.
///
/// # Errors
///
/// Returns `ExpandError` if:
/// - Neither dimensions nor metrics are requested (`EmptyRequest`)
/// - A requested dimension or metric name is not found (`UnknownDimension`, `UnknownMetric`)
/// - A dimension or metric name is duplicated (`DuplicateDimension`, `DuplicateMetric`)
#[allow(clippy::too_many_lines, clippy::result_large_err)]
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate: at least one dimension or metric is required.
    if req.dimensions.is_empty() && req.metrics.is_empty() {
        return Err(ExpandError::EmptyRequest {
            view_name: view_name.to_string(),
        });
    }

    // 2. Resolve requested dimensions to their definitions.
    let mut resolved_dims = Vec::with_capacity(req.dimensions.len());
    let mut seen_dims = std::collections::HashSet::new();
    for name in &req.dimensions {
        if !seen_dims.insert(name.to_ascii_lowercase()) {
            return Err(ExpandError::DuplicateDimension {
                view_name: view_name.to_string(),
                name: name.clone(),
            });
        }
        let dim = find_dimension(def, name).ok_or_else(|| {
            let available: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();
            let suggestion = suggest_closest(name, &available);
            ExpandError::UnknownDimension {
                view_name: view_name.to_string(),
                name: name.clone(),
                available,
                suggestion,
            }
        })?;
        resolved_dims.push(dim);
    }

    // 3. Resolve requested metrics to their definitions.
    let mut resolved_mets = Vec::with_capacity(req.metrics.len());
    let mut seen_mets = std::collections::HashSet::new();
    for name in &req.metrics {
        if !seen_mets.insert(name.to_ascii_lowercase()) {
            return Err(ExpandError::DuplicateMetric {
                view_name: view_name.to_string(),
                name: name.clone(),
            });
        }
        let met = find_metric(def, name).ok_or_else(|| {
            let available: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();
            let suggestion = suggest_closest(name, &available);
            ExpandError::UnknownMetric {
                view_name: view_name.to_string(),
                name: name.clone(),
                available,
                suggestion,
            }
        })?;
        resolved_mets.push(met);
    }

    // 4. Pre-compute all metric expressions: inline facts into base metrics,
    //    then inline metric references into derived metrics.
    let topo_order = toposort_facts(&def.facts).unwrap_or_default();
    let resolved_exprs = inline_derived_metrics(&def.metrics, &def.facts, &topo_order);

    // Phase 31: Check for fan traps before generating SQL.
    check_fan_traps(view_name, def, &resolved_dims, &resolved_mets)?;

    // Phase 32: Pre-compute dimension scoped aliases for role-playing tables.
    // Maps dimension index -> scoped alias (e.g., "a__dep_airport").
    let mut dim_scoped_aliases: Vec<Option<String>> = Vec::with_capacity(resolved_dims.len());
    for dim in &resolved_dims {
        let scoped = find_using_context(view_name, def, dim, &resolved_mets)?;
        dim_scoped_aliases.push(scoped);
    }

    // 5. Build the SELECT clause.
    //    Dimensions-only (no metrics): SELECT DISTINCT, no GROUP BY.
    //    Metrics-only (no dimensions): SELECT (global aggregate), no GROUP BY.
    //    Both: SELECT with GROUP BY.
    let mut sql = String::with_capacity(256);
    if !resolved_dims.is_empty() && resolved_mets.is_empty() {
        sql.push_str("SELECT DISTINCT\n");
    } else {
        sql.push_str("SELECT\n");
    }

    let mut select_items: Vec<String> = Vec::new();
    for (i, dim) in resolved_dims.iter().enumerate() {
        let mut base_expr = dim.expr.clone();
        // Phase 32: If this dimension has a scoped alias, rewrite the expression.
        if let Some(ref scoped) = dim_scoped_aliases[i] {
            if let Some(ref st) = dim.source_table {
                // Replace bare alias with scoped alias in expression
                // e.g., "a.city" -> "a__dep_airport.city"
                base_expr = replace_word_boundary(&base_expr, st, scoped);
            }
        }
        // If output_type is set, wrap the expression in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = dim.output_type {
            format!("CAST({base_expr} AS {type_str})")
        } else {
            base_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&dim.name)));
    }
    for met in &resolved_mets {
        // Look up the pre-computed resolved expression (handles both base + derived metrics)
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| met.expr.clone());
        // If output_type is set, wrap the aggregate in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = met.output_type {
            format!("CAST({resolved_expr} AS {type_str})")
        } else {
            resolved_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&met.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 6. FROM clause with base table.
    sql.push_str("\nFROM ");
    sql.push_str(&quote_table_ref(&def.base_table));

    // If tables aliases are declared (Phase 11.1), emit AS "alias" after the base table.
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // Join resolution via PK/FK graph (legacy resolve_joins removed in Phase 27).
    // Phase 32: ordered_aliases may contain scoped aliases like "a__dep_airport".
    let ordered_aliases = resolve_joins_pkfk(def, &resolved_dims, &resolved_mets);
    for alias in &ordered_aliases {
        // Phase 32: Check if this is a scoped alias (contains "__").
        if let Some(sep_pos) = alias.find("__") {
            let rel_name = &alias[sep_pos + 2..];
            // Find the Join by relationship name.
            let Some(join) = def.joins.iter().find(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(rel_name))
            }) else {
                continue;
            };
            // Find physical table name from the bare alias (before __).
            let bare_alias = &alias[..sep_pos];
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == bare_alias);
            let physical_table = table_ref.map_or(bare_alias, |t| t.table.as_str());
            sql.push_str("\nLEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause_scoped(join, &def.tables, alias));
        } else {
            // Standard bare alias join (non-role-playing).
            let Some(join) = def.joins.iter().find(|j| {
                j.table.to_ascii_lowercase() == *alias
                    || j.from_alias.to_ascii_lowercase() == *alias
            }) else {
                continue;
            };
            // Find the TableRef for this alias to get the physical table name.
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == *alias);
            let physical_table = table_ref.map_or(alias.as_str(), |t| t.table.as_str());
            sql.push_str("\nLEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause(join, &def.tables));
        }
    }

    // 7. GROUP BY (only when both dimensions and metrics are present).
    //    Use ordinal positions (GROUP BY 1, 2, ...) instead of expressions to avoid
    //    ambiguity when an expression matches its alias (e.g., `status AS "status"`).
    if !resolved_dims.is_empty() && !resolved_mets.is_empty() {
        sql.push_str("\nGROUP BY\n");
        let group_items: Vec<String> = (1..=resolved_dims.len())
            .map(|i| format!("    {i}"))
            .collect();
        sql.push_str(&group_items.join(",\n"));
    }

    Ok(sql)
}

#[cfg(test)]
mod tests {
    use crate::expand::{expand, ExpandError, QueryRequest};

    mod expand_tests {
        use super::*;
        use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
        use crate::model::{Join, SemanticViewDefinition};

        #[test]
        fn test_basic_single_dimension_single_metric() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    region AS \"region\",
    sum(amount) AS \"total_revenue\"
FROM \"orders\"
GROUP BY
    1";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_multiple_dimensions_multiple_metrics() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "status".to_string()],
                metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    region AS \"region\",
    status AS \"status\",
    sum(amount) AS \"total_revenue\",
    count(*) AS \"order_count\"
FROM \"orders\"
GROUP BY
    1,
    2";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_global_aggregate_no_dimensions() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    sum(amount) AS \"total_revenue\"
FROM \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_identifier_quoting() {
            let def = minimal_def("select", "col", "col", "cnt", "count(*)");
            let req = QueryRequest {
                dimensions: vec!["col".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            // Base table "select" must be quoted
            assert!(sql.contains("FROM \"select\""));
        }

        #[test]
        fn test_dimension_expression_not_quoted() {
            let def = minimal_def(
                "orders",
                "month",
                "date_trunc('month', created_at)",
                "total_revenue",
                "sum(amount)",
            );
            let req = QueryRequest {
                dimensions: vec!["month".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Expression appears verbatim in SELECT; GROUP BY uses ordinal position
            assert!(sql.contains("date_trunc('month', created_at) AS \"month\""));
            assert!(sql.contains("GROUP BY\n    1"));
        }

        #[test]
        fn test_empty_request_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::EmptyRequest { view_name } => {
                    assert_eq!(view_name, "orders");
                }
                other => panic!("Expected EmptyRequest, got: {other}"),
            }
        }

        #[test]
        fn test_dimensions_only_generates_distinct() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "status".to_string()],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT DISTINCT
    region AS \"region\",
    status AS \"status\"
FROM \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_metrics_only_still_works() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    sum(amount) AS \"total_revenue\",
    count(*) AS \"order_count\"
FROM \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_case_insensitive_dimension_lookup() {
            let def = minimal_def("orders", "Region", "region", "total_revenue", "sum(amount)");
            // Request uses lowercase "region" but definition has "Region"
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's expression
            assert!(sql.contains("region AS \"Region\""));
            assert!(sql.contains("GROUP BY\n    1"));
        }

        #[test]
        fn test_unknown_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["reigon".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownDimension {
                    view_name,
                    name,
                    available,
                    suggestion,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "reigon");
                    assert!(available.contains(&"region".to_string()));
                    assert_eq!(suggestion, Some("region".to_string()));
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_unknown_metric_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["totl_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownMetric {
                    view_name,
                    name,
                    available,
                    suggestion,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "totl_revenue");
                    assert!(available.contains(&"total_revenue".to_string()));
                    assert_eq!(suggestion, Some("total_revenue".to_string()));
                }
                other => panic!("Expected UnknownMetric, got: {other}"),
            }
        }

        #[test]
        fn test_unknown_dimension_no_suggestion() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["xyzzy".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownDimension { suggestion, .. } => {
                    assert_eq!(suggestion, None);
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_duplicate_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::DuplicateDimension { view_name, name } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "region");
                }
                other => panic!("Expected DuplicateDimension, got: {other}"),
            }
        }

        #[test]
        fn test_duplicate_metric_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string(), "total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::DuplicateMetric { view_name, name } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "total_revenue");
                }
                other => panic!("Expected DuplicateMetric, got: {other}"),
            }
        }

        #[test]
        fn test_case_insensitive_metric_lookup() {
            let def = SemanticViewDefinition::default()
                .with_base_table("orders")
                .with_metric("Total_Revenue", "sum(amount)", None);
            // Request uses lowercase "total_revenue" but definition has "Total_Revenue"
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's name casing in the alias
            assert!(sql.contains("sum(amount) AS \"Total_Revenue\""));
        }

        #[test]
        fn test_error_display_messages() {
            // EmptyRequest
            let err = ExpandError::EmptyRequest {
                view_name: "orders".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("specify at least dimensions"));

            // UnknownDimension with suggestion
            let err = ExpandError::UnknownDimension {
                view_name: "orders".to_string(),
                name: "reigon".to_string(),
                available: vec!["region".to_string(), "status".to_string()],
                suggestion: Some("region".to_string()),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("reigon"));
            assert!(msg.contains("region, status"));
            assert!(msg.contains("Did you mean 'region'?"));

            // UnknownDimension without suggestion
            let err = ExpandError::UnknownDimension {
                view_name: "orders".to_string(),
                name: "xyzzy".to_string(),
                available: vec!["region".to_string()],
                suggestion: None,
            };
            let msg = format!("{err}");
            assert!(msg.contains("xyzzy"));
            assert!(!msg.contains("Did you mean"));

            // UnknownMetric with suggestion
            let err = ExpandError::UnknownMetric {
                view_name: "orders".to_string(),
                name: "totl_revenue".to_string(),
                available: vec!["total_revenue".to_string()],
                suggestion: Some("total_revenue".to_string()),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("totl_revenue"));
            assert!(msg.contains("Did you mean 'total_revenue'?"));

            // DuplicateDimension
            let err = ExpandError::DuplicateDimension {
                view_name: "orders".to_string(),
                name: "region".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("duplicate dimension 'region'"));

            // DuplicateMetric
            let err = ExpandError::DuplicateMetric {
                view_name: "orders".to_string(),
                name: "total_revenue".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("duplicate metric 'total_revenue'"));
        }

        // Legacy join tests removed in Phase 27

        #[test]
        fn test_join_excluded_when_not_needed() {
            let def = orders_view()
                .with_dimension("customer_name", "customers.name", Some("customers"))
                .with_join_on("customers", "orders.customer_id = customers.id");
            // Request only "region" which comes from base table
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "JOIN should not appear when only base-table dims/metrics requested"
            );
        }

        #[test]
        fn test_no_joins_declared_no_error() {
            let def = minimal_def("orders", "region", "region", "total_revenue", "sum(amount)");
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "no JOIN clauses when no joins declared"
            );
        }

        #[test]
        fn test_dot_qualified_base_table() {
            let def = minimal_def(
                "jaffle.raw_orders",
                "status",
                "status",
                "order_count",
                "count(*)",
            );
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["order_count".to_string()],
            };
            let sql = expand("jaffle_orders", &def, &req).unwrap();
            // Must produce "jaffle"."raw_orders" not "jaffle.raw_orders"
            assert!(
                sql.contains("FROM \"jaffle\".\"raw_orders\""),
                "dot-qualified base_table must be split and quoted: {sql}"
            );
        }
    }

    mod phase11_1_expand_tests {
        use super::*;
        use crate::model::{JoinColumn, TableRef};

        fn def_with_join_columns() -> crate::model::SemanticViewDefinition {
            crate::model::SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    crate::model::Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),

                        output_type: None,
                    },
                    crate::model::Dimension {
                        name: "tier".to_string(),
                        expr: "c.tier".to_string(),
                        source_table: Some("c".to_string()),

                        output_type: None,
                    },
                ],
                metrics: vec![crate::model::Metric {
                    name: "revenue".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![crate::model::Join {
                    table: "customers".to_string(),
                    on: String::new(),
                    from_cols: vec![],
                    join_columns: vec![JoinColumn {
                        from: "customer_id".to_string(),
                        to: "id".to_string(),
                    }],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        // Legacy join_columns tests removed in Phase 27

        #[test]
        fn table_qualified_dimension_lookup_with_matching_source_table() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["o.region".to_string()],
                metrics: vec![],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("o.region"),
                "Must include the dimension expr: {sql}"
            );
            assert!(
                sql.contains("AS \"region\""),
                "Must alias as bare name: {sql}"
            );
        }

        #[test]
        fn bare_dimension_name_still_resolves() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            assert!(
                result.is_ok(),
                "Bare name lookup must succeed: {:?}",
                result.err()
            );
        }

        #[test]
        fn table_qualified_unknown_dimension_returns_error() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["o.nosuch".to_string()],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            match result {
                Err(ExpandError::UnknownDimension { name, .. }) => {
                    let _ = name;
                }
                other => panic!("Expected UnknownDimension error, got: {:?}", other),
            }
        }

        #[test]
        fn table_qualified_metric_lookup_with_matching_source_table() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["o.revenue".to_string()],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("sum(o.amount)"),
                "Must include metric expr: {sql}"
            );
        }
    }

    mod phase12_cast_tests {
        use super::*;
        use crate::expand::test_helpers::TestFixtureExt;
        use crate::model::{Dimension, Metric, SemanticViewDefinition};

        #[test]
        fn output_type_on_metric_emits_cast() {
            let mut def = SemanticViewDefinition::default()
                .with_base_table("orders")
                .with_metric("revenue", "sum(amount)", None);
            def.metrics[0].output_type = Some("BIGINT".to_string());
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(sum(amount) AS BIGINT)"),
                "output_type BIGINT must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn output_type_on_dimension_emits_cast() {
            let mut def = SemanticViewDefinition::default().with_base_table("orders");
            def.dimensions.push(Dimension {
                name: "region_id".to_string(),
                expr: "region_id".to_string(),
                source_table: None,
                output_type: Some("INTEGER".to_string()),
            });
            let req = QueryRequest {
                dimensions: vec!["region_id".to_string()],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(region_id AS INTEGER)"),
                "output_type INTEGER on dimension must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn no_output_type_no_cast() {
            let def = SemanticViewDefinition::default()
                .with_base_table("orders")
                .with_metric("revenue", "sum(amount)", None);
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("CAST(sum(amount) AS"),
                "No output_type must not generate CAST: {sql}"
            );
            assert!(
                sql.contains("sum(amount) AS"),
                "Bare expr must be present: {sql}"
            );
        }
    }

    mod phase26_pkfk_expand_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition, TableRef};

        /// Helper: build a 2-table PK/FK definition (orders -> customers).
        fn pkfk_two_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        /// Helper: build a 3-table PK/FK definition (li -> o -> c).
        fn pkfk_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "line_items".to_string(),
                tables: vec![
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "product".to_string(),
                        expr: "li.product".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_qty".to_string(),
                    expr: "sum(li.qty)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![
                    Join {
                        table: "o".to_string(),
                        from_alias: "li".to_string(),
                        fk_columns: vec!["order_id".to_string()],
                        ..Default::default()
                    },
                    Join {
                        table: "c".to_string(),
                        from_alias: "o".to_string(),
                        fk_columns: vec!["customer_id".to_string()],
                        ..Default::default()
                    },
                ],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        #[test]
        fn test_pkfk_on_clause_simple() {
            let def = pkfk_two_table_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("\"o\".\"customer_id\" = \"c\".\"id\""),
                "PK/FK ON clause must use from_alias.fk = to_alias.pk: {sql}"
            );
        }

        #[test]
        fn test_pkfk_on_clause_composite() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "a".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "b".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["pk1".to_string(), "pk2".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "b.detail".to_string(),
                    source_table: Some("b".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: Some("a".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "b".to_string(),
                    from_alias: "a".to_string(),
                    fk_columns: vec!["fk1".to_string(), "fk2".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["detail".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("\"a\".\"fk1\" = \"b\".\"pk1\""),
                "First FK/PK pair must appear: {sql}"
            );
            assert!(sql.contains("AND"), "Composite ON must use AND: {sql}");
            assert!(
                sql.contains("\"a\".\"fk2\" = \"b\".\"pk2\""),
                "Second FK/PK pair must appear: {sql}"
            );
        }

        #[test]
        fn test_pkfk_left_join_emitted() {
            let def = pkfk_two_table_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN"),
                "PK/FK path must emit LEFT JOIN: {sql}"
            );
            let join_lines: Vec<&str> = sql
                .lines()
                .filter(|l| l.trim().starts_with("LEFT JOIN") || l.trim().starts_with("JOIN"))
                .collect();
            for line in &join_lines {
                assert!(
                    line.trim().starts_with("LEFT JOIN"),
                    "All joins must be LEFT JOIN, got: {line}"
                );
            }
        }

        #[test]
        fn test_pkfk_transitive_join_inclusion() {
            let def = pkfk_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_qty".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"orders\" AS \"o\""),
                "Transitive intermediate join (o) must be included: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"customers\" AS \"c\""),
                "Target join (c) must be included: {sql}"
            );
        }

        #[test]
        fn test_pkfk_pruning() {
            let def = pkfk_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["product".to_string()],
                metrics: vec!["total_qty".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "No joins needed when only base-table dims requested: {sql}"
            );
        }

        #[test]
        fn test_pkfk_topological_order() {
            let mut def = pkfk_three_table_def();
            def.joins.reverse();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_qty".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            let o_pos = sql
                .find("LEFT JOIN \"orders\"")
                .expect("orders join missing");
            let c_pos = sql
                .find("LEFT JOIN \"customers\"")
                .expect("customers join missing");
            assert!(
                o_pos < c_pos,
                "orders (closer to root) must appear before customers (further from root) in topo order: {sql}"
            );
        }
    }

    mod phase27_qualified_refs_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition, TableRef};

        fn qualified_ref_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "p27_orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        #[test]
        fn test_expand_qualified_column_refs_verbatim() {
            let def = qualified_ref_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("p27_test", &def, &req).unwrap();

            assert!(
                sql.contains("c.name AS"),
                "Qualified dim expr 'c.name' must appear verbatim in SQL: {sql}"
            );

            assert!(
                sql.contains("sum(o.amount) AS"),
                "Qualified metric expr 'sum(o.amount)' must appear verbatim in SQL: {sql}"
            );
        }

        #[test]
        fn test_expand_multiple_qualified_refs_different_tables() {
            let def = SemanticViewDefinition {
                base_table: "p27_orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "order_region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string(), "order_region".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("p27_test", &def, &req).unwrap();

            assert!(
                sql.contains("c.name AS"),
                "Qualified dim expr 'c.name' must appear verbatim: {sql}"
            );
            assert!(
                sql.contains("o.region AS"),
                "Qualified dim expr 'o.region' must appear verbatim: {sql}"
            );
            assert!(
                sql.contains("sum(o.amount) AS"),
                "Qualified metric expr 'sum(o.amount)' must appear verbatim: {sql}"
            );
        }
    }

    mod phase29_fact_inlining_tests {
        use super::*;
        use crate::expand::facts::{inline_facts, toposort_facts};
        use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
        use crate::model::{Fact, SemanticViewDefinition};

        #[test]
        fn toposort_facts_empty() {
            let order = toposort_facts(&[]).unwrap();
            assert!(order.is_empty());
        }

        #[test]
        fn toposort_facts_independent() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "x + 1".to_string(),
                    source_table: None,
                    output_type: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "y + 2".to_string(),
                    source_table: None,
                    output_type: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 2);
            assert!(order.contains(&0));
            assert!(order.contains(&1));
        }

        #[test]
        fn toposort_facts_chain() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price * qty".to_string(),
                    source_table: None,
                    output_type: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * (1 - discount)".to_string(),
                    source_table: None,
                    output_type: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 2);
            let a_pos = order.iter().position(|&x| x == 0).unwrap();
            let b_pos = order.iter().position(|&x| x == 1).unwrap();
            assert!(a_pos < b_pos, "a (leaf) must come before b (depends on a)");
        }

        #[test]
        fn toposort_facts_three_level_chain() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price".to_string(),
                    source_table: None,
                    output_type: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * qty".to_string(),
                    source_table: None,
                    output_type: None,
                },
                Fact {
                    name: "c".to_string(),
                    expr: "b * tax".to_string(),
                    source_table: None,
                    output_type: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 3);
            let a_pos = order.iter().position(|&x| x == 0).unwrap();
            let b_pos = order.iter().position(|&x| x == 1).unwrap();
            let c_pos = order.iter().position(|&x| x == 2).unwrap();
            assert!(a_pos < b_pos);
            assert!(b_pos < c_pos);
        }

        #[test]
        fn inline_facts_no_facts() {
            let result = inline_facts("SUM(price)", &[], &[]);
            assert_eq!(result, "SUM(price)");
        }

        #[test]
        fn inline_facts_single_fact() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "price * (1 - discount)".to_string(),
                source_table: None,
                output_type: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(net_price)", &facts, &order);
            assert_eq!(result, "SUM((price * (1 - discount)))");
        }

        #[test]
        fn inline_facts_multi_level() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price * qty".to_string(),
                    source_table: None,
                    output_type: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * (1 - discount)".to_string(),
                    source_table: None,
                    output_type: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(b)", &facts, &order);
            assert_eq!(result, "SUM(((price * qty) * (1 - discount)))");
        }

        #[test]
        fn inline_facts_preserves_parenthesization() {
            let facts = vec![Fact {
                name: "total".to_string(),
                expr: "a + b".to_string(),
                source_table: None,
                output_type: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("x * total", &facts, &order);
            assert_eq!(result, "x * (a + b)");
        }

        #[test]
        fn inline_facts_word_boundary_prevents_collision() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "p * q".to_string(),
                source_table: None,
                output_type: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(net_price_total)", &facts, &order);
            assert_eq!(
                result, "SUM(net_price_total)",
                "Word boundary must prevent matching"
            );
        }

        #[test]
        fn inline_facts_with_qualified_name_in_metric() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "li.price * (1 - li.discount)".to_string(),
                source_table: Some("li".to_string()),
                output_type: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(li.net_price)", &facts, &order);
            assert_eq!(result, "SUM((li.price * (1 - li.discount)))");
        }

        #[test]
        fn expand_with_facts_inlines_into_metric() {
            let def = minimal_def(
                "line_items",
                "region",
                "region",
                "total_net",
                "SUM(net_price)",
            )
            .with_fact("net_price", "price * (1 - discount)", "line_items");
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_net".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM((price * (1 - discount)))"),
                "Fact inlining must resolve net_price in metric expr: {sql}"
            );
        }

        #[test]
        fn expand_without_facts_unchanged() {
            let def = SemanticViewDefinition::default()
                .with_base_table("orders")
                .with_metric("total", "SUM(amount)", None);
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM(amount) AS"),
                "Without facts, metric expr unchanged: {sql}"
            );
        }

        #[test]
        fn expand_multi_level_facts() {
            let def = SemanticViewDefinition::default()
                .with_base_table("line_items")
                .with_metric("total_tax", "SUM(tax_amount)", None)
                .with_fact("net_price", "extended_price * (1 - discount)", "line_items")
                .with_fact("tax_amount", "net_price * tax_rate", "line_items");
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_tax".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM(((extended_price * (1 - discount)) * tax_rate))"),
                "Multi-level fact chain must resolve correctly: {sql}"
            );
        }
    }

    mod phase30_derived_metric_tests {
        use super::*;
        use crate::expand::facts::{inline_derived_metrics, toposort_facts};
        use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
        use crate::model::{Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef};

        #[test]
        fn inline_derived_one_base_one_derived() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("profit").unwrap(),
                "(SUM(amount)) - (SUM(unit_cost))"
            );
        }

        #[test]
        fn inline_derived_stacked() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / revenue * 100".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("profit").unwrap(),
                "(SUM(amount)) - (SUM(unit_cost))"
            );
            assert_eq!(
                resolved.get("margin").unwrap(),
                "((SUM(amount)) - (SUM(unit_cost))) / (SUM(amount)) * 100"
            );
        }

        #[test]
        fn inline_derived_with_facts() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(net_price)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "double_rev".to_string(),
                    expr: "revenue * 2".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "extended_price * (1 - discount)".to_string(),
                source_table: Some("li".to_string()),
                output_type: None,
            }];
            let topo_order = toposort_facts(&facts).unwrap();
            let resolved = inline_derived_metrics(&metrics, &facts, &topo_order);
            assert_eq!(
                resolved.get("revenue").unwrap(),
                "SUM((extended_price * (1 - discount)))"
            );
            assert_eq!(
                resolved.get("double_rev").unwrap(),
                "(SUM((extended_price * (1 - discount)))) * 2"
            );
        }

        #[test]
        fn inline_derived_parenthesization_prevents_precedence_error() {
            let metrics = vec![
                Metric {
                    name: "a".to_string(),
                    expr: "SUM(x)".to_string(),
                    source_table: Some("t".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "b".to_string(),
                    expr: "SUM(y)".to_string(),
                    source_table: Some("t".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "a - b".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / a".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("margin").unwrap(),
                "((SUM(x)) - (SUM(y))) / (SUM(x))"
            );
        }

        #[test]
        fn inline_derived_word_boundary_safety() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "revenue_total".to_string(),
                    expr: "SUM(total)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "derived".to_string(),
                    expr: "revenue + revenue_total".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("derived").unwrap(),
                "(SUM(amount)) + (SUM(total))"
            );
        }

        #[test]
        fn expand_derived_metric_correct_sql() {
            let def = minimal_def("orders", "region", "region", "revenue", "SUM(amount)")
                .with_metric("cost", "SUM(unit_cost)", Some("o"))
                .with_metric("profit", "revenue - cost", None);
            // Fix revenue source_table to match original
            let mut def = def;
            def.metrics[0].source_table = Some("o".to_string());
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["profit".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("(SUM(amount)) - (SUM(unit_cost)) AS \"profit\""),
                "Derived metric must expand to inlined expression: {sql}"
            );
            assert!(
                sql.contains("GROUP BY\n    1"),
                "GROUP BY should reference only the dimension: {sql}"
            );
        }

        #[test]
        fn expand_derived_only_no_base_metrics_requested() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(li.unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],
                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["profit".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(sql.contains("LEFT JOIN \"line_items\" AS \"li\""), "JOIN to li must be included for derived metric referencing li-based metrics: {sql}");
            assert!(
                sql.contains("(SUM(li.amount)) - (SUM(li.unit_cost)) AS \"profit\""),
                "Derived metric expression must be inlined: {sql}"
            );
        }

        #[test]
        fn resolve_joins_includes_transitive_deps_from_derived() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(DISTINCT o.id)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "avg_order_value".to_string(),
                        expr: "revenue / order_count".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],
                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["avg_order_value".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"line_items\" AS \"li\""),
                "JOIN to li must be included for derived metric avg_order_value: {sql}"
            );
        }

        #[test]
        fn expand_derived_metric_with_facts_chain() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(net_price)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],
                joins: vec![],
                facts: vec![Fact {
                    name: "net_price".to_string(),
                    expr: "extended_price * (1 - discount)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                }],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["profit".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains(
                    "(SUM((extended_price * (1 - discount)))) - (SUM(unit_cost)) AS \"profit\""
                ),
                "Fact->base->derived chain must resolve correctly: {sql}"
            );
        }
    }

    mod phase31_fan_trap_tests {
        use super::*;
        use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
        use crate::model::{
            Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn fan_trap_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "li.status".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "segment".to_string(),
                        expr: "c.segment".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.extended_price)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],
                joins: vec![
                    Join {
                        table: "o".to_string(),
                        from_alias: "li".to_string(),
                        fk_columns: vec!["order_id".to_string()],
                        ref_columns: vec!["id".to_string()],
                        name: Some("li_to_order".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                    Join {
                        table: "c".to_string(),
                        from_alias: "o".to_string(),
                        fk_columns: vec!["customer_id".to_string()],
                        ref_columns: vec!["id".to_string()],
                        name: Some("order_to_customer".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                ],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        #[test]
        fn fan_trap_one_to_many_blocked() {
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["order_count".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Fan trap must block the query");
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    view_name,
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(view_name, "sales");
                    assert_eq!(metric_name, "order_count");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_many_to_one_safe() {
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_ok(),
                "MANY TO ONE direction must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_one_to_one_safe() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "d".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "d.detail".to_string(),
                    source_table: Some("d".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],
                joins: vec![Join {
                    table: "d".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["detail_id".to_string()],
                    ref_columns: vec!["id".to_string()],
                    name: Some("order_to_detail".to_string()),
                    cardinality: Cardinality::OneToOne,
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["detail".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(
                result.is_ok(),
                "ONE TO ONE must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_same_table_safe() {
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_ok(),
                "Same table must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_no_joins_safe() {
            let def = minimal_def("orders", "region", "region", "cnt", "COUNT(*)");
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(result.is_ok(), "No joins must be safe: {:?}", result.err());
        }

        #[test]
        fn fan_trap_transitive_chain() {
            let mut def = fan_trap_three_table_def();
            def.metrics.push(Metric {
                name: "customer_count".to_string(),
                expr: "COUNT(DISTINCT c.id)".to_string(),
                source_table: Some("c".to_string()),
                output_type: None,
                using_relationships: vec![],
            });
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["customer_count".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_err(),
                "Transitive chain fan trap must be detected"
            );
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(metric_name, "customer_count");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_derived_metric_blocked() {
            let mut def = fan_trap_three_table_def();
            def.metrics.push(Metric {
                name: "avg_order".to_string(),
                expr: "order_count / 1".to_string(),
                source_table: None,
                output_type: None,
                using_relationships: vec![],
            });
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["avg_order".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Derived metric fan trap must be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(metric_name, "avg_order");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_error_message_format() {
            let err = ExpandError::FanTrap {
                view_name: "sales".to_string(),
                metric_name: "order_count".to_string(),
                metric_table: "o".to_string(),
                dimension_name: "status".to_string(),
                dimension_table: "li".to_string(),
                relationship_name: "li_to_order".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("sales"), "Must contain view name");
            assert!(msg.contains("order_count"), "Must contain metric name");
            assert!(msg.contains("status"), "Must contain dimension name");
            assert!(
                msg.contains("li_to_order"),
                "Must contain relationship name"
            );
            assert!(
                msg.contains("fan trap detected"),
                "Must contain 'fan trap detected'"
            );
            assert!(
                msg.contains("many-to-one cardinality"),
                "Must describe the cardinality direction"
            );
        }
    }

    mod phase32_role_playing_tests {
        use super::*;
        use crate::expand::test_helpers::TestFixtureExt;
        use crate::model::{
            Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn flights_airports_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "flights".to_string(),
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "city".to_string(),
                        expr: "a.city".to_string(),
                        source_table: Some("a".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "country".to_string(),
                        expr: "a.country".to_string(),
                        source_table: Some("a".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "carrier".to_string(),
                        expr: "f.carrier".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "departure_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        using_relationships: vec!["dep_airport".to_string()],
                    },
                    Metric {
                        name: "arrival_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        using_relationships: vec!["arr_airport".to_string()],
                    },
                    Metric {
                        name: "total_flights".to_string(),
                        expr: "departure_count + arrival_count".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],
                joins: vec![
                    Join {
                        table: "a".to_string(),
                        from_alias: "f".to_string(),
                        fk_columns: vec!["departure_code".to_string()],
                        ref_columns: vec!["airport_code".to_string()],
                        name: Some("dep_airport".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                    Join {
                        table: "a".to_string(),
                        from_alias: "f".to_string(),
                        fk_columns: vec!["arrival_code".to_string()],
                        ref_columns: vec!["airport_code".to_string()],
                        name: Some("arr_airport".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                ],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        #[test]
        fn using_metric_generates_scoped_join_alias() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("a__dep_airport"),
                "Scoped alias a__dep_airport must appear: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "LEFT JOIN with scoped alias must appear: {sql}"
            );
        }

        #[test]
        fn two_using_metrics_generate_two_scoped_joins() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["departure_count".to_string(), "arrival_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "dep_airport scoped JOIN must appear: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
                "arr_airport scoped JOIN must appear: {sql}"
            );
        }

        #[test]
        fn dimension_rewritten_to_scoped_alias() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("a__dep_airport.city"),
                "Dimension must be rewritten to scoped alias: {sql}"
            );
        }

        #[test]
        fn ambiguous_dimension_without_using_produces_error() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec![],
            };
            let result = expand("test_flights", &def, &req);
            assert!(result.is_err(), "Ambiguous dimension must produce error");
            match result.unwrap_err() {
                ExpandError::AmbiguousPath {
                    view_name,
                    dimension_name,
                    dimension_table,
                    available_relationships,
                } => {
                    assert_eq!(view_name, "test_flights");
                    assert_eq!(dimension_name, "city");
                    assert_eq!(dimension_table, "a");
                    assert!(available_relationships.contains(&"dep_airport".to_string()));
                    assert!(available_relationships.contains(&"arr_airport".to_string()));
                }
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn ambiguous_path_error_lists_relationships() {
            let err = ExpandError::AmbiguousPath {
                view_name: "test_flights".to_string(),
                dimension_name: "city".to_string(),
                dimension_table: "a".to_string(),
                available_relationships: vec!["dep_airport".to_string(), "arr_airport".to_string()],
            };
            let msg = format!("{err}");
            assert!(msg.contains("test_flights"));
            assert!(msg.contains("city"));
            assert!(msg.contains("ambiguous"));
            assert!(msg.contains("dep_airport"));
            assert!(msg.contains("arr_airport"));
        }

        #[test]
        fn non_ambiguous_single_relationship_works_without_using() {
            let mut def = SemanticViewDefinition::default()
                .with_base_table("orders")
                .with_table("o", "orders", &["id"])
                .with_table("c", "customers", &["id"])
                .with_dimension("customer_name", "c.name", Some("c"))
                .with_metric("revenue", "SUM(o.amount)", Some("o"));
            def.joins.push(Join {
                table: "c".to_string(),
                from_alias: "o".to_string(),
                fk_columns: vec!["customer_id".to_string()],
                name: Some("order_to_customer".to_string()),
                ..Default::default()
            });
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(
                result.is_ok(),
                "Single relationship must work without USING: {:?}",
                result.err()
            );
        }

        #[test]
        fn base_table_dimension_works_unchanged() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("f.carrier AS \"carrier\""),
                "Base table dimension must appear unchanged: {sql}"
            );
        }

        #[test]
        fn fan_trap_detection_works_with_using_paths() {
            let def = SemanticViewDefinition {
                base_table: "flights".to_string(),
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "carrier".to_string(),
                    expr: "f.carrier".to_string(),
                    source_table: Some("f".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "airport_count".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("a".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],
                joins: vec![Join {
                    table: "a".to_string(),
                    from_alias: "f".to_string(),
                    fk_columns: vec!["dep_airport_code".to_string()],
                    ref_columns: vec!["airport_code".to_string()],
                    name: Some("dep_flights".to_string()),
                    cardinality: Cardinality::ManyToOne,
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["airport_count".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(result.is_err(), "Fan trap must still be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap { .. } => {}
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn derived_metric_with_two_using_resolves_both_joins() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["total_flights".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "Derived metric must resolve dep_airport join: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
                "Derived metric must resolve arr_airport join: {sql}"
            );
        }

        #[test]
        fn metric_using_from_base_table_no_unnecessary_join() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![TableRef {
                    alias: "o".to_string(),
                    table: "orders".to_string(),
                    pk_columns: vec!["id".to_string()],
                    unique_constraints: vec![],
                }],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "No JOIN needed when everything is on base table: {sql}"
            );
        }

        #[test]
        fn backward_compat_no_using_expands_as_before() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],
                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    name: Some("order_to_customer".to_string()),
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"customers\" AS \"c\""),
                "Non-USING definition must use bare alias: {sql}"
            );
            assert!(
                sql.contains("c.name AS"),
                "Dimension expr must use bare alias: {sql}"
            );
        }

        #[test]
        fn ambiguous_dimension_with_derived_metric_using_both_paths() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["total_flights".to_string()],
            };
            let result = expand("test_flights", &def, &req);
            assert!(
                result.is_err(),
                "City dimension must be ambiguous when derived metric uses both paths"
            );
            match result.unwrap_err() {
                ExpandError::AmbiguousPath { .. } => {}
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn scoped_join_on_clause_uses_correct_fk_pk() {
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("\"f\".\"departure_code\" = \"a__dep_airport\".\"airport_code\""),
                "Scoped JOIN ON clause must use correct FK/PK: {sql}"
            );
        }
    }
}
