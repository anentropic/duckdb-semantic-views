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

/// A window metric whose inner aggregate lives at a NON-ROOT grain is answered
/// by anchoring `__sv_agg` at that grain (TECH-DEBT #36, first sub-item).
///
/// `total_balance` is `SUM(c.balance)` on the parent `customers`; `segment` is
/// also on `customers`. Base-anchoring the CTE at `orders` would join each
/// customer once per order and sum the balance that many times — the inflation
/// the v0.11.0 fence turned into `RootGrainFanTrap`. Anchored at `c` the inner
/// aggregate sees one row per customer, and the window function then runs over
/// the CTE exactly as before: only the inner aggregate is grain-sensitive.
///
/// The window metric is declared QUALIFIED (`o.running_balance`), the shape real
/// DDL always produces — `metric_grain_tables` unions that alias with the
/// inner's, so an anchor derived from it would see two grains and decline for
/// every DDL-declared window metric. Using `source_table: None` here would have
/// passed while the feature did nothing through actual DDL.
#[test]
fn multi_grain_window_metric_anchors_the_cte_at_its_own_grain() {
    let def = orders_with_parent_customers()
        .with_metric("running_balance", "SUM(total_balance)", Some("o"))
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
    let sql = expand("sales", &def, &req(&["segment"], &["running_balance"]))
        .expect("a window metric at its own grain is answerable");

    // The CTE is anchored at `customers`, NOT at the base table `orders`.
    assert!(
        sql.contains("FROM \"customers\" AS \"c\""),
        "__sv_agg must anchor at the inner metric's own grain: {sql}"
    );
    assert!(
        !sql.contains("\"orders\""),
        "the base table must not appear — joining it is what inflated the sum: {sql}"
    );
    // The window function still runs over the CTE, unchanged.
    assert!(
        sql.contains("__sv_agg"),
        "the window CTE shape is retained: {sql}"
    );
    assert!(
        sql.contains("OVER (PARTITION BY"),
        "the OVER clause still partitions: {sql}"
    );
}

