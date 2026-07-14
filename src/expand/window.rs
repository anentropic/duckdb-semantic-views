//! CTE-based expansion for window function metrics.
//!
//! Window metrics wrap another metric in a SQL window function (e.g., `AVG(total_qty)`
//! OVER (PARTITION BY ... ORDER BY ...)). The expansion generates:
//! 1. A CTE that aggregates the inner metric(s) by ALL queried dimensions
//! 2. An outer SELECT that applies the window function over the CTE results
//!
//! PARTITION BY EXCLUDING is resolved at expansion time: the partition columns
//! are all queried dimensions MINUS the excluded dimensions.

use std::collections::{HashMap, HashSet};

use crate::model::{Metric, NullsOrder, SemanticViewDefinition, SortOrder};
use crate::util::replace_word_boundary;

use super::join_resolver::{push_join_clauses, resolve_joins_pkfk};
use super::resolution::quote_ident;
use super::select_spec::{
    push_from_base, push_group_by_ordinals, FromSource, GroupBy, SelectItem, SelectSpec,
};
use super::types::{ExpandError, ResolvedDim};

/// Generate CTE-based expansion SQL for queries containing window function metrics.
///
/// Called from `expand()` when all resolved metrics are window metrics.
/// Receives already-resolved dims, metrics, expressions, and scoped aliases.
///
/// # Strategy
///
/// 1. **CTE `__sv_agg`**: Aggregates all inner metrics by ALL queried dimensions
///    (standard GROUP BY). This pre-aggregates the data so that window functions
///    operate on grouped results, not raw rows.
///
/// 2. **Outer SELECT**: Applies each window function over the CTE results with
///    computed PARTITION BY (all queried dims minus EXCLUDING dims) and ORDER BY.
///    No GROUP BY in the outer query -- window functions are row-level operations.
#[allow(clippy::too_many_lines, clippy::unnecessary_wraps)]
pub(super) fn expand_window_metrics(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[ResolvedDim],
    resolved_mets: &[&Metric],
    resolved_exprs: &HashMap<String, String>,
) -> Result<String, ExpandError> {
    // 1. Validate required dimensions for each window metric.
    let queried_dim_names: HashSet<String> = resolved_dims
        .iter()
        .map(|rd| rd.dim.name.to_ascii_lowercase())
        .collect();

    for met in resolved_mets {
        let Some(ref ws) = met.window_spec else {
            continue;
        };
        // Check EXCLUDING dims are all in the query
        for excl_dim in &ws.excluding_dims {
            if !queried_dim_names.contains(&excl_dim.to_ascii_lowercase()) {
                return Err(ExpandError::WindowMetricRequiredDimension {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    dimension_name: excl_dim.clone(),
                    reason: "PARTITION BY EXCLUDING".to_string(),
                });
            }
        }
        // Check explicit PARTITION BY dims are all in the query
        for part_dim in &ws.partition_dims {
            if !queried_dim_names.contains(&part_dim.to_ascii_lowercase()) {
                return Err(ExpandError::WindowMetricRequiredDimension {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    dimension_name: part_dim.clone(),
                    reason: "PARTITION BY".to_string(),
                });
            }
        }
        // Check ORDER BY dims are all in the query
        for ob in &ws.order_by {
            if !queried_dim_names.contains(&ob.expr.to_ascii_lowercase()) {
                return Err(ExpandError::WindowMetricRequiredDimension {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    dimension_name: ob.expr.clone(),
                    reason: "ORDER BY".to_string(),
                });
            }
        }
    }

    // 2. Collect distinct inner metric names needed for the CTE.
    let mut inner_metric_set: HashSet<String> = HashSet::new();
    let mut inner_metric_order: Vec<String> = Vec::new();
    for met in resolved_mets {
        let Some(ref ws) = met.window_spec else {
            continue;
        };
        let key = ws.inner_metric.to_ascii_lowercase();
        if inner_metric_set.insert(key.clone()) {
            inner_metric_order.push(key);
        }
    }

    // Resolve inner metric expressions via resolved_exprs.
    let mut inner_metric_exprs: HashMap<String, String> = HashMap::new();
    for inner_name in &inner_metric_order {
        let expr = resolved_exprs.get(inner_name).cloned().unwrap_or_else(|| {
            // Fall back to finding the metric definition directly
            def.metrics
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(inner_name))
                .map_or_else(|| inner_name.clone(), |m| m.expr.clone())
        });
        inner_metric_exprs.insert(inner_name.clone(), expr);
    }

    // 3. Build CTE __sv_agg
    let mut sql = String::with_capacity(512);
    sql.push_str("WITH __sv_agg AS (\n    SELECT\n");

    let mut cte_select_items: Vec<String> = Vec::new();

    // Dimension columns in CTE
    for rd in resolved_dims {
        let dim = rd.dim;
        let mut base_expr = dim.expr.clone();
        if let Some(ref scoped) = rd.scoped_alias {
            if let Some(ref st) = dim.source_table {
                base_expr = replace_word_boundary(&base_expr, st, scoped);
            }
        }
        let item = SelectItem::new(base_expr, dim.output_type.clone(), quote_ident(&dim.name));
        cte_select_items.push(format!("        {}", item.render()));
    }

    // Inner metric aggregated columns in CTE
    for inner_name in &inner_metric_order {
        let expr = &inner_metric_exprs[inner_name];
        // The inner metric expression is already an aggregate (e.g., SUM(s.quantity))
        // so we include it directly in the CTE SELECT with GROUP BY.
        cte_select_items.push(format!("        {} AS {}", expr, quote_ident(inner_name)));
    }

    sql.push_str(&cte_select_items.join(",\n"));

    // CTE FROM clause
    push_from_base(&mut sql, def, "\n    ");

    // CTE JOINs
    let dims: Vec<&crate::model::Dimension> = resolved_dims.iter().map(|rd| rd.dim).collect();
    let resolved_joins = resolve_joins_pkfk(def, &dims, resolved_mets, &[]);
    push_join_clauses(&mut sql, &resolved_joins, def, "\n    LEFT JOIN ");

    // CTE GROUP BY (all dimension columns)
    if !resolved_dims.is_empty() {
        push_group_by_ordinals(&mut sql, resolved_dims.len(), "\n    ", "        ");
    }

    sql.push_str("\n)\n");

    // 4. Build the outer SELECT over the aggregation CTE.
    let mut outer_items: Vec<SelectItem> = Vec::new();

    // Dimension columns: reference CTE aliases (outer query over the CTE, so
    // referencing the alias is safe here).
    for rd in resolved_dims {
        outer_items.push(SelectItem::new(
            quote_ident(&rd.dim.name),
            None,
            quote_ident(&rd.dim.name),
        ));
    }

    // Window metric columns
    for met in resolved_mets {
        let Some(ref ws) = met.window_spec else {
            continue;
        };

        // Build the function call: window_function(inner_metric_alias, extra_args...)
        let inner_alias = quote_ident(&ws.inner_metric.to_ascii_lowercase());
        let mut func_args = vec![inner_alias];
        for arg in &ws.extra_args {
            func_args.push(arg.clone());
        }
        let func_call = format!("{}({})", ws.window_function, func_args.join(", "));

        // Compute PARTITION BY columns
        let partition_cols: Vec<String> = if ws.partition_dims.is_empty() {
            // PARTITION BY EXCLUDING: all queried dims minus excluding_dims
            let excluding_set: HashSet<String> = ws
                .excluding_dims
                .iter()
                .map(|d| d.to_ascii_lowercase())
                .collect();
            resolved_dims
                .iter()
                .filter(|rd| !excluding_set.contains(&rd.dim.name.to_ascii_lowercase()))
                .map(|rd| quote_ident(&rd.dim.name))
                .collect()
        } else {
            // Explicit PARTITION BY: use listed dims directly
            ws.partition_dims.iter().map(|d| quote_ident(d)).collect()
        };

        // Build OVER clause
        let mut over_parts: Vec<String> = Vec::new();
        if !partition_cols.is_empty() {
            over_parts.push(format!("PARTITION BY {}", partition_cols.join(", ")));
        }
        if !ws.order_by.is_empty() {
            let order_items: Vec<String> = ws
                .order_by
                .iter()
                .map(|ob| {
                    let dir = match ob.order {
                        SortOrder::Asc => "ASC",
                        SortOrder::Desc => "DESC",
                    };
                    let nulls = match ob.nulls {
                        NullsOrder::First => "NULLS FIRST",
                        NullsOrder::Last => "NULLS LAST",
                    };
                    format!("{} {} {}", quote_ident(&ob.expr), dir, nulls)
                })
                .collect();
            over_parts.push(format!("ORDER BY {}", order_items.join(", ")));
        }
        if let Some(ref frame) = ws.frame_clause {
            over_parts.push(frame.clone());
        }

        let over_clause = over_parts.join(" ");
        let window_expr = format!("{func_call} OVER ({over_clause})");

        outer_items.push(SelectItem::new(
            window_expr,
            met.output_type.clone(),
            quote_ident(&met.name),
        ));
    }

    // Window functions are row-level ⇒ no GROUP BY in the outer query.
    sql.push_str(
        &SelectSpec {
            distinct: false,
            items: outer_items,
            from: FromSource::Named("__sv_agg".to_string()),
            group_by: GroupBy::None,
        }
        .render(),
    );

    Ok(sql)
}

