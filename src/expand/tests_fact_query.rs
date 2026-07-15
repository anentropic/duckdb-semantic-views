//! Fact query expansion (unaggregated row-level SELECT).
//!
//! Extracted from `sql_gen.rs`'s `mod tests::phase46_fact_query_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::TestFixtureExt;
use crate::model::SemanticViewDefinition;

/// Build a multi-table def: orders (o) -> line_items (li), with a dim on o and facts on li.
fn multi_table_def() -> SemanticViewDefinition {
    SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_dimension("region", "o.region", Some("o"))
        .with_fact("net_price", "li.price * (1 - li.discount)", "li")
        .with_metric("total_revenue", "sum(li.price)", Some("li"))
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
}

#[test]
fn test_fact_query_basic() {
    let def = multi_table_def();
    let req = QueryRequest {
        facts: vec![FactName::new("net_price")],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![],
    };
    let sql = expand("test_view", &def, &req).unwrap();
    assert!(
        !sql.contains("GROUP BY"),
        "Fact queries must NOT have GROUP BY: {sql}"
    );
    assert!(sql.contains("o.region"), "Must include dim expr: {sql}");
    assert!(
        sql.contains("li.price * (1 - li.discount)"),
        "Must include fact expr: {sql}"
    );
    assert!(sql.contains("FROM"), "Must have FROM clause: {sql}");
    assert!(sql.contains("LEFT JOIN"), "Must include JOIN for li: {sql}");
}

#[test]
fn test_fact_query_no_dimensions() {
    let def = multi_table_def();
    let req = QueryRequest {
        facts: vec![FactName::new("net_price")],
        dimensions: vec![],
        metrics: vec![],
    };
    let sql = expand("test_view", &def, &req).unwrap();
    assert!(
        !sql.contains("GROUP BY"),
        "Fact queries must NOT have GROUP BY: {sql}"
    );
    assert!(
        sql.contains("li.price * (1 - li.discount)"),
        "Must include fact expr: {sql}"
    );
    assert!(
        !sql.contains("DISTINCT"),
        "Fact queries without dims should not use DISTINCT: {sql}"
    );
}

#[test]
fn test_fact_query_inline_facts() {
    let def = SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_fact("net_price", "li.price * (1 - li.discount)", "li")
        .with_fact("line_total", "net_price * li.quantity", "li")
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![FactName::new("line_total")],
        dimensions: vec![],
        metrics: vec![],
    };
    let sql = expand("test_view", &def, &req).unwrap();
    // line_total's expression should have net_price inlined (parenthesized)
    assert!(
        sql.contains("(li.price * (1 - li.discount))"),
        "Must inline net_price into line_total: {sql}"
    );
}

#[test]
fn test_fact_query_unknown_fact() {
    let def = multi_table_def();
    let req = QueryRequest {
        facts: vec![FactName::new("nonexistent")],
        dimensions: vec![],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ExpandError::UnknownFact { .. }),
        "Expected UnknownFact, got: {err}"
    );
}

#[test]
fn test_fact_query_duplicate_fact() {
    let def = multi_table_def();
    let req = QueryRequest {
        facts: vec![FactName::new("net_price"), FactName::new("net_price")],
        dimensions: vec![],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ExpandError::DuplicateFact { .. }),
        "Expected DuplicateFact, got: {err}"
    );
}

#[test]
fn test_fact_query_private_fact() {
    let def = multi_table_def().with_private_fact("raw_price", "li.price", "li");
    let req = QueryRequest {
        facts: vec![FactName::new("raw_price")],
        dimensions: vec![],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ExpandError::PrivateFact { .. }),
        "Expected PrivateFact, got: {err}"
    );
}

#[test]
fn test_fact_path_violation() {
    // Fan shape: o -> li, o -> payments (divergent paths)
    let def = SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_table("p", "payments", &["id"])
        .with_fact("net_price", "li.price * (1 - li.discount)", "li")
        .with_dimension("pay_status", "CAST(p.amount AS VARCHAR)", Some("p"))
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
        .with_pkfk_join("p_to_o", "p", "o", &["order_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![FactName::new("net_price")],
        dimensions: vec![DimensionName::new("pay_status")],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, ExpandError::FactPathViolation { .. }),
        "Expected FactPathViolation, got: {err}"
    );
}

#[test]
fn test_fact_path_valid_linear() {
    // Chain: o -> li -> details (linear path)
    let def = SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_table("d", "details", &["id"])
        .with_fact("detail_val", "d.value", "d")
        .with_dimension("region", "o.region", Some("o"))
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
        .with_pkfk_join("d_to_li", "d", "li", &["line_id"], &["id"]);
    let req = QueryRequest {
        facts: vec![FactName::new("detail_val")],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(result.is_ok(), "Linear path should be valid: {result:?}");
}

#[test]
fn test_fact_query_with_output_type() {
    let mut def = multi_table_def();
    def.facts[0].output_type = Some("DECIMAL(10,2)".to_string());
    let req = QueryRequest {
        facts: vec![FactName::new("net_price")],
        dimensions: vec![],
        metrics: vec![],
    };
    let sql = expand("test_view", &def, &req).unwrap();
    assert!(
        sql.contains("CAST("),
        "Must wrap fact in CAST when output_type is set: {sql}"
    );
    assert!(
        sql.contains("DECIMAL(10,2)"),
        "Must include output type: {sql}"
    );
}
