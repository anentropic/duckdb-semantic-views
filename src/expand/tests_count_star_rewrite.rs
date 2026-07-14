//! COUNT(*) -> COUNT(pk) rewrite behaviour.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::count_star_rewrite_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::{orders_view, TestFixtureExt};
use crate::model::WindowSpec;

/// `orders` (base) + `line_items` child with a declared PK.
fn child_count_def() -> crate::model::SemanticViewDefinition {
    orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_dimension("region", "region", None)
        .with_table("li", "line_items", &["id"])
        .with_metric("item_count", "COUNT(*)", Some("li"))
        .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"])
}

#[test]
fn test_child_count_star_rewritten_exact_sql() {
    // SG-8: COUNT(*) on the LEFT-JOINed child must count the child's
    // PK, not NULL-extended rows (one per childless order).
    let def = child_count_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("item_count")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    let expected = "\
SELECT
    COUNT(\"li\".\"id\") AS \"item_count\"
FROM \"orders\" AS \"orders\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"orders\".\"id\"";
    assert_eq!(sql, expected);
}

#[test]
fn test_child_count_star_rewritten_with_base_dimension() {
    let def = child_count_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("item_count")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("COUNT(\"li\".\"id\") AS \"item_count\""),
        "child COUNT(*) must be rewritten to COUNT(pk): {sql}"
    );
    assert!(sql.contains("GROUP BY"), "grouped query expected: {sql}");
}

#[test]
fn test_base_table_count_star_unchanged() {
    // Metrics on the base table keep plain COUNT(*): the base table
    // is never NULL-extended by the synthesized LEFT JOINs.
    let def = child_count_def().with_metric("order_count", "COUNT(*)", Some("orders"));
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("order_count")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    let expected = "\
SELECT
    COUNT(*) AS \"order_count\"
FROM \"orders\" AS \"orders\"";
    assert_eq!(sql, expected);
}

#[test]
fn test_unqualified_count_star_metric_unchanged() {
    // Legacy single-table shape: metric declared without a source
    // table (None) is a base-table/derived metric — no rewrite.
    let def = orders_view();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("order_count")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("count(*) AS \"order_count\""),
        "COUNT(*) without a non-base source table must be preserved: {sql}"
    );
}

#[test]
fn test_child_count_star_without_pk_errors() {
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &[]) // no PK declared
        .with_metric("item_count", "COUNT(*)", Some("li"))
        .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("item_count")],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match &err {
        ExpandError::CountStarRequiresPrimaryKey {
            view_name,
            metric_name,
            table_alias,
        } => {
            assert_eq!(view_name, "orders");
            assert_eq!(metric_name, "item_count");
            assert_eq!(table_alias, "li");
        }
        other => panic!("Expected CountStarRequiresPrimaryKey, got: {other}"),
    }
    let msg = err.to_string();
    assert!(
        msg.contains("no PRIMARY KEY declared") && msg.contains("COUNT(*)"),
        "error must explain the rewrite requirement: {msg}"
    );
}

#[test]
fn test_unrelated_metric_still_works_when_sibling_count_star_lacks_pk() {
    // The no-PK failure is scoped to queries that actually use the
    // metric: other metrics on the same view keep working.
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &[]) // no PK declared
        .with_metric("item_count", "COUNT(*)", Some("li"))
        .with_metric("revenue", "SUM(li.amount)", Some("li"))
        .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("revenue")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(sql.contains("SUM(li.amount)"), "SQL: {sql}");
}

#[test]
fn test_derived_metric_reaching_no_pk_count_star_errors() {
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &[]) // no PK declared
        .with_metric("item_count", "COUNT(*)", Some("li"))
        .with_metric("double_items", "item_count * 2", None)
        .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("double_items")],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match &err {
        ExpandError::CountStarRequiresPrimaryKey { metric_name, .. } => {
            assert_eq!(
                metric_name, "item_count",
                "error must name the failing base metric"
            );
        }
        other => panic!("Expected CountStarRequiresPrimaryKey, got: {other}"),
    }
}

#[test]
fn test_derived_metric_inherits_rewritten_count_star() {
    let def = child_count_def().with_metric("double_items", "item_count * 2", None);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("double_items")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("(COUNT(\"li\".\"id\")) * 2 AS \"double_items\""),
        "derived metric must inline the REWRITTEN child count: {sql}"
    );
}

#[test]
fn test_window_inner_aggregate_gets_rewrite() {
    // Window path: the inner aggregate is emitted from the shared
    // resolved expressions, so the rewrite must appear in the CTE.
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &["id"])
        .with_dimension("product", "li.product", Some("li"))
        .with_metric("item_count", "COUNT(*)", Some("li"))
        .with_metric("rolling_items", "AVG(item_count)", None)
        .with_window_spec(
            "rolling_items",
            WindowSpec {
                window_function: "AVG".to_string(),
                inner_metric: "item_count".to_string(),
                ..Default::default()
            },
        )
        .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("product")],
        metrics: vec![MetricName::new("rolling_items")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("COUNT(\"li\".\"id\") AS \"item_count\""),
        "window CTE inner aggregate must use the rewritten count: {sql}"
    );
}

#[test]
fn test_semi_additive_co_query_uses_rewritten_count() {
    // Semi-additive path: a same-grain COUNT(*) co-metric on the
    // child table decomposes into a CTE capture of the child PK and
    // an outer COUNT over it (NULL-extended rows excluded). The
    // base-table COUNT(*) rejection in parse_snapshot_aggregate is
    // untouched (covered by semi_additive tests).
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_dimension("customer_id", "customer_id", None)
        .with_table("li", "line_items", &["id"])
        .with_dimension("report_date", "li.report_date", Some("li"))
        .with_metric("balance", "SUM(li.balance)", Some("li"))
        .with_metric("txn_count", "COUNT(*)", Some("li"))
        .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"])
        .with_non_additive_by(
            "balance",
            &[(
                "report_date",
                crate::model::SortOrder::Desc,
                crate::model::NullsOrder::First,
            )],
        );
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("customer_id")],
        metrics: vec![MetricName::new("balance"), MetricName::new("txn_count")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("\"li\".\"id\" AS \"__sv_reg_1\""),
        "CTE must capture the rewritten count argument: {sql}"
    );
    assert!(
        sql.contains("COUNT(\"__sv_reg_1\") AS \"txn_count\""),
        "outer select must re-aggregate the captured PK column: {sql}"
    );
}
