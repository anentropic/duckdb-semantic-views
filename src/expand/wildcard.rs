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
///
/// Items declared without a table qualifier (`source_table == None`) are
/// base-table items everywhere else in the expansion layer, so they are
/// included when the wildcard alias is the base/root (first declared) table's
/// alias (SG-15). Both spellings — `source_table == Some(base_alias)` and
/// `source_table == None` — expand identically under `base_alias.*`.
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
            // SG-15: items declared without a source table are base-table
            // items; `base_alias.*` must include them.
            let is_base_alias = def
                .tables
                .first()
                .is_some_and(|t| t.alias.eq_ignore_ascii_case(alias));
            let on_table = |source_table: Option<&str>| -> bool {
                source_table.map_or(is_base_alias, |st| st.eq_ignore_ascii_case(alias))
            };
            match item_type {
                WildcardItemType::Dimension => {
                    for dim in &def.dimensions {
                        if on_table(dim.source_table.as_deref()) {
                            // Dedup on the identifier match key so quoted and
                            // unquoted names collapse exactly as resolution
                            // matches them (review on #84).
                            let key = crate::ident::normalize_ident_part(&dim.name);
                            if seen.insert(key) {
                                result.push(dim.name.clone());
                            }
                        }
                    }
                }
                WildcardItemType::Metric => {
                    for met in &def.metrics {
                        if on_table(met.source_table.as_deref())
                            && met.access != AccessModifier::Private
                        {
                            let key = crate::ident::normalize_ident_part(&met.name);
                            if seen.insert(key) {
                                result.push(met.name.clone());
                            }
                        }
                    }
                }
                WildcardItemType::Fact => {
                    for fact in &def.facts {
                        if on_table(fact.source_table.as_deref())
                            && fact.access != AccessModifier::Private
                        {
                            let key = crate::ident::normalize_ident_part(&fact.name);
                            if seen.insert(key) {
                                result.push(fact.name.clone());
                            }
                        }
                    }
                }
            }
        } else {
            let key = crate::ident::normalize_ident_part(item);
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

    // -------------------------------------------------------------------
    // SG-15 (code review 2026-07-02): base-alias wildcards must include
    // items declared WITHOUT a source table (base-table items).
    // -------------------------------------------------------------------

    /// Base table `o` with items in BOTH spellings: `source_table == None`
    /// (unqualified declaration) and `source_table == Some("o")`.
    fn base_none_def() -> SemanticViewDefinition {
        let mut def = test_def();
        def.dimensions.push(Dimension {
            name: "order_year".to_string(),
            expr: "year(order_date)".to_string(),
            source_table: None,
            ..Default::default()
        });
        def.metrics.push(Metric {
            name: "base_count".to_string(),
            expr: "count(*)".to_string(),
            source_table: None,
            access: AccessModifier::Public,
            ..Default::default()
        });
        def.facts.push(Fact {
            name: "base_flag".to_string(),
            expr: "flag".to_string(),
            source_table: None,
            access: AccessModifier::Public,
            ..Default::default()
        });
        def
    }

    #[test]
    fn test_base_alias_wildcard_includes_unqualified_dimensions() {
        let def = base_none_def();
        let items = vec!["o.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension).unwrap();
        // Both spellings expand under the base alias: Some("o") dims AND the
        // unqualified (None) dim.
        assert_eq!(result, vec!["region", "status", "order_year"]);
    }

    #[test]
    fn test_non_base_alias_wildcard_excludes_unqualified_dimensions() {
        let def = base_none_def();
        let items = vec!["li.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension).unwrap();
        assert_eq!(
            result,
            vec!["product"],
            "None-source items are base-table items, not li items"
        );
    }

    #[test]
    fn test_base_alias_wildcard_includes_unqualified_metrics() {
        let def = base_none_def();
        let items = vec!["o.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Metric).unwrap();
        // order_count (Some "o") + base_count (None); secret_metric stays
        // excluded (PRIVATE).
        assert_eq!(result, vec!["order_count", "base_count"]);
    }

    #[test]
    fn test_base_alias_wildcard_includes_unqualified_facts() {
        let def = base_none_def();
        let items = vec!["o.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Fact).unwrap();
        assert_eq!(result, vec!["base_flag"]);
    }

    #[test]
    fn test_base_alias_wildcard_case_insensitive() {
        let def = base_none_def();
        let items = vec!["O.*".to_string()];
        let result = expand_wildcards(&items, &def, &WildcardItemType::Dimension).unwrap();
        assert_eq!(result, vec!["region", "status", "order_year"]);
    }
}
