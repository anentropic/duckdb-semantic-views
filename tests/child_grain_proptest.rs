//! Differential proptest for metrics and facts on a **child** of the base
//! table — the NULL-extension direction (EXP-21, code-review 2026-08-06;
//! EXP-25/26/28/29 and PBT-13, code-review 2026-08-08).
//!
//! Both existing join harnesses ([`star_schema_proptest`],
//! [`multi_hop_join_proptest`]) root their tree at the "many"-most table, so
//! every generated join reaches from the base table UPWARDS to a parent. That
//! leaves one whole direction unrandomized: a base table LEFT JOINed DOWN to a
//! child, where a base row with no matching child rows survives the join as a
//! single **NULL-extended** row.
//!
//! That phantom row is what SG-8 exists for. `COUNT(*)` on the child is
//! rewritten to `COUNT(<child pk>)` so it does not count it — but the rewrite
//! matched only the literal `*`, so `COUNT(1)` sailed past every guard and
//! counted the phantom (EXP-21: `COUNT(1)` returned 2 next to `COUNT(*)`'s 1,
//! in one result row, with a single childless order). EXP-21's replacement
//! fence was a per-spelling whitelist and leaked four more ways
//! (EXP-25/EXP-26), so what is randomized here is now the whole family:
//!
//! - constant arguments in every spelling the whitelist missed —
//!   `COUNT(DISTINCT 1)` (multiplicity-invariant but *existence*-sensitive),
//!   `COUNT(1+0)` (a constant expression is not a literal), `MIN(1)`
//!   (empty-group, not multiplicity);
//! - NULL-INSENSITIVE arguments that are not constants at all —
//!   `SUM(COALESCE(li.amount, 99))`, which resurrects the phantom row through
//!   its own NULL child column;
//! - a CROSS-TABLE fact reference — `SUM(orate)` where `orate` is a fact on the
//!   *parent*, aggregated at child grain, which `li -> o` never fans and so no
//!   fan-trap check catches;
//! - a **FACTS** request (PBT-8's blind spot, where EXP-28 landed): a row-level
//!   query on a child fact, whose phantom is a whole result row rather than a
//!   wrong number inside one;
//! - a **`where_clause`** (PBT-13): the pin this harness shipped with, and the
//!   shape that most directly stresses the fence — a predicate that filters out
//!   every child of a real parent turns it into a childless one *at filter
//!   time*, exactly where the phantom-row rewrite has to hold.
//!
//! Scope: SELECTED dimensions are drawn from the BASE table only. Grouping by a
//! child dimension puts childless parents in their own NULL group, whose oracle
//! is a different (and much less independent) formulation; that case is covered
//! by the fixed tests in `src/expand/tests_count_star_rewrite.rs` and
//! `src/expand/tests_phantom_row_guard.rs`. `where_clause` members, by
//! contrast, are drawn from BOTH sides.
//!
//! The oracle is formulated WITHOUT a join at all — each metric is a correlated
//! subquery over `li`, and the fact query is a `FROM li` scan with the parent
//! reached by correlated subquery — so it shares no structure with the
//! expansion's `FROM o LEFT JOIN li` + `CASE WHEN li.id IS NOT NULL` rewrite. A
//! rewrite that guards the wrong column, guards nothing, or guards too much
//! shows up as a non-zero multiset difference.

use proptest::prelude::*;
use semantic_views::expand::{expand, DimensionName, FactName, MetricName, QueryRequest};
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef,
};

/// Base rows `o` have ids `0..n_o`; child rows `li` carry a foreign key into
/// them. `None` is a SQL NULL throughout.
#[derive(Debug, Clone)]
struct Instance {
    /// Base rows: `(region, rate)` for ids `0..n_o`.
    o_rows: Vec<(Option<i64>, Option<i64>)>,
    /// Child rows: `(order_id, amount)`.
    li_rows: Vec<(Option<i64>, Option<i64>)>,
}

