//! Differential proptest for the two-table star join / fan-trap fence
//! (code-review 2026-07-18 PBT-1 / "Top-3 new property #1").
//!
//! The single-table [`differential_proptest`] never exercises a join, so the
//! fan-trap safety fence (`expand::fan_trap::check_fan_traps`) — the code that
//! decides *which* metric/dimension combinations may be computed at the
//! root-anchored grain and which must be rejected — had no randomized coverage.
//! Its three leak-throughs (EXP-1/2/3) were exactly there.
//!
//! Shape: a `ManyToOne` star. The ROOT (base) table `t` is the FK/"many" side
//! (`t.fk REFERENCES u.id`); `u` is the parent/"one" side. Generated data
//! includes dangling and NULL foreign keys and NULL group keys / values. `u.id`
//! is generated distinct so the declared PRIMARY KEY holds in the data and the
//! LEFT JOIN never fans `t`.
//!
//! Two invariants are checked per case:
//!
//! 1. **Parent-table metric + child dimension ⇒ rejected.** `SUM(u.w)` grouped
//!    by `t.d` asks for a parent-grain aggregate at a grain below it: each
//!    parent row genuinely fans across the child values. Neither the root-
//!    anchored path (which silently inflated it — EXP-1) nor per-grain
//!    aggregation can define it, so `expand` MUST reject it with a
//!    fan-trap-family error.
//! 2. **Accepted query ⇒ numerically correct.** For every query `expand`
//!    accepts, the result must equal an independently hand-written oracle,
//!    compared as a multiset inside DuckDB via a symmetric `EXCEPT ALL`
//!    difference (the same type-agnostic, order-independent comparator the
//!    single-table harness uses). Since v0.12.0 that set includes queries
//!    carrying the parent metric `SUM(u.w)`: they are computed **at the parent's
//!    own grain** (`FROM u`, never through the join) and joined back to the
//!    child-grain aggregate, so the oracle is a per-grain one. This pins both
//!    halves — no inflation, and no over-rejection of safe queries.

use proptest::prelude::*;
use semantic_views::expand::{expand, DimensionName, MetricName, QueryRequest};
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
};

/// A generated star-schema instance: `n_u` parent rows and a list of child
/// rows. Parent row `i` has id `i` (distinct, so the declared PK holds),
/// category `ucat`, and value `w`. Child rows carry a foreign key `fk` (a valid
/// parent id, `None` = NULL, or a dangling id), a dimension `d`, and a value
/// `v`. `None` is a SQL NULL throughout.
#[derive(Debug, Clone)]
struct Instance {
    /// Parent rows: `(ucat, w)` for ids `0..n_u`.
    u_rows: Vec<(Option<i64>, Option<i64>)>,
    /// Child rows: `(fk, d, v)`.
    t_rows: Vec<(Option<i64>, Option<i64>, Option<i64>)>,
}

/// The queryable objects, referenced by these stable names in every case.
const DIMS: [&str; 2] = ["td", "ucat"];
const METS: [&str; 3] = ["sv", "ct", "sw"];

/// Members a generated `where_clause` may name (PBT-6), and which side of the
/// join each lives on. `ftd` / `fucat` are `LABELS = (FILTER)` members whose
/// expressions are compound `OR`s, so a splice that drops the parentheses
/// changes the answer — the same substitution surface the single-table harness
/// exercises, here across a join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WMember {
    /// `t.d` — the CHILD side.
    Td,
    /// `u.ucat` — the PARENT side.
    Ucat,
    /// Filter member on the child: `t.d = 0 OR t.d = 2`.
    Ftd,
    /// Filter member on the parent: `u.ucat = 0 OR u.ucat = 2`.
    Fucat,
}

