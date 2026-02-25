use proptest::prelude::*;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::{Dimension, Join, Metric, SemanticViewDefinition};

// ---------------------------------------------------------------------------
// Test fixture definitions
// ---------------------------------------------------------------------------

/// Simple definition: base_table "orders", 3 dimensions, 3 metrics, 1 filter, no joins.
fn simple_definition() -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                source_table: None,
            },
            Dimension {
                name: "month".to_string(),
                expr: "date_trunc('month', created_at)".to_string(),
                source_table: None,
            },
            Dimension {
                name: "status".to_string(),
                expr: "status".to_string(),
                source_table: None,
            },
        ],
        metrics: vec![
            Metric {
                name: "total_revenue".to_string(),
                expr: "sum(amount)".to_string(),
                source_table: None,
            },
            Metric {
                name: "order_count".to_string(),
                expr: "count(*)".to_string(),
                source_table: None,
            },
            Metric {
                name: "avg_amount".to_string(),
                expr: "avg(amount)".to_string(),
                source_table: None,
            },
        ],
        filters: vec!["status = 'active'".to_string()],
        joins: vec![],
    }
}

/// Joined definition: base_table "orders", 4 dimensions (2 with source_table),
/// 3 metrics (2 with source_table), 2 joins, 1 filter.
fn joined_definition() -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                source_table: None,
            },
            Dimension {
                name: "customer_name".to_string(),
                expr: "customers.name".to_string(),
                source_table: Some("customers".to_string()),
            },
            Dimension {
                name: "month".to_string(),
                expr: "date_trunc('month', created_at)".to_string(),
                source_table: None,
            },
            Dimension {
                name: "product_category".to_string(),
                expr: "products.category".to_string(),
                source_table: Some("products".to_string()),
            },
        ],
        metrics: vec![
            Metric {
                name: "total_revenue".to_string(),
                expr: "sum(amount)".to_string(),
                source_table: None,
            },
            Metric {
                name: "customer_count".to_string(),
                expr: "count(DISTINCT customer_id)".to_string(),
                source_table: Some("customers".to_string()),
            },
            Metric {
                name: "product_count".to_string(),
                expr: "count(DISTINCT product_id)".to_string(),
                source_table: Some("products".to_string()),
            },
        ],
        filters: vec!["status = 'active'".to_string()],
        joins: vec![
            Join {
                table: "customers".to_string(),
                on: "\"orders\".\"customer_id\" = \"customers\".\"id\"".to_string(),
            },
            Join {
                table: "products".to_string(),
                on: "\"orders\".\"product_id\" = \"products\".\"id\"".to_string(),
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Strategy: generate arbitrary valid QueryRequest from a definition
// ---------------------------------------------------------------------------

/// Generate a random valid `QueryRequest` from a definition.
///
/// Dimensions: 0..all (empty = global aggregate).
/// Metrics: 1..all (at least one required).
fn arb_query_request(def: &SemanticViewDefinition) -> impl Strategy<Value = QueryRequest> {
    let dim_names: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();
    let met_names: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();

    let dim_strategy = proptest::sample::subsequence(dim_names, 0..=def.dimensions.len());
    // At least 1 metric required
    let met_strategy = proptest::sample::subsequence(met_names, 1..=def.metrics.len());

    (dim_strategy, met_strategy).prop_map(|(dims, mets)| QueryRequest {
        dimensions: dims,
        metrics: mets,
    })
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

proptest! {
    /// Property 1: All requested dimensions appear in GROUP BY (or GROUP BY absent if no dims).
    #[test]
    fn all_dimensions_in_group_by(req in arb_query_request(&simple_definition())) {
        let def = simple_definition();
        let sql = expand("test", &def, &req).unwrap();

        if req.dimensions.is_empty() {
            prop_assert!(
                !sql.contains("GROUP BY"),
                "Empty dimensions should produce no GROUP BY. SQL:\n{sql}"
            );
        } else {
            let group_by_section = sql.split("GROUP BY").nth(1)
                .expect("GROUP BY section must exist when dimensions are non-empty");
            for dim_name in &req.dimensions {
                // Find the definition dimension to get its expr
                let dim_def = def.dimensions.iter()
                    .find(|d| d.name.eq_ignore_ascii_case(dim_name))
                    .unwrap();
                prop_assert!(
                    group_by_section.contains(&dim_def.expr),
                    "GROUP BY must contain expr '{}' for dimension '{}'. GROUP BY section:\n{}",
                    dim_def.expr, dim_name, group_by_section
                );
            }
        }
    }

    /// Property 2: All requested dimensions and metrics appear as aliases in SELECT.
    #[test]
    fn all_dimensions_and_metrics_in_select(req in arb_query_request(&simple_definition())) {
        let def = simple_definition();
        let sql = expand("test", &def, &req).unwrap();

        for dim_name in &req.dimensions {
            let dim_def = def.dimensions.iter()
                .find(|d| d.name.eq_ignore_ascii_case(dim_name))
                .unwrap();
            let alias = format!("AS \"{}\"", dim_def.name);
            prop_assert!(
                sql.contains(&alias),
                "SELECT must contain alias '{alias}' for dimension '{dim_name}'. SQL:\n{sql}"
            );
        }
        for met_name in &req.metrics {
            let met_def = def.metrics.iter()
                .find(|m| m.name.eq_ignore_ascii_case(met_name))
                .unwrap();
            let alias = format!("AS \"{}\"", met_def.name);
            prop_assert!(
                sql.contains(&alias),
                "SELECT must contain alias '{alias}' for metric '{met_name}'. SQL:\n{sql}"
            );
        }
    }

    /// Property 3: SQL structure is valid (WITH, SELECT, FROM present; GROUP BY iff dims).
    #[test]
    fn sql_structure_valid(req in arb_query_request(&simple_definition())) {
        let def = simple_definition();
        let sql = expand("test", &def, &req).unwrap();

        prop_assert!(
            sql.starts_with("WITH \"_base\" AS ("),
            "SQL must start with CTE. SQL:\n{sql}"
        );
        prop_assert!(
            sql.contains("SELECT"),
            "SQL must contain SELECT. SQL:\n{sql}"
        );
        prop_assert!(
            sql.contains("FROM \"_base\""),
            "SQL must contain FROM \"_base\". SQL:\n{sql}"
        );
        if !req.dimensions.is_empty() {
            prop_assert!(
                sql.contains("GROUP BY"),
                "Non-empty dimensions must produce GROUP BY. SQL:\n{sql}"
            );
        }
    }

    /// Property 4: Joins are only included when a requested dim/metric needs them.
    #[test]
    fn joins_only_when_needed(req in arb_query_request(&joined_definition())) {
        let def = joined_definition();
        let sql = expand("test", &def, &req).unwrap();

        for join in &def.joins {
            let join_table_needed = req.dimensions.iter().any(|d| {
                def.dimensions.iter()
                    .find(|dd| dd.name.eq_ignore_ascii_case(d))
                    .and_then(|dd| dd.source_table.as_ref())
                    .map_or(false, |st: &String| st.eq_ignore_ascii_case(&join.table))
            }) || req.metrics.iter().any(|m| {
                def.metrics.iter()
                    .find(|mm| mm.name.eq_ignore_ascii_case(m))
                    .and_then(|mm| mm.source_table.as_ref())
                    .map_or(false, |st: &String| st.eq_ignore_ascii_case(&join.table))
            });

            let join_marker = format!("JOIN \"{}\"", join.table);
            if !join_table_needed {
                prop_assert!(
                    !sql.contains(&join_marker),
                    "JOIN '{}' should NOT be included when no requested dim/metric uses it. SQL:\n{}",
                    join.table, sql
                );
            }
        }
    }

    /// Property 5: Filters are always present regardless of dimension/metric selection.
    #[test]
    fn filters_always_present(req in arb_query_request(&simple_definition())) {
        let def = simple_definition();
        let sql = expand("test", &def, &req).unwrap();

        for filter in &def.filters {
            let f: &str = filter;
            prop_assert!(
                sql.contains(f),
                "Filter '{}' must always be present in SQL. SQL:\n{}", f, sql
            );
        }
    }

    /// Property 6: Global aggregate (empty dimensions) has no GROUP BY but includes metric expr.
    #[test]
    fn global_aggregate_no_group_by(
        _dummy in Just(QueryRequest {
            dimensions: vec![],
            metrics: vec!["total_revenue".to_string()],
        })
    ) {
        let def = simple_definition();
        let req = QueryRequest {
            dimensions: vec![],
            metrics: vec!["total_revenue".to_string()],
        };
        let sql = expand("test", &def, &req).unwrap();

        prop_assert!(
            !sql.contains("GROUP BY"),
            "Global aggregate must not contain GROUP BY. SQL:\n{sql}"
        );
        let met_def = def.metrics.iter()
            .find(|m| m.name == "total_revenue")
            .unwrap();
        prop_assert!(
            sql.contains(&met_def.expr),
            "Global aggregate SQL must contain metric expr '{}'. SQL:\n{}",
            met_def.expr, sql
        );
    }
}
