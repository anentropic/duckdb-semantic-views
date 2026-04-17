//! Wildcard expansion for `table_alias.*` patterns.
//!
//! Expands `table_alias.*` wildcards in dimension/metric/fact item lists
//! to concrete names, respecting PRIVATE access modifiers.

use crate::model::{AccessModifier, SemanticViewDefinition};

/// Item types for wildcard expansion.
pub enum WildcardItemType {
    Dimension,
    Metric,
    Fact,
}

/// Expand `table_alias.*` wildcards in an item list to concrete names.
///
/// Only qualified wildcards (`alias.*`) are supported -- bare `*` is rejected
/// (matches Snowflake behavior). PRIVATE metrics and facts are excluded from
/// expansion. Dimensions have no access modifier and are always included.
/// Duplicates (from wildcard + explicit overlap) are removed.
pub fn expand_wildcards(
    items: &[String],
    def: &SemanticViewDefinition,
    item_type: &WildcardItemType,
) -> Result<Vec<String>, String> {
    // Quick path: no wildcards at all
    if !items.iter().any(|s| s.contains('*')) {
        return Ok(items.to_vec());
    }

    let mut result = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for item in items {
        if item == "*" {
            return Err(
                "unqualified wildcard '*' is not supported. Use table_alias.* to select all items for a specific table."
                    .to_string(),
            );
        }
        if item.ends_with(".*") {
            let alias = &item[..item.len() - 2];
            // Validate alias exists in tables
            let alias_exists = def
                .tables
                .iter()
                .any(|t| t.alias.eq_ignore_ascii_case(alias));
            if !alias_exists {
                return Err(format!(
                    "unknown table alias '{alias}' in wildcard '{item}'. Available aliases: [{}]",
                    def.tables
                        .iter()
                        .map(|t| t.alias.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            match item_type {
                WildcardItemType::Dimension => {
                    for dim in &def.dimensions {
                        if dim
                            .source_table
                            .as_deref()
                            .is_some_and(|st| st.eq_ignore_ascii_case(alias))
                        {
                            let key = dim.name.to_ascii_lowercase();
                            if seen.insert(key) {
                                result.push(dim.name.clone());
                            }
                        }
                    }
                }
                WildcardItemType::Metric => {
                    for met in &def.metrics {
                        if met
                            .source_table
                            .as_deref()
                            .is_some_and(|st| st.eq_ignore_ascii_case(alias))
                            && met.access != AccessModifier::Private
                        {
                            let key = met.name.to_ascii_lowercase();
                            if seen.insert(key) {
                                result.push(met.name.clone());
                            }
                        }
                    }
                }
                WildcardItemType::Fact => {
                    for fact in &def.facts {
                        if fact
                            .source_table
                            .as_deref()
                            .is_some_and(|st| st.eq_ignore_ascii_case(alias))
                            && fact.access != AccessModifier::Private
                        {
                            let key = fact.name.to_ascii_lowercase();
                            if seen.insert(key) {
                                result.push(fact.name.clone());
                            }
                        }
                    }
                }
            }
        } else {
            let key = item.to_ascii_lowercase();
            if seen.insert(key) {
                result.push(item.clone());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Dimension, Fact, Metric, SemanticViewDefinition, TableRef};

    fn test_def() -> SemanticViewDefinition {
        SemanticViewDefinition {
            base_table: "orders".to_string(),
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
            dimensions: vec![
                Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Dimension {
                    name: "status".to_string(),
                    expr: "o.status".to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                },
                Dimension {
                    name: "product".to_string(),
                    expr: "li.product".to_string(),
                    source_table: Some("li".to_string()),
                    ..Default::default()
                },
            ],
            metrics: vec![
                Metric {
                    name: "order_count".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: Some("o".to_string()),
                    access: AccessModifier::Public,
                    ..Default::default()
                },
                Metric {
                    name: "secret_metric".to_string(),
                    expr: "sum(secret)".to_string(),
                    source_table: Some("o".to_string()),
                    access: AccessModifier::Private,
                    ..Default::default()
                },
                Metric {
                    name: "total_qty".to_string(),
                    expr: "sum(li.quantity)".to_string(),
                    source_table: Some("li".to_string()),
                    access: AccessModifier::Public,
                    ..Default::default()
                },
            ],
            facts: vec![
                Fact {
                    name: "line_total".to_string(),
                    expr: "li.quantity * li.price".to_string(),
                    source_table: Some("li".to_string()),
                    access: AccessModifier::Public,
                    ..Default::default()
                },
                Fact {
                    name: "hidden_fact".to_string(),
                    expr: "li.discount".to_string(),
                    source_table: Some("li".to_string()),
                    access: AccessModifier::Private,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_wildcard_expands_dimensions() {
        let def = test_def();
        let items = vec!["o.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension).unwrap();
        assert_eq!(result, vec!["region", "status"]);
    }

    #[test]
    fn test_wildcard_expands_metrics_excludes_private() {
        let def = test_def();
        let items = vec!["o.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Metric).unwrap();
        // secret_metric is PRIVATE, should be excluded
        assert_eq!(result, vec!["order_count"]);
    }

    #[test]
    fn test_wildcard_expands_facts_excludes_private() {
        let def = test_def();
        let items = vec!["li.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Fact).unwrap();
        // hidden_fact is PRIVATE, should be excluded
        assert_eq!(result, vec!["line_total"]);
    }

    #[test]
    fn test_wildcard_unknown_alias_error() {
        let def = test_def();
        let items = vec!["nonexistent.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("unknown table alias"),
            "Error should contain 'unknown table alias': {err}"
        );
        assert!(
            err.contains("nonexistent"),
            "Error should contain bad alias: {err}"
        );
    }

    #[test]
    fn test_wildcard_bare_star_error() {
        let def = test_def();
        let items = vec!["*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("unqualified wildcard"),
            "Error should contain 'unqualified wildcard': {err}"
        );
    }

    #[test]
    fn test_wildcard_deduplication() {
        let def = test_def();
        // "region" is explicitly listed AND also part of o.*
        let items = vec!["region".to_string(), "o.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension).unwrap();
        // region should appear only once; status added from wildcard
        assert_eq!(result, vec!["region", "status"]);
    }

    #[test]
    fn test_wildcard_no_wildcards_passthrough() {
        let def = test_def();
        let items = vec!["region".to_string(), "status".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension).unwrap();
        assert_eq!(result, vec!["region", "status"]);
    }
}
