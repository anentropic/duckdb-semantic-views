//! Per-grain ("own-grain") metric aggregation — the Snowflake-parity path for
//! multi-grain queries (TECH-DEBT #35).
//!
//! # Why
//!
//! Every other aggregation strategy in this crate anchors the generated query
//! `FROM <base table>` and `LEFT JOIN`s outward, so every metric is computed
//! over one joined row set. A metric whose grain is *not* the base grain is
//! therefore aggregated over the multiplied join: a metric on a parent of the
//! base table is counted once per base row, and two metrics either side of a
//! fan-out edge inflate each other. v0.11.0 made those shapes **error**
//! (`RootGrainFanTrap` / `MetricFanTrap`) rather than silently inflate.
//!
//! Snowflake answers them instead, by computing each metric at its own grain.
//! This module does the same: each grain gets a CTE anchored at *its* table,
//! aggregating only that table's rows, and the pre-aggregated results are joined
//! on the queried dimensions.
//!
//! ```sql
//! WITH __sv_grain_0 AS (          -- the base-grain metrics
//!     SELECT o.status AS "__sv_d0", SUM(o.amount) AS "__sv_m0"
//!     FROM "orders" AS "o" GROUP BY 1
//! ), __sv_grain_1 AS (            -- the line-item-grain metrics
//!     SELECT o.status AS "__sv_d0", COUNT(*) AS "__sv_m0"
//!     FROM "line_items" AS "li"
//!     LEFT JOIN "orders" AS "o" ON "li"."order_id" = "o"."id" GROUP BY 1
//! )
//! SELECT COALESCE("__sv_grain_0"."__sv_d0", "__sv_grain_1"."__sv_d0") AS "status",
//!        "__sv_grain_0"."__sv_m0" AS "order_total",
//!        "__sv_grain_1"."__sv_m0" AS "item_count"
//! FROM __sv_grain_0
//! FULL OUTER JOIN __sv_grain_1
//!   ON "__sv_grain_0"."__sv_d0" IS NOT DISTINCT FROM "__sv_grain_1"."__sv_d0"
//! ```
//!
//! # Scope
//!
//! [`plan`] returns `None` — leaving the base-anchored path and its fan-trap
//! fence exactly as they were — unless the query BOTH needs per-grain treatment
//! (a base-anchored `FROM` would inflate it) AND is eligible for it. So
//! single-grain SQL is byte-identical to v0.11.0, and the shapes per-grain
//! cannot define keep their v0.11.0 error:
//!
//! - a dimension **below** a metric's grain (the metric's rows genuinely fan
//!   across the dimension's values) — `FanTrap`, raised by the fence;
//! - *active* semi-additive metrics **spanning more than one grain**. A single
//!   grain is now answered: [`snapshot_cte_anchor`] re-points `__sv_snapshot` at
//!   the metric's own table, the same move [`window_cte_anchor`] makes for
//!   `__sv_agg`. Two grains give that single CTE no one anchor, so they keep the
//!   error until the `RANK` shape can be emitted as one group's CTE inside a
//!   multi-grain plan (TECH-DEBT #36);
//! - a query that reaches a **role-played** table without saying which role it
//!   means. Which of the several relationship instances a grain CTE should join
//!   is what `USING` answers, and a co-queried metric's `USING` is now honoured:
//!   the grain CTEs join the NAMED edge under its scoped alias (`a__dep`) and
//!   emit the dimension against it, matching the base-anchored path.
//!
//!   Without that context the query is still declined, and the rescue is
//!   deliberately narrow — it covers a queried DIMENSION's own table only. A
//!   `where_clause` member on a role-played table, a metric's own grain table, or
//!   a table reachable only *through* a role-played one are all still declined:
//!   only a dimension's expression is rewritten to the scoped alias, so nothing
//!   else can say which role it means without guessing.
//!
//!   Note the scope: this is asked of the QUERY, not the definition, so a
//!   definition that declares role-playing somewhere does not lose per-grain
//!   emission for queries that never reach it.
//!
//! Window metrics are no longer in that list. [`window_cte_anchor`] picks the
//! grain for an all-window query and `expand_window_metrics` anchors its
//! `__sv_agg` CTE there — the window function is not grain-sensitive, so moving
//! the CTE is the whole fix. That path does not build [`Plan`] groups; it reuses
//! only [`anchor_joins`] and the fence's per-grain mode.

use std::collections::{HashMap, HashSet};

use crate::model::{Dimension, Join, Metric, SemanticViewDefinition};

use super::fan_trap::{metric_grain_tables, GrainGraph};
use super::join_resolver::{push_join_clauses, ResolvedJoin};
use super::resolution::{quote_ident, quote_stored_ident};
use super::select_spec::{
    push_from_anchor, push_group_by_ordinals, FromSource, GroupBy, SelectItem, SelectSpec,
};
use super::where_clause::ResolvedWhere;

/// The CTE name for grain group `i`. Bare (unquoted) by construction — the
/// index is the only variable part — matching `__sv_agg` / `__sv_snapshot`.
fn grain_cte(index: usize) -> String {
    format!("__sv_grain_{index}")
}

/// Reference to grain group `group`'s column `column`, qualified by its CTE.
fn column_ref(group: usize, column: &str) -> String {
    format!("{}.{}", quote_ident(&grain_cte(group)), quote_ident(column))
}

/// The internal alias of the `i`th queried dimension inside every grain CTE.
/// Positional rather than the dimension's own name so a declared name can never
/// collide with the join keys (or with a metric column) inside the CTE.
fn dim_column(index: usize) -> String {
    format!("__sv_d{index}")
}

