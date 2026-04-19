//! Materialization routing for the expansion engine.
//!
//! When a query's requested dimensions and metrics exactly match a
//! declared materialization, the engine routes to the pre-aggregated
//! table instead of expanding raw sources with JOINs and GROUP BY.

use std::collections::HashSet;

use crate::model::{Dimension, Metric, SemanticViewDefinition};

use super::resolution::{quote_ident, quote_table_ref};

/// Attempt to route a query to a materialization table.
///
/// Returns `Some(sql)` if an exact-match materialization is found,
/// `None` if no match or if routing is excluded (semi-additive/window metrics).
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
pub(crate) fn try_route_materialization(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> Option<String> {
    // Fast path: no materializations declared -> None (MAT-05)
    if def.materializations.is_empty() {
        return None;
    }

    // MAT-04: Exclude semi-additive metrics from routing
    if resolved_mets.iter().any(|m| !m.non_additive_by.is_empty()) {
        return None;
    }

    // MAT-04: Exclude window function metrics from routing
    if resolved_mets.iter().any(|m| m.is_window()) {
        return None;
    }

    // Build requested dimension/metric name sets (lowercase for case-insensitive matching)
    let req_dims: HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    let req_mets: HashSet<String> = resolved_mets
        .iter()
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    // Scan materializations in definition order (first match wins)
    for mat in &def.materializations {
        let mat_dims: HashSet<String> = mat
            .dimensions
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        let mat_mets: HashSet<String> =
            mat.metrics.iter().map(|m| m.to_ascii_lowercase()).collect();

        if mat_dims == req_dims && mat_mets == req_mets {
            return Some(build_materialized_sql(
                &mat.table,
                resolved_dims,
                resolved_mets,
            ));
        }
    }

    None
}

/// Generate a SELECT from the materialization table.
///
/// The materialization table is expected to have columns named after the
/// dimension and metric names. The SQL simply selects them by name,
/// applying `output_type` casts when declared.
fn build_materialized_sql(table: &str, dims: &[&Dimension], mets: &[&Metric]) -> String {
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
    sql.push_str("SELECT\n");
    sql.push_str(&items.join(",\n"));
    sql.push_str("\nFROM ");
    sql.push_str(&quote_table_ref(table));
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
