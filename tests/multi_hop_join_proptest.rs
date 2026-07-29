//! Differential proptest for **multi-hop** join trees (code-review 2026-07-18
//! PBT-1 follow-up — the multi-hop facet of "no randomized coverage of the
//! hardest expansion semantics").
//!
//! The two-table [`star_schema_proptest`] exercises a single `ManyToOne` hop.
//! Nothing exercised a chain of hops — where the join resolver must pull in an
//! **intermediate** table to reach a far dimension, topologically order two
//! joins from the root, and the fan-trap fence must reject a metric on a table
//! **two hops** above the root (not just the immediate parent).
//!
//! Shape: a `ManyToOne` **chain** rooted at the base/"many"-most table.
//! `t.fk_u REFERENCES u.id` and `u.fk_w REFERENCES w.id`, so the join tree is
//! `t → u → w` (t at the root grain, u its parent, w its grandparent). `u.id`
//! and `w.id` are generated distinct so the declared PKs hold and the chained
//! LEFT JOINs never fan `t`. Foreign keys at both levels include NULL and
//! dangling ids; group keys and values include NULLs.
//!
//! Two invariants are checked per case:
//!
//! 1. **Dimension below a metric's grain ⇒ rejected.** `SUM(u.uw)` grouped by
//!    `t.d`, or `SUM(w.ww)` grouped by `u.ucat` / `t.d`, asks for an ancestor's
//!    aggregate at a grain below it: those rows genuinely fan across the
//!    descendant's values, at one hop or two. `expand` MUST reject it with a
//!    fan-trap-family error — neither the root-anchored path (which silently
//!    inflated it, EXP-1) nor per-grain aggregation can define it.
//! 2. **Accepted query ⇒ numerically correct.** For every query `expand`
//!    accepts, the result must equal an independently hand-written oracle,
//!    compared as a multiset inside DuckDB via a symmetric `EXCEPT ALL`
//!    difference (the same comparator the single-table and star harnesses use).
//!    Since v0.12.0 that set includes the **ancestor metrics** `SUM(u.uw)` /
//!    `SUM(w.ww)`, alone or mixed with root-grain metrics: each is computed at
//!    its own grain (`FROM u` / `FROM w`, never through the descendant join) and
//!    the grains are joined back together, so the oracle is a per-grain one
//!    spanning up to three grains at once. Selecting a grandparent dimension
//!    (`wcat`) without the parent dimension still forces the resolver to include
//!    the intermediate `u` to reach `w`; a multi-hop resolution bug (dropped
//!    intermediate, wrong join order, wrong ON columns) surfaces as invalid SQL
//!    or a non-zero diff.

use proptest::prelude::*;
use semantic_views::expand::{expand, DimensionName, MetricName, QueryRequest};
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
};

/// A generated chain instance. Grandparent rows `w` have ids `0..n_w`; parent
/// rows `u` have ids `0..n_u` and a foreign key `fk_w` into `w`; child rows `t`
/// carry a foreign key `fk_u` into `u`. `None` is a SQL NULL throughout.
#[derive(Debug, Clone)]
struct Instance {
    /// Grandparent rows: `(wcat, ww)` for ids `0..n_w`.
    w_rows: Vec<(Option<i64>, Option<i64>)>,
    /// Parent rows: `(fk_w, ucat, uw)` for ids `0..n_u`.
    u_rows: Vec<(Option<i64>, Option<i64>, Option<i64>)>,
    /// Child rows: `(fk_u, d, v)`.
    t_rows: Vec<(Option<i64>, Option<i64>, Option<i64>)>,
}

/// Queryable objects, by stable name. `td`/`ucat`/`wcat` are dimensions at the
/// three grains; `sv`/`ct` are root-grain metrics; `su`/`sw` are the
/// parent/grandparent (ancestor) metrics, each computed at its own grain.
const DIMS: [&str; 3] = ["td", "ucat", "wcat"];
const METS: [&str; 4] = ["sv", "ct", "su", "sw"];

/// A full case: an instance plus the non-empty subset of dims + metrics to query.
#[derive(Debug, Clone)]
struct Case {
    inst: Instance,
    sel_dims: Vec<usize>,
    sel_metrics: Vec<usize>,
}

