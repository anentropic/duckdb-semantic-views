use crate::model::{AccessModifier, Dimension, Fact, Metric, SemanticViewDefinition};
use crate::util::suggest_closest;

use super::facts::{
    collect_transitive_metric_names, inline_derived_metrics, inline_facts, toposort_facts,
};
use super::fan_trap::{check_fan_traps, validate_fact_table_path};
use super::join_resolver::resolve_joins_pkfk;
use super::resolution::{find_dimension, find_metric, quote_ident};
use super::role_playing::{check_fact_role_playing_path, find_using_context};
use super::select_spec::{FromSource, GroupBy, SelectItem, SelectSpec};
use super::types::{ExpandError, QueryRequest, ResolvedDim};

/// An entity kind resolvable by name against a [`SemanticViewDefinition`]
/// (dimensions, metrics, facts). Encapsulates lookup, the PRIVATE-access
/// policy, and the three error variants so [`resolve_names`] takes the
/// definition plus the requested names — not nine positional closures (R-5).
///
/// Modelling the error variants per kind is what makes a slot transposition
/// unrepresentable: the old positional API let the dimension call sites pass
/// `DuplicateDimension` in the private-error slot (harmless only because
/// dimensions are never private), a mistake the compiler could not catch.
trait Resolvable: Sized {
    /// Find this entity by (possibly qualified) name in the definition.
    fn find<'a>(def: &'a SemanticViewDefinition, name: &str) -> Option<&'a Self>;
    /// Is this resolved entity PRIVATE — barred from direct querying?
    fn is_private(&self) -> bool;
    /// All declared names of this kind, for the not-found error + suggestion.
    fn available(def: &SemanticViewDefinition) -> Vec<String>;
    /// Error: the same entity was requested twice (keyed on resolved identity).
    fn duplicate_err(view_name: String, name: String) -> ExpandError;
    /// Error: no entity of this kind by that name.
    fn unknown_err(
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    ) -> ExpandError;
    /// Error: the entity is PRIVATE. Never called for kinds whose
    /// [`is_private`](Self::is_private) is always `false`.
    fn private_err(view_name: String, name: String) -> ExpandError;
}

impl Resolvable for Fact {
    fn find<'a>(def: &'a SemanticViewDefinition, name: &str) -> Option<&'a Self> {
        def.facts
            .iter()
            .find(|f| crate::ident::ident_matches(&f.name, name))
    }
    fn is_private(&self) -> bool {
        self.access == AccessModifier::Private
    }
    fn available(def: &SemanticViewDefinition) -> Vec<String> {
        def.facts.iter().map(|f| f.name.clone()).collect()
    }
    fn duplicate_err(view_name: String, name: String) -> ExpandError {
        ExpandError::DuplicateFact { view_name, name }
    }
    fn unknown_err(
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    ) -> ExpandError {
        ExpandError::UnknownFact {
            view_name,
            name,
            available,
            suggestion,
        }
    }
    fn private_err(view_name: String, name: String) -> ExpandError {
        ExpandError::PrivateFact { view_name, name }
    }
}

impl Resolvable for Dimension {
    fn find<'a>(def: &'a SemanticViewDefinition, name: &str) -> Option<&'a Self> {
        find_dimension(def, name)
    }
    fn is_private(&self) -> bool {
        // Dimensions carry no access modifier — never private.
        false
    }
    fn available(def: &SemanticViewDefinition) -> Vec<String> {
        def.dimensions.iter().map(|d| d.name.clone()).collect()
    }
    fn duplicate_err(view_name: String, name: String) -> ExpandError {
        ExpandError::DuplicateDimension { view_name, name }
    }
    fn unknown_err(
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    ) -> ExpandError {
        ExpandError::UnknownDimension {
            view_name,
            name,
            available,
            suggestion,
        }
    }
    fn private_err(_view_name: String, _name: String) -> ExpandError {
        // `is_private` is always false for dimensions, so `resolve_names`
        // never reaches this. There is no `PrivateDimension` variant; the old
        // positional API filled this slot with `DuplicateDimension` (dead but
        // misleading) — the trait removes the footgun entirely.
        unreachable!("dimensions cannot be private")
    }
}

impl Resolvable for Metric {
    fn find<'a>(def: &'a SemanticViewDefinition, name: &str) -> Option<&'a Self> {
        find_metric(def, name)
    }
    fn is_private(&self) -> bool {
        self.access == AccessModifier::Private
    }
    fn available(def: &SemanticViewDefinition) -> Vec<String> {
        def.metrics.iter().map(|m| m.name.clone()).collect()
    }
    fn duplicate_err(view_name: String, name: String) -> ExpandError {
        ExpandError::DuplicateMetric { view_name, name }
    }
    fn unknown_err(
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    ) -> ExpandError {
        ExpandError::UnknownMetric {
            view_name,
            name,
            available,
            suggestion,
        }
    }
    fn private_err(view_name: String, name: String) -> ExpandError {
        ExpandError::PrivateMetric { view_name, name }
    }
}

