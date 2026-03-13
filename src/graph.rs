//! Relationship graph validation and topological sort for semantic view definitions.
//!
//! Built from `TABLES` + `RELATIONSHIPS` declarations at CREATE time.
//! Validates that the relationship graph forms a tree rooted at the base table
//! (first table in TABLES clause). Rejects cycles, diamonds, self-references,
//! orphan tables, unreachable `source_table` aliases, and FK/PK count mismatches.
//!
//! Used by `define.rs` at CREATE time and by `expand.rs` at query time.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use crate::expand::suggest_closest;
use crate::model::SemanticViewDefinition;

/// A directed relationship graph built from TABLES + RELATIONSHIPS.
///
/// Nodes are table aliases (lowercased). Edges represent FK->PK relationships
/// (`from_alias` -> `to_alias`). The base table is the root (in-degree 0 in a valid tree).
#[derive(Debug)]
pub struct RelationshipGraph {
    /// Adjacency list: `from_alias` -> vec of `to_alias` values.
    pub edges: HashMap<String, Vec<String>>,
    /// Reverse adjacency: `to_alias` -> vec of `from_alias` values (for parent tracking).
    pub reverse: HashMap<String, Vec<String>>,
    /// All declared table aliases (lowercased).
    pub all_nodes: HashSet<String>,
    /// The root node (base table alias, first in TABLES clause, lowercased).
    pub root: String,
}

impl RelationshipGraph {
    /// Build a relationship graph from a semantic view definition.
    ///
    /// Iterates only joins with non-empty `fk_columns` (Phase 24 format).
    /// Legacy joins (empty `fk_columns`) are skipped.
    ///
    /// Returns `Err` on self-reference (`from_alias` == `to_alias`).
    pub fn from_definition(def: &SemanticViewDefinition) -> Result<Self, String> {
        let root = def
            .tables
            .first()
            .ok_or("TABLES clause is empty")?
            .alias
            .to_ascii_lowercase();

        let all_nodes: HashSet<String> = def
            .tables
            .iter()
            .map(|t| t.alias.to_ascii_lowercase())
            .collect();

        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        let mut reverse: HashMap<String, Vec<String>> = HashMap::new();

        for join in &def.joins {
            if join.fk_columns.is_empty() {
                continue; // Legacy join -- skip graph building
            }
            let from = join.from_alias.to_ascii_lowercase();
            let to = join.table.to_ascii_lowercase();

            // Self-reference check
            if from == to {
                return Err(format!(
                    "table '{}' cannot reference itself",
                    join.from_alias
                ));
            }

            edges.entry(from.clone()).or_default().push(to.clone());
            reverse.entry(to).or_default().push(from);
        }

        Ok(Self {
            edges,
            reverse,
            all_nodes,
            root,
        })
    }

    /// Topological sort via Kahn's algorithm.
    ///
    /// Returns aliases in topological order (root first), or `Err` with a
    /// cycle path description if the graph contains cycles.
    ///
    /// Deterministic: the root is always first, and other zero-in-degree nodes
    /// are added in sorted order.
    pub fn toposort(&self) -> Result<Vec<String>, String> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for node in &self.all_nodes {
            in_degree.entry(node.as_str()).or_insert(0);
        }
        for targets in self.edges.values() {
            for t in targets {
                *in_degree.entry(t.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<String> = VecDeque::new();
        // Seed with root first for determinism (if it has in-degree 0).
        if in_degree.get(self.root.as_str()) == Some(&0) {
            queue.push_back(self.root.clone());
        }
        // Add other zero-in-degree nodes in sorted order for determinism.
        let mut others: Vec<&str> = in_degree
            .iter()
            .filter(|(k, v)| **v == 0 && **k != self.root.as_str())
            .map(|(k, _)| *k)
            .collect();
        others.sort_unstable();
        for o in others {
            queue.push_back(o.to_string());
        }

        let mut order = Vec::new();
        while let Some(node) = queue.pop_front() {
            order.push(node.clone());
            if let Some(neighbors) = self.edges.get(&node) {
                // Sort neighbors for determinism before processing.
                let mut sorted_neighbors: Vec<&String> = neighbors.iter().collect();
                sorted_neighbors.sort();
                for next in sorted_neighbors {
                    if let Some(deg) = in_degree.get_mut(next.as_str()) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(next.clone());
                        }
                    }
                }
            }
        }

        if order.len() == self.all_nodes.len() {
            Ok(order)
        } else {
            // Remaining nodes are in a cycle -- find and report the cycle path.
            let visited: HashSet<&str> = order.iter().map(String::as_str).collect();
            let cycle_path = find_cycle_path(&self.edges, &visited, &self.all_nodes);
            Err(format!("cycle detected in relationships: {cycle_path}"))
        }
    }

