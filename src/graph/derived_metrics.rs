//! Derived metric validation (Phase 30).
//!
//! Validates derived metric uniqueness, aggregate prohibition, reference validity,
//! and cycle detection in the derived metric dependency graph.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use crate::model::SemanticViewDefinition;
use crate::util::suggest_closest;

use super::facts::find_fact_references;

/// Known SQL aggregate function names (lowercase).
const AGGREGATE_FUNCTIONS: &[&str] = &[
    "sum",
    "count",
    "avg",
    "min",
    "max",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "string_agg",
    "listagg",
    "group_concat",
    "array_agg",
    "any_value",
    "approx_count_distinct",
    "median",
    "mode",
    "percentile_cont",
    "percentile_disc",
    "corr",
    "covar_pop",
    "covar_samp",
    "regr_avgx",
    "regr_avgy",
    "regr_count",
    "regr_intercept",
    "regr_r2",
    "regr_slope",
    "regr_sxx",
    "regr_sxy",
    "regr_syy",
    "bit_and",
    "bit_or",
    "bit_xor",
    "bool_and",
    "bool_or",
];

/// Check if an expression contains an aggregate function call.
///
/// Scans for known aggregate function names at word boundaries followed by `(`.
/// Case-insensitive matching. Skips matches inside single-quoted string literals.
///
/// Returns the first aggregate function name found (lowercase), or `None`.
#[must_use]
pub fn contains_aggregate_function(expr: &str) -> Option<&'static str> {
    let bytes = expr.as_bytes();
    let expr_lower = expr.to_ascii_lowercase();
    let lower_bytes = expr_lower.as_bytes();

    for &func_name in AGGREGATE_FUNCTIONS {
        let fn_bytes = func_name.as_bytes();
        let fn_len = fn_bytes.len();
        if fn_len > lower_bytes.len() {
            continue;
        }

        let mut i = 0;
        let mut in_string = false;
        while i < lower_bytes.len() {
            // Track string literal state
            if bytes[i] == b'\'' {
                in_string = !in_string;
                i += 1;
                continue;
            }
            if in_string {
                i += 1;
                continue;
            }

            // Check if we have a match at position i
            if i + fn_len <= lower_bytes.len() && &lower_bytes[i..i + fn_len] == fn_bytes {
                // Check word boundary before
                let before_ok = i == 0 || is_word_boundary_byte(bytes[i - 1]);
                // Check followed by '(' (with optional whitespace)
                if before_ok {
                    let after_pos = i + fn_len;
                    let mut j = after_pos;
                    // Skip whitespace
                    while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'(' {
                        return Some(func_name);
                    }
                }
            }
            i += 1;
        }
    }

    None
}

/// Validate derived metrics in a semantic view definition.
///
/// Checks:
/// 1. No duplicate metric names across base and derived (case-insensitive).
/// 2. Derived metrics must not contain aggregate function calls.
/// 3. All metric names referenced in derived metric expressions must exist.
/// 4. The derived metric dependency graph has no cycles (Kahn's algorithm).
///
/// Returns `Ok(())` if valid, `Err` with descriptive message otherwise.
#[allow(clippy::too_many_lines)]
pub fn validate_derived_metrics(def: &SemanticViewDefinition) -> Result<(), String> {
    let derived: Vec<&crate::model::Metric> = def
        .metrics
        .iter()
        .filter(|m| m.source_table.is_none())
        .collect();

    if derived.is_empty() {
        return Ok(());
    }

    // 1. Check metric name uniqueness (case-insensitive)
    check_metric_name_uniqueness(def)?;

    // 2. Check for aggregate functions in derived metrics
    check_no_aggregates_in_derived(&derived)?;

    // 3. Check for unknown metric references in derived expressions
    let all_metric_names: Vec<&str> = def.metrics.iter().map(|m| m.name.as_str()).collect();
    let all_metric_names_display: Vec<String> = all_metric_names
        .iter()
        .copied()
        .map(ToString::to_string)
        .collect();
    check_derived_metric_references(&derived, &all_metric_names, &all_metric_names_display)?;

    // 4. Check for cycles in derived metric dependency graph
    let derived_name_strs: Vec<&str> = derived.iter().map(|m| m.name.as_str()).collect();
    check_derived_metric_cycles(&derived, &derived_name_strs)
}

/// Check that no two metrics (base or derived) share the same name (case-insensitive).
fn check_metric_name_uniqueness(def: &SemanticViewDefinition) -> Result<(), String> {
    let mut seen_names: HashSet<String> = HashSet::new();
    for met in &def.metrics {
        let lower = met.name.to_ascii_lowercase();
        if !seen_names.insert(lower) {
            return Err(format!("duplicate metric name '{}'", met.name));
        }
    }
    Ok(())
}