/// Resolve a list of requested names to their [`Resolvable`] definitions,
/// checking for unknown names, duplicates, and PRIVATE access.
///
/// Duplicate detection keys on the RESOLVED item's identity, not the raw
/// request string (SG-14): `region` and `o.region` resolve to the same
/// dimension and are rejected as duplicates instead of emitting the same
/// column twice.
fn resolve_names<'a, T: Resolvable, N: AsRef<str>>(
    names: &[N],
    view_name: &str,
    def: &'a SemanticViewDefinition,
) -> Result<Vec<&'a T>, ExpandError> {
    let mut resolved = Vec::with_capacity(names.len());
    let mut seen: std::collections::HashSet<*const T> = std::collections::HashSet::new();
    for name in names {
        let name_str = name.as_ref();
        let item = T::find(def, name_str).ok_or_else(|| {
            let available = T::available(def);
            let suggestion = suggest_closest(name_str, &available);
            T::unknown_err(
                view_name.to_string(),
                name_str.to_string(),
                available,
                suggestion,
            )
        })?;
        if !seen.insert(std::ptr::from_ref(item)) {
            return Err(T::duplicate_err(
                view_name.to_string(),
                name_str.to_string(),
            ));
        }
        if item.is_private() {
            return Err(T::private_err(view_name.to_string(), name_str.to_string()));
        }
        resolved.push(item);
    }
    Ok(resolved)
}

/// Expand a fact query into unaggregated SQL.
///
/// Facts are row-level expressions — the generated SQL has no GROUP BY and no
/// aggregation. Fact expressions are resolved via `inline_facts` (DAG resolution)
/// just like metric expansion inlines facts into aggregate expressions.
///
/// Dimensions, when present, add columns to SELECT but do NOT trigger GROUP BY
/// (unlike metric queries where dims + metrics => GROUP BY).
#[allow(clippy::too_many_lines)]
fn expand_facts(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate + resolve requested facts.
    let resolved_facts = resolve_names::<Fact, _>(&req.facts, view_name, def)?;

    // 2. Resolve requested dimensions (same logic as expand()).
    let resolved_dims = resolve_names::<Dimension, _>(&req.dimensions, view_name, def)?;

    // 3. Validate table path constraint (FACT-04).
    let fact_tables: Vec<String> = resolved_facts
        .iter()
        .filter_map(|f| f.source_table.clone())
        .collect();
    let dim_tables: Vec<String> = resolved_dims
        .iter()
        .filter_map(|d| d.source_table.clone())
        .collect();
    validate_fact_table_path(view_name, def, &fact_tables, &dim_tables)?;

    // 3b. Role-playing ambiguity detection (SG-17), mirroring the metrics
    // path in expand(). Fact queries carry no metrics, so there is never a
    // USING context to disambiguate: a dimension on a table reached by
    // multiple named relationships always raises AmbiguousPath here — the
    // same error the metrics path raises when no co-queried metric supplies
    // USING. Previously the facts path skipped this check and silently bound
    // the dimension to an arbitrary relationship edge.
    for dim in &resolved_dims {
        let _ = find_using_context(view_name, def, dim, &[])?;
    }

    // 3c. EXP-5: a fact sourced on (or reached only through) a role-playing
    // table has no USING context to pick a role — reject rather than silently
    // binding to the first-declared relationship, mirroring the dimension
    // check above.
    for fact in &resolved_facts {
        check_fact_role_playing_path(view_name, def, fact)?;
    }

    // 4. Resolve fact expressions via DAG inlining (fact-to-fact dependencies).
    let topo_order = toposort_facts(&def.facts).map_err(|e| ExpandError::CycleDetected {
        view_name: view_name.to_string(),
        cycle_description: e,
    })?;

    // 5. Build the SELECT list (no DISTINCT, no aggregation).
    let mut items: Vec<SelectItem> = Vec::new();

    // Dimensions first
    for dim in &resolved_dims {
        items.push(SelectItem::new(
            dim.expr.clone(),
            dim.output_type.clone(),
            quote_ident(&dim.name),
        ));
    }

    // Then facts (inlined expressions, no aggregation)
    for fact in &resolved_facts {
        let resolved_expr = inline_facts(&fact.expr, &def.facts, &topo_order);
        items.push(SelectItem::new(
            resolved_expr,
            fact.output_type.clone(),
            quote_ident(&fact.name),
        ));
    }

    // 6. JOIN clauses — resolve required joins for dim + fact source tables.
    // Fact queries have no metrics; fact source tables are resolved through
    // the same path walk as dimensions (SG-10) and their joins are appended
    // after the dimension-driven joins.
    let fact_sources: Vec<String> = resolved_facts
        .iter()
        .filter_map(|f| f.source_table.clone())
        .collect();
    let joins = resolve_joins_pkfk(def, &resolved_dims, &[], &fact_sources);

    // 7. A fact query is an unaggregated top-level SELECT over the base table
    //    (+ joins): no DISTINCT, no GROUP BY.
    Ok(SelectSpec {
        distinct: false,
        items,
        from: FromSource::BaseTable { def, joins },
        group_by: GroupBy::None,
    }
    .render())
}

