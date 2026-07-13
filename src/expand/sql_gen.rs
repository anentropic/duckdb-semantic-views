use crate::model::{AccessModifier, Dimension, Fact, Metric, SemanticViewDefinition};
use crate::util::{replace_word_boundary, suggest_closest};

use super::facts::{
    collect_transitive_metric_names, inline_derived_metrics, inline_facts, toposort_facts,
};
use super::fan_trap::{check_fan_traps, validate_fact_table_path};
use super::join_resolver::resolve_joins_pkfk;
use super::resolution::{find_dimension, find_metric, quote_ident};
use super::role_playing::find_using_context;
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
                        .find(|m| m.name.eq_ignore_ascii_case(&name))
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
    let queried_dim_names: std::collections::HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    let has_active_semi_additive = resolved_mets
        .iter()
        .any(|m| super::semi_additive::is_active_semi_additive(m, &queried_dim_names));

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
                // Replace bare alias with scoped alias in expression
                // e.g., "a.city" -> "a__dep_airport.city"
                base_expr = replace_word_boundary(&base_expr, st, scoped);
            }
        }
        items.push(SelectItem::new(
            base_expr,
            dim.output_type.clone(),
            quote_ident(&dim.name),
        ));
    }
    for met in &resolved_mets {
        // Look up the pre-computed resolved expression (handles both base + derived metrics)
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
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

#[cfg(test)]
mod tests {
    use crate::expand::{
        expand, DimensionName, ExpandError, FanTrapError, MetricName, QueryRequest,
    };

    mod expand_tests {
        use super::*;
        use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
        use crate::model::SemanticViewDefinition;

        #[test]
        fn test_basic_single_dimension_single_metric() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    region AS \"region\",
    sum(amount) AS \"total_revenue\"
FROM \"orders\" AS \"orders\"
GROUP BY
    1";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_multiple_dimensions_multiple_metrics() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region"), DimensionName::new("status")],
                metrics: vec![
                    MetricName::new("total_revenue"),
                    MetricName::new("order_count"),
                ],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.starts_with("SELECT\n"), "Should start with SELECT");
            assert!(
                sql.contains("region AS \"region\""),
                "Should include region dim"
            );
            assert!(
                sql.contains("status AS \"status\""),
                "Should include status dim"
            );
            assert!(
                sql.contains("sum(amount) AS \"total_revenue\""),
                "Should include revenue metric"
            );
            assert!(
                sql.contains("count(*) AS \"order_count\""),
                "Should include count metric"
            );
            assert!(
                sql.contains("FROM \"orders\""),
                "Should reference orders table"
            );
            assert!(sql.contains("GROUP BY"), "Should have GROUP BY");
            assert!(sql.contains("1,"), "Should group by ordinal 1");
            assert!(sql.contains("2"), "Should group by ordinal 2");
        }

        #[test]
        fn test_global_aggregate_no_dimensions() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.starts_with("SELECT\n"), "Should start with SELECT");
            assert!(
                sql.contains("sum(amount) AS \"total_revenue\""),
                "Should include revenue metric"
            );
            assert!(
                sql.contains("FROM \"orders\""),
                "Should reference orders table"
            );
            assert!(!sql.contains("GROUP BY"), "No GROUP BY when no dimensions");
        }

        #[test]
        fn test_identifier_quoting() {
            let def = minimal_def("select", "col", "col", "cnt", "count(*)");
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("col")],
                metrics: vec![MetricName::new("cnt")],
            };
            let sql = expand("test", &def, &req).unwrap();
            // Base table "select" must be quoted
            assert!(sql.contains("FROM \"select\""));
        }

        #[test]
        fn test_dimension_expression_not_quoted() {
            let def = minimal_def(
                "orders",
                "month",
                "date_trunc('month', created_at)",
                "total_revenue",
                "sum(amount)",
            );
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("month")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Expression appears verbatim in SELECT; GROUP BY uses ordinal position
            assert!(sql.contains("date_trunc('month', created_at) AS \"month\""));
            assert!(sql.contains("GROUP BY\n    1"));
        }

        #[test]
        fn test_empty_request_error() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::EmptyRequest { view_name } => {
                    assert_eq!(view_name, "orders");
                }
                other => panic!("Expected EmptyRequest, got: {other}"),
            }
        }

        #[test]
        fn test_dimensions_only_generates_distinct() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region"), DimensionName::new("status")],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.starts_with("SELECT DISTINCT\n"),
                "Should start with SELECT DISTINCT"
            );
            assert!(
                sql.contains("region AS \"region\""),
                "Should include region dim"
            );
            assert!(
                sql.contains("status AS \"status\""),
                "Should include status dim"
            );
            assert!(
                sql.contains("FROM \"orders\""),
                "Should reference orders table"
            );
            assert!(
                !sql.contains("GROUP BY"),
                "No GROUP BY for dims-only (DISTINCT instead)"
            );
        }

        #[test]
        fn test_metrics_only_still_works() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![
                    MetricName::new("total_revenue"),
                    MetricName::new("order_count"),
                ],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.starts_with("SELECT\n"), "Should start with SELECT");
            assert!(
                !sql.starts_with("SELECT DISTINCT"),
                "Should NOT be DISTINCT for metrics-only"
            );
            assert!(
                sql.contains("sum(amount) AS \"total_revenue\""),
                "Should include revenue metric"
            );
            assert!(
                sql.contains("count(*) AS \"order_count\""),
                "Should include count metric"
            );
            assert!(
                sql.contains("FROM \"orders\""),
                "Should reference orders table"
            );
            assert!(!sql.contains("GROUP BY"), "No GROUP BY when no dimensions");
        }

        #[test]
        fn test_case_insensitive_dimension_lookup() {
            let def = minimal_def("orders", "Region", "region", "total_revenue", "sum(amount)");
            // Request uses lowercase "region" but definition has "Region"
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's expression
            assert!(sql.contains("region AS \"Region\""));
            assert!(sql.contains("GROUP BY\n    1"));
        }

        #[test]
        fn test_unknown_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("reigon")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownDimension {
                    view_name,
                    name,
                    available,
                    suggestion,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "reigon");
                    assert!(available.contains(&"region".to_string()));
                    assert_eq!(suggestion, Some("region".to_string()));
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_unknown_metric_error() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("totl_revenue")],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownMetric {
                    view_name,
                    name,
                    available,
                    suggestion,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "totl_revenue");
                    assert!(available.contains(&"total_revenue".to_string()));
                    assert_eq!(suggestion, Some("total_revenue".to_string()));
                }
                other => panic!("Expected UnknownMetric, got: {other}"),
            }
        }

        #[test]
        fn test_unknown_dimension_no_suggestion() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("xyzzy")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownDimension { suggestion, .. } => {
                    assert_eq!(suggestion, None);
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_duplicate_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region"), DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::DuplicateDimension { view_name, name } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "region");
                }
                other => panic!("Expected DuplicateDimension, got: {other}"),
            }
        }

        #[test]
        fn test_duplicate_metric_error() {
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![
                    MetricName::new("total_revenue"),
                    MetricName::new("total_revenue"),
                ],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::DuplicateMetric { view_name, name } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "total_revenue");
                }
                other => panic!("Expected DuplicateMetric, got: {other}"),
            }
        }

        #[test]
        fn test_case_insensitive_metric_lookup() {
            let def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_metric("Total_Revenue", "sum(amount)", None);
            // Request uses lowercase "total_revenue" but definition has "Total_Revenue"
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's name casing in the alias
            assert!(sql.contains("sum(amount) AS \"Total_Revenue\""));
        }

        #[test]
        fn test_error_display_messages() {
            // EmptyRequest
            let err = ExpandError::EmptyRequest {
                view_name: "orders".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("specify at least dimensions"));

            // UnknownDimension with suggestion
            let err = ExpandError::UnknownDimension {
                view_name: "orders".to_string(),
                name: "reigon".to_string(),
                available: vec!["region".to_string(), "status".to_string()],
                suggestion: Some("region".to_string()),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("reigon"));
            assert!(msg.contains("region, status"));
            assert!(msg.contains("Did you mean 'region'?"));

            // UnknownDimension without suggestion
            let err = ExpandError::UnknownDimension {
                view_name: "orders".to_string(),
                name: "xyzzy".to_string(),
                available: vec!["region".to_string()],
                suggestion: None,
            };
            let msg = format!("{err}");
            assert!(msg.contains("xyzzy"));
            assert!(!msg.contains("Did you mean"));

            // UnknownMetric with suggestion
            let err = ExpandError::UnknownMetric {
                view_name: "orders".to_string(),
                name: "totl_revenue".to_string(),
                available: vec!["total_revenue".to_string()],
                suggestion: Some("total_revenue".to_string()),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("totl_revenue"));
            assert!(msg.contains("Did you mean 'total_revenue'?"));

            // DuplicateDimension
            let err = ExpandError::DuplicateDimension {
                view_name: "orders".to_string(),
                name: "region".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("duplicate dimension 'region'"));

            // DuplicateMetric
            let err = ExpandError::DuplicateMetric {
                view_name: "orders".to_string(),
                name: "total_revenue".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("duplicate metric 'total_revenue'"));
        }

        #[test]
        fn test_join_excluded_when_not_needed() {
            let def = orders_view()
                .with_table("customers", "customers", &["id"])
                .with_dimension("customer_name", "customers.name", Some("customers"))
                .with_pkfk_join("cust", "orders", "customers", &["customer_id"], &["id"]);
            // Request only "region" which comes from base table
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "JOIN should not appear when only base-table dims/metrics requested"
            );
        }

        #[test]
        fn test_no_joins_declared_no_error() {
            let def = minimal_def("orders", "region", "region", "total_revenue", "sum(amount)");
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "no JOIN clauses when no joins declared"
            );
        }

        #[test]
        fn test_dot_qualified_base_table() {
            let def = minimal_def(
                "jaffle.raw_orders",
                "status",
                "status",
                "order_count",
                "count(*)",
            );
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("status")],
                metrics: vec![MetricName::new("order_count")],
            };
            let sql = expand("jaffle_orders", &def, &req).unwrap();
            // Must produce "jaffle"."raw_orders" not "jaffle.raw_orders"
            assert!(
                sql.contains("FROM \"jaffle\".\"raw_orders\""),
                "dot-qualified base_table must be split and quoted: {sql}"
            );
        }

        /// When database_name and schema_name are set on the definition,
        /// the generated SQL must fully-qualify table references in FROM.
        /// Without this, ADBC connections fail because they don't maintain
        /// the same catalog/schema search path as normal connections.
        #[test]
        fn test_base_table_qualified_with_catalog_schema() {
            let mut def = minimal_def("orders", "region", "region", "total_revenue", "sum(amount)");
            def.database_name = Some("memory".to_string());
            def.schema_name = Some("main".to_string());
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders_view", &def, &req).unwrap();
            assert!(
                sql.contains("FROM \"memory\".\"main\".\"orders\""),
                "base table must be catalog.schema qualified when database_name/schema_name are set: {sql}"
            );
        }

        /// Same as above but for JOIN targets — joined tables must also be
        /// fully qualified when database_name/schema_name are present.
        #[test]
        fn test_join_table_qualified_with_catalog_schema() {
            let mut def = orders_view()
                .with_table("c", "customers", &["id"])
                .with_pkfk_join("orders_customers", "orders", "c", &["customer_id"], &["id"])
                .with_dimension("customer_name", "c.name", Some("c"));
            def.database_name = Some("memory".to_string());
            def.schema_name = Some("main".to_string());
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![
                    DimensionName::new("region"),
                    DimensionName::new("customer_name"),
                ],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders_view", &def, &req).unwrap();
            assert!(
                sql.contains("FROM \"memory\".\"main\".\"orders\""),
                "base table in JOIN query must be qualified: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"memory\".\"main\".\"customers\""),
                "joined table must be catalog.schema qualified: {sql}"
            );
        }

        /// When only schema_name is set (no database_name), qualify with
        /// just the schema prefix.
        #[test]
        fn test_base_table_qualified_schema_only() {
            let mut def = minimal_def("orders", "region", "region", "total_revenue", "sum(amount)");
            def.schema_name = Some("analytics".to_string());
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders_view", &def, &req).unwrap();
            assert!(
                sql.contains("FROM \"analytics\".\"orders\""),
                "base table must be schema-qualified when only schema_name is set: {sql}"
            );
        }

        /// When neither database_name nor schema_name are set, table refs
        /// remain unqualified (backward compat).
        #[test]
        fn test_base_table_unqualified_when_no_catalog_schema() {
            let def = minimal_def("orders", "region", "region", "total_revenue", "sum(amount)");
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders_view", &def, &req).unwrap();
            // Should NOT have any dot-qualification beyond what's in the table name itself
            assert!(
                sql.contains("FROM \"orders\""),
                "table must remain unqualified when no catalog/schema set: {sql}"
            );
            assert!(
                !sql.contains("FROM \"memory\""),
                "must not inject catalog when not set: {sql}"
            );
        }

        /// Tables that are already dot-qualified in the definition should
        /// NOT get double-qualified.
        #[test]
        fn test_already_qualified_table_not_double_qualified() {
            let mut def = minimal_def(
                "mydb.myschema.orders",
                "region",
                "region",
                "total_revenue",
                "sum(amount)",
            );
            // Even with database_name/schema_name set, a table that's already
            // dot-qualified should be used as-is.
            def.database_name = Some("memory".to_string());
            def.schema_name = Some("main".to_string());
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders_view", &def, &req).unwrap();
            assert!(
                sql.contains("FROM \"mydb\".\"myschema\".\"orders\""),
                "already-qualified table must not be re-qualified: {sql}"
            );
            assert!(
                !sql.contains("\"memory\".\"main\".\"mydb\""),
                "must not double-qualify: {sql}"
            );
        }
    }

    mod phase11_1_expand_tests {
        use super::*;
        use crate::model::TableRef;

        fn def_with_join_columns() -> crate::model::SemanticViewDefinition {
            crate::model::SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    crate::model::Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),

                        ..Default::default()
                    },
                    crate::model::Dimension {
                        name: "tier".to_string(),
                        expr: "c.tier".to_string(),
                        source_table: Some("c".to_string()),

                        ..Default::default()
                    },
                ],
                metrics: vec![crate::model::Metric {
                    name: "revenue".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],

                joins: vec![crate::model::Join {
                    // Modern (Phase 24) FK encoding: source alias `o`, target
                    // alias `c`, with fk/ref columns so the fan-trap safety
                    // check can build the relationship graph (SG-7 / AR-4).
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ref_columns: vec!["id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        #[test]
        fn table_qualified_dimension_lookup_with_matching_source_table() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("o.region")],
                metrics: vec![],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("o.region"),
                "Must include the dimension expr: {sql}"
            );
            assert!(
                sql.contains("AS \"region\""),
                "Must alias as bare name: {sql}"
            );
        }

        #[test]
        fn bare_dimension_name_still_resolves() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            assert!(
                result.is_ok(),
                "Bare name lookup must succeed: {:?}",
                result.err()
            );
        }

        #[test]
        fn table_qualified_unknown_dimension_returns_error() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("o.nosuch")],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            match result {
                Err(ExpandError::UnknownDimension { name, .. }) => {
                    let _ = name;
                }
                other => panic!("Expected UnknownDimension error, got: {:?}", other),
            }
        }

        #[test]
        fn table_qualified_metric_lookup_with_matching_source_table() {
            let def = def_with_join_columns();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("o.revenue")],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("sum(o.amount)"),
                "Must include metric expr: {sql}"
            );
        }
    }

    mod phase12_cast_tests {
        use super::*;
        use crate::expand::test_helpers::TestFixtureExt;
        use crate::model::{Dimension, SemanticViewDefinition};

        #[test]
        fn output_type_on_metric_emits_cast() {
            let mut def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_metric("revenue", "sum(amount)", None);
            def.metrics[0].output_type = Some("BIGINT".to_string());
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(sum(amount) AS BIGINT)"),
                "output_type BIGINT must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn output_type_on_dimension_emits_cast() {
            let mut def = SemanticViewDefinition::default().with_table("orders", "orders", &[]);
            def.dimensions.push(Dimension {
                name: "region_id".to_string(),
                expr: "region_id".to_string(),
                output_type: Some("INTEGER".to_string()),
                ..Default::default()
            });
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region_id")],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(region_id AS INTEGER)"),
                "output_type INTEGER on dimension must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn no_output_type_no_cast() {
            let def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_metric("revenue", "sum(amount)", None);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("CAST(sum(amount) AS"),
                "No output_type must not generate CAST: {sql}"
            );
            assert!(
                sql.contains("sum(amount) AS"),
                "Bare expr must be present: {sql}"
            );
        }
    }

    mod phase26_pkfk_expand_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition, TableRef};

        /// Helper: build a 2-table PK/FK definition (orders -> customers).
        fn pkfk_two_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        ..Default::default()
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        /// Helper: build a 3-table PK/FK definition (li -> o -> c).
        fn pkfk_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "product".to_string(),
                        expr: "li.product".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        ..Default::default()
                    },
                ],
                metrics: vec![Metric {
                    name: "total_qty".to_string(),
                    expr: "sum(li.qty)".to_string(),
                    source_table: Some("li".to_string()),
                    ..Default::default()
                }],

                joins: vec![
                    Join {
                        table: "o".to_string(),
                        from_alias: "li".to_string(),
                        fk_columns: vec!["order_id".to_string()],
                        ..Default::default()
                    },
                    Join {
                        table: "c".to_string(),
                        from_alias: "o".to_string(),
                        fk_columns: vec!["customer_id".to_string()],
                        ..Default::default()
                    },
                ],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        #[test]
        fn test_pkfk_on_clause_simple() {
            let def = pkfk_two_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("total_amount")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("\"o\".\"customer_id\" = \"c\".\"id\""),
                "PK/FK ON clause must use from_alias.fk = to_alias.pk: {sql}"
            );
        }

        #[test]
        fn test_pkfk_on_clause_composite() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "a".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "b".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["pk1".to_string(), "pk2".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "b.detail".to_string(),
                    source_table: Some("b".to_string()),
                    ..Default::default()
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: Some("a".to_string()),
                    ..Default::default()
                }],

                joins: vec![Join {
                    table: "b".to_string(),
                    from_alias: "a".to_string(),
                    fk_columns: vec!["fk1".to_string(), "fk2".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("detail")],
                metrics: vec![MetricName::new("cnt")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("\"a\".\"fk1\" = \"b\".\"pk1\""),
                "First FK/PK pair must appear: {sql}"
            );
            assert!(sql.contains("AND"), "Composite ON must use AND: {sql}");
            assert!(
                sql.contains("\"a\".\"fk2\" = \"b\".\"pk2\""),
                "Second FK/PK pair must appear: {sql}"
            );
        }

        #[test]
        fn test_pkfk_left_join_emitted() {
            let def = pkfk_two_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("total_amount")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN"),
                "PK/FK path must emit LEFT JOIN: {sql}"
            );
            let join_lines: Vec<&str> = sql
                .lines()
                .filter(|l| l.trim().starts_with("LEFT JOIN") || l.trim().starts_with("JOIN"))
                .collect();
            for line in &join_lines {
                assert!(
                    line.trim().starts_with("LEFT JOIN"),
                    "All joins must be LEFT JOIN, got: {line}"
                );
            }
        }

        #[test]
        fn test_pkfk_transitive_join_inclusion() {
            let def = pkfk_three_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("total_qty")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"orders\" AS \"o\""),
                "Transitive intermediate join (o) must be included: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"customers\" AS \"c\""),
                "Target join (c) must be included: {sql}"
            );
        }

        #[test]
        fn test_pkfk_pruning() {
            let def = pkfk_three_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("product")],
                metrics: vec![MetricName::new("total_qty")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "No joins needed when only base-table dims requested: {sql}"
            );
        }

        #[test]
        fn test_pkfk_topological_order() {
            let mut def = pkfk_three_table_def();
            def.joins.reverse();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("total_qty")],
            };
            let sql = expand("test", &def, &req).unwrap();
            let o_pos = sql
                .find("LEFT JOIN \"orders\"")
                .expect("orders join missing");
            let c_pos = sql
                .find("LEFT JOIN \"customers\"")
                .expect("customers join missing");
            assert!(
                o_pos < c_pos,
                "orders (closer to root) must appear before customers (further from root) in topo order: {sql}"
            );
        }
    }

    mod phase27_qualified_refs_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition, TableRef};

        fn qualified_ref_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    ..Default::default()
                }],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        #[test]
        fn test_expand_qualified_column_refs_verbatim() {
            let def = qualified_ref_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("total_amount")],
            };
            let sql = expand("p27_test", &def, &req).unwrap();

            assert!(
                sql.contains("c.name AS"),
                "Qualified dim expr 'c.name' must appear verbatim in SQL: {sql}"
            );

            assert!(
                sql.contains("sum(o.amount) AS"),
                "Qualified metric expr 'sum(o.amount)' must appear verbatim in SQL: {sql}"
            );
        }

        #[test]
        fn test_expand_multiple_qualified_refs_different_tables() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "order_region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        ..Default::default()
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![
                    DimensionName::new("customer_name"),
                    DimensionName::new("order_region"),
                ],
                metrics: vec![MetricName::new("total_amount")],
            };
            let sql = expand("p27_test", &def, &req).unwrap();

            assert!(
                sql.contains("c.name AS"),
                "Qualified dim expr 'c.name' must appear verbatim: {sql}"
            );
            assert!(
                sql.contains("o.region AS"),
                "Qualified dim expr 'o.region' must appear verbatim: {sql}"
            );
            assert!(
                sql.contains("sum(o.amount) AS"),
                "Qualified metric expr 'sum(o.amount)' must appear verbatim: {sql}"
            );
        }
    }

    mod phase29_fact_inlining_tests {
        use super::*;
        use crate::expand::facts::{inline_facts, toposort_facts};
        use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
        use crate::model::{AccessModifier, Fact, SemanticViewDefinition};

        #[test]
        fn toposort_facts_empty() {
            let order = toposort_facts(&[]).unwrap();
            assert!(order.is_empty());
        }

        #[test]
        fn toposort_facts_independent() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "x + 1".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "y + 2".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 2);
            assert!(order.contains(&0));
            assert!(order.contains(&1));
        }

        #[test]
        fn toposort_facts_chain() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price * qty".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * (1 - discount)".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 2);
            let a_pos = order.iter().position(|&x| x == 0).unwrap();
            let b_pos = order.iter().position(|&x| x == 1).unwrap();
            assert!(a_pos < b_pos, "a (leaf) must come before b (depends on a)");
        }

        #[test]
        fn toposort_facts_three_level_chain() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * qty".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
                Fact {
                    name: "c".to_string(),
                    expr: "b * tax".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 3);
            let a_pos = order.iter().position(|&x| x == 0).unwrap();
            let b_pos = order.iter().position(|&x| x == 1).unwrap();
            let c_pos = order.iter().position(|&x| x == 2).unwrap();
            assert!(a_pos < b_pos);
            assert!(b_pos < c_pos);
        }

        #[test]
        fn inline_facts_no_facts() {
            let result = inline_facts("SUM(price)", &[], &[]);
            assert_eq!(result, "SUM(price)");
        }

        #[test]
        fn inline_facts_single_fact() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "price * (1 - discount)".to_string(),
                source_table: None,
                output_type: None,
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(net_price)", &facts, &order);
            assert_eq!(result, "SUM((price * (1 - discount)))");
        }

        #[test]
        fn inline_facts_multi_level() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price * qty".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * (1 - discount)".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(b)", &facts, &order);
            assert_eq!(result, "SUM(((price * qty) * (1 - discount)))");
        }

        #[test]
        fn inline_facts_preserves_parenthesization() {
            let facts = vec![Fact {
                name: "total".to_string(),
                expr: "a + b".to_string(),
                source_table: None,
                output_type: None,
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("x * total", &facts, &order);
            assert_eq!(result, "x * (a + b)");
        }

        #[test]
        fn inline_facts_word_boundary_prevents_collision() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "p * q".to_string(),
                source_table: None,
                output_type: None,
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(net_price_total)", &facts, &order);
            assert_eq!(
                result, "SUM(net_price_total)",
                "Word boundary must prevent matching"
            );
        }

        #[test]
        fn inline_facts_with_qualified_name_in_metric() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "li.price * (1 - li.discount)".to_string(),
                source_table: Some("li".to_string()),
                output_type: None,
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(li.net_price)", &facts, &order);
            assert_eq!(result, "SUM((li.price * (1 - li.discount)))");
        }

        #[test]
        fn expand_with_facts_inlines_into_metric() {
            let def = minimal_def(
                "line_items",
                "region",
                "region",
                "total_net",
                "SUM(net_price)",
            )
            .with_fact("net_price", "price * (1 - discount)", "line_items");
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_net")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM((price * (1 - discount)))"),
                "Fact inlining must resolve net_price in metric expr: {sql}"
            );
        }

        #[test]
        fn expand_without_facts_unchanged() {
            let def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_metric("total", "SUM(amount)", None);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("total")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM(amount) AS"),
                "Without facts, metric expr unchanged: {sql}"
            );
        }

        #[test]
        fn expand_multi_level_facts() {
            let def = SemanticViewDefinition::default()
                .with_table("line_items", "line_items", &[])
                .with_metric("total_tax", "SUM(tax_amount)", None)
                .with_fact("net_price", "extended_price * (1 - discount)", "line_items")
                .with_fact("tax_amount", "net_price * tax_rate", "line_items");
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("total_tax")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM(((extended_price * (1 - discount)) * tax_rate))"),
                "Multi-level fact chain must resolve correctly: {sql}"
            );
        }
    }

    mod phase30_derived_metric_tests {
        use super::*;
        use crate::expand::facts::{inline_derived_metrics, toposort_facts};
        use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
        use crate::model::{
            AccessModifier, Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef,
        };

        #[test]
        fn inline_derived_one_base_one_derived() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    ..Default::default()
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
                .unwrap()
                .exprs;
            assert_eq!(
                resolved.get("profit").unwrap(),
                "(SUM(amount)) - (SUM(unit_cost))"
            );
        }

        #[test]
        fn inline_derived_stacked() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    ..Default::default()
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / revenue * 100".to_string(),
                    ..Default::default()
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
                .unwrap()
                .exprs;
            assert_eq!(
                resolved.get("profit").unwrap(),
                "(SUM(amount)) - (SUM(unit_cost))"
            );
            assert_eq!(
                resolved.get("margin").unwrap(),
                "((SUM(amount)) - (SUM(unit_cost))) / (SUM(amount)) * 100"
            );
        }

        #[test]
        fn inline_derived_with_facts() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(net_price)".to_string(),
                    source_table: Some("li".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "double_rev".to_string(),
                    expr: "revenue * 2".to_string(),
                    ..Default::default()
                },
            ];
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "extended_price * (1 - discount)".to_string(),
                source_table: Some("li".to_string()),
                output_type: None,
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
            }];
            let topo_order = toposort_facts(&facts).unwrap();
            let resolved = inline_derived_metrics(&metrics, &facts, &topo_order, &[])
                .unwrap()
                .exprs;
            assert_eq!(
                resolved.get("revenue").unwrap(),
                "SUM((extended_price * (1 - discount)))"
            );
            assert_eq!(
                resolved.get("double_rev").unwrap(),
                "(SUM((extended_price * (1 - discount)))) * 2"
            );
        }

        #[test]
        fn inline_derived_parenthesization_prevents_precedence_error() {
            let metrics = vec![
                Metric {
                    name: "a".to_string(),
                    expr: "SUM(x)".to_string(),
                    source_table: Some("t".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "b".to_string(),
                    expr: "SUM(y)".to_string(),
                    source_table: Some("t".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "a - b".to_string(),
                    ..Default::default()
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / a".to_string(),
                    ..Default::default()
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
                .unwrap()
                .exprs;
            assert_eq!(
                resolved.get("margin").unwrap(),
                "((SUM(x)) - (SUM(y))) / (SUM(x))"
            );
        }

        #[test]
        fn inline_derived_word_boundary_safety() {
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "revenue_total".to_string(),
                    expr: "SUM(total)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "derived".to_string(),
                    expr: "revenue + revenue_total".to_string(),
                    ..Default::default()
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
                .unwrap()
                .exprs;
            assert_eq!(
                resolved.get("derived").unwrap(),
                "(SUM(amount)) + (SUM(total))"
            );
        }

        #[test]
        fn expand_derived_metric_correct_sql() {
            let def = minimal_def("orders", "region", "region", "revenue", "SUM(amount)")
                .with_metric("cost", "SUM(unit_cost)", Some("o"))
                .with_metric("profit", "revenue - cost", None);
            // Fix revenue source_table to match original
            let mut def = def;
            def.metrics[0].source_table = Some("o".to_string());
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("profit")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("(SUM(amount)) - (SUM(unit_cost)) AS \"profit\""),
                "Derived metric must expand to inlined expression: {sql}"
            );
            assert!(
                sql.contains("GROUP BY\n    1"),
                "GROUP BY should reference only the dimension: {sql}"
            );
        }

        #[test]
        fn expand_derived_only_no_base_metrics_requested() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(li.unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        ..Default::default()
                    },
                ],
                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("profit")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(sql.contains("LEFT JOIN \"line_items\" AS \"li\""), "JOIN to li must be included for derived metric referencing li-based metrics: {sql}");
            assert!(
                sql.contains("(SUM(li.amount)) - (SUM(li.unit_cost)) AS \"profit\""),
                "Derived metric expression must be inlined: {sql}"
            );
        }

        #[test]
        fn resolve_joins_includes_transitive_deps_from_derived() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(DISTINCT o.id)".to_string(),
                        source_table: Some("o".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "avg_order_value".to_string(),
                        expr: "revenue / order_count".to_string(),
                        ..Default::default()
                    },
                ],
                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("avg_order_value")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"line_items\" AS \"li\""),
                "JOIN to li must be included for derived metric avg_order_value: {sql}"
            );
        }

        #[test]
        fn expand_derived_metric_with_facts_chain() {
            let def = SemanticViewDefinition {
                tables: vec![],
                dimensions: vec![],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(net_price)".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        ..Default::default()
                    },
                ],
                joins: vec![],
                facts: vec![Fact {
                    name: "net_price".to_string(),
                    expr: "extended_price * (1 - discount)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                }],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("profit")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains(
                    "(SUM((extended_price * (1 - discount)))) - (SUM(unit_cost)) AS \"profit\""
                ),
                "Fact->base->derived chain must resolve correctly: {sql}"
            );
        }
    }

    mod phase31_fan_trap_tests {
        use super::*;
        use crate::expand::test_helpers::minimal_def;
        use crate::model::{
            Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn fan_trap_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "li.status".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "segment".to_string(),
                        expr: "c.segment".to_string(),
                        source_table: Some("c".to_string()),
                        ..Default::default()
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.extended_price)".to_string(),
                        source_table: Some("li".to_string()),
                        ..Default::default()
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("o".to_string()),
                        ..Default::default()
                    },
                ],
                joins: vec![
                    Join {
                        table: "o".to_string(),
                        from_alias: "li".to_string(),
                        fk_columns: vec!["order_id".to_string()],
                        ref_columns: vec!["id".to_string()],
                        name: Some("li_to_order".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                    Join {
                        table: "c".to_string(),
                        from_alias: "o".to_string(),
                        fk_columns: vec!["customer_id".to_string()],
                        ref_columns: vec!["id".to_string()],
                        name: Some("order_to_customer".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                ],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        #[test]
        fn fan_trap_one_to_many_blocked() {
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("status")],
                metrics: vec![MetricName::new("order_count")],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Fan trap must block the query");
            match result.unwrap_err() {
                ExpandError::FanTrap { detail } => {
                    assert_eq!(detail.view_name, "sales");
                    assert_eq!(detail.metric_name, "order_count");
                    assert_eq!(detail.dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_many_to_one_safe() {
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("revenue")],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_ok(),
                "MANY TO ONE direction must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_one_to_one_safe() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "d".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "d.detail".to_string(),
                    source_table: Some("d".to_string()),
                    ..Default::default()
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],
                joins: vec![Join {
                    table: "d".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["detail_id".to_string()],
                    ref_columns: vec!["id".to_string()],
                    name: Some("order_to_detail".to_string()),
                    cardinality: Cardinality::OneToOne,
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("detail")],
                metrics: vec![MetricName::new("cnt")],
            };
            let result = expand("test", &def, &req);
            assert!(
                result.is_ok(),
                "ONE TO ONE must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_same_table_safe() {
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("status")],
                metrics: vec![MetricName::new("revenue")],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_ok(),
                "Same table must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_no_joins_safe() {
            let def = minimal_def("orders", "region", "region", "cnt", "COUNT(*)");
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("cnt")],
            };
            let result = expand("test", &def, &req);
            assert!(result.is_ok(), "No joins must be safe: {:?}", result.err());
        }

        #[test]
        fn fan_trap_transitive_chain() {
            let mut def = fan_trap_three_table_def();
            def.metrics.push(Metric {
                name: "customer_count".to_string(),
                expr: "COUNT(DISTINCT c.id)".to_string(),
                source_table: Some("c".to_string()),
                ..Default::default()
            });
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("status")],
                metrics: vec![MetricName::new("customer_count")],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_err(),
                "Transitive chain fan trap must be detected"
            );
            match result.unwrap_err() {
                ExpandError::FanTrap { detail } => {
                    assert_eq!(detail.metric_name, "customer_count");
                    assert_eq!(detail.dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_derived_metric_blocked() {
            let mut def = fan_trap_three_table_def();
            def.metrics.push(Metric {
                name: "avg_order".to_string(),
                expr: "order_count / 1".to_string(),
                ..Default::default()
            });
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("status")],
                metrics: vec![MetricName::new("avg_order")],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Derived metric fan trap must be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap { detail } => {
                    assert_eq!(detail.metric_name, "avg_order");
                    assert_eq!(detail.dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_error_message_format() {
            let err = ExpandError::FanTrap {
                detail: Box::new(FanTrapError {
                    view_name: "sales".to_string(),
                    metric_name: "order_count".to_string(),
                    metric_table: "o".to_string(),
                    dimension_name: "status".to_string(),
                    dimension_table: "li".to_string(),
                    relationship_name: "li_to_order".to_string(),
                }),
            };
            let msg = format!("{err}");
            assert!(msg.contains("sales"), "Must contain view name");
            assert!(msg.contains("order_count"), "Must contain metric name");
            assert!(msg.contains("status"), "Must contain dimension name");
            assert!(
                msg.contains("li_to_order"),
                "Must contain relationship name"
            );
            assert!(
                msg.contains("fan trap detected"),
                "Must contain 'fan trap detected'"
            );
            assert!(
                msg.contains("many-to-one cardinality"),
                "Must describe the cardinality direction"
            );
        }
    }

    mod phase32_role_playing_tests {
        use super::*;
        use crate::expand::test_helpers::TestFixtureExt;
        use crate::model::{
            Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn flights_airports_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "city".to_string(),
                        expr: "a.city".to_string(),
                        source_table: Some("a".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "country".to_string(),
                        expr: "a.country".to_string(),
                        source_table: Some("a".to_string()),
                        ..Default::default()
                    },
                    Dimension {
                        name: "carrier".to_string(),
                        expr: "f.carrier".to_string(),
                        source_table: Some("f".to_string()),
                        ..Default::default()
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "departure_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        using_relationships: vec!["dep_airport".to_string()],
                        ..Default::default()
                    },
                    Metric {
                        name: "arrival_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        using_relationships: vec!["arr_airport".to_string()],
                        ..Default::default()
                    },
                    Metric {
                        name: "total_flights".to_string(),
                        expr: "departure_count + arrival_count".to_string(),
                        ..Default::default()
                    },
                ],
                joins: vec![
                    Join {
                        table: "a".to_string(),
                        from_alias: "f".to_string(),
                        fk_columns: vec!["departure_code".to_string()],
                        ref_columns: vec!["airport_code".to_string()],
                        name: Some("dep_airport".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                    Join {
                        table: "a".to_string(),
                        from_alias: "f".to_string(),
                        fk_columns: vec!["arrival_code".to_string()],
                        ref_columns: vec!["airport_code".to_string()],
                        name: Some("arr_airport".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                ],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        #[test]
        fn using_metric_generates_scoped_join_alias() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![MetricName::new("departure_count")],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("a__dep_airport"),
                "Scoped alias a__dep_airport must appear: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "LEFT JOIN with scoped alias must appear: {sql}"
            );
        }

        #[test]
        fn two_using_metrics_generate_two_scoped_joins() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("carrier")],
                metrics: vec![
                    MetricName::new("departure_count"),
                    MetricName::new("arrival_count"),
                ],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "dep_airport scoped JOIN must appear: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
                "arr_airport scoped JOIN must appear: {sql}"
            );
        }

        #[test]
        fn dimension_rewritten_to_scoped_alias() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![MetricName::new("departure_count")],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("a__dep_airport.city"),
                "Dimension must be rewritten to scoped alias: {sql}"
            );
        }

        #[test]
        fn ambiguous_dimension_without_using_produces_error() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![],
            };
            let result = expand("test_flights", &def, &req);
            assert!(result.is_err(), "Ambiguous dimension must produce error");
            match result.unwrap_err() {
                ExpandError::AmbiguousPath {
                    view_name,
                    dimension_name,
                    dimension_table,
                    available_relationships,
                } => {
                    assert_eq!(view_name, "test_flights");
                    assert_eq!(dimension_name, "city");
                    assert_eq!(dimension_table, "a");
                    assert!(available_relationships.contains(&"dep_airport".to_string()));
                    assert!(available_relationships.contains(&"arr_airport".to_string()));
                }
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn ambiguous_path_error_lists_relationships() {
            let err = ExpandError::AmbiguousPath {
                view_name: "test_flights".to_string(),
                dimension_name: "city".to_string(),
                dimension_table: "a".to_string(),
                available_relationships: vec!["dep_airport".to_string(), "arr_airport".to_string()],
            };
            let msg = format!("{err}");
            assert!(msg.contains("test_flights"));
            assert!(msg.contains("city"));
            assert!(msg.contains("ambiguous"));
            assert!(msg.contains("dep_airport"));
            assert!(msg.contains("arr_airport"));
        }

        #[test]
        fn non_ambiguous_single_relationship_works_without_using() {
            let mut def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_table("o", "orders", &["id"])
                .with_table("c", "customers", &["id"])
                .with_dimension("customer_name", "c.name", Some("c"))
                .with_metric("revenue", "SUM(o.amount)", Some("o"));
            def.joins.push(Join {
                table: "c".to_string(),
                from_alias: "o".to_string(),
                fk_columns: vec!["customer_id".to_string()],
                name: Some("order_to_customer".to_string()),
                ..Default::default()
            });
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("revenue")],
            };
            let result = expand("test", &def, &req);
            assert!(
                result.is_ok(),
                "Single relationship must work without USING: {:?}",
                result.err()
            );
        }

        #[test]
        fn base_table_dimension_works_unchanged() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("carrier")],
                metrics: vec![MetricName::new("departure_count")],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("f.carrier AS \"carrier\""),
                "Base table dimension must appear unchanged: {sql}"
            );
        }

        #[test]
        fn fan_trap_detection_works_with_using_paths() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "carrier".to_string(),
                    expr: "f.carrier".to_string(),
                    source_table: Some("f".to_string()),
                    ..Default::default()
                }],
                metrics: vec![Metric {
                    name: "airport_count".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("a".to_string()),
                    ..Default::default()
                }],
                joins: vec![Join {
                    table: "a".to_string(),
                    from_alias: "f".to_string(),
                    fk_columns: vec!["dep_airport_code".to_string()],
                    ref_columns: vec!["airport_code".to_string()],
                    name: Some("dep_flights".to_string()),
                    cardinality: Cardinality::ManyToOne,
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("carrier")],
                metrics: vec![MetricName::new("airport_count")],
            };
            let result = expand("test", &def, &req);
            assert!(result.is_err(), "Fan trap must still be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap { .. } => {}
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn derived_metric_with_two_using_resolves_both_joins() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("carrier")],
                metrics: vec![MetricName::new("total_flights")],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "Derived metric must resolve dep_airport join: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
                "Derived metric must resolve arr_airport join: {sql}"
            );
        }

        #[test]
        fn metric_using_from_base_table_no_unnecessary_join() {
            let def = SemanticViewDefinition {
                tables: vec![TableRef {
                    alias: "o".to_string(),
                    table: "orders".to_string(),
                    pk_columns: vec!["id".to_string()],
                    ..Default::default()
                }],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],
                joins: vec![],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("cnt")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "No JOIN needed when everything is on base table: {sql}"
            );
        }

        #[test]
        fn backward_compat_no_using_expands_as_before() {
            let def = SemanticViewDefinition {
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        ..Default::default()
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    ..Default::default()
                }],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                }],
                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    name: Some("order_to_customer".to_string()),
                    ..Default::default()
                }],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            };
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("revenue")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"customers\" AS \"c\""),
                "Non-USING definition must use bare alias: {sql}"
            );
            assert!(
                sql.contains("c.name AS"),
                "Dimension expr must use bare alias: {sql}"
            );
        }

        #[test]
        fn ambiguous_dimension_with_derived_metric_using_both_paths() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![MetricName::new("total_flights")],
            };
            let result = expand("test_flights", &def, &req);
            assert!(
                result.is_err(),
                "City dimension must be ambiguous when derived metric uses both paths"
            );
            match result.unwrap_err() {
                ExpandError::AmbiguousPath { .. } => {}
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn scoped_join_on_clause_uses_correct_fk_pk() {
            let def = flights_airports_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![MetricName::new("departure_count")],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("\"f\".\"departure_code\" = \"a__dep_airport\".\"airport_code\""),
                "Scoped JOIN ON clause must use correct FK/PK: {sql}"
            );
        }
    }

    mod phase43_private_access_tests {
        use super::*;
        use crate::model::{AccessModifier, Dimension, Metric, SemanticViewDefinition};

        fn make_def_with_private_metric() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    ..Default::default()
                }],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "SUM(amount)".to_string(),
                        ..Default::default()
                    },
                    Metric {
                        name: "secret_cost".to_string(),
                        expr: "SUM(cost)".to_string(),
                        access: AccessModifier::Private,
                        ..Default::default()
                    },
                ],
                joins: vec![],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        fn make_def_with_private_and_derived() -> SemanticViewDefinition {
            SemanticViewDefinition {
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    ..Default::default()
                }],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "SUM(amount)".to_string(),
                        ..Default::default()
                    },
                    Metric {
                        name: "secret_cost".to_string(),
                        expr: "SUM(cost)".to_string(),
                        access: AccessModifier::Private,
                        ..Default::default()
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "total_revenue - secret_cost".to_string(),
                        // no source_table: derived metric
                        ..Default::default()
                    },
                ],
                joins: vec![],
                facts: vec![],
                materializations: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        #[test]
        fn private_metric_rejected() {
            let def = make_def_with_private_metric();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("secret_cost")],
            };
            match expand("test_view", &def, &req) {
                Err(ExpandError::PrivateMetric { name, .. }) => {
                    assert_eq!(name, "secret_cost");
                }
                other => panic!("Expected PrivateMetric error, got: {:?}", other),
            }
        }

        #[test]
        fn private_metric_error_message_contains_private() {
            let def = make_def_with_private_metric();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("secret_cost")],
            };
            let err = expand("test_view", &def, &req).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("private"),
                "Error message should contain 'private': {msg}"
            );
            assert!(
                msg.contains("secret_cost"),
                "Error message should contain metric name: {msg}"
            );
        }

        #[test]
        fn public_metric_still_works() {
            let def = make_def_with_private_metric();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("test_view", &def, &req).unwrap();
            assert!(
                sql.contains("total_revenue"),
                "SQL should contain public metric"
            );
        }

        #[test]
        fn derived_metric_referencing_private_base_works() {
            let def = make_def_with_private_and_derived();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("profit")],
            };
            let sql = expand("test_view", &def, &req).unwrap();
            assert!(sql.contains("profit"), "SQL should contain profit metric");
            // The derived metric expression should be inlined:
            // profit = total_revenue - secret_cost = SUM(amount) - SUM(cost)
            assert!(
                sql.contains("SUM(amount)"),
                "Derived metric should inline base expressions"
            );
            assert!(
                sql.contains("SUM(cost)"),
                "Derived metric should inline private base expression"
            );
        }
    }

    mod phase46_facts_awareness_tests {
        use super::*;
        use crate::expand::test_helpers::{orders_view, TestFixtureExt};

        #[test]
        fn test_facts_metrics_mutual_exclusion() {
            let def = orders_view().with_fact("line_total", "quantity * price", "orders");
            let req = QueryRequest {
                facts: vec!["line_total".to_string()],
                dimensions: vec![],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let result = expand("test_view", &def, &req);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(err, ExpandError::FactsMetricsMutualExclusion { .. }),
                "Expected FactsMetricsMutualExclusion, got: {err}"
            );
            let msg = err.to_string();
            assert!(
                msg.contains("cannot combine facts and metrics"),
                "Error message should contain 'cannot combine facts and metrics', got: {msg}"
            );
        }

        #[test]
        fn test_empty_request_with_facts_is_not_empty() {
            let def = orders_view().with_fact("line_total", "quantity * price", "orders");
            let req = QueryRequest {
                facts: vec!["line_total".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("test_view", &def, &req);
            // The expand should NOT return EmptyRequest. It may return another error
            // since fact expansion is not yet implemented — the test verifies the
            // guard condition only.
            assert!(
                !matches!(result, Err(ExpandError::EmptyRequest { .. })),
                "facts-only request should not be treated as empty"
            );
        }

        #[test]
        fn test_unknown_fact_display() {
            let err = ExpandError::UnknownFact {
                view_name: "v".to_string(),
                name: "bad_fact".to_string(),
                available: vec!["f1".to_string(), "f2".to_string()],
                suggestion: Some("f1".to_string()),
            };
            let msg = err.to_string();
            assert!(
                msg.contains("unknown fact"),
                "Should contain 'unknown fact': {msg}"
            );
            assert!(msg.contains("f1, f2"), "Should list available facts: {msg}");
            assert!(msg.contains("Did you mean"), "Should suggest: {msg}");
        }

        #[test]
        fn test_duplicate_fact_display() {
            let err = ExpandError::DuplicateFact {
                view_name: "v".to_string(),
                name: "f1".to_string(),
            };
            let msg = err.to_string();
            assert!(
                msg.contains("duplicate fact"),
                "Should contain 'duplicate fact': {msg}"
            );
        }

        #[test]
        fn test_fact_path_violation_display() {
            let err = ExpandError::FactPathViolation {
                view_name: "v".to_string(),
                table_a: "orders".to_string(),
                table_b: "products".to_string(),
            };
            let msg = err.to_string();
            assert!(
                msg.contains("fact query references"),
                "Should contain 'fact query references': {msg}"
            );
            assert!(msg.contains("orders"), "Should contain table_a: {msg}");
            assert!(msg.contains("products"), "Should contain table_b: {msg}");
        }
    }

    mod phase46_fact_query_tests {
        use super::*;
        use crate::expand::test_helpers::TestFixtureExt;
        use crate::model::SemanticViewDefinition;

        /// Build a multi-table def: orders (o) -> line_items (li), with a dim on o and facts on li.
        fn multi_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_table("o", "orders", &["id"])
                .with_table("li", "line_items", &["id"])
                .with_dimension("region", "o.region", Some("o"))
                .with_fact("net_price", "li.price * (1 - li.discount)", "li")
                .with_metric("total_revenue", "sum(li.price)", Some("li"))
                .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
        }

        #[test]
        fn test_fact_query_basic() {
            let def = multi_table_def();
            let req = QueryRequest {
                facts: vec!["net_price".to_string()],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![],
            };
            let sql = expand("test_view", &def, &req).unwrap();
            assert!(
                !sql.contains("GROUP BY"),
                "Fact queries must NOT have GROUP BY: {sql}"
            );
            assert!(sql.contains("o.region"), "Must include dim expr: {sql}");
            assert!(
                sql.contains("li.price * (1 - li.discount)"),
                "Must include fact expr: {sql}"
            );
            assert!(sql.contains("FROM"), "Must have FROM clause: {sql}");
            assert!(sql.contains("LEFT JOIN"), "Must include JOIN for li: {sql}");
        }

        #[test]
        fn test_fact_query_no_dimensions() {
            let def = multi_table_def();
            let req = QueryRequest {
                facts: vec!["net_price".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let sql = expand("test_view", &def, &req).unwrap();
            assert!(
                !sql.contains("GROUP BY"),
                "Fact queries must NOT have GROUP BY: {sql}"
            );
            assert!(
                sql.contains("li.price * (1 - li.discount)"),
                "Must include fact expr: {sql}"
            );
            assert!(
                !sql.contains("DISTINCT"),
                "Fact queries without dims should not use DISTINCT: {sql}"
            );
        }

        #[test]
        fn test_fact_query_inline_facts() {
            let def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_table("o", "orders", &["id"])
                .with_table("li", "line_items", &["id"])
                .with_fact("net_price", "li.price * (1 - li.discount)", "li")
                .with_fact("line_total", "net_price * li.quantity", "li")
                .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec!["line_total".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let sql = expand("test_view", &def, &req).unwrap();
            // line_total's expression should have net_price inlined (parenthesized)
            assert!(
                sql.contains("(li.price * (1 - li.discount))"),
                "Must inline net_price into line_total: {sql}"
            );
        }

        #[test]
        fn test_fact_query_unknown_fact() {
            let def = multi_table_def();
            let req = QueryRequest {
                facts: vec!["nonexistent".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("test_view", &def, &req);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(err, ExpandError::UnknownFact { .. }),
                "Expected UnknownFact, got: {err}"
            );
        }

        #[test]
        fn test_fact_query_duplicate_fact() {
            let def = multi_table_def();
            let req = QueryRequest {
                facts: vec!["net_price".to_string(), "net_price".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("test_view", &def, &req);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(err, ExpandError::DuplicateFact { .. }),
                "Expected DuplicateFact, got: {err}"
            );
        }

        #[test]
        fn test_fact_query_private_fact() {
            let def = multi_table_def().with_private_fact("raw_price", "li.price", "li");
            let req = QueryRequest {
                facts: vec!["raw_price".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("test_view", &def, &req);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(err, ExpandError::PrivateFact { .. }),
                "Expected PrivateFact, got: {err}"
            );
        }

        #[test]
        fn test_fact_path_violation() {
            // Fan shape: o -> li, o -> payments (divergent paths)
            let def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_table("o", "orders", &["id"])
                .with_table("li", "line_items", &["id"])
                .with_table("p", "payments", &["id"])
                .with_fact("net_price", "li.price * (1 - li.discount)", "li")
                .with_dimension("pay_status", "CAST(p.amount AS VARCHAR)", Some("p"))
                .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
                .with_pkfk_join("p_to_o", "p", "o", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec!["net_price".to_string()],
                dimensions: vec![DimensionName::new("pay_status")],
                metrics: vec![],
            };
            let result = expand("test_view", &def, &req);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(err, ExpandError::FactPathViolation { .. }),
                "Expected FactPathViolation, got: {err}"
            );
        }

        #[test]
        fn test_fact_path_valid_linear() {
            // Chain: o -> li -> details (linear path)
            let def = SemanticViewDefinition::default()
                .with_table("orders", "orders", &[])
                .with_table("o", "orders", &["id"])
                .with_table("li", "line_items", &["id"])
                .with_table("d", "details", &["id"])
                .with_fact("detail_val", "d.value", "d")
                .with_dimension("region", "o.region", Some("o"))
                .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
                .with_pkfk_join("d_to_li", "d", "li", &["line_id"], &["id"]);
            let req = QueryRequest {
                facts: vec!["detail_val".to_string()],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![],
            };
            let result = expand("test_view", &def, &req);
            assert!(result.is_ok(), "Linear path should be valid: {result:?}");
        }

        #[test]
        fn test_fact_query_with_output_type() {
            let mut def = multi_table_def();
            def.facts[0].output_type = Some("DECIMAL(10,2)".to_string());
            let req = QueryRequest {
                facts: vec!["net_price".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let sql = expand("test_view", &def, &req).unwrap();
            assert!(
                sql.contains("CAST("),
                "Must wrap fact in CAST when output_type is set: {sql}"
            );
            assert!(
                sql.contains("DECIMAL(10,2)"),
                "Must include output type: {sql}"
            );
        }
    }

    /// Regression tests for the join-emission overhaul (code review 2026-07-02,
    /// findings SG-2, SG-10, SG-12): the resolver returns structured edges, so
    /// the join emitted for an alias is always the edge connecting it to an
    /// already-emitted table, independent of relationship declaration order,
    /// and scoped role-playing aliases are never re-parsed from strings.
    mod join_emission_regression_tests {
        use super::*;
        use crate::expand::test_helpers::TestFixtureExt;
        use crate::model::SemanticViewDefinition;

        /// li (base) -> o -> c chain with configurable relationship
        /// declaration order.
        fn li_o_c_def(o_to_c_first: bool) -> SemanticViewDefinition {
            let def = SemanticViewDefinition::default()
                .with_table("li", "line_items", &["id"])
                .with_table("o", "orders", &["id"])
                .with_table("c", "customers", &["id"])
                .with_dimension("customer_name", "c.name", Some("c"))
                .with_metric("total_qty", "sum(li.qty)", Some("li"));
            if o_to_c_first {
                def.with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
                    .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
            } else {
                def.with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
                    .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
            }
        }

        /// SG-2: the join emitted for alias `o` must be the edge that connects
        /// `o` to the already-emitted base (li -> o), not whichever declared
        /// relationship mentions `o` first (o -> c would forward-reference the
        /// not-yet-joined `c`). Both declaration orders must produce identical
        /// SQL with no forward references and no dropped joins.
        #[test]
        fn sg2_join_selection_is_declaration_order_independent() {
            let expected = "\
SELECT
    c.name AS \"customer_name\",
    sum(li.qty) AS \"total_qty\"
FROM \"line_items\" AS \"li\"
LEFT JOIN \"orders\" AS \"o\" ON \"li\".\"order_id\" = \"o\".\"id\"
LEFT JOIN \"customers\" AS \"c\" ON \"o\".\"customer_id\" = \"c\".\"id\"
GROUP BY
    1";
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_name")],
                metrics: vec![MetricName::new("total_qty")],
            };
            let sql_a = expand("test", &li_o_c_def(true), &req).unwrap();
            let sql_b = expand("test", &li_o_c_def(false), &req).unwrap();
            assert_eq!(sql_a, expected, "o->c declared first must be correct");
            assert_eq!(sql_b, expected, "li->o declared first must be correct");
        }

        /// SG-2: a child table (`li`) with FKs to two parents (`p` declared
        /// first, then `o` = base). Query needing only the li -> o edge must
        /// emit it — not the first-declared li -> p edge, whose ON clause
        /// would reference the never-joined `p`.
        #[test]
        fn sg2_two_parent_child_picks_connecting_edge() {
            let def = SemanticViewDefinition::default()
                .with_table("o", "orders", &["id"])
                .with_table("li", "line_items", &["id"])
                .with_table("p", "products", &["id"])
                .with_dimension("region", "o.region", Some("o"))
                .with_metric("qty", "sum(li.qty)", Some("li"))
                .with_pkfk_join("li_to_p", "li", "p", &["product_id"], &["id"])
                .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("qty")],
            };
            let sql = expand("test", &def, &req).unwrap();
            let expected = "\
SELECT
    o.region AS \"region\",
    sum(li.qty) AS \"qty\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"o\".\"id\"
GROUP BY
    1";
            assert_eq!(sql, expected, "must join li via li->o, not li->p");
            assert!(
                !sql.contains("\"p\"."),
                "ON clause must not reference the never-joined p: {sql}"
            );
        }

        /// ld -> li -> o (base) chain: metric two hops below the root.
        fn ld_li_o_def() -> SemanticViewDefinition {
            SemanticViewDefinition::default()
                .with_table("o", "orders", &["id"])
                .with_table("li", "line_items", &["id"])
                .with_table("ld", "line_item_details", &["id"])
                .with_dimension("region", "o.region", Some("o"))
                .with_metric("detail_qty", "sum(ld.qty)", Some("ld"))
                .with_fact("detail_amount", "ld.amount", "ld")
                .with_pkfk_join("ld_to_li", "ld", "li", &["line_item_id"], &["id"])
                .with_pkfk_join("li_to_o", "li", "o", &["order_id"], &["id"])
        }

        /// SG-10: a needed table two hops below the root must pull in its
        /// intermediate (`li`) and join in dependency order (li before ld),
        /// with each ON clause referencing only already-joined tables.
        #[test]
        fn sg10_fk_side_chain_includes_intermediate_join() {
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("detail_qty")],
            };
            let sql = expand("test", &ld_li_o_def(), &req).unwrap();
            let expected = "\
SELECT
    o.region AS \"region\",
    sum(ld.qty) AS \"detail_qty\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"o\".\"id\"
LEFT JOIN \"line_item_details\" AS \"ld\" ON \"ld\".\"line_item_id\" = \"li\".\"id\"
GROUP BY
    1";
            assert_eq!(sql, expected);

            // Reversed declaration order must produce the same SQL.
            let mut def_rev = ld_li_o_def();
            def_rev.joins.reverse();
            let sql_rev = expand("test", &def_rev, &req).unwrap();
            assert_eq!(sql_rev, expected, "declaration order must not matter");
        }

        /// SG-10 (facts path): `expand_facts` previously joined only the
        /// fact's direct source table with no path walk; the intermediate
        /// `li` join was missing entirely.
        #[test]
        fn sg10_fact_source_chain_includes_intermediate_join() {
            let req = QueryRequest {
                facts: vec!["detail_amount".to_string()],
                dimensions: vec![],
                metrics: vec![],
            };
            let sql = expand("test", &ld_li_o_def(), &req).unwrap();
            let expected = "\
SELECT
    ld.amount AS \"detail_amount\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"o\".\"id\"
LEFT JOIN \"line_item_details\" AS \"ld\" ON \"ld\".\"line_item_id\" = \"li\".\"id\"";
            assert_eq!(sql, expected);
        }

        /// SG-12: a user table alias containing `__` is a bare alias, not a
        /// role-playing scoped alias. It must be joined normally — previously
        /// the emitter re-parsed the alias at the first `__`, looked up a
        /// relationship named after the suffix, and silently dropped the join.
        #[test]
        fn sg12_bare_alias_containing_double_underscore_joins_normally() {
            let def = SemanticViewDefinition::default()
                .with_table("o", "orders", &["id"])
                .with_table("my__dim", "dim_table", &["id"])
                .with_dimension("dim_name", "my__dim.name", Some("my__dim"))
                .with_metric("cnt", "count(*)", Some("o"))
                .with_pkfk_join("o_to_dim", "o", "my__dim", &["dim_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("dim_name")],
                metrics: vec![MetricName::new("cnt")],
            };
            let sql = expand("test", &def, &req).unwrap();
            let expected = "\
SELECT
    my__dim.name AS \"dim_name\",
    count(*) AS \"cnt\"
FROM \"orders\" AS \"o\"
LEFT JOIN \"dim_table\" AS \"my__dim\" ON \"o\".\"dim_id\" = \"my__dim\".\"id\"
GROUP BY
    1";
            assert_eq!(sql, expected);
        }

        /// SG-12: role-playing scoped aliases keep the documented
        /// `{alias}__{relationship}` SQL alias format, and the scoped alias is
        /// used on the PK side of the ON clause.
        #[test]
        fn sg12_role_playing_scoped_alias_format_preserved() {
            let def = SemanticViewDefinition::default()
                .with_table("f", "flights", &["flight_id"])
                .with_table("a", "airports", &["airport_code"])
                .with_dimension("city", "a.city", Some("a"))
                .with_metric("departure_count", "COUNT(*)", Some("f"))
                .with_using_relationship("departure_count", &["dep_airport"])
                .with_pkfk_join(
                    "dep_airport",
                    "f",
                    "a",
                    &["departure_code"],
                    &["airport_code"],
                )
                .with_pkfk_join(
                    "arr_airport",
                    "f",
                    "a",
                    &["arrival_code"],
                    &["airport_code"],
                );
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![MetricName::new("departure_count")],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "Scoped alias format {{alias}}__{{rel}} must be preserved: {sql}"
            );
            assert!(
                sql.contains("\"f\".\"departure_code\" = \"a__dep_airport\".\"airport_code\""),
                "ON clause must use the scoped alias on the PK side: {sql}"
            );
        }
    }

    // ===================================================================
    // Code review 2026-07-02 remediation: SG-8 (COUNT(*) on non-base
    // tables), SG-14 (qualified-name resolution), SG-17 (facts-path
    // role-playing ambiguity).
    // ===================================================================

    mod count_star_rewrite_tests {
        use super::*;
        use crate::expand::test_helpers::{orders_view, TestFixtureExt};
        use crate::model::WindowSpec;

        /// `orders` (base) + `line_items` child with a declared PK.
        fn child_count_def() -> crate::model::SemanticViewDefinition {
            orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_dimension("region", "region", None)
                .with_table("li", "line_items", &["id"])
                .with_metric("item_count", "COUNT(*)", Some("li"))
                .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"])
        }

        #[test]
        fn test_child_count_star_rewritten_exact_sql() {
            // SG-8: COUNT(*) on the LEFT-JOINed child must count the child's
            // PK, not NULL-extended rows (one per childless order).
            let def = child_count_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("item_count")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    COUNT(\"li\".\"id\") AS \"item_count\"
FROM \"orders\" AS \"orders\"
LEFT JOIN \"line_items\" AS \"li\" ON \"li\".\"order_id\" = \"orders\".\"id\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_child_count_star_rewritten_with_base_dimension() {
            let def = child_count_def();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("item_count")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("COUNT(\"li\".\"id\") AS \"item_count\""),
                "child COUNT(*) must be rewritten to COUNT(pk): {sql}"
            );
            assert!(sql.contains("GROUP BY"), "grouped query expected: {sql}");
        }

        #[test]
        fn test_base_table_count_star_unchanged() {
            // Metrics on the base table keep plain COUNT(*): the base table
            // is never NULL-extended by the synthesized LEFT JOINs.
            let def = child_count_def().with_metric("order_count", "COUNT(*)", Some("orders"));
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("order_count")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    COUNT(*) AS \"order_count\"
FROM \"orders\" AS \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_unqualified_count_star_metric_unchanged() {
            // Legacy single-table shape: metric declared without a source
            // table (None) is a base-table/derived metric — no rewrite.
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("order_count")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("count(*) AS \"order_count\""),
                "COUNT(*) without a non-base source table must be preserved: {sql}"
            );
        }

        #[test]
        fn test_child_count_star_without_pk_errors() {
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("li", "line_items", &[]) // no PK declared
                .with_metric("item_count", "COUNT(*)", Some("li"))
                .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("item_count")],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match &err {
                ExpandError::CountStarRequiresPrimaryKey {
                    view_name,
                    metric_name,
                    table_alias,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(metric_name, "item_count");
                    assert_eq!(table_alias, "li");
                }
                other => panic!("Expected CountStarRequiresPrimaryKey, got: {other}"),
            }
            let msg = err.to_string();
            assert!(
                msg.contains("no PRIMARY KEY declared") && msg.contains("COUNT(*)"),
                "error must explain the rewrite requirement: {msg}"
            );
        }

        #[test]
        fn test_unrelated_metric_still_works_when_sibling_count_star_lacks_pk() {
            // The no-PK failure is scoped to queries that actually use the
            // metric: other metrics on the same view keep working.
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("li", "line_items", &[]) // no PK declared
                .with_metric("item_count", "COUNT(*)", Some("li"))
                .with_metric("revenue", "SUM(li.amount)", Some("li"))
                .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.contains("SUM(li.amount)"), "SQL: {sql}");
        }

        #[test]
        fn test_derived_metric_reaching_no_pk_count_star_errors() {
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("li", "line_items", &[]) // no PK declared
                .with_metric("item_count", "COUNT(*)", Some("li"))
                .with_metric("double_items", "item_count * 2", None)
                .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("double_items")],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match &err {
                ExpandError::CountStarRequiresPrimaryKey { metric_name, .. } => {
                    assert_eq!(
                        metric_name, "item_count",
                        "error must name the failing base metric"
                    );
                }
                other => panic!("Expected CountStarRequiresPrimaryKey, got: {other}"),
            }
        }

        #[test]
        fn test_derived_metric_inherits_rewritten_count_star() {
            let def = child_count_def().with_metric("double_items", "item_count * 2", None);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("double_items")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("(COUNT(\"li\".\"id\")) * 2 AS \"double_items\""),
                "derived metric must inline the REWRITTEN child count: {sql}"
            );
        }

        #[test]
        fn test_window_inner_aggregate_gets_rewrite() {
            // Window path: the inner aggregate is emitted from the shared
            // resolved expressions, so the rewrite must appear in the CTE.
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("li", "line_items", &["id"])
                .with_dimension("product", "li.product", Some("li"))
                .with_metric("item_count", "COUNT(*)", Some("li"))
                .with_metric("rolling_items", "AVG(item_count)", None)
                .with_window_spec(
                    "rolling_items",
                    WindowSpec {
                        window_function: "AVG".to_string(),
                        inner_metric: "item_count".to_string(),
                        ..Default::default()
                    },
                )
                .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("product")],
                metrics: vec![MetricName::new("rolling_items")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("COUNT(\"li\".\"id\") AS \"item_count\""),
                "window CTE inner aggregate must use the rewritten count: {sql}"
            );
        }

        #[test]
        fn test_semi_additive_co_query_uses_rewritten_count() {
            // Semi-additive path: a same-grain COUNT(*) co-metric on the
            // child table decomposes into a CTE capture of the child PK and
            // an outer COUNT over it (NULL-extended rows excluded). The
            // base-table COUNT(*) rejection in parse_snapshot_aggregate is
            // untouched (covered by semi_additive tests).
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_dimension("customer_id", "customer_id", None)
                .with_table("li", "line_items", &["id"])
                .with_dimension("report_date", "li.report_date", Some("li"))
                .with_metric("balance", "SUM(li.balance)", Some("li"))
                .with_metric("txn_count", "COUNT(*)", Some("li"))
                .with_pkfk_join("li_orders", "li", "orders", &["order_id"], &["id"])
                .with_non_additive_by(
                    "balance",
                    &[(
                        "report_date",
                        crate::model::SortOrder::Desc,
                        crate::model::NullsOrder::First,
                    )],
                );
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_id")],
                metrics: vec![MetricName::new("balance"), MetricName::new("txn_count")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("\"li\".\"id\" AS \"__sv_reg_1\""),
                "CTE must capture the rewritten count argument: {sql}"
            );
            assert!(
                sql.contains("COUNT(\"__sv_reg_1\") AS \"txn_count\""),
                "outer select must re-aggregate the captured PK column: {sql}"
            );
        }
    }

    mod qualified_name_resolution_tests {
        use super::*;
        use crate::expand::test_helpers::{orders_view, TestFixtureExt};

        #[test]
        fn test_qualified_dimension_wrong_table_errors() {
            // SG-14: no fallback to "any dimension with that bare name".
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("x.region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match err {
                ExpandError::UnknownDimension { name, .. } => {
                    assert_eq!(name, "x.region");
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_qualified_metric_wrong_table_errors() {
            let def = orders_view().clear_metrics().with_metric(
                "total_revenue",
                "sum(amount)",
                Some("orders"),
            );
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![MetricName::new("x.total_revenue")],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match err {
                ExpandError::UnknownMetric { name, .. } => {
                    assert_eq!(name, "x.total_revenue");
                }
                other => panic!("Expected UnknownMetric, got: {other}"),
            }
        }

        #[test]
        fn test_base_alias_qualification_matches_unqualified_declaration() {
            // A dimension declared without a source table is a base-table
            // item; qualifying the request with the base alias must resolve.
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("orders.region")],
                metrics: vec![MetricName::new("total_revenue")],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.contains("region AS \"region\""), "SQL: {sql}");
        }

        #[test]
        fn test_bare_and_qualified_same_dimension_rejected_as_duplicate() {
            // SG-14: the duplicate check keys on the RESOLVED item, so
            // `region` and `orders.region` cannot emit the column twice.
            let def = orders_view();
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![
                    DimensionName::new("region"),
                    DimensionName::new("orders.region"),
                ],
                metrics: vec![],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match err {
                ExpandError::DuplicateDimension { name, .. } => {
                    assert_eq!(name, "orders.region");
                }
                other => panic!("Expected DuplicateDimension, got: {other}"),
            }
        }

        #[test]
        fn test_bare_and_qualified_same_metric_rejected_as_duplicate() {
            let def = orders_view().clear_metrics().with_metric(
                "total_revenue",
                "sum(amount)",
                Some("orders"),
            );
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![],
                metrics: vec![
                    MetricName::new("total_revenue"),
                    MetricName::new("orders.total_revenue"),
                ],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match err {
                ExpandError::DuplicateMetric { name, .. } => {
                    assert_eq!(name, "orders.total_revenue");
                }
                other => panic!("Expected DuplicateMetric, got: {other}"),
            }
        }
    }

    mod facts_path_role_playing_tests {
        use super::*;
        use crate::expand::test_helpers::{orders_view, TestFixtureExt};

        fn role_playing_facts_def(two_rels: bool) -> crate::model::SemanticViewDefinition {
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("a", "airports", &["code"])
                .with_dimension("city", "a.city", Some("a"))
                .with_fact("order_note", "orders.note", "orders")
                .with_pkfk_join("dep_airport", "orders", "a", &["dep_code"], &["code"]);
            if two_rels {
                def.with_pkfk_join("arr_airport", "orders", "a", &["arr_code"], &["code"])
            } else {
                def
            }
        }

        #[test]
        fn test_facts_path_role_playing_dimension_raises_ambiguous_path() {
            // SG-17: the facts path must run the same role-playing ambiguity
            // detection as the metrics path. With two named relationships to
            // `a` and no USING context (facts cannot supply one), the
            // dimension is ambiguous — previously it silently bound to an
            // arbitrary edge.
            let def = role_playing_facts_def(true);
            let req = QueryRequest {
                facts: vec!["order_note".to_string()],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![],
            };
            let err = expand("orders", &def, &req).unwrap_err();
            match err {
                ExpandError::AmbiguousPath {
                    dimension_name,
                    dimension_table,
                    available_relationships,
                    ..
                } => {
                    assert_eq!(dimension_name, "city");
                    assert_eq!(dimension_table, "a");
                    assert!(
                        available_relationships.contains(&"dep_airport".to_string())
                            && available_relationships.contains(&"arr_airport".to_string()),
                        "both relationships must be listed: {available_relationships:?}"
                    );
                }
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn test_facts_path_single_relationship_dimension_ok() {
            let def = role_playing_facts_def(false);
            let req = QueryRequest {
                facts: vec!["order_note".to_string()],
                dimensions: vec![DimensionName::new("city")],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a\""),
                "single relationship stays unambiguous: {sql}"
            );
        }

        #[test]
        fn test_facts_path_convergent_parent_dimension_not_ambiguous() {
            // Two relationships converging on the same target from DIFFERENT
            // source tables (`li -> orders`, `pay -> orders`) is NOT
            // role-playing: the parent joins as one bare instance and the
            // path walk picks the unique connecting edge. The SG-17 check
            // over-fired here (it counted inbound relationships without
            // grouping by source), breaking plain child-fact +
            // parent-dimension queries — the regression surfaced in
            // test/sql/phase46_fact_query.test (p46f_fan_test).
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("li", "line_items", &["id"])
                .with_table("pay", "payments", &["id"])
                .with_dimension("region", "orders.region", Some("orders"))
                .with_fact("net_price", "li.price", "li")
                .with_pkfk_join("li_to_o", "li", "orders", &["order_id"], &["id"])
                .with_pkfk_join("pay_to_o", "pay", "orders", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec!["net_price".to_string()],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req)
                .expect("convergent parent must not raise AmbiguousPath");
            assert!(sql.contains("net_price"), "fact survives: {sql}");
        }

        #[test]
        fn test_metrics_path_convergent_parent_dimension_not_ambiguous() {
            // Same shape through the metrics path (find_using_context is
            // shared): a metric on one child + a dimension on the shared
            // parent, no USING context anywhere.
            let def = orders_view()
                .clear_dimensions()
                .clear_metrics()
                .with_table("li", "line_items", &["id"])
                .with_table("pay", "payments", &["id"])
                .with_dimension("region", "orders.region", Some("orders"))
                .with_metric("revenue", "SUM(li.price)", Some("li"))
                .with_pkfk_join("li_to_o", "li", "orders", &["order_id"], &["id"])
                .with_pkfk_join("pay_to_o", "pay", "orders", &["order_id"], &["id"]);
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("region")],
                metrics: vec![MetricName::new("revenue")],
            };
            let sql = expand("orders", &def, &req)
                .expect("convergent parent must not raise AmbiguousPath");
            assert!(sql.contains("SUM"), "metric survives: {sql}");
        }
    }
}
