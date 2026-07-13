//! Materialization routing for the expansion engine.
//!
//! When a query's requested dimensions and metrics exactly match a
//! declared materialization, the engine routes to the pre-aggregated
//! table instead of expanding raw sources with JOINs and GROUP BY.

use std::collections::HashSet;

use crate::model::{Dimension, Materialization, Metric, SemanticViewDefinition};

use super::resolution::{qualify_and_quote_table_ref, quote_ident};

/// Find the materialization whose declared dimension and metric name sets
/// EXACTLY match the requested ones (case-insensitive), honoring the routing
/// exclusions. First match wins (definition order); `None` when nothing matches
/// or routing is excluded.
///
/// # Matching rules (v0.7.0 -- exact match only)
///
/// A materialization matches when:
/// 1. Its dimension set EXACTLY equals the requested dimension set (case-insensitive)
/// 2. Its metric set EXACTLY equals the requested metric set (case-insensitive)
/// 3. No requested metric has `non_additive_by` (semi-additive exclusion)
/// 4. No requested metric has `window_spec` (window function exclusion)
///
/// Re-aggregation routing (materialization covers a SUPERSET of requested dims)
/// is deferred to v2 (MAT-F01).
///
/// Single source of the routing decision (E-6, code-review 2026-07-11), shared
/// by [`try_route_materialization`] (which emits the materialized SELECT) and
/// [`find_routing_materialization_name`] (which reports the chosen name for
/// `explain`) so the two can never disagree about what would be routed.
fn find_matching_materialization<'a>(
    def: &'a SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> Option<&'a Materialization> {
    // Fast path: no materializations declared (MAT-05).
    if def.materializations.is_empty() {
        return None;
    }
    // MAT-04: semi-additive and window-function metrics are never routed.
    if resolved_mets.iter().any(|m| !m.non_additive_by.is_empty()) {
        return None;
    }
    if resolved_mets.iter().any(|m| m.is_window()) {
        return None;
    }

    // Requested dimension/metric name sets (lowercase for case-insensitive
    // matching).
    let req_dims: HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    let req_mets: HashSet<String> = resolved_mets
        .iter()
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    // Definition order -> first exact match wins.
    def.materializations.iter().find(|mat| {
        let mat_dims: HashSet<String> = mat
            .dimensions
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        let mat_mets: HashSet<String> =
            mat.metrics.iter().map(|m| m.to_ascii_lowercase()).collect();
        mat_dims == req_dims && mat_mets == req_mets
    })
}

