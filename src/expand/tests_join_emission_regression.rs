//! Join-clause emission ordering regressions.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::join_emission_regression_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::TestFixtureExt;
use crate::model::SemanticViewDefinition;

/// li (base) -> o -> c chain with configurable relationship
/// declaration order.
fn li_o_c_def(o_to_c_first: bool) -> SemanticViewDefinition {
    let def = SemanticViewDefinition::default()
        .with_table("li", "line_items", &["id"])
        .with_table("o", "orders", &["id"])
        .with_table("c", "customers", &["id"])
        .with_dimension("customer_name", "c.name", Some("c"))
        .with_metric("total_qty", "sum(li.qty)", Some("li"));
    if o_to_c_first {
        def.with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
            .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
    } else {
        def.with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
            .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
    }
}

/// SG-2: the join emitted for alias `o` must be the edge that connects
/// `o` to the already-emitted base (li -> o), not whichever declared
/// relationship mentions `o` first (o -> c would forward-reference the
/// not-yet-joined `c`). Both declaration orders must produce identical
/// SQL with no forward references and no dropped joins.
#[test]
fn sg2_join_selection_is_declaration_order_independent() {
    let expected = "\
SELECT
    c.name AS \"customer_name\",
    sum(li.qty) AS \"total_qty\"
FROM \"line_items\" AS \"li\"
LEFT JOIN \"orders\" AS \"o\" ON \"li\".\"order_id\" = \"o\".\"id\"
LEFT JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\"
GROUP BY
    1";
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("customer_name")],
        metrics: vec![MetricName::new("total_qty")],
    };
    let sql_a = expand("test", &li_o_c_def(true), &req).unwrap();
    let sql_b = expand("test", &li_o_c_def(false), &req).unwrap();
    assert_eq!(sql_a, expected, "o->c declared first must be correct");
    assert_eq!(sql_b, expected, "li->o declared first must be correct");
}

/// SG-2: a child table (`li`) with FKs to two parents (`p` declared
/// first, then `o` = base). Query needing only the li -> o edge must
/// emit it — not the first-declared li -> p edge, whose ON clause
/// would reference the never-joined `p`.
#[test]
fn sg2_two_parent_child_picks_connecting_edge() {
    let def = SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_table("p", "products", &["id"])
        .with_dimension("region", "o.region", Some("o"))
        .with_metric("qty", "sum(li.qty)", Some("li"))
        .with_pkfk_join("li_to_p", "li", "p", &["product_id"], &["id"])
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("qty")],
    };
    let sql = expand("test", &def, &req).unwrap();
    let expected = "\
SELECT
    o.region AS \"region\",
    sum(li.qty) AS \"qty\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"o\".\"id\"
GROUP BY
    1";
    assert_eq!(sql, expected, "must join li via li->o, not li->p");
    assert!(
        !sql.contains("\"p\"."),
        "ON clause must not reference the never-joined p: {sql}"
    );
}

/// ld -> li -> o (base) chain: metric two hops below the root.
fn ld_li_o_def() -> SemanticViewDefinition {
    SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_table("ld", "line_item_details", &["id"])
        .with_dimension("region", "o.region", Some("o"))
        .with_metric("detail_qty", "sum(ld.qty)", Some("ld"))
        .with_fact("detail_amount", "ld.amount", "ld")
        .with_pkfk_join("ld_to_li", "ld", "li", &["line_item_id"], &["id"])
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
}

/// SG-10: a needed table two hops below the root must pull in its
/// intermediate (`li`) and join in dependency order (li before ld),
/// with each ON clause referencing only already-joined tables.
#[test]
fn sg10_fk_side_chain_includes_intermediate_join() {
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("detail_qty")],
    };
    let sql = expand("test", &ld_li_o_def(), &req).unwrap();
    let expected = "\
SELECT
    o.region AS \"region\",
    sum(ld.qty) AS \"detail_qty\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"o\".\"id\"
LEFT JOIN \"line_item_details\" AS \"ld\" ON \"ld\".\"line_item_id\" = \"li\".\"id\"
GROUP BY
    1";
    assert_eq!(sql, expected);

    // Reversed declaration order must produce the same SQL.
    let mut def_rev = ld_li_o_def();
    def_rev.joins.reverse();
    let sql_rev = expand("test", &def_rev, &req).unwrap();
    assert_eq!(sql_rev, expected, "declaration order must not matter");
}

