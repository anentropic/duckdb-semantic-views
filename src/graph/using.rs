//! USING relationship validation (Phase 32).
//!
//! Validates that all `using_relationships` references on metrics are valid:
//! derived metrics must not have USING, each relationship must exist, and
//! each relationship must originate from the metric's source table.

use crate::model::SemanticViewDefinition;

/// Validate that all `using_relationships` references on metrics are valid.
///
/// For each metric with non-empty `using_relationships`:
/// 1. Derived metrics (`source_table` is None) must not have USING.
/// 2. Each referenced relationship name must exist in `def.joins`.
/// 3. Each referenced relationship must originate from the metric's `source_table`.
///
/// Returns `Ok(())` if all references are valid, `Err` with descriptive message otherwise.
pub fn validate_using_relationships(def: &SemanticViewDefinition) -> Result<(), String> {
    // Collect all named relationships for lookup
    let named_rels: Vec<(&crate::model::Join, String)> = def
        .joins
        .iter()
        .filter_map(|j| j.name.as_ref().map(|n| (j, n.to_ascii_lowercase())))
        .collect();

    let available_names: Vec<String> = named_rels.iter().map(|(_, n)| n.clone()).collect();

    for metric in &def.metrics {
        if metric.using_relationships.is_empty() {
            continue;
        }

        // Check 1: derived metrics must not have USING
        if metric.source_table.is_none() {
            return Err(format!(
                "USING clause not allowed on derived metric '{}'",
                metric.name
            ));
        }

        let metric_source = metric.source_table.as_ref().unwrap().to_ascii_lowercase();

        for rel_name in &metric.using_relationships {
            let rel_lower = rel_name.to_ascii_lowercase();

            // Check 2: relationship must exist
            let found = named_rels.iter().find(|(_, n)| *n == rel_lower);

            match found {
                None => {
                    return Err(format!(
                        "unknown relationship '{rel_name}' in USING clause of metric '{}'. \
                         Available: [{}]",
                        metric.name,
                        available_names.join(", ")
                    ));
                }
                Some((join, _)) => {
                    // Check 3: relationship must originate from metric's source_table
                    let from_lower = join.from_alias.to_ascii_lowercase();
                    if from_lower != metric_source {
                        return Err(format!(
                            "relationship '{rel_name}' does not originate from table '{}' \
                             (metric '{}')",
                            metric.source_table.as_ref().unwrap(),
                            metric.name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::graph::validate_using_relationships;

    use super::super::test_helpers::*;

    #[test]
    fn validate_using_valid_reference() {
        // USING references existing named relationship -> Ok
        let def = make_def_with_named_joins(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![
                (Some("dep_airport"), "f", "a", vec!["dep_id"]),
                (Some("arr_airport"), "f", "a", vec!["arr_id"]),
            ],
            vec![("departure_count", Some("f"), vec!["dep_airport"])],
        );
        assert!(
            validate_using_relationships(&def).is_ok(),
            "Valid USING reference should be accepted"
        );
    }

    #[test]
    fn validate_using_unknown_relationship_rejected() {
        // USING references non-existent relationship -> Err with suggestion
        let def = make_def_with_named_joins(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![(Some("dep_airport"), "f", "a", vec!["dep_id"])],
            vec![("departure_count", Some("f"), vec!["nonexistent"])],
        );
        let err = validate_using_relationships(&def).unwrap_err();
        assert!(
            err.contains("unknown relationship") && err.contains("nonexistent"),
            "Expected unknown relationship error, got: {err}"
        );
        assert!(
            err.contains("dep_airport"),
            "Error should list available relationships: {err}"
        );
    }

    #[test]
    fn validate_using_wrong_source_table_rejected() {
        // USING references relationship from wrong source table -> Err
        let def = make_def_with_named_joins(
            vec![
                ("f", "flights", vec!["id"]),
                ("a", "airports", vec!["id"]),
                ("p", "passengers", vec!["id"]),
            ],
            vec![
                (Some("dep_airport"), "f", "a", vec!["dep_id"]),
                (Some("pax_to_flight"), "p", "f", vec!["flight_id"]),
            ],
            // Metric is on "p" but references "dep_airport" which originates from "f"
            vec![("pax_count", Some("p"), vec!["dep_airport"])],
        );
        let err = validate_using_relationships(&def).unwrap_err();
        assert!(
            err.contains("does not originate"),
            "Expected wrong source table error, got: {err}"
        );
    }

    #[test]
    fn validate_using_derived_metric_rejected() {
        // USING on derived metric (source_table is None) -> Err
        let def = make_def_with_named_joins(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![(Some("dep_airport"), "f", "a", vec!["dep_id"])],
            vec![("derived_met", None, vec!["dep_airport"])],
        );
        let err = validate_using_relationships(&def).unwrap_err();
        assert!(
            err.contains("derived metric") && err.contains("USING"),
            "Expected USING on derived metric error, got: {err}"
        );
    }
}
