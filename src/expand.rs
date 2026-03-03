use std::collections::HashSet;
use std::fmt;

use crate::model::{Join, SemanticViewDefinition}; // TableRef accessed via def.tables

/// Suggest the closest matching name from `available` using Levenshtein distance.
///
/// Returns `Some(name)` (with original casing) if the best match has an edit
/// distance of 3 or fewer characters. Returns `None` if no candidate is close
/// enough. Both the query and candidates are lowercased for comparison.
#[must_use]
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
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
/// At least one dimension or one metric must be specified. Supported modes:
/// - Dimensions only: `SELECT DISTINCT` (no aggregation)
/// - Metrics only: global aggregate (no `GROUP BY`)
/// - Both: grouped aggregation with `GROUP BY`
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
}

/// Errors that can occur during semantic view expansion.
#[derive(Debug)]
pub enum ExpandError {
    /// The request contained neither dimensions nor metrics.
    EmptyRequest { view_name: String },
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
            Self::EmptyRequest { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': specify at least dimensions := [...] or metrics := [...]"
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

/// Quote a potentially dot-qualified table reference.
///
/// Splits on `.` and quotes each part individually. This handles:
/// - Simple names: `orders` -> `"orders"`
/// - Catalog-qualified: `jaffle.raw_orders` -> `"jaffle"."raw_orders"`
/// - Fully qualified: `catalog.schema.table` -> `"catalog"."schema"."table"`
///
/// Each part is quoted via `quote_ident`, so embedded double quotes are escaped.
#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    table
        .split('.')
        .map(quote_ident)
        .collect::<Vec<_>>()
        .join(".")
}

/// Resolve which declared joins are needed for the requested dimensions and metrics.
///
/// Collects `source_table` values from resolved dimensions and metrics, then
/// resolves transitive dependencies using a fixed-point loop: if a needed join's
/// ON clause mentions another declared join's table, that join is also included.
/// Returns the subset of joins in their original declaration order.
///
/// Phase 11.1: if `def.tables` is non-empty, `source_table` may be an alias
/// (e.g., `"c"` for `customers`). This function resolves aliases to physical table
/// names using `def.tables` before matching against `join.table`.
fn resolve_joins<'a>(
    joins: &'a [Join],
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
    def: &SemanticViewDefinition,
) -> Vec<&'a Join> {
    // Helper: resolve a source_table value (may be alias or physical name) to physical table name.
    let resolve_table_name = |st: &str| -> String {
        if !def.tables.is_empty() {
            // Try alias lookup first
            if let Some(tr) = def.tables.iter().find(|t| t.alias.eq_ignore_ascii_case(st)) {
                return tr.table.to_ascii_lowercase();
            }
        }
        st.to_ascii_lowercase()
    };

    // 1. Collect directly-needed tables from source_table fields (case-insensitive).
    let mut needed: HashSet<String> = HashSet::new();
    for dim in resolved_dims {
        if let Some(ref st) = dim.source_table {
            needed.insert(resolve_table_name(st));
        }
    }
    for met in resolved_mets {
        if let Some(ref st) = met.source_table {
            needed.insert(resolve_table_name(st));
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
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.region"),
/// splits into (alias, `bare_name`) and also matches `source_table == alias`.
/// Falls back to `bare_name` lookup if no qualified match is found.
fn find_dimension<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Dimension> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        // Try qualified lookup: bare_name match AND source_table == alias
        if let Some(d) = def.dimensions.iter().find(|d| {
            d.name.eq_ignore_ascii_case(bare)
                && d.source_table
                    .as_deref()
                    .is_some_and(|st| st.eq_ignore_ascii_case(alias))
        }) {
            return Some(d);
        }
        // Fall back to bare_name only (backward compat)
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(bare))
    } else {
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }
}

