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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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

/// Fan-in onto the base table: `li` and `s` both reference the base table `o`,
/// which in turn references `c`.
///
/// This is the only legal fan-in — `RelationshipGraph::check_no_diamonds`
/// exempts the root and rejects every other multi-parent node as an ambiguous
/// join diamond.
///
/// TECH-DEBT #37: the parent map used to take `reverse[node].first()`, so the
/// base table was handed a "parent" — whichever child was declared first — and
/// chains ran through the root and out the far branch. `c`'s chain became
/// `[c, o, li]`, which never reaches `s`.
fn fan_in_def() -> SemanticViewDefinition {
    SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_table("s", "shipments", &["id"])
        .with_table("c", "customers", &["id"])
        .with_fact("ship_cost", "s.cost", "s")
        .with_fact("line_price", "li.price", "li")
        .with_dimension("carrier", "s.carrier", Some("s"))
        .with_dimension("country", "c.country", Some("c"))
        // Declared first -- this ordering is what gave the root its bogus parent.
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
        .with_pkfk_join("s_to_o", "s", "o", &["order_id"], &["id"])
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
}

#[test]
fn fact_with_dimension_across_fan_in_is_allowed() {
    // TECH-DEBT #37: `s -> o -> c` is many-to-one at every hop, so a fact on the
    // base table with a dimension on `c` joins without multiplying rows. The
    // unrelated fan-in sibling `li` must not hide `c` from it.
    let def = fan_in_def();
    let req = QueryRequest {
        where_clause: None,
        facts: vec![FactName::new("ship_cost")],
        dimensions: vec![DimensionName::new("country")],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(
        result.is_ok(),
        "s -> o -> c is safe at every hop; the fan-in sibling must not cause a \
         FactPathViolation: {result:?}"
    );
}

#[test]
fn fact_across_fan_in_siblings_still_violates() {
    // The over-widening guard. `li` and `s` are the two fan-in siblings:
    // reaching one from the other traverses a many-to-one edge BACKWARDS
    // (`o -> li` or `o -> s`), which multiplies rows. Once the parent map is
    // rooted at the base table both siblings share the ancestor `o`, so a check
    // that only asked about ancestry would be tempted to accept them — the path
    // must be checked for fan-out DIRECTION, not merely for connectivity.
    let def = fan_in_def();
    let req = QueryRequest {
        where_clause: None,
        facts: vec![FactName::new("line_price")],
        dimensions: vec![DimensionName::new("carrier")],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(
        matches!(result, Err(ExpandError::FactPathViolation { .. })),
        "Expected FactPathViolation for fan-in siblings li/s, got: {result:?}"
    );
}

#[test]
fn fact_reaches_a_multi_hop_chain_across_fan_in() {
    // The safe direction holds for more than one hop past the fan-in:
    // s -> o -> c -> r is many-to-one throughout.
    let def = fan_in_def()
        .with_table("r", "regions", &["id"])
        .with_dimension("region_name", "r.name", Some("r"))
        .with_pkfk_join("c_to_r", "c", "r", &["region_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![FactName::new("ship_cost")],
        dimensions: vec![DimensionName::new("region_name")],
        metrics: vec![],
    };
    let result = expand("test_view", &def, &req);
    assert!(
        result.is_ok(),
        "s -> o -> c -> r is safe at every hop: {result:?}"
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
        where_clause: None,
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
        where_clause: None,
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