/// Check that derived metrics do not contain aggregate function calls.
fn check_no_aggregates_in_derived(derived: &[&crate::model::Metric]) -> Result<(), String> {
    for met in derived {
        if let Some(func) = contains_aggregate_function(&met.expr) {
            return Err(format!(
                "derived metric '{}' must not contain aggregate function '{}'. \
                 Derived metrics compose other metrics; use a regular metric for aggregation.",
                met.name, func
            ));
        }
    }
    Ok(())
}

/// Check that all identifiers in derived metric expressions are known metric names.
fn check_derived_metric_references(
    derived: &[&crate::model::Metric],
    all_metric_names: &[&str],
    all_metric_names_display: &[String],
) -> Result<(), String> {
    for met in derived {
        let potential_refs = extract_identifiers(&met.expr);
        for ident in &potential_refs {
            let ident_lower = ident.to_ascii_lowercase();
            if all_metric_names
                .iter()
                .any(|n| n.to_ascii_lowercase() == ident_lower)
            {
                continue;
            }
            if is_sql_keyword_or_builtin(&ident_lower) {
                continue;
            }
            if ident.chars().next().is_none_or(|c| c.is_ascii_digit()) {
                continue;
            }
            let suggestion = suggest_closest(&ident_lower, all_metric_names_display);
            let mut msg = format!(
                "unknown metric '{}' referenced in derived metric '{}'",
                ident, met.name
            );
            if let Some(s) = suggestion {
                let _ = write!(msg, "; did you mean '{s}'?");
            }
            let _ = write!(
                msg,
                ". Available metrics: [{}]",
                all_metric_names.join(", ")
            );
            return Err(msg);
        }
    }
    Ok(())
}

/// Build derived-to-derived DAG and check for cycles via Kahn's algorithm.
fn check_derived_metric_cycles(
    derived: &[&crate::model::Metric],
    derived_name_strs: &[&str],
) -> Result<(), String> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for &name in derived_name_strs {
        in_degree.entry(name).or_insert(0);
    }

    for met in derived {
        let refs = find_fact_references(&met.expr, derived_name_strs);
        for &referenced in &refs {
            edges.entry(met.name.as_str()).or_default().push(referenced);
            *in_degree.entry(referenced).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&name, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(name);
        }
    }

    let mut visited_count = 0;
    while let Some(node) = queue.pop_front() {
        visited_count += 1;
        if let Some(neighbors) = edges.get(node) {
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
    }

    if visited_count != derived_name_strs.len() {
        let remaining: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(&name, _)| name)
            .collect();

        if let Some(&start) = remaining.first() {
            let mut path = vec![start];
            let mut current = start;
            let mut seen: HashSet<&str> = HashSet::new();
            seen.insert(current);

            while let Some(neighbors) = edges.get(current) {
                if let Some(&next) = neighbors.iter().find(|n| remaining.contains(n)) {
                    if seen.contains(next) {
                        path.push(next);
                        return Err(format!(
                            "cycle detected in derived metrics: {}",
                            path.join(" -> ")
                        ));
                    }
                    seen.insert(next);
                    path.push(next);
                    current = next;
                } else {
                    break;
                }
            }
        }

        return Err("cycle detected in derived metrics".to_string());
    }

    Ok(())
}

/// Extract identifiers from an expression (word-boundary tokens).
/// Skips content inside single-quoted strings and dot-qualified identifiers.
fn extract_identifiers(expr: &str) -> Vec<String> {
    let bytes = expr.as_bytes();
    let mut result = Vec::new();
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string {
            i += 1;
            continue;
        }

        // Skip dot-qualified identifiers (e.g., "o.amount" -- the "o" and "amount" parts)
        // We want bare identifiers only
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &expr[start..i];

            // Check if preceded by a dot (table-qualified) -- skip
            if start > 0 && bytes[start - 1] == b'.' {
                continue;
            }
            // Check if followed by a dot -- skip (it's a table alias prefix)
            if i < bytes.len() && bytes[i] == b'.' {
                continue;
            }
            // Check if followed by '(' -- it's a function call, skip
            let mut j = i;
            while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'(' {
                continue;
            }

            result.push(ident.to_string());
        } else {
            i += 1;
        }
    }

    result
}

/// Check if a token is a SQL keyword or common builtin (not a metric reference).
fn is_sql_keyword_or_builtin(token: &str) -> bool {
    const SQL_KEYWORDS: &[&str] = &[
        "and",
        "or",
        "not",
        "is",
        "null",
        "true",
        "false",
        "case",
        "when",
        "then",
        "else",
        "end",
        "in",
        "between",
        "like",
        "ilike",
        "as",
        "cast",
        "distinct",
        "asc",
        "desc",
        "by",
        "having",
        "where",
        "from",
        "select",
        "group",
        "order",
        "limit",
        "offset",
        "union",
        "intersect",
        "except",
        "all",
        "any",
        "exists",
        "over",
        "partition",
        "rows",
        "range",
        "unbounded",
        "preceding",
        "following",
        "current",
        "row",
        "filter",
        "within",
        "nulls",
        "first",
        "last",
        "if",
        "coalesce",
        "nullif",
        "ifnull",
    ];
    SQL_KEYWORDS.contains(&token)
}

