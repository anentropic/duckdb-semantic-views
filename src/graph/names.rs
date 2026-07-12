//! Define-time name-uniqueness validation (SG-13).
//!
//! Dimensions, metrics, and facts are resolved from one request namespace at
//! query time: `semantic_view(...)` looks names up under the Snowflake
//! identifier contract and the first declaration wins, so any collision —
//! within a kind or across kinds — silently shadows (duplicate metrics) or
//! emits duplicate output columns (dimension/metric sharing a name). Reject
//! collisions when the definition is created or altered.
//!
//! The uniqueness key MUST be the same key resolution uses
//! ([`crate::ident::normalize_ident_part`]): an unquoted name folds to
//! lowercase, a `"quoted"` name keeps its exact case. Keying on a plain
//! `to_ascii_lowercase` instead would diverge from resolution — a quoted
//! `"region"` and an unquoted `REGION` would pass as distinct here yet both
//! resolve to key `region`, silently shadowing (the SG-13 class this module
//! prevents); conversely `"Region"` and `"REGION"` are genuinely distinct
//! objects and must NOT be rejected as duplicates.
//!
//! This is define-time-only validation: read paths (`SHOW`, `DESCRIBE`,
//! expansion) intentionally keep first-match behavior so legacy catalog rows
//! that predate this check still load and query.

use std::collections::HashMap;

use crate::model::SemanticViewDefinition;

/// Validate that dimension, metric, and fact names are unique across the
/// shared namespace, under the same identifier contract resolution uses
/// (unquoted case-insensitive, `"quoted"` case-sensitive — see the module
/// docs and [`crate::ident::normalize_ident_part`]).
///
/// Returns `Err` naming the colliding item and the kinds involved.
pub fn validate_name_uniqueness(def: &SemanticViewDefinition) -> Result<(), String> {
    let mut seen: HashMap<String, (&str, &str)> = HashMap::new();
    let items = def
        .dimensions
        .iter()
        .map(|d| ("dimension", d.name.as_str()))
        .chain(def.metrics.iter().map(|m| ("metric", m.name.as_str())))
        .chain(def.facts.iter().map(|f| ("fact", f.name.as_str())));
    for (kind, name) in items {
        let key = crate::ident::normalize_ident_part(name);
        if let Some((first_kind, first_name)) = seen.get(key.as_str()) {
            return Err(format!(
                "duplicate name '{name}': {kind} '{name}' collides with {first_kind} \
                 '{first_name}' -- dimension, metric, and fact names share one namespace; \
                 unquoted names are case-insensitive (quoted names case-sensitive)"
            ));
        }
        seen.insert(key, (kind, name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_name_uniqueness;
    use crate::model::{Dimension, Fact, Metric, SemanticViewDefinition};

    fn def_with(dims: &[&str], metrics: &[&str], facts: &[&str]) -> SemanticViewDefinition {
        SemanticViewDefinition {
            dimensions: dims
                .iter()
                .map(|n| Dimension {
                    name: (*n).to_string(),
                    expr: (*n).to_string(),
                    ..Default::default()
                })
                .collect(),
            metrics: metrics
                .iter()
                .map(|n| Metric {
                    name: (*n).to_string(),
                    expr: format!("sum({n})"),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                })
                .collect(),
            facts: facts
                .iter()
                .map(|n| Fact {
                    name: (*n).to_string(),
                    expr: (*n).to_string(),
                    source_table: Some("o".to_string()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn distinct_names_accepted() {
        let def = def_with(&["region", "status"], &["revenue"], &["net_price"]);
        assert!(validate_name_uniqueness(&def).is_ok());
    }

    #[test]
    fn duplicate_metric_names_rejected_case_insensitively() {
        let def = def_with(&[], &["Revenue", "revenue"], &[]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("duplicate name 'revenue'")
                && err.contains("metric 'revenue' collides with metric 'Revenue'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dimension_metric_collision_rejected() {
        let def = def_with(&["region"], &["REGION"], &[]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("metric 'REGION' collides with dimension 'region'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fact_dimension_collision_rejected() {
        let def = def_with(&["amount"], &[], &["amount"]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("fact 'amount' collides with dimension 'amount'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn duplicate_dimension_names_rejected() {
        let def = def_with(&["region", "Region"], &[], &[]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("dimension 'Region' collides with dimension 'region'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn metric_fact_collision_rejected() {
        let def = def_with(&[], &["net_price"], &["Net_Price"]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("fact 'Net_Price' collides with metric 'net_price'"),
            "unexpected error: {err}"
        );
    }

    /// Regression (code-review on #84): the uniqueness key must be the same
    /// key resolution uses. A quoted `"region"` and an unquoted `REGION` both
    /// resolve to key `region`, so they MUST be rejected as duplicates —
    /// otherwise an unquoted reference silently shadows whichever was declared
    /// first (SG-13). Previously they keyed on `to_ascii_lowercase`, which saw
    /// `"region"` and `region` as distinct and accepted the pair.
    #[test]
    fn quoted_and_unquoted_folding_to_same_key_collide() {
        let def = def_with(&["\"region\"", "REGION"], &[], &[]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("duplicate name 'REGION'") && err.contains("dimension '\"region\"'"),
            "unexpected error: {err}"
        );
    }

    /// Mirror case: two quoted names differing only in case are DISTINCT
    /// objects under the identifier contract and must NOT be rejected as
    /// duplicates (previously `to_ascii_lowercase` conflated them).
    #[test]
    fn quoted_names_differing_in_case_are_distinct() {
        let def = def_with(&["\"Region\"", "\"REGION\""], &[], &[]);
        assert!(
            validate_name_uniqueness(&def).is_ok(),
            "quoted mixed-case names must be treated as distinct objects"
        );
    }

    /// An unquoted name and a quoted name that preserves the SAME lowercase
    /// spelling collide (unquoted `region` ≡ quoted `"region"` under the
    /// lowercase fold, mirroring Snowflake's unquoted ≡ capitalized-quoted).
    #[test]
    fn unquoted_and_matching_lowercase_quoted_collide() {
        let def = def_with(&["region", "\"region\""], &[], &[]);
        let err = validate_name_uniqueness(&def).unwrap_err();
        assert!(
            err.contains("duplicate name"),
            "unquoted `region` and quoted `\"region\"` share key `region`: {err}"
        );
    }
}
