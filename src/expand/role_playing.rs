use crate::model::SemanticViewDefinition;

use super::facts::collect_derived_metric_using;
use super::types::ExpandError;

/// Determine which relationships point to a given table alias in the definition.
///
/// Returns a list of relationship names that have `to_alias` as their target.
/// Used for ambiguity detection: if a table is reached by multiple named relationships,
/// dimensions from that table require USING context to disambiguate.
pub(super) fn relationships_to_table(
    def: &SemanticViewDefinition,
    target_alias: &str,
) -> Vec<String> {
    let target_lower = target_alias.to_ascii_lowercase();
    def.joins
        .iter()
        .filter(|j| !j.fk_columns.is_empty() && j.table.to_ascii_lowercase() == target_lower)
        .filter_map(|j| j.name.clone())
        .collect()
}

/// Determine the scoped alias for a dimension from a role-playing table.
///
/// Checks whether the dimension's `source_table` is reached by multiple relationships.
/// If so, looks at co-queried metrics' `using_relationships` to determine which
/// relationship (and thus which scoped alias) to use for the dimension.
///
/// Returns:
/// - `Ok(None)` if the dimension's table is not a role-playing table (single or no relationship)
/// - `Ok(Some(scoped_alias))` if exactly one USING path disambiguates
/// - `Err(ExpandError::AmbiguousPath)` if ambiguous with no single USING context
#[allow(clippy::result_large_err)]
pub(super) fn find_using_context(
    view_name: &str,
    def: &SemanticViewDefinition,
    dim: &crate::model::Dimension,
    resolved_mets: &[&crate::model::Metric],
) -> Result<Option<String>, ExpandError> {
    let Some(ref dim_table) = dim.source_table else {
        return Ok(None); // No source table -> base table, no scoping needed
    };
    let dim_table_lower = dim_table.to_ascii_lowercase();

    // Find all relationships pointing to this table
    let rels = relationships_to_table(def, &dim_table_lower);
    if rels.len() <= 1 {
        return Ok(None); // Single or no relationship -> unambiguous, use bare alias
    }

    // Multiple relationships -> role-playing table. Look for USING context.
    // Collect all USING relationships from co-queried metrics that target this table.
    let mut using_rels_for_table: Vec<String> = Vec::new();
    for met in resolved_mets {
        for using_rel in &met.using_relationships {
            // Check if this USING relationship targets our dimension's table
            let using_rel_lower = using_rel.to_ascii_lowercase();
            let targets_our_table = def.joins.iter().any(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.to_ascii_lowercase() == using_rel_lower)
                    && j.table.to_ascii_lowercase() == dim_table_lower
            });
            if targets_our_table && !using_rels_for_table.contains(&using_rel_lower) {
                using_rels_for_table.push(using_rel_lower);
            }
        }
        // Also check derived metrics: walk their transitive dependencies
        if met.source_table.is_none() {
            let transitive_using = collect_derived_metric_using(met, &def.metrics);
            for using_rel in transitive_using {
                let using_rel_lower = using_rel.to_ascii_lowercase();
                let targets_our_table = def.joins.iter().any(|j| {
                    j.name
                        .as_ref()
                        .is_some_and(|n| n.to_ascii_lowercase() == using_rel_lower)
                        && j.table.to_ascii_lowercase() == dim_table_lower
                });
                if targets_our_table && !using_rels_for_table.contains(&using_rel_lower) {
                    using_rels_for_table.push(using_rel_lower);
                }
            }
        }
    }

    if using_rels_for_table.len() == 1 {
        // Exactly one USING path disambiguates -> return scoped alias
        let scoped = format!("{dim_table_lower}__{}", using_rels_for_table[0]);
        Ok(Some(scoped))
    } else {
        // Zero or multiple USING paths -> ambiguous
        Err(ExpandError::AmbiguousPath {
            view_name: view_name.to_string(),
            dimension_name: dim.name.clone(),
            dimension_table: dim_table_lower,
            available_relationships: rels,
        })
    }
}
