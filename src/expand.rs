use std::collections::HashSet;
use std::fmt;

use crate::model::{Join, SemanticViewDefinition};

/// Suggest the closest matching name from `available` using Levenshtein distance.
///
/// Returns `Some(name)` (with original casing) if the best match has an edit
/// distance of 3 or fewer characters. Returns `None` if no candidate is close
/// enough. Both the query and candidates are lowercased for comparison.
fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let query = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let dist = strsim::levenshtein(&query, &candidate.to_ascii_lowercase());
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, candidate));
                }
            } else {
                best = Some((dist, candidate));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// A request to expand a semantic view into SQL.
///
/// Contains the names of dimensions and metrics to include in the query.
/// Dimension names may be empty (producing a global aggregate), but at least
/// one metric is required.
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
}

/// Errors that can occur during semantic view expansion.
#[derive(Debug)]
pub enum ExpandError {
    /// The request contained no metrics — at least one metric is required.
    EmptyMetrics { view_name: String },
    /// A requested dimension name does not exist in the view definition.
    UnknownDimension {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A requested metric name does not exist in the view definition.
    UnknownMetric {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A dimension name was requested more than once.
    DuplicateDimension { view_name: String, name: String },
    /// A metric name was requested more than once.
    DuplicateMetric { view_name: String, name: String },
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMetrics { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': at least one metric is required"
                )
            }
            Self::UnknownDimension {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown dimension '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::UnknownMetric {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown metric '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::DuplicateDimension { view_name, name } => {
                write!(
                    f,
                    "semantic view '{view_name}': duplicate dimension '{name}'"
                )
            }
            Self::DuplicateMetric { view_name, name } => {
                write!(f, "semantic view '{view_name}': duplicate metric '{name}'")
            }
        }
    }
}

impl std::error::Error for ExpandError {}

