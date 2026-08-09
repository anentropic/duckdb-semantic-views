//! Self-checking (metamorphic) properties over the query expansion —
//! `_notes/proactive-defect-discovery.md` §2.1 (a) definition algebra,
//! (b) roll-up consistency, (d) data metamorphism.
//!
//! # Why this harness exists, and why it is not a differential one
//!
//! The five numeric harnesses in this directory each compare `expand()` against
//! an **independently formulated oracle**. That works — every oracle written so
//! far has held — but each oracle must be hand-derived per feature, so the
//! oracle is the bottleneck and the generator is pinned down to whatever the
//! oracle can express. Role-playing (PBT-10, TECH-DEBT #66) has had *zero*
//! randomized numeric coverage across three review rounds for exactly that
//! reason: its independent oracle is genuinely hard to write.
//!
//! A metamorphic property needs no oracle. It asserts that two formulations
//! which *must* agree, do:
//!
//! - **(a) definition algebra** — a derived metric `d = f(m1 … mk)` queried on
//!   its own must equal `f` applied to `m1 … mk` queried separately over the
//!   same dimensions. Neither side is "the right answer"; they only have to be
//!   the same answer. This catches the EXP-19/20/24 class generically: any place
//!   where inlining, re-anchoring or role resolution treats a composed metric
//!   differently from its components.
//! - **(b) roll-up consistency** — the ungrouped total of an additive metric
//!   must equal the sum of its values over any grouped query (and `MIN`/`MAX`
//!   must combine with `min`/`max`). Fan-out duplication, a phantom row or a
//!   grain substitution breaks that without anyone knowing the right number.
//! - **(d) data metamorphism** — inserting a PARENT row with no children must
//!   leave every child-grain metric and every child-fact row untouched. That is
//!   precisely the EXP-21/25/26/28/29 invariant.
//!
//! Because there is no oracle to satisfy, the generator is free to range over
//! the cells the oracled harnesses cannot reach:
//!
//! - **role-playing** (PBT-10): `ap` is reached from `o` by TWO named
//!   relationships, and metrics carry `USING` to pick a role. Every property
//!   generates cases where a role-scoped join alias (`ap__<rel>`) is emitted,
//!   and cases where the queried dimension itself lives on the role-played
//!   table and must bind to the right instance;
//! - **hostile identifiers** (PBT-12): every dimension, metric, fact, derived
//!   metric and physical table name is drawn from a style axis covering bare,
//!   upper-case, quoted-with-space, quoted-with-embedded-quote and non-ASCII
//!   spellings — and those names are what the generated `where_clause` and the
//!   generated derived-metric expressions reference;
//! - **four tables in one model**, mixing the downward (base → child) and
//!   upward (base → parent) join directions in a single query;
//! - **`where_clause`** and **FACTS** requests, neither pinned at its inert
//!   default (PBT-8/PBT-13).
//!
//! # Findings this harness produced, and the properties that are `#[ignore]`d
//!
//! Four real defects fell out of the cells above. Per the project rule that a
//! finding is recorded rather than quietly fixed inside a test change, each has
//! a property that states the CORRECT invariant, is `#[ignore]`d, and carries
//! the reproduction inline:
//!
//! - **MM-1** — [`mm1_role_playing_metric_over_a_fact_on_the_role_played_table`]
//! - **MM-2** — [`mm2_role_playing_with_a_quoted_relationship_name`]
//! - **MM-3** — [`mm3_metric_value_changes_when_co_queried_across_grains`]
//! - **MM-4** — [`mm4_childless_parent_manufactures_a_null_group_under_a_child_dimension`]
//!
//! The three live properties are scoped away from those cells (single-grain
//! metric sets for (a)/(b); bare role-relationship names; parent-side grouping
//! for (d)) so the harness lands green and the findings stay visible.

use std::collections::BTreeSet;

use proptest::prelude::*;
use semantic_views::expand::{
    expand, quote_stored_ident, quote_table_ref, DimensionName, FactName, MetricName, QueryRequest,
};
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef,
};

// ---------------------------------------------------------------------------
// Identifier styles (PBT-12)
// ---------------------------------------------------------------------------

/// How one generated name is spelled. Quoted styles are stored in the same form
/// the body parser keeps (`"` delimiters retained, embedded quotes doubled),
/// which is what every reference site — `where_clause` text, a derived metric's
/// expression, `USING (…)` — must also spell.
///
/// Table ALIASES are deliberately NOT styled: an alias is spliced into
/// user-written member expressions verbatim (`o.region`), so a non-bare alias
/// is the user's problem to quote, not the engine's. Physical table names, by
/// contrast, are engine-quoted through `quote_table_ref` and are styled here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Bare,
    Upper,
    Spaced,
    Escaped,
    Unicode,
}

impl Style {
    const ALL: [Style; 5] = [
        Style::Bare,
        Style::Upper,
        Style::Spaced,
        Style::Escaped,
        Style::Unicode,
    ];

    fn apply(self, stem: &str) -> String {
        match self {
            Style::Bare => stem.to_string(),
            Style::Upper => stem.to_ascii_uppercase(),
            Style::Spaced => format!("\"{stem} x\""),
            Style::Escaped => format!("\"{stem}\"\"q\""),
            Style::Unicode => format!("\"{stem}_東é\""),
        }
    }

    fn is_quoted(self) -> bool {
        matches!(self, Style::Spaced | Style::Escaped | Style::Unicode)
    }
}

fn arb_style() -> impl Strategy<Value = Style> {
    prop::sample::select(Style::ALL.to_vec())
}

/// The role-playing relationship names are drawn from the BARE styles only.
///
/// A quoted relationship name is spliced raw into the scoped join alias
/// (`ap__"de p"`) at the reference site while the join clause quotes the whole
/// alias (`"ap__""de p"""`), so the emitted SQL does not parse — see
/// [`mm2_role_playing_with_a_quoted_relationship_name`]. Case still varies, so
/// the `USING` ↔ relationship-name match stays quote-free but not case-fixed.
fn arb_bare_style() -> impl Strategy<Value = Style> {
    prop::sample::select(vec![Style::Bare, Style::Upper])
}

// ---------------------------------------------------------------------------
// The model: four tables, one role-played target
// ---------------------------------------------------------------------------
//
//                 ap  (role-played: reached from `o` by TWO relationships)
//                 ^ ^
//        r_dep   /   \  r_arr
//               /     \
//   li --r_li--> o --r_car--> cr
//   (child)   (BASE)        (plain parent)
//
// `o` is the base table, so `li` joins DOWNWARD (a childless `o` row survives
// the LEFT JOIN as a phantom) and `ap` / `cr` join UPWARD (an attribute of each
// `o` row).

/// Which table a metric aggregates over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Grain {
    /// The child table `li` — below the base.
    Child,
    /// The base table `o`.
    Base,
}

/// Which of the two relationships to the role-played table a metric names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Dep,
    Arr,
}

/// How a metric's per-group values combine into the ungrouped total — the
/// roll-up law property (b) checks. `SUM` and `COUNT` add; `MIN` / `MAX` are not
/// additive but are idempotent under their own combiner, which is the same
/// invariant one level up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comb {
    Sum,
    /// `COUNT`, which differs from `SUM` only at the empty end: a `COUNT` over
    /// zero rows is 0, while `sum()` over zero GROUPS is NULL. A predicate that
    /// removes every base row produces exactly that pair, so the count family
    /// carries its empty-set identity (0) into the comparison. This is the
    /// aggregate's own semantics, not a weakened assertion — for any non-empty
    /// result the two sides are compared unchanged.
    Count,
    Min,
    Max,
}

impl Comb {
    fn sql(self) -> &'static str {
        match self {
            Comb::Sum | Comb::Count => "sum",
            Comb::Min => "min",
            Comb::Max => "max",
        }
    }

    /// Wrap one side of the roll-up comparison in the aggregate's empty-set
    /// identity, where it has one.
    fn identity(self, expr: &str) -> String {
        match self {
            Comb::Count => format!("coalesce({expr}, 0)"),
            _ => expr.to_string(),
        }
    }
}

struct BaseMetric {
    stem: &'static str,
    expr: &'static str,
    grain: Grain,
    role: Option<Role>,
    comb: Comb,
}