/// Queryable objects, by stable name.
const DIMS: [&str; 1] = ["region"];
/// `n_star` is the spelling SG-8 always handled. `n_one`, `s_one`, `n_str` and
/// `n_paren` are the constant-argument spellings it did not — `n_paren` is the
/// redundantly-parenthesized literal raised by review on #203, which the first
/// cut of the constant check still let through. `n_dist`, `n_expr` and `mn_one`
/// are EXP-25's three escapes of the constant WHITELIST; `s_coal` and
/// `s_parent` are EXP-26's two shapes that the whitelist could never have
/// covered because their arguments are not constants. `s_amt` and `mx_amt` are
/// ordinary column aggregates — the controls that must stay untouched.
const METS: [&str; 12] = [
    "n_star", "n_one", "s_one", "n_str", "n_paren", "n_dist", "n_expr", "mn_one", "s_coal",
    "s_parent", "s_amt", "mx_amt",
];
/// The only requestable fact: a row-level child value. A fact query is at the
/// grain of its facts, so a childless parent has no row in it (EXP-28).
const FACTS: [&str; 1] = ["f_amt"];

/// Members a generated `where_clause` may name, and which side of the join each
/// lives on. `fregion` / `camt` / `famt` are `LABELS = (FILTER)` members whose
/// expressions are compound, so a splice that drops the parentheses changes the
/// answer; `camt` and `famt` are the CHILD-side members, the ones that can
/// empty a real parent's child set at filter time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WMember {
    /// `o.region` — the BASE side, comparable.
    Region,
    /// Filter member on the base: `o.region = 0 OR o.region = 2`.
    Fregion,
    /// Filter member on the child: `li.amount > 0`.
    Camt,
    /// Compound filter member on the child: `li.amount > 0 OR li.amount < -1`.
    Famt,
}

impl WMember {
    /// The member's declared name, as written in a `where_clause`.
    fn name(self) -> &'static str {
        match self {
            WMember::Region => "region",
            WMember::Fregion => "fregion",
            WMember::Camt => "camt",
            WMember::Famt => "famt",
        }
    }

    /// The raw SQL the member's expression stands for, evaluated where `o` and
    /// `li` are both in scope (the metrics oracle's correlated subquery).
    fn raw(self) -> &'static str {
        match self {
            WMember::Region => "o.region",
            WMember::Fregion => "(o.region = 0 OR o.region = 2)",
            WMember::Camt => "(li.amount > 0)",
            WMember::Famt => "(li.amount > 0 OR li.amount < -1)",
        }
    }

    /// The same expression as the LEFT JOIN's NULL-extended row sees it: every
    /// child column is NULL there. Used to decide whether a CHILDLESS parent
    /// survives the predicate at all.
    fn raw_phantom(self) -> &'static str {
        match self {
            WMember::Region => "o.region",
            WMember::Fregion => "(o.region = 0 OR o.region = 2)",
            WMember::Camt => "(CAST(NULL AS INTEGER) > 0)",
            WMember::Famt => "(CAST(NULL AS INTEGER) > 0 OR CAST(NULL AS INTEGER) < -1)",
        }
    }

    /// The same expression evaluated with only `li` in scope (the fact oracle's
    /// `FROM li` scan): the parent side is reached by correlated subquery, which
    /// is precisely the join the expansion writes and this oracle must not.
    fn raw_from_child(self) -> &'static str {
        match self {
            WMember::Region => "(SELECT op.region FROM o op WHERE op.id = li.order_id)",
            WMember::Fregion => {
                "((SELECT op.region FROM o op WHERE op.id = li.order_id) = 0 \
                  OR (SELECT op.region FROM o op WHERE op.id = li.order_id) = 2)"
            }
            WMember::Camt => "(li.amount > 0)",
            WMember::Famt => "(li.amount > 0 OR li.amount < -1)",
        }
    }

    /// Whether the member lives on the CHILD table.
    fn is_child_side(self) -> bool {
        matches!(self, WMember::Camt | WMember::Famt)
    }

    /// Whether the member is a bare column reference (so it can carry a
    /// comparison operator) as opposed to a self-contained boolean filter.
    fn is_comparable(self) -> bool {
        matches!(self, WMember::Region)
    }
}