/// Double-quote a SQL identifier, escaping embedded double quotes.
///
/// `DuckDB` uses `"` for identifier quoting. Internal `"` must be escaped
/// as `""` per the SQL standard.
///
/// # Examples
///
/// ```
/// # use semantic_views::expand::quote_ident;
/// assert_eq!(quote_ident("orders"), "\"orders\"");
/// assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
/// ```
#[must_use]
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Resolve which declared joins are needed for the requested dimensions and metrics.
///
/// Collects `source_table` values from resolved dimensions and metrics, then
/// resolves transitive dependencies using a fixed-point loop: if a needed join's
/// ON clause mentions another declared join's table, that join is also included.
/// Returns the subset of joins in their original declaration order.
fn resolve_joins<'a>(
    joins: &'a [Join],
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
) -> Vec<&'a Join> {
    // 1. Collect directly-needed tables from source_table fields (case-insensitive).
    let mut needed: HashSet<String> = HashSet::new();
    for dim in resolved_dims {
        if let Some(ref st) = dim.source_table {
            needed.insert(st.to_ascii_lowercase());
        }
    }
    for met in resolved_mets {
        if let Some(ref st) = met.source_table {
            needed.insert(st.to_ascii_lowercase());
        }
    }

    if needed.is_empty() {
        return Vec::new();
    }

    // 2. Fixed-point loop: resolve transitive dependencies.
    //    If a needed join's ON clause references another declared join's table name,
    //    add that table to the needed set too.
    loop {
        let mut changed = false;
        for join in joins {
            let table_lower = join.table.to_ascii_lowercase();
            if needed.contains(&table_lower) {
                // This join is needed — check if its ON clause references other join tables.
                let on_lower = join.on.to_ascii_lowercase();
                for other in joins {
                    let other_lower = other.table.to_ascii_lowercase();
                    if other_lower != table_lower
                        && !needed.contains(&other_lower)
                        && on_lower.contains(&other_lower)
                    {
                        needed.insert(other_lower);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // 3. Filter declared joins, preserving declaration order.
    joins
        .iter()
        .filter(|j| needed.contains(&j.table.to_ascii_lowercase()))
        .collect()
}

/// Look up a dimension by name using case-insensitive matching.
fn find_dimension<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Dimension> {
    def.dimensions
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
}

/// Look up a metric by name using case-insensitive matching.
fn find_metric<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Metric> {
    def.metrics
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Expand a semantic view definition into a CTE-wrapped SQL query string.
///
/// Takes a view name (for error messages), its definition, and a query request
/// specifying which dimensions and metrics to include. Returns the generated SQL
/// or an `ExpandError` if the request is invalid.
///
/// # Errors
///
/// Returns `ExpandError` if:
/// - No metrics are requested (`EmptyMetrics`)
/// - A requested dimension or metric name is not found (`UnknownDimension`, `UnknownMetric`)
/// - A dimension or metric name is duplicated (`DuplicateDimension`, `DuplicateMetric`)
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate: at least one metric is required.
    if req.metrics.is_empty() {
        return Err(ExpandError::EmptyMetrics {
            view_name: view_name.to_string(),
        });
    }

    // 2. Resolve requested dimensions to their definitions.
    let mut resolved_dims = Vec::with_capacity(req.dimensions.len());
    let mut seen_dims = std::collections::HashSet::new();
    for name in &req.dimensions {
        if !seen_dims.insert(name.to_ascii_lowercase()) {
            return Err(ExpandError::DuplicateDimension {
                view_name: view_name.to_string(),
                name: name.clone(),
            });
        }
        let dim = find_dimension(def, name).ok_or_else(|| {
            let available: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();
            let suggestion = suggest_closest(name, &available);
            ExpandError::UnknownDimension {
                view_name: view_name.to_string(),
                name: name.clone(),
                available,
                suggestion,
            }
        })?;
        resolved_dims.push(dim);
    }

    // 3. Resolve requested metrics to their definitions.
    let mut resolved_mets = Vec::with_capacity(req.metrics.len());
    let mut seen_mets = std::collections::HashSet::new();
    for name in &req.metrics {
        if !seen_mets.insert(name.to_ascii_lowercase()) {
            return Err(ExpandError::DuplicateMetric {
                view_name: view_name.to_string(),
                name: name.clone(),
            });
        }
        let met = find_metric(def, name).ok_or_else(|| {
            let available: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();
            let suggestion = suggest_closest(name, &available);
            ExpandError::UnknownMetric {
                view_name: view_name.to_string(),
                name: name.clone(),
                available,
                suggestion,
            }
        })?;
        resolved_mets.push(met);
    }

    // 4. Resolve which joins are needed.
    let needed_joins = resolve_joins(&def.joins, &resolved_dims, &resolved_mets);

    // 5. Build the base CTE.
    let mut sql = String::with_capacity(256);
    sql.push_str("WITH \"_base\" AS (\n    SELECT *\n    FROM ");
    sql.push_str(&quote_ident(&def.base_table));

    // Include only the joins needed by requested dimensions/metrics.
    for join in &needed_joins {
        sql.push_str("\n    JOIN ");
        sql.push_str(&quote_ident(&join.table));
        sql.push_str(" ON ");
        sql.push_str(&join.on);
    }

    // Append filters as WHERE clause (each parenthesized, AND-composed).
    if !def.filters.is_empty() {
        sql.push_str("\n    WHERE ");
        let filter_clauses: Vec<String> = def.filters.iter().map(|f| format!("({f})")).collect();
        sql.push_str(&filter_clauses.join(" AND "));
    }

    sql.push_str("\n)");

    // 6. Build the outer SELECT.
    sql.push_str("\nSELECT\n");

    let mut select_items: Vec<String> = Vec::new();
    for dim in &resolved_dims {
        select_items.push(format!("    {} AS {}", dim.expr, quote_ident(&dim.name)));
    }
    for met in &resolved_mets {
        select_items.push(format!("    {} AS {}", met.expr, quote_ident(&met.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 7. FROM the base CTE.
    sql.push_str("\nFROM \"_base\"");

    // 8. GROUP BY (only if dimensions are present).
    if !resolved_dims.is_empty() {
        sql.push_str("\nGROUP BY\n");
        let group_items: Vec<String> = resolved_dims
            .iter()
            .map(|d| format!("    {}", d.expr))
            .collect();
        sql.push_str(&group_items.join(",\n"));
    }

    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod quote_ident_tests {
        use super::*;

        #[test]
        fn simple_identifier() {
            assert_eq!(quote_ident("orders"), "\"orders\"");
        }

        #[test]
        fn reserved_word() {
            assert_eq!(quote_ident("select"), "\"select\"");
        }

        #[test]
        fn embedded_double_quote() {
            assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
        }

        #[test]
        fn identifier_with_spaces() {
            assert_eq!(quote_ident("my table"), "\"my table\"");
        }
    }

    mod expand_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition};

        /// Helper to build a simple orders view definition.
        fn orders_view() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "status".to_string(),
                        source_table: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "sum(amount)".to_string(),
                        source_table: None,
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "count(*)".to_string(),
                        source_table: None,
                    },
                ],
                filters: vec![],
                joins: vec![],
            }
        }

        #[test]
        fn test_basic_single_dimension_single_metric() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
)
SELECT
    region AS \"region\",
    sum(amount) AS \"total_revenue\"
FROM \"_base\"
GROUP BY
    region";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_multiple_dimensions_multiple_metrics() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "status".to_string()],
                metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
)
SELECT
    region AS \"region\",
    status AS \"status\",
    sum(amount) AS \"total_revenue\",
    count(*) AS \"order_count\"
FROM \"_base\"
GROUP BY
    region,
    status";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_global_aggregate_no_dimensions() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
)
SELECT
    sum(amount) AS \"total_revenue\"
FROM \"_base\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_filters_and_composed() {
            let mut def = orders_view();
            def.filters = vec![
                "status = 'completed'".to_string(),
                "amount > 100".to_string(),
            ];
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
    WHERE (status = 'completed') AND (amount > 100)
)
SELECT
    region AS \"region\",
    sum(amount) AS \"total_revenue\"
