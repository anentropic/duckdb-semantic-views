use std::collections::HashSet;

use crate::model::SemanticViewDefinition;

use super::facts::collect_derived_metric_using;
use super::join_resolver::scoped_join_alias;
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

/// Is `target_alias` a genuinely *role-playing* target — i.e. does any single
/// source table declare two or more named relationships to it (e.g.
/// `orders(dep_code) REFERENCES airports` + `orders(arr_code) REFERENCES
/// airports`)? Only then can one query legitimately need multiple aliased
/// instances of the target, making an unqualified dimension on it ambiguous.
///
/// Multiple relationships converging from *different* source tables
/// (`li -> o`, `p -> o`) are NOT role-playing: the target joins as a single
/// bare instance and path resolution picks the unique connecting edge for
/// whichever source the query actually reaches it from. (Queries that pull
/// in both divergent sources are rejected separately by path validation /
/// fan-trap checks.) Treating that shape as ambiguous broke plain
/// child-fact + parent-dimension queries.
pub(super) fn is_role_playing_target(def: &SemanticViewDefinition, target_alias: &str) -> bool {
    let target_lower = target_alias.to_ascii_lowercase();
    let mut seen_from: HashSet<String> = HashSet::new();
    for j in &def.joins {
        if j.fk_columns.is_empty()
            || j.name.is_none()
            || j.table.to_ascii_lowercase() != target_lower
        {
            continue;
        }
        // A second relationship from the same source table to this target is
        // what makes it role-playing. `insert` returns false when the alias
        // was already present — linear time, no O(n²) `Vec::contains` scan.
        if !seen_from.insert(j.from_alias.to_ascii_lowercase()) {
            return true;
        }
    }
    false
}

/// The role-playing target on the join path from `table` up to the root
/// (inclusive of `table` itself), if any — the lowercased alias of a table that
/// `table` *is*, or is reachable only *through*. Such a table has an ambiguous
/// join path: which of the role-playing target's several relationship instances
/// does it hang off? Used to reject dimensions on descendants of a role-playing
/// table (EXP-4) and facts on/through one (EXP-5), one hop past the direct
/// `AmbiguousPath` case.
///
/// The relationship graph is a tree apart from the sanctioned role-playing
/// multi-edge (cross-source diamonds are rejected at define time), so each
/// non-root, non-role-playing node has exactly one inbound edge — the first
/// reverse entry is its parent toward the root.
///
/// A definition whose relationship graph cannot be rebuilt (missing FK metadata
/// or otherwise malformed), or one that contains a cycle, is reported as
/// [`ExpandError::UncheckableDefinition`] — the same fail-loud stance
/// `fan_trap::build_relationship_graph` takes — rather than silently treated as
/// "no role-playing on path", which would re-open the declaration-order-
/// dependent mis-binding this check exists to close.
pub(super) fn role_playing_on_path(
    view_name: &str,
    def: &SemanticViewDefinition,
    table: &str,
) -> Result<Option<String>, ExpandError> {
    let table_lower = table.to_ascii_lowercase();
    if is_role_playing_target(def, &table_lower) {
        return Ok(Some(table_lower));
    }
    if def.has_incomplete_relationships() {
        return Err(ExpandError::UncheckableDefinition {
            view_name: view_name.to_string(),
            reason: "one or more relationships are missing foreign-key column metadata \
                     (a legacy pre-Phase-24 definition format)"
                .to_string(),
        });
    }
    let graph = crate::graph::RelationshipGraph::from_definition(def).map_err(|reason| {
        ExpandError::UncheckableDefinition {
            view_name: view_name.to_string(),
            reason,
        }
    })?;
    let mut current = table_lower;
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(current.clone());
    while current != graph.root {
        // A non-root, non-role-playing node has exactly one inbound edge; no
        // reverse entry means it is disconnected from the root (an orphan,
        // rejected at define time) — no role-playing ancestor to find.
        let Some(parent) = graph.reverse.get(&current).and_then(|p| p.first()) else {
            return Ok(None);
        };
        let parent = parent.to_ascii_lowercase();
        if !visited.insert(parent.clone()) {
            // A cycle in the relationship graph is a definition-validity
            // problem; fail loudly rather than skip the ambiguity check.
            return Err(ExpandError::UncheckableDefinition {
                view_name: view_name.to_string(),
                reason: "relationship graph contains a cycle".to_string(),
            });
        }
        if is_role_playing_target(def, &parent) {
            return Ok(Some(parent));
        }
        current = parent;
    }
    Ok(None)
}

