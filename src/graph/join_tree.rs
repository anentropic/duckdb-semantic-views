//! The rooted parent tree of a validated relationship graph.
//!
//! §6.2 move 5 (E-7, code-review 2026-07-11): the fan-trap safety check, the
//! fact-path validator, and the `SHOW ... DIMENSIONS FOR METRIC` reachability
//! filter each independently rebuilt the *same* child→parent map from
//! [`RelationshipGraph::reverse`] (the map was literally built three times —
//! twice in `fan_trap.rs`, once in `ddl/show_dims_for_metric.rs`) and carried
//! their own copies of the ancestor / ancestor-path walks over it. [`JoinTree`]
//! owns that parent tree and those walks once.
//!
//! Two of those three consumers have since moved off it, because both needed a
//! path's fan-out DIRECTION and ancestry does not carry it: the fan-trap fence
//! in v0.12.0, and the fact-path validator when TECH-DEBT #37 was fixed. The
//! sole remaining consumer is the `extension`-gated `SHOW ... DIMENSIONS FOR
//! METRIC` filter, which pairs this tree with its own cardinality map — so in
//! the default build the type is dead, like the rest of that FFI path.
//!
//! Scope: this is the rooted parent tree — each non-root alias mapped to its
//! neighbor toward the base table, derived by BFS over the relationship edges
//! taken as undirected (the same walk `expand::join_resolver`'s
//! `build_tree_parents` performs, which is why it also spans the
//! FK-side-of-root, SG-10). It answers ANCESTRY questions only; it carries no
//! cardinality, so a chain running through it says nothing about whether
//! traversing it would fan out. Callers that need that must check edge
//! direction themselves — `fan_trap`'s `find_path` + `fanning_edge_on_path`
//! pair is the one that does.

use std::collections::{HashMap, HashSet, VecDeque};

use super::relationship::RelationshipGraph;

/// The rooted parent tree of a validated relationship graph: each non-root
/// alias mapped to its single parent, the neighbor toward the base table.
///
/// Scope note (v0.12.0): this tree answers *ancestry* questions —
/// "is A on the path between B and the base table?" — and nothing else. Finding
/// the join **path** between two arbitrary tables is not done here: the
/// fan-trap fence walks the undirected relationship graph
/// (`expand::fan_trap::GrainGraph`) because two sibling children of one table
/// have no ancestor relationship at all, so a parent-chain walk finds no path
/// between them.
///
/// Ancestry is NOT safety. Because the tree is rooted at the base table, the
/// base table is an ancestor of every other alias — including aliases whose
/// path back to it traverses a many-to-one edge backwards. Anything deciding
/// whether a join multiplies rows must inspect edge direction as well.
pub(crate) struct JoinTree {
    parent: HashMap<String, String>,
}