/// The internal alias of the `i`th aggregate column within one grain CTE.
fn metric_column(index: usize) -> String {
    format!("__sv_m{index}")
}

/// One grain: the table its CTE is anchored at, and the aggregate columns
/// computed there.
struct Group {
    /// Lowercased table alias the CTE is anchored `FROM`.
    anchor: String,
    /// Aggregate expressions, in emission order. The `i`th is aliased
    /// [`metric_column(i)`](metric_column).
    exprs: Vec<String>,
}

/// How one requested metric's output column is produced from the grain CTEs.
enum Output {
    /// The whole metric is one grain column.
    Direct { group: usize, column: usize },
    /// A derived metric assembled from component columns spanning grains: its
    /// expression with every base-metric reference already replaced by the
    /// component's qualified CTE column reference.
    Derived { expr: String },
}

/// A per-grain execution plan: the grain groups to emit as CTEs, and how each
/// requested metric's output column is built from them.
pub(super) struct Plan {
    groups: Vec<Group>,
    /// One entry per requested metric, in request order.
    outputs: Vec<Output>,
}

impl Plan {
    /// Whether the plan is a single grain with no cross-grain assembly, in which
    /// case the query is emitted flat (anchored at that grain) instead of as a
    /// one-CTE wrapper.
    fn is_single_group(&self) -> bool {
        self.groups.len() == 1
    }
}

/// Working state while partitioning a query's metrics into grain groups.
struct Partition {
    groups: Vec<Group>,
    /// Lowercased anchor alias -> index into `groups`.
    index: HashMap<String, usize>,
}

