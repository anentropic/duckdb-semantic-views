use std::fmt;

use crate::model::SemanticViewDefinition;

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
        let dim = find_dimension(def, name).ok_or_else(|| ExpandError::UnknownDimension {
            view_name: view_name.to_string(),
            name: name.clone(),
            available: def.dimensions.iter().map(|d| d.name.clone()).collect(),
            suggestion: None, // Fuzzy matching added in Plan 02
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
        let met = find_metric(def, name).ok_or_else(|| ExpandError::UnknownMetric {
            view_name: view_name.to_string(),
            name: name.clone(),
            available: def.metrics.iter().map(|m| m.name.clone()).collect(),
            suggestion: None, // Fuzzy matching added in Plan 02
        })?;
        resolved_mets.push(met);
    }

    // 4. Build the base CTE.
    let mut sql = String::with_capacity(256);
    sql.push_str("WITH \"_base\" AS (\n    SELECT *\n    FROM ");
    sql.push_str(&quote_ident(&def.base_table));

    // Include all declared joins (join pruning deferred to Plan 02).
    for join in &def.joins {
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

    // 5. Build the outer SELECT.
    sql.push_str("\nSELECT\n");

    let mut select_items: Vec<String> = Vec::new();
    for dim in &resolved_dims {
        select_items.push(format!("    {} AS {}", dim.expr, quote_ident(&dim.name)));
    }
    for met in &resolved_mets {
        select_items.push(format!("    {} AS {}", met.expr, quote_ident(&met.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 6. FROM the base CTE.
    sql.push_str("\nFROM \"_base\"");

    // 7. GROUP BY (only if dimensions are present).
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
        fn test_joins_included_in_cte() {
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
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                }],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // For Plan 01, all declared joins are included (join pruning is Plan 02)
            assert!(sql.contains("JOIN \"customers\" ON orders.customer_id = customers.id"));
        }
    }
}
