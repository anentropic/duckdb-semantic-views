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
use crate::model::{Cardinality, NullsOrder, SemanticViewDefinition, SortOrder, WindowSpec};

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
        where_clause: None,
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

// ---------------------------------------------------------------------------
// GRAIN-04: a derived metric whose components span grains
// ---------------------------------------------------------------------------

/// `ratio = order_total / item_count` fuses an order-grain aggregate and a
/// line-item-grain one. Its components are computed at their own grains and the
/// division is evaluated over the pre-aggregates. v0.11.0 raised
/// `MetricFanTrap` naming the derived metric.
#[test]
fn derived_metric_spanning_grains_is_assembled_from_components() {
    let def = orders_with_child_line_items().with_metric(
        "avg_order_size",
        "order_total / item_count",
        None,
    );
    let sql = expand("sales", &def, &req(&[], &["avg_order_size"]))
        .expect("a derived metric spanning grains must be answerable");
    assert!(
        sql.contains(r#"FROM "orders" AS "o""#) && sql.contains(r#"FROM "line_items" AS "li""#),
        "each component must be aggregated at its own grain, got:\n{sql}"
    );
    assert!(
        sql.contains(r#""__sv_grain_0"."__sv_m0" / "__sv_grain_1"."__sv_m0" AS "avg_order_size""#),
        "the derived expression must combine the two component columns, got:\n{sql}"
    );
    // The components are aggregates in the CTEs, never in the outer SELECT.
    let outer = sql.split_once("\nSELECT\n").expect("outer SELECT").1;
    assert!(
        !outer.contains("SUM(") && !outer.contains("COUNT("),
        "the outer SELECT combines pre-aggregates, it does not re-aggregate:\n{outer}"
    );
}

/// The same, grouped by a dimension both grains can reach.
#[test]
fn derived_metric_spanning_grains_with_dimension() {
    let def = orders_with_child_line_items().with_metric(
        "avg_order_size",
        "order_total / item_count",
        None,
    );
    let sql = expand("sales", &def, &req(&["order_status"], &["avg_order_size"]))
        .expect("derived multi-grain metric with a shared dimension must be answerable");
    assert!(
        sql.contains("FULL OUTER JOIN") && sql.contains("IS NOT DISTINCT FROM"),
        "components join on the shared dimension, got:\n{sql}"
    );
    assert!(
        sql.matches("GROUP BY").count() == 2,
        "each component CTE groups by the dimension, got:\n{sql}"
    );
}

/// A derived metric that reaches its multi-grain components through ANOTHER
/// derived metric: `nested = avg_order_size * 2` where `avg_order_size =
/// order_total / item_count`. The intermediate derived metric has no grain of
/// its own — it must be inlined over the same component columns rather than
/// left as a dangling reference.
#[test]
fn derived_metric_nested_over_multi_grain_components() {
    let def = orders_with_child_line_items()
        .with_metric("avg_order_size", "order_total / item_count", None)
        .with_metric("scaled", "avg_order_size * 2", None);
    let sql = expand("sales", &def, &req(&[], &["scaled"]))
        .expect("a nested derived metric over multi-grain components must be answerable");
    assert!(
        !sql.contains("avg_order_size *"),
        "the intermediate derived metric must be inlined, not referenced as a column:\n{sql}"
    );
    assert!(
        sql.contains(r#"("__sv_grain_0"."__sv_m0" / "__sv_grain_1"."__sv_m0") * 2 AS "scaled""#),
        "expected the inlined nested expression, got:\n{sql}"
    );
}

/// Two derived metrics over the same components, queried together: each must be
/// assembled independently — the shape that regressed when a single visited-set
/// was shared across the whole rebuild instead of guarding one recursion path.
#[test]
fn two_derived_metrics_over_the_same_components() {
    let def = orders_with_child_line_items()
        .with_metric("avg_order_size", "order_total / item_count", None)
        .with_metric("inverse", "item_count / order_total", None);
    let sql = expand("sales", &def, &req(&[], &["avg_order_size", "inverse"]))
        .expect("two derived multi-grain metrics must both be answerable");
    assert!(
        sql.contains(r#"AS "avg_order_size""#) && sql.contains(r#"AS "inverse""#),
        "both metrics must be emitted, got:\n{sql}"
    );
    assert!(
        !sql.contains("order_total / item_count") && !sql.contains("item_count / order_total"),
        "neither expression may survive with unresolved metric references:\n{sql}"
    );
}

/// A chain of intermediate derived metrics, where a later one references an
/// earlier one: `half = order_total / item_count`, `scaled = half * 2`,
/// `combined = scaled + half`. Every intermediate must be resolved down to
/// component columns on each path it appears on — a rebuild that remembers
/// "already inlined `half`" across sibling branches leaks the raw metric name
/// into the SQL, which then fails to bind.
#[test]
fn derived_metric_chain_resolves_every_reference_to_columns() {
    let def = orders_with_child_line_items()
        .with_metric("half", "order_total / item_count", None)
        .with_metric("scaled", "half * 2", None)
        .with_metric("combined", "scaled + half", None);
    let sql = expand("sales", &def, &req(&[], &["combined"]))
        .expect("a chain of derived metrics over multi-grain components must be answerable");
    for leaked in ["half", "scaled", "order_total", "item_count"] {
        assert!(
            !sql.contains(leaked),
            "metric name '{leaked}' leaked into the SQL unresolved — it is not a column:\n{sql}"
        );
    }
}

/// The grain CTEs must be numbered deterministically: the same definition and
/// request always produce byte-identical SQL. (Component collection walks a
/// name set; iterating it directly made group 0 the line-item grain on some
/// runs and the order grain on others.)
#[test]
fn per_grain_sql_is_deterministic() {
    let def = orders_with_child_line_items().with_metric(
        "avg_order_size",
        "order_total / item_count",
        None,
    );
    let first = expand("sales", &def, &req(&[], &["avg_order_size"])).expect("expands");
    for _ in 0..16 {
        assert_eq!(
            first,
            expand("sales", &def, &req(&[], &["avg_order_size"])).expect("expands"),
            "per-grain SQL must not vary between expansions of the same query"
        );
    }
}

// ---------------------------------------------------------------------------
// Sibling grains (the chasm trap): two child tables of one parent
// ---------------------------------------------------------------------------

/// root `o` with TWO children — `li` (line items) and `s` (shipments). Neither
/// is an ancestor of the other; the path between them runs through the root,
/// and the `o -> s` leg of it fans.
fn orders_with_two_children() -> SemanticViewDefinition {
    base_table(minimal_def("o", "d", "d", "m", "count(*)"), "orders", "id")
        .clear_dimensions()
        .clear_metrics()
        .with_table("li", "line_items", &["id"])
        .with_table("s", "shipments", &["id"])
        .with_dimension("order_status", "o.status", Some("o"))
        .with_dimension("carrier", "s.carrier", Some("s"))
        .with_metric("item_qty", "SUM(li.qty)", Some("li"))
        .with_metric("ship_weight", "SUM(s.weight)", Some("s"))
        .with_pkfk_join("item_to_order", "li", "o", &["order_id"], &["id"])
        .with_pkfk_join("shipment_to_order", "s", "o", &["order_id"], &["id"])
}

/// GRAIN-03: metrics on two SIBLING child tables — the chasm trap. Joined into
/// one query each multiplies the other (an order's 2 line items x 2 shipments =
/// 4 rows), so each must be aggregated at its own grain.
#[test]
fn sibling_grain_metrics_are_computed_per_grain() {
    let def = orders_with_two_children();
    let sql = expand("sales", &def, &req(&[], &["item_qty", "ship_weight"]))
        .expect("two sibling child grains must be answerable per-grain");
    assert!(
        sql.contains(r#"FROM "line_items" AS "li""#) && sql.contains(r#"FROM "shipments" AS "s""#),
        "each sibling anchors its own CTE, got:\n{sql}"
    );
    assert!(
        sql.contains("CROSS JOIN"),
        "one row per grain, no dimensions to join on, got:\n{sql}"
    );
}

/// The same pair grouped by a dimension on their shared parent — reachable from
/// both grains without fanning either.
#[test]
fn sibling_grain_metrics_with_parent_dimension() {
    let def = orders_with_two_children();
    let sql = expand(
        "sales",
        &def,
        &req(&["order_status"], &["item_qty", "ship_weight"]),
    )
    .expect("sibling grains grouped by a parent dimension must be answerable");
    assert!(
        sql.contains("FULL OUTER JOIN"),
        "grain results join on the shared dimension, got:\n{sql}"
    );
    // Each CTE reaches the parent dimension through its own FK.
    assert!(
        sql.matches(r#"LEFT JOIN "orders" AS "o""#).count() == 2,
        "each grain CTE joins the parent itself, got:\n{sql}"
    );
}

/// A metric on one child table grouped by a dimension on its SIBLING is a fan
/// trap — line-item rows are multiplied by the order's shipments before
/// aggregation — and must be rejected. The path between two siblings runs
/// through their shared parent, which the fence's parent-chain walk could not
/// express: neither sibling is an ancestor of the other, so the walk found no
/// path and the check silently passed.
#[test]
fn metric_grouped_by_sibling_dimension_errors() {
    let def = orders_with_two_children();
    let err = expand("sales", &def, &req(&["carrier"], &["item_qty"]))
        .expect_err("a dimension on a sibling child table fans the metric");
    assert!(
        matches!(err, ExpandError::FanTrap { .. }),
        "expected FanTrap, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// GRAIN-07: the documented residual boundary (TECH-DEBT #36)
// ---------------------------------------------------------------------------

/// A multi-grain query carrying an ACTIVE semi-additive metric keeps the
/// fan-trap error: the snapshot CTE that metric needs is anchored at the base
/// table, and routing it through per-grain emission without designing that
/// interaction would risk the silent-inflation class the fence exists to stop.
#[test]
fn multi_grain_with_active_semi_additive_metric_still_errors() {
    let def = orders_with_parent_customers()
        .with_dimension("snapshot_day", "c.as_of", Some("c"))
        .with_non_additive_by(
            "total_balance",
            &[("snapshot_day", SortOrder::Asc, NullsOrder::Last)],
        );
    // `snapshot_day` is NOT queried, so the metric is *active* semi-additive.
    let err = expand("sales", &def, &req(&[], &["total_balance"]))
        .expect_err("an active semi-additive metric is not per-grain eligible");
    assert!(
        matches!(err, ExpandError::RootGrainFanTrap { .. }),
        "expected the v0.11.0 fan-trap error, got: {err}"
    );
}

/// The same boundary for window metrics.
#[test]
fn multi_grain_with_window_metric_still_errors() {
    let def = orders_with_parent_customers()
        .with_metric("running_balance", "SUM(total_balance)", None)
        .with_window_spec(
            "running_balance",
            WindowSpec {
                window_function: "SUM".to_string(),
                inner_metric: "total_balance".to_string(),
                extra_args: vec![],
                excluding_dims: vec![],
                partition_dims: vec!["segment".to_string()],
                order_by: vec![],
                frame_clause: None,
            },
        );
    let err = expand("sales", &def, &req(&["segment"], &["running_balance"]))
        .expect_err("a window metric is not per-grain eligible");
    assert!(
        matches!(err, ExpandError::RootGrainFanTrap { .. }),
        "expected the v0.11.0 fan-trap error, got: {err}"
    );
}

/// A metric carrying `USING` role-playing context is not per-grain eligible
/// either: which role a grain CTE should join is exactly what `USING` answers on
/// the base-anchored path, and the grain CTEs do not carry that context.
#[test]
fn multi_grain_with_role_playing_still_errors() {
    // `orders` reaches `customers` through TWO named relationships — the
    // role-playing shape.
    let def = orders_with_parent_customers().with_pkfk_join(
        "o_to_billing_c",
        "o",
        "c",
        &["billing_customer_id"],
        &["id"],
    );
    let err = expand("sales", &def, &req(&[], &["total_balance"]))
        .expect_err("role-playing is not per-grain eligible");
    assert!(
        matches!(err, ExpandError::RootGrainFanTrap { .. }),
        "expected the v0.11.0 fan-trap error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Pre-aggregation `where_clause` on the per-grain path
//
// The predicate goes into EACH grain CTE, so every metric aggregates over only
// the matching rows. On the outer query it would filter the already-combined
// result -- and because the grains are joined FULL OUTER, that would drop whole
// groups rather than recompute them.
// ---------------------------------------------------------------------------

/// The two-grain shape plus a `customers` parent that neither metric anchors,
/// so a predicate on `segment` forces every grain CTE to join it.
fn two_grains_with_unqueried_parent() -> SemanticViewDefinition {
    orders_with_child_line_items()
        .with_table("c", "customers", &["id"])
        .with_dimension("segment", "c.segment", Some("c"))
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
}

/// The bodies of the grain CTEs, excluding the outer query (which also mentions
/// `__sv_grain_N`, so splitting on the CTE name alone would over-match).
fn grain_cte_bodies(sql: &str) -> Vec<&str> {
    sql.split(" AS (\n").skip(1).collect()
}

#[test]
fn where_clause_is_injected_into_every_grain_cte() {
    let def = two_grains_with_unqueried_parent();
    let req = QueryRequest {
        dimensions: vec![DimensionName::new("order_status")],
        metrics: vec![
            MetricName::new("order_total"),
            MetricName::new("item_count"),
        ],
        facts: vec![],
        where_clause: Some("segment = 'ENTERPRISE'".to_string()),
    };
    let sql = expand("test_view", &def, &req).unwrap();

    let bodies = grain_cte_bodies(&sql);
    assert_eq!(bodies.len(), 2, "expected two grain CTEs: {sql}");
    for (i, body) in bodies.iter().enumerate() {
        let where_at = body.find("WHERE (c.segment) = 'ENTERPRISE'");
        let group_at = body.find("GROUP BY");
        assert!(
            where_at.is_some(),
            "grain {i} missing the predicate: {body}"
        );
        assert!(
            group_at.is_some() && where_at < group_at,
            "grain {i}: WHERE must precede GROUP BY so it filters the rows going \
             INTO the aggregation: {body}"
        );
    }

    // The outer query must NOT carry it -- there it would be post-aggregation.
    let outer = sql.rsplit_once(")\n").expect("a closing CTE").1;
    assert!(
        !outer.contains("WHERE"),
        "predicate must not leak onto the outer query: {outer}"
    );
}

#[test]
fn where_clause_joins_its_table_into_every_grain_cte() {
    // `c` is referenced only by the predicate, and the `li` grain reaches it
    // only via li -> o -> c, so this also pins the multi-hop join path.
    let def = two_grains_with_unqueried_parent();
    let req = QueryRequest {
        dimensions: vec![DimensionName::new("order_status")],
        metrics: vec![
            MetricName::new("order_total"),
            MetricName::new("item_count"),
        ],
        facts: vec![],
        where_clause: Some("segment = 'ENTERPRISE'".to_string()),
    };
    let sql = expand("test_view", &def, &req).unwrap();
    for (i, body) in grain_cte_bodies(&sql).iter().enumerate() {
        assert!(
            body.contains("\"customers\" AS \"c\""),
            "grain {i} must join customers to evaluate the filter: {body}"
        );
    }
}

#[test]
fn where_clause_on_a_single_grain_plan_filters_before_aggregation() {
    let def = orders_with_parent_customers();
    let req = QueryRequest {
        dimensions: vec![DimensionName::new("segment")],
        metrics: vec![MetricName::new("total_balance")],
        facts: vec![],
        where_clause: Some("segment <> 'CHURNED'".to_string()),
    };
    let sql = expand("test_view", &def, &req).unwrap();
    let where_at = sql.find("WHERE (c.segment) <> 'CHURNED'");
    assert!(where_at.is_some(), "predicate must be emitted: {sql}");
    if let Some(group_at) = sql.find("GROUP BY") {
        assert!(where_at < Some(group_at), "WHERE before GROUP BY: {sql}");
    }
}