impl Partition {
    fn new() -> Self {
        Self {
            groups: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Add `expr` to `anchor`'s group (creating it on first use) and return the
    /// (group, column) coordinates of the added column.
    fn push(&mut self, anchor: &str, expr: String) -> (usize, usize) {
        let group = *self.index.entry(anchor.to_string()).or_insert_with(|| {
            self.groups.push(Group {
                anchor: anchor.to_string(),
                exprs: Vec::new(),
            });
            self.groups.len() - 1
        });
        self.groups[group].exprs.push(expr);
        (group, self.groups[group].exprs.len() - 1)
    }
}

/// Decide whether this query must be computed per-grain, and how.
///
/// Returns `None` — meaning "leave the query on the base-anchored path, with the
/// full fan-trap fence" — when the query does not need per-grain treatment, or
/// when it needs it but is not eligible (see the module docs). A `Some` plan is
/// a commitment: the caller emits it via [`expand_per_grain`] and runs the fence
/// in its per-grain mode.
pub(super) fn plan(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    resolved_exprs: &HashMap<String, String>,
    where_tables: &[String],
) -> Option<Plan> {
    if def.joins.is_empty() || resolved_mets.is_empty() {
        return None; // Single-table view, or a dimensions-only query.
    }
    if !is_eligible(def, resolved_dims, resolved_mets, where_tables) {
        return None;
    }
    let graph = GrainGraph::build(def)?;
    let root = graph.root().to_string();

    // Every queried dimension's table, in request order. Eligibility already
    // established that each carries a source table.
    let dim_tables: Vec<String> = resolved_dims
        .iter()
        .filter_map(|d| d.source_table.as_ref().map(|t| t.to_ascii_lowercase()))
        .collect();

    // Partition the metrics into grains, decomposing a metric whose own grain
    // spans several tables into one component per referenced base metric.
    let mut partition = Partition::new();
    let mut outputs: Vec<Output> = Vec::with_capacity(resolved_mets.len());
    for met in resolved_mets {
        let mut grains = metric_grain_tables(met, def);
        if grains.is_empty() {
            grains.push(root.clone()); // A metric with no source table sits at the root grain.
        }
        if grains.len() == 1 {
            let expr = resolved_exprs
                .get(&crate::ident::normalize_ident_part(&met.name))
                .cloned()
                .unwrap_or_else(|| met.expr.clone());
            let (group, column) = partition.push(&grains[0], expr);
            outputs.push(Output::Direct { group, column });
        } else {
            outputs.push(decompose(def, met, resolved_exprs, &mut partition)?);
        }
    }

    // Does a base-anchored FROM actually get this wrong? Only then is rewriting
    // it justified: an anchor that fans relative to the root (the metric is
    // duplicated once per root row), or a pair of anchors either side of a
    // fan-out edge (they inflate each other over the shared join).
    let anchors: Vec<&String> = partition.groups.iter().map(|g| &g.anchor).collect();
    let needs_per_grain = anchors
        .iter()
        .any(|a| graph.fanning_relationship(a, &root).is_some())
        || anchors.iter().enumerate().any(|(i, a)| {
            anchors
                .iter()
                .enumerate()
                .any(|(j, b)| i != j && graph.fanning_relationship(a, b).is_some())
        });
    if !needs_per_grain {
        return None;
    }

    // Per-grain only helps when every queried dimension is reachable from every
    // anchor without fanning it. Where it is not, the fence's metric ×
    // dimension error is the right answer — decline and let it speak.
    for anchor in &anchors {
        for dim_table in &dim_tables {
            if graph.path(anchor, dim_table).is_none()
                || graph.fanning_relationship(anchor, dim_table).is_some()
            {
                return None;
            }
        }
    }

    Some(Plan {
        groups: partition.groups,
        outputs,
    })
}

/// Split a metric whose grain spans several tables into one component per
/// referenced base metric, each computed at its own grain, and rebuild the
/// metric's expression over those component columns.
///
/// `None` when the metric cannot be decomposed — a component that is itself
/// multi-grain, or a reference that resolves to no declared metric — in which
/// case the query stays on the base-anchored path and keeps its fence error.
fn decompose(
    def: &SemanticViewDefinition,
    met: &Metric,
    resolved_exprs: &HashMap<String, String>,
    partition: &mut Partition,
) -> Option<Output> {
    // The base metrics this one transitively depends on: each is an aggregate
    // that must be computed at its own grain before the outer expression can
    // combine them.
    //
    // Walked in DECLARATION order, filtered by the dependency set, rather than
    // iterating the set itself: the set is a `HashSet`, and its order decides
    // which grain becomes `__sv_grain_0`. Iterating it directly made the
    // generated SQL vary between runs of the same query.
    let dependencies = super::facts::collect_transitive_metric_names(met, &def.metrics);
    let mut replacements: HashMap<String, String> = HashMap::new();
    for base in &def.metrics {
        let name = crate::ident::normalize_ident_part(&base.name);
        if !dependencies.contains(&name) {
            continue;
        }
        let Some(ref source) = base.source_table else {
            continue; // Derived: inlined by the reference walk below, not a component.
        };
        if base.window_spec.is_some() || !base.non_additive_by.is_empty() {
            return None; // Component strategies that are base-anchored — not eligible.
        }
        let expr = resolved_exprs.get(&name).cloned()?;
        let (group, column) = partition.push(&source.to_ascii_lowercase(), expr);
        let reference = column_ref(group, &metric_column(column));
        // A base metric may be referenced bare (`revenue`) or qualified by its
        // own source table (`o.revenue`) — the same two spellings
        // `collect_transitive_metric_names` matches. Both keys map to the
        // component column.
        replacements.insert(
            format!("{}.{}", source.to_ascii_lowercase(), name),
            reference.clone(),
        );
        replacements.insert(name, reference);
    }
    if replacements.is_empty() {
        return None;
    }
    Some(Output::Derived {
        expr: rebuild_expr(def, met, &replacements)?,
    })
}

/// How deep a chain of derived metrics [`rebuild_expr`] will follow before
/// giving up. Cycles are already rejected by `inline_derived_metrics`, which
/// runs first — this only keeps the recursion total if a malformed definition
/// ever reaches here, and mirrors the derivation-depth cap that path enforces.
const MAX_REBUILD_DEPTH: usize = 64;

/// Rebuild `met`'s expression with each base-metric reference replaced by its
/// per-grain column reference, inlining any intermediate *derived* metric it
/// references on the way (they carry no grain of their own, so there is no
/// column to read them from).
///
/// Resolution is on demand, per reference: every occurrence of an intermediate
/// metric is expanded wherever it appears, on each branch. Inlined text is
/// never rescanned — [`crate::expr_tokens::inline_references`] splices by byte
/// span — so an intermediate must arrive already fully resolved. Remembering
/// "this one was inlined earlier" across sibling branches is exactly what must
/// not happen: the second branch would then splice the metric's *raw* text,
/// leaking base-metric names into the SQL as non-existent columns.
///
/// `None` when the chain exceeds [`MAX_REBUILD_DEPTH`], leaving the query on
/// the base-anchored path with its fence error.
fn rebuild_expr(
    def: &SemanticViewDefinition,
    met: &Metric,
    replacements: &HashMap<String, String>,
) -> Option<String> {
    fn resolve(
        def: &SemanticViewDefinition,
        met: &Metric,
        replacements: &HashMap<String, String>,
        depth: usize,
    ) -> Option<String> {
        if depth > MAX_REBUILD_DEPTH {
            return None;
        }
        let mut owned: HashMap<String, String> = HashMap::new();
        for key in crate::expr_tokens::reference_keys(&met.expr) {
            if let Some(column) = replacements.get(&key) {
                owned.insert(key, column.clone());
            } else if let Some(inner) = def.metrics.iter().find(|m| {
                m.source_table.is_none() && crate::ident::normalize_ident_part(&m.name) == key
            }) {
                let inlined = resolve(def, inner, replacements, depth + 1)?;
                owned.insert(key, format!("({inlined})"));
            }
        }
        let borrowed: HashMap<String, &str> =
            owned.iter().map(|(k, v)| (k.clone(), v.as_str())).collect();
        Some(crate::expr_tokens::inline_references(&met.expr, &borrowed))
    }

    resolve(def, met, replacements, 0)
}

/// Whether role-playing is relevant to **this query** — not merely present
/// somewhere in the definition.
///
/// Which role an anchored CTE should join is exactly the question `USING`
/// answers on the base-anchored path, and neither the per-grain grain CTEs nor
/// the anchored window CTE carries that context, so both still decline a query
/// that would have to answer it.
///
/// The question is asked per query rather than per definition. A definition-wide
/// test costs *every* query against a definition that declares role-playing
/// anywhere — including queries that never reach the role-played table, whose
/// grain CTEs join nothing ambiguous and need no role context at all. Two
/// unrelated grains lost per-grain emission because some third table happened to
/// be reachable by two relationships.
///
/// A table is ambiguous here if it *is* a role-playing target or is reachable
/// only *through* one, which is what [`role_playing_on_path`] walks. Checking
/// the tables the query touches also covers the joins between them: the
/// relationship graph is a tree apart from the sanctioned role-playing
/// multi-edge, so any node on the path between two touched tables is an ancestor
/// of one of them, and the walk from that endpoint to the root passes through it.
///
/// [`role_playing_on_path`]: super::role_playing::role_playing_on_path
/// The role each role-played table plays in **this** query: lowercased table
/// alias -> the lowercased relationship name a co-queried metric's `USING`
/// named for it.
///
/// [`find_using_context`] is the base-anchored path's answer to the same
/// question, and reusing it is what keeps the two paths agreeing on which role a
/// dimension means — it returns the scoped alias (`a__dep`), from which the
/// relationship name is the suffix after `__`. Rather than re-split that string,
/// this asks the definition directly: the relationship whose target is the
/// dimension's table and whose scoped alias matches.
///
/// Only a queried DIMENSION's table is resolved. A role-played table reached
/// only as a metric's grain, or named only by a `where_clause` member, has no
/// dimension for `USING` to scope and stays ineligible — see
/// [`role_playing_affects_query`].
fn scoped_roles(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> HashMap<String, String> {
    let mut roles: HashMap<String, String> = HashMap::new();
    for dim in resolved_dims {
        let Some(table) = dim.source_table.as_ref().map(|t| t.to_ascii_lowercase()) else {
            continue;
        };
        // The view name only decorates an error discarded here: an ambiguous
        // dimension yields no role, so the query stays ineligible and the
        // base-anchored path reports the real diagnostic.
        if let Ok(Some(scoped)) =
            super::role_playing::find_using_context("", def, dim, resolved_mets)
        {
            if let Some(rel) = def.joins.iter().find_map(|j| {
                let name = j.name.as_ref()?.to_ascii_lowercase();
                (j.table.to_ascii_lowercase() == table
                    && super::join_resolver::scoped_join_alias(&table, &name) == scoped)
                    .then_some(name)
            }) {
                roles.insert(table, rel);
            }
        }
    }
    roles
}

fn role_playing_affects_query(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    where_tables: &[String],
    allow_using_scoped: bool,
) -> bool {
    // The view name passed to `role_playing_on_path` below only decorates an
    // error discarded here: a definition too malformed to walk declines
    // per-grain, and the base-anchored path it falls back to reports the real
    // diagnostic.
    let ambiguity = |table: &String| match super::role_playing::role_playing_on_path("", def, table)
    {
        Ok(v) => v,
        Err(_) => Some(table.clone()), // Unwalkable: treat as ambiguous, decline.
    };

    // Tables that CANNOT be rescued by `USING`, so any role-playing on their
    // path declines outright. Deduplicated: several metrics commonly share a
    // grain, and each walk can rebuild the relationship graph.
    //
    // A `where_clause` member's table is joined into every grain CTE exactly
    // like a dimension's — `anchor_joins` chains both into the sources it walks
    // — but only a DIMENSION's expression is rewritten to a scoped alias, so a
    // predicate naming a role-played member has no way to say which role it
    // means and stays ineligible. A metric's grain table is likewise strict:
    // `USING` scopes the dimension a metric is grouped BY, not the table a
    // metric is aggregated AT.
    let mut strict: HashSet<String> = where_tables
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    for met in resolved_mets {
        strict.extend(metric_grain_tables(met, def));
    }
    if strict.iter().any(|t| ambiguity(t).is_some()) {
        return true;
    }

    // A queried dimension's table may be rescued, but only when the ambiguous
    // table is the dimension's OWN table and a co-queried metric's `USING`
    // named its role. A table reached only THROUGH a role-played ancestor is
    // never rescued: `find_using_context` rejects that case outright, because a
    // descendant cannot be scoped by a co-queried metric's `USING`, so it
    // yields no role here either.
    let roles = if allow_using_scoped {
        scoped_roles(def, resolved_dims, resolved_mets)
    } else {
        // The window path emits its own `__sv_agg` SELECT list and does not
        // rewrite dimension expressions to scoped aliases, so it cannot honour a
        // role even when `USING` names one. It stays strict.
        HashMap::new()
    };
    let dim_tables: HashSet<String> = resolved_dims
        .iter()
        .filter_map(|d| d.source_table.as_ref().map(|s| s.to_ascii_lowercase()))
        .collect();
    dim_tables.iter().any(|table| match ambiguity(table) {
        None => false,
        Some(ambiguous) => !(ambiguous == *table && roles.contains_key(table)),
    })
}

/// The declared table a window query's `__sv_agg` CTE should anchor at, or
/// `None` to leave it anchored at the base table.
///
/// A window metric's inner aggregate is the grain-sensitive part: the window
/// function itself runs over the already-grouped CTE, so anchoring the CTE at
/// the inner metric's own table is the whole fix (TECH-DEBT #36). With
/// `total_balance = SUM(c.balance)` on the parent `customers`, a base-anchored
/// CTE joins `orders` and sums each customer's balance once per order — the
/// inflation the v0.11.0 fence turned into `RootGrainFanTrap`. Anchored at `c`
/// the inner aggregate sees one row per customer.
///
/// Returns `None` — leaving the query base-anchored with the full fence — when:
/// - the anchor would be the root anyway (base-anchoring is already correct);
/// - the window metrics' inner aggregates span **several** grains, which would
///   need those grains joined before the window runs. That is the next
///   increment, not this one;
/// - a queried dimension is unqualified (its binding would move with the
///   anchor), or the view role-plays, matching [`is_eligible`]'s conservatism;
/// - a queried dimension is unreachable from the anchor.
///
/// A dimension *below* the anchor's grain is deliberately NOT screened here: it
/// has no single value per group in either engine, and the fan-trap fence's
/// metric × dimension check — which per-grain mode keeps — is what reports it.
/// The table an active semi-additive metric's `__sv_snapshot` CTE should anchor
/// at, or `None` to leave it base-anchored.
///
/// The sibling of [`window_cte_anchor`], and deliberately the same shape: both
/// re-anchor a single base-anchored CTE, so both carry the same restrictions.
/// Probed against Snowflake (TECH-DEBT #36): it computes the snapshot inside the
/// metric's own-grain aggregation and joins pre-aggregated results, rather than
/// ranking rows a base-anchored join has already multiplied.
///
/// `extra_tables` carries the NA dimensions' source tables and any
/// `where_clause` members'. The NA dims matter especially: an *active*
/// semi-additive metric is by definition one whose NA dim is NOT queried, so its
/// table never appears in `resolved_dims`, yet the snapshot's `ORDER BY` names
/// it. Snowflake accepts an NA dimension declared on another logical table when
/// the reference is qualified (probed), so this must be checked for
/// reachability rather than assumed to be the metric's own table.
pub(super) fn snapshot_cte_anchor(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    extra_tables: &[String],
) -> Option<String> {
    if def.joins.is_empty() || resolved_mets.is_empty() {
        return None;
    }
    if resolved_dims.iter().any(|d| d.source_table.is_none())
        || role_playing_affects_query(def, resolved_dims, resolved_mets, extra_tables, false)
    {
        return None;
    }
    // A metric's `USING` scopes its joins on the base-anchored path; the
    // snapshot emitter does not thread that context, so decline rather than
    // silently re-anchor onto a different relationship instance.
    if resolved_mets
        .iter()
        .any(|m| !m.using_relationships.is_empty())
    {
        return None;
    }

    let graph = GrainGraph::build(def)?;
    let root = graph.root().to_string();

    // Every metric in the query must sit at one shared grain — the snapshot is a
    // SINGLE CTE, so there is only one anchor to give it. A query mixing grains
    // is the multi-grain case, which belongs to the per-grain planner.
    let mut anchor: Option<String> = None;
    for met in resolved_mets {
        let grains = metric_grain_tables(met, def);
        let [only] = grains.as_slice() else {
            return None; // No grain, or one metric spanning several.
        };
        match &anchor {
            None => anchor = Some(only.clone()),
            Some(existing) if existing == only => {}
            Some(_) => return None,
        }
    }
    let anchor = anchor?;
    if anchor == root {
        return None; // Already correct as-is.
    }
    // Only re-anchor where base-anchoring actually inflates — the anchor on the
    // "one" side of the path from the root. The other direction must be left
    // alone: for a metric on a CHILD of the base table, `FROM base LEFT JOIN
    // child` already yields each child row once, and that LEFT JOIN keeps
    // childless parents as NULL-extended rows. Flipping to `FROM child` would
    // silently DROP those groups, and this single-CTE path has no FULL OUTER
    // JOIN reassembly to restore them. Identical reasoning to
    // [`window_cte_anchor`]; see its note for the worked example.
    graph.fanning_relationship(&anchor, &root)?;
    // Everything the CTE must reference has to be reachable from the anchor, or
    // its FROM could not bind the column: the queried dimensions...
    for dim in resolved_dims {
        let table = dim.source_table.as_ref()?.to_ascii_lowercase();
        graph.path(&anchor, &table)?;
    }
    // ...and the NA dimensions / `where_clause` members.
    for table in extra_tables {
        graph.path(&anchor, &table.to_ascii_lowercase())?;
    }
    Some(anchor)
}

pub(super) fn window_cte_anchor(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    where_tables: &[String],
) -> Option<String> {
    if def.joins.is_empty() || resolved_mets.is_empty() {
        return None;
    }
    // Only an all-window query reaches the window emitter — mixing window and
    // aggregate metrics is rejected upstream — and only then is `__sv_agg` the
    // shape being anchored.
    if !resolved_mets.iter().all(|m| m.is_window()) {
        return None;
    }
    if resolved_dims.iter().any(|d| d.source_table.is_none())
        || role_playing_affects_query(def, resolved_dims, resolved_mets, where_tables, false)
    {
        return None;
    }
    if resolved_mets
        .iter()
        .any(|m| !m.using_relationships.is_empty())
    {
        return None;
    }

    let graph = GrainGraph::build(def)?;
    let root = graph.root().to_string();

    // Every window metric's INNER aggregate must sit at the same single table.
    //
    // The inner metric is deliberately what is measured, not the window metric
    // itself: DDL qualifies a window metric with a source alias
    // (`o.running_balance AS SUM(total_balance) OVER (…)`), and
    // `metric_grain_tables` unions that alias with the inner's for its own
    // conservative fan-trap purpose. Here that union would report two grains for
    // every DDL-declared window metric and decline unconditionally. The alias on
    // a window metric is declarative — emission never references it, because the
    // window function runs over `__sv_agg`'s columns — so the inner aggregate's
    // grain is the one the CTE must anchor at.
    let mut anchor: Option<String> = None;
    for met in resolved_mets {
        let ws = met.window_spec.as_ref()?;
        if ws.inner_metric.eq_ignore_ascii_case(&met.name) {
            return None; // Self-referential; not a shape to re-anchor.
        }
        let inner = def
            .metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(&ws.inner_metric))?;
        let grains = metric_grain_tables(inner, def);
        let [only] = grains.as_slice() else {
            return None; // No grain, or an inner spanning several — decline.
        };
        match &anchor {
            None => anchor = Some(only.clone()),
            Some(existing) if existing == only => {}
            Some(_) => return None, // Two inner aggregates at different grains.
        }
    }
    let anchor = anchor?;
    if anchor == root {
        return None; // Already correct as-is.
    }
    // Only re-anchor in the direction where base-anchoring actually inflates —
    // when the anchor is on the "one" side of the path from the root, so joining
    // it to the base table replicates each of its rows once per base row.
    //
    // The other direction must be left alone, and not merely as an optimisation:
    // for a metric on a CHILD of the base table, `FROM base LEFT JOIN child`
    // already yields each child row once, and its LEFT JOIN deliberately keeps
    // childless parents as NULL-extended rows (a `COUNT` of 0 for them). Flipping
    // to `FROM child LEFT JOIN base` would silently DROP those groups — an order
    // with no line items would vanish from the result rather than counting 0.
    // Per-grain's own planner can re-anchor either way because it FULL OUTER
    // JOINs the grain CTEs, which restores such groups; this single-CTE path has
    // no such reassembly, so it restricts itself to the safe direction.
    //
    // `?` discards the relationship NAME deliberately — only its presence matters
    // here, and `None` (no fanning edge, so no inflation to fix) declines.
    graph.fanning_relationship(&anchor, &root)?;
    // Each queried dimension must be joinable from the anchor, or the CTE's
    // FROM could not reach the column its SELECT names.
    for dim in resolved_dims {
        let table = dim.source_table.as_ref()?.to_ascii_lowercase();
        graph.path(&anchor, &table)?;
    }
    Some(anchor)
}

/// Whether the query's shape is one the per-grain emitter can express.
///
/// Deliberately conservative: anything outside the plain-aggregate,
/// qualified-dimension core keeps the v0.11.0 fan-trap error rather than being
/// routed through a strategy that was not designed for it.
fn is_eligible(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    where_tables: &[String],
) -> bool {
    // A dimension with no source table is an unqualified expression, resolved
    // against whatever the FROM happens to expose. That is well defined only
    // when the anchor is the base table (the case per-grain never rewrites), so
    // decline rather than re-anchor an expression whose binding would move.
    if resolved_dims.iter().any(|d| d.source_table.is_none()) {
        return false;
    }
    if role_playing_affects_query(def, resolved_dims, resolved_mets, where_tables, true) {
        return false;
    }

    let queried_dim_keys: HashSet<String> = resolved_dims
        .iter()
        .map(|d| crate::ident::normalize_ident_part(&d.name))
        .collect();
    // A metric's `USING` is honoured only in the one shape this emitter threads:
    // every relationship it names targets a role-played table whose role was
    // resolved into `roles` and is therefore emitted as a scoped join. `USING`
    // naming anything else — a relationship to a table reached only one way,
    // where the base-anchored path still scopes the alias — is left on that
    // path, unchanged, rather than guessed at here.
    let roles = scoped_roles(def, resolved_dims, resolved_mets);
    let using_is_threaded = |m: &Metric| {
        m.using_relationships.iter().all(|rel| {
            let rel = rel.to_ascii_lowercase();
            def.joins.iter().any(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.to_ascii_lowercase() == rel)
                    && roles.get(&j.table.to_ascii_lowercase()) == Some(&rel)
            })
        })
    };
    resolved_mets.iter().all(|met| {
        super::facts::collect_transitive_metric_names(met, &def.metrics)
            .iter()
            .filter_map(|name| {
                def.metrics
                    .iter()
                    .find(|m| crate::ident::normalize_ident_part(&m.name) == *name)
            })
            .all(|m| {
                !m.is_window()
                    && using_is_threaded(m)
                    && !super::semi_additive::is_active_semi_additive(def, m, &queried_dim_keys)
            })
    })
}