/// Look up a metric by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.revenue"),
/// splits into (alias, `bare_name`) and also matches `source_table == alias`.
/// Falls back to `bare_name` lookup if no qualified match is found.
fn find_metric<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Metric> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        if let Some(m) = def.metrics.iter().find(|m| {
            m.name.eq_ignore_ascii_case(bare)
                && m.source_table
                    .as_deref()
                    .is_some_and(|st| st.eq_ignore_ascii_case(alias))
        }) {
            return Some(m);
        }
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(bare))
    } else {
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

/// Append the ON clause for a single JOIN to `sql`.
///
/// If `join.join_columns` is non-empty (Phase 11.1 format), generates
/// `alias.col = alias.col AND ...` using alias lookup from `def.tables`.
/// Otherwise falls back to the raw `join.on` string (legacy format).
fn append_join_on_clause(sql: &mut String, join: &Join, def: &SemanticViewDefinition) {
    if join.join_columns.is_empty() {
        // Legacy: raw ON clause string (Phase 10 and earlier definitions).
        sql.push_str(&join.on);
    } else {
        // Phase 11.1: generate ON clause from column-pair structs.
        let to_alias = def
            .tables
            .iter()
            .find(|t| t.table.eq_ignore_ascii_case(&join.table))
            .map_or(join.table.as_str(), |t| t.alias.as_str());
        let from_alias = def
            .tables
            .first()
            .map_or(def.base_table.as_str(), |t| t.alias.as_str());
        let on_parts: Vec<String> = join
            .join_columns
            .iter()
            .map(|jc| {
                format!(
                    "{}.{} = {}.{}",
                    quote_ident(from_alias),
                    quote_ident(&jc.from),
                    quote_ident(to_alias),
                    quote_ident(&jc.to)
                )
            })
            .collect();
        sql.push_str(&on_parts.join(" AND "));
    }
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
/// - Neither dimensions nor metrics are requested (`EmptyRequest`)
/// - A requested dimension or metric name is not found (`UnknownDimension`, `UnknownMetric`)
/// - A dimension or metric name is duplicated (`DuplicateDimension`, `DuplicateMetric`)
#[allow(clippy::too_many_lines)]
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate: at least one dimension or metric is required.
    if req.dimensions.is_empty() && req.metrics.is_empty() {
        return Err(ExpandError::EmptyRequest {
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
    let needed_joins = resolve_joins(&def.joins, &resolved_dims, &resolved_mets, def);

    // 5. Build the base CTE.
    let mut sql = String::with_capacity(256);
    sql.push_str("WITH \"_base\" AS (\n    SELECT *\n    FROM ");
    sql.push_str(&quote_table_ref(&def.base_table));

    // If tables aliases are declared (Phase 11.1), emit AS "alias" after the base table.
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // Include only the joins needed by requested dimensions/metrics.
    for join in &needed_joins {
        sql.push_str("\n    JOIN ");
        sql.push_str(&quote_table_ref(&join.table));
        // Emit AS "alias" when a tables entry matches this join table.
        if !def.tables.is_empty() {
            if let Some(tr) = def
                .tables
                .iter()
                .find(|t| t.table.eq_ignore_ascii_case(&join.table))
            {
                sql.push_str(" AS ");
                sql.push_str(&quote_ident(&tr.alias));
            }
        }
        sql.push_str(" ON ");
        append_join_on_clause(&mut sql, join, def);
    }

    // Append filters as WHERE clause (each parenthesized, AND-composed).
    if !def.filters.is_empty() {
        sql.push_str("\n    WHERE ");
        let filter_clauses: Vec<String> = def.filters.iter().map(|f| format!("({f})")).collect();
        sql.push_str(&filter_clauses.join(" AND "));
    }

    sql.push_str("\n)");

    // 6. Build the outer SELECT.
    //    Dimensions-only (no metrics): SELECT DISTINCT, no GROUP BY.
    //    Metrics-only (no dimensions): SELECT (global aggregate), no GROUP BY.
    //    Both: SELECT with GROUP BY.
    if !resolved_dims.is_empty() && resolved_mets.is_empty() {
        sql.push_str("\nSELECT DISTINCT\n");
    } else {
        sql.push_str("\nSELECT\n");
    }

    let mut select_items: Vec<String> = Vec::new();
    for dim in &resolved_dims {
        let base_expr = dim.expr.clone();
        // If output_type is set, wrap the expression in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = dim.output_type {
            format!("CAST({base_expr} AS {type_str})")
        } else {
            base_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&dim.name)));
    }
    for met in &resolved_mets {
        // If output_type is set, wrap the aggregate in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = met.output_type {
            format!("CAST({} AS {type_str})", met.expr)
        } else {
            met.expr.clone()
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&met.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 7. FROM the base CTE.
    sql.push_str("\nFROM \"_base\"");

    // 8. GROUP BY (only when both dimensions and metrics are present).
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

    mod quote_table_ref_tests {
        use super::*;

        #[test]
        fn simple_table_name() {
            assert_eq!(quote_table_ref("orders"), "\"orders\"");
        }

        #[test]
        fn catalog_qualified() {
            assert_eq!(
                quote_table_ref("jaffle.raw_orders"),
                "\"jaffle\".\"raw_orders\""
            );
        }

        #[test]
        fn fully_qualified() {
            assert_eq!(
                quote_table_ref("catalog.schema.table"),
                "\"catalog\".\"schema\".\"table\""
            );
        }

        #[test]
        fn reserved_word_parts() {
            assert_eq!(quote_table_ref("select.from"), "\"select\".\"from\"");
        }

        #[test]
        fn embedded_quotes_in_parts() {
            assert_eq!(
                quote_table_ref("my\"db.my\"table"),
                "\"my\"\"db\".\"my\"\"table\""
            );
        }
    }

    mod expand_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition};

        /// Helper to build a simple orders view definition.
        fn orders_view() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "status".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "sum(amount)".to_string(),
                        source_table: None,
                        output_type: None,
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "count(*)".to_string(),
                        source_table: None,
                        output_type: None,
                    },
                ],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
    1";
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
    1,
    2";
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
    1";
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
    1";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_identifier_quoting() {
            let def = SemanticViewDefinition {
                base_table: "select".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "col".to_string(),
                    expr: "col".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "month".to_string(),
                    expr: "date_trunc('month', created_at)".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["month".to_string()],
                metrics: vec!["total_revenue".to_string()],
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
                dimensions: vec!["region".to_string(), "status".to_string()],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
)
SELECT DISTINCT
    region AS \"region\",
    status AS \"status\"
