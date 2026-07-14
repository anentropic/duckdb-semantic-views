//! Fact-path role-playing resolution.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::facts_path_role_playing_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::{orders_view, TestFixtureExt};

fn role_playing_facts_def(two_rels: bool) -> crate::model::SemanticViewDefinition {
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("a", "airports", &["code"])
        .with_dimension("city", "a.city", Some("a"))
        .with_fact("order_note", "orders.note", "orders")
        .with_pkfk_join("dep_airport", "orders", "a", &["dep_code"], &["code"]);
    if two_rels {
        def.with_pkfk_join("arr_airport", "orders", "a", &["arr_code"], &["code"])
    } else {
        def
    }
}

#[test]
fn test_facts_path_role_playing_dimension_raises_ambiguous_path() {
    // SG-17: the facts path must run the same role-playing ambiguity
    // detection as the metrics path. With two named relationships to
    // `a` and no USING context (facts cannot supply one), the
    // dimension is ambiguous — previously it silently bound to an
    // arbitrary edge.
    let def = role_playing_facts_def(true);
    let req = QueryRequest {
        facts: vec!["order_note".to_string()],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match err {
        ExpandError::AmbiguousPath {
            dimension_name,
            dimension_table,
            available_relationships,
            ..
        } => {
            assert_eq!(dimension_name, "city");
            assert_eq!(dimension_table, "a");
            assert!(
                available_relationships.contains(&"dep_airport".to_string())
                    && available_relationships.contains(&"arr_airport".to_string()),
                "both relationships must be listed: {available_relationships:?}"
            );
        }
        other => panic!("Expected AmbiguousPath, got: {other}"),
    }
}

#[test]
fn test_facts_path_single_relationship_dimension_ok() {
    let def = role_playing_facts_def(false);
    let req = QueryRequest {
        facts: vec!["order_note".to_string()],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a\""),
        "single relationship stays unambiguous: {sql}"
    );
}

#[test]
fn test_facts_path_convergent_parent_dimension_not_ambiguous() {
    // Two relationships converging on the same target from DIFFERENT
    // source tables (`li -> orders`, `pay -> orders`) is NOT
    // role-playing: the parent joins as one bare instance and the
    // path walk picks the unique connecting edge. The SG-17 check
    // over-fired here (it counted inbound relationships without
    // grouping by source), breaking plain child-fact +
    // parent-dimension queries — the regression surfaced in
    // test/sql/phase46_fact_query.test (p46f_fan_test).
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &["id"])
        .with_table("pay", "payments", &["id"])
        .with_dimension("region", "orders.region", Some("orders"))
        .with_fact("net_price", "li.price", "li")
        .with_pkfk_join("li_to_o", "li", "orders", &["order_id"], &["id"])
        .with_pkfk_join("pay_to_o", "pay", "orders", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec!["net_price".to_string()],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![],
    };
    let sql = expand("orders", &def, &req).expect("convergent parent must not raise AmbiguousPath");
    assert!(sql.contains("net_price"), "fact survives: {sql}");
}

#[test]
fn test_metrics_path_convergent_parent_dimension_not_ambiguous() {
    // Same shape through the metrics path (find_using_context is
    // shared): a metric on one child + a dimension on the shared
    // parent, no USING context anywhere.
    let def = orders_view()
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &["id"])
        .with_table("pay", "payments", &["id"])
        .with_dimension("region", "orders.region", Some("orders"))
        .with_metric("revenue", "SUM(li.price)", Some("li"))
        .with_pkfk_join("li_to_o", "li", "orders", &["order_id"], &["id"])
        .with_pkfk_join("pay_to_o", "pay", "orders", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("revenue")],
    };
    let sql = expand("orders", &def, &req).expect("convergent parent must not raise AmbiguousPath");
    assert!(sql.contains("SUM"), "metric survives: {sql}");
}