/// Emit the SQL for a per-grain [`Plan`].
///
/// Single grain: a flat `SELECT` anchored at that grain (the base-anchored shape
/// with a different anchor). Several grains: one CTE per grain, combined with
/// `FULL OUTER JOIN` on the queried dimensions — NULL-safe, so a group present
/// at one grain and absent at another survives with a NULL metric — or
/// `CROSS JOIN` when the query has no dimensions and each grain yields one row.
pub(super) fn expand_per_grain(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    plan: &Plan,
    where_clause: Option<&ResolvedWhere>,
) -> String {
    // Resolved once here and threaded into both renderers, so the join and the
    // dimension expression cannot disagree about which role a table plays.
    let roles = scoped_roles(def, resolved_dims, resolved_mets);
    // A cross-grain assembly always spans at least two groups, so a single-group
    // plan holds only `Direct` outputs — but check rather than assert: falling
    // through to the general emitter is correct for any plan, and a wrong
    // assumption here would panic inside DuckDB rather than mis-render.
    if plan.is_single_group()
        && plan
            .outputs
            .iter()
            .all(|o| matches!(o, Output::Direct { .. }))
    {
        return render_single_grain(
            def,
            resolved_dims,
            resolved_mets,
            plan,
            where_clause,
            &roles,
        );
    }
    render_multi_grain(
        def,
        resolved_dims,
        resolved_mets,
        plan,
        where_clause,
        &roles,
    )
}