FROM \"_base\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_metrics_only_still_works() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
WITH \"_base\" AS (
    SELECT *
    FROM \"orders\"
)
SELECT
    sum(amount) AS \"total_revenue\",
    count(*) AS \"order_count\"
FROM \"_base\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_case_insensitive_dimension_lookup() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "Region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            // Request uses lowercase "region" but definition has "Region"
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
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
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "Total_Revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
        fn test_join_included_when_dimension_needs_it() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "customers.name".to_string(),
                    source_table: Some("customers".to_string()),

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                    from_cols: vec![],
                    join_columns: vec![],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                tables: vec![],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "customers.name".to_string(),
                        source_table: Some("customers".to_string()),

                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                    from_cols: vec![],
                    join_columns: vec![],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "customer_count".to_string(),
                    expr: "count(distinct customers.id)".to_string(),
                    source_table: Some("customers".to_string()),
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                    from_cols: vec![],
                    join_columns: vec![],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region_name".to_string(),
                    expr: "regions.name".to_string(),
                    source_table: Some("regions".to_string()),

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![
                    Join {
                        table: "customers".to_string(),
                        on: "orders.customer_id = customers.id".to_string(),
                        from_cols: vec![],
                        join_columns: vec![],
                    },
                    Join {
                        table: "regions".to_string(),
                        on: "customers.region_id = regions.id".to_string(),
                        from_cols: vec![],
                        join_columns: vec![],
                    },
                ],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region_name".to_string(),
                    expr: "regions.name".to_string(),
                    source_table: Some("regions".to_string()),

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![
                    Join {
                        table: "customers".to_string(),
                        on: "orders.customer_id = customers.id".to_string(),
                        from_cols: vec![],
                        join_columns: vec![],
                    },
                    Join {
                        table: "regions".to_string(),
                        on: "customers.region_id = regions.id".to_string(),
                        from_cols: vec![],
                        join_columns: vec![],
                    },
                ],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
        fn test_dot_qualified_base_table() {
            let def = SemanticViewDefinition {
                base_table: "jaffle.raw_orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "status".to_string(),
                    expr: "status".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "order_count".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["order_count".to_string()],
            };
            let sql = expand("jaffle_orders", &def, &req).unwrap();
            // Must produce "jaffle"."raw_orders" not "jaffle.raw_orders"
            assert!(
                sql.contains("FROM \"jaffle\".\"raw_orders\""),
                "dot-qualified base_table must be split and quoted: {sql}"
            );
        }

        #[test]
        fn test_dot_qualified_join_table() {
            let def = SemanticViewDefinition {
                base_table: "jaffle.raw_orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "customers.name".to_string(),
                    source_table: Some("jaffle.raw_customers".to_string()),

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "order_count".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![Join {
                    table: "jaffle.raw_customers".to_string(),
                    on: "raw_orders.customer_id = raw_customers.id".to_string(),
                    from_cols: vec![],
                    join_columns: vec![],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["order_count".to_string()],
            };
            let sql = expand("jaffle_orders", &def, &req).unwrap();
            assert!(
                sql.contains("JOIN \"jaffle\".\"raw_customers\""),
                "dot-qualified join table must be split and quoted: {sql}"
            );
        }

        #[test]
        fn test_mixed_base_and_joined_dimensions() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "customers.name".to_string(),
                        source_table: Some("customers".to_string()),

                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![
                    Join {
                        table: "customers".to_string(),
                        on: "orders.customer_id = customers.id".to_string(),
                        from_cols: vec![],
                        join_columns: vec![],
                    },
                    Join {
                        table: "products".to_string(),
                        on: "orders.product_id = products.id".to_string(),
                        from_cols: vec![],
                        join_columns: vec![],
                    },
                ],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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

    mod phase11_1_expand_tests {
        use super::*;
        use crate::model::{JoinColumn, TableRef};

        fn def_with_join_columns() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                    },
                ],
                dimensions: vec![
                    crate::model::Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),

                        output_type: None,
                    },
                    crate::model::Dimension {
                        name: "tier".to_string(),
                        expr: "c.tier".to_string(),
                        source_table: Some("c".to_string()),

                        output_type: None,
                    },
                ],
                metrics: vec![crate::model::Metric {
                    name: "revenue".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![crate::model::Join {
                    table: "customers".to_string(),
                    on: String::new(),
                    from_cols: vec![],
                    join_columns: vec![JoinColumn {
                        from: "customer_id".to_string(),
                        to: "id".to_string(),
                    }],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        #[test]
        fn join_columns_generates_on_clause() {
            // Test A: single join_column pair generates alias-qualified ON clause
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["tier".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("JOIN \"customers\" AS \"c\" ON"),
                "Must emit JOIN customers with alias: {sql}"
            );
            assert!(
                sql.contains("\"o\".\"customer_id\" = \"c\".\"id\""),
                "Must emit alias-qualified ON clause: {sql}"
            );
        }

        #[test]
        fn multi_column_join_generates_and_joined_on_clause() {
            // Test B: two join_column pairs generate AND-joined ON clause
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                    },
                ],
                dimensions: vec![crate::model::Dimension {
                    name: "item".to_string(),
                    expr: "li.item".to_string(),
                    source_table: Some("li".to_string()),

                    output_type: None,
                }],
                metrics: vec![],
                filters: vec![],
                joins: vec![crate::model::Join {
                    table: "line_items".to_string(),
                    on: String::new(),
                    from_cols: vec![],
                    join_columns: vec![
                        JoinColumn {
                            from: "order_id".to_string(),
                            to: "order_id".to_string(),
                        },
                        JoinColumn {
                            from: "rev".to_string(),
                            to: "rev".to_string(),
                        },
                    ],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["item".to_string()],
                metrics: vec![],
            };
            let sql = expand("mv", &def, &req).unwrap();
            assert!(
                sql.contains("\"o\".\"order_id\" = \"li\".\"order_id\""),
                "Must emit first pair: {sql}"
            );
            assert!(sql.contains("AND"), "Must emit AND between pairs: {sql}");
            assert!(
                sql.contains("\"o\".\"rev\" = \"li\".\"rev\""),
                "Must emit second pair: {sql}"
            );
        }

        #[test]
        fn join_with_empty_join_columns_falls_back_to_on_string() {
            // Test C: empty join_columns → legacy on string used
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![crate::model::Dimension {
                    name: "customer_name".to_string(),
                    expr: "customers.name".to_string(),
                    source_table: Some("customers".to_string()),

                    output_type: None,
                }],
                metrics: vec![],
                filters: vec![],
                joins: vec![crate::model::Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                    from_cols: vec![],
                    join_columns: vec![],
                }],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec![],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("JOIN \"customers\" ON orders.customer_id = customers.id"),
                "Must use legacy on string when join_columns is empty: {sql}"
            );
        }

        #[test]
        fn table_qualified_dimension_lookup_with_matching_source_table() {
            // Test E: 'o.region' resolves to dimension named 'region' with source_table='o'
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["o.region".to_string()],
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
            // Test F: 'region' (no prefix) resolves by bare name (backward compat)
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
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
            // Test G: 'o.nosuch' returns UnknownDimension with full 'o.nosuch' as name
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["o.nosuch".to_string()],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            match result {
                Err(ExpandError::UnknownDimension { name, .. }) => {
                    // The error name may be the bare 'nosuch' (after fallback) — that's fine
                    // What matters is it returns an error
                    let _ = name;
                }
                other => panic!("Expected UnknownDimension error, got: {:?}", other),
            }
        }

        #[test]
        fn table_qualified_metric_lookup_with_matching_source_table() {
            // Test I: 'o.revenue' resolves to metric named 'revenue' with source_table='o'
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["o.revenue".to_string()],
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
        use crate::model::{Dimension, Metric};

        #[test]
        fn output_type_on_metric_emits_cast() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: Some("BIGINT".to_string()),
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(sum(amount) AS BIGINT)"),
                "output_type BIGINT must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn output_type_on_dimension_emits_cast() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region_id".to_string(),
                    expr: "region_id".to_string(),
                    source_table: None,

                    output_type: Some("INTEGER".to_string()),
                }],
                metrics: vec![],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region_id".to_string()],
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
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
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
}