impl WMember {
    /// The member's declared name, as written in a `where_clause`.
    fn name(self) -> &'static str {
        match self {
            WMember::Td => "td",
            WMember::Ucat => "ucat",
            WMember::Ftd => "ftd",
            WMember::Fucat => "fucat",
        }
    }

    /// The raw SQL the member's expression stands for, for the oracle.
    fn raw(self) -> &'static str {
        match self {
            WMember::Td => "t.d",
            WMember::Ucat => "u.ucat",
            WMember::Ftd => "(t.d = 0 OR t.d = 2)",
            WMember::Fucat => "(u.ucat = 0 OR u.ucat = 2)",
        }
    }

    /// Whether the member lives on the CHILD table. A predicate touching the
    /// child cannot be evaluated inside the parent-grain CTE without joining
    /// `t` into it — which fans `u` — so such a query must be rejected when the
    /// parent metric is selected.
    fn is_child_side(self) -> bool {
        matches!(self, WMember::Td | WMember::Ftd)
    }

    /// Whether the member is a bare column reference (so it can carry a
    /// comparison operator) as opposed to a self-contained boolean filter.
    fn is_comparable(self) -> bool {
        matches!(self, WMember::Td | WMember::Ucat)
    }
}

/// A generated pre-aggregation predicate over the star's members (PBT-6).
#[derive(Debug, Clone)]
enum Pred {
    /// `<member> <op> <literal>` for a comparable member.
    Cmp(WMember, &'static str, i64),
    /// A bare filter member — the substitution/precedence surface.
    Filter(WMember),
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
}

impl Pred {
    /// The `where_clause` text: member NAMES, composites parenthesized so the
    /// SQL parse matches this AST, filter members left BARE.
    fn to_member_sql(&self) -> String {
        match self {
            Pred::Cmp(m, op, k) => format!("{} {op} {k}", m.name()),
            Pred::Filter(m) => m.name().to_string(),
            Pred::And(a, b) => format!("({} AND {})", a.to_member_sql(), b.to_member_sql()),
            Pred::Or(a, b) => format!("({} OR {})", a.to_member_sql(), b.to_member_sql()),
            Pred::Not(a) => format!("(NOT {})", a.to_member_sql()),
        }
    }

    /// The oracle's independent rendering: raw physical columns, filter members
    /// expanded, every operand explicitly parenthesized.
    fn to_raw_sql(&self) -> String {
        match self {
            Pred::Cmp(m, op, k) => format!("({} {op} {k})", m.raw()),
            Pred::Filter(m) => format!("({})", m.raw()),
            Pred::And(a, b) => format!("({} AND {})", a.to_raw_sql(), b.to_raw_sql()),
            Pred::Or(a, b) => format!("({} OR {})", a.to_raw_sql(), b.to_raw_sql()),
            Pred::Not(a) => format!("(NOT {})", a.to_raw_sql()),
        }
    }

    /// Whether any named member lives on the child table.
    fn touches_child(&self) -> bool {
        match self {
            Pred::Cmp(m, _, _) | Pred::Filter(m) => m.is_child_side(),
            Pred::And(a, b) | Pred::Or(a, b) => a.touches_child() || b.touches_child(),
            Pred::Not(a) => a.touches_child(),
        }
    }

    /// Whether any named member is a filter member (generator-coverage guard).
    fn references_filter(&self) -> bool {
        match self {
            Pred::Cmp(m, _, _) | Pred::Filter(m) => !m.is_comparable(),
            Pred::And(a, b) | Pred::Or(a, b) => a.references_filter() || b.references_filter(),
            Pred::Not(a) => a.references_filter(),
        }
    }
}

fn arb_pred() -> impl Strategy<Value = Pred> {
    let leaf = prop_oneof![
        3 => (
            prop_oneof![Just(WMember::Td), Just(WMember::Ucat)],
            prop_oneof![Just("<"), Just("<="), Just("="), Just("<>"), Just(">="), Just(">")],
            -1i64..=3,
        ).prop_map(|(m, op, k)| Pred::Cmp(m, op, k)),
        2 => prop_oneof![Just(WMember::Ftd), Just(WMember::Fucat)].prop_map(Pred::Filter),
    ];
    leaf.prop_recursive(3, 8, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Pred::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Pred::Or(Box::new(a), Box::new(b))),
            inner.prop_map(|a| Pred::Not(Box::new(a))),
        ]
    })
}