/// A generated pre-aggregation predicate over the view's members (PBT-13).
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

    /// The oracle's independent rendering, with each member's raw expression
    /// chosen by `render` (real row / phantom row / child-only scope) and every
    /// operand explicitly parenthesized.
    fn to_raw_sql(&self, render: fn(WMember) -> &'static str) -> String {
        match self {
            Pred::Cmp(m, op, k) => format!("({} {op} {k})", render(*m)),
            Pred::Filter(m) => format!("({})", render(*m)),
            Pred::And(a, b) => format!("({} AND {})", a.to_raw_sql(render), b.to_raw_sql(render)),
            Pred::Or(a, b) => format!("({} OR {})", a.to_raw_sql(render), b.to_raw_sql(render)),
            Pred::Not(a) => format!("(NOT {})", a.to_raw_sql(render)),
        }
    }

    /// Whether any named member lives on the child table (generator-coverage
    /// guard: the child-side predicate is the one that can empty a real
    /// parent's child set).
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
            Just(WMember::Region),
            prop_oneof![Just("<"), Just("<="), Just("="), Just("<>"), Just(">="), Just(">")],
            -1i64..=3,
        ).prop_map(|(m, op, k)| Pred::Cmp(m, op, k)),
        3 => prop_oneof![
            Just(WMember::Fregion),
            Just(WMember::Camt),
            Just(WMember::Famt),
        ].prop_map(Pred::Filter),
    ];
    leaf.prop_recursive(3, 8, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Pred::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Pred::Or(Box::new(a), Box::new(b))),
            inner.prop_map(|a| Pred::Not(Box::new(a))),
        ]
    })
}

/// What the request asks for. Facts and metrics are mutually exclusive, and the
/// two take entirely different emission paths — aggregate vs row-level — so the
/// generator picks one (PBT-8: every harness pinned `facts: vec![]`).
#[derive(Debug, Clone)]
enum Ask {
    /// Indices into [`METS`], non-empty.
    Metrics(Vec<usize>),
    /// The child fact — a row-level query at line-item grain.
    ChildFacts,
}

#[derive(Debug, Clone)]
struct Case {
    inst: Instance,
    sel_dims: Vec<usize>,
    ask: Ask,
    where_pred: Option<Pred>,
}

fn arb_instance() -> impl Strategy<Value = Instance> {
    // 1..=4 base rows, 0..=6 child rows. Foreign keys are drawn from a range
    // WIDER than the base ids, so dangling references and childless orders
    // both occur often; `None` covers the NULL foreign key.
    (
        prop::collection::vec((prop::option::of(0i64..3), prop::option::of(0i64..4)), 1..5),
        prop::collection::vec(
            (prop::option::of(0i64..6), prop::option::of(-3i64..8)),
            0..7,
        ),
    )
        .prop_map(|(o_rows, li_rows)| Instance { o_rows, li_rows })
}

fn arb_case() -> impl Strategy<Value = Case> {
    (
        arb_instance(),
        prop::collection::vec(any::<bool>(), DIMS.len()),
        prop::collection::vec(any::<bool>(), METS.len()),
        // 1 in 4 requests is a row-level FACTS query.
        prop::bool::weighted(0.25),
        prop::option::weighted(0.6, arb_pred()),
    )
        .prop_map(|(inst, dim_mask, met_mask, want_facts, where_pred)| {
            let sel_dims: Vec<usize> = dim_mask
                .iter()
                .enumerate()
                .filter_map(|(i, &b)| b.then_some(i))
                .collect();
            let mut sel_metrics: Vec<usize> = met_mask
                .iter()
                .enumerate()
                .filter_map(|(i, &b)| b.then_some(i))
                .collect();
            // A request with no metrics and no facts is a different code path
            // (dims-only DISTINCT); this harness is about the aggregates and
            // the row-level fact query.
            if sel_metrics.is_empty() {
                sel_metrics.push(1); // n_one — the metric EXP-21 was about.
            }
            let ask = if want_facts {
                Ask::ChildFacts
            } else {
                Ask::Metrics(sel_metrics)
            };
            Case {
                inst,
                sel_dims,
                ask,
                where_pred,
            }
        })
}

