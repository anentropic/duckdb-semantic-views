use crate::model::{AccessModifier, SemanticViewDefinition};
use crate::util::{replace_word_boundary, suggest_closest};

use super::facts::{inline_derived_metrics, inline_facts, toposort_facts};
use super::fan_trap::{check_fan_traps, validate_fact_table_path};
use super::join_resolver::{resolve_joins_pkfk, synthesize_on_clause, synthesize_on_clause_scoped};
use super::resolution::{find_dimension, find_metric, quote_ident, quote_table_ref};
use super::role_playing::find_using_context;
use super::types::{ExpandError, QueryRequest};

/// Resolve a list of names against a definition, checking for duplicates,
/// unknown names, and optional access restrictions.
///
/// Generic helper that deduplicates the resolution pattern used for
/// dimensions, metrics, and facts.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn resolve_names<'a, T, N: AsRef<str>>(
    names: &[N],
    view_name: &str,
    find_fn: impl Fn(&str) -> Option<&'a T>,
    is_private: impl Fn(&T) -> bool,
    available_fn: impl Fn() -> Vec<String>,
    suggest_fn: impl Fn(&str) -> Option<String>,
    make_dup_err: impl Fn(String, String) -> ExpandError,
    make_not_found_err: impl Fn(String, String, Vec<String>, Option<String>) -> ExpandError,
    make_private_err: impl Fn(String, String) -> ExpandError,
) -> Result<Vec<&'a T>, ExpandError> {
    let mut resolved = Vec::with_capacity(names.len());
    let mut seen = std::collections::HashSet::new();
    for name in names {
        let name_str = name.as_ref();
        if !seen.insert(name_str.to_ascii_lowercase()) {
            return Err(make_dup_err(view_name.to_string(), name_str.to_string()));
        }
        let item = find_fn(name_str).ok_or_else(|| {
            let avail = available_fn();
            let suggestion = suggest_fn(name_str);
            make_not_found_err(
                view_name.to_string(),
                name_str.to_string(),
                avail,
                suggestion,
            )
        })?;
        if is_private(item) {
            return Err(make_private_err(
                view_name.to_string(),
                name_str.to_string(),
            ));
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
#[allow(clippy::too_many_lines, clippy::result_large_err)]
fn expand_facts(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate + resolve requested facts.
    let resolved_facts = resolve_names(
        &req.facts,
        view_name,
        |name| def.facts.iter().find(|f| f.name.eq_ignore_ascii_case(name)),
        |fact| fact.access == AccessModifier::Private,
        || def.facts.iter().map(|f| f.name.clone()).collect(),
        |name| {
            suggest_closest(
                name,
                &def.facts.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            )
        },
        |vn, n| ExpandError::DuplicateFact {
            view_name: vn,
            name: n,
        },
        |vn, n, avail, sug| ExpandError::UnknownFact {
            view_name: vn,
            name: n,
            available: avail,
            suggestion: sug,
        },
        |vn, n| ExpandError::PrivateFact {
            view_name: vn,
            name: n,
        },
    )?;

    // 2. Resolve requested dimensions (same logic as expand()).
    let resolved_dims = resolve_names(
        &req.dimensions,
        view_name,
        |name| find_dimension(def, name),
        |_dim| false,
        || def.dimensions.iter().map(|d| d.name.clone()).collect(),
        |name| {
            suggest_closest(
                name,
                &def.dimensions
                    .iter()
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>(),
            )
        },
        |vn, n| ExpandError::DuplicateDimension {
            view_name: vn,
            name: n,
        },
        |vn, n, avail, sug| ExpandError::UnknownDimension {
            view_name: vn,
            name: n,
            available: avail,
            suggestion: sug,
        },
        |vn, n| ExpandError::DuplicateDimension {
            view_name: vn,
            name: n,
        },
    )?;

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

    // 4. Resolve fact expressions via DAG inlining (fact-to-fact dependencies).
    let topo_order = toposort_facts(&def.facts).map_err(|e| ExpandError::CycleDetected {
        view_name: view_name.to_string(),
        cycle_description: e,
    })?;

    // 5. Build SELECT clause (no DISTINCT, no aggregation).
    let mut sql = String::with_capacity(256);
    sql.push_str("SELECT\n");

    let mut select_items: Vec<String> = Vec::new();

    // Dimensions first
    for dim in &resolved_dims {
        let base_expr = dim.expr.clone();
        let final_expr = if let Some(ref type_str) = dim.output_type {
            format!("CAST({base_expr} AS {type_str})")
        } else {
            base_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&dim.name)));
    }

    // Then facts (inlined expressions, no aggregation)
    for fact in &resolved_facts {
        let resolved_expr = inline_facts(&fact.expr, &def.facts, &topo_order);
        let final_expr = if let Some(ref type_str) = fact.output_type {
            format!("CAST({resolved_expr} AS {type_str})")
        } else {
            resolved_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&fact.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 6. FROM clause — same pattern as expand().
    sql.push_str("\nFROM ");
    sql.push_str(&quote_table_ref(&def.base_table));
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // 7. JOIN clauses — resolve required joins for fact + dim tables.
    // Build temporary Metric slice as empty (no metrics in fact queries)
    let empty_mets: Vec<&crate::model::Metric> = vec![];
    let ordered_aliases = resolve_joins_pkfk(def, &resolved_dims, &empty_mets);

    // Also ensure fact source tables are included in join resolution.
    let mut fact_aliases: Vec<String> = Vec::new();
    for fact in &resolved_facts {
        if let Some(ref st) = fact.source_table {
            let lower = st.to_ascii_lowercase();
            if !ordered_aliases
                .iter()
                .any(|a| a.to_ascii_lowercase() == lower)
                && !fact_aliases.contains(&lower)
                && def
                    .tables
                    .first()
                    .is_none_or(|t| t.alias.to_ascii_lowercase() != lower)
            {
                fact_aliases.push(lower);
            }
        }
    }

    // Emit joins from resolve_joins_pkfk
    for alias in &ordered_aliases {
        let Some(join) = def.joins.iter().find(|j| {
            j.table.to_ascii_lowercase() == *alias || j.from_alias.to_ascii_lowercase() == *alias
        }) else {
            continue;
        };
        let table_ref = def
            .tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == *alias);
        let physical_table = table_ref.map_or(alias.as_str(), |t| t.table.as_str());
        sql.push_str("\nLEFT JOIN ");
        sql.push_str(&quote_table_ref(physical_table));
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(alias));
        sql.push_str(" ON ");
        sql.push_str(&synthesize_on_clause(join, &def.tables));
    }

    // Emit additional joins for fact source tables not covered by dim resolution
    for alias in &fact_aliases {
        let Some(join) = def.joins.iter().find(|j| {
            j.table.to_ascii_lowercase() == *alias || j.from_alias.to_ascii_lowercase() == *alias
        }) else {
            continue;
        };
        let table_ref = def
            .tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == *alias);
        let physical_table = table_ref.map_or(alias.as_str(), |t| t.table.as_str());
        sql.push_str("\nLEFT JOIN ");
        sql.push_str(&quote_table_ref(physical_table));
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(alias));
        sql.push_str(" ON ");
        sql.push_str(&synthesize_on_clause(join, &def.tables));
    }

    // NO GROUP BY — fact queries are unaggregated

    Ok(sql)
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
#[allow(clippy::too_many_lines, clippy::result_large_err)]
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
    let resolved_dims = resolve_names(
        &req.dimensions,
        view_name,
        |name| find_dimension(def, name),
        |_dim| false,
        || def.dimensions.iter().map(|d| d.name.clone()).collect(),
        |name| {
            suggest_closest(
                name,
                &def.dimensions
                    .iter()
                    .map(|d| d.name.clone())
                    .collect::<Vec<_>>(),
            )
        },
        |vn, n| ExpandError::DuplicateDimension {
            view_name: vn,
            name: n,
        },
        |vn, n, avail, sug| ExpandError::UnknownDimension {
            view_name: vn,
            name: n,
            available: avail,
            suggestion: sug,
        },
        |vn, n| ExpandError::DuplicateDimension {
            view_name: vn,
            name: n,
        },
    )?;

    // 3. Resolve requested metrics to their definitions.
    // Phase 43: PRIVATE access check -- private metrics cannot be queried directly.
    // Derived metrics that reference private bases still work because
    // inline_derived_metrics resolves expressions, not access modifiers.
    let resolved_mets = resolve_names(
        &req.metrics,
        view_name,
        |name| find_metric(def, name),
        |met| met.access == AccessModifier::Private,
        || def.metrics.iter().map(|m| m.name.clone()).collect(),
        |name| {
            suggest_closest(
                name,
                &def.metrics
                    .iter()
                    .map(|m| m.name.clone())
                    .collect::<Vec<_>>(),
            )
        },
        |vn, n| ExpandError::DuplicateMetric {
            view_name: vn,
            name: n,
        },
        |vn, n, avail, sug| ExpandError::UnknownMetric {
            view_name: vn,
            name: n,
            available: avail,
            suggestion: sug,
        },
        |vn, n| ExpandError::PrivateMetric {
            view_name: vn,
            name: n,
        },
    )?;

    // 4. Pre-compute all metric expressions: inline facts into base metrics,
    //    then inline metric references into derived metrics.
    let topo_order = toposort_facts(&def.facts).map_err(|e| ExpandError::CycleDetected {
        view_name: view_name.to_string(),
        cycle_description: e,
    })?;
    let resolved_exprs =
        inline_derived_metrics(&def.metrics, &def.facts, &topo_order).map_err(|e| {
            ExpandError::CycleDetected {
                view_name: view_name.to_string(),
                cycle_description: e,
            }
        })?;

    // Phase 31: Check for fan traps before generating SQL.
    check_fan_traps(view_name, def, &resolved_dims, &resolved_mets)?;

    // Phase 32: Pre-compute dimension scoped aliases for role-playing tables.
    // Maps dimension index -> scoped alias (e.g., "a__dep_airport").
    let mut dim_scoped_aliases: Vec<Option<String>> = Vec::with_capacity(resolved_dims.len());
    for dim in &resolved_dims {
        let scoped = find_using_context(view_name, def, dim, &resolved_mets)?;
        dim_scoped_aliases.push(scoped);
    }

    // Phase 47: Check if any resolved metric ACTUALLY needs semi-additive expansion.
    // A semi-additive metric only needs CTE treatment when at least one of its
    // NA dims is NOT in the queried dimension set. When ALL NA dims are in the
    // query, the metric acts as regular (Snowflake semantics).
    let queried_dim_names: std::collections::HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    let has_active_semi_additive = resolved_mets.iter().any(|m| {
        !m.non_additive_by.is_empty()
            && m.non_additive_by
                .iter()
                .any(|na| !queried_dim_names.contains(&na.dimension.to_ascii_lowercase()))
    });

    if has_active_semi_additive {
        return super::semi_additive::expand_semi_additive(
            view_name,
            def,
            &resolved_dims,
            &resolved_mets,
            &resolved_exprs,
            &dim_scoped_aliases,
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
            &resolved_dims,
            &resolved_mets,
            &resolved_exprs,
            &dim_scoped_aliases,
        );
    }

    // 5. Build the SELECT clause.
    //    Dimensions-only (no metrics): SELECT DISTINCT, no GROUP BY.
    //    Metrics-only (no dimensions): SELECT (global aggregate), no GROUP BY.
    //    Both: SELECT with GROUP BY.
    let mut sql = String::with_capacity(256);
    if !resolved_dims.is_empty() && resolved_mets.is_empty() {
        sql.push_str("SELECT DISTINCT\n");
    } else {
        sql.push_str("SELECT\n");
    }

    let mut select_items: Vec<String> = Vec::new();
    for (i, dim) in resolved_dims.iter().enumerate() {
        let mut base_expr = dim.expr.clone();
        // Phase 32: If this dimension has a scoped alias, rewrite the expression.
        if let Some(ref scoped) = dim_scoped_aliases[i] {
            if let Some(ref st) = dim.source_table {
                // Replace bare alias with scoped alias in expression
                // e.g., "a.city" -> "a__dep_airport.city"
                base_expr = replace_word_boundary(&base_expr, st, scoped);
            }
        }
        // If output_type is set, wrap the expression in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = dim.output_type {
            format!("CAST({base_expr} AS {type_str})")
        } else {
            base_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&dim.name)));
    }
    for met in &resolved_mets {
        // Look up the pre-computed resolved expression (handles both base + derived metrics)
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| met.expr.clone());
        // If output_type is set, wrap the aggregate in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = met.output_type {
            format!("CAST({resolved_expr} AS {type_str})")
        } else {
            resolved_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&met.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 6. FROM clause with base table.
    sql.push_str("\nFROM ");
    sql.push_str(&quote_table_ref(&def.base_table));

    // If tables aliases are declared (Phase 11.1), emit AS "alias" after the base table.
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // Join resolution via PK/FK graph (legacy resolve_joins removed in Phase 27).
    // Phase 32: ordered_aliases may contain scoped aliases like "a__dep_airport".
    let ordered_aliases = resolve_joins_pkfk(def, &resolved_dims, &resolved_mets);
    for alias in &ordered_aliases {
        // Phase 32: Check if this is a scoped alias (contains "__").
        if let Some(sep_pos) = alias.find("__") {
            let rel_name = &alias[sep_pos + 2..];
            // Find the Join by relationship name.
            let Some(join) = def.joins.iter().find(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(rel_name))
            }) else {
                continue;
            };
            // Find physical table name from the bare alias (before __).
            let bare_alias = &alias[..sep_pos];
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == bare_alias);
            let physical_table = table_ref.map_or(bare_alias, |t| t.table.as_str());
            sql.push_str("\nLEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause_scoped(join, &def.tables, alias));
        } else {
            // Standard bare alias join (non-role-playing).
            let Some(join) = def.joins.iter().find(|j| {
                j.table.to_ascii_lowercase() == *alias
                    || j.from_alias.to_ascii_lowercase() == *alias
            }) else {
                continue;
            };
            // Find the TableRef for this alias to get the physical table name.
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == *alias);
            let physical_table = table_ref.map_or(alias.as_str(), |t| t.table.as_str());
            sql.push_str("\nLEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause(join, &def.tables));
        }
    }

    // 7. GROUP BY (only when both dimensions and metrics are present).
    //    Use ordinal positions (GROUP BY 1, 2, ...) instead of expressions to avoid
    //    ambiguity when an expression matches its alias (e.g., `status AS "status"`).
    if !resolved_dims.is_empty() && !resolved_mets.is_empty() {
        sql.push_str("\nGROUP BY\n");
        let group_items: Vec<String> = (1..=resolved_dims.len())
            .map(|i| format!("    {i}"))
            .collect();
        sql.push_str(&group_items.join(",\n"));
    }

    Ok(sql)
}