/// The one-grain case: the ordinary aggregation shape, anchored at the metrics'
/// own table instead of the base table.
fn render_single_grain(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    plan: &Plan,
    where_clause: Option<&ResolvedWhere>,
    roles: &HashMap<String, String>,
) -> String {
    let group = &plan.groups[0];
    let where_tables = where_clause
        .map(|w| w.source_tables.clone())
        .unwrap_or_default();
    let mut items: Vec<SelectItem> = resolved_dims
        .iter()
        .map(|dim| {
            SelectItem::new(
                dim_expr(dim, roles),
                dim.output_type.clone(),
                quote_stored_ident(&dim.name),
            )
        })
        .collect();
    for (met, output) in resolved_mets.iter().zip(&plan.outputs) {
        let Output::Direct { column, .. } = output else {
            continue; // Caller established every output is Direct here.
        };
        items.push(SelectItem::new(
            group.exprs[*column].clone(),
            met.output_type.clone(),
            quote_stored_ident(&met.name),
        ));
    }
    let group_by = if resolved_dims.is_empty() {
        GroupBy::None
    } else {
        GroupBy::Ordinals(resolved_dims.len())
    };
    SelectSpec {
        // Filters the anchor's rows on their way into the aggregation, the same
        // position the base-anchored path uses.
        where_clause: where_clause.map(|w| w.sql.clone()),
        distinct: false,
        items,
        from: FromSource::AnchorTable {
            def,
            anchor: group.anchor.clone(),
            joins: anchor_joins(def, &group.anchor, resolved_dims, &where_tables, roles),
        },
        group_by,
    }
    .render()
}