/// Check if a byte is a word-boundary character (NOT alphanumeric or underscore).
fn is_word_boundary_byte(b: u8) -> bool {
    !b.is_ascii_alphanumeric() && b != b'_'
}

#[cfg(test)]
mod tests {
    use crate::graph::{contains_aggregate_function, validate_derived_metrics};

    use super::super::test_helpers::*;

    // -----------------------------------------------------------------------
    // Phase 30: contains_aggregate_function tests
    // -----------------------------------------------------------------------

    #[test]
    fn contains_aggregate_sum() {
        let result = contains_aggregate_function("SUM(revenue)");
        assert_eq!(result, Some("sum"));
    }

    #[test]
    fn contains_aggregate_count_distinct() {
        let result = contains_aggregate_function("COUNT(DISTINCT order_id)");
        assert_eq!(result, Some("count"));
    }

    #[test]
    fn contains_aggregate_avg() {
        let result = contains_aggregate_function("AVG(price)");
        assert_eq!(result, Some("avg"));
    }

    #[test]
    fn contains_aggregate_none_arithmetic() {
        let result = contains_aggregate_function("revenue - cost");
        assert_eq!(result, None);
    }

    #[test]
    fn contains_aggregate_none_multiply() {
        let result = contains_aggregate_function("revenue * 100");
        assert_eq!(result, None);
    }

    #[test]
    fn contains_aggregate_none_summary_no_paren() {
        // "SUMMARY" should not match -- not followed by open-paren
        let result = contains_aggregate_function("SUMMARY");
        assert_eq!(result, None);
    }

    #[test]
    fn contains_aggregate_none_string_literal() {
        // Inside single-quoted string literal -- best effort skip
        let result = contains_aggregate_function("'SUM of values'");
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Phase 30: validate_derived_metrics tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_derived_metrics_cycle_detected() {
        // a -> b -> a (cycle)
        let def = make_def_with_derived_metrics(vec![], vec![("a", "b + 1"), ("b", "a + 1")]);
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("cycle detected in derived metrics"),
            "Expected cycle error, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_unknown_reference() {
        // "profit AS revenue - nonexistent" where "nonexistent" is not a metric name
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("profit", "revenue - nonexistent")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("unknown metric") && err.contains("nonexistent"),
            "Expected unknown metric error, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_aggregate_rejected() {
        // Derived metric must not contain aggregate function
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("bad", "SUM(revenue)")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("aggregate function") && err.contains("bad"),
            "Expected aggregate error, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_valid_simple() {
        // "profit AS revenue - cost" where both are base metrics
        let def = make_def_with_derived_metrics(
            vec![
                ("revenue", "SUM(o.amount)", "o"),
                ("cost", "SUM(o.cost)", "o"),
            ],
            vec![("profit", "revenue - cost")],
        );
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "Valid derived metric should be accepted"
        );
    }

    #[test]
    fn validate_derived_metrics_valid_stacking() {
        // "margin AS profit / revenue" where profit is derived and revenue is base
        let def = make_def_with_derived_metrics(
            vec![
                ("revenue", "SUM(o.amount)", "o"),
                ("cost", "SUM(o.cost)", "o"),
            ],
            vec![("profit", "revenue - cost"), ("margin", "profit / revenue")],
        );
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "Stacking derived metrics should be accepted"
        );
    }

    #[test]
    fn validate_derived_metrics_valid_negation() {
        // "neg_revenue AS -revenue"
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("neg_revenue", "-revenue")],
        );
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "Single-metric reference should be accepted"
        );
    }

    #[test]
    fn validate_derived_metrics_did_you_mean() {
        // Close misspelling: "revnue" instead of "revenue"
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("profit", "revnue - cost")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("did you mean"),
            "Expected 'did you mean?' suggestion, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_no_derived_returns_ok() {
        // No derived metrics -> should return Ok immediately
        let def = make_def_with_derived_metrics(vec![("revenue", "SUM(o.amount)", "o")], vec![]);
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "No derived metrics should return Ok"
        );
    }

    #[test]
    fn validate_derived_metrics_duplicate_name_rejected() {
        // Same name used by both base and derived metric
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("revenue", "cost + 1")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("duplicate metric name"),
            "Expected duplicate name error, got: {err}"
        );
    }
}
