//! Per-grain ("own-grain") metric aggregation — the Snowflake-parity path
//! (TECH-DEBT #35, v0.12.0).
//!
//! v0.11.0 closed the *silent-wrong-answer* half of the multi-grain problem: a
//! metric whose grain is not the base-table grain raised `RootGrainFanTrap` /
//! `MetricFanTrap` instead of aggregating over the multiplied join. These tests
//! pin the other half — those queries are now **answered**, by pre-aggregating
//! each metric at its own grain in a CTE and joining the results, the way
//! Snowflake computes them.
//!
//! The fence itself is unchanged for everything per-grain does not supersede: a
//! dimension BELOW a metric's grain still errors (see the `still_errors` tests),
//! and `fan_trap.rs`'s own unit tests keep pinning the base-anchored fence used
//! by the paths that are not per-grain eligible.

use super::*;
use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
use crate::model::{Cardinality, SemanticViewDefinition};

/// Rename the base table declared by [`minimal_def`] so the fixture's physical
/// table name differs from its alias (`"orders" AS "o"`), the shape real DDL
/// produces — and the shape that makes an anchor assertion meaningful.
fn base_table(mut def: SemanticViewDefinition, table: &str, pk: &str) -> SemanticViewDefinition {
    def.tables[0].table = table.to_string();
    def.tables[0].pk_columns = vec![pk.to_string()];
    def
}

/// root `o` (orders) --customer_id--> `c` (customers).
///
/// `c` is a PARENT of the base table: the base-anchored `FROM orders LEFT JOIN
/// customers` duplicates each customer row once per order, so any metric on `c`
/// is inflated there (EXP-1). Its own grain is `customers`.
fn orders_with_parent_customers() -> SemanticViewDefinition {
    base_table(minimal_def("o", "d", "d", "m", "count(*)"), "orders", "id")
        .clear_dimensions()
        .clear_metrics()
        .with_table("c", "customers", &["id"])
        .with_dimension("segment", "c.segment", Some("c"))
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("total_balance", "SUM(c.balance)", Some("c"))
        .with_metric("order_count", "COUNT(*)", Some("o"))
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
}

/// root `o` (orders) <--order_id-- `li` (line_items): `li` is a CHILD of the
/// base table (the "many" side). A metric on `o` and a metric on `li` are at
/// different grains separated by a fan-out edge — the classic two-fact-table
/// query, `MetricFanTrap` before v0.12.0.
fn orders_with_child_line_items() -> SemanticViewDefinition {
    base_table(minimal_def("o", "d", "d", "m", "count(*)"), "orders", "id")
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &["id"])
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("order_total", "SUM(o.amount)", Some("o"))
        .with_metric("item_count", "COUNT(li.id)", Some("li"))
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
}

/// The same two-fact shape, but the child table declares **no** PRIMARY KEY and
/// its metric is a bare `COUNT(*)` — the shape the base-anchored path cannot
/// emit safely (`CountStarRequiresPrimaryKey`, SG-8) but per-grain can, because
/// the child anchors its own CTE instead of being LEFT JOINed.
fn orders_with_pkless_child() -> SemanticViewDefinition {
    base_table(minimal_def("o", "d", "d", "m", "count(*)"), "orders", "id")
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &[])
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("order_total", "SUM(o.amount)", Some("o"))
        .with_metric("item_count", "COUNT(*)", Some("li"))
        .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
}

fn req(dims: &[&str], mets: &[&str]) -> QueryRequest {
    QueryRequest {
        facts: vec![],
        dimensions: dims.iter().map(|d| DimensionName::new(*d)).collect(),
        metrics: mets.iter().map(|m| MetricName::new(*m)).collect(),
    }
}

// ---------------------------------------------------------------------------
// GRAIN-01 / GRAIN-02: a metric on a parent of the base table
// ---------------------------------------------------------------------------