/// The general case: one CTE per grain, joined on the queried dimensions.
fn render_multi_grain(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    plan: &Plan,
    where_clause: Option<&ResolvedWhere>,
    roles: &HashMap<String, String>,
) -> String {
    let mut sql = String::with_capacity(512);
    let where_tables = where_clause
        .map(|w| w.source_tables.clone())
        .unwrap_or_default();
    for (i, group) in plan.groups.iter().enumerate() {
        sql.push_str(if i == 0 { "WITH " } else { ",\n" });
        sql.push_str(&grain_cte(i));
        sql.push_str(" AS (\n    SELECT\n");
        let mut items: Vec<String> = Vec::new();
        for (d, dim) in resolved_dims.iter().enumerate() {
            let item = SelectItem::new(
                dim_expr(dim, roles),
                dim.output_type.clone(),
                quote_ident(&dim_column(d)),
            );
            items.push(format!("        {}", item.render()));
        }
        for (m, expr) in group.exprs.iter().enumerate() {
            items.push(format!(
                "        {} AS {}",
                expr,
                quote_ident(&metric_column(m))
            ));
        }
        sql.push_str(&items.join(",\n"));
        push_from_anchor(&mut sql, def, &group.anchor, "\n    ");
        push_join_clauses(
            &mut sql,
            &anchor_joins(def, &group.anchor, resolved_dims, &where_tables, roles),
            def,
            "\n    LEFT JOIN ",
        );
        // Inside the CTE, before its GROUP BY: each grain must aggregate over
        // only the matching rows. On the outer query this would instead filter
        // the already-combined result — a post-aggregation filter.
        if let Some(w) = where_clause {
            sql.push_str("\n    WHERE ");
            sql.push_str(&w.sql);
        }
        push_group_by_ordinals(&mut sql, resolved_dims.len(), "\n    ", "        ");
        sql.push_str("\n)");
    }
    sql.push('\n');

    // Outer SELECT: dimensions coalesced across the grains that carry them,
    // then each metric read back from its grain column(s).
    let mut items: Vec<SelectItem> = Vec::new();
    for (d, dim) in resolved_dims.iter().enumerate() {
        items.push(SelectItem::new(
            coalesced_key(plan.groups.len(), d),
            None,
            quote_stored_ident(&dim.name),
        ));
    }
    for (met, output) in resolved_mets.iter().zip(&plan.outputs) {
        let expr = match output {
            Output::Direct { group, column } => column_ref(*group, &metric_column(*column)),
            Output::Derived { expr } => expr.clone(),
        };
        items.push(SelectItem::new(
            expr,
            met.output_type.clone(),
            quote_stored_ident(&met.name),
        ));
    }
    let rendered: Vec<String> = items
        .iter()
        .map(|item| format!("    {}", item.render()))
        .collect();
    sql.push_str("SELECT\n");
    sql.push_str(&rendered.join(",\n"));
    sql.push_str("\nFROM ");
    sql.push_str(&grain_cte(0));
    for g in 1..plan.groups.len() {
        if resolved_dims.is_empty() {
            // One row per grain: the combination is a plain product.
            sql.push_str("\nCROSS JOIN ");
            sql.push_str(&grain_cte(g));
        } else {
            sql.push_str("\nFULL OUTER JOIN ");
            sql.push_str(&grain_cte(g));
            let conditions: Vec<String> = (0..resolved_dims.len())
                .map(|d| {
                    format!(
                        "{} IS NOT DISTINCT FROM {}",
                        coalesced_key(g, d),
                        column_ref(g, &dim_column(d))
                    )
                })
                .collect();
            sql.push_str("\n    ON ");
            sql.push_str(&conditions.join("\n    AND "));
        }
    }
    sql
}