/// The base (non-derived) metrics every generated definition declares.
///
/// `dep_*` / `arr_*` are the role-playing half: same expression, different
/// `USING`, so their *numbers* differ only once a dimension on the role-played
/// table is grouped by — which is exactly the cell PBT-10 names.
const BASE_METRICS: [BaseMetric; 14] = [
    BaseMetric {
        stem: "n_li",
        expr: "COUNT(*)",
        grain: Grain::Child,
        role: None,
        comb: Comb::Count,
    },
    BaseMetric {
        stem: "s_amt",
        expr: "SUM(li.amount)",
        grain: Grain::Child,
        role: None,
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "mx_amt",
        expr: "MAX(li.amount)",
        grain: Grain::Child,
        role: None,
        comb: Comb::Max,
    },
    BaseMetric {
        stem: "mn_qty",
        expr: "MIN(li.qty)",
        grain: Grain::Child,
        role: None,
        comb: Comb::Min,
    },
    BaseMetric {
        stem: "s_qty",
        expr: "SUM(li.qty)",
        grain: Grain::Child,
        role: None,
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "dep_amt",
        expr: "SUM(li.amount)",
        grain: Grain::Child,
        role: Some(Role::Dep),
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "dep_cnt",
        expr: "COUNT(*)",
        grain: Grain::Child,
        role: Some(Role::Dep),
        comb: Comb::Count,
    },
    BaseMetric {
        stem: "arr_amt",
        expr: "SUM(li.amount)",
        grain: Grain::Child,
        role: Some(Role::Arr),
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "arr_qty",
        expr: "SUM(li.qty)",
        grain: Grain::Child,
        role: Some(Role::Arr),
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "s_rate",
        expr: "SUM(o.rate)",
        grain: Grain::Base,
        role: None,
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "n_ord",
        expr: "COUNT(*)",
        grain: Grain::Base,
        role: None,
        comb: Comb::Count,
    },
    BaseMetric {
        stem: "dep_rate",
        expr: "SUM(o.rate)",
        grain: Grain::Base,
        role: Some(Role::Dep),
        comb: Comb::Sum,
    },
    BaseMetric {
        stem: "arr_rate",
        expr: "SUM(o.rate)",
        grain: Grain::Base,
        role: Some(Role::Arr),
        comb: Comb::Sum,
    },
    // EXP-26's shape: a NULL-INSENSITIVE aggregate argument. `COALESCE` turns
    // the phantom row's NULL child column back into a value, so this metric is
    // fenced only by `guard_aggregate_args` — nothing about `COUNT(*)` or a
    // constant literal saves it. It keeps that guard load-bearing here.
    BaseMetric {
        stem: "s_coal",
        expr: "SUM(COALESCE(li.amount, 99))",
        grain: Grain::Child,
        role: None,
        comb: Comb::Sum,
    },
];

/// Dimension slots, by index into [`Names::dims`].
const D_REGION: usize = 0;
const D_CITY: usize = 1;
const D_CNAME: usize = 2;
const D_LQTY: usize = 3;
const D_FBASE: usize = 4;
const D_FCHILD: usize = 5;

const DIM_STEMS: [&str; 6] = ["region", "city", "cname", "lqty", "fbase", "fchild"];
/// `(expr, source alias, is_filter)` for each dimension slot.
const DIM_SPECS: [(&str, &str, bool); 6] = [
    ("o.region", "o", false),
    ("ap.city", "ap", false),
    ("cr.cname", "cr", false),
    ("li.qty", "li", false),
    ("o.region > 0", "o", true),
    ("li.amount > 0", "li", true),
];

const FACT_STEMS: [&str; 2] = ["fact_amt", "fact_rate"];
const FACT_SPECS: [(&str, &str); 2] = [("li.amount", "li"), ("o.rate", "o")];

const REL_STEMS: [&str; 4] = ["r_li", "r_dep", "r_arr", "r_car"];
const TABLE_STEMS: [&str; 4] = ["o", "li", "ap", "cr"];

/// Every generated identifier in one definition, in stored form.
#[derive(Debug, Clone)]
struct Names {
    tables: Vec<String>,
    dims: Vec<String>,
    mets: Vec<String>,
    derived: Vec<String>,
    facts: Vec<String>,
    rels: Vec<String>,
    /// The style each name was drawn with — the anti-vacuity guards read this.
    styles: Vec<Style>,
}

impl Names {
    fn any_quoted(&self) -> bool {
        self.styles.iter().any(|s| s.is_quoted())
    }
}

fn arb_names() -> impl Strategy<Value = Names> {
    (
        prop::collection::vec(arb_style(), TABLE_STEMS.len()),
        prop::collection::vec(arb_style(), DIM_STEMS.len()),
        prop::collection::vec(arb_style(), BASE_METRICS.len()),
        prop::collection::vec(arb_style(), MAX_DERIVED),
        prop::collection::vec(arb_style(), FACT_STEMS.len()),
        // r_li and r_car may be spelled any way; r_dep / r_arr are the
        // role-playing pair and stay bare (MM-2).
        (arb_style(), arb_bare_style(), arb_bare_style(), arb_style()),
    )
        .prop_map(|(t, d, m, dv, f, (rl, rd, ra, rc))| {
            let rel_styles = vec![rl, rd, ra, rc];
            let mut styles = Vec::new();
            styles.extend(&t);
            styles.extend(&d);
            styles.extend(&m);
            styles.extend(&dv);
            styles.extend(&f);
            styles.extend(&rel_styles);
            Names {
                tables: t
                    .iter()
                    .zip(TABLE_STEMS)
                    .map(|(s, stem)| s.apply(stem))
                    .collect(),
                dims: d
                    .iter()
                    .zip(DIM_STEMS)
                    .map(|(s, stem)| s.apply(stem))
                    .collect(),
                mets: m
                    .iter()
                    .zip(BASE_METRICS)
                    .map(|(s, bm)| s.apply(bm.stem))
                    .collect(),
                derived: dv
                    .iter()
                    .enumerate()
                    .map(|(i, s)| s.apply(&format!("dm{i}")))
                    .collect(),
                facts: f
                    .iter()
                    .zip(FACT_STEMS)
                    .map(|(s, stem)| s.apply(stem))
                    .collect(),
                rels: rel_styles
                    .iter()
                    .zip(REL_STEMS)
                    .map(|(s, stem)| s.apply(stem))
                    .collect(),
                styles,
            }
        })
}

// ---------------------------------------------------------------------------
// Derived metrics (the `f` of property (a))
// ---------------------------------------------------------------------------

const MAX_DERIVED: usize = 3;

/// The combining operator. `Ratio` is `a / NULLIF(b, 0)` — the shape that turns
/// integer inputs into a DOUBLE result, which is why the comparators below carry
/// a relative tolerance rather than comparing bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Ratio,
}

impl Op {
    const ALL: [Op; 4] = [Op::Add, Op::Sub, Op::Mul, Op::Ratio];

    /// Render `lhs op rhs` from two already-parenthesized operand strings.
    fn render(self, lhs: &str, rhs: &str) -> String {
        match self {
            Op::Add => format!("({lhs}) + ({rhs})"),
            Op::Sub => format!("({lhs}) - ({rhs})"),
            Op::Mul => format!("({lhs}) * ({rhs})"),
            Op::Ratio => format!("({lhs}) / NULLIF(({rhs}), 0)"),
        }
    }
}

/// One operand of a derived metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Term {
    /// A base metric, by index into [`BASE_METRICS`].
    Base(usize),
    /// An EARLIER derived metric in the same chain — the derived-over-derived
    /// axis (PBT-9's open half: no harness generates depth ≥ 2 today).
    Derived(usize),
}

#[derive(Debug, Clone, Copy)]
struct DerivedSpec {
    op: Op,
    lhs: Term,
    rhs: Term,
}

/// The chain `dm0 … dm{k-1}`, where a derived metric may name the one directly
/// before it (`dm_i` lifts to `dm{i-1}`) or the base metrics.
///
/// The *shape* the type can express is any back-reference `dm_j`, `j < i`, and
/// `depth`/`collect_leaves`/`recompute` all handle that generally — but the
/// generator only ever emits `i-1`, so chains are linear rather than branching.
/// That is enough to reach depth 3, which is what the property needs; widening
/// it to an arbitrary `j < i` would re-roll the fixed-seed sample the
/// anti-vacuity guards are calibrated against, so it is a deliberate next step
/// rather than an oversight.
#[derive(Debug, Clone)]
struct Chain {
    specs: Vec<DerivedSpec>,
    /// Which member of the chain the property queries.
    target: usize,
}

impl Chain {
    /// Nesting depth of `dm{i}` counted in derived levels: 1 for a derived
    /// metric over base metrics only, 2 when it names another derived metric.
    fn depth(&self, i: usize) -> usize {
        let d = |t: Term| match t {
            Term::Base(_) => 0,
            Term::Derived(j) => self.depth(j),
        };
        1 + d(self.specs[i].lhs).max(d(self.specs[i].rhs))
    }

    /// The distinct base metrics `dm{i}` bottoms out in, in a stable order.
    fn leaves(&self, i: usize) -> Vec<usize> {
        let mut out = BTreeSet::new();
        self.collect_leaves(i, &mut out);
        out.into_iter().collect()
    }

    fn collect_leaves(&self, i: usize, out: &mut BTreeSet<usize>) {
        for t in [self.specs[i].lhs, self.specs[i].rhs] {
            match t {
                Term::Base(b) => {
                    out.insert(b);
                }
                Term::Derived(j) => self.collect_leaves(j, out),
            }
        }
    }

