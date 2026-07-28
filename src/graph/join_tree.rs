//! The rooted parent tree of a validated relationship graph.
//!
//! §6.2 move 5 (E-7, code-review 2026-07-11): the fan-trap safety check, the
//! fact-path validator, and the `SHOW ... DIMENSIONS FOR METRIC` reachability
//! filter each independently rebuilt the *same* child→parent map from
//! [`RelationshipGraph::reverse`] (the map was literally built three times —
//! twice in `fan_trap.rs`, once in `ddl/show_dims_for_metric.rs`) and carried
//! their own copies of the ancestor / ancestor-path walks over it. [`JoinTree`]
//! owns that directed parent tree and those walks once.
//!
//! Scope: this is the DIRECTED FK parent tree (each non-root alias → the
//! neighbor toward the root along an FK edge). The UNDIRECTED traversals —
//! `expand::join_resolver`'s BFS `build_tree_parents` (which also spans the
//! FK-side-of-root, SG-10) and `fan_trap`'s `find_path` adjacency BFS — are a
//! genuinely different tree and are intentionally NOT folded in here: they walk
//! different edges, and merging them would change which path the fan-trap check
//! inspects.

use std::collections::{HashMap, HashSet};

use super::relationship::RelationshipGraph;

/// The directed parent tree of a validated relationship graph: each non-root
/// alias mapped to its single parent (the first reverse edge — in a validated
/// tree each non-root node has exactly one, and role-playing multi-edges to one
/// node all share the same parent table).
///
/// Scope note (v0.12.0): this tree answers *ancestry* questions —
/// "is A on the path between B and the base table?" — and nothing else. Finding
/// the join **path** between two arbitrary tables is no longer done here: the
/// fan-trap fence walks the undirected relationship graph
/// (`expand::fan_trap::GrainGraph`) because two sibling children of one table
/// have no ancestor relationship at all, so a parent-chain walk finds no path
/// between them (see TECH-DEBT #37 for the same weakness in the remaining
/// ancestry consumers).
pub(crate) struct JoinTree {
    parent: HashMap<String, String>,
}

impl JoinTree {
    /// Derive the parent tree from a relationship graph's reverse adjacency.
    /// This is the map that `fan_trap` and `show_dims_for_metric` previously
    /// built inline (identically).
    pub(crate) fn from_graph(graph: &RelationshipGraph) -> Self {
        let mut parent: HashMap<String, String> = HashMap::new();
        for (child, parents) in &graph.reverse {
            if let Some(p) = parents.first() {
                parent.insert(child.clone(), p.clone());
            }
        }
        Self { parent }
    }

    /// This alias's parent (the neighbor toward the root), if any. Used by the
    /// `extension`-gated `SHOW ... DIMENSIONS FOR METRIC` reachability walk; dead
    /// in the default build, like the rest of that FFI path.
    #[cfg_attr(not(feature = "extension"), allow(dead_code))]
    pub(crate) fn parent_of(&self, node: &str) -> Option<&String> {
        self.parent.get(node)
    }

    /// Walk from `node` to the root, returning the chain including `node`
    /// itself (`[node, parent, …, root]`). In a well-formed acyclic tree the
    /// last element is the root; on a malformed CYCLIC parent map the walk stops
    /// at the first revisited node, so the chain may end before the root (#141).
    pub(crate) fn ancestors_to_root(&self, node: &str) -> Vec<String> {
        let mut chain = vec![node.to_string()];
        let mut current = node.to_string();
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(current.clone());
        while let Some(parent) = self.parent.get(&current) {
            // A validated relationship tree has an acyclic parent map, but a
            // MALFORMED cyclic definition (a -> b -> a) yields a cyclic map here
            // — stop at the first repeat so the walk is total instead of looping
            // forever (issue #141: this was an unbounded `Vec` push → OOM in
            // `check_fan_traps`).
            if !seen.insert(parent.clone()) {
                break;
            }
            chain.push(parent.clone());
            current = parent.clone();
        }
        chain
    }
}

#[cfg(test)]
impl JoinTree {
    /// Build directly from a parent map (test-only; production always derives
    /// via [`Self::from_graph`]).
    fn from_parts(parent: HashMap<String, String>) -> Self {
        Self { parent }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestors_to_root_at_root() {
        let tree = JoinTree::from_parts(HashMap::new());
        assert_eq!(tree.ancestors_to_root("root"), vec!["root"]);
    }

    #[test]
    fn ancestors_to_root_single_parent() {
        let mut parent = HashMap::new();
        parent.insert("child".to_string(), "root".to_string());
        let tree = JoinTree::from_parts(parent);
        assert_eq!(tree.ancestors_to_root("child"), vec!["child", "root"]);
    }

    #[test]
    fn ancestors_to_root_multi_level() {
        let mut parent = HashMap::new();
        parent.insert("leaf".to_string(), "mid".to_string());
        parent.insert("mid".to_string(), "root".to_string());
        let tree = JoinTree::from_parts(parent);
        assert_eq!(tree.ancestors_to_root("leaf"), vec!["leaf", "mid", "root"]);
    }

    #[test]
    fn walks_terminate_on_cyclic_parent_map() {
        // #141: a cyclic relationship graph (a -> b -> a) yields a cyclic
        // child->parent map. The parent-chain walks must TERMINATE — before the
        // fix they looped forever, pushing to the chain Vec until OOM (a hang in
        // check_fan_traps). A malformed cyclic map is not a validated tree, so we
        // only require the walks to stop with no node repeated, not a specific
        // ordering.
        let mut parent = HashMap::new();
        parent.insert("a".to_string(), "b".to_string());
        parent.insert("b".to_string(), "a".to_string());
        let tree = JoinTree::from_parts(parent);

        let chain = tree.ancestors_to_root("b");
        assert!(
            chain.len() <= 2,
            "ancestors_to_root did not stop at the cycle: {chain:?}"
        );
        let mut deduped = chain.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(
            deduped.len(),
            chain.len(),
            "cycle revisited a node: {chain:?}"
        );

        // The walk from a node whose chain never reaches a given ancestor is
        // the shape that looped forever: `ancestors_to_root` covers it above,
        // and the end-to-end guard is
        // `expand::tests_fan_trap::cyclic_relationships_do_not_hang_expand`.
    }
}