fn arb_instance() -> impl Strategy<Value = Instance> {
    let val_cell = prop_oneof![
        1 => Just(None),
        3 => (-5i64..=5).prop_map(Some),
    ];
    let cat_cell = prop_oneof![
        1 => Just(None),
        4 => (0i64..3).prop_map(Some),
    ];
    // Grandparent and parent counts kept small so parent/child fan-in is common.
    (1usize..=3, 1usize..=4).prop_flat_map(move |(n_w, n_u)| {
        let w_row = (cat_cell.clone(), val_cell.clone());
        let w_rows = prop::collection::vec(w_row, n_w);
        // u.fk_w: NULL, a valid w id (0..n_w), or a dangling id (n_w).
        let fk_w_cell = prop_oneof![
            1 => Just(None),
            4 => (0i64..n_w as i64).prop_map(Some),
            1 => Just(Some(n_w as i64)),
        ];
        let u_row = (fk_w_cell, cat_cell.clone(), val_cell.clone());
        let u_rows = prop::collection::vec(u_row, n_u);
        // t.fk_u: NULL, a valid u id (0..n_u), or a dangling id (n_u).
        let fk_u_cell = prop_oneof![
            1 => Just(None),
            4 => (0i64..n_u as i64).prop_map(Some),
            1 => Just(Some(n_u as i64)),
        ];
        let t_row = (fk_u_cell, cat_cell.clone(), val_cell.clone());
        let t_rows = prop::collection::vec(t_row, 0..=20);
        (w_rows, u_rows, t_rows).prop_map(|(w_rows, u_rows, t_rows)| Instance {
            w_rows,
            u_rows,
            t_rows,
        })
    })
}

fn arb_case() -> impl Strategy<Value = Case> {
    arb_instance().prop_flat_map(|inst| {
        let dim_sel =
            prop::sample::subsequence((0..DIMS.len()).collect::<Vec<_>>(), 0..=DIMS.len());
        let met_sel =
            prop::sample::subsequence((0..METS.len()).collect::<Vec<_>>(), 0..=METS.len());
        (Just(inst), dim_sel, met_sel)
            .prop_filter(
                "at least one of dimensions/metrics must be selected",
                |(_, sel_dims, sel_metrics)| !sel_dims.is_empty() || !sel_metrics.is_empty(),
            )
            .prop_map(|(inst, sel_dims, sel_metrics)| Case {
                inst,
                sel_dims,
                sel_metrics,
            })
    })
}

/// Build the semantic-view definition: a `ManyToOne` chain `t → u → w`, a
/// dimension at each grain, root-grain safe metrics (`sum(t.v)`, `count(*)`),
/// and ancestor metrics on `u` and `w`.
fn build_def() -> SemanticViewDefinition {
    let table = |alias: &str, pk: &[&str]| TableRef {
        alias: alias.to_string(),
        table: alias.to_string(),
        pk_columns: pk.iter().map(|s| (*s).to_string()).collect(),
        unique_constraints: vec![],
        comment: None,
        synonyms: vec![],
    };
    // `t` is listed first: base_table() == the first declared table, and the
    // FROM is anchored there with LEFT JOINs outward along the chain.
    let tables = vec![table("t", &[]), table("u", &["id"]), table("w", &["id"])];
    let dim = |name: &str, expr: &str, source: &str| Dimension {
        name: name.to_string(),
        expr: expr.to_string(),
        source_table: Some(source.to_string()),
        output_type: None,
        comment: None,
        synonyms: vec![],
    };
    let dimensions = vec![
        dim("td", "t.d", "t"),
        dim("ucat", "u.ucat", "u"),
        dim("wcat", "w.wcat", "w"),
    ];
    let base_metric = |name: &str, expr: &str, source: Option<&str>| Metric {
        name: name.to_string(),
        expr: expr.to_string(),
        source_table: source.map(str::to_string),
        output_type: None,
        using_relationships: vec![],
        comment: None,
        synonyms: vec![],
        access: AccessModifier::Public,
        non_additive_by: vec![],
        window_spec: None,
    };
    let metrics = vec![
        base_metric("sv", "sum(t.v)", Some("t")),
        base_metric("ct", "count(*)", None),
        base_metric("su", "sum(u.uw)", Some("u")),
        base_metric("sw", "sum(w.ww)", Some("w")),
    ];
    let joins = vec![
        Join {
            from_alias: "t".to_string(),
            table: "u".to_string(),
            fk_columns: vec!["fk_u".to_string()],
            ref_columns: vec!["id".to_string()],
            name: Some("t_u".to_string()),
            cardinality: Cardinality::ManyToOne,
        },
        Join {
            from_alias: "u".to_string(),
            table: "w".to_string(),
            fk_columns: vec!["fk_w".to_string()],
            ref_columns: vec!["id".to_string()],
            name: Some("u_w".to_string()),
            cardinality: Cardinality::ManyToOne,
        },
    ];
    SemanticViewDefinition {
        tables,
        dimensions,
        metrics,
        joins,
        facts: vec![],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    }
}