    /// The chain member's DDL expression, naming its operands by their stored
    /// identifiers — so a quoted metric name has to survive the reference
    /// scanner on the way into the inliner.
    fn expr(&self, i: usize, names: &Names) -> String {
        let spec = self.specs[i];
        let render = |t: Term| match t {
            Term::Base(b) => names.mets[b].clone(),
            Term::Derived(j) => names.derived[j].clone(),
        };
        spec.op.render(&render(spec.lhs), &render(spec.rhs))
    }

    /// The same algebra applied to the SEPARATELY queried leaf columns. This is
    /// not an oracle: it is the definition's own operator tree, evaluated over
    /// numbers the engine produced one metric at a time.
    fn recompute(&self, i: usize, col_of: &dyn Fn(usize) -> String) -> String {
        let spec = self.specs[i];
        let render = |t: Term| match t {
            Term::Base(b) => col_of(b),
            Term::Derived(j) => self.recompute(j, col_of),
        };
        spec.op.render(&render(spec.lhs), &render(spec.rhs))
    }
}

/// Build a chain whose leaves all sit at one grain and (when `single_role` is
/// set) all name the same relationship.
fn arb_chain(grain: Grain, single_role: Option<Role>) -> impl Strategy<Value = Chain> {
    let pool: Vec<usize> = (0..BASE_METRICS.len())
        .filter(|&i| BASE_METRICS[i].grain == grain)
        .filter(|&i| match single_role {
            Some(r) => BASE_METRICS[i].role == Some(r),
            None => true,
        })
        .collect();
    (1usize..=MAX_DERIVED).prop_flat_map(move |k| {
        let pool = pool.clone();
        (
            prop::collection::vec(
                (
                    prop::sample::select(Op::ALL.to_vec()),
                    prop::sample::select(pool.clone()),
                    prop::sample::select(pool.clone()),
                    any::<bool>(),
                    any::<bool>(),
                ),
                k,
            ),
            0..k,
        )
            .prop_map(move |(raw, target)| {
                let specs: Vec<DerivedSpec> = raw
                    .into_iter()
                    .enumerate()
                    .map(|(i, (op, lb, rb, lift_l, lift_r))| {
                        // Lifting an operand to an earlier chain member is what
                        // produces depth ≥ 2; it is only possible from i > 0.
                        let lhs = if lift_l && i > 0 {
                            Term::Derived(i - 1)
                        } else {
                            Term::Base(lb)
                        };
                        let rhs = if lift_r && i > 0 {
                            Term::Derived(i.saturating_sub(1))
                        } else {
                            Term::Base(rb)
                        };
                        DerivedSpec { op, lhs, rhs }
                    })
                    .collect();
                Chain { specs, target }
            })
    })
}

// ---------------------------------------------------------------------------
// where_clause (PBT-13: never pinned at None here)
// ---------------------------------------------------------------------------

/// A member a generated predicate may name. `city` is deliberately absent: a
/// `where_clause` member on the role-played table is refused by design
/// (TECH-DEBT #65), so naming it would test the error path, not the numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WMember {
    /// `region` — comparable, on the base table.
    Region,
    /// `fbase` — a compound `LABELS = (FILTER)` member on the base table.
    FBase,
    /// `fchild` — a compound filter member on the CHILD table, the one that can
    /// empty a real parent's child set at filter time.
    FChild,
}

impl WMember {
    fn slot(self) -> usize {
        match self {
            WMember::Region => D_REGION,
            WMember::FBase => D_FBASE,
            WMember::FChild => D_FCHILD,
        }
    }

    fn is_child_side(self) -> bool {
        self == WMember::FChild
    }

    fn is_comparable(self) -> bool {
        self == WMember::Region
    }
}

#[derive(Debug, Clone)]
enum Pred {
    Cmp(WMember, &'static str, i64),
    Filter(WMember),
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
}

impl Pred {
    /// The `where_clause` text: member NAMES in their stored (possibly quoted)
    /// spelling, composites parenthesized, filter members left bare.
    fn to_member_sql(&self, names: &Names) -> String {
        match self {
            Pred::Cmp(m, op, k) => format!("{} {op} {k}", names.dims[m.slot()]),
            Pred::Filter(m) => names.dims[m.slot()].clone(),
            Pred::And(a, b) => format!(
                "({} AND {})",
                a.to_member_sql(names),
                b.to_member_sql(names)
            ),
            Pred::Or(a, b) => format!("({} OR {})", a.to_member_sql(names), b.to_member_sql(names)),
            Pred::Not(a) => format!("(NOT {})", a.to_member_sql(names)),
        }
    }

    fn touches_child(&self) -> bool {
        match self {
            Pred::Cmp(m, _, _) | Pred::Filter(m) => m.is_child_side(),
            Pred::And(a, b) | Pred::Or(a, b) => a.touches_child() || b.touches_child(),
            Pred::Not(a) => a.touches_child(),
        }
    }

    fn references_filter(&self) -> bool {
        match self {
            Pred::Cmp(m, _, _) | Pred::Filter(m) => !m.is_comparable(),
            Pred::And(a, b) | Pred::Or(a, b) => a.references_filter() || b.references_filter(),
            Pred::Not(a) => a.references_filter(),
        }
    }
}

/// `allow_child` is false when any queried metric aggregates at the BASE grain:
/// filtering on a child member then requires joining the fanning edge, which the
/// fan-trap fence refuses (correctly) rather than answering.
fn arb_pred(allow_child: bool) -> impl Strategy<Value = Pred> {
    let mut filters = vec![WMember::FBase];
    if allow_child {
        filters.push(WMember::FChild);
    }
    let leaf = prop_oneof![
        3 => (
            Just(WMember::Region),
            prop_oneof![Just("<"), Just("<="), Just("="), Just("<>"), Just(">="), Just(">")],
            -1i64..=3,
        ).prop_map(|(m, op, k)| Pred::Cmp(m, op, k)),
        3 => prop::sample::select(filters).prop_map(Pred::Filter),
    ];
    leaf.prop_recursive(3, 8, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Pred::And(Box::new(a), Box::new(b))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Pred::Or(Box::new(a), Box::new(b))),
            inner.prop_map(|a| Pred::Not(Box::new(a))),
        ]
    })
}

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

/// `(region, rate, dep_code, arr_code, car_code)`; the id is the row index.
type ORow = (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);
/// `(order_id, amount, qty)`; the id is the row index. `order_id` may be NULL (a
/// legal nullable FK) or point past the last `o` row (a dangling reference).
type LiRow = (Option<i64>, Option<i64>, Option<i64>);

#[derive(Debug, Clone)]
struct Data {
    o: Vec<ORow>,
    li: Vec<LiRow>,
}

impl Data {
    /// Base rows with no child rows at all — the phantom LEFT JOIN row's source.
    fn childless_parents(&self) -> usize {
        (0..self.o.len() as i64)
            .filter(|id| !self.li.iter().any(|(fk, _, _)| *fk == Some(*id)))
            .count()
    }

    /// Child rows whose foreign key is NULL or points at no base row. Both are
    /// dropped by a base-anchored `FROM o LEFT JOIN li` and kept by a
    /// child-anchored grain CTE — see MM-3.
    fn unparented_children(&self) -> usize {
        let n = self.o.len() as i64;
        self.li
            .iter()
            .filter(|(fk, _, _)| fk.is_none_or(|v| v < 0 || v >= n))
            .count()
    }
}

fn arb_data() -> impl Strategy<Value = Data> {
    (
        prop::collection::vec(
            (
                prop::option::of(0i64..3),
                prop::option::of(-2i64..7),
                prop::option::of(1i64..4),
                prop::option::of(1i64..4),
                prop::option::of(1i64..3),
            ),
            1..=4,
        ),
        prop::collection::vec(
            (
                // 0..=4 covers every base id plus one dangling value.
                prop::option::of(0i64..5),
                prop::option::of(-3i64..8),
                prop::option::of(0i64..5),
            ),
            0..=6,
        ),
    )
        .prop_map(|(o, li)| Data { o, li })
}

// ---------------------------------------------------------------------------
// Definition + database construction
// ---------------------------------------------------------------------------