/// SG-10 (facts path): `expand_facts` previously joined only the
/// fact's direct source table with no path walk; the intermediate
/// `li` join was missing entirely.
#[test]
fn sg10_fact_source_chain_includes_intermediate_join() {
    let req = QueryRequest {
        where_clause: None,
        facts: vec![FactName::new("detail_amount")],
        dimensions: vec![],
        metrics: vec![],
    };
    let sql = expand("test", &ld_li_o_def(), &req).unwrap();
    let expected = "\
SELECT
    ld.amount AS \"detail_amount\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"o\".\"id\"
LEFT JOIN \"line_item_details\" AS \"ld\" ON \"ld\".\"line_item_id\" = \"li\".\"id\"";
    assert_eq!(sql, expected);
}

/// SG-12: a user table alias containing `__` is a bare alias, not a
/// role-playing scoped alias. It must be joined normally — previously
/// the emitter re-parsed the alias at the first `__`, looked up a
/// relationship named after the suffix, and silently dropped the join.
#[test]
fn sg12_bare_alias_containing_double_underscore_joins_normally() {
    let def = SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("my__dim", "dim_table", &["id"])
        .with_dimension("dim_name", "my__dim.name", Some("my__dim"))
        .with_metric("cnt", "count(*)", Some("o"))
        .with_pkfk_join("o_to_dim", "o", "my__dim", &["dim_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("dim_name")],
        metrics: vec![MetricName::new("cnt")],
    };
    let sql = expand("test", &def, &req).unwrap();
    let expected = "\
SELECT
    my__dim.name AS \"dim_name\",
    count(*) AS \"cnt\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"dim_table\" AS \"my__dim\" ON \"o\".\"dim_id\" = \"my__dim\".\"id\"
GROUP BY
    1";
    assert_eq!(sql, expected);
}

/// SG-12: role-playing scoped aliases keep the documented
/// `{alias}__{relationship}` SQL alias format, and the scoped alias is
/// used on the PK side of the ON clause.
#[test]
fn sg12_role_playing_scoped_alias_format_preserved() {
    let def = SemanticViewDefinition::default()
        .with_table("f", "flights", &["flight_id"])
        .with_table("a", "airports", &["airport_code"])
        .with_dimension("city", "a.city", Some("a"))
        .with_metric("departure_count", "COUNT(*)", Some("f"))
        .with_using_relationship("departure_count", &["dep_airport"])
        .with_pkfk_join(
            "dep_airport",
            "f",
            "a",
            &["departure_code"],
            &["airport_code"],
        )
        .with_pkfk_join(
            "arr_airport",
            "f",
            "a",
            &["arrival_code"],
            &["airport_code"],
        );
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![MetricName::new("departure_count")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
        "Scoped alias format {{alias}}__{{rel}} must be preserved: {sql}"
    );
    assert!(
        sql.contains("\"f\".\"departure_code\" = \"a__dep_airport\".\"airport_code\""),
        "ON clause must use the scoped alias on the PK side: {sql}"
    );
}

// ---------------------------------------------------------------------------
// PAR-3: joins come from declared `source_table`, never from scanning an
// expression for foreign aliases
// ---------------------------------------------------------------------------

/// PAR-3 (code-review 2026-08-03), **pinning a documented limitation — see
/// TECH-DEBT #52**, not behaviour to preserve.
///
/// A member expression may reference base-table columns of its own logical
/// table only; a raw column of *another* logical table (`c.discount` on a
/// metric declared `source_table = o`) is not a supported reference. Snowflake
/// rejects the same shape — "Expressions cannot refer to base table columns
/// from other tables" — but rejects it at CREATE time, while here the DDL is
/// accepted and the expression is emitted verbatim, pulling no join for `c`.
/// DuckDB then raises a binder error for the unknown alias at query time.
///
/// The number is never wrong: `c` is alias-qualified, so with no `c` in scope
/// the query cannot bind, and the failure is loud. What is missing is the
/// CREATE-time validation that would name the real problem. TECH-DEBT #52
/// records that; this test exists so the interim behaviour is visible rather
/// than merely absent, and must be *replaced* — not deleted — when the
/// validator lands.
#[test]
fn par3_cross_table_column_reference_pulls_no_join() {
    let def = SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("c", "customers", &["id"])
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("mixed_margin", "SUM(o.amount - c.discount)", Some("o"))
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("order_status")],
        metrics: vec![MetricName::new("mixed_margin")],
    };
    let sql = expand("test", &def, &req).expect("the DDL and the query are both accepted today");
    assert!(
        !sql.contains("customers"),
        "the `c.discount` reference must not pull a join — joins come from \
         `source_table` alone (TECH-DEBT #52): {sql}"
    );
    assert!(
        sql.contains("SUM(o.amount - c.discount)"),
        "the expression is emitted verbatim, leaving `c` unbound: {sql}"
    );
}