/// Reject a fact whose source table is (or is reached only through) a
/// role-playing table (EXP-5). Facts carry no `USING` context, so the role is
/// unresolvable; erroring beats silently binding to the first-declared
/// relationship.
pub(super) fn check_fact_role_playing_path(
    view_name: &str,
    def: &SemanticViewDefinition,
    fact: &crate::model::Fact,
) -> Result<(), ExpandError> {
    let Some(ref fact_table) = fact.source_table else {
        return Ok(());
    };
    if let Some(rp) = role_playing_on_path(view_name, def, fact_table)? {
        let available_relationships = relationships_to_table(def, &rp);
        return Err(ExpandError::AmbiguousFactPath {
            view_name: view_name.to_string(),
            fact_name: fact.name.clone(),
            fact_table: fact_table.to_ascii_lowercase(),
            role_playing_table: rp,
            available_relationships,
        });
    }
    Ok(())
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
        // EXP-4: the dimension's own table has a single inbound relationship,
        // but if it is reachable only THROUGH a role-playing table, its join
        // path is ambiguous — and, unlike a dimension directly on the
        // role-playing table, a descendant cannot be scoped by a co-queried
        // metric's USING. Reject it instead of silently binding to the
        // first-declared relationship (a declaration-order-dependent wrong
        // grouping).
        if let Some(rp) = role_playing_on_path(view_name, def, &dim_table_lower)? {
            let available_relationships = relationships_to_table(def, &rp);
            return Err(ExpandError::AmbiguousDescendantPath {
                view_name: view_name.to_string(),
                dimension_name: dim.name.clone(),
                dimension_table: dim_table_lower,
                role_playing_table: rp,
                available_relationships,
            });
        }
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
        let scoped = scoped_join_alias(&dim_table_lower, &using_rels_for_table[0]);
        Ok(Some(scoped))
    } else if using_rels_for_table.is_empty() && !is_role_playing_target(def, &dim_table_lower) {
        // Multiple inbound relationships but each from a DIFFERENT source
        // table, and no USING context in the query: a convergent parent,
        // not a role-playing target. It joins as one bare instance and the
        // path walk picks the unique connecting edge; divergent-source
        // queries are rejected by path validation / fan-trap checks.
        Ok(None)
    } else {
        // No (or conflicting) USING context for a role-playing target ->
        // ambiguous.
        Err(ExpandError::AmbiguousPath {
            view_name: view_name.to_string(),
            dimension_name: dim.name.clone(),
            dimension_table: dim_table_lower,
            available_relationships: rels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Join, SemanticViewDefinition, TableRef};

    /// A two-table definition whose only relationship is missing its FK column
    /// metadata (a legacy pre-Phase-24 row): the graph cannot be safely rebuilt.
    fn incomplete_rel_def() -> SemanticViewDefinition {
        SemanticViewDefinition {
            tables: vec![
                TableRef {
                    alias: "o".to_string(),
                    table: "o".to_string(),
                    ..Default::default()
                },
                TableRef {
                    alias: "c".to_string(),
                    table: "c".to_string(),
                    ..Default::default()
                },
            ],
            joins: vec![Join {
                from_alias: "o".to_string(),
                table: "c".to_string(),
                fk_columns: vec![], // incomplete: missing FK metadata
                ref_columns: vec![],
                name: Some("o_c".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn role_playing_on_path_fails_loudly_on_uncheckable_definition() {
        // Copilot review (#148): a definition whose relationship graph cannot be
        // safely rebuilt must surface `UncheckableDefinition`, not silently
        // report "no role-playing on path" — the latter would re-open the
        // declaration-order-dependent mis-binding this check exists to close.
        let def = incomplete_rel_def();
        let err = role_playing_on_path("v", &def, "c").unwrap_err();
        assert!(
            matches!(err, ExpandError::UncheckableDefinition { .. }),
            "expected UncheckableDefinition, got: {err:?}"
        );
    }

    #[test]
    fn check_fact_role_playing_path_propagates_uncheckable() {
        // The fact-path check surfaces the same fail-loud error (facts have no
        // fan-trap pre-check, so this is their only guard).
        let def = incomplete_rel_def();
        let fact = crate::model::Fact {
            name: "f".to_string(),
            expr: "c.x".to_string(),
            source_table: Some("c".to_string()),
            ..Default::default()
        };
        let err = check_fact_role_playing_path("v", &def, &fact).unwrap_err();
        assert!(
            matches!(err, ExpandError::UncheckableDefinition { .. }),
            "expected UncheckableDefinition, got: {err:?}"
        );
    }
}