/// Attempt to route a query to a materialization table.
///
/// Returns `Some(sql)` selecting from the pre-aggregated table when an
/// exact-match materialization is found (rules: [`find_matching_materialization`]),
/// else `None` and the caller expands raw sources.
pub(crate) fn try_route_materialization(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> Option<String> {
    find_matching_materialization(def, resolved_dims, resolved_mets)
        .map(|mat| build_materialized_sql(&mat.table, def, resolved_dims, resolved_mets))
}

/// Name of the materialization that would be selected for routing, or `None`.
///
/// Used by `explain_semantic_view` to report the routing decision; delegates to
/// the shared [`find_matching_materialization`] so it cannot drift from the
/// routing [`try_route_materialization`] actually performs.
// Used only under the `extension` feature (explain.rs); scope the allow to the
// default build so genuine dead code is still caught under `extension` (ST-8).
#[cfg_attr(not(feature = "extension"), allow(dead_code))]
pub(crate) fn find_routing_materialization_name<'a>(
    def: &'a SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> Option<&'a str> {
    find_matching_materialization(def, resolved_dims, resolved_mets).map(|mat| mat.name.as_str())
}

/// Generate a SELECT from the materialization table.
///
/// The materialization table is expected to have columns named after the
/// dimension and metric names. The SQL simply selects them by name,
/// applying `output_type` casts when declared.
fn build_materialized_sql(
    table: &str,
    def: &SemanticViewDefinition,
    dims: &[&Dimension],
    mets: &[&Metric],
) -> String {
    let mut items: Vec<String> = Vec::with_capacity(dims.len() + mets.len());

    for dim in dims {
        let col = quote_ident(&dim.name);
        if let Some(ref type_str) = dim.output_type {
            items.push(format!("    CAST({col} AS {type_str}) AS {col}"));
        } else {
            items.push(format!("    {col}"));
        }
    }

    for met in mets {
        let col = quote_ident(&met.name);
        if let Some(ref type_str) = met.output_type {
            items.push(format!("    CAST({col} AS {type_str}) AS {col}"));
        } else {
            items.push(format!("    {col}"));
        }
    }

    let mut sql = String::with_capacity(128);
    if mets.is_empty() {
        // Dims-only requests emit SELECT DISTINCT on the normal expansion
        // path; a routed query must match those semantics — a materialization
        // table containing duplicate rows would otherwise silently change
        // results (SG-11, code-review 2026-07-02).
        sql.push_str("SELECT DISTINCT\n");
    } else {
        sql.push_str("SELECT\n");
    }
    sql.push_str(&items.join(",\n"));
    sql.push_str("\nFROM ");
    sql.push_str(&qualify_and_quote_table_ref(table, def));
    sql
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::test_helpers::{orders_view, TestFixtureExt};
    use crate::model::{NullsOrder, SortOrder, WindowSpec};

    // Helper: create a QueryRequest-like set of resolved dims/mets from a def
    fn resolve_dims<'a>(def: &'a SemanticViewDefinition, names: &[&str]) -> Vec<&'a Dimension> {
        names
            .iter()
            .map(|n| {
                def.dimensions
                    .iter()
                    .find(|d| d.name.eq_ignore_ascii_case(n))
                    .unwrap_or_else(|| panic!("dimension '{n}' not found in def"))
            })
            .collect()
    }

    fn resolve_mets<'a>(def: &'a SemanticViewDefinition, names: &[&str]) -> Vec<&'a Metric> {
        names
            .iter()
            .map(|n| {
                def.metrics
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(n))
                    .unwrap_or_else(|| panic!("metric '{n}' not found in def"))
            })
            .collect()
    }

    // ================================================
    // MAT-05: Empty materializations fast path
    // ================================================

    #[test]
    fn empty_materializations_returns_none() {
        let def = orders_view();
        assert!(def.materializations.is_empty());
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        assert!(try_route_materialization(&def, &dims, &mets).is_none());
    }

    // ================================================
    // MAT-02: Exact match routing
    // ================================================

    #[test]
    fn exact_match_returns_some_sql() {
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue", "order_count"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        let sql = try_route_materialization(&def, &dims, &mets);
        assert!(sql.is_some(), "should match exact dims+mets");
        let sql = sql.unwrap();
        assert!(
            sql.contains("\"agg_table\""),
            "SQL should reference mat table: {sql}"
        );
        assert!(
            sql.contains("\"region\""),
            "SQL should select region: {sql}"
        );
        assert!(
            sql.contains("\"total_revenue\""),
            "SQL should select total_revenue: {sql}"
        );
        assert!(
            sql.contains("\"order_count\""),
            "SQL should select order_count: {sql}"
        );
    }

    #[test]
    fn case_insensitive_matching() {
        // Materialization dims/mets use different case than definition
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["Region"],
            &["Total_Revenue", "Order_Count"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        let sql = try_route_materialization(&def, &dims, &mets);
        assert!(sql.is_some(), "case-insensitive matching should work");
    }

    #[test]
    fn dimension_superset_in_mat_does_not_match() {
        // Materialization has MORE dimensions than requested
        let def = orders_view().with_materialization(
            "region_status_agg",
            "agg_table",
            &["region", "status"],
            &["total_revenue"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        assert!(
            try_route_materialization(&def, &dims, &mets).is_none(),
            "superset dims in mat should not match"
        );
    }

    #[test]
    fn metric_superset_in_mat_does_not_match() {
        // Materialization has MORE metrics than requested
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue", "order_count"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        assert!(
            try_route_materialization(&def, &dims, &mets).is_none(),
            "superset mets in mat should not match"
        );
    }

    #[test]
    fn dimension_subset_in_mat_does_not_match() {
        // Materialization has FEWER dimensions than requested
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        let dims = resolve_dims(&def, &["region", "status"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        assert!(
            try_route_materialization(&def, &dims, &mets).is_none(),
            "subset dims in mat should not match"
        );
    }

    // ================================================
    // MAT-03: No-match fallback
    // ================================================

    #[test]
    fn no_materialization_covers_request_returns_none() {
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        // Request different dims+mets than any materialization covers
        let dims = resolve_dims(&def, &["status"]);
        let mets = resolve_mets(&def, &["order_count"]);
        assert!(try_route_materialization(&def, &dims, &mets).is_none());
    }

    // ================================================
    // MAT-04: Semi-additive exclusion
    // ================================================

    #[test]
    fn semi_additive_metric_returns_none_even_with_matching_mat() {
        let def = orders_view()
            .with_non_additive_by(
                "total_revenue",
                &[("region", SortOrder::Desc, NullsOrder::Last)],
            )
            .with_materialization(
                "region_agg",
                "agg_table",
                &["region"],
                &["total_revenue", "order_count"],
            );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        assert!(
            try_route_materialization(&def, &dims, &mets).is_none(),
            "semi-additive metrics should exclude routing"
        );
    }

    // ================================================
    // MAT-04: Window metric exclusion
    // ================================================

    #[test]
    fn window_metric_returns_none_even_with_matching_mat() {
        let def = orders_view()
            .with_window_spec(
                "total_revenue",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "order_count".to_string(),
                    ..Default::default()
                },
            )
            .with_materialization(
                "region_agg",
                "agg_table",
                &["region"],
                &["total_revenue", "order_count"],
            );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        assert!(
            try_route_materialization(&def, &dims, &mets).is_none(),
            "window metrics should exclude routing"
        );
    }

    // ================================================
    // Output type casts (Pitfall 3)
    // ================================================

    #[test]
    fn output_type_on_dimension_produces_cast() {
        let mut def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        def.dimensions[0].output_type = Some("VARCHAR(50)".to_string());
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        let sql = try_route_materialization(&def, &dims, &mets).unwrap();
        assert!(
            sql.contains("CAST(\"region\" AS VARCHAR(50)) AS \"region\""),
            "should cast dimension with output_type: {sql}"
        );
    }

    #[test]
    fn output_type_on_metric_produces_cast() {
        let mut def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        def.metrics[0].output_type = Some("DOUBLE".to_string());
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        let sql = try_route_materialization(&def, &dims, &mets).unwrap();
        assert!(
            sql.contains("CAST(\"total_revenue\" AS DOUBLE) AS \"total_revenue\""),
            "should cast metric with output_type: {sql}"
        );
    }

    // ================================================
    // Multi-part table name quoting
    // ================================================

    #[test]
    fn multi_part_table_name_properly_quoted() {
        let def = orders_view().with_materialization(
            "region_agg",
            "catalog.schema.agg_table",
            &["region"],
            &["total_revenue"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        let sql = try_route_materialization(&def, &dims, &mets).unwrap();
        assert!(
            sql.contains("\"catalog\".\"schema\".\"agg_table\""),
            "multi-part table name should be quoted: {sql}"
        );
    }

    // ================================================
    // First-match wins (definition order)
    // ================================================

    #[test]
    fn first_matching_materialization_wins() {
        let def = orders_view()
            .with_materialization("first_agg", "first_table", &["region"], &["total_revenue"])
            .with_materialization(
                "second_agg",
                "second_table",
                &["region"],
                &["total_revenue"],
            );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        let sql = try_route_materialization(&def, &dims, &mets).unwrap();
        assert!(
            sql.contains("\"first_table\""),
            "first match should win: {sql}"
        );
        assert!(
            !sql.contains("\"second_table\""),
            "second match should not appear: {sql}"
        );
    }

    // ================================================
    // Dimensions-only query vs mat with both dims and mets
    // ================================================

    #[test]
    fn dimensions_only_query_does_not_match_mat_with_metrics() {
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets: Vec<&Metric> = vec![];
        assert!(
            try_route_materialization(&def, &dims, &mets).is_none(),
            "dims-only query should not match mat that has metrics"
        );
    }

    // ================================================
    // Dims-only routed queries apply SELECT DISTINCT (SG-11)
    // ================================================

    #[test]
    fn dims_only_routed_query_selects_distinct() {
        let def =
            orders_view().with_materialization("region_list", "region_table", &["region"], &[]);
        let dims = resolve_dims(&def, &["region"]);
        let mets: Vec<&Metric> = vec![];
        let sql = try_route_materialization(&def, &dims, &mets)
            .expect("dims-only mat should match dims-only query");
        assert!(
            sql.starts_with("SELECT DISTINCT"),
            "dims-only routed SQL must apply DISTINCT to match the raw \
             expansion path's semantics (SG-11): {sql}"
        );
    }

    #[test]
    fn routed_query_with_metrics_does_not_select_distinct() {
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        let sql = try_route_materialization(&def, &dims, &mets).expect("exact match should route");
        assert!(
            sql.starts_with("SELECT\n"),
            "routed SQL with metrics must not apply DISTINCT: {sql}"
        );
    }

    // ================================================
    // Metrics-only materialization matches metrics-only query
    // ================================================

    #[test]
    fn metrics_only_mat_matches_metrics_only_query() {
        let def = orders_view().with_materialization(
            "global_agg",
            "global_table",
            &[],
            &["total_revenue"],
        );
        let dims: Vec<&Dimension> = vec![];
        let mets = resolve_mets(&def, &["total_revenue"]);
        let sql = try_route_materialization(&def, &dims, &mets);
        assert!(
            sql.is_some(),
            "metrics-only mat should match metrics-only query"
        );
        let sql = sql.unwrap();
        assert!(
            sql.contains("\"global_table\""),
            "should reference global_table: {sql}"
        );
    }

    // ================================================
    // End-to-end via expand() -- matching materialization
    // ================================================

    #[test]
    fn end_to_end_expand_matching_mat_produces_simple_select() {
        use crate::expand::expand;
        use crate::expand::{DimensionName, MetricName, QueryRequest};

        let def = orders_view().with_materialization(
            "region_agg",
            "my_agg",
            &["region"],
            &["total_revenue"],
        );
        let req = QueryRequest {
            dimensions: vec![DimensionName::new("region")],
            metrics: vec![MetricName::new("total_revenue")],
            facts: vec![],
        };
        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("\"my_agg\""),
            "expand() should route to mat table: {sql}"
        );
        assert!(
            !sql.contains("GROUP BY"),
            "materialized query should not have GROUP BY: {sql}"
        );
    }

    // ================================================
    // End-to-end via expand() -- no match, standard expansion
    // ================================================

    // ================================================
    // find_routing_materialization_name tests (INTR-01)
    // ================================================

    #[test]
    fn find_name_returns_none_for_empty_materializations() {
        let def = orders_view();
        assert!(def.materializations.is_empty());
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue"]);
        assert!(find_routing_materialization_name(&def, &dims, &mets).is_none());
    }

    #[test]
    fn find_name_returns_matching_mat_name() {
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue", "order_count"],
        );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        assert_eq!(
            find_routing_materialization_name(&def, &dims, &mets),
            Some("region_agg")
        );
    }

    #[test]
    fn find_name_returns_none_for_no_match() {
        let def = orders_view().with_materialization(
            "region_agg",
            "agg_table",
            &["region"],
            &["total_revenue"],
        );
        let dims = resolve_dims(&def, &["status"]);
        let mets = resolve_mets(&def, &["order_count"]);
        assert!(find_routing_materialization_name(&def, &dims, &mets).is_none());
    }

    #[test]
    fn find_name_returns_none_for_semi_additive() {
        let def = orders_view()
            .with_non_additive_by(
                "total_revenue",
                &[("region", SortOrder::Desc, NullsOrder::Last)],
            )
            .with_materialization(
                "region_agg",
                "agg_table",
                &["region"],
                &["total_revenue", "order_count"],
            );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        assert!(find_routing_materialization_name(&def, &dims, &mets).is_none());
    }

    #[test]
    fn find_name_returns_none_for_window() {
        let def = orders_view()
            .with_window_spec(
                "total_revenue",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "order_count".to_string(),
                    ..Default::default()
                },
            )
            .with_materialization(
                "region_agg",
                "agg_table",
                &["region"],
                &["total_revenue", "order_count"],
            );
        let dims = resolve_dims(&def, &["region"]);
        let mets = resolve_mets(&def, &["total_revenue", "order_count"]);
        assert!(find_routing_materialization_name(&def, &dims, &mets).is_none());
    }

    // ================================================
    // End-to-end via expand() -- matching materialization
    // ================================================

    #[test]
    fn end_to_end_expand_no_match_produces_standard_expansion() {
        use crate::expand::expand;
        use crate::expand::{DimensionName, MetricName, QueryRequest};

        let def = orders_view().with_materialization(
            "region_agg",
            "my_agg",
            &["region"],
            &["total_revenue"],
        );
        // Request dims/mets that don't match the materialization
        let req = QueryRequest {
            dimensions: vec![DimensionName::new("region"), DimensionName::new("status")],
            metrics: vec![MetricName::new("total_revenue")],
            facts: vec![],
        };
        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            !sql.contains("\"my_agg\""),
            "should not route to mat table: {sql}"
        );
        assert!(
            sql.contains("GROUP BY"),
            "standard expansion should have GROUP BY: {sql}"
        );
    }
}