/// A full case: an instance plus the non-empty subset of dims + metrics to
/// query (indices into `DIMS` / `METS`), and the optional pre-aggregation
/// predicate (PBT-6).
#[derive(Debug, Clone)]
struct Case {
    inst: Instance,
    sel_dims: Vec<usize>,
    sel_metrics: Vec<usize>,
    where_pred: Option<Pred>,
}

fn arb_instance() -> impl Strategy<Value = Instance> {
    // Small signed value domain + NULL, mirroring the single-table harness.
    let val_cell = prop_oneof![
        1 => Just(None),
        3 => (-5i64..=5).prop_map(Some),
    ];
    let cat_cell = prop_oneof![
        1 => Just(None),
        4 => (0i64..3).prop_map(Some),
    ];
    (1usize..=4).prop_flat_map(move |n_u| {
        let u_row = (cat_cell.clone(), val_cell.clone());
        let u_rows = prop::collection::vec(u_row, n_u);
        // fk: NULL, a valid parent id (0..n_u), or a dangling id (n_u, which is
        // never a generated parent id since ids are 0..n_u).
        let fk_cell = prop_oneof![
            1 => Just(None),
            4 => (0i64..n_u as i64).prop_map(Some),
            1 => Just(Some(n_u as i64)),
        ];
        let t_row = (fk_cell, cat_cell.clone(), val_cell.clone()).prop_map(|(fk, d, v)| (fk, d, v));
        let t_rows = prop::collection::vec(t_row, 0..=20);
        (u_rows, t_rows).prop_map(|(u_rows, t_rows)| Instance { u_rows, t_rows })
    })
}

fn arb_case() -> impl Strategy<Value = Case> {
    arb_instance().prop_flat_map(|inst| {
        let dim_sel =
            prop::sample::subsequence((0..DIMS.len()).collect::<Vec<_>>(), 0..=DIMS.len());
        let met_sel =
            prop::sample::subsequence((0..METS.len()).collect::<Vec<_>>(), 0..=METS.len());
        // PBT-6: one case in four keeps the original no-predicate coverage.
        let where_pred = prop_oneof![
            1 => Just(None),
            3 => arb_pred().prop_map(Some),
        ];
        (Just(inst), dim_sel, met_sel, where_pred)
            .prop_filter(
                "at least one of dimensions/metrics must be selected",
                |(_, sel_dims, sel_metrics, _)| !sel_dims.is_empty() || !sel_metrics.is_empty(),
            )
            .prop_map(|(inst, sel_dims, sel_metrics, where_pred)| Case {
                inst,
                sel_dims,
                sel_metrics,
                where_pred,
            })
    })
}