/// The join key for dimension `d` over the first `groups` grain CTEs: a plain
/// column reference for one, `COALESCE(...)` beyond that.
///
/// `FULL OUTER JOIN` leaves the key NULL on whichever side has no matching
/// group, so each successive join — and the output column — must read the key
/// from whichever grain supplied it.
fn coalesced_key(groups: usize, d: usize) -> String {
    let refs: Vec<String> = (0..groups).map(|g| column_ref(g, &dim_column(d))).collect();
    if refs.len() == 1 {
        refs.into_iter().next().unwrap_or_default()
    } else {
        format!("COALESCE({})", refs.join(", "))
    }
}

/// The `LEFT JOIN`s a grain CTE anchored at `anchor` needs to reach every
/// queried dimension's table.
///
/// Each hop on the tree path from the anchor to a dimension's table is emitted
/// once, in path order, so every ON clause references only the table it
/// introduces and one already in scope. The planner has already established
/// that none of these paths fans the anchor, so the anchor's row count — and
/// therefore its aggregate — is unaffected by them.
pub(super) fn anchor_joins<'a>(
    def: &'a SemanticViewDefinition,
    anchor: &str,
    resolved_dims: &[&Dimension],
    where_tables: &[String],
    roles: &HashMap<String, String>,
) -> Vec<ResolvedJoin<'a>> {
    let Some(graph) = GrainGraph::build(def) else {
        return Vec::new();
    };
    let mut joins: Vec<ResolvedJoin<'a>> = Vec::new();
    let mut emitted: HashSet<String> = HashSet::new();
    emitted.insert(anchor.to_string());
    // A `where_clause` member's table has to be reachable from THIS grain's
    // anchor, exactly like a dimension's — the predicate is injected into every
    // grain CTE, so every one of them must join what the predicate names or the
    // filter would reference an alias absent from that CTE's FROM.
    let dim_sources = resolved_dims.iter().filter_map(|d| d.source_table.clone());
    let sources: Vec<String> = dim_sources.chain(where_tables.iter().cloned()).collect();
    for source in &sources {
        let Some(path) = graph.path(anchor, &source.to_ascii_lowercase()) else {
            continue;
        };
        for pair in path.windows(2) {
            let (from, to) = (&pair[0], &pair[1]);
            // A role-played hop is emitted under its scoped alias, joined on the
            // relationship `USING` NAMED. `edge_between` cannot be used here: it
            // returns whichever of the several edges is declared first, which is
            // the declaration-order mis-binding the eligibility gate exists to
            // keep out of this emitter.
            if let Some(rel) = roles.get(to) {
                let scoped = super::join_resolver::scoped_join_alias(to, rel);
                if !emitted.insert(scoped.clone()) {
                    continue;
                }
                if let Some(join) = def.joins.iter().find(|j| {
                    j.name
                        .as_ref()
                        .is_some_and(|n| n.to_ascii_lowercase() == *rel)
                }) {
                    joins.push(ResolvedJoin {
                        emit_alias: scoped,
                        bare_alias: to.clone(),
                        join,
                        scoped: true,
                    });
                }
                continue;
            }
            if !emitted.insert(to.clone()) {
                continue;
            }
            if let Some(join) = edge_between(def, from, to) {
                joins.push(ResolvedJoin {
                    emit_alias: to.clone(),
                    bare_alias: to.clone(),
                    join,
                    scoped: false,
                });
            }
        }
    }
    joins
}