/// PAR-6 (found while verifying PAR-3, TECH-DEBT #53). The *named-fact*
/// cross-table form is the one Snowflake supports ("define facts on source
/// tables, then refer to these expressions from connected logical tables") and
/// the one this codebase built deliberately: `fact_replacement_map` keys each
/// fact by its own `source_table.name` so that "a fact referenced across tables
/// in its own-qualified form is then actually inlined, not just detected".
///
/// Only the inlining half was wired up. `join_resolver` collected aliases from
/// each member's declared `source_table` and from *queried* facts, never from
/// facts a metric merely references — so `c.discount` was spliced into a metric
/// on `o` while `customers` stayed out of the FROM clause, and the emitted SQL
/// could not bind. The referenced fact's table is now collected too.
#[test]
fn par6_cross_table_fact_reference_pulls_its_join() {
    let def = SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("c", "customers", &["id"])
        .with_fact("cust_discount", "c.discount", "c")
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("mixed_margin", "SUM(o.amount - c.cust_discount)", Some("o"))
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("order_status")],
        metrics: vec![MetricName::new("mixed_margin")],
    };
    let sql = expand("test", &def, &req).expect("a cross-table fact reference is answerable");
    assert!(
        sql.contains("(c.discount)"),
        "the referenced fact must be inlined at its reference site: {sql}"
    );
    assert!(
        sql.contains(r#"LEFT JOIN "customers" AS "c""#),
        "the referenced fact's table must be joined so the expression binds: {sql}"
    );
}

/// The same reference, but the fact sits on a table that **fans** relative to
/// the metric's: `li` is a child of `o`, so joining it multiplies each order
/// row and `SUM(o.amount - …)` would count an order once per line item.
///
/// Joining is only safe when the path from the metric's table to the fact's is
/// non-fanning, which is the same rule the fence already applies to a queried
/// dimension and to a `where_clause` member. A fanning one errors rather than
/// silently inflating — the failure PAR-6's fix would otherwise introduce, since
/// before it this query raised no error only because the join was missing.
#[test]
fn par6_cross_table_fact_on_a_fanning_table_errors() {
    let def = SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("li", "line_items", &["id"])
        .with_fact("item_cost", "li.cost", "li")
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("bad_margin", "SUM(o.amount - li.item_cost)", Some("o"))
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("order_status")],
        metrics: vec![MetricName::new("bad_margin")],
    };
    let err = expand("test", &def, &req)
        .expect_err("a fact on a fanning table must not be joined into the aggregate");
    let msg = err.to_string();
    assert!(
        msg.contains("fan trap detected"),
        "must report the fan trap rather than emitting an inflated aggregate: {msg}"
    );
    assert!(
        msg.contains("item_cost") && msg.contains("li_to_o"),
        "the message must name the referenced fact and the fanning relationship: {msg}"
    );
}

/// The form that *does* work today, and therefore the only cross-table
/// workaround worth documenting: query the fact directly. `fact_source_tables`
/// feeds `join_resolver`, so the fact's table is joined. Pinned next to #53 so
/// that a regression here would not leave the limitation without any escape.
#[test]
fn queried_cross_table_fact_pulls_its_join() {
    let def = SemanticViewDefinition::default()
        .with_table("o", "orders", &["id"])
        .with_table("c", "customers", &["id"])
        .with_fact("cust_discount", "c.discount", "c")
        .with_dimension("order_status", "o.status", Some("o"))
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"]);
    let req = QueryRequest {
        where_clause: None,
        facts: vec![FactName::new("cust_discount")],
        dimensions: vec![DimensionName::new("order_status")],
        metrics: vec![],
    };
    let sql = expand("test", &def, &req).expect("a queried cross-table fact is supported");
    assert!(
        sql.contains("LEFT JOIN \"customers\" AS \"c\""),
        "a queried fact must pull its own table's join: {sql}"
    );
}