/// A window metric already at the ROOT grain is left exactly as it was — the
/// base-anchored CTE is already correct there, so the anchor decision declines
/// and the emitted SQL is unchanged. Guards against re-anchoring queries that
/// never needed it.
#[test]
fn root_grain_window_metric_stays_base_anchored() {
    let def = orders_with_parent_customers()
        .with_metric("running_orders", "SUM(order_count)", Some("o"))
        .with_window_spec(
            "running_orders",
            WindowSpec {
                window_function: "SUM".to_string(),
                inner_metric: "order_count".to_string(),
                extra_args: vec![],
                excluding_dims: vec![],
                partition_dims: vec!["order_status".to_string()],
                order_by: vec![],
                frame_clause: None,
            },
        );
    let sql = expand("sales", &def, &req(&["order_status"], &["running_orders"]))
        .expect("a root-grain window metric was always answerable");
    assert!(
        sql.contains(r#"FROM "orders" AS "o""#),
        "must stay anchored at the base table: {sql}"
    );
}

/// A window metric whose inner aggregate is on a CHILD of the base table stays
/// base-anchored — re-anchoring there would be a correctness REGRESSION, not an
/// optimisation.
///
/// `FROM orders LEFT JOIN line_items` already yields each line-item row once, so
/// nothing is inflated, and that LEFT JOIN deliberately keeps an order with no
/// line items as a NULL-extended row whose `COUNT` is 0. Flipping to
/// `FROM line_items LEFT JOIN orders` would DROP that order from the result
/// entirely. Per-grain's own planner may re-anchor either way because it FULL
/// OUTER JOINs the grain CTEs; this single-CTE path has no such reassembly, so it
/// only re-anchors toward the "one" side, where base-anchoring genuinely fans.
///
/// Caught by `cr20260718_quoted_metric_window.test`, whose childless EU order
/// disappeared when the direction was not checked.
#[test]
fn window_metric_on_a_child_table_stays_base_anchored() {
    let def = orders_with_child_line_items()
        .with_metric("rolling_items", "SUM(item_count)", Some("li"))
        .with_window_spec(
            "rolling_items",
            WindowSpec {
                window_function: "SUM".to_string(),
                inner_metric: "item_count".to_string(),
                extra_args: vec![],
                excluding_dims: vec![],
                partition_dims: vec!["order_status".to_string()],
                order_by: vec![],
                frame_clause: None,
            },
        );
    let sql = expand("sales", &def, &req(&["order_status"], &["rolling_items"]))
        .expect("a child-grain window metric was always answerable");
    assert!(
        sql.contains(r#"FROM "orders" AS "o""#),
        "must stay anchored at the base table so childless parents survive: {sql}"
    );
    assert!(
        sql.contains("LEFT JOIN"),
        "the outer-join that preserves childless parents must remain: {sql}"
    );
}

/// The boundary this increment keeps: window metrics whose inner aggregates sit
/// at DIFFERENT grains would need those grains joined before the window runs,
/// which the single-anchor CTE cannot express. Declined, so the fence still
/// reports it rather than emitting a silently wrong shape.
#[test]
fn window_metrics_at_two_different_grains_still_error() {
    let def = orders_with_parent_customers()
        .with_metric("running_balance", "SUM(total_balance)", Some("o"))
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
        )
        .with_metric("running_orders", "SUM(order_count)", Some("o"))
        .with_window_spec(
            "running_orders",
            WindowSpec {
                window_function: "SUM".to_string(),
                inner_metric: "order_count".to_string(),
                extra_args: vec![],
                excluding_dims: vec![],
                partition_dims: vec!["segment".to_string()],
                order_by: vec![],
                frame_clause: None,
            },
        );
    let err = expand(
        "sales",
        &def,
        &req(&["segment"], &["running_balance", "running_orders"]),
    )
    .expect_err("inner aggregates at two grains are not answerable by one anchored CTE");
    assert!(
        matches!(
            err,
            ExpandError::RootGrainFanTrap { .. } | ExpandError::MetricFanTrap { .. }
        ),
        "expected a fan-trap error, got: {err}"
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

/// A PARENT of the base table whose metric is a bare `COUNT(*)` and which
/// declares `UNIQUE (id)` rather than `PRIMARY KEY (id)` — the SG-8 shape, one
/// grain UP instead of one grain down.
///
/// The `UNIQUE`-not-`PRIMARY KEY` detail is what makes this shape reachable
/// through real DDL, and it is load-bearing. D-06 rejects a table that an FK
/// references unless it declares a PRIMARY KEY **or** a UNIQUE, so a parent with
/// neither cannot be created at all (`65_pk_error.test`). `UNIQUE` satisfies
/// D-06, and it also makes `c` the "one" side that [`window_cte_anchor`]
/// re-anchors toward — while leaving `pk_columns` empty, so the
/// `COUNT(*)` → `COUNT(<pk>)` rewrite is impossible and SG-8 records the metric.
///
/// Declaring `c` with neither constraint would be a fixture DDL cannot produce —
/// the same trap that made the first cut of this feature a no-op through actual
/// DDL, so it is spelled out rather than left implicit.
fn orders_with_unique_only_parent() -> SemanticViewDefinition {
    let mut def = base_table(minimal_def("o", "d", "d", "m", "count(*)"), "orders", "id")
        .clear_dimensions()
        .clear_metrics()
        .with_table("c", "customers", &[])
        .with_dimension("segment", "c.segment", Some("c"))
        .with_dimension("order_status", "o.status", Some("o"))
        .with_metric("customer_count", "COUNT(*)", Some("c"))
        .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"]);
    let c = def
        .tables
        .iter_mut()
        .find(|t| t.alias == "c")
        .expect("the fixture declares `c`");
    c.unique_constraints = vec![vec!["id".to_string()]];
    def
}

/// A `SUM(<inner>) OVER (PARTITION BY <dim>)` window spec.
fn sum_over(inner: &str, partition_dim: &str) -> WindowSpec {
    WindowSpec {
        window_function: "SUM".to_string(),
        inner_metric: inner.to_string(),
        extra_args: vec![],
        excluding_dims: vec![],
        partition_dims: vec![partition_dim.to_string()],
        order_by: vec![],
        frame_clause: None,
    }
}

/// SG-8 must not reject a query the anchor makes safe (PR #175 review).
///
/// `customer_count` is a bare `COUNT(*)` on the PK-less parent `customers`.
/// Base-anchored, `FROM orders LEFT JOIN customers` would count NULL-extended
/// rows, which is exactly what `CountStarRequiresPrimaryKey` exists to prevent —
/// but the anchored CTE is `FROM customers`, where `COUNT(*)` counts customer
/// rows exactly and needs no PRIMARY KEY, the same reasoning that already
/// exempts the per-grain path.
///
/// This regressed on nothing — it was never answerable — but the guard ran
/// before `window_anchor` was computed, so a subset of the shape this change set
/// out to support still errored. Verified red before the fix, with
/// `CountStarRequiresPrimaryKey`.
#[test]
fn anchored_window_count_star_needs_no_primary_key() {
    let def = orders_with_unique_only_parent()
        .with_metric("running_customers", "SUM(customer_count)", Some("o"))
        .with_window_spec("running_customers", sum_over("customer_count", "segment"));
    let sql = expand("sales", &def, &req(&["segment"], &["running_customers"]))
        .expect("COUNT(*) at the anchored grain needs no PRIMARY KEY");
    assert!(
        sql.contains("FROM \"customers\" AS \"c\""),
        "the CTE must anchor at the counted table: {sql}"
    );
    assert!(
        sql.contains("COUNT(*)"),
        "the count stays a plain COUNT(*) at its own grain: {sql}"
    );
    assert!(
        !sql.contains("\"orders\""),
        "joining the base table is what would have inflated the count: {sql}"
    );
}

/// The invariant that makes the SG-8 bypass above sound, and the reason it is a
/// bypass rather than a deletion.
///
/// A dimension BELOW the anchor's grain pulls a "many"-side join back into the
/// anchored CTE (`FROM customers JOIN orders`), where `COUNT(*)` would count
/// orders rather than customers. The retained metric × dimension fan-trap check
/// rejects that shape before emission, so skipping SG-8 cannot turn a loud error
/// into a silently inflated number — it only changes WHICH error is reported
/// (`CountStarRequiresPrimaryKey` before, `FanTrap` now). Were that check ever
/// dropped, this test emits inflated SQL and fails.
#[test]
fn anchored_window_count_star_below_its_grain_still_errors() {
    let def = orders_with_unique_only_parent()
        .with_metric("running_customers", "SUM(customer_count)", Some("o"))
        .with_window_spec(
            "running_customers",
            sum_over("customer_count", "order_status"),
        );
    let err = expand(
        "sales",
        &def,
        &req(&["order_status"], &["running_customers"]),
    )
    .expect_err("a dimension below the anchor grain re-fans the CTE");
    assert!(
        matches!(
            err,
            ExpandError::FanTrap { .. } | ExpandError::MetricFanTrap { .. }
        ),
        "expected a fan-trap error, got: {err}"
    );
}

/// The same invariant for an inner `SUM` on a PK-ful parent, independent of
/// SG-8: a dimension below the anchor's grain must error rather than emit a CTE
/// whose fanning join re-inflates the very aggregate this change set out to fix.
#[test]
fn anchored_window_sum_below_its_grain_still_errors() {
    let def = orders_with_parent_customers()
        .with_metric("running_balance", "SUM(total_balance)", Some("o"))
        .with_window_spec("running_balance", sum_over("total_balance", "order_status"));
    let err = expand("sales", &def, &req(&["order_status"], &["running_balance"]))
        .expect_err("a dimension below the anchor grain re-fans the inner SUM");
    assert!(
        matches!(
            err,
            ExpandError::FanTrap { .. } | ExpandError::MetricFanTrap { .. }
        ),
        "expected a fan-trap error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// GRAIN-09: role-playing is a per-QUERY question, not a per-definition one
// ---------------------------------------------------------------------------

/// Base `f` (flights) reaches `a` (airports) through TWO named relationships —
/// the role-playing shape — and separately reaches an unrelated parent `c`
/// (carriers) through one.
fn flights_with_role_playing_airports() -> SemanticViewDefinition {
    base_table(minimal_def("f", "d", "d", "m", "count(*)"), "flights", "id")
        .clear_dimensions()
        .clear_metrics()
        .with_table("a", "airports", &["code"])
        .with_table("c", "carriers", &["id"])
        .with_dimension("dep_city", "a.city", Some("a"))
        .with_dimension("carrier_name", "c.name", Some("c"))
        .with_metric("flight_count", "COUNT(*)", Some("f"))
        .with_metric("fleet_size", "SUM(c.fleet)", Some("c"))
        .with_pkfk_join("dep", "f", "a", &["dep_code"], &["code"])
        .with_pkfk_join("arr", "f", "a", &["arr_code"], &["code"])
        .with_pkfk_join("f_to_c", "f", "c", &["carrier_id"], &["id"])
}

/// GRAIN-09: role-playing that the query never reaches must not cost it
/// per-grain emission.
///
/// Two grains (`f` and `c`) and a dimension on `c`: nothing here touches the
/// role-played `airports`, so no grain CTE joins anything ambiguous and no role
/// context is needed. The eligibility test used to ask a definition-level
/// question — "does any table have two inbound relationships from one source?" —
/// so every query against this definition lost per-grain emission and got the
/// v0.11.0 fan-trap error, however unrelated its grains were.
#[test]
fn role_playing_elsewhere_in_the_definition_does_not_block_per_grain() {
    let def = flights_with_role_playing_airports();
    let sql = expand(
        "flights_sv",
        &def,
        &req(&["carrier_name"], &["flight_count", "fleet_size"]),
    )
    .expect("role-playing the query never reaches must not block per-grain");
    assert!(
        sql.contains("__sv_grain_0") && sql.contains("__sv_grain_1"),
        "expected one CTE per grain, got:\n{sql}"
    );
    assert!(
        sql.contains(r#"FROM "carriers" AS "c""#),
        "the carrier-grain metric must anchor at its own table, got:\n{sql}"
    );
    assert!(
        !sql.contains("airports"),
        "the role-played table is not part of this query and must not be joined:\n{sql}"
    );
}

/// The single-grain half of the same fix, reported independently so the
/// sqllogictest file's halt-at-first-failure cannot leave it unproven: the
/// carrier-grain metric queried ALONE with its own dimension is a one-CTE
/// per-grain query, and it was declined by the definition-level test too.
#[test]
fn role_playing_elsewhere_does_not_block_a_single_grain_query() {
    let def = flights_with_role_playing_airports();
    let sql = expand("flights_sv", &def, &req(&["carrier_name"], &["fleet_size"]))
        .expect("a single-grain parent metric must be answerable here too");
    assert!(
        sql.contains(r#"FROM "carriers" AS "c""#),
        "anchored at the metric's own table, got:\n{sql}"
    );
    assert!(
        !sql.contains("flights") && !sql.contains("airports"),
        "neither the base table nor the role-played table belongs in this CTE:\n{sql}"
    );
}

/// The boundary the narrowed test keeps: when the query DOES reach the
/// role-played table, per-grain still declines.
///
/// `dep_city` is on `airports`, reachable by both `dep` and `arr`, and no
/// co-queried metric carries `USING` to say which. A grain CTE would have to
/// pick an edge, and picking one silently is the declaration-order-dependent
/// mis-binding the fence exists to prevent — so the query keeps its error until
/// `USING` context is threaded into `anchor_joins`.
#[test]
fn dimension_on_a_role_playing_target_still_declines_per_grain() {
    let def = flights_with_role_playing_airports();
    let err = expand(
        "flights_sv",
        &def,
        &req(&["dep_city"], &["flight_count", "fleet_size"]),
    )
    .expect_err("an unscoped dimension on a role-playing target is still declined");
    assert!(
        matches!(
            err,
            ExpandError::FanTrap { .. } | ExpandError::MetricFanTrap { .. }
        ),
        "expected the v0.11.0 fan-trap error, got: {err}"
    );
}
