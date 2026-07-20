//! Differential proptest for the semi-additive (`NON ADDITIVE BY`) snapshot
//! path (code-review 2026-07-18 PBT-1 / "Top-3 new property #2").
//!
//! `expand_semi_additive` is the crate's longest function and the file where
//! the most recent behavioural bugs landed (the F-1 snapshot-polarity inversion,
//! the #30 dotted/quoted NA-dim resolution), yet the snapshot *math* — which
//! rows a `SUM(x) NON ADDITIVE BY ts` selects, how ties at the snapshot value
//! aggregate, and how NULL timestamps rank — had no randomized coverage. The
//! example tests pin a handful of fixed shapes; this exercises it over random
//! data with **duplicate timestamps** (RANK ties) and **NULLs** in every column.
//!
//! Shape: a single table `s(entity, ts, balance)`, a dimension on `entity` and
//! on `ts`, and a semi-additive metric `bal = SUM(s.balance) NON ADDITIVE BY
//! (ts)`. The declared sort direction is randomized (`ASC`/`DESC`), which flips
//! *which* snapshot is selected — the exact axis the F-1 bug inverted.
//!
//! For every query `expand` accepts, the result must equal an **independently
//! written** oracle, compared as a multiset inside DuckDB via a symmetric
//! `EXCEPT ALL` difference (the same comparator the single-table and star-join
//! harnesses use). The oracle computes the snapshot with `MAX`/`MIN` + `IS NOT
//! DISTINCT FROM` rather than the extension's `RANK() OVER (...)` CTE, so a bug
//! in the RANK formulation (wrong reversal, wrong NULLS placement, ties dropped)
//! surfaces as a non-zero diff.
//!
//! Snapshot semantics being pinned (verified against `semi_additive.rs`, which
//! emits the *reverse* of the declared direction and picks `RANK() = 1`):
//! - **`NON ADDITIVE BY (ts)`** (default `ASC` / `NULLS LAST`): the LATEST
//!   snapshot — rows at `MAX(ts)`; a NULL `ts` is selected only when the whole
//!   partition is NULL.
//! - **`NON ADDITIVE BY (ts DESC)`** (`DESC` / `NULLS FIRST`): the EARLIEST
//!   snapshot — NULL-`ts` rows when any exist (NULLS FIRST), otherwise `MIN(ts)`.
//! - The snapshot is per-partition = the queried dims minus the NA dim; when the
//!   NA dim (`ts`) is itself queried the metric is effectively regular (a plain
//!   `GROUP BY` aggregate), which the oracle mirrors.

use proptest::prelude::*;
use semantic_views::expand::{expand, DimensionName, MetricName, QueryRequest};
use semantic_views::model::{
    AccessModifier, Dimension, Metric, NonAdditiveDim, NullsOrder, SemanticViewDefinition,
    SortOrder, TableRef,
};

/// A generated instance: rows of `(entity, ts, balance)`. `None` is a SQL NULL
/// throughout. Small `entity`/`ts` domains force duplicate `(entity, ts)` pairs
/// (RANK ties within a snapshot) and repeated snapshot timestamps.
#[derive(Debug, Clone)]
struct Instance {
    rows: Vec<(Option<i64>, Option<i64>, Option<i64>)>,
}

/// Queryable dimensions and the single semi-additive metric, by stable name.
const DIMS: [&str; 2] = ["ent", "ts"];
const METS: [&str; 1] = ["bal"];
/// Index of the NA dimension (`ts`) within `DIMS`.
const TS_DIM: usize = 1;

/// A full case: an instance, the declared NA sort direction, and the non-empty
/// subset of dims + metrics to query (indices into `DIMS` / `METS`).
#[derive(Debug, Clone)]
struct Case {
    inst: Instance,
    order: SortOrder,
    sel_dims: Vec<usize>,
    sel_metrics: Vec<usize>,
}

fn arb_instance() -> impl Strategy<Value = Instance> {
    // Small signed value domain + NULL, mirroring the sibling harnesses.
    let bal_cell = prop_oneof![
        1 => Just(None),
        3 => (-5i64..=5).prop_map(Some),
    ];
    // Small entity/ts domains so ties and repeated snapshot timestamps are common.
    let ent_cell = prop_oneof![
        1 => Just(None),
        4 => (0i64..3).prop_map(Some),
    ];
    let ts_cell = prop_oneof![
        1 => Just(None),
        4 => (0i64..3).prop_map(Some),
    ];
    let row = (ent_cell, ts_cell, bal_cell);
    prop::collection::vec(row, 0..=20).prop_map(|rows| Instance { rows })
}