fn build_def() -> SemanticViewDefinition {
    let tables = vec![
        // The BASE table is the parent here — the direction neither other
        // join harness generates.
        TableRef {
            alias: "o".to_string(),
            table: "o".to_string(),
            pk_columns: vec!["id".to_string()],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        },
        TableRef {
            alias: "li".to_string(),
            table: "li".to_string(),
            pk_columns: vec!["id".to_string()],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        },
    ];
    let dimension = |name: &str, expr: &str, source: &str, is_filter: bool| Dimension {
        name: name.to_string(),
        expr: expr.to_string(),
        source_table: Some(source.to_string()),
        output_type: None,
        comment: None,
        synonyms: vec![],
        is_filter,
    };
    let dimensions = vec![
        dimension("region", "o.region", "o", false),
        // PBT-13 filter members: never selected as output, named only by a
        // generated `where_clause`. Compound expressions, so a splice that
        // drops the parentheses changes the answer.
        dimension("fregion", "o.region = 0 OR o.region = 2", "o", true),
        dimension("camt", "li.amount > 0", "li", true),
        dimension("famt", "li.amount > 0 OR li.amount < -1", "li", true),
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
        base_metric("n_star", "COUNT(*)", Some("li")),
        base_metric("n_one", "COUNT(1)", Some("li")),
        base_metric("s_one", "SUM(1)", Some("li")),
        base_metric("n_str", "COUNT('x')", Some("li")),
        base_metric("n_paren", "COUNT((1))", Some("li")),
        base_metric("n_dist", "COUNT(DISTINCT 1)", Some("li")),
        base_metric("n_expr", "COUNT(1+0)", Some("li")),
        base_metric("mn_one", "MIN(1)", Some("li")),
        base_metric("s_coal", "SUM(COALESCE(li.amount, 99))", Some("li")),
        // EXP-26: a child-grain metric reaching a fact on the PARENT table.
        base_metric("s_parent", "SUM(orate)", Some("li")),
        base_metric("s_amt", "SUM(li.amount)", Some("li")),
        base_metric("mx_amt", "MAX(li.amount)", Some("li")),
    ];
    let facts = vec![
        Fact {
            name: "orate".to_string(),
            expr: "o.rate".to_string(),
            source_table: Some("o".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
            is_filter: false,
        },
        Fact {
            name: "f_amt".to_string(),
            expr: "li.amount".to_string(),
            source_table: Some("li".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
            is_filter: false,
        },
    ];
    let joins = vec![Join {
        from_alias: "li".to_string(),
        table: "o".to_string(),
        fk_columns: vec!["order_id".to_string()],
        ref_columns: vec!["id".to_string()],
        name: Some("li_o".to_string()),
        cardinality: Cardinality::ManyToOne,
    }];
    SemanticViewDefinition {
        tables,
        dimensions,
        metrics,
        joins,
        facts,
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        resolution_schema_name: None,
        comment: None,
    }
}

fn make_db(inst: &Instance) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    conn.execute_batch(
        "CREATE TABLE o (id INTEGER, region INTEGER, rate INTEGER); \
         CREATE TABLE li (id INTEGER, order_id INTEGER, amount INTEGER);",
    )
    .expect("create tables");

    let cell = |c: &Option<i64>| c.map_or_else(|| "NULL".to_string(), |v| v.to_string());

    if !inst.o_rows.is_empty() {
        let values: Vec<String> = inst
            .o_rows
            .iter()
            .enumerate()
            .map(|(i, (region, rate))| format!("({i},{},{})", cell(region), cell(rate)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO o VALUES {};", values.join(",")))
            .expect("insert o rows");
    }
    if !inst.li_rows.is_empty() {
        let values: Vec<String> = inst
            .li_rows
            .iter()
            .enumerate()
            .map(|(i, (order_id, amount))| format!("({i},{},{})", cell(order_id), cell(amount)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO li VALUES {};", values.join(",")))
            .expect("insert li rows");
    }
    conn
}

/// The predicate, rendered for the correlated subquery over `li` (both `o` and
/// `li` in scope), as ` AND (...)` or the empty string.
fn subquery_pred(case: &Case) -> String {
    case.where_pred.as_ref().map_or_else(String::new, |p| {
        format!(" AND {}", p.to_raw_sql(WMember::raw))
    })
}

/// Which base rows survive the pre-aggregation predicate at all.
///
/// The expansion filters the JOINED rows: a parent keeps its group when at
/// least one real `(o, li)` row passes, or — when it has no children — when the
/// single NULL-extended row passes with every child column NULL. Formulated
/// with `EXISTS` rather than a join, so it stays independent of the expansion.
fn survivor_pred(case: &Case) -> String {
    let Some(pred) = case.where_pred.as_ref() else {
        return String::new();
    };
    let real = pred.to_raw_sql(WMember::raw);
    let phantom = pred.to_raw_sql(WMember::raw_phantom);
    format!(
        " WHERE (EXISTS (SELECT 1 FROM li WHERE li.order_id = o.id AND {real}) \
           OR (NOT EXISTS (SELECT 1 FROM li WHERE li.order_id = o.id) AND {phantom}))"
    )
}

/// The oracle for a METRICS request: every metric as a correlated subquery over
/// `li` combined across the group, so the formulation shares nothing with the
/// expansion's LEFT JOIN + PK guard.
///
/// `count(*)` in the subquery returns 0 for a childless order and `sum(...)` /
/// `min(...)` return NULL, which is exactly the empty-group semantics each
/// aggregate should have had over its own rows — the property the phantom
/// NULL-extended row destroys. Each metric names its own combiner: summing
/// per-order counts is right for `COUNT`, but `MIN(1)` combines with `min` and
/// `COUNT(DISTINCT 1)` — which is 1 per order with any row and 0 otherwise —
/// combines with `max`.
fn metrics_oracle_sql(case: &Case, sel_metrics: &[usize]) -> String {
    let and_pred = subquery_pred(case);
    let per_order = |i: usize| -> String {
        let inner = match METS[i] {
            "n_star" | "n_one" | "n_str" | "n_paren" | "n_expr" => "count(*)",
            "n_dist" => "count(DISTINCT 1)",
            "s_one" => "sum(1)",
            "s_amt" => "sum(li.amount)",
            "s_coal" => "sum(COALESCE(li.amount, 99))",
            // The cross-table fact reference: the parent's own `o.rate` is
            // added once per line item of that order, and is NULL when the
            // order has none. Written as a product rather than `sum(o.rate)`
            // because DuckDB rejects an aggregate over a purely correlated
            // (outer-constant) argument inside a correlated subquery.
            "s_parent" => "o.rate * NULLIF(count(*), 0)",
            "mn_one" => "min(1)",
            "mx_amt" => "max(li.amount)",
            other => unreachable!("unexpected metric {other}"),
        };
        format!("(SELECT {inner} FROM li WHERE li.order_id = o.id{and_pred})")
    };
    // Combining the per-order values across the group. The COUNT family also
    // needs `coalesce(…, 0)`: a `COUNT` over zero rows is 0, but `sum`/`max`
    // over zero rows is NULL, and a dimensionless request whose predicate
    // removes every base row is exactly that empty global aggregate.
    let combined = |i: usize| -> String {
        let per = per_order(i);
        match METS[i] {
            "n_star" | "n_one" | "n_str" | "n_paren" | "n_expr" => {
                format!("coalesce(sum({per}), 0)")
            }
            "n_dist" => format!("coalesce(max({per}), 0)"),
            "s_one" | "s_amt" | "s_coal" | "s_parent" => format!("sum({per})"),
            "mn_one" => format!("min({per})"),
            "mx_amt" => format!("max({per})"),
            other => unreachable!("unexpected metric {other}"),
        }
    };
    let met_items: Vec<String> = sel_metrics
        .iter()
        .map(|&i| format!("{} AS {}", combined(i), METS[i]))
        .collect();
    let dim_items: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&i| format!("o.{} AS {}", DIMS[i], DIMS[i]))
        .collect();
    let select_items = dim_items
        .iter()
        .chain(met_items.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let where_sql = survivor_pred(case);
    if case.sel_dims.is_empty() {
        format!("SELECT {select_items} FROM o{where_sql}")
    } else {
        let group: Vec<String> = (1..=case.sel_dims.len()).map(|n| n.to_string()).collect();
        format!(
            "SELECT {select_items} FROM o{where_sql} GROUP BY {}",
            group.join(", ")
        )
    }
}

/// The oracle for a FACTS request (EXP-28): one row per REAL line item whose
/// parent exists, scanned straight off `li` with the parent reached by
/// correlated subquery. A childless order contributes nothing — it has no row
/// at line-item grain — which is exactly what the base-anchored
/// `FROM o LEFT JOIN li` manufactured.
fn facts_oracle_sql(case: &Case) -> String {
    let mut items: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&i| {
            let col = DIMS[i];
            format!("(SELECT op.{col} FROM o op WHERE op.id = li.order_id) AS {col}")
        })
        .collect();
    items.push("li.amount AS f_amt".to_string());
    let and_pred = case.where_pred.as_ref().map_or_else(String::new, |p| {
        format!(" AND {}", p.to_raw_sql(WMember::raw_from_child))
    });
    format!(
        "SELECT {} FROM li WHERE EXISTS (SELECT 1 FROM o oe WHERE oe.id = li.order_id){and_pred}",
        items.join(", ")
    )
}

fn oracle_sql(case: &Case) -> String {
    match &case.ask {
        Ask::Metrics(sel) => metrics_oracle_sql(case, sel),
        Ask::ChildFacts => facts_oracle_sql(case),
    }
}

/// The output column names of the request, sorted — the canonical projection
/// that makes a column-order difference between the two formulations not a
/// false diff.
fn projection(case: &Case) -> String {
    let mut cols: Vec<String> = case.sel_dims.iter().map(|&i| DIMS[i].to_string()).collect();
    match &case.ask {
        Ask::Metrics(sel) => cols.extend(sel.iter().map(|&i| METS[i].to_string())),
        Ask::ChildFacts => cols.extend(FACTS.iter().map(ToString::to_string)),
    }
    cols.sort();
    cols.join(", ")
}

fn build_request(case: &Case) -> QueryRequest {
    let (metrics, facts) = match &case.ask {
        Ask::Metrics(sel) => (
            sel.iter().map(|&i| MetricName::new(METS[i])).collect(),
            vec![],
        ),
        Ask::ChildFacts => (
            vec![],
            FACTS.iter().map(|f| FactName::new(*f)).collect::<Vec<_>>(),
        ),
    };
    QueryRequest {
        where_clause: case.where_pred.as_ref().map(Pred::to_member_sql),
        dimensions: case
            .sel_dims
            .iter()
            .map(|&i| DimensionName::new(DIMS[i]))
            .collect(),
        metrics,
        facts,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn child_grain_queries_match_the_oracle(case in arb_case()) {
        let def = build_def();
        let req = build_request(&case);

        let expanded = match expand("child", &def, &req) {
            Ok(sql) => sql,
            Err(e) => {
                prop_assert!(false, "child-grain query unexpectedly rejected: {e}");
                unreachable!()
            }
        };
        let oracle = oracle_sql(&case);
        let proj = projection(&case);

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
        let diff: i64 = conn
            .query_row(&cmp, [], |r| r.get(0))
            .unwrap_or_else(|e| panic!("comparison failed: {e}\nEXPANDED:\n{expanded}\nORACLE:\n{oracle}"));
        prop_assert_eq!(
            diff, 0,
            "expansion disagrees with the oracle\nEXPANDED:\n{}\nORACLE:\n{}",
            expanded, oracle
        );
    }
}

/// Draw a deterministic sample of generated cases for the anti-vacuity guards
/// below, so each one asks its question of the same generator the property uses.
fn sample_cases(n: usize) -> Vec<Case> {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    (0..n)
        .map(|_| arb_case().new_tree(&mut runner).unwrap().current())
        .collect()
}

/// Anti-vacuity guard, in the spirit of the PBT-6 harnesses: the generator must
/// actually produce the shape the property is about — at least one childless
/// base row, which is the only row that can expose EXP-21/25/26/28. Without a
/// childless order every metric spelling agrees and the property proves nothing.
#[test]
fn generator_produces_childless_base_rows() {
    let saw_childless = sample_cases(256).iter().any(|case| {
        let n_o = case.inst.o_rows.len() as i64;
        (0..n_o).any(|id| {
            !case
                .inst
                .li_rows
                .iter()
                .any(|(order_id, _)| *order_id == Some(id))
        })
    });
    assert!(
        saw_childless,
        "the generator never produced a childless base row, so the property is vacuous"
    );
}

/// Companion guard: every metric must be reachable in a request, not merely
/// declared. A mask that never selects one would make the numeric property
/// green without ever testing that spelling.
#[test]
fn generator_selects_every_metric() {
    let mut seen = [false; METS.len()];
    for case in sample_cases(256) {
        if let Ask::Metrics(sel) = &case.ask {
            for &i in sel {
                seen[i] = true;
            }
        }
    }
    for (i, was_seen) in seen.iter().enumerate() {
        assert!(
            *was_seen,
            "metric {} is never selected by the generator",
            METS[i]
        );
    }
}

/// PBT-8: the row-level FACTS path must actually be requested. Every harness
/// pinned `facts: vec![]`, and EXP-28 landed in exactly that cell.
#[test]
fn generator_produces_both_metric_and_fact_requests() {
    let cases = sample_cases(256);
    let facts = cases
        .iter()
        .filter(|c| matches!(c.ask, Ask::ChildFacts))
        .count();
    let metrics = cases.len() - facts;
    assert!(facts >= 16, "only {facts}/256 requests asked for facts");
    assert!(
        metrics >= 16,
        "only {metrics}/256 requests asked for metrics"
    );
}

/// PBT-13: `where_clause` must vary, and must reach both sides of the join and
/// the filter members. A predicate pinned at `None` — the state this harness
/// shipped in — is how EXP-9/EXP-10 reached `main`.
#[test]
fn generator_varies_the_where_clause() {
    let cases = sample_cases(256);
    let with = cases.iter().filter(|c| c.where_pred.is_some()).count();
    let without = cases.len() - with;
    assert!(with >= 32, "only {with}/256 cases carried a predicate");
    assert!(
        without >= 16,
        "only {without}/256 cases omitted the predicate"
    );

    let child = cases
        .iter()
        .filter_map(|c| c.where_pred.as_ref())
        .filter(|p| p.touches_child())
        .count();
    assert!(
        child >= 16,
        "only {child} predicates named a child-side member"
    );

    let filters = cases
        .iter()
        .filter_map(|c| c.where_pred.as_ref())
        .filter(|p| p.references_filter())
        .count();
    assert!(
        filters >= 16,
        "only {filters} predicates named a filter member"
    );
}

/// The shape PBT-13 named specifically: a predicate that removes every child of
/// a REAL parent, so a parent with line items becomes childless *at filter
/// time* — where the phantom-row rewrite has to hold even though the data has
/// no childless parent at all. Checked against `camt` (`li.amount > 0`), the
/// simplest child-side member, over the generated instances.
#[test]
fn generator_empties_a_real_parents_child_set_at_filter_time() {
    let saw = sample_cases(512).iter().any(|case| {
        let Some(pred) = case.where_pred.as_ref() else {
            return false;
        };
        if !pred.touches_child() {
            return false;
        }
        let n_o = case.inst.o_rows.len() as i64;
        (0..n_o).any(|id| {
            let children: Vec<Option<i64>> = case
                .inst
                .li_rows
                .iter()
                .filter(|(order_id, _)| *order_id == Some(id))
                .map(|(_, amount)| *amount)
                .collect();
            // A real parent (has children) all of whose children fail the
            // simplest child-side test the generator can emit.
            !children.is_empty() && children.iter().all(|a| a.is_none_or(|v| v <= 0))
        })
    });
    assert!(
        saw,
        "the generator never emptied a real parent's child set with a child-side predicate"
    );
}