/// Expand a semantic view definition into a SQL query string.
///
/// Takes a view name (for error messages), its definition, and a query request
/// specifying which dimensions and metrics to include. Returns the generated SQL
/// or an `ExpandError` if the request is invalid.
///
/// # Errors
///
/// Returns `ExpandError` if:
/// - Neither dimensions nor metrics are requested (`EmptyRequest`)
/// - A requested dimension or metric name is not found (`UnknownDimension`, `UnknownMetric`)
/// - A dimension or metric name is duplicated (`DuplicateDimension`, `DuplicateMetric`)
#[allow(clippy::too_many_lines)]
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 0. Facts and metrics are mutually exclusive.
    if !req.facts.is_empty() && !req.metrics.is_empty() {
        return Err(ExpandError::FactsMetricsMutualExclusion {
            view_name: view_name.to_string(),
        });
    }

    // 1. Validate: at least one dimension, metric, or fact is required.
    if req.dimensions.is_empty() && req.metrics.is_empty() && req.facts.is_empty() {
        return Err(ExpandError::EmptyRequest {
            view_name: view_name.to_string(),
        });
    }

    // Dispatch to fact expansion path when facts are requested.
    if !req.facts.is_empty() {
        return expand_facts(view_name, def, req);
    }

    // 2. Resolve requested dimensions to their definitions.
    let resolved_dims = resolve_names::<Dimension, _>(&req.dimensions, view_name, def)?;

    // 3. Resolve requested metrics to their definitions.
    // Phase 43: PRIVATE access check -- private metrics cannot be queried directly.
    // Derived metrics that reference private bases still work because
    // inline_derived_metrics resolves expressions, not access modifiers.
    let resolved_mets = resolve_names::<Metric, _>(&req.metrics, view_name, def)?;

    // Phase 55: Materialization routing.
    // Attempt to route to a pre-aggregated table if an exact match exists.
    // Returns None if no match, or if any metric is semi-additive / window.
    if let Some(routed_sql) =
        super::materialization::try_route_materialization(def, &resolved_dims, &resolved_mets)
    {
        return Ok(routed_sql);
    }

    // 4. Pre-compute all metric expressions: inline facts into base metrics,
    //    then inline metric references into derived metrics.
    let topo_order = toposort_facts(&def.facts).map_err(|e| ExpandError::CycleDetected {
        view_name: view_name.to_string(),
        cycle_description: e,
    })?;
    let resolved = inline_derived_metrics(&def.metrics, &def.facts, &topo_order, &def.tables)
        .map_err(|e| ExpandError::CycleDetected {
            view_name: view_name.to_string(),
            cycle_description: e,
        })?;

    // SG-8: fail loudly when a REQUESTED metric (directly, via a derived
    // metric, or as a window metric's inner aggregate) depends on a COUNT(*)
    // that could not be rewritten to COUNT(<pk>) — a non-base source table
    // with no PRIMARY KEY declared. Emitting it as-is would count
    // NULL-extended LEFT JOIN rows (one per childless base row).
    if !resolved.count_star_no_pk.is_empty() {
        for met in &resolved_mets {
            for name in collect_transitive_metric_names(met, &def.metrics) {
                if let Some(table_alias) = resolved.count_star_no_pk.get(&name) {
                    let metric_name = def
                        .metrics
                        .iter()
                        .find(|m| crate::ident::ident_matches(&m.name, &name))
                        .map_or(name.clone(), |m| m.name.clone());
                    return Err(ExpandError::CountStarRequiresPrimaryKey {
                        view_name: view_name.to_string(),
                        metric_name,
                        table_alias: table_alias.clone(),
                    });
                }
            }
        }
    }
    let resolved_exprs = resolved.exprs;

    // Phase 31: Check for fan traps before generating SQL.
    check_fan_traps(view_name, def, &resolved_dims, &resolved_mets)?;

    // Phase 32: pair each resolved dimension with its role-playing scoped alias
    // (e.g. "a__dep_airport"). R-8 (code-review 2026-07-11): zipped into
    // `ResolvedDim` so the alias travels with its dimension instead of a
    // position-indexed side array (`dim_scoped_aliases[i]`).
    let mut resolved: Vec<ResolvedDim> = Vec::with_capacity(resolved_dims.len());
    for &dim in &resolved_dims {
        let scoped_alias = find_using_context(view_name, def, dim, &resolved_mets)?;
        resolved.push(ResolvedDim { dim, scoped_alias });
    }

    // Phase 47: Check if any resolved metric ACTUALLY needs semi-additive expansion.
    // A semi-additive metric only needs CTE treatment when at least one of its
    // NA dims is NOT in the queried dimension set. When ALL NA dims are in the
    // query, the metric acts as regular (Snowflake semantics). The predicate
    // is shared with expand_semi_additive and the fan-trap check (SG-6).
    // Canonical keys (quote-stripped + folded) so a dotted/quoted NA reference
    // resolves against the queried dims (#30, shared with the CTE path).
    let queried_dim_keys: std::collections::HashSet<String> = resolved_dims
        .iter()
        .map(|d| crate::ident::normalize_ident_part(&d.name))
        .collect();
    let has_active_semi_additive = resolved_mets
        .iter()
        .any(|m| super::semi_additive::is_active_semi_additive(def, m, &queried_dim_keys));

    if has_active_semi_additive {
        return super::semi_additive::expand_semi_additive(
            view_name,
            def,
            &resolved,
            &resolved_mets,
            &resolved_exprs,
        );
    }

    // Phase 48: Check if any resolved metric is a window function metric.
    let has_window = resolved_mets.iter().any(|m| m.is_window());
    if has_window {
        // Window metrics cannot be mixed with aggregate metrics.
        let window_names: Vec<String> = resolved_mets
            .iter()
            .filter(|m| m.is_window())
            .map(|m| m.name.clone())
            .collect();
        let aggregate_names: Vec<String> = resolved_mets
            .iter()
            .filter(|m| !m.is_window())
            .map(|m| m.name.clone())
            .collect();
        if !aggregate_names.is_empty() {
            return Err(ExpandError::WindowAggregateMixing {
                view_name: view_name.to_string(),
                window_metrics: window_names,
                aggregate_metrics: aggregate_names,
            });
        }
        return super::window::expand_window_metrics(
            view_name,
            def,
            &resolved,
            &resolved_mets,
            &resolved_exprs,
        );
    }

    // 5. Build the top-level SELECT.
    //    Dimensions-only (no metrics): SELECT DISTINCT, no GROUP BY.
    //    Metrics-only (no dimensions): SELECT (global aggregate), no GROUP BY.
    //    Both: SELECT with an ordinal GROUP BY over the dimensions.
    let distinct = !resolved_dims.is_empty() && resolved_mets.is_empty();

    let mut items: Vec<SelectItem> = Vec::new();
    for rd in &resolved {
        let dim = rd.dim;
        let mut base_expr = dim.expr.clone();
        // Phase 32: If this dimension has a scoped alias, rewrite the expression.
        if let Some(ref scoped) = rd.scoped_alias {
            if let Some(ref st) = dim.source_table {
                // Rewrite the source-table qualifier to the scoped alias
                // e.g., "a.city" -> "a__dep_airport.city"
                base_expr = crate::expr_tokens::rewrite_qualifier(&base_expr, st, scoped);
            }
        }
        items.push(SelectItem::new(
            base_expr,
            dim.output_type.clone(),
            quote_ident(&dim.name),
        ));
    }
    for met in &resolved_mets {
        // Look up the pre-computed resolved expression (handles both base +
        // derived metrics) by the metric's canonical key, matching how
        // `inline_derived_metrics` keys the map (EXP-6).
        let resolved_expr = resolved_exprs
            .get(&crate::ident::normalize_ident_part(&met.name))
            .cloned()
            .unwrap_or_else(|| met.expr.clone());
        items.push(SelectItem::new(
            resolved_expr,
            met.output_type.clone(),
            quote_ident(&met.name),
        ));
    }

    // 6. Join resolution via PK/FK graph.
    //    The resolver returns structured edges in emission order; role-playing
    //    scoped joins (e.g. "a__dep_airport") follow the bare joins.
    let joins = resolve_joins_pkfk(def, &resolved_dims, &resolved_mets, &[]);

    // 7. GROUP BY (only when both dimensions and metrics are present).
    //    Ordinal positions avoid ambiguity when an expression matches its alias
    //    (e.g. `status AS "status"`) — see push_group_by_ordinals (E-1).
    let group_by = if !resolved_dims.is_empty() && !resolved_mets.is_empty() {
        GroupBy::Ordinals(resolved_dims.len())
    } else {
        GroupBy::None
    };

    Ok(SelectSpec {
        distinct,
        items,
        from: FromSource::BaseTable { def, joins },
        group_by,
    }
    .render())
}