#[cfg(test)]
mod tests {
    use crate::expand::{expand, DimensionName, ExpandError, MetricName, QueryRequest};

    mod expand_tests {
        use super::*;
        use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
        use crate::model::{Join, SemanticViewDefinition};

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
FROM \"orders\"
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
                .with_base_table("orders")
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

        // Legacy join tests removed in Phase 27

        #[test]
        fn test_join_excluded_when_not_needed() {
            let def = orders_view()
                .with_dimension("customer_name", "customers.name", Some("customers"))
                .with_join_on("customers", "orders.customer_id = customers.id");
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
    }

    mod phase11_1_expand_tests {
        use super::*;
        use crate::model::{AccessModifier, JoinColumn, TableRef};

        fn def_with_join_columns() -> crate::model::SemanticViewDefinition {
            crate::model::SemanticViewDefinition {
                base_table: "orders".to_string(),
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

                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    crate::model::Dimension {
                        name: "tier".to_string(),
                        expr: "c.tier".to_string(),
                        source_table: Some("c".to_string()),

                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                metrics: vec![crate::model::Metric {
                    name: "revenue".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],

                joins: vec![crate::model::Join {
                    table: "customers".to_string(),
                    on: String::new(),
                    from_cols: vec![],
                    join_columns: vec![JoinColumn {
                        from: "customer_id".to_string(),
                        to: "id".to_string(),
                    }],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        // Legacy join_columns tests removed in Phase 27

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
        use crate::model::{AccessModifier, Dimension, Metric, SemanticViewDefinition};

        #[test]
        fn output_type_on_metric_emits_cast() {
            let mut def = SemanticViewDefinition::default()
                .with_base_table("orders")
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
            let mut def = SemanticViewDefinition::default().with_base_table("orders");
            def.dimensions.push(Dimension {
                name: "region_id".to_string(),
                expr: "region_id".to_string(),
                source_table: None,
                output_type: Some("INTEGER".to_string()),
                comment: None,
                synonyms: vec![],
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
                .with_base_table("orders")
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
        use crate::model::{
            AccessModifier, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        /// Helper: build a 2-table PK/FK definition (orders -> customers).
        fn pkfk_two_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        /// Helper: build a 3-table PK/FK definition (li -> o -> c).
        fn pkfk_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "line_items".to_string(),
                tables: vec![
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "product".to_string(),
                        expr: "li.product".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                metrics: vec![Metric {
                    name: "total_qty".to_string(),
                    expr: "sum(li.qty)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
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

                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "a".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "b".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["pk1".to_string(), "pk2".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "b.detail".to_string(),
                    source_table: Some("b".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: Some("a".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],

                joins: vec![Join {
                    table: "b".to_string(),
                    from_alias: "a".to_string(),
                    fk_columns: vec!["fk1".to_string(), "fk2".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
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
        use crate::model::{
            AccessModifier, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn qualified_ref_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "p27_orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "p27_orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "order_region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
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
                .with_base_table("orders")
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
                .with_base_table("line_items")
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
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]).unwrap();
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
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / revenue * 100".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]).unwrap();
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
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "double_rev".to_string(),
                    expr: "revenue * 2".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
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
            let resolved = inline_derived_metrics(&metrics, &facts, &topo_order).unwrap();
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
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "b".to_string(),
                    expr: "SUM(y)".to_string(),
                    source_table: Some("t".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "a - b".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / a".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]).unwrap();
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
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "revenue_total".to_string(),
                    expr: "SUM(total)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
                Metric {
                    name: "derived".to_string(),
                    expr: "revenue + revenue_total".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]).unwrap();
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
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(li.unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                ],
                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(DISTINCT o.id)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "avg_order_value".to_string(),
                        expr: "revenue / order_count".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                ],
                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(net_price)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
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
                column_type_names: vec![],
                column_types_inferred: vec![],
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
        use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
        use crate::model::{
            AccessModifier, Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn fan_trap_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "li.status".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "segment".to_string(),
                        expr: "c.segment".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.extended_price)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
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
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                ExpandError::FanTrap {
                    view_name,
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(view_name, "sales");
                    assert_eq!(metric_name, "order_count");
                    assert_eq!(dimension_name, "status");
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
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "d".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "d.detail".to_string(),
                    source_table: Some("d".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
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
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                output_type: None,
                using_relationships: vec![],
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
                non_additive_by: vec![],
                window_spec: None,
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
                ExpandError::FanTrap {
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(metric_name, "customer_count");
                    assert_eq!(dimension_name, "status");
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
                source_table: None,
                output_type: None,
                using_relationships: vec![],
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
                non_additive_by: vec![],
                window_spec: None,
            });
            let req = QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("status")],
                metrics: vec![MetricName::new("avg_order")],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Derived metric fan trap must be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(metric_name, "avg_order");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_error_message_format() {
            let err = ExpandError::FanTrap {
                view_name: "sales".to_string(),
                metric_name: "order_count".to_string(),
                metric_table: "o".to_string(),
                dimension_name: "status".to_string(),
                dimension_table: "li".to_string(),
                relationship_name: "li_to_order".to_string(),
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
            AccessModifier, Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
        };

        fn flights_airports_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "flights".to_string(),
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "city".to_string(),
                        expr: "a.city".to_string(),
                        source_table: Some("a".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "country".to_string(),
                        expr: "a.country".to_string(),
                        source_table: Some("a".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                    Dimension {
                        name: "carrier".to_string(),
                        expr: "f.carrier".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "departure_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        using_relationships: vec!["dep_airport".to_string()],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "arrival_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        using_relationships: vec!["arr_airport".to_string()],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "total_flights".to_string(),
                        expr: "departure_count + arrival_count".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
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
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                .with_base_table("orders")
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
                base_table: "flights".to_string(),
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "carrier".to_string(),
                    expr: "f.carrier".to_string(),
                    source_table: Some("f".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![Metric {
                    name: "airport_count".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("a".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
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
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "orders".to_string(),
                tables: vec![TableRef {
                    alias: "o".to_string(),
                    table: "orders".to_string(),
                    pk_columns: vec!["id".to_string()],
                    unique_constraints: vec![],
                    comment: None,
                    synonyms: vec![],
                }],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                    comment: None,
                    synonyms: vec![],
                    access: AccessModifier::Public,
                    non_additive_by: vec![],
                    window_spec: None,
                }],
                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    name: Some("order_to_customer".to_string()),
                    ..Default::default()
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "SUM(amount)".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "secret_cost".to_string(),
                        expr: "SUM(cost)".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Private,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                ],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
            }
        }

        fn make_def_with_private_and_derived() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                    output_type: None,
                    comment: None,
                    synonyms: vec![],
                }],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "SUM(amount)".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "secret_cost".to_string(),
                        expr: "SUM(cost)".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Private,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "total_revenue - secret_cost".to_string(),
                        source_table: None, // derived metric
                        output_type: None,
                        using_relationships: vec![],
                        comment: None,
                        synonyms: vec![],
                        access: AccessModifier::Public,
                        non_additive_by: vec![],
                        window_spec: None,
                    },
                ],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
        use crate::expand::test_helpers::{orders_view, TestFixtureExt};
        use crate::model::{AccessModifier, Fact, Join, SemanticViewDefinition};

        /// Build a multi-table def: orders (o) -> line_items (li), with a dim on o and facts on li.
        fn multi_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition::default()
                .with_base_table("orders")
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
                .with_base_table("orders")
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
                .with_base_table("orders")
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
                .with_base_table("orders")
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
}