FROM \"_base\"
GROUP BY
    region";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_single_filter() {
            let mut def = orders_view();
            def.filters = vec!["status = 'completed'".to_string()];
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
    WHERE (status = 'completed')
)
SELECT
    region AS \"region\",
    sum(amount) AS \"total_revenue\"
FROM \"_base\"
GROUP BY
    region";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_identifier_quoting() {
            let def = SemanticViewDefinition {
                base_table: "select".to_string(),
                dimensions: vec![Dimension {
                    name: "col".to_string(),
                    expr: "col".to_string(),
                    source_table: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["col".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            // Base table "select" must be quoted, CTE name is always "_base" quoted
            assert!(sql.contains("FROM \"select\""));
            assert!(sql.contains("\"_base\""));
        }

        #[test]
        fn test_dimension_expression_not_quoted() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "month".to_string(),
                    expr: "date_trunc('month', created_at)".to_string(),
                    source_table: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["month".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Expression appears verbatim in both SELECT and GROUP BY (not quoted)
            assert!(sql.contains("date_trunc('month', created_at) AS \"month\""));
            assert!(sql.contains("GROUP BY\n    date_trunc('month', created_at)"));
        }

        #[test]
        fn test_empty_metrics_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec![],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::EmptyMetrics { view_name } => {
                    assert_eq!(view_name, "orders");
                }
                other => panic!("Expected EmptyMetrics, got: {other}"),
            }
        }

        #[test]
        fn test_case_insensitive_dimension_lookup() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "Region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![],
            };
            // Request uses lowercase "region" but definition has "Region"
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's expression
            assert!(sql.contains("region AS \"Region\""));
            assert!(sql.contains("GROUP BY\n    region"));
        }

        #[test]
        fn test_unknown_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["reigon".to_string()],
                metrics: vec!["total_revenue".to_string()],
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
                dimensions: vec![],
                metrics: vec!["totl_revenue".to_string()],
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
                dimensions: vec!["xyzzy".to_string()],
                metrics: vec!["total_revenue".to_string()],
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
                dimensions: vec!["region".to_string(), "region".to_string()],
                metrics: vec!["total_revenue".to_string()],
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
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string(), "total_revenue".to_string()],
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
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "Total_Revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![],
            };
            // Request uses lowercase "total_revenue" but definition has "Total_Revenue"
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's name casing in the alias
            assert!(sql.contains("sum(amount) AS \"Total_Revenue\""));
        }

        #[test]
        fn test_error_display_messages() {
            // EmptyMetrics
            let err = ExpandError::EmptyMetrics {
                view_name: "orders".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("at least one metric is required"));

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
        fn test_join_included_when_dimension_needs_it() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "customers.name".to_string(),
                    source_table: Some("customers".to_string()),
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                }],
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.contains("JOIN \"customers\" ON orders.customer_id = customers.id"));
        }

        #[test]
        fn test_join_excluded_when_not_needed() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "customers.name".to_string(),
                        source_table: Some("customers".to_string()),
                    },
                ],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                }],
            };
            // Request only "region" which comes from base table
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "JOIN should not appear when only base-table dims/metrics requested"
            );
        }

        #[test]
        fn test_join_included_when_metric_needs_it() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                }],
                metrics: vec![Metric {
                    name: "customer_count".to_string(),
                    expr: "count(distinct customers.id)".to_string(),
                    source_table: Some("customers".to_string()),
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                }],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["customer_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(sql.contains("JOIN \"customers\" ON orders.customer_id = customers.id"));
        }

        #[test]
        fn test_transitive_join_resolution() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "region_name".to_string(),
                    expr: "regions.name".to_string(),
                    source_table: Some("regions".to_string()),
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![
                    Join {
                        table: "customers".to_string(),
                        on: "orders.customer_id = customers.id".to_string(),
                    },
                    Join {
                        table: "regions".to_string(),
                        on: "customers.region_id = regions.id".to_string(),
                    },
                ],
            };
            let req = QueryRequest {
                dimensions: vec!["region_name".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // regions depends on customers (ON clause references customers), so both must be included
            assert!(
                sql.contains("JOIN \"customers\""),
                "transitive dependency 'customers' must be included"
            );
            assert!(
                sql.contains("JOIN \"regions\""),
                "directly needed 'regions' must be included"
            );
        }

        #[test]
        fn test_joins_emitted_in_declaration_order() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "region_name".to_string(),
                    expr: "regions.name".to_string(),
                    source_table: Some("regions".to_string()),
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![
                    Join {
                        table: "customers".to_string(),
                        on: "orders.customer_id = customers.id".to_string(),
                    },
                    Join {
                        table: "regions".to_string(),
                        on: "customers.region_id = regions.id".to_string(),
                    },
                ],
            };
            let req = QueryRequest {
                dimensions: vec!["region_name".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let customers_pos = sql
                .find("JOIN \"customers\"")
                .expect("customers join missing");
            let regions_pos = sql.find("JOIN \"regions\"").expect("regions join missing");
            assert!(
                customers_pos < regions_pos,
                "customers must appear before regions (declaration order)"
            );
        }

        #[test]
        fn test_no_joins_declared_no_error() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "no JOIN clauses when no joins declared"
            );
        }

        #[test]
        fn test_mixed_base_and_joined_dimensions() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "customers.name".to_string(),
                        source_table: Some("customers".to_string()),
                    },
                ],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                }],
                filters: vec![],
                joins: vec![
                    Join {
                        table: "customers".to_string(),
                        on: "orders.customer_id = customers.id".to_string(),
                    },
                    Join {
                        table: "products".to_string(),
                        on: "orders.product_id = products.id".to_string(),
                    },
                ],
            };
            // Request base-table "region" AND joined "customer_name"
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "customer_name".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("JOIN \"customers\""),
                "customers join needed for customer_name"
            );
            assert!(
                !sql.contains("JOIN \"products\""),
                "products join NOT needed"
            );
        }
    }
}