/// Create the physical tables and insert the generated rows.
fn make_db(inst: &Instance) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    conn.execute_batch(
        "CREATE TABLE w (id INTEGER, wcat INTEGER, ww INTEGER); \
         CREATE TABLE u (id INTEGER, fk_w INTEGER, ucat INTEGER, uw INTEGER); \
         CREATE TABLE t (fk_u INTEGER, d INTEGER, v INTEGER);",
    )
    .expect("create tables");

    let cell = |c: &Option<i64>| c.map_or_else(|| "NULL".to_string(), |v| v.to_string());

    if !inst.w_rows.is_empty() {
        let values: Vec<String> = inst
            .w_rows
            .iter()
            .enumerate()
            .map(|(i, (wcat, ww))| format!("({i},{},{})", cell(wcat), cell(ww)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO w VALUES {};", values.join(",")))
            .expect("insert w rows");
    }
    if !inst.u_rows.is_empty() {
        let values: Vec<String> = inst
            .u_rows
            .iter()
            .enumerate()
            .map(|(i, (fk_w, ucat, uw))| {
                format!("({i},{},{},{})", cell(fk_w), cell(ucat), cell(uw))
            })
            .collect();
        conn.execute_batch(&format!("INSERT INTO u VALUES {};", values.join(",")))
            .expect("insert u rows");
    }
    if !inst.t_rows.is_empty() {
        let values: Vec<String> = inst
            .t_rows
            .iter()
            .map(|(fk_u, d, v)| format!("({},{},{})", cell(fk_u), cell(d), cell(v)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO t VALUES {};", values.join(",")))
            .expect("insert t rows");
    }
    conn
}

/// Independent oracle SQL for a query `expand` should accept (only root-grain
/// metrics). The FROM is always the full chain `t LEFT JOIN u LEFT JOIN w`:
/// because `u.id`/`w.id` are unique, joining the parents never changes `t`'s
/// multiset for `count(*)`/`sum(t.v)`, and grouping by an ancestor dimension is
/// a plain group key. Metrics-only ⇒ global aggregate (no GROUP BY); anything
/// with dimensions ⇒ GROUP BY the projected dimension ordinals (multiset-equal
/// to the expansion's SELECT DISTINCT for the dims-only case).
/// The chain position of a table: `t` (the base/"many"-most) is 0, its parent
/// `u` is 1, its grandparent `w` is 2. A grain can be grouped by a dimension at
/// its own level or ABOVE it (each hop up is many-to-one, so the join adds no
/// rows); a dimension BELOW it fans it, which is the one shape that stays
/// rejected.
fn level(table: &str) -> usize {
    match table {
        "t" => 0,
        "u" => 1,
        "w" => 2,
        other => unreachable!("unexpected table {other}"),
    }
}

/// The table each dimension lives on.
fn dim_table(dim: &str) -> &'static str {
    match dim {
        "td" => "t",
        "ucat" => "u",
        "wcat" => "w",
        other => unreachable!("unexpected dim {other}"),
    }
}

/// The table each metric aggregates — its **grain**. `ct` is `count(*)` with no
/// declared source table, which sits at the base grain.
fn metric_table(metric: &str) -> &'static str {
    match metric {
        "sv" | "ct" => "t",
        "su" => "u",
        "sw" => "w",
        other => unreachable!("unexpected metric {other}"),
    }
}

/// One grain's half of the oracle: the metrics that live on `anchor`, computed
/// over `anchor`'s own rows and grouped by the selected dimensions, which are
/// reached by chaining LEFT JOINs **upward** from the anchor.
///
/// This is the independent statement of "at its own grain": `su` is summed over
/// `u` itself, never over `t LEFT JOIN u` (where each parent row appears once
/// per child row and childless parents vanish).
fn grain_sql(case: &Case, anchor: &str) -> String {
    let dim_items: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&i| match DIMS[i] {
            "td" => "t.d AS td".to_string(),
            "ucat" => "u.ucat AS ucat".to_string(),
            "wcat" => "w.wcat AS wcat".to_string(),
            other => unreachable!("unexpected dim {other}"),
        })
        .collect();
    let met_items: Vec<String> = case
        .sel_metrics
        .iter()
        .filter(|&&i| metric_table(METS[i]) == anchor)
        .map(|&i| match METS[i] {
            "sv" => "sum(t.v) AS sv".to_string(),
            "ct" => "count(*) AS ct".to_string(),
            "su" => "sum(u.uw) AS su".to_string(),
            "sw" => "sum(w.ww) AS sw".to_string(),
            other => unreachable!("unexpected metric {other}"),
        })
        .collect();
    let select_items = dim_items
        .iter()
        .chain(met_items.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    // The up-chain from the anchor. Joining the whole remaining chain is
    // harmless — every hop is many-to-one, so none of them changes the anchor's
    // row multiset.
    let from = match anchor {
        "t" => "FROM t LEFT JOIN u ON t.fk_u = u.id LEFT JOIN w ON u.fk_w = w.id",
        "u" => "FROM u LEFT JOIN w ON u.fk_w = w.id",
        "w" => "FROM w",
        other => unreachable!("unexpected anchor {other}"),
    };
    if case.sel_dims.is_empty() {
        format!("SELECT {select_items} {from}")
    } else {
        let group_by = (1..=case.sel_dims.len())
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("SELECT {select_items} {from} GROUP BY {group_by}")
    }
}

/// Independent oracle SQL: each selected grain aggregated on its own, then the
/// grains combined — `CROSS JOIN` when the query has no dimensions (one row per
/// grain), otherwise `FULL OUTER JOIN` on the NULL-safe dimension keys so a
/// group present at one grain and absent at another is preserved.
fn oracle_sql(case: &Case) -> String {
    // Grains in first-selected order.
    let mut anchors: Vec<&str> = Vec::new();
    for &i in &case.sel_metrics {
        let table = metric_table(METS[i]);
        if !anchors.contains(&table) {
            anchors.push(table);
        }
    }
    if anchors.is_empty() {
        anchors.push("t"); // Dimensions-only: the base-anchored DISTINCT shape.
    }
    let mut sql = format!(
        "SELECT {} FROM ({}) g0",
        oracle_projection(case, &anchors),
        grain_sql(case, anchors[0])
    );
    for (n, anchor) in anchors.iter().enumerate().skip(1) {
        let joined = grain_sql(case, anchor);
        if case.sel_dims.is_empty() {
            sql.push_str(&format!(" CROSS JOIN ({joined}) g{n}"));
        } else {
            let conditions: Vec<String> = case
                .sel_dims
                .iter()
                .map(|&d| {
                    let dim = DIMS[d];
                    format!("{} IS NOT DISTINCT FROM g{n}.{dim}", coalesced(dim, n))
                })
                .collect();
            sql.push_str(&format!(
                " FULL OUTER JOIN ({joined}) g{n} ON {}",
                conditions.join(" AND ")
            ));
        }
    }
    sql
}

/// `g0.<dim>` for one grain, `COALESCE(g0.<dim>, …, g{n-1}.<dim>)` beyond —
/// the key must be read from whichever grain supplied the row.
fn coalesced(dim: &str, groups: usize) -> String {
    if groups == 1 {
        format!("g0.{dim}")
    } else {
        let refs: Vec<String> = (0..groups).map(|g| format!("g{g}.{dim}")).collect();
        format!("COALESCE({})", refs.join(", "))
    }
}

/// The oracle's output projection: each dimension coalesced across every grain,
/// each metric read from the grain that computed it.
fn oracle_projection(case: &Case, anchors: &[&str]) -> String {
    let mut items: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&d| format!("{} AS {}", coalesced(DIMS[d], anchors.len()), DIMS[d]))
        .collect();
    for &m in &case.sel_metrics {
        let metric = METS[m];
        let group = anchors
            .iter()
            .position(|a| *a == metric_table(metric))
            .expect("every selected metric's grain is an anchor");
        items.push(format!("g{group}.{metric}"));
    }
    items.join(", ")
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn multi_hop_fence_and_aggregation(case in arb_case()) {
        let def = build_def();
        let req = QueryRequest {
            where_clause: None,
            dimensions: case
                .sel_dims
                .iter()
                .map(|&i| DimensionName::new(DIMS[i]))
                .collect(),
            metrics: case
                .sel_metrics
                .iter()
                .map(|&i| MetricName::new(METS[i]))
                .collect(),
            facts: vec![],
        };

        // A dimension BELOW some selected metric's grain is the one shape that
        // must stay rejected: those parent rows genuinely fan across the
        // descendant's values, at one hop or two.
        let dimension_below_a_metric_grain = case.sel_metrics.iter().any(|&m| {
            case.sel_dims
                .iter()
                .any(|&d| level(dim_table(DIMS[d])) < level(metric_table(METS[m])))
        });
        let result = expand("multihop", &def, &req);

        if dimension_below_a_metric_grain {
            match result {
                Err(e) => {
                    let msg = e.to_string();
                    prop_assert!(
                        msg.contains("fan trap"),
                        "dimension below a metric's grain rejected, but not as a fan trap: {msg}"
                    );
                }
                Ok(sql) => prop_assert!(
                    false,
                    "a dimension below a metric's grain must be rejected, got SQL:\n{sql}"
                ),
            }
            return Ok(());
        }

        // Accepted-query branch: must expand and match the independent oracle.
        let expanded = match result {
            Ok(sql) => sql,
            Err(e) => {
                prop_assert!(false, "safe multi-hop query unexpectedly rejected: {e}");
                unreachable!()
            }
        };
        let oracle = oracle_sql(&case);

        // Canonical projection (output columns sorted by name) so a column-order
        // difference between the two formulations is not a false diff.
        let mut proj_cols: Vec<String> = case
            .sel_dims
            .iter()
            .map(|&i| DIMS[i].to_string())
            .chain(case.sel_metrics.iter().map(|&i| METS[i].to_string()))
            .collect();
        proj_cols.sort();
        let proj = proj_cols.join(", ");

        let cmp = format!(
            "SELECT \
               (SELECT count(*) FROM (SELECT {proj} FROM ({expanded}) qa \
                                      EXCEPT ALL \
                                      SELECT {proj} FROM ({oracle}) qb) e1) \
             + (SELECT count(*) FROM (SELECT {proj} FROM ({oracle}) qc \
                                      EXCEPT ALL \
                                      SELECT {proj} FROM ({expanded}) qd) e2) AS diff"
        );

        let conn = make_db(&case.inst);
        let diff: i64 = conn.query_row(&cmp, [], |r| r.get(0)).unwrap_or_else(|e| {
            panic!("differential comparison query failed: {e}\n--- expanded:\n{expanded}\n--- oracle:\n{oracle}")
        });

        prop_assert_eq!(
            diff, 0,
            "multi-hop expansion disagrees with hand-written chained LEFT JOIN aggregation \
             (symmetric multiset diff = {}); dims={:?} metrics={:?}\n--- expanded:\n{}\n--- oracle:\n{}",
            diff, case.sel_dims, case.sel_metrics, expanded, oracle
        );
    }
}