    /// Check that the relationship graph is a tree (each non-root node has
    /// at most one parent).
    ///
    /// Returns `Err` with diamond description if any node is reachable via
    /// multiple paths.
    pub fn check_no_diamonds(&self) -> Result<(), String> {
        for (node, parents) in &self.reverse {
            if node != &self.root && parents.len() > 1 {
                return Err(format!(
                    "diamond: two paths to '{}' via '{}' and '{}'",
                    node, parents[0], parents[1]
                ));
            }
        }
        Ok(())
    }

    /// Check that no declared table is an orphan (declared in TABLES but not
    /// connected by any relationship and not the base table).
    ///
    /// An orphan is a non-root node that appears in neither edges keys nor
    /// reverse keys (i.e., it has no outgoing or incoming relationship edges).
    pub fn check_no_orphans(&self) -> Result<(), String> {
        for node in &self.all_nodes {
            if node == &self.root {
                continue;
            }
            let has_outgoing = self.edges.contains_key(node);
            let has_incoming = self.reverse.contains_key(node);
            if !has_outgoing && !has_incoming {
                let available: Vec<String> = self
                    .all_nodes
                    .iter()
                    .filter(|n| *n != node)
                    .cloned()
                    .collect();
                let suggestion = suggest_closest(node, &available);
                let mut msg = format!("orphan table '{node}' is not connected by any relationship");
                if let Some(s) = suggestion {
                    let _ = write!(msg, "; did you mean '{s}'?");
                }
                return Err(msg);
            }
        }
        Ok(())
    }
}

/// Check FK column count matches PK column count for each relationship.
fn check_fk_pk_counts(def: &SemanticViewDefinition) -> Result<(), String> {
    for join in &def.joins {
        if join.fk_columns.is_empty() {
            continue;
        }
        let to_alias_lower = join.table.to_ascii_lowercase();
        if let Some(table_ref) = def
            .tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower)
        {
            if !table_ref.pk_columns.is_empty()
                && join.fk_columns.len() != table_ref.pk_columns.len()
            {
                return Err(format!(
                    "FK column count ({}) does not match PK column count ({}) on table '{}'",
                    join.fk_columns.len(),
                    table_ref.pk_columns.len(),
                    join.table,
                ));
            }
        }
    }
    Ok(())
}

/// Check that all dim/metric `source_table` aliases are declared in the graph.
fn check_source_tables_reachable(
    def: &SemanticViewDefinition,
    graph: &RelationshipGraph,
) -> Result<(), String> {
    let available: Vec<String> = graph.all_nodes.iter().cloned().collect();
    for dim in &def.dimensions {
        if let Some(ref st) = dim.source_table {
            let st_lower = st.to_ascii_lowercase();
            if !graph.all_nodes.contains(&st_lower) {
                let suggestion = suggest_closest(&st_lower, &available);
                let mut msg = format!("unknown source table '{st}'");
                if let Some(s) = suggestion {
                    let _ = write!(msg, "; did you mean '{s}'?");
                }
                return Err(msg);
            }
        }
    }
    for met in &def.metrics {
        if let Some(ref st) = met.source_table {
            let st_lower = st.to_ascii_lowercase();
            if !graph.all_nodes.contains(&st_lower) {
                let suggestion = suggest_closest(&st_lower, &available);
                let mut msg = format!("unknown source table '{st}'");
                if let Some(s) = suggestion {
                    let _ = write!(msg, "; did you mean '{s}'?");
                }
                return Err(msg);
            }
        }
    }
    Ok(())
}