/// A queried dimension's expression as the grain CTEs must emit it: qualified by
/// its role's scoped alias when `USING` named one, and unchanged otherwise.
///
/// Mirrors what the base-anchored path does with [`ResolvedDim::scoped_alias`],
/// so both paths emit `a__dep.city` for the same query.
fn dim_expr(dim: &Dimension, roles: &HashMap<String, String>) -> String {
    let Some(table) = dim.source_table.as_ref().map(|t| t.to_ascii_lowercase()) else {
        return dim.expr.clone();
    };
    match roles.get(&table) {
        Some(rel) => crate::expr_tokens::rewrite_qualifier(
            &dim.expr,
            &table,
            &super::join_resolver::scoped_join_alias(&table, rel),
        ),
        None => dim.expr.clone(),
    }
}

/// The relationship edge connecting two adjacent tables, in either direction.
///
/// Direction does not matter for emission: [`synthesize_on_clause`] names both
/// sides explicitly, and the caller emits hops in path order so the other side
/// is always already in scope.
fn edge_between<'a>(def: &'a SemanticViewDefinition, a: &str, b: &str) -> Option<&'a Join> {
    def.joins.iter().find(|j| {
        !j.fk_columns.is_empty() && {
            let (from, to) = (
                j.from_alias.to_ascii_lowercase(),
                j.table.to_ascii_lowercase(),
            );
            (from == a && to == b) || (from == b && to == a)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::join_resolver::synthesize_on_clause;

    /// The ON clause is synthesized from the edge regardless of which direction
    /// the path walks it — the property `anchor_joins` relies on when a grain
    /// CTE joins *up* an FK edge its anchor sits below.
    #[test]
    fn edge_between_finds_either_direction() {
        let def = crate::expand::test_helpers::minimal_def("o", "d", "d", "m", "count(*)");
        let def = crate::expand::test_helpers::TestFixtureExt::with_pkfk_join(
            crate::expand::test_helpers::TestFixtureExt::with_table(def, "c", "customers", &["id"]),
            "o_to_c",
            "o",
            "c",
            &["customer_id"],
            &["id"],
        );
        let forward = edge_between(&def, "o", "c").expect("edge o -> c");
        let reverse = edge_between(&def, "c", "o").expect("same edge, walked backwards");
        assert_eq!(forward.name, reverse.name);
        assert_eq!(
            synthesize_on_clause(forward, &def.tables),
            r#""o"."customer_id" = "c"."id""#
        );
    }

    #[test]
    fn coalesced_key_is_bare_for_one_grain_and_coalesced_beyond() {
        assert_eq!(coalesced_key(1, 0), r#""__sv_grain_0"."__sv_d0""#);
        assert_eq!(
            coalesced_key(2, 1),
            r#"COALESCE("__sv_grain_0"."__sv_d1", "__sv_grain_1"."__sv_d1")"#
        );
    }
}
