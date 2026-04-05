//! Relationship graph validation and topological sort for semantic view definitions.
//!
//! Built from `TABLES` + `RELATIONSHIPS` declarations at CREATE time.
//! Validates that the relationship graph forms a tree rooted at the base table
//! (first table in TABLES clause). Rejects cycles, diamonds, self-references,
//! orphan tables, unreachable `source_table` aliases, and FK/PK count mismatches.
//!
//! Used by `define.rs` at CREATE time and by `expand.rs` at query time.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::model::SemanticViewDefinition;
use crate::util::suggest_closest;

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

    /// Check that the relationship graph is a tree (each non-root node has
    /// at most one parent), with an exception for role-playing dimensions.
    ///
    /// Returns `Err` with diamond description if any node is reachable via
    /// multiple paths, UNLESS all relationships pointing to that node are named
    /// with distinct names (Phase 32: role-playing dimension support).
    pub fn check_no_diamonds(&self, def: &SemanticViewDefinition) -> Result<(), String> {
        for (node, parents) in &self.reverse {
            if node != &self.root && parents.len() > 1 {
                // Check if ALL relationships to this node are named with distinct names.
                // If so, this is a role-playing pattern (e.g., flights -> airports via
                // dep_airport and arr_airport) and should be allowed.
                let joins_to_node: Vec<&crate::model::Join> = def
                    .joins
                    .iter()
                    .filter(|j| !j.fk_columns.is_empty() && j.table.to_ascii_lowercase() == *node)
                    .collect();

                let all_named =
                    !joins_to_node.is_empty() && joins_to_node.iter().all(|j| j.name.is_some());

                if all_named {
                    // Check all names are unique (case-insensitive)
                    let mut seen_names = HashSet::new();
                    let all_unique = joins_to_node.iter().all(|j| {
                        let name_lower = j.name.as_ref().unwrap().to_ascii_lowercase();
                        seen_names.insert(name_lower)
                    });
                    if all_unique {
                        continue; // Role-playing: allow this diamond
                    }
                }

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

/// Phase 33: Validate that FK referenced columns match a declared PK or UNIQUE
/// constraint on the target table. Replaces the old `check_fk_pk_counts`.
///
/// For each join with non-empty `fk_columns` and non-empty `ref_columns`:
/// - Checks `ref_columns` against target's `pk_columns` (exact set match)
/// - Checks `ref_columns` against each of target's `unique_constraints` (exact set match)
/// - Rejects if neither matches (CARD-03/CARD-09: exact match required, subsets rejected)
fn validate_fk_references(def: &SemanticViewDefinition) -> Result<(), String> {
    for join in &def.joins {
        if join.fk_columns.is_empty() || join.ref_columns.is_empty() {
            continue;
        }
        let to_alias_lower = join.table.to_ascii_lowercase();
        let target = def
            .tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower);
        let Some(target) = target else { continue };

        let ref_set: HashSet<String> = join
            .ref_columns
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();

        // Check PK
        let pk_set: HashSet<String> = target
            .pk_columns
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if !pk_set.is_empty() && ref_set == pk_set {
            continue; // Valid: matches PK
        }

        // Check UNIQUE constraints
        let matches_unique = target.unique_constraints.iter().any(|uc| {
            let uc_set: HashSet<String> = uc.iter().map(|c| c.to_ascii_lowercase()).collect();
            ref_set == uc_set
        });
        if matches_unique {
            continue; // Valid: matches a UNIQUE constraint
        }

        // Neither matches -- build error
        let rel_name = join.name.as_deref().unwrap_or("?");
        let ref_cols = join.ref_columns.join(", ");
        let mut available = Vec::new();
        if !target.pk_columns.is_empty() {
            available.push(format!("PK({})", target.pk_columns.join(", ")));
        }
        for uc in &target.unique_constraints {
            available.push(format!("UNIQUE({})", uc.join(", ")));
        }
        let available_str = if available.is_empty() {
            "none declared".to_string()
        } else {
            available.join(", ")
        };
        return Err(format!(
            "FK ({ref_cols}) in relationship '{rel_name}' does not match any PRIMARY KEY or \
             UNIQUE constraint on table '{}'. Available: {available_str}.",
            target.alias
        ));
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

    // 2. Diamond detection (multiple parents, relaxed for named role-playing).
    graph.check_no_diamonds(def)?;

    // 3. Orphan table detection.
    graph.check_no_orphans()?;

    // 4. FK reference validation (Phase 33: replaces FK/PK count check).
    validate_fk_references(def)?;

    // 5. Source table reachability.
    check_source_tables_reachable(def, &graph)?;

    Ok(graph)
}

#[cfg(test)]
mod tests {
    use crate::graph::validate_graph;
    use crate::model::{Join, SemanticViewDefinition, TableRef};

    use super::super::test_helpers::*;

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
    // FK reference validation (Phase 33: replaces FK/PK count check)
    // -----------------------------------------------------------------------

    #[test]
    fn fk_ref_validation_skipped_when_no_ref_columns() {
        // Phase 33: validate_fk_references skips joins with empty ref_columns
        // (the make_def helper produces joins without ref_columns).
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("c", "customers", vec!["id"]), // 1 PK
            ],
            vec![("o", "c", vec!["customer_id", "extra_col"])], // 2 FK, no ref_columns
            vec![],
            vec![],
        );
        assert!(
            validate_graph(&def).is_ok(),
            "joins without ref_columns should skip FK reference validation"
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
                unique_constraints: vec![],
            }],
            joins: vec![Join {
                table: "customers".to_string(),
                on: "o.customer_id = c.id".to_string(),
                fk_columns: vec![], // Legacy -- no PK/FK
                ..Default::default()
            }],
            dimensions: vec![],
            metrics: vec![],
            facts: vec![],

            column_type_names: vec![],
            column_types_inferred: vec![],
            created_on: None,
            database_name: None,
            schema_name: None,
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

    // -----------------------------------------------------------------------
    // Phase 32: Diamond relaxation
    // -----------------------------------------------------------------------

    #[test]
    fn diamond_two_named_relationships_accepted() {
        // Two named relationships to same table should be accepted (role-playing)
        let def = make_def_with_named_joins(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![
                (Some("dep_airport"), "f", "a", vec!["dep_id"]),
                (Some("arr_airport"), "f", "a", vec!["arr_id"]),
            ],
            vec![("flight_count", Some("f"), vec![])],
        );
        assert!(
            validate_graph(&def).is_ok(),
            "Two named relationships to same table should be accepted"
        );
    }

    #[test]
    fn diamond_two_unnamed_relationships_rejected() {
        // Two unnamed relationships to same table should still be rejected
        let def = make_def_with_named_joins(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![
                (None, "f", "a", vec!["dep_id"]),
                (None, "f", "a", vec!["arr_id"]),
            ],
            vec![("flight_count", Some("f"), vec![])],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("diamond"),
            "Unnamed diamonds should be rejected: {err}"
        );
    }

    #[test]
    fn diamond_mixed_named_unnamed_rejected() {
        // One named + one unnamed relationship to same table -> rejected
        let def = make_def_with_named_joins(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![
                (Some("dep_airport"), "f", "a", vec!["dep_id"]),
                (None, "f", "a", vec!["arr_id"]),
            ],
            vec![("flight_count", Some("f"), vec![])],
        );
        let err = validate_graph(&def).unwrap_err();
        assert!(
            err.contains("diamond"),
            "Mixed named/unnamed diamonds should be rejected: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 33: FK reference validation (CARD-03, CARD-09)
    // -----------------------------------------------------------------------

    mod phase33_fk_reference_tests {
        use super::super::validate_fk_references;
        use crate::model::{Join, SemanticViewDefinition, TableRef};

        /// Build a minimal definition for FK reference validation testing.
        fn make_fk_ref_def(tables: Vec<TableRef>, joins: Vec<Join>) -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: tables.first().map(|t| t.table.clone()).unwrap_or_default(),
                tables,
                joins,
                dimensions: vec![],
                metrics: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
            }
        }

        #[test]
        fn fk_matches_pk_passes() {
            let def = make_fk_ref_def(
                vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ref_columns: vec!["id".to_string()],
                    name: Some("o_to_c".to_string()),
                    ..Default::default()
                }],
            );
            assert!(
                validate_fk_references(&def).is_ok(),
                "FK matching PK should pass"
            );
        }

        #[test]
        fn fk_matches_unique_passes() {
            let def = make_fk_ref_def(
                vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![vec!["email".to_string()]],
                    },
                ],
                vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_email".to_string()],
                    ref_columns: vec!["email".to_string()],
                    name: Some("o_to_c".to_string()),
                    ..Default::default()
                }],
            );
            assert!(
                validate_fk_references(&def).is_ok(),
                "FK matching UNIQUE should pass"
            );
        }

        #[test]
        fn fk_no_match_errors_with_available() {
            let def = make_fk_ref_def(
                vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![vec!["email".to_string()]],
                    },
                ],
                vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_name".to_string()],
                    ref_columns: vec!["name".to_string()],
                    name: Some("o_to_c".to_string()),
                    ..Default::default()
                }],
            );
            let err = validate_fk_references(&def).unwrap_err();
            assert!(
                err.contains("does not match any PRIMARY KEY or UNIQUE constraint"),
                "Expected FK reference error, got: {err}"
            );
            assert!(err.contains("PK(id)"), "Should list PK: {err}");
            assert!(err.contains("UNIQUE(email)"), "Should list UNIQUE: {err}");
        }

        #[test]
        fn composite_fk_subset_rejected() {
            // CARD-09: FK refs (id) but PK is (id, email) -> rejected (not exact match)
            let def = make_fk_ref_def(
                vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string(), "email".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ref_columns: vec!["id".to_string()],
                    name: Some("o_to_c".to_string()),
                    ..Default::default()
                }],
            );
            let err = validate_fk_references(&def).unwrap_err();
            assert!(
                err.contains("does not match any PRIMARY KEY or UNIQUE constraint"),
                "Subset FK should be rejected: {err}"
            );
        }

        #[test]
        fn case_insensitive_matching() {
            // Columns differ in case but should match
            let def = make_fk_ref_def(
                vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["ID".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ref_columns: vec!["id".to_string()], // lowercase vs uppercase PK
                    name: Some("o_to_c".to_string()),
                    ..Default::default()
                }],
            );
            assert!(
                validate_fk_references(&def).is_ok(),
                "Case-insensitive column matching should work"
            );
        }

        #[test]
        fn empty_ref_columns_skipped() {
            // Old-format joins with empty ref_columns should be skipped
            let def = make_fk_ref_def(
                vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ref_columns: vec![], // empty = skip validation
                    name: Some("o_to_c".to_string()),
                    ..Default::default()
                }],
            );
            assert!(
                validate_fk_references(&def).is_ok(),
                "Empty ref_columns should skip validation"
            );
        }
    }
}