/// Build the semantic-view definition: root/child table `t` with `t.fk
/// REFERENCES u.id` (ManyToOne), a dimension + metric on each side, and a
/// `count(*)` base metric at the root grain.
fn build_def() -> SemanticViewDefinition {
    let tables = vec![
        TableRef {
            alias: "t".to_string(),
            table: "t".to_string(),
            pk_columns: vec![],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        },
        TableRef {
            alias: "u".to_string(),
            table: "u".to_string(),
            pk_columns: vec!["id".to_string()],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        },
    ];
    let dimensions = vec![
        Dimension {
            name: "td".to_string(),
            expr: "t.d".to_string(),
            source_table: Some("t".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            is_filter: false,
        },
        Dimension {
            name: "ucat".to_string(),
            expr: "u.ucat".to_string(),
            source_table: Some("u".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            is_filter: false,
        },
        // PBT-6 filter members: never selected as output, named only by a
        // generated `where_clause`. Compound expressions, so a splice that
        // loses its parentheses changes the answer.
        Dimension {
            name: "ftd".to_string(),
            expr: "t.d = 0 OR t.d = 2".to_string(),
            source_table: Some("t".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            is_filter: true,
        },
        Dimension {
            name: "fucat".to_string(),
            expr: "u.ucat = 0 OR u.ucat = 2".to_string(),
            source_table: Some("u".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            is_filter: true,
        },
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
        base_metric("sw", "sum(u.w)", Some("u")),
    ];
    let joins = vec![Join {
        from_alias: "t".to_string(),
        table: "u".to_string(),
        fk_columns: vec!["fk".to_string()],
        ref_columns: vec!["id".to_string()],
        name: Some("t_u".to_string()),
        cardinality: Cardinality::ManyToOne,
    }];
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
        resolution_schema_name: None,
        comment: None,
    }
}

/// Create the physical tables and insert the generated rows.
fn make_db(inst: &Instance) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    conn.execute_batch(
        "CREATE TABLE u (id INTEGER, ucat INTEGER, w INTEGER); \
         CREATE TABLE t (fk INTEGER, d INTEGER, v INTEGER);",
    )
    .expect("create tables");

    let cell = |c: &Option<i64>| c.map_or_else(|| "NULL".to_string(), |v| v.to_string());

    if !inst.u_rows.is_empty() {
        let values: Vec<String> = inst
            .u_rows
            .iter()
            .enumerate()
            .map(|(i, (ucat, w))| format!("({i},{},{})", cell(ucat), cell(w)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO u VALUES {};", values.join(",")))
            .expect("insert u rows");
    }
    if !inst.t_rows.is_empty() {
        let values: Vec<String> = inst
            .t_rows
            .iter()
            .map(|(fk, d, v)| format!("({},{},{})", cell(fk), cell(d), cell(v)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO t VALUES {};", values.join(",")))
            .expect("insert t rows");
    }
    conn
}

/// The child-grain (`t`) half of the oracle: the metrics computed over `t`,
/// grouped by the selected dimensions. The FROM is always `t LEFT JOIN u`:
/// because `u.id` is unique, joining `u` never changes `t`'s multiset for
/// `count(*)` / `sum(t.v)`, and grouping by a parent dimension is a plain group
/// key. Metrics-only ⇒ global aggregate (no GROUP BY); anything with dimensions
/// ⇒ GROUP BY the projected dimension ordinals (multiset-equal to the
/// expansion's SELECT DISTINCT for the dims-only case).
fn child_grain_sql(case: &Case) -> String {
    let dim_items: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&i| match DIMS[i] {
            "td" => "t.d AS td".to_string(),
            "ucat" => "u.ucat AS ucat".to_string(),
            other => unreachable!("unexpected dim {other}"),
        })
        .collect();
    let met_items: Vec<String> = case
        .sel_metrics
        .iter()
        .filter(|&&i| METS[i] != "sw")
        .map(|&i| match METS[i] {
            "sv" => "sum(t.v) AS sv".to_string(),
            "ct" => "count(*) AS ct".to_string(),
            other => unreachable!("unexpected child-grain metric {other}"),
        })
        .collect();
    let select_items = dim_items
        .iter()
        .chain(met_items.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let from = "FROM t LEFT JOIN u ON t.fk = u.id";
    // PBT-6: the predicate filters the rows feeding this grain's aggregate,
    // BEFORE the grouping. A parent-side member filters through the LEFT JOIN
    // here, exactly as the expansion's own child-grain CTE does.
    let where_sql = case
        .where_pred
        .as_ref()
        .map_or_else(String::new, |p| format!(" WHERE {}", p.to_raw_sql()));
    if case.sel_dims.is_empty() {
        format!("SELECT {select_items} {from}{where_sql}")
    } else {
        let group_by = (1..=case.sel_dims.len())
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("SELECT {select_items} {from}{where_sql} GROUP BY {group_by}")
    }
}

/// The parent-grain (`u`) half of the oracle: `sum(u.w)` over the parent table
/// **on its own**, never through the join — that is the whole point of
/// computing a metric at its own grain, and the independent statement of what
/// the root-anchored `FROM t LEFT JOIN u` got wrong (each parent row counted
/// once per child row, childless parents dropped entirely).
///
/// Only `ucat` can be selected alongside it: `td` lives below `u`'s grain and
/// the query is rejected before it gets here.
/// PBT-6: the predicate is applied inside THIS grain too, before its
/// aggregate. Only parent-side members can appear — a predicate touching the
/// child is rejected before it reaches the oracle (see the fence branch in the
/// property), because evaluating it here would require joining `t` into the
/// parent CTE and fanning `u`.
fn parent_grain_sql(case: &Case) -> String {
    let where_sql = case
        .where_pred
        .as_ref()
        .map_or_else(String::new, |p| format!(" WHERE {}", p.to_raw_sql()));
    if case.sel_dims.is_empty() {
        format!("SELECT sum(u.w) AS sw FROM u{where_sql}")
    } else {
        format!("SELECT u.ucat AS ucat, sum(u.w) AS sw FROM u{where_sql} GROUP BY 1")
    }
}

/// Independent oracle SQL for a query `expand` should accept.
///
/// Without the parent metric this is the plain child-grain aggregation. With it
/// the two grains are computed separately and combined — `CROSS JOIN` for the
/// dimensionless case (one row each), `FULL OUTER JOIN` on the NULL-safe
/// dimension key otherwise, so a `ucat` present at only one grain survives.
fn oracle_sql(case: &Case) -> String {
    let selects_parent_metric = case.sel_metrics.iter().any(|&i| METS[i] == "sw");
    if !selects_parent_metric {
        return child_grain_sql(case);
    }
    let child_metrics: Vec<&str> = case
        .sel_metrics
        .iter()
        .map(|&i| METS[i])
        .filter(|&m| m != "sw")
        .collect();
    let parent = parent_grain_sql(case);
    if child_metrics.is_empty() {
        // Only the parent metric: one grain, nothing to join it to.
        return parent;
    }
    let mut items: Vec<String> = Vec::new();
    if case.sel_dims.is_empty() {
        for m in &child_metrics {
            items.push(format!("a.{m}"));
        }
        items.push("b.sw".to_string());
        let child = child_grain_sql(case);
        return format!(
            "SELECT {} FROM ({child}) a CROSS JOIN ({parent}) b",
            items.join(", ")
        );
    }
    items.push("COALESCE(a.ucat, b.ucat) AS ucat".to_string());
    for m in &child_metrics {
        items.push(format!("a.{m}"));
    }
    items.push("b.sw".to_string());
    let child = child_grain_sql(case);
    format!(
        "SELECT {} FROM ({child}) a FULL OUTER JOIN ({parent}) b \
         ON a.ucat IS NOT DISTINCT FROM b.ucat",
        items.join(", ")
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn star_join_fence_and_aggregation(case in arb_case()) {
        let def = build_def();
        let req = QueryRequest {
            where_clause: case.where_pred.as_ref().map(Pred::to_member_sql),
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

        let selects_parent_metric = case.sel_metrics.iter().any(|&i| METS[i] == "sw");
        let selects_child_dim = case.sel_dims.iter().any(|&i| DIMS[i] == "td");
        let result = expand("star", &def, &req);

        if selects_parent_metric && selects_child_dim {
            // The parent metric grouped by a CHILD dimension stays rejected:
            // `t.d` is below `u`'s grain, so each parent row genuinely fans
            // across the child values. Per-grain aggregation cannot define this
            // query — only the fan-trap error is correct.
            match result {
                Err(e) => {
                    let msg = e.to_string();
                    prop_assert!(
                        msg.contains("fan trap"),
                        "parent metric + child dimension rejected, but not as a fan trap: {msg}"
                    );
                }
                Ok(sql) => prop_assert!(
                    false,
                    "SUM(u.w) grouped by the child dimension t.d must be rejected, got SQL:\n{sql}"
                ),
            }
            return Ok(());
        }

        // PBT-6: a predicate naming a CHILD-side member alongside the PARENT
        // metric is the where_clause analogue of the case above. Filtering on
        // `t` requires joining it into the parent-grain aggregate, and that
        // join fans `u` — each parent row once per matching child row — so
        // `sum(u.w)` would be inflated. `check_where_clause_fan_traps` must
        // reject it; there is no correct number to return.
        if selects_parent_metric
            && case.where_pred.as_ref().is_some_and(Pred::touches_child)
        {
            match result {
                Err(e) => {
                    let msg = e.to_string();
                    prop_assert!(
                        msg.contains("fan trap"),
                        "parent metric + child-side where_clause member rejected, \
                         but not as a fan trap: {msg}"
                    );
                }
                Ok(sql) => prop_assert!(
                    false,
                    "a where_clause on the child table must not be applied to the \
                     parent-grain metric sum(u.w) -- the join fans it. Got SQL:\n{sql}"
                ),
            }
            return Ok(());
        }

        // Accepted-query branch: must expand and match the independent oracle.
        let expanded = match result {
            Ok(sql) => sql,
            Err(e) => {
                prop_assert!(false, "safe query unexpectedly rejected: {e}");
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
            "star-join expansion disagrees with hand-written LEFT JOIN aggregation \
             (symmetric multiset diff = {})\n--- expanded:\n{}\n--- oracle:\n{}",
            diff, expanded, oracle
        );
    }
}

/// PBT-6 guard: prove the generated cases actually reach the branches the
/// property distinguishes, rather than the suite passing because a branch is
/// never generated. A `prop_assert!` inside an unreachable branch is exactly as
/// green as a correct one, so the branch counts are asserted directly.
///
/// Also pins that the predicate itself varies (CLAUDE.md: the field being
/// present in a struct literal is not coverage — the generator has to vary it).
#[test]
fn generator_reaches_both_where_clause_branches() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    use std::collections::HashSet;

    let mut runner = TestRunner::deterministic();
    let mut with_pred = 0usize;
    let mut without_pred = 0usize;
    let mut with_filter = 0usize;
    // The two branches the property treats differently under a predicate.
    let mut parent_metric_child_pred = 0usize;
    let mut parent_metric_parent_only_pred = 0usize;
    let mut distinct: HashSet<String> = HashSet::new();

    for _ in 0..400 {
        let case = arb_case()
            .new_tree(&mut runner)
            .expect("strategy must produce a value")
            .current();
        let selects_parent_metric = case.sel_metrics.iter().any(|&i| METS[i] == "sw");
        let selects_child_dim = case.sel_dims.iter().any(|&i| DIMS[i] == "td");
        match &case.where_pred {
            None => without_pred += 1,
            Some(p) => {
                with_pred += 1;
                if p.references_filter() {
                    with_filter += 1;
                }
                distinct.insert(p.to_member_sql());
                // The dim-based fence branch takes precedence in the property,
                // so only count cases that actually reach the where-clause one.
                if selects_parent_metric && !selects_child_dim {
                    if p.touches_child() {
                        parent_metric_child_pred += 1;
                    } else {
                        parent_metric_parent_only_pred += 1;
                    }
                }
            }
        }
    }

    assert!(
        with_pred > 0 && without_pred > 0,
        "where_clause must be both present and absent across cases \
         (present={with_pred}, absent={without_pred})"
    );
    assert!(
        with_filter > 0,
        "generator never referenced a filter member -- the cross-table \
         member-substitution surface is untested"
    );
    assert!(
        parent_metric_child_pred > 0,
        "no case reached the parent-metric + child-side-predicate branch; the \
         where_clause fan-trap assertion is unreachable and therefore vacuous"
    );
    assert!(
        parent_metric_parent_only_pred > 0,
        "no case reached the parent-metric + parent-only-predicate branch; the \
         per-grain predicate oracle is never compared"
    );
    assert!(
        distinct.len() > 50,
        "generator produced only {} distinct predicates; search space collapsed",
        distinct.len()
    );
}
