use std::collections::HashMap;

use crate::model::{Cardinality, SemanticViewDefinition};

use super::facts::collect_derived_metric_source_tables;
use super::types::ExpandError;

/// Check for fan traps: a metric aggregating across a one-to-many boundary.
///
/// Walks the join path between each (metric source, dimension source) pair
/// and checks whether any edge is traversed in the fan-out direction.
/// Returns `Err(ExpandError::FanTrap)` with details if a fan-out is detected.
///
/// # Fan-out direction
///
/// For an edge `(from_alias, to_alias)` with cardinality:
/// - `ManyToOne`: from->to is safe (many go to one), to->from is fan-out
/// - `OneToOne`: both directions are safe
#[allow(clippy::result_large_err)]
pub(super) fn check_fan_traps(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
) -> Result<(), ExpandError> {
    if def.joins.is_empty() {
        return Ok(());
    }

    let Ok(graph) = crate::graph::RelationshipGraph::from_definition(def) else {
        return Ok(()); // Graph was validated at define time
    };

    // Build cardinality map: (from_lower, to_lower) -> (Cardinality, relationship_name)
    let card_map: HashMap<(String, String), (Cardinality, String)> = def
        .joins
        .iter()
        .filter(|j| !j.fk_columns.is_empty())
        .map(|j| {
            let rel_name = j.name.as_deref().unwrap_or(&j.from_alias).to_string();
            (
                (
                    j.from_alias.to_ascii_lowercase(),
                    j.table.to_ascii_lowercase(),
                ),
                (j.cardinality, rel_name),
            )
        })
        .collect();

    // Build parent map for tree path finding.
    // In a validated tree, each non-root node has exactly one parent via the reverse map.
    let mut parent_map: HashMap<String, String> = HashMap::new();
    for (child, parents) in &graph.reverse {
        if let Some(parent) = parents.first() {
            parent_map.insert(child.clone(), parent.clone());
        }
    }

    // For each metric + dimension pair, check for fan-out on the join path.
    for met in resolved_mets {
        // Get source tables for this metric
        let met_tables: Vec<String> = if let Some(ref st) = met.source_table {
            vec![st.to_ascii_lowercase()]
        } else {
            // Derived metric: walk dependency graph to find transitive base metric source tables
            collect_derived_metric_source_tables(met, &def.metrics)
                .into_iter()
                .map(|s| s.to_ascii_lowercase())
                .collect()
        };

        for dim in resolved_dims {
            let Some(ref dim_table_raw) = dim.source_table else {
                continue; // No source table -> base table dim, skip
            };
            let dim_table = dim_table_raw.to_ascii_lowercase();

            for met_table in &met_tables {
                if *met_table == dim_table {
                    continue; // Same table, no fan-out possible
                }

                // Find path from met_table to dim_table through the tree.
                // Walk both up to root to get ancestor chains, then derive path.
                let met_ancestors = ancestors_to_root(met_table, &parent_map);
                let dim_ancestors = ancestors_to_root(&dim_table, &parent_map);

                // Find the lowest common ancestor (LCA)
                let dim_ancestor_set: std::collections::HashSet<&String> =
                    dim_ancestors.iter().collect();
                let lca = met_ancestors
                    .iter()
                    .find(|a| dim_ancestor_set.contains(a))
                    .cloned();

                let Some(lca) = lca else {
                    continue; // No common ancestor (shouldn't happen in a tree)
                };

                // Build path: met_table -> ... -> LCA -> ... -> dim_table
                // Check edges from met_table up to LCA
                if let Some(err) =
                    check_path_up(met_table, &lca, &parent_map, &card_map, view_name, met, dim)
                {
                    return Err(err);
                }
                // Check edges from LCA down to dim_table
                // We need the path from LCA down to dim_table. Build it from dim_ancestors.
                let path_down = path_from_ancestor_to_node(&lca, &dim_table, &dim_ancestors);
                if let Some(err) = check_path_down(&path_down, &card_map, view_name, met, dim) {
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

/// Walk from `node` to the root through the parent map, returning the chain
/// including `node` itself. The last element is the root.
pub(crate) fn ancestors_to_root(node: &str, parent_map: &HashMap<String, String>) -> Vec<String> {
    let mut chain = vec![node.to_string()];
    let mut current = node.to_string();
    while let Some(parent) = parent_map.get(&current) {
        chain.push(parent.clone());
        current = parent.clone();
    }
    chain
}

/// Build the path from an ancestor down to a target node, given the target's ancestor chain.
/// Returns a vec starting at `ancestor` and ending at `target`.
fn path_from_ancestor_to_node(
    ancestor: &str,
    target: &str,
    target_ancestors: &[String],
) -> Vec<String> {
    // target_ancestors is [target, parent, grandparent, ..., root]
    // Find ancestor in this chain and take the sub-chain, reversed.
    if let Some(pos) = target_ancestors.iter().position(|a| a == ancestor) {
        let mut path: Vec<String> = target_ancestors[..=pos].to_vec();
        path.reverse();
        path
    } else {
        vec![ancestor.to_string(), target.to_string()]
    }
}

/// Check edges going UP from `start` to `ancestor` (toward root).
/// Walking up means: at each step, current -> parent. The actual edge in the
/// graph might be current->parent (forward edge) or parent->current (forward edge).
///
/// Returns `None` if no fan-out, `Some(ExpandError)` if fan-out detected.
fn check_path_up(
    start: &str,
    ancestor: &str,
    parent_map: &HashMap<String, String>,
    card_map: &HashMap<(String, String), (Cardinality, String)>,
    view_name: &str,
    met: &crate::model::Metric,
    dim: &crate::model::Dimension,
) -> Option<ExpandError> {
    let mut current = start.to_string();
    while current != ancestor {
        let Some(parent) = parent_map.get(&current) else {
            break;
        };
        // Determine which direction this edge goes in the card_map.
        // The graph stores edges as from_alias -> to_alias (FK -> PK).
        // So either (current, parent) or (parent, current) is in the map.
        if let Some((_card, _rel_name)) = card_map.get(&(current.clone(), parent.clone())) {
            // Edge is current -> parent (current has FK pointing to parent)
            // Walking current -> parent: this is the forward direction of the edge.
            // ManyToOne forward = safe, OneToOne = safe -- no fan-out possible going forward
        } else if let Some((card, rel_name)) = card_map.get(&(parent.clone(), current.clone())) {
            // Edge is parent -> current (parent has FK pointing to current)
            // Walking current -> parent means traversing this edge in REVERSE.
            // ManyToOne reverse = fan-out, OneToOne = safe
            if *card == Cardinality::ManyToOne {
                let met_table = met.source_table.as_deref().unwrap_or(&current);
                return Some(ExpandError::FanTrap {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    metric_table: met_table.to_string(),
                    dimension_name: dim.name.clone(),
                    dimension_table: dim.source_table.as_deref().unwrap_or("").to_string(),
                    relationship_name: rel_name.clone(),
                });
            }
        }
        current = parent.clone();
    }
    None
}

/// Check edges going DOWN a path (from ancestor toward target).
/// The path is [ancestor, ..., target]. For each consecutive pair (a, b),
/// check the traversal direction vs cardinality.
///
/// Returns `None` if no fan-out, `Some(ExpandError)` if fan-out detected.
fn check_path_down(
    path: &[String],
    card_map: &HashMap<(String, String), (Cardinality, String)>,
    view_name: &str,
    met: &crate::model::Metric,
    dim: &crate::model::Dimension,
) -> Option<ExpandError> {
    for window in path.windows(2) {
        let a = &window[0];
        let b = &window[1];
        // Walking a -> b (downward in the tree, away from root)
        if let Some((_card, _rel_name)) = card_map.get(&(a.clone(), b.clone())) {
            // Edge is a -> b (a has FK pointing to b)
            // Walking a -> b: forward direction
            // ManyToOne forward = safe, OneToOne = safe -- no fan-out possible going forward
        } else if let Some((card, rel_name)) = card_map.get(&(b.clone(), a.clone())) {
            // Edge is b -> a (b has FK pointing to a)
            // Walking a -> b means traversing this edge in REVERSE.
            // ManyToOne reverse = fan-out, OneToOne = safe
            if *card == Cardinality::ManyToOne {
                let met_table = met.source_table.as_deref().unwrap_or("").to_string();
                return Some(ExpandError::FanTrap {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    metric_table: met_table,
                    dimension_name: dim.name.clone(),
                    dimension_table: dim.source_table.as_deref().unwrap_or("").to_string(),
                    relationship_name: rel_name.clone(),
                });
            }
        }
    }
    None
}