/// GRAIN-01: queried alone, a parent-table metric is aggregated over its OWN
/// table — not over `FROM orders LEFT JOIN customers` (which counts each
/// customer once per order). v0.11.0 raised `RootGrainFanTrap` here.
#[test]
fn parent_grain_metric_alone_aggregates_at_its_own_grain() {
    let def = orders_with_parent_customers();
    let sql = expand("sales", &def, &req(&[], &["total_balance"]))
        .expect("a parent-grain metric must be answerable at its own grain");
    assert!(
        sql.contains(r#"FROM "customers" AS "c""#),
        "must be anchored at the metric's own table, got:\n{sql}"
    );
    assert!(
        !sql.contains("orders"),
        "the base table must not appear — joining it is what inflated the metric:\n{sql}"
    );
    assert!(
        sql.contains(r#"SUM(c.balance) AS "total_balance""#),
        "must emit the metric expression, got:\n{sql}"
    );
}

/// GRAIN-02: grouping that metric by a dimension on its own table groups at its
/// own grain.
#[test]
fn parent_grain_metric_with_own_table_dimension() {
    let def = orders_with_parent_customers();
    let sql = expand("sales", &def, &req(&["segment"], &["total_balance"]))
        .expect("parent-grain metric + parent-grain dimension must be answerable");
    assert!(
        sql.contains(r#"FROM "customers" AS "c""#),
        "anchored at customers, got:\n{sql}"
    );
    assert!(
        sql.contains("GROUP BY"),
        "must group by the dimension:\n{sql}"
    );
    assert!(!sql.contains("orders"), "no base-table join:\n{sql}");
}

// ---------------------------------------------------------------------------
// GRAIN-03 / GRAIN-05: two metrics at different grains
// ---------------------------------------------------------------------------

/// GRAIN-03 + GRAIN-05: a base-grain metric and a parent-grain metric queried
/// together, with no dimensions: each is computed in its own CTE and the two
/// one-row results are combined with a CROSS JOIN.
#[test]
fn metrics_at_two_grains_no_dimensions_cross_join() {
    let def = orders_with_parent_customers();
    let sql = expand("sales", &def, &req(&[], &["order_count", "total_balance"]))
        .expect("two grains must be answerable per-grain");
    assert!(
        sql.starts_with("WITH __sv_grain_0 AS ("),
        "expected per-grain CTEs, got:\n{sql}"
    );
    assert!(
        sql.contains("__sv_grain_1"),
        "expected a second grain CTE, got:\n{sql}"
    );
    assert!(
        sql.contains("CROSS JOIN"),
        "dimensionless grain results combine with CROSS JOIN, got:\n{sql}"
    );
    assert!(
        !sql.contains("FULL OUTER JOIN"),
        "no dimensions to join on, got:\n{sql}"
    );
}

/// GRAIN-03: the two-fact-table shape — a metric on the base table and a metric
/// on a child table, grouped by a base-table dimension. Each grain aggregates
/// separately, then the results are joined on the dimension with a NULL-safe
/// comparison so a group present on only one side survives.
#[test]
fn metrics_at_two_grains_with_dimension_full_outer_join() {
    let def = orders_with_child_line_items();
    let sql = expand(
        "sales",
        &def,
        &req(&["order_status"], &["order_total", "item_count"]),
    )
    .expect("base-grain + child-grain metrics must be answerable per-grain");
    assert!(
        sql.contains("FULL OUTER JOIN"),
        "grain results join on the shared dimension, got:\n{sql}"
    );
    assert!(
        sql.contains("IS NOT DISTINCT FROM"),
        "the join key comparison must be NULL-safe, got:\n{sql}"
    );
    assert!(
        sql.contains("COALESCE"),
        "the output dimension must coalesce the per-grain keys, got:\n{sql}"
    );
    // The child grain reaches the base-table dimension through its FK — safe.
    assert!(
        sql.contains(r#"FROM "line_items" AS "li""#),
        "the child metric anchors its own CTE, got:\n{sql}"
    );
    assert!(
        sql.contains(r#"FROM "orders" AS "o""#),
        "the base-grain metric keeps its own CTE, got:\n{sql}"
    );
}

/// GRAIN-08: `COUNT(*)` on a table with no declared PRIMARY KEY is answerable
/// per-grain. The `CountStarRequiresPrimaryKey` guard exists only because the
/// base-anchored path LEFT JOINs the metric's table (NULL-extended rows would
/// be counted); when the table anchors its own CTE there is nothing to extend.
#[test]
fn count_star_without_primary_key_is_answerable_per_grain() {
    let def = orders_with_pkless_child();
    let sql = expand("sales", &def, &req(&[], &["order_total", "item_count"]))
        .expect("COUNT(*) at its own grain needs no PRIMARY KEY");
    assert!(
        sql.contains("COUNT(*)"),
        "the count stays a plain COUNT(*) at its own grain, got:\n{sql}"
    );
}

// ---------------------------------------------------------------------------
// GRAIN-06: single-grain queries are untouched
// ---------------------------------------------------------------------------

/// GRAIN-06: a query whose metrics all sit at the base grain keeps the flat,
/// base-anchored SQL — the per-grain path must not capture queries the existing
/// path already answers correctly.
#[test]
fn single_grain_query_keeps_flat_base_anchored_sql() {
    let def = orders_with_parent_customers();
    let sql = expand("sales", &def, &req(&["segment"], &["order_count"]))
        .expect("base-grain metric with a parent dimension is safe today");
    assert!(
        sql.starts_with("SELECT\n"),
        "must stay a flat SELECT, got:\n{sql}"
    );
    assert!(
        !sql.contains("__sv_grain"),
        "no per-grain CTEs for a single-grain query, got:\n{sql}"
    );
    assert!(
        sql.contains(r#"FROM "orders" AS "o""#) && sql.contains("LEFT JOIN"),
        "keeps the base-anchored join, got:\n{sql}"
    );
}

/// GRAIN-06: a child-grain metric alone is *already* correct base-anchored
/// (its rows are not multiplied by the LEFT JOIN to its parent), so it must not
/// be re-routed either.
#[test]
fn child_grain_metric_alone_keeps_flat_sql() {
    let def = orders_with_child_line_items();
    let sql = expand("sales", &def, &req(&["order_status"], &["item_count"]))
        .expect("a child-grain metric is safe base-anchored");
    assert!(
        !sql.contains("__sv_grain"),
        "single grain must not route per-grain, got:\n{sql}"
    );
}

// ---------------------------------------------------------------------------
// GRAIN-07: what per-grain does NOT make answerable
// ---------------------------------------------------------------------------

/// GRAIN-07: a dimension BELOW a metric's own grain still errors. Per-grain
/// aggregation cannot define this: the metric's rows genuinely fan across the
/// dimension's values (one order spans many line-item statuses). Snowflake
/// requires dimensions to be reachable through many-to-one relationships too.
#[test]
fn dimension_below_metric_grain_still_errors() {
    let def = orders_with_child_line_items().with_dimension("item_sku", "li.sku", Some("li"));
    let err = expand("sales", &def, &req(&["item_sku"], &["order_total"]))
        .expect_err("a dimension below the metric's grain must stay rejected");
    assert!(
        matches!(err, ExpandError::FanTrap { .. }),
        "expected FanTrap, got: {err}"
    );
}

/// GRAIN-07: the same rule applied to the parent-grain metric — grouping a
/// customer-grain metric by an order-grain dimension fans the customer rows.
#[test]
fn parent_grain_metric_with_base_dimension_still_errors() {
    let def = orders_with_parent_customers();
    let err = expand("sales", &def, &req(&["order_status"], &["total_balance"]))
        .expect_err("customer-grain metric grouped by an order-grain dimension must be rejected");
    assert!(
        matches!(err, ExpandError::FanTrap { .. }),
        "expected FanTrap, got: {err}"
    );
}

/// GRAIN-07: `OneToOne` edges never fan, so a query across a one-to-one
/// boundary stays on the flat path (nothing to fix, nothing to route).
#[test]
fn one_to_one_parent_metric_stays_flat() {
    let mut def = orders_with_parent_customers();
    def.joins[0].cardinality = Cardinality::OneToOne;
    let sql = expand("sales", &def, &req(&[], &["total_balance"]))
        .expect("a OneToOne parent is not fanned by the base-anchored FROM");
    assert!(
        !sql.contains("__sv_grain"),
        "OneToOne needs no per-grain rewrite, got:\n{sql}"
    );
}