fn arb_case() -> impl Strategy<Value = Case> {
    let order = prop_oneof![Just(SortOrder::Asc), Just(SortOrder::Desc)];
    (arb_instance(), order).prop_flat_map(|(inst, order)| {
        let dim_sel =
            prop::sample::subsequence((0..DIMS.len()).collect::<Vec<_>>(), 0..=DIMS.len());
        let met_sel =
            prop::sample::subsequence((0..METS.len()).collect::<Vec<_>>(), 0..=METS.len());
        (Just(inst), Just(order), dim_sel, met_sel)
            .prop_filter(
                "at least one of dimensions/metrics must be selected",
                |(_, _, sel_dims, sel_metrics)| !sel_dims.is_empty() || !sel_metrics.is_empty(),
            )
            .prop_map(|(inst, order, sel_dims, sel_metrics)| Case {
                inst,
                order,
                sel_dims,
                sel_metrics,
            })
    })
}

/// Build the semantic-view definition for the given NA sort direction: single
/// table `s`, dimensions on `entity` and `ts`, and `bal = SUM(s.balance) NON
/// ADDITIVE BY (ts <order>)`.
fn build_def(order: SortOrder) -> SemanticViewDefinition {
    let tables = vec![TableRef {
        alias: "s".to_string(),
        table: "s".to_string(),
        pk_columns: vec![],
        unique_constraints: vec![],
        comment: None,
        synonyms: vec![],
    }];
    let dimensions = vec![
        Dimension {
            name: "ent".to_string(),
            expr: "s.entity".to_string(),
            source_table: Some("s".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
        },
        Dimension {
            name: "ts".to_string(),
            expr: "s.ts".to_string(),
            source_table: Some("s".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
        },
    ];
    let metrics = vec![Metric {
        name: "bal".to_string(),
        expr: "sum(s.balance)".to_string(),
        source_table: Some("s".to_string()),
        output_type: None,
        using_relationships: vec![],
        comment: None,
        synonyms: vec![],
        access: AccessModifier::Public,
        // Default NULLS placement per direction (Last for ASC, First for DESC)
        // is what the parser assigns for a bare `NON ADDITIVE BY (ts [DESC])`;
        // the oracle below matches that default.
        non_additive_by: vec![NonAdditiveDim {
            dimension: "ts".to_string(),
            order,
            nulls: match order {
                SortOrder::Asc => NullsOrder::Last,
                SortOrder::Desc => NullsOrder::First,
            },
        }],
        window_spec: None,
    }];
    SemanticViewDefinition {
        tables,
        dimensions,
        metrics,
        joins: vec![],
        facts: vec![],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    }
}

/// Create the physical table and insert the generated rows.
fn make_db(inst: &Instance) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    conn.execute_batch("CREATE TABLE s (entity INTEGER, ts INTEGER, balance INTEGER);")
        .expect("create table");
    let cell = |c: &Option<i64>| c.map_or_else(|| "NULL".to_string(), |v| v.to_string());
    if !inst.rows.is_empty() {
        let values: Vec<String> = inst
            .rows
            .iter()
            .map(|(e, t, b)| format!("({},{},{})", cell(e), cell(t), cell(b)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO s VALUES {};", values.join(",")))
            .expect("insert rows");
    }
    conn
}

/// Physical `SELECT` expression + output alias for a queried dimension.
fn dim_item(i: usize) -> &'static str {
    match DIMS[i] {
        "ent" => "s.entity AS ent",
        "ts" => "s.ts AS ts",
        other => unreachable!("unexpected dim {other}"),
    }
}

/// Independent oracle SQL for a case. Structurally different from the
/// extension's RANK-CTE: the snapshot value is computed with `MAX`/`MIN` and
/// selected with `IS NOT DISTINCT FROM` (so NULL keys and a NULL snapshot match
/// by identity, not `=`).
fn oracle_sql(case: &Case) -> String {
    let dims: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&i| dim_item(i).to_string())
        .collect();
    let ts_queried = case.sel_dims.contains(&TS_DIM);
    let has_metric = !case.sel_metrics.is_empty();

    // Dims-only query -> SELECT DISTINCT (no aggregation, no snapshot).
    if !has_metric {
        return format!("SELECT DISTINCT {} FROM s", dims.join(", "));
    }

    // ts is queried (or is the only projection) -> the metric is effectively
    // regular: a plain grouped SUM, no snapshot.
    if ts_queried {
        let select = dims
            .iter()
            .cloned()
            .chain(std::iter::once("sum(s.balance) AS bal".to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        if case.sel_dims.is_empty() {
            return format!("SELECT {select} FROM s");
        }
        let group_by = (1..=case.sel_dims.len())
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return format!("SELECT {select} FROM s GROUP BY {group_by}");
    }

    // Active snapshot. Partition = queried dims (a subset of {ent}); the NA dim
    // `ts` is not queried here. Snapshot timestamp per partition:
    //   ASC  / NULLS LAST : MAX(ts)                       (latest; NULL only if all NULL)
    //   DESC / NULLS FIRST: NULL if any NULL ts, else MIN (earliest; NULL wins)
    let snap_expr = match case.order {
        SortOrder::Asc => "max(s.ts)".to_string(),
        SortOrder::Desc => {
            "CASE WHEN count(*) FILTER (WHERE s.ts IS NULL) > 0 THEN NULL ELSE min(s.ts) END"
                .to_string()
        }
    };

    // Partition dimensions among the queried dims (everything except `ts`, which
    // is not queried in this branch, so this is exactly `sel_dims`).
    let part_cols: Vec<&str> = case
        .sel_dims
        .iter()
        .map(|&i| match DIMS[i] {
            "ent" => "entity",
            other => unreachable!("unexpected partition dim {other}"),
        })
        .collect();

    let select = dims
        .iter()
        .cloned()
        .chain(std::iter::once("sum(s.balance) AS bal".to_string()))
        .collect::<Vec<_>>()
        .join(", ");

    if part_cols.is_empty() {
        // Global snapshot: single-row subquery, filter to the snapshot ts.
        format!(
            "SELECT {select} FROM s, (SELECT {snap_expr} AS snap FROM s) m \
             WHERE s.ts IS NOT DISTINCT FROM m.snap"
        )
    } else {
        let sub_group = part_cols.join(", ");
        let sub_select = part_cols
            .iter()
            .map(|c| format!("s.{c} AS p_{c}"))
            .chain(std::iter::once(format!("{snap_expr} AS snap")))
            .collect::<Vec<_>>()
            .join(", ");
        let join_on = part_cols
            .iter()
            .map(|c| format!("s.{c} IS NOT DISTINCT FROM m.p_{c}"))
            .chain(std::iter::once(
                "s.ts IS NOT DISTINCT FROM m.snap".to_string(),
            ))
            .collect::<Vec<_>>()
            .join(" AND ");
        let group_by = (1..=case.sel_dims.len())
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "SELECT {select} FROM s JOIN (SELECT {sub_select} FROM s GROUP BY {sub_group}) m \
             ON {join_on} GROUP BY {group_by}"
        )
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 192, ..ProptestConfig::default() })]

    #[test]
    fn semi_additive_snapshot_matches_independent_oracle(case in arb_case()) {
        let def = build_def(case.order);
        let req = QueryRequest {
            dimensions: case.sel_dims.iter().map(|&i| DimensionName::new(DIMS[i])).collect(),
            metrics: case.sel_metrics.iter().map(|&i| MetricName::new(METS[i])).collect(),
            facts: vec![],
        };

        // Single table, no joins -> no fan trap; every query is accepted.
        let expanded = match expand("semi", &def, &req) {
            Ok(sql) => sql,
            Err(e) => {
                prop_assert!(false, "single-table semi-additive query unexpectedly rejected: {e}\ncase: {case:?}");
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
            "semi-additive expansion disagrees with the independent snapshot oracle \
             (symmetric multiset diff = {}); order={:?} dims={:?} metrics={:?}\n--- expanded:\n{}\n--- oracle:\n{}",
            diff, case.order, case.sel_dims, case.sel_metrics, expanded, oracle
        );
    }
}