impl JoinTree {
    /// Derive the parent tree by breadth-first search outward from the base
    /// table, over the relationship edges taken as UNDIRECTED — so each node's
    /// parent is its neighbour *toward* the base table. This is the same walk
    /// `expand::join_resolver::build_tree_parents` performs for join emission.
    ///
    /// TECH-DEBT #37 (fixed): this used to take `graph.reverse[node].first()` —
    /// the first table that *references* `node`. That coincides with the
    /// neighbour-toward-the-base-table only when the base table is the sole
    /// FK-holder at every fan-in point. With two children of one parent (say
    /// `line_items` and `shipments` both referencing `orders`) `reverse["orders"]`
    /// has two entries, so `orders` was given whichever child was declared
    /// first; if that was not the base table, the base table hung off its own
    /// sibling and ancestry chains ran through the root and out the far branch.
    /// `s -> o -> c` then looked unrelated to `s`, which surfaced as a spurious
    /// `FactPathViolation` and as dimensions missing from
    /// `SHOW SEMANTIC DIMENSIONS ... FOR METRIC`.
    ///
    /// Nodes with no path to the declared base table keep the legacy
    /// reverse-edge parent: a definition whose base table no relationship
    /// mentions is degenerate but accepted, and `join_resolver` already falls
    /// back the same way rather than dropping such aliases.
    #[cfg_attr(not(feature = "extension"), allow(dead_code))]
    pub(crate) fn from_graph(graph: &RelationshipGraph) -> Self {
        let mut parent: HashMap<String, String> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(graph.root.clone());
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(graph.root.clone());

        // Neighbours are visited FK-side first, then PK-side, each in
        // declaration order, so the derived tree is deterministic.
        while let Some(current) = queue.pop_front() {
            let outgoing = graph.edges.get(&current).into_iter().flatten();
            let incoming = graph.reverse.get(&current).into_iter().flatten();
            for neighbor in outgoing.chain(incoming) {
                if visited.insert(neighbor.clone()) {
                    parent.insert(neighbor.clone(), current.clone());
                    queue.push_back(neighbor.clone());
                }
            }
        }

        // Legacy derivation for anything the walk could not reach.
        for (child, parents) in &graph.reverse {
            if visited.contains(child) {
                continue;
            }
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
    #[cfg_attr(not(feature = "extension"), allow(dead_code))]
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
    use crate::graph::test_helpers::make_def;

    /// TECH-DEBT #37: with fan-in onto the base table (`li` and `s` both
    /// referencing the base table `o`) the old derivation took
    /// `reverse["o"].first()` — the first-declared child — as `o`'s parent. The
    /// root then hung off one of its own children and every chain ran through
    /// the root and out the far branch, so `c`'s chain was `[c, o, li]` and
    /// never reached `s`. The parent map must be rooted at the base table.
    ///
    /// Fan-in onto the root is the only legal fan-in: `check_no_diamonds`
    /// rejects any other multi-parent node as an ambiguous join diamond.
    #[test]
    fn from_graph_roots_the_tree_at_the_base_table_under_fan_in() {
        // Base table `o`; the sibling edge `li -> o` is declared FIRST.
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("li", "line_items", vec!["id"]),
                ("s", "shipments", vec!["id"]),
                ("c", "customers", vec!["id"]),
            ],
            vec![
                ("li", "o", vec!["order_id"]),
                ("s", "o", vec!["order_id"]),
                ("o", "c", vec!["customer_id"]),
            ],
            vec![],
            vec![],
        );
        let graph = RelationshipGraph::from_definition(&def).unwrap();
        let tree = JoinTree::from_graph(&graph);

        // The base table is the root: it has no parent, and its chain is itself.
        assert_eq!(
            tree.parent_of("o"),
            None,
            "the base table must have no parent"
        );
        assert_eq!(tree.ancestors_to_root("o"), vec!["o"]);

        // Every other node's chain terminates at the base table.
        assert_eq!(tree.ancestors_to_root("li"), vec!["li", "o"]);
        assert_eq!(tree.ancestors_to_root("s"), vec!["s", "o"]);
        assert_eq!(tree.ancestors_to_root("c"), vec!["c", "o"]);
    }

    /// Nodes with no path to the declared base table keep the legacy
    /// reverse-edge parent, so degenerate definitions (a base table that no
    /// relationship mentions) expand exactly as they did before.
    #[test]
    fn from_graph_falls_back_to_reverse_edges_for_unreachable_nodes() {
        // `orders` is the base table but no relationship mentions it; the real
        // chain is d -> li -> o, disconnected from the root.
        let def = make_def(
            vec![
                ("orders", "orders", vec![]),
                ("o", "orders", vec!["id"]),
                ("li", "line_items", vec!["id"]),
                ("d", "details", vec!["id"]),
            ],
            vec![("li", "o", vec!["order_id"]), ("d", "li", vec!["line_id"])],
            vec![],
            vec![],
        );
        let graph = RelationshipGraph::from_definition(&def).unwrap();
        let tree = JoinTree::from_graph(&graph);

        assert_eq!(tree.ancestors_to_root("o"), vec!["o", "li", "d"]);
        assert_eq!(tree.ancestors_to_root("d"), vec!["d"]);
    }

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
        // the shape that looped forever, and `ancestors_to_root` covers it
        // above. This test is now the ONLY guard on that termination: EXP-15
        // made `build_relationship_graph` reject cyclic definitions, so
        // `expand::tests_fan_trap::cyclic_relationships_are_rejected_by_expand`
        // errors at the fence and no longer reaches these walks.
    }
}
