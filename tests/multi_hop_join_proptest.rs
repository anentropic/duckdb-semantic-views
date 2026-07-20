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
//! 1. **Ancestor-table metric ⇒ rejected.** A metric on the parent `u`
//!    (`SUM(u.uw)`, one hop up) or the grandparent `w` (`SUM(w.ww)`, two hops up)
//!    aggregated at the root grain is duplicated once per descendant `t` row —
//!    the classic silent inflation (EXP-1). `expand` MUST reject any query
//!    selecting either, with a fan-trap-family error. The two-hop case is the
//!    piece the single-hop star harness never reached.
//! 2. **Accepted query ⇒ numerically correct.** For every query `expand` accepts
//!    (metrics only on the root `t`: `SUM(t.v)` / `count(*)`), the result must
//!    equal an independently hand-written `t LEFT JOIN u LEFT JOIN w`
//!    aggregation, compared as a multiset inside DuckDB via a symmetric
//!    `EXCEPT ALL` difference (the same comparator the single-table and star
//!    harnesses use). Selecting a grandparent dimension (`wcat`) without the
//!    parent dimension forces the resolver to include the intermediate `u` to
//!    reach `w`; a multi-hop resolution bug (dropped intermediate, wrong join
//!    order, wrong ON columns) surfaces as invalid SQL or a non-zero diff.

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
/// three grains; `sv`/`ct` are root-grain (safe) metrics; `su`/`sw` are the
/// parent/grandparent (ancestor) metrics the fence must reject.
const DIMS: [&str; 3] = ["td", "ucat", "wcat"];
const METS: [&str; 4] = ["sv", "ct", "su", "sw"];
/// Metric names that aggregate an ancestor table and must be rejected (EXP-1).
const ANCESTOR_METS: [&str; 2] = ["su", "sw"];

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
fn oracle_sql(case: &Case) -> String {
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
        .map(|&i| match METS[i] {
            "sv" => "sum(t.v) AS sv".to_string(),
            "ct" => "count(*) AS ct".to_string(),
            other => unreachable!("unexpected safe metric {other}"),
        })
        .collect();
    let select_items = dim_items
        .iter()
        .chain(met_items.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let from = "FROM t LEFT JOIN u ON t.fk_u = u.id LEFT JOIN w ON u.fk_w = w.id";
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

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn multi_hop_fence_and_aggregation(case in arb_case()) {
        let def = build_def();
        let req = QueryRequest {
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

        let selects_ancestor_metric = case
            .sel_metrics
            .iter()
            .any(|&i| ANCESTOR_METS.contains(&METS[i]));
        let result = expand("multihop", &def, &req);

        if selects_ancestor_metric {
            // EXP-1: a metric on a parent (u, one hop) or grandparent (w, two
            // hops) aggregated at the root grain is silently inflated; the fence
            // must reject it. The two-hop case is the multi-hop delta over the
            // single-hop star harness.
            match result {
                Err(e) => {
                    let msg = e.to_string();
                    prop_assert!(
                        msg.contains("fan trap"),
                        "ancestor-table metric rejected, but not as a fan trap: {msg}"
                    );
                }
                Ok(sql) => prop_assert!(
                    false,
                    "ancestor-table metric (SUM(u.uw) / SUM(w.ww)) must be rejected (EXP-1), \
                     got SQL:\n{sql}"
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
