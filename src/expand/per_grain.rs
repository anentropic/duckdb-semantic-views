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
//! - *active* semi-additive metrics, whose own CTE strategy is base-anchored
//!   (see `docs/`);
//! - a query that reaches a **role-played** table — one this query's dimensions
//!   or metric grains sit on, or can reach only through. Which of its several
//!   relationship instances a grain CTE should join is exactly what `USING`
//!   answers on the base-anchored path, and the grain CTEs do not carry that
//!   context yet. Note the scope: this is asked of the QUERY, not the
//!   definition, so a definition that declares role-playing somewhere does not
//!   lose per-grain emission for queries that never reach it.
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
) -> Option<Plan> {
    if def.joins.is_empty() || resolved_mets.is_empty() {
        return None; // Single-table view, or a dimensions-only query.
    }
    if !is_eligible(def, resolved_dims, resolved_mets) {
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
fn role_playing_affects_query(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> bool {
    let mut tables: Vec<String> = resolved_dims
        .iter()
        .filter_map(|d| d.source_table.as_ref().map(|s| s.to_ascii_lowercase()))
        .collect();
    for met in resolved_mets {
        tables.extend(metric_grain_tables(met, def));
    }
    tables.iter().any(|table| {
        // The view name only decorates an error that is discarded here: a
        // definition too malformed to walk declines per-grain, and the
        // base-anchored path this falls back to reports the real diagnostic.
        !matches!(
            super::role_playing::role_playing_on_path("", def, table),
            Ok(None)
        )
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
pub(super) fn window_cte_anchor(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
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
        || role_playing_affects_query(def, resolved_dims, resolved_mets)
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
) -> bool {
    // A dimension with no source table is an unqualified expression, resolved
    // against whatever the FROM happens to expose. That is well defined only
    // when the anchor is the base table (the case per-grain never rewrites), so
    // decline rather than re-anchor an expression whose binding would move.
    if resolved_dims.iter().any(|d| d.source_table.is_none()) {
        return false;
    }
    if role_playing_affects_query(def, resolved_dims, resolved_mets) {
        return false;
    }

    let queried_dim_keys: HashSet<String> = resolved_dims
        .iter()
        .map(|d| crate::ident::normalize_ident_part(&d.name))
        .collect();
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
                    && m.using_relationships.is_empty()
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
        return render_single_grain(def, resolved_dims, resolved_mets, plan, where_clause);
    }
    render_multi_grain(def, resolved_dims, resolved_mets, plan, where_clause)
}

/// The one-grain case: the ordinary aggregation shape, anchored at the metrics'
/// own table instead of the base table.
fn render_single_grain(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    plan: &Plan,
    where_clause: Option<&ResolvedWhere>,
) -> String {
    let group = &plan.groups[0];
    let where_tables = where_clause
        .map(|w| w.source_tables.clone())
        .unwrap_or_default();
    let mut items: Vec<SelectItem> = resolved_dims
        .iter()
        .map(|dim| {
            SelectItem::new(
                dim.expr.clone(),
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
            joins: anchor_joins(def, &group.anchor, resolved_dims, &where_tables),
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
                dim.expr.clone(),
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
            &anchor_joins(def, &group.anchor, resolved_dims, &where_tables),
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
