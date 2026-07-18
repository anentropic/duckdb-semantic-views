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
pub(crate) struct JoinTree {
    root: String,
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
        Self {
            root: graph.root.clone(),
            parent,
        }
    }

    /// The root (base-table) alias.
    pub(crate) fn root(&self) -> &str {
        &self.root
    }

    /// This alias's parent (the neighbor toward the root), if any. Used by the
    /// `extension`-gated `SHOW ... DIMENSIONS FOR METRIC` reachability walk; dead
    /// in the default build, like the rest of that FFI path.
    #[cfg_attr(not(feature = "extension"), allow(dead_code))]
    pub(crate) fn parent_of(&self, node: &str) -> Option<&String> {
        self.parent.get(node)
    }

    /// Walk from `node` to the root, returning the chain including `node`
    /// itself (`[node, parent, …, root]`; the last element is the root).
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

    /// Build the chain `[start, parent, …, ancestor]` by walking toward the
    /// root. Stops early (returning a partial chain) if the parent chain breaks
    /// before reaching `ancestor`.
    pub(crate) fn path_to_ancestor(&self, start: &str, ancestor: &str) -> Vec<String> {
        let mut path = vec![start.to_string()];
        let mut current = start.to_string();
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(current.clone());
        while current != ancestor {
            let Some(parent) = self.parent.get(&current) else {
                break;
            };
            // Cyclic parent map guard (see `ancestors_to_root` / issue #141):
            // if `ancestor` is not on the chain, a cyclic map would loop forever.
            if !seen.insert(parent.clone()) {
                break;
            }
            path.push(parent.clone());
            current = parent.clone();
        }
        path
    }

    /// Build the path from `ancestor` down to `target`: `[ancestor, …,
    /// target]`. Walks `target` UP to `ancestor` (a prefix of `target`'s full
    /// ancestor chain — it stops at `ancestor` rather than continuing to the
    /// root) and reverses, so callers that already hold `target`'s ancestor
    /// chain don't pay for a second full walk to the root. Falls back to
    /// `[ancestor, target]` when `ancestor` is not on `target`'s parent chain.
    pub(crate) fn path_from_ancestor_to_node(&self, ancestor: &str, target: &str) -> Vec<String> {
        let up = self.path_to_ancestor(target, ancestor);
        if up.last().is_some_and(|last| last == ancestor) {
            up.into_iter().rev().collect()
        } else {
            vec![ancestor.to_string(), target.to_string()]
        }
    }
}

#[cfg(test)]
impl JoinTree {
    /// Build directly from a root + parent map (test-only; production always
    /// derives via [`Self::from_graph`]).
    fn from_parts(root: &str, parent: HashMap<String, String>) -> Self {
        Self {
            root: root.to_string(),
            parent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ancestors_to_root_at_root() {
        let tree = JoinTree::from_parts("root", HashMap::new());
        assert_eq!(tree.ancestors_to_root("root"), vec!["root"]);
    }

    #[test]
    fn ancestors_to_root_single_parent() {
        let mut parent = HashMap::new();
        parent.insert("child".to_string(), "root".to_string());
        let tree = JoinTree::from_parts("root", parent);
        assert_eq!(tree.ancestors_to_root("child"), vec!["child", "root"]);
    }

    #[test]
    fn ancestors_to_root_multi_level() {
        let mut parent = HashMap::new();
        parent.insert("leaf".to_string(), "mid".to_string());
        parent.insert("mid".to_string(), "root".to_string());
        let tree = JoinTree::from_parts("root", parent);
        assert_eq!(tree.ancestors_to_root("leaf"), vec!["leaf", "mid", "root"]);
    }

    #[test]
    fn path_to_ancestor_walks_up() {
        let mut parent = HashMap::new();
        parent.insert("leaf".to_string(), "mid".to_string());
        parent.insert("mid".to_string(), "root".to_string());
        let tree = JoinTree::from_parts("root", parent);
        assert_eq!(tree.path_to_ancestor("leaf", "mid"), vec!["leaf", "mid"]);
        assert_eq!(
            tree.path_to_ancestor("leaf", "root"),
            vec!["leaf", "mid", "root"]
        );
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
        let tree = JoinTree::from_parts("a", parent);

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

        // `ancestor` deliberately absent from the chain, so a naive walk never
        // reaches it and loops the cycle forever.
        let path = tree.path_to_ancestor("b", "not-on-chain");
        assert!(
            path.len() <= 2,
            "path_to_ancestor did not stop at the cycle: {path:?}"
        );
    }

    #[test]
    fn path_from_ancestor_to_node_walks_down() {
        let mut parent = HashMap::new();
        parent.insert("leaf".to_string(), "mid".to_string());
        parent.insert("mid".to_string(), "root".to_string());
        let tree = JoinTree::from_parts("root", parent);
        assert_eq!(
            tree.path_from_ancestor_to_node("root", "leaf"),
            vec!["root", "mid", "leaf"]
        );
        // Not an ancestor -> direct two-element fallback.
        assert_eq!(
            tree.path_from_ancestor_to_node("sibling", "leaf"),
            vec!["sibling", "leaf"]
        );
    }
}