fn build_def(names: &Names, chain: Option<&Chain>) -> SemanticViewDefinition {
    let table = |i: usize, alias: &str, pk: &str| TableRef {
        alias: alias.to_string(),
        table: names.tables[i].clone(),
        pk_columns: vec![pk.to_string()],
        ..Default::default()
    };
    let tables = vec![
        table(0, "o", "id"),
        table(1, "li", "id"),
        table(2, "ap", "code"),
        table(3, "cr", "code"),
    ];

    let dimensions: Vec<Dimension> = DIM_SPECS
        .iter()
        .enumerate()
        .map(|(i, (expr, src, is_filter))| Dimension {
            name: names.dims[i].clone(),
            expr: (*expr).to_string(),
            source_table: Some((*src).to_string()),
            is_filter: *is_filter,
            ..Default::default()
        })
        .collect();

    let facts: Vec<Fact> = FACT_SPECS
        .iter()
        .enumerate()
        .map(|(i, (expr, src))| Fact {
            name: names.facts[i].clone(),
            expr: (*expr).to_string(),
            source_table: Some((*src).to_string()),
            access: AccessModifier::Public,
            ..Default::default()
        })
        .collect();

    let mut metrics: Vec<Metric> = BASE_METRICS
        .iter()
        .enumerate()
        .map(|(i, bm)| Metric {
            name: names.mets[i].clone(),
            expr: bm.expr.to_string(),
            source_table: Some(
                match bm.grain {
                    Grain::Child => "li",
                    Grain::Base => "o",
                }
                .to_string(),
            ),
            using_relationships: match bm.role {
                Some(Role::Dep) => vec![names.rels[1].clone()],
                Some(Role::Arr) => vec![names.rels[2].clone()],
                None => vec![],
            },
            ..Default::default()
        })
        .collect();
    if let Some(chain) = chain {
        for i in 0..chain.specs.len() {
            metrics.push(Metric {
                name: names.derived[i].clone(),
                expr: chain.expr(i, names),
                source_table: None,
                ..Default::default()
            });
        }
    }

    let joins = vec![
        Join {
            table: "o".to_string(),
            from_alias: "li".to_string(),
            fk_columns: vec!["order_id".to_string()],
            ref_columns: vec!["id".to_string()],
            name: Some(names.rels[0].clone()),
            cardinality: Cardinality::ManyToOne,
            ..Default::default()
        },
        Join {
            table: "ap".to_string(),
            from_alias: "o".to_string(),
            fk_columns: vec!["dep_code".to_string()],
            ref_columns: vec!["code".to_string()],
            name: Some(names.rels[1].clone()),
            cardinality: Cardinality::ManyToOne,
            ..Default::default()
        },
        Join {
            table: "ap".to_string(),
            from_alias: "o".to_string(),
            fk_columns: vec!["arr_code".to_string()],
            ref_columns: vec!["code".to_string()],
            name: Some(names.rels[2].clone()),
            cardinality: Cardinality::ManyToOne,
            ..Default::default()
        },
        Join {
            table: "cr".to_string(),
            from_alias: "o".to_string(),
            fk_columns: vec!["car_code".to_string()],
            ref_columns: vec!["code".to_string()],
            name: Some(names.rels[3].clone()),
            cardinality: Cardinality::ManyToOne,
            ..Default::default()
        },
    ];

    SemanticViewDefinition {
        tables,
        dimensions,
        metrics,
        facts,
        joins,
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        resolution_schema_name: None,
        comment: None,
    }
}

fn cell(v: &Option<i64>) -> String {
    v.map_or_else(|| "NULL".to_string(), |x| x.to_string())
}

/// Load the fixture. `extra_o` is appended verbatim to the `o` VALUES list —
/// property (d) uses it to add the childless parent.
fn make_db(names: &Names, data: &Data, extra_o: &str) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    let t = |i: usize| quote_table_ref(&names.tables[i]);
    conn.execute_batch(&format!(
        "CREATE TABLE {o} (id INTEGER, region INTEGER, rate INTEGER, dep_code INTEGER, \
                           arr_code INTEGER, car_code INTEGER); \
         CREATE TABLE {li} (id INTEGER, order_id INTEGER, amount INTEGER, qty INTEGER); \
         CREATE TABLE {ap} (code INTEGER, city INTEGER, alt INTEGER); \
         CREATE TABLE {cr} (code INTEGER, cname INTEGER); \
         INSERT INTO {ap} VALUES (1,10,100),(2,20,200),(3,NULL,300); \
         INSERT INTO {cr} VALUES (1,111),(2,222);",
        o = t(0),
        li = t(1),
        ap = t(2),
        cr = t(3),
    ))
    .expect("create fixture tables");

    let o_values: Vec<String> = data
        .o
        .iter()
        .enumerate()
        .map(|(i, (region, rate, dep, arr, car))| {
            format!(
                "({i},{},{},{},{},{})",
                cell(region),
                cell(rate),
                cell(dep),
                cell(arr),
                cell(car)
            )
        })
        .collect();
    conn.execute_batch(&format!(
        "INSERT INTO {} VALUES {}{extra_o};",
        t(0),
        o_values.join(",")
    ))
    .expect("insert base rows");

    if !data.li.is_empty() {
        let li_values: Vec<String> = data
            .li
            .iter()
            .enumerate()
            .map(|(i, (fk, amount, qty))| {
                format!("({i},{},{},{})", cell(fk), cell(amount), cell(qty))
            })
            .collect();
        conn.execute_batch(&format!(
            "INSERT INTO {} VALUES {};",
            t(1),
            li_values.join(",")
        ))
        .expect("insert child rows");
    }
    conn
}

/// A childless parent that clones `o` row 0's dimension columns and carries a
/// NULL measure — so every group it could land in already exists and no base
/// aggregate over `rate` can see it. Property (d) asserts it is invisible.
fn childless_parent_row(data: &Data) -> String {
    let (region, _, dep, arr, car) = data.o[0];
    format!(
        ",(9000,{},NULL,{},{},{})",
        cell(&region),
        cell(&dep),
        cell(&arr),
        cell(&car)
    )
}

// ---------------------------------------------------------------------------
// Comparison plumbing
// ---------------------------------------------------------------------------

/// A SQL boolean that is TRUE when `a` and `b` differ.
///
/// `IS NOT DISTINCT FROM` makes NULLs compare equal (a metric over an empty
/// group is NULL on both sides and that is agreement, not a difference); the
/// second arm is the relative-tolerance escape for the DOUBLE results the
/// `Ratio` operator produces.
fn differs(a: &str, b: &str) -> String {
    format!(
        "(NOT (({a}) IS NOT DISTINCT FROM ({b})) \
          AND NOT (({a}) IS NOT NULL AND ({b}) IS NOT NULL \
                   AND abs(CAST(({a}) AS DOUBLE) - CAST(({b}) AS DOUBLE)) \
                       <= 1e-9 * greatest(1.0, abs(CAST(({a}) AS DOUBLE)), \
                                               abs(CAST(({b}) AS DOUBLE)))))"
    )
}

/// The output column of member `name`, as referenced from an outer SELECT.
/// `quote_stored_ident` is the same function the expansion uses to write the
/// `AS` alias, so a hostile name round-trips exactly.
fn out_col(name: &str) -> String {
    quote_stored_ident(name)
}

/// Wrap one expanded query as a CTE projecting the selected dimensions to
/// `k0…kn`, the named member to `v`, and a presence marker to `p`.
///
/// The constant `k0` in the dimensionless case makes the join shape uniform.
fn as_cte(sql: &str, dim_names: &[String], member: &str, suffix: &str) -> String {
    let mut items: Vec<String> = if dim_names.is_empty() {
        vec!["0 AS k0".to_string()]
    } else {
        dim_names
            .iter()
            .enumerate()
            .map(|(i, n)| format!("z.{} AS k{i}", out_col(n)))
            .collect()
    };
    items.push(format!("z.{} AS v{suffix}", out_col(member)));
    items.push(format!("1 AS p{suffix}"));
    format!("SELECT {} FROM ({sql}) z", items.join(", "))
}

fn key_count(dim_names: &[String]) -> usize {
    dim_names.len().max(1)
}