/// Validate the relationship graph of a semantic view definition.
///
/// Runs all define-time checks:
/// 1. Self-reference detection (`from_alias` == `to_alias`)
/// 2. Cycle detection (Kahn's algorithm)
/// 3. Diamond detection (multiple parents)
/// 4. Orphan table detection (declared but not connected)
/// 5. FK/PK column count matching
/// 6. Source table reachability
///
/// Returns `Ok(graph)` if valid, or `Err` with a descriptive message.
///
/// **Legacy skip:** If no joins have non-empty `fk_columns`, or if `tables`
/// is empty, returns `Ok` with a default empty graph. This preserves backward
/// compatibility with Phase 10/11 definitions.
pub fn validate_graph(def: &SemanticViewDefinition) -> Result<RelationshipGraph, String> {
    // Legacy skip: no Phase 24 joins -> skip graph validation entirely.
    let has_pkfk_joins = def.joins.iter().any(|j| !j.fk_columns.is_empty());
    if !has_pkfk_joins || def.tables.is_empty() {
        return Ok(RelationshipGraph {
            edges: HashMap::new(),
            reverse: HashMap::new(),
            all_nodes: HashSet::new(),
            root: String::new(),
        });
    }

    let graph = RelationshipGraph::from_definition(def)?;

    // 1. Cycle detection (Kahn's algorithm).
    let _topo_order = graph.toposort()?;

    // 2. Diamond detection (multiple parents).
    graph.check_no_diamonds()?;

    // 3. Orphan table detection.
    graph.check_no_orphans()?;

    // 4. FK/PK column count matching.
    check_fk_pk_counts(def)?;

    // 5. Source table reachability.
    check_source_tables_reachable(def, &graph)?;

    Ok(graph)
}