#[cfg(test)]
mod tests {
    use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
    use crate::expand::{expand, DimensionName, ExpandError, MetricName, QueryRequest};
    use crate::model::{NullsOrder, SortOrder, WindowOrderBy, WindowSpec};

    /// Single window metric with 3 dims -- CTE with GROUP BY all dims,
    /// outer SELECT with window function and PARTITION BY (all minus excluded).
    #[test]
    fn test_window_single_metric_three_dims() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(s.quantity)")
            .with_dimension("date", "date", None)
            .with_dimension("year", "year", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![
                DimensionName::new("store"),
                DimensionName::new("date"),
                DimensionName::new("year"),
            ],
            metrics: vec![MetricName::new("total_qty")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(sql.contains("WITH __sv_agg AS"), "Should have CTE: {sql}");
        assert!(sql.contains("GROUP BY"), "CTE should have GROUP BY: {sql}");
        // PARTITION BY should exclude "date" -- only store and year
        assert!(
            sql.contains("PARTITION BY \"store\", \"year\""),
            "Should partition by non-excluded dims: {sql}"
        );
        assert!(
            sql.contains("ORDER BY \"date\" ASC NULLS LAST"),
            "Should have ORDER BY date: {sql}"
        );
        assert!(
            sql.contains("AVG(\"total_qty\")"),
            "Should have AVG window function: {sql}"
        );
        assert!(
            sql.contains("FROM __sv_agg"),
            "Outer should reference CTE: {sql}"
        );
    }

    /// Window metric excluding 2 dims -- PARTITION BY only remaining dim.
    #[test]
    fn test_window_excluding_two_dims() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(quantity)")
            .with_dimension("date", "date", None)
            .with_dimension("year", "year", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string(), "year".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![
                DimensionName::new("store"),
                DimensionName::new("date"),
                DimensionName::new("year"),
            ],
            metrics: vec![MetricName::new("total_qty")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        // Only "store" should be in PARTITION BY
        assert!(
            sql.contains("PARTITION BY \"store\""),
            "Should partition by only non-excluded dim: {sql}"
        );
        // "date" and "year" should NOT be in PARTITION BY
        assert!(
            !sql.contains("PARTITION BY \"store\", \"date\""),
            "Excluded dims should not be in PARTITION BY: {sql}"
        );
    }

    /// Window metric with frame clause -- RANGE BETWEEN included in OVER.
    #[test]
    fn test_window_with_frame_clause() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(quantity)")
            .with_dimension("date", "date", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: Some(
                        "RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW".to_string(),
                    ),
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("store"), DimensionName::new("date")],
            metrics: vec![MetricName::new("total_qty")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW"),
            "Should include frame clause in OVER: {sql}"
        );
    }

    /// Window metric with extra args -- LAG(metric, 30).
    #[test]
    fn test_window_with_extra_args() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(quantity)")
            .with_dimension("date", "date", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "LAG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec!["30".to_string()],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("store"), DimensionName::new("date")],
            metrics: vec![MetricName::new("total_qty")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("LAG(\"total_qty\", 30)"),
            "Should include extra args in function call: {sql}"
        );
    }

    /// Two window metrics sharing same CTE but different EXCLUDING sets.
    #[test]
    fn test_two_window_metrics_different_excluding() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(quantity)")
            .with_dimension("date", "date", None)
            .with_dimension("year", "year", None)
            .with_metric("avg_7", "SUM(quantity)", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            )
            .with_window_spec(
                "avg_7",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "avg_7".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string(), "year".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![
                DimensionName::new("store"),
                DimensionName::new("date"),
                DimensionName::new("year"),
            ],
            metrics: vec![MetricName::new("total_qty"), MetricName::new("avg_7")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(sql.contains("WITH __sv_agg AS"), "Should have CTE: {sql}");
        // First metric: PARTITION BY store, year (excluding date)
        assert!(
            sql.contains("PARTITION BY \"store\", \"year\""),
            "First metric should partition by store, year: {sql}"
        );
        // Second metric: PARTITION BY store (excluding date, year)
        assert!(
            sql.contains("PARTITION BY \"store\" ORDER"),
            "Second metric should partition by store only: {sql}"
        );
    }

    /// Mixing window + regular aggregate metric returns error.
    #[test]
    fn test_window_aggregate_mixing_error() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(quantity)")
            .with_dimension("date", "date", None)
            .with_metric("avg_price", "AVG(price)", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("store"), DimensionName::new("date")],
            metrics: vec![MetricName::new("total_qty"), MetricName::new("avg_price")],
        };

        let result = expand("test_view", &def, &req);
        assert!(result.is_err(), "Should error on mixing: {:?}", result);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("cannot mix window function metrics"),
            "Error message should mention mixing: {msg}"
        );
        assert!(
            msg.contains("total_qty"),
            "Should mention window metric name: {msg}"
        );
        assert!(
            msg.contains("avg_price"),
            "Should mention aggregate metric name: {msg}"
        );
    }

    /// Missing required dimension (EXCLUDING dim not in query) returns error.
    #[test]
    fn test_window_missing_required_dim() {
        let def = minimal_def("sales", "store", "store", "total_qty", "SUM(quantity)")
            .with_dimension("date", "date", None)
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        // Query with only 'store' -- missing 'date' which is required by EXCLUDING and ORDER BY
        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("store")],
            metrics: vec![MetricName::new("total_qty")],
        };

        let result = expand("test_view", &def, &req);
        assert!(
            result.is_err(),
            "Should error on missing required dim: {:?}",
            result
        );
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("requires dimension 'date'"),
            "Should mention missing dim: {msg}"
        );
    }

    /// SG-6 (code review 2026-07-02): window metrics get the standard
    /// fan-trap check. The previous unconditional `is_window()` skip was
    /// based on the incorrect premise that the CTE pre-aggregation handles
    /// fan-out — but the CTE's inner aggregate is computed OVER the
    /// already-fanned join, so it is inflated before the window function
    /// runs. This fixture (metric grain on `c`, dims on `a`, where a->c is
    /// `ManyToOne`) fans `c`'s rows and must now error.
    #[test]
    fn test_fan_trap_checks_window_metrics() {
        // Multi-table view: window metric crosses many-to-one boundary
        // in the fan-out direction.
        let def = orders_view()
            .with_table("customers", "customers", &[])
            .clear_dimensions()
            .clear_metrics()
            .with_table("c", "customers", &["id"])
            .with_table("a", "accounts", &["id"])
            .with_dimension("acct_name", "a.name", Some("a"))
            .with_dimension("date", "a.report_date", Some("a"))
            .with_metric("total_balance", "SUM(a.balance)", Some("c"))
            .with_window_spec(
                "total_balance",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_balance".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec!["date".to_string()],
                    partition_dims: vec![],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            )
            .with_pkfk_join("cust_acct", "a", "c", &["customer_id"], &["id"]);

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("acct_name"), DimensionName::new("date")],
            metrics: vec![MetricName::new("total_balance")],
        };

        // Window metrics are checked like any other aggregate: fan-out error.
        let result = expand("test_view", &def, &req);
        assert!(
            matches!(result, Err(ExpandError::FanTrap { .. })),
            "Window metric over a fanning join must be a fan trap error, got: {result:?}"
        );
    }

    /// Explicit PARTITION BY uses listed dims directly.
    #[test]
    fn test_partition_by_explicit() {
        let def = orders_view()
            .with_dimension("store", "o.store_id", Some("o"))
            .with_dimension("date", "o.sale_date", Some("o"))
            .with_metric("total_qty", "SUM(o.qty)", Some("o"))
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec![],
                    partition_dims: vec!["store".to_string()],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("store"), DimensionName::new("date")],
            metrics: vec![MetricName::new("total_qty")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        // PARTITION BY should use "store" directly, not exclude anything
        assert!(
            sql.contains("PARTITION BY \"store\""),
            "Should use explicit PARTITION BY store: {sql}"
        );
        assert!(
            !sql.contains("EXCLUDING"),
            "Should not contain EXCLUDING: {sql}"
        );
    }

    /// Explicit PARTITION BY dims are required in the query.
    #[test]
    fn test_partition_by_explicit_missing_dim_error() {
        let def = orders_view()
            .with_dimension("store", "o.store_id", Some("o"))
            .with_dimension("date", "o.sale_date", Some("o"))
            .with_metric("total_qty", "SUM(o.qty)", Some("o"))
            .with_window_spec(
                "total_qty",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    extra_args: vec![],
                    excluding_dims: vec![],
                    partition_dims: vec!["store".to_string()],
                    order_by: vec![WindowOrderBy {
                        expr: "date".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    }],
                    frame_clause: None,
                },
            );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("date")],
            metrics: vec![MetricName::new("total_qty")],
        };

        let result = expand("test_view", &def, &req);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("requires dimension 'store'"),
            "Should mention missing partition dim: {msg}"
        );
    }
}