fn key_list(n: usize) -> String {
    (0..n)
        .map(|i| format!("k{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn key_match(n: usize, left: &str, right: &str) -> String {
    (0..n)
        .map(|i| format!("{left}.k{i} IS NOT DISTINCT FROM {right}.k{i}"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

// ---------------------------------------------------------------------------
// Request assembly
// ---------------------------------------------------------------------------

fn request(
    names: &Names,
    dims: &[usize],
    metrics: &[String],
    facts: &[String],
    pred: Option<&Pred>,
) -> QueryRequest {
    QueryRequest {
        where_clause: pred.map(|p| p.to_member_sql(names)),
        dimensions: dims
            .iter()
            .map(|&i| DimensionName::new(names.dims[i].clone()))
            .collect(),
        metrics: metrics.iter().map(MetricName::new).collect(),
        facts: facts.iter().map(FactName::new).collect(),
    }
}

fn expand_or_fail(
    def: &SemanticViewDefinition,
    req: &QueryRequest,
    what: &str,
) -> Result<String, String> {
    expand("mm", def, req).map_err(|e| format!("{what} was rejected: {e}"))
}

// ---------------------------------------------------------------------------
// (a) Definition algebra
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct AlgebraCase {
    names: Names,
    data: Data,
    chain: Chain,
    /// Dimension slots selected as OUTPUT dimensions.
    dims: Vec<usize>,
    pred: Option<Pred>,
    /// Set when the case groups by the role-played table's dimension, so the
    /// scoped join alias is what the numbers depend on.
    groups_by_role_played: bool,
    grain: Grain,
    role: Option<Role>,
}

fn arb_algebra_case() -> impl Strategy<Value = AlgebraCase> {
    (
        arb_names(),
        arb_data(),
        prop::sample::select(vec![Grain::Child, Grain::Base]),
        // A role is forced 40% of the time; that is the branch that can group by
        // the role-played table's dimension at all.
        prop::option::weighted(0.4, prop::sample::select(vec![Role::Dep, Role::Arr])),
        prop::collection::vec(any::<bool>(), 3),
        prop::option::weighted(0.6, arb_pred(true)),
        any::<bool>(),
    )
        .prop_flat_map(
            |(names, data, grain, role, dim_mask, pred_seed, want_city)| {
                arb_chain(grain, role).prop_map(move |chain| {
                    let mut dims = Vec::new();
                    if dim_mask[0] {
                        dims.push(D_REGION);
                    }
                    if dim_mask[1] {
                        dims.push(D_CNAME);
                    }
                    // A dimension on the child table forces the query to the
                    // child grain; a base-grain metric alongside it is a fan
                    // trap (refused by design), so it is only offered there.
                    if dim_mask[2] && grain == Grain::Child {
                        dims.push(D_LQTY);
                    }
                    // The role-played dimension: legal exactly when every
                    // queried metric names the same single relationship, which
                    // `arb_chain(grain, Some(role))` guarantees.
                    let groups_by_role_played = want_city && role.is_some();
                    if groups_by_role_played {
                        dims.push(D_CITY);
                    }
                    // A child-side predicate needs the child edge joined, which
                    // a base-grain metric cannot tolerate.
                    let pred = match (&pred_seed, grain) {
                        (Some(p), Grain::Child) => Some(p.clone()),
                        (Some(p), Grain::Base) if !p.touches_child() => Some(p.clone()),
                        _ => None,
                    };
                    AlgebraCase {
                        names: names.clone(),
                        data: data.clone(),
                        chain,
                        dims,
                        pred,
                        groups_by_role_played,
                        grain,
                        role,
                    }
                })
            },
        )
}

/// Run property (a) for one case; `Ok(true)` when the comparison ran and agreed.
fn check_algebra(case: &AlgebraCase) -> Result<(), String> {
    let names = &case.names;
    let def = build_def(names, Some(&case.chain));
    let target = case.chain.target;
    let leaves = case.chain.leaves(target);
    let dim_names: Vec<String> = case.dims.iter().map(|&i| names.dims[i].clone()).collect();

    let derived_name = names.derived[target].clone();
    let sql_d = expand_or_fail(
        &def,
        &request(
            names,
            &case.dims,
            &[derived_name.clone()],
            &[],
            case.pred.as_ref(),
        ),
        "the derived-metric query",
    )?;

    let mut ctes = vec![format!(
        "q_t AS ({})",
        as_cte(&sql_d, &dim_names, &derived_name, "_t")
    )];
    for (n, &leaf) in leaves.iter().enumerate() {
        let met = names.mets[leaf].clone();
        let sql_m = expand_or_fail(
            &def,
            &request(names, &case.dims, &[met.clone()], &[], case.pred.as_ref()),
            &format!("the component query for metric {n}"),
        )?;
        ctes.push(format!(
            "q_{n} AS ({})",
            as_cte(&sql_m, &dim_names, &met, &format!("_{n}"))
        ));
    }

    let nk = key_count(&dim_names);
    let keys = std::iter::once("SELECT ".to_string() + &key_list(nk) + " FROM q_t")
        .chain((0..leaves.len()).map(|n| format!("SELECT {} FROM q_{n}", key_list(nk))))
        .collect::<Vec<_>>()
        .join(" UNION ");
    ctes.push(format!("keys AS ({keys})"));

    let mut joins = String::new();
    joins.push_str(&format!(
        " LEFT JOIN q_t ON {}",
        key_match(nk, "keys", "q_t")
    ));
    for n in 0..leaves.len() {
        joins.push_str(&format!(
            " LEFT JOIN q_{n} ON {}",
            key_match(nk, "keys", &format!("q_{n}"))
        ));
    }

    let col_of = |b: usize| {
        let n = leaves.iter().position(|&l| l == b).expect("leaf index");
        format!("q_{n}.v_{n}")
    };
    let recomputed = case.chain.recompute(target, &col_of);

    let mut missing = vec!["q_t.p_t IS NULL".to_string()];
    for n in 0..leaves.len() {
        missing.push(format!("q_{n}.p_{n} IS NULL"));
    }

    let cmp = format!(
        "WITH {} SELECT count(*) FROM keys{joins} WHERE ({}) OR {}",
        ctes.join(", "),
        missing.join(" OR "),
        differs("q_t.v_t", &recomputed)
    );

    let conn = make_db(names, &case.data, "");
    let diff: i64 = conn
        .query_row(&cmp, [], |r| r.get(0))
        .map_err(|e| format!("comparison query failed: {e}\nSQL:\n{cmp}"))?;
    if diff == 0 {
        Ok(())
    } else {
        Err(format!(
            "{diff} row(s) where the derived metric disagrees with its own components\
             \nDERIVED SQL:\n{sql_d}\nCOMPARISON:\n{cmp}"
        ))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// (a) Definition algebra. `d = f(m1 … mk)` queried alone must equal `f`
    /// applied to `m1 … mk` queried separately over the same dimensions and the
    /// same `where_clause`.
    ///
    /// Scope: the leaf metrics share one grain. A derived metric whose leaves
    /// span two grains takes the per-grain re-anchoring path, which changes a
    /// component's own value — see
    /// [`mm3_metric_value_changes_when_co_queried_across_grains`].
    #[test]
    fn derived_metric_equals_its_components(case in arb_algebra_case()) {
        if let Err(msg) = check_algebra(&case) {
            prop_assert!(false, "{}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// (b) Roll-up consistency
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RollupCase {
    names: Names,
    data: Data,
    /// Indices into [`BASE_METRICS`], all at one grain.
    metrics: Vec<usize>,
    dims: Vec<usize>,
    pred: Option<Pred>,
    groups_by_role_played: bool,
    grain: Grain,
}

fn arb_rollup_case() -> impl Strategy<Value = RollupCase> {
    (
        arb_names(),
        arb_data(),
        prop::sample::select(vec![Grain::Child, Grain::Base]),
        prop::option::weighted(0.4, prop::sample::select(vec![Role::Dep, Role::Arr])),
        prop::collection::vec(any::<bool>(), 3),
        prop::option::weighted(0.6, arb_pred(true)),
        any::<bool>(),
        prop::collection::vec(any::<bool>(), BASE_METRICS.len()),
    )
        .prop_map(
            |(names, data, grain, role, dim_mask, pred_seed, want_city, met_mask)| {
                let eligible: Vec<usize> = (0..BASE_METRICS.len())
                    .filter(|&i| BASE_METRICS[i].grain == grain)
                    .filter(|&i| match role {
                        Some(r) => BASE_METRICS[i].role == Some(r),
                        None => true,
                    })
                    .collect();
                let mut metrics: Vec<usize> = eligible
                    .iter()
                    .copied()
                    .filter(|&i| met_mask[i])
                    .take(3)
                    .collect();
                if metrics.is_empty() {
                    metrics.push(eligible[0]);
                }
                let mut dims = Vec::new();
                if dim_mask[0] {
                    dims.push(D_REGION);
                }
                if dim_mask[1] {
                    dims.push(D_CNAME);
                }
                if dim_mask[2] && grain == Grain::Child {
                    dims.push(D_LQTY);
                }
                let groups_by_role_played = want_city && role.is_some();
                if groups_by_role_played {
                    dims.push(D_CITY);
                }
                let pred = match (&pred_seed, grain) {
                    (Some(p), Grain::Child) => Some(p.clone()),
                    (Some(p), Grain::Base) if !p.touches_child() => Some(p.clone()),
                    _ => None,
                };
                RollupCase {
                    names,
                    data,
                    metrics,
                    dims,
                    pred,
                    groups_by_role_played,
                    grain,
                }
            },
        )
}

fn check_rollup(case: &RollupCase) -> Result<(), String> {
    let names = &case.names;
    let def = build_def(names, None);
    let met_names: Vec<String> = case
        .metrics
        .iter()
        .map(|&i| names.mets[i].clone())
        .collect();

    let grouped = expand_or_fail(
        &def,
        &request(names, &case.dims, &met_names, &[], case.pred.as_ref()),
        "the grouped query",
    )?;
    let total = expand_or_fail(
        &def,
        &request(names, &[], &met_names, &[], case.pred.as_ref()),
        "the ungrouped query",
    )?;

    let conn = make_db(names, &case.data, "");
    for (&idx, name) in case.metrics.iter().zip(&met_names) {
        let comb = BASE_METRICS[idx].comb;
        let col = out_col(name);
        let sql = format!(
            "SELECT count(*) FROM \
               (SELECT {}({col}) AS g FROM ({grouped}) gq) a, \
               (SELECT {col} AS t FROM ({total}) tq) b \
             WHERE {}",
            comb.sql(),
            differs(&comb.identity("a.g"), &comb.identity("b.t"))
        );
        let diff: i64 = conn
            .query_row(&sql, [], |r| r.get(0))
            .map_err(|e| format!("roll-up query failed: {e}\nSQL:\n{sql}"))?;
        if diff != 0 {
            return Err(format!(
                "metric {} does not roll up ({} over the grouped rows \
                 differs from the ungrouped total)\nGROUPED:\n{grouped}\nTOTAL:\n{total}",
                BASE_METRICS[idx].stem,
                comb.sql()
            ));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// (b) Roll-up consistency. Combining a metric's per-group values with its
    /// own combiner (`sum` for `SUM`/`COUNT`, `min`/`max` for `MIN`/`MAX`) must
    /// reproduce the ungrouped total, for every generated grouping — including
    /// groupings on the role-played table, where the scoped join alias decides
    /// which rows land in which group.
    ///
    /// Any fan-out duplication, phantom row or grain substitution shows up here
    /// without anyone having to know the right number.
    ///
    /// This harness declares no semi-additive metric: a `NON ADDITIVE BY` metric
    /// is legitimately non-additive across its own dimension, and its numeric
    /// coverage already has a differential oracle in `semi_additive_proptest`.
    #[test]
    fn every_metric_rolls_up_over_every_grouping(case in arb_rollup_case()) {
        if let Err(msg) = check_rollup(&case) {
            prop_assert!(false, "{}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// (d) Data metamorphism
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PhantomCase {
    names: Names,
    data: Data,
    /// Child-grain metrics only (indices into [`BASE_METRICS`]).
    metrics: Vec<usize>,
    /// When set the request asks for the child FACT instead of metrics.
    facts_request: bool,
    dims: Vec<usize>,
    pred: Option<Pred>,
    groups_by_role_played: bool,
}

fn arb_phantom_case() -> impl Strategy<Value = PhantomCase> {
    (
        arb_names(),
        arb_data(),
        prop::option::weighted(0.4, prop::sample::select(vec![Role::Dep, Role::Arr])),
        prop::collection::vec(any::<bool>(), 2),
        prop::option::weighted(0.6, arb_pred(true)),
        any::<bool>(),
        prop::collection::vec(any::<bool>(), BASE_METRICS.len()),
        // One request in four is a row-level FACTS query (PBT-8 / EXP-28).
        prop::bool::weighted(0.25),
    )
        .prop_map(
            |(names, data, role, dim_mask, pred, want_city, met_mask, facts_request)| {
                let eligible: Vec<usize> = (0..BASE_METRICS.len())
                    .filter(|&i| BASE_METRICS[i].grain == Grain::Child)
                    .filter(|&i| match role {
                        Some(r) => BASE_METRICS[i].role == Some(r),
                        None => true,
                    })
                    .collect();
                let mut metrics: Vec<usize> = eligible
                    .iter()
                    .copied()
                    .filter(|&i| met_mask[i])
                    .take(3)
                    .collect();
                if metrics.is_empty() {
                    metrics.push(eligible[0]);
                }
                let mut dims = Vec::new();
                if dim_mask[0] {
                    dims.push(D_REGION);
                }
                if dim_mask[1] {
                    dims.push(D_CNAME);
                }
                // NOT the child dimension: grouping by one puts the childless
                // parent in a manufactured NULL group on the aggregate path —
                // see `mm4_…`. A role-played dimension needs the request to
                // carry USING, which a FACTS-only request cannot.
                let groups_by_role_played = want_city && role.is_some() && !facts_request;
                if groups_by_role_played {
                    dims.push(D_CITY);
                }
                PhantomCase {
                    names,
                    data,
                    metrics,
                    facts_request,
                    dims,
                    pred,
                    groups_by_role_played,
                }
            },
        )
}

/// Run one expanded query over both databases and return the two canonical
/// renderings (row multiset, order-independent).
fn render_both(
    sql: &str,
    before: &duckdb::Connection,
    after: &duckdb::Connection,
) -> Result<(String, String), String> {
    let wrapped = format!(
        "SELECT COALESCE(string_agg(s, ' | ' ORDER BY s), '<empty>') \
         FROM (SELECT CAST(t AS VARCHAR) AS s FROM ({sql}) t)"
    );
    let read = |c: &duckdb::Connection| {
        c.query_row(&wrapped, [], |r| r.get::<_, String>(0))
            .map_err(|e| format!("query failed: {e}\nSQL:\n{wrapped}"))
    };
    Ok((read(before)?, read(after)?))
}

fn check_phantom(case: &PhantomCase) -> Result<(), String> {
    let names = &case.names;
    let def = build_def(names, None);
    let (metrics, facts): (Vec<String>, Vec<String>) = if case.facts_request {
        (vec![], vec![names.facts[0].clone()])
    } else {
        (
            case.metrics
                .iter()
                .map(|&i| names.mets[i].clone())
                .collect(),
            vec![],
        )
    };
    let req = request(names, &case.dims, &metrics, &facts, case.pred.as_ref());
    let sql = expand_or_fail(&def, &req, "the child-grain query")?;

    let before = make_db(names, &case.data, "");
    let after = make_db(names, &case.data, &childless_parent_row(&case.data));
    let (b, a) = render_both(&sql, &before, &after)?;
    if a == b {
        Ok(())
    } else {
        Err(format!(
            "inserting a childless parent changed a child-grain result\
             \nBEFORE: {b}\nAFTER:  {a}\nSQL:\n{sql}"
        ))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// (d) Data metamorphism. Inserting one extra BASE row that has no child
    /// rows — and whose dimension columns clone an existing base row, so no new
    /// group can appear — must leave every child-grain metric and every
    /// child-fact row byte-identical.
    ///
    /// That is the EXP-21/25/26/28/29 invariant stated as a property of the
    /// data rather than of any particular aggregate spelling: it does not care
    /// which aggregate function, which constant-argument spelling, or which
    /// request shape manufactured the phantom.
    #[test]
    fn a_childless_parent_changes_no_child_grain_result(case in arb_phantom_case()) {
        if let Err(msg) = check_phantom(&case) {
            prop_assert!(false, "{}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// Anti-vacuity guards
// ---------------------------------------------------------------------------

fn sample<S: Strategy>(strategy: &S, n: usize) -> Vec<S::Value> {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    (0..n)
        .map(|_| strategy.new_tree(&mut runner).unwrap().current())
        .collect()
}

/// PBT-10, the headline: the generator must actually reach the role-played
/// table — both by declaring `USING` metrics (which emit a scoped join alias)
/// and by GROUPING BY a dimension on the role-played table, where the scoped
/// alias decides the numbers.
#[test]
fn generator_reaches_the_role_playing_edges() {
    let cases = sample(&arb_algebra_case(), 256);
    let with_role = cases.iter().filter(|c| c.role.is_some()).count();
    let by_city = cases.iter().filter(|c| c.groups_by_role_played).count();
    let dep = cases.iter().filter(|c| c.role == Some(Role::Dep)).count();
    let arr = cases.iter().filter(|c| c.role == Some(Role::Arr)).count();
    assert!(
        with_role >= 32,
        "only {with_role}/256 algebra cases named a role-playing relationship"
    );
    assert!(
        by_city >= 16,
        "only {by_city}/256 algebra cases grouped by the role-played dimension, \
         so the scoped join alias never decided a number"
    );
    assert!(
        dep >= 8 && arr >= 8,
        "roles are lopsided: dep={dep} arr={arr}"
    );

    let rollups = sample(&arb_rollup_case(), 256);
    let r_city = rollups.iter().filter(|c| c.groups_by_role_played).count();
    assert!(
        r_city >= 16,
        "only {r_city}/256 roll-up cases grouped by the role-played dimension"
    );

    let phantoms = sample(&arb_phantom_case(), 256);
    let p_city = phantoms.iter().filter(|c| c.groups_by_role_played).count();
    assert!(
        p_city >= 8,
        "only {p_city}/256 phantom cases grouped by the role-played dimension"
    );
}

/// Both join DIRECTIONS must be exercised. A generator that only ever picked
/// the base grain would never emit the downward LEFT JOIN whose phantom row the
/// EXP-21/25/26 family is about; one that only picked the child grain would
/// never emit a metric aggregating over the base with parents joined above it.
#[test]
fn generator_reaches_both_join_directions() {
    let cases = sample(&arb_algebra_case(), 256);
    let child = cases.iter().filter(|c| c.grain == Grain::Child).count();
    let base = cases.len() - child;
    assert!(
        child >= 64,
        "only {child}/256 algebra cases used the child grain"
    );
    assert!(
        base >= 64,
        "only {base}/256 algebra cases used the base grain"
    );

    let rollups = sample(&arb_rollup_case(), 256);
    let r_child = rollups.iter().filter(|c| c.grain == Grain::Child).count();
    let r_base = rollups.len() - r_child;
    assert!(
        r_child >= 64 && r_base >= 64,
        "roll-up grains are lopsided: child={r_child} base={r_base}"
    );

    // The child dimension — grouping BELOW the base — must also be reached.
    let lqty = cases.iter().filter(|c| c.dims.contains(&D_LQTY)).count();
    assert!(
        lqty >= 32,
        "only {lqty}/256 algebra cases grouped by the child-table dimension"
    );
}

/// The model really does declare two relationships from one source to one
/// target — the structural precondition the guard above assumes. A refactor
/// that collapsed them would leave every count above intact and the property
/// meaningless.
#[test]
fn the_definition_declares_a_role_playing_pair() {
    let names = sample(&arb_names(), 1).pop().unwrap();
    let def = build_def(&names, None);
    let to_ap: Vec<&Join> = def.joins.iter().filter(|j| j.table == "ap").collect();
    assert_eq!(to_ap.len(), 2, "expected two relationships targeting `ap`");
    assert_eq!(
        to_ap[0].from_alias, to_ap[1].from_alias,
        "role-playing requires both edges to leave the SAME source table"
    );
    assert_ne!(
        to_ap[0].name, to_ap[1].name,
        "the two role edges must be distinguishable by name"
    );
    assert_ne!(
        to_ap[0].fk_columns, to_ap[1].fk_columns,
        "the two role edges must use different foreign keys"
    );
}

/// PBT-9's open half: derived-over-derived. A chain that only ever bottomed out
/// in base metrics would make the recursion in `Chain::recompute` dead code.
#[test]
fn generator_produces_derived_over_derived_chains() {
    let cases = sample(&arb_algebra_case(), 256);
    let deep = cases
        .iter()
        .filter(|c| c.chain.depth(c.chain.target) >= 2)
        .count();
    let deepest = cases
        .iter()
        .map(|c| c.chain.depth(c.chain.target))
        .max()
        .unwrap_or(0);
    assert!(
        deep >= 32,
        "only {deep}/256 algebra cases queried a derived metric of depth >= 2"
    );
    assert!(
        deepest >= 3,
        "the deepest generated chain was {deepest}; depth 3 must occur"
    );

    let mut ops = [0usize; 4];
    for c in &cases {
        for spec in &c.chain.specs {
            ops[Op::ALL.iter().position(|o| *o == spec.op).unwrap()] += 1;
        }
    }
    for (i, n) in ops.iter().enumerate() {
        assert!(*n >= 8, "operator {:?} appeared only {n} times", Op::ALL[i]);
    }

    let multi_leaf = cases
        .iter()
        .filter(|c| c.chain.leaves(c.chain.target).len() >= 2)
        .count();
    assert!(
        multi_leaf >= 64,
        "only {multi_leaf}/256 chains had two or more distinct component metrics; \
         a single-leaf chain cannot detect a mis-combined pair"
    );
}

/// PBT-12: hostile identifiers must actually reach the numeric path — every
/// style, on every kind of name, and in the *reference* positions (a derived
/// metric's expression, a `where_clause`) as well as declaration positions.
#[test]
fn generator_produces_hostile_identifiers() {
    let cases = sample(&arb_algebra_case(), 256);
    let mut seen = [0usize; Style::ALL.len()];
    for c in &cases {
        for s in &c.names.styles {
            seen[Style::ALL.iter().position(|x| x == s).unwrap()] += 1;
        }
    }
    for (i, n) in seen.iter().enumerate() {
        assert!(
            *n >= 32,
            "identifier style {:?} appeared only {n} times",
            Style::ALL[i]
        );
    }

    let quoted = cases.iter().filter(|c| c.names.any_quoted()).count();
    assert!(
        quoted >= 200,
        "only {quoted}/256 cases carried any quoted identifier"
    );

    // A quoted metric name inside a DERIVED metric's expression — the reference
    // position the scanner has to get right.
    let quoted_ref = cases
        .iter()
        .filter(|c| {
            let expr = c.chain.expr(c.chain.target, &c.names);
            expr.contains('"')
        })
        .count();
    assert!(
        quoted_ref >= 64,
        "only {quoted_ref}/256 derived-metric expressions referenced a quoted metric name"
    );

    // A quoted member name inside a generated `where_clause`.
    let quoted_pred = cases
        .iter()
        .filter(|c| {
            c.pred
                .as_ref()
                .is_some_and(|p| p.to_member_sql(&c.names).contains('"'))
        })
        .count();
    assert!(
        quoted_pred >= 32,
        "only {quoted_pred}/256 predicates named a quoted member"
    );

    // A quoted PHYSICAL table name, which the expansion must quote itself.
    let quoted_table = cases
        .iter()
        .filter(|c| c.names.tables.iter().any(|t| t.contains('"')))
        .count();
    assert!(
        quoted_table >= 128,
        "only {quoted_table}/256 cases used a quoted physical table name"
    );
}

/// PBT-13: `where_clause` is never pinned at `None` here, and both sides of the
/// join and both member kinds are reached.
#[test]
fn generator_varies_the_where_clause() {
    let cases = sample(&arb_algebra_case(), 256);
    let with = cases.iter().filter(|c| c.pred.is_some()).count();
    let without = cases.len() - with;
    assert!(
        with >= 48,
        "only {with}/256 algebra cases carried a predicate"
    );
    assert!(
        without >= 16,
        "only {without}/256 cases omitted the predicate"
    );
    let child = cases
        .iter()
        .filter_map(|c| c.pred.as_ref())
        .filter(|p| p.touches_child())
        .count();
    assert!(
        child >= 16,
        "only {child} predicates named a child-side member"
    );
    let filters = cases
        .iter()
        .filter_map(|c| c.pred.as_ref())
        .filter(|p| p.references_filter())
        .count();
    assert!(
        filters >= 16,
        "only {filters} predicates named a filter member"
    );
}

/// The data must contain the shapes the properties are about: childless parents
/// (property (d)'s subject), unparented children, and enough rows that the
/// aggregates are not all NULL.
#[test]
fn generator_produces_the_data_shapes_under_test() {
    let cases = sample(&arb_algebra_case(), 256);
    let childless = cases
        .iter()
        .filter(|c| c.data.childless_parents() > 0)
        .count();
    let unparented = cases
        .iter()
        .filter(|c| c.data.unparented_children() > 0)
        .count();
    let both = cases
        .iter()
        .filter(|c| c.data.childless_parents() > 0 && c.data.unparented_children() > 0)
        .count();
    assert!(
        childless >= 64,
        "only {childless}/256 cases had a childless parent"
    );
    assert!(
        unparented >= 32,
        "only {unparented}/256 cases had a child row with a NULL or dangling key"
    );
    assert!(both >= 16, "only {both}/256 cases had both shapes at once");

    let phantoms = sample(&arb_phantom_case(), 256);
    let facts = phantoms.iter().filter(|c| c.facts_request).count();
    assert!(
        facts >= 32,
        "only {facts}/256 phantom cases issued a row-level FACTS request"
    );
}

/// The properties must actually see NUMBERS. A generator that produced only
/// empty groupings, or only NULL aggregates, would satisfy every comparison
/// above vacuously — `IS NOT DISTINCT FROM` makes NULL == NULL agreement.
#[test]
fn the_properties_compare_non_null_numbers() {
    let cases = sample(&arb_algebra_case(), 128);
    let mut with_rows = 0usize;
    let mut with_values = 0usize;
    for case in &cases {
        let def = build_def(&case.names, Some(&case.chain));
        let name = case.names.derived[case.chain.target].clone();
        let Ok(sql) = expand(
            "mm",
            &def,
            &request(
                &case.names,
                &case.dims,
                &[name.clone()],
                &[],
                case.pred.as_ref(),
            ),
        ) else {
            continue;
        };
        let conn = make_db(&case.names, &case.data, "");
        let probe = format!("SELECT count(*), count({}) FROM ({sql}) q", out_col(&name));
        let (rows, vals): (i64, i64) = conn
            .query_row(&probe, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or_else(|e| panic!("probe failed: {e}\nSQL:\n{probe}"));
        if rows > 0 {
            with_rows += 1;
        }
        if vals > 0 {
            with_values += 1;
        }
    }
    assert!(
        with_rows >= 80,
        "only {with_rows}/128 algebra cases returned any row at all"
    );
    assert!(
        with_values >= 24,
        "only {with_values}/128 algebra cases produced a NON-NULL derived value; \
         the comparison would be agreeing about NULLs"
    );
}

// ---------------------------------------------------------------------------
// Findings — properties that state the correct invariant and fail today
// ---------------------------------------------------------------------------

fn fixed_names() -> Names {
    let styles = vec![
        Style::Bare;
        TABLE_STEMS.len()
            + DIM_STEMS.len()
            + BASE_METRICS.len()
            + MAX_DERIVED
            + FACT_STEMS.len()
            + REL_STEMS.len()
    ];
    Names {
        tables: TABLE_STEMS.iter().map(ToString::to_string).collect(),
        dims: DIM_STEMS.iter().map(ToString::to_string).collect(),
        mets: BASE_METRICS.iter().map(|b| b.stem.to_string()).collect(),
        derived: (0..MAX_DERIVED).map(|i| format!("dm{i}")).collect(),
        facts: FACT_STEMS.iter().map(ToString::to_string).collect(),
        rels: REL_STEMS.iter().map(ToString::to_string).collect(),
        styles,
    }
}

/// **MM-1** — a metric that reaches a FACT on the role-played table emits SQL
/// that does not bind.
///
/// `dep_alt = SUM(alt)` where `alt` is a fact whose expression is `ap.alt`, with
/// `USING (r_dep)`. Fact inlining splices the fact's RAW expression (`ap.alt`)
/// into the metric, but the role-playing rewrite has already renamed the table
/// to its scoped alias, so the join is emitted as `"ap" AS "ap__r_dep"` while
/// the aggregate still says `ap.alt`:
///
/// ```text
/// SELECT ap__r_dep.city AS "city", SUM((ap.alt)) AS "dep_alt"
/// FROM "o" AS "o" LEFT JOIN "ap" AS "ap__r_dep" ON …
///   -> Binder Error: Referenced table "ap" not found! Candidate tables: "ap__r_dep"
/// ```
///
/// Loud, not silent — but it makes an expressible definition unqueryable. The
/// sibling that is *not* broken is a DIMENSION on the role-played table, whose
/// expression IS rewritten; only the inlined fact text is missed.
#[test]
#[ignore = "MM-1: fact inlining does not apply the role-playing scoped-alias rewrite"]
fn mm1_role_playing_metric_over_a_fact_on_the_role_played_table() {
    let names = fixed_names();
    let mut def = build_def(&names, None);
    def.facts.push(Fact {
        name: "alt".to_string(),
        expr: "ap.alt".to_string(),
        source_table: Some("ap".to_string()),
        access: AccessModifier::Public,
        ..Default::default()
    });
    def.metrics.push(Metric {
        name: "dep_alt".to_string(),
        expr: "SUM(alt)".to_string(),
        source_table: Some("o".to_string()),
        using_relationships: vec!["r_dep".to_string()],
        ..Default::default()
    });
    let req = request(&names, &[D_CITY], &["dep_alt".to_string()], &[], None);
    let sql = expand("mm", &def, &req).expect("the query must expand");
    let data = Data {
        o: vec![(Some(1), Some(10), Some(1), Some(2), Some(1))],
        li: vec![(Some(0), Some(5), Some(1))],
    };
    let conn = make_db(&names, &data, "");
    conn.prepare(&sql)
        .unwrap_or_else(|e| panic!("emitted SQL must bind, got: {e}\nSQL:\n{sql}"));
}

/// **MM-2** — a QUOTED relationship name on a role-played edge emits SQL that
/// does not parse.
///
/// The scoped alias is built by concatenating the raw stored strings
/// (`ap` + `__` + `"de p"`). The join clause quotes the whole result
/// (`"ap__""de p"""`), but the member expression's rewritten qualifier is
/// emitted unquoted (`ap__"de p".city`), so the two disagree and DuckDB reports
/// a syntax error at the stray quote. `tests/common/mod.rs` generates
/// relationship names from `arb_stored_ident()` (RT-5) — but only for the
/// render/parse round-trip, never on a path that emits a scoped alias.
#[test]
#[ignore = "MM-2: a quoted relationship name produces an unparseable scoped join alias"]
fn mm2_role_playing_with_a_quoted_relationship_name() {
    let mut names = fixed_names();
    names.rels[1] = "\"de p\"".to_string();
    let def = build_def(&names, None);
    // `dep_rate` is the metric that names relationship 1 via USING.
    let dep_rate = names.mets[11].clone();
    let req = request(&names, &[D_CITY], &[dep_rate], &[], None);
    let sql = expand("mm", &def, &req).expect("the query must expand");
    let data = Data {
        o: vec![(Some(1), Some(10), Some(1), Some(2), Some(1))],
        li: vec![(Some(0), Some(5), Some(1))],
    };
    let conn = make_db(&names, &data, "");
    conn.prepare(&sql)
        .unwrap_or_else(|e| panic!("emitted SQL must parse, got: {e}\nSQL:\n{sql}"));
}

/// **MM-3** — a metric's VALUE changes when it is co-queried with a metric of
/// another grain. Silent wrong number.
///
/// A single-grain request is base-anchored (`FROM o LEFT JOIN li`); a
/// multi-grain request routes through `per_grain`, which anchors each grain's
/// CTE at its OWN table (`FROM li`, no join). The two disagree in two ways, and
/// this test pins both:
///
/// - a child row whose foreign key is NULL (legal data, not a broken FK) is
///   dropped by the base-anchored query and counted by the child-anchored CTE:
///   `s_amt` is 12 alone and 112 alongside `s_rate`;
/// - a base row with no children contributes `COUNT(*) = 0` to the
///   base-anchored query and NO ROW to the child-anchored CTE, so the FULL OUTER
///   JOIN reports NULL: `n_li` is 0 alone and NULL alongside `s_rate`.
///
/// Per-grain anchoring is deliberate (TECH-DEBT #35/#36) but the resulting
/// value-instability of an individual metric is recorded nowhere.
#[test]
#[ignore = "MM-3: per-grain re-anchoring changes a component metric's own value"]
fn mm3_metric_value_changes_when_co_queried_across_grains() {
    let names = fixed_names();
    let def = build_def(&names, None);
    let n_li = names.mets[0].clone();
    let s_amt = names.mets[1].clone();
    let s_rate = names.mets[9].clone();

    // Row 1 of `o` has a rate and no children; child row 1 has a NULL key.
    let data = Data {
        o: vec![
            (Some(1), Some(10), Some(1), Some(2), Some(1)),
            (Some(2), Some(7), Some(2), Some(2), Some(1)),
        ],
        li: vec![(Some(0), Some(12), Some(1)), (None, Some(100), Some(1))],
    };
    let conn = make_db(&names, &data, "");
    let value = |mets: &[String], want: &str| -> String {
        let sql = expand("mm", &def, &request(&names, &[], mets, &[], None)).expect("expand");
        conn.query_row(
            &format!("SELECT CAST({} AS VARCHAR) FROM ({sql}) q", out_col(want)),
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .expect("query")
        .unwrap_or_else(|| "NULL".to_string())
    };

    assert_eq!(
        value(&[s_amt.clone()], &s_amt),
        value(&[s_amt.clone(), s_rate.clone()], &s_amt),
        "SUM(li.amount) must not change when a base-grain metric joins the request"
    );
    assert_eq!(
        value(&[n_li.clone()], &n_li),
        value(&[n_li.clone(), s_rate.clone()], &n_li),
        "COUNT over the child must not change when a base-grain metric joins the request"
    );
}

/// **MM-4** — the aggregate path still manufactures a NULL group for a
/// childless parent when the query groups by a CHILD dimension.
///
/// EXP-29 fenced the row-level shapes: a dimensions-only `SELECT DISTINCT` and a
/// FACTS request over one below-base table both filter the phantom with
/// `WHERE <pk> IS NOT NULL`. The AGGREGATE path with the same member set does
/// not: it guards each aggregate's ARGUMENT (`CASE WHEN <pk> IS NOT NULL …`),
/// which fixes the value but not the group key, so a childless parent still
/// contributes a `(NULL, 0)` / `(NULL, NULL)` row describing nothing.
///
/// Not listed among TECH-DEBT #67's three recorded residuals.
#[test]
#[ignore = "MM-4: aggregate + child dimension still emits the join-manufactured NULL group"]
fn mm4_childless_parent_manufactures_a_null_group_under_a_child_dimension() {
    let names = fixed_names();
    let def = build_def(&names, None);
    let data = Data {
        o: vec![(Some(1), Some(10), Some(1), Some(2), Some(1))],
        li: vec![(Some(0), Some(5), Some(1))],
    };
    let req = request(&names, &[D_LQTY], &[names.mets[0].clone()], &[], None);
    let sql = expand("mm", &def, &req).expect("expand");
    let before = make_db(&names, &data, "");
    let after = make_db(&names, &data, &childless_parent_row(&data));
    let (b, a) = render_both(&sql, &before, &after).expect("render");
    assert_eq!(
        b, a,
        "inserting a childless parent added a group under a child dimension\nSQL:\n{sql}"
    );
}