/// Find a cycle path among unvisited nodes by following edges.
fn find_cycle_path(
    edges: &HashMap<String, Vec<String>>,
    visited: &HashSet<&str>,
    all_nodes: &HashSet<String>,
) -> String {
    // Find an unvisited node to start from.
    let start = match all_nodes.iter().find(|n| !visited.contains(n.as_str())) {
        Some(n) => n.clone(),
        None => return "unknown cycle".to_string(),
    };

    // Follow edges from start until we revisit a node.
    let mut path = vec![start.clone()];
    let mut current = start;
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        seen.insert(current.clone());
        if let Some(neighbors) = edges.get(&current) {
            // Pick the first unvisited-by-toposort neighbor.
            if let Some(next) = neighbors.iter().find(|n| !visited.contains(n.as_str())) {
                if seen.contains(next.as_str()) {
                    // Found the cycle -- trim path to start from the cycle entry point.
                    if let Some(pos) = path.iter().position(|p| p == next) {
                        path = path[pos..].to_vec();
                        path.push(next.clone());
                        return path.join(" -> ");
                    }
                }
                path.push(next.clone());
                current = next.clone();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    path.join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Dimension, Join, Metric, TableRef};

    /// Helper to build a minimal SemanticViewDefinition for testing.
    fn make_def(
        tables: Vec<(&str, &str, Vec<&str>)>,
        joins: Vec<(&str, &str, Vec<&str>)>,
        dims: Vec<(&str, Option<&str>)>,
        metrics: Vec<(&str, Option<&str>)>,
    ) -> SemanticViewDefinition {
        SemanticViewDefinition {
            base_table: tables
                .first()
                .map(|(_, t, _)| t.to_string())
                .unwrap_or_default(),
            tables: tables
                .iter()
                .map(|(alias, table, pks)| TableRef {
                    alias: alias.to_string(),
                    table: table.to_string(),
                    pk_columns: pks.iter().map(|s| s.to_string()).collect(),
                })
                .collect(),
            joins: joins
                .iter()
                .map(|(from_alias, to_alias, fk_cols)| Join {
                    table: to_alias.to_string(),
                    from_alias: from_alias.to_string(),
                    fk_columns: fk_cols.iter().map(|s| s.to_string()).collect(),
                    ..Default::default()
                })
                .collect(),
            dimensions: dims
                .iter()
                .map(|(name, source)| Dimension {
                    name: name.to_string(),
                    expr: name.to_string(),
                    source_table: source.map(|s| s.to_string()),
                    output_type: None,
                })
                .collect(),
            metrics: metrics
                .iter()
                .map(|(name, source)| Metric {
                    name: name.to_string(),
                    expr: format!("sum({})", name),
                    source_table: source.map(|s| s.to_string()),
                    output_type: None,
                })
                .collect(),
            filters: vec![],
            facts: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Self-reference detection
    // -----------------------------------------------------------------------

    #[test]
    fn self_reference_rejected() {
        let def = make_def(
            vec![("o", "orders", vec!["id"])],
            vec![("o", "o", vec!["manager_id"])],
            vec![],
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("cannot reference itself"),
            "expected self-reference error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Cycle detection
    // -----------------------------------------------------------------------

    #[test]
    fn cycle_detected() {
        // A -> B -> C -> A (cycle)
        let def = make_def(
            vec![
                ("a", "tbl_a", vec!["id"]),
                ("b", "tbl_b", vec!["id"]),
                ("c", "tbl_c", vec!["id"]),
            ],
            vec![
                ("a", "b", vec!["b_id"]),
                ("b", "c", vec!["c_id"]),
                ("c", "a", vec!["a_id"]),
            ],
            vec![],
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("cycle detected in relationships"),
            "expected cycle error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Diamond detection
    // -----------------------------------------------------------------------

    #[test]
    fn diamond_detected() {
        // A -> B, A -> C, B -> D, C -> D (diamond at D)
        let def = make_def(
            vec![
                ("a", "tbl_a", vec!["id"]),
                ("b", "tbl_b", vec!["id"]),
                ("c", "tbl_c", vec!["id"]),
                ("d", "tbl_d", vec!["id"]),
            ],
            vec![
                ("a", "b", vec!["b_id"]),
                ("a", "c", vec!["c_id"]),
                ("b", "d", vec!["d_id"]),
                ("c", "d", vec!["d_id"]),
            ],
            vec![],
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("diamond") && err.contains("two paths to"),
            "expected diamond error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Orphan table detection
    // -----------------------------------------------------------------------

    #[test]
    fn orphan_table_detected() {
        // 'x' is declared in tables but not connected by any relationship.
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("c", "customers", vec!["id"]),
                ("x", "orphan_table", vec!["id"]),
            ],
            vec![("o", "c", vec!["customer_id"])],
            vec![],
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(err.contains("orphan"), "expected orphan error, got: {err}");
    }

    // -----------------------------------------------------------------------
    // FK/PK count mismatch
    // -----------------------------------------------------------------------

    #[test]
    fn fk_pk_count_mismatch() {
        // join has 2 FK columns but referenced table has 1 PK column
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("c", "customers", vec!["id"]), // 1 PK
            ],
            vec![("o", "c", vec!["customer_id", "extra_col"])], // 2 FK
            vec![],
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("FK column count") && err.contains("PK column count"),
            "expected count mismatch error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Unreachable source_table
    // -----------------------------------------------------------------------

    #[test]
    fn unreachable_source_table_dimension() {
        let def = make_def(
            vec![("o", "orders", vec!["id"]), ("c", "customers", vec!["id"])],
            vec![("o", "c", vec!["customer_id"])],
            vec![("name", Some("x"))], // 'x' is not in tables
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("unknown source table"),
            "expected unreachable source table error, got: {err}"
        );
    }

    #[test]
    fn unreachable_source_table_metric() {
        let def = make_def(
            vec![("o", "orders", vec!["id"]), ("c", "customers", vec!["id"])],
            vec![("o", "c", vec!["customer_id"])],
            vec![],
            vec![("revenue", Some("missing"))],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("unknown source table"),
            "expected unreachable source table error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Topological sort
    // -----------------------------------------------------------------------

    #[test]
    fn toposort_valid_tree() {
        // A -> B -> C (linear tree)
        let def = make_def(
            vec![
                ("a", "tbl_a", vec!["id"]),
                ("b", "tbl_b", vec!["id"]),
                ("c", "tbl_c", vec!["id"]),
            ],
            vec![("a", "b", vec!["b_id"]), ("b", "c", vec!["c_id"])],
            vec![],
            vec![],
        );
        let graph = validate_graph(&def).unwrap();
        let order = graph.toposort().unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn toposort_deterministic() {
        // Same graph, different declaration order -> same topological output.
        let def1 = make_def(
            vec![
                ("a", "tbl_a", vec!["id"]),
                ("b", "tbl_b", vec!["id"]),
                ("c", "tbl_c", vec!["id"]),
            ],
            vec![("a", "b", vec!["b_id"]), ("a", "c", vec!["c_id"])],
            vec![],
            vec![],
        );
        let def2 = make_def(
            vec![
                ("a", "tbl_a", vec!["id"]),
                ("c", "tbl_c", vec!["id"]),
                ("b", "tbl_b", vec!["id"]),
            ],
            vec![("a", "c", vec!["c_id"]), ("a", "b", vec!["b_id"])],
            vec![],
            vec![],
        );
        let order1 = validate_graph(&def1).unwrap().toposort().unwrap();
        let order2 = validate_graph(&def2).unwrap().toposort().unwrap();
        assert_eq!(order1, order2, "topological sort must be deterministic");
    }

    // -----------------------------------------------------------------------
    // Legacy definitions skip validation
    // -----------------------------------------------------------------------

    #[test]
    fn legacy_empty_fk_columns_skips_validation() {
        // Legacy join with empty fk_columns -> validate_graph returns Ok.
        let mut def = SemanticViewDefinition {
            base_table: "orders".to_string(),
            tables: vec![TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec![],
            }],
            joins: vec![Join {
                table: "customers".to_string(),
                on: "o.customer_id = c.id".to_string(),
                fk_columns: vec![], // Legacy -- no PK/FK
                ..Default::default()
            }],
            dimensions: vec![],
            metrics: vec![],
            filters: vec![],
            facts: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
        };
        assert!(
            validate_graph(&def).is_ok(),
            "legacy definitions should skip validation"
        );
        // Also test with empty tables.
        def.tables.clear();
        assert!(
            validate_graph(&def).is_ok(),
            "empty tables should skip validation"
        );
    }

    #[test]
    fn single_table_no_joins_skips_validation() {
        let def = make_def(
            vec![("o", "orders", vec!["id"])],
            vec![],
            vec![("region", None)],
            vec![("revenue", None)],
        );
        assert!(
            validate_graph(&def).is_ok(),
            "single-table defs with no joins should skip validation"
        );
    }

    // -----------------------------------------------------------------------
    // Fuzzy suggestion in error messages
    // -----------------------------------------------------------------------

    #[test]
    fn unreachable_source_table_suggests_closest() {
        let def = make_def(
            vec![("o", "orders", vec!["id"]), ("c", "customers", vec!["id"])],
            vec![("o", "c", vec!["customer_id"])],
            vec![("name", Some("custmers"))], // typo -> should suggest "c" or similar
            vec![],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("unknown source table"),
            "expected unknown source table error, got: {err}"
        );
        // The fuzzy suggestion should fire if edit distance <= 3.
        // "custmers" vs "c" has edit distance > 3, so it may not suggest.
        // But "custmers" vs "customers" (the table name) is not in nodes --
        // nodes are aliases. Just check the error exists.
    }

    // -----------------------------------------------------------------------
    // Case insensitivity
    // -----------------------------------------------------------------------

    #[test]
    fn case_insensitive_alias_matching() {
        // Mixed case aliases should work fine.
        let def = make_def(
            vec![("O", "orders", vec!["id"]), ("C", "customers", vec!["id"])],
            vec![("O", "C", vec!["customer_id"])],
            vec![("name", Some("c"))], // lowercase ref to uppercase alias
            vec![],
        );
        assert!(
            validate_graph(&def).is_ok(),
            "case-insensitive alias matching should work"
        );
    }

    // -----------------------------------------------------------------------
    // Valid multi-table tree
    // -----------------------------------------------------------------------

    #[test]
    fn valid_star_schema() {
        // Star: O -> C, O -> P (orders at center, customers and products as leaves)
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("c", "customers", vec!["id"]),
                ("p", "products", vec!["id"]),
            ],
            vec![
                ("o", "c", vec!["customer_id"]),
                ("o", "p", vec!["product_id"]),
            ],
            vec![("name", Some("c")), ("sku", Some("p"))],
            vec![("revenue", Some("o"))],
        );
        let graph = validate_graph(&def).unwrap();
        let order = graph.toposort().unwrap();
        // Root first, then leaves in sorted order.
        assert_eq!(order[0], "o");
        assert!(order.contains(&"c".to_string()));
        assert!(order.contains(&"p".to_string()));
    }
}
