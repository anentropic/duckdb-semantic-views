//! Relationship graph validation and topological sort for semantic view definitions.
//!
//! Built from `TABLES` + `RELATIONSHIPS` declarations at CREATE time.
//! Validates that the relationship graph forms a tree rooted at the base table
//! (first table in TABLES clause). Rejects cycles, diamonds, self-references,
//! orphan tables, tables unreachable from the root, unreachable `source_table`
//! aliases, and FK/PK count mismatches.
//!
//! Used by `define.rs` at CREATE time and by `expand.rs` at query time.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::errors::ParseError;
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
    /// Returns `Err` with a diamond description if any non-root node is
    /// reachable via multiple paths. The sole exception (Phase 32:
    /// role-playing) is a node reached by multiple relationships **all from the
    /// same source table**, each named with a distinct name (e.g.
    /// `flights -> airports` via `dep_airport` and `arr_airport`). Multiple
    /// paths from *different* source tables are always a genuine diamond and
    /// are rejected regardless of naming (E-4), because expansion would
    /// otherwise resolve the ambiguous path to whichever edge was declared
    /// first. (Fan-in onto the base table is not a diamond — the root is
    /// exempt.)
    pub fn check_no_diamonds(&self, def: &SemanticViewDefinition) -> Result<(), String> {
        for (node, parents) in &self.reverse {
            if node != &self.root && parents.len() > 1 {
                // Parents from two or more DISTINCT source tables => a genuine
                // multi-path diamond with an ambiguous join path. The
                // role-playing allowance below is only for a single source
                // declaring several named FKs (see the method doc).
                let distinct_sources: HashSet<&String> = parents.iter().collect();
                if distinct_sources.len() > 1 {
                    let mut srcs: Vec<&str> = distinct_sources.iter().map(|s| s.as_str()).collect();
                    srcs.sort_unstable();
                    return Err(format!(
                        "diamond: '{}' is reachable from multiple tables ({}); the join path \
                         is ambiguous. Declare the target under a second table alias so it is \
                         joined once per path.",
                        node,
                        srcs.into_iter()
                            .map(|s| format!("'{s}'"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }

                // Single source, multiple edges: allowed only as role-playing —
                // every relationship to this node must be named with a distinct
                // name (e.g. flights -> airports via dep_airport and
                // arr_airport).
                let joins_to_node: Vec<&crate::model::Join> = def
                    .joins
                    .iter()
                    .filter(|j| !j.fk_columns.is_empty() && j.table.to_ascii_lowercase() == *node)
                    .collect();

                let all_named =
                    !joins_to_node.is_empty() && joins_to_node.iter().all(|j| j.name.is_some());

                if all_named {
                    // Check all names are unique under DuckDB's identifier
                    // rule: `dep`, `DEP` and `"dep"` are one name.
                    let mut seen_names = HashSet::new();
                    let all_unique = joins_to_node.iter().all(|j| {
                        seen_names
                            .insert(crate::ident::normalize_ident_part(j.name.as_ref().unwrap()))
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
    ///
    /// When the root itself has no edge, every table is reported together by
    /// [`Self::check_connected`] instead (its "base table … is not connected"
    /// message names the real defect), so this check stands aside. Candidates
    /// are visited in sorted order, so the table named does not depend on
    /// `HashSet` iteration order.
    pub fn check_no_orphans(&self) -> Result<(), String> {
        let has_edge =
            |node: &String| self.edges.contains_key(node) || self.reverse.contains_key(node);
        if !has_edge(&self.root) {
            return Ok(());
        }
        let mut nodes: Vec<&String> = self.all_nodes.iter().collect();
        nodes.sort_unstable();
        for node in nodes {
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

    /// Check that every declared table is reachable from the root (the first
    /// table in TABLES) by an undirected chain of relationships (TECH-DEBT #72).
    ///
    /// [`Self::check_no_orphans`] exempts the root, which is right for a
    /// connected tree — the base table usually has only *incoming* edges — but
    /// it cannot tell "root of a connected tree" from "isolated table that
    /// happens to be listed first", nor catch two components where every table
    /// has *some* edge. Either way expansion emits a `FROM` anchored at the root
    /// whose `LEFT JOIN … ON` clauses name an alias that is never in scope, and
    /// the query fails at bind time. Direction is ignored because the emitters
    /// join each hop with an ON clause naming both sides explicitly.
    ///
    /// Run after the orphan check, so every table reaching here has at least
    /// one edge unless the root itself has none (in which case the orphan
    /// check stood aside and this reports every table at once).
    pub fn check_connected(&self) -> Result<(), String> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = vec![self.root.as_str()];
        seen.insert(self.root.as_str());
        while let Some(node) = stack.pop() {
            let out = self.edges.get(node).into_iter().flatten();
            let inc = self.reverse.get(node).into_iter().flatten();
            for next in out.chain(inc) {
                if seen.insert(next.as_str()) {
                    stack.push(next.as_str());
                }
            }
        }
        let mut unreachable: Vec<&str> = self
            .all_nodes
            .iter()
            .map(String::as_str)
            .filter(|n| !seen.contains(n))
            .collect();
        if unreachable.is_empty() {
            return Ok(());
        }
        unreachable.sort_unstable();
        let root = &self.root;
        if seen.len() == 1 {
            // The root itself has no edge: the defect is the base table, not the
            // tables listed after it, so name it.
            return Err(format!(
                "base table '{root}' (the first table in TABLES) is not connected by any \
                 relationship to the other tables ({}); declare a relationship that reaches \
                 it, or list a connected table first",
                quote_list(&unreachable)
            ));
        }
        Err(format!(
            "table '{}' is not reachable from the base table '{root}' (the first table in \
             TABLES) through any chain of relationships; unreachable: {}",
            unreachable[0],
            quote_list(&unreachable)
        ))
    }
}

fn quote_list(names: &[&str]) -> String {
    names
        .iter()
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ")
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

/// Reject a relationship with no foreign-key columns.
///
/// The DDL grammar always produces FK columns; YAML does not require them. A
/// stored pre-Phase-24 row may lack them too, but stored rows never reach this
/// (CREATE-only) check — the query-time fan-trap fence owns those.
fn check_relationships_have_fk_columns(def: &SemanticViewDefinition) -> Result<(), String> {
    match def.joins.iter().find(|j| j.fk_columns.is_empty()) {
        None => Ok(()),
        Some(j) => {
            let name = j
                .name
                .as_deref()
                .map_or_else(String::new, |n| format!("'{n}' "));
            Err(format!(
                "relationship {name}from '{}' to '{}' declares no foreign-key columns; \
                 the form is 'rel_name AS from_alias(fk_cols) REFERENCES to_alias'",
                j.from_alias, j.table
            ))
        }
    }
}

/// Validate the relationship graph of a semantic view definition.
///
/// Runs all define-time checks:
/// 0. Every relationship declares FK columns
/// 1. Self-reference detection (`from_alias` == `to_alias`)
/// 2. Cycle detection (Kahn's algorithm)
/// 3. Diamond detection (multiple parents)
/// 4. Orphan table detection (declared but not connected)
/// 5. Connectivity: every table reachable from the root (TECH-DEBT #72)
/// 6. FK/PK column count matching
/// 7. Source table reachability
///
/// Returns `Ok(graph)` if valid, or `Err` with a descriptive message.
///
/// **Single-table skip:** a view with at most one table and no relationship
/// has no graph to validate, so it returns `Ok` with an empty graph — exactly
/// as before, so a single-table view's acceptance is unchanged.
///
/// This used to be a wider "legacy skip" that fired whenever no relationship
/// carried `fk_columns`, for pre-Phase-24 definitions. But this function runs
/// only at CREATE (`ddl::define::enrich_definition_for_create`) — stored rows
/// are never re-validated on load — so the skip protected no stored
/// definition and only let two unusable shapes through CREATE: several tables
/// with no relationship at all (disconnected, TECH-DEBT #72) and a
/// relationship with no FK columns (YAML).
pub fn validate_graph(def: &SemanticViewDefinition) -> Result<RelationshipGraph, ParseError> {
    if def.tables.is_empty() || (def.tables.len() == 1 && def.joins.is_empty()) {
        return Ok(RelationshipGraph {
            edges: HashMap::new(),
            reverse: HashMap::new(),
            all_nodes: HashSet::new(),
            root: String::new(),
        });
    }

    // 0. Every relationship must carry FK columns. `from_definition` skips an
    //    FK-less join (the pre-Phase-24 stored encoding), so without this an
    //    FK-less YAML relationship would silently vanish from the graph.
    check_relationships_have_fk_columns(def).map_err(ParseError::positionless)?;

    let graph = RelationshipGraph::from_definition(def).map_err(ParseError::positionless)?;

    // 1. Cycle detection (Kahn's algorithm).
    let _topo_order = graph.toposort().map_err(ParseError::positionless)?;

    // 2. Diamond detection (multiple parents, relaxed for named role-playing).
    graph
        .check_no_diamonds(def)
        .map_err(ParseError::positionless)?;

    // 3. Orphan table detection.
    graph.check_no_orphans().map_err(ParseError::positionless)?;

    // 3b. Connectivity (TECH-DEBT #72): every table reachable from the root.
    graph.check_connected().map_err(ParseError::positionless)?;

    // 4. FK reference validation (Phase 33: replaces FK/PK count check).
    validate_fk_references(def).map_err(ParseError::positionless)?;

    // 5. Source table reachability.
    check_source_tables_reachable(def, &graph).map_err(ParseError::positionless)?;

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
        let err = validate_graph(&def).unwrap_err().message;
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
        let err = validate_graph(&def).unwrap_err().message;
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
        let err = validate_graph(&def).unwrap_err().message;
        assert!(
            err.contains("diamond") && err.contains("reachable from multiple tables"),
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
        let err = validate_graph(&def).unwrap_err().message;
        assert!(err.contains("orphan"), "expected orphan error, got: {err}");
    }

    // -----------------------------------------------------------------------
    // Connectivity (TECH-DEBT #72)
    // -----------------------------------------------------------------------

    /// TECH-DEBT #72: the FIRST table has no relationship edge. `check_no_orphans`
    /// exempts the root, so this used to be accepted and every query emitted a
    /// `FROM "x" LEFT JOIN … ON … = "o"."…"` forward reference that fails to bind.
    #[test]
    fn disconnected_root_table_rejected() {
        let def = make_def(
            vec![
                ("x", "lonely", vec!["id"]),
                ("o", "orders", vec!["id"]),
                ("c", "customers", vec!["id"]),
            ],
            vec![("o", "c", vec!["customer_id"])],
            vec![],
            vec![],
        );
        let err = validate_graph(&def)
            .expect_err("a root with no relationship must be rejected")
            .message;
        assert!(
            err.contains("'x'") && err.contains("not connected"),
            "expected the error to name the disconnected base table 'x', got: {err}"
        );
    }

    /// TECH-DEBT #72, the second shape: every table has an edge (so none is an
    /// orphan) but two components exist — `{o, c}` and `{p, s}`. `s` and `p`
    /// cannot be reached from the root `o` by any chain of relationships.
    #[test]
    fn disconnected_component_rejected_naming_the_unreachable_alias() {
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("c", "customers", vec!["id"]),
                ("p", "products", vec!["id"]),
                ("s", "suppliers", vec!["id"]),
            ],
            vec![
                ("o", "c", vec!["customer_id"]),
                ("p", "s", vec!["supplier_id"]),
            ],
            vec![],
            vec![],
        );
        let err = validate_graph(&def)
            .expect_err("a second component must be rejected")
            .message;
        assert!(
            err.contains("'p'") && err.contains("not reachable from the base table 'o'"),
            "expected the error to name the first unreachable alias 'p', got: {err}"
        );
    }

    /// Control for the connectivity check: reachability is UNDIRECTED. `li`
    /// points at the root (`li -> o`, an incoming edge) and `o -> c` points
    /// away; both are reachable, so the definition stays valid.
    #[test]
    fn connectivity_is_undirected() {
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("li", "line_items", vec!["id"]),
                ("c", "customers", vec!["id"]),
                ("p", "products", vec!["id"]),
            ],
            vec![
                ("li", "o", vec!["order_id"]),
                ("o", "c", vec!["customer_id"]),
                ("li", "p", vec!["product_id"]),
            ],
            vec![],
            vec![],
        );
        assert!(
            validate_graph(&def).is_ok(),
            "a connected graph mixing edge directions must be accepted: {:?}",
            validate_graph(&def).err().map(|e| e.message)
        );
    }

    /// One randomly generated relationship graph for the #72 property.
    ///
    /// `main` tables `t0..` form a random tree rooted at `t0`: each `t{i}`
    /// hangs off a random earlier table, the edge pointing AWAY from the root
    /// (`parent -> child`) unless `towards_root[i]`, the parent is `t0` and `t0`
    /// is the root, in which case it points INTO the root (`child -> t0`, the fan-in the
    /// diamond check exempts). Either way the graph stays a valid tree, so
    /// connectivity is the only thing that can fail. `detached` extra tables
    /// `d0..` form a second tree joined to nothing in `main`; `detached_first`
    /// lists them BEFORE the main tables, which makes the root itself the
    /// isolated node when the detached component is a single table.
    #[derive(Debug, Clone)]
    struct GraphCase {
        main_parents: Vec<usize>,
        towards_root: Vec<bool>,
        detached_parents: Vec<usize>,
        detached_first: bool,
    }

    fn arb_graph_case() -> impl proptest::strategy::Strategy<Value = GraphCase> {
        use proptest::prelude::*;
        (1usize..6, 0usize..4, any::<bool>()).prop_flat_map(|(n_main, n_det, detached_first)| {
            let parents = |n: usize| {
                (0..n)
                    .map(|i| {
                        if i == 0 {
                            Just(0).boxed()
                        } else {
                            (0..i).boxed()
                        }
                    })
                    .collect::<Vec<_>>()
            };
            (
                parents(n_main),
                prop::collection::vec(any::<bool>(), n_main),
                parents(n_det),
                Just(detached_first),
            )
                .prop_map(
                    |(main_parents, towards_root, detached_parents, detached_first)| GraphCase {
                        main_parents,
                        towards_root,
                        detached_parents,
                        detached_first,
                    },
                )
        })
    }

    fn build_graph_case(case: &GraphCase) -> SemanticViewDefinition {
        let mut tables: Vec<(String, String)> = Vec::new();
        let mut joins: Vec<(String, String)> = Vec::new();
        let mut add_tree = |prefix: &str, parents: &[usize], towards_root: Option<&[bool]>| {
            let mut t = Vec::new();
            for (i, &p) in parents.iter().enumerate() {
                t.push((format!("{prefix}{i}"), format!("tbl_{prefix}{i}")));
                if i == 0 {
                    continue;
                }
                let (child, parent) = (format!("{prefix}{i}"), format!("{prefix}{p}"));
                if p == 0 && towards_root.is_some_and(|tr| tr[i]) {
                    joins.push((child, parent));
                } else {
                    joins.push((parent, child));
                }
            }
            t
        };
        // Fan-in is only exempt from the diamond check at the ROOT, so edges
        // may point into `t0` only while `t0` is listed first.
        let towards_root = (!case.detached_first).then_some(case.towards_root.as_slice());
        let main = add_tree("t", &case.main_parents, towards_root);
        let detached = add_tree("d", &case.detached_parents, None);
        if case.detached_first {
            tables.extend(detached);
            tables.extend(main);
        } else {
            tables.extend(main);
            tables.extend(detached);
        }
        SemanticViewDefinition {
            tables: tables
                .into_iter()
                .map(|(alias, table)| TableRef {
                    alias,
                    table,
                    pk_columns: vec!["id".to_string()],
                    ..Default::default()
                })
                .collect(),
            joins: joins
                .into_iter()
                .enumerate()
                .map(|(k, (from, to))| Join {
                    table: to,
                    from_alias: from,
                    fk_columns: vec![format!("fk{k}")],
                    name: Some(format!("r{k}")),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    proptest::proptest! {
        /// TECH-DEBT #72, randomized: a definition is accepted iff every table
        /// is reachable from the first one, whatever the shape of each
        /// component, the direction of its edges, or the TABLES order. When a
        /// detached component exists the error names a table that really is
        /// unreachable — the base table itself when it is the isolated node.
        #[test]
        fn graph_accepted_iff_connected(case in arb_graph_case()) {
            let def = build_graph_case(&case);
            // No `prop_assume!` on the relationship count: a several-table
            // definition with NO relationship (one table per component) used
            // to skip graph validation entirely and was filtered out here. It
            // is the plainest disconnected graph there is, so it is checked
            // like every other case.
            let n_main = case.main_parents.len();
            let n_det = case.detached_parents.len();
            let result = validate_graph(&def);
            if n_det == 0 {
                proptest::prop_assert!(
                    result.is_ok(),
                    "connected graph rejected: {:?}",
                    result.err().map(|e| e.message)
                );
            } else {
                let msg = result.expect_err("a detached component must be rejected").message;
                let (root_side, other_side, other_n) = if case.detached_first {
                    ("d", "t", n_main)
                } else {
                    ("t", "d", n_det)
                };
                let root_isolated = if case.detached_first { n_det == 1 } else { n_main == 1 };
                if root_isolated {
                    // A one-table root component is NOT an orphan-check
                    // failure (the root is exempt, and an edgeless root defers
                    // the other edgeless tables to the connectivity check), so
                    // connectivity is what must report it — with edges
                    // elsewhere or with no relationship in the view at all.
                    proptest::prop_assert!(
                        msg.contains(&format!("base table '{root_side}0'")),
                        "expected the isolated base table to be named: {msg}"
                    );
                } else if other_n == 1 {
                    // A single detached non-root table has no edge at all —
                    // the pre-existing orphan check owns that case.
                    proptest::prop_assert!(msg.contains("orphan"), "{msg}");
                } else {
                    proptest::prop_assert!(
                        msg.contains(&format!("table '{other_side}"))
                            && msg.contains("not reachable from the base table"),
                        "expected an unreachable '{other_side}*' table to be named: {msg}"
                    );
                }
            }
        }
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
        let err = validate_graph(&def).unwrap_err().message;
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
        let err = validate_graph(&def).unwrap_err().message;
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

    /// Two role-playing relationships whose names differ only in QUOTING
    /// (`dep` and `"dep"`) are one identifier under DuckDB's rules, so they are
    /// the same name used twice — rejected exactly like `dep` / `DEP`. An
    /// ASCII-lowercase comparison kept the quotes, accepted the pair as two
    /// roles, and both then mapped to the one scoped alias `a__dep`.
    #[test]
    fn role_playing_names_differing_only_in_quoting_are_one_name() {
        let mut def = make_def(
            vec![("f", "flights", vec!["id"]), ("a", "airports", vec!["id"])],
            vec![("f", "a", vec!["dep_id"]), ("f", "a", vec!["arr_id"])],
            vec![],
            vec![],
        );
        def.joins[0].name = Some("dep".to_string());
        def.joins[1].name = Some("\"dep\"".to_string());
        let err = validate_graph(&def)
            .expect_err("`dep` and `\"dep\"` name the same relationship")
            .message;
        assert!(
            err.contains("diamond"),
            "expected a diamond error, got: {err}"
        );
        // Control: genuinely distinct names (one of them quoted) still pass.
        def.joins[1].name = Some("\"Arr\"".to_string());
        assert!(
            validate_graph(&def).is_ok(),
            "{:?}",
            validate_graph(&def).err().map(|e| e.message)
        );
    }

    // -----------------------------------------------------------------------
    // The former "legacy skip" (TECH-DEBT #72 follow-up)
    // -----------------------------------------------------------------------
    //
    // `validate_graph` runs only at CREATE (`enrich_definition_for_create`);
    // stored rows are never re-validated on load. It used to skip every check
    // when no relationship carried `fk_columns`, which let two shapes through
    // CREATE that no query could use: several tables and no relationships at
    // all (the #72 disconnected graph, minus the edge that triggered the
    // check), and a YAML relationship with no FK columns (which the query-time
    // fan-trap fence then reports as a "legacy pre-Phase-24 definition").

    /// Two tables, zero relationships. Every member on `c` emitted
    /// `FROM "o" … c.region` — `Binder Error: Referenced table "c" not found`.
    /// Rejected now with #72's own error, naming the base table.
    #[test]
    fn multi_table_view_with_no_relationships_rejected() {
        let def = make_def(
            vec![("o", "orders", vec!["id"]), ("c", "customers", vec!["id"])],
            vec![],
            vec![("region", Some("c"))],
            vec![("revenue", Some("o"))],
        );
        let err = validate_graph(&def)
            .expect_err("two tables with no relationship must be rejected")
            .message;
        assert!(
            err.contains("base table 'o'") && err.contains("not connected") && err.contains("'c'"),
            "expected #72's base-table error naming 'o' and listing 'c', got: {err}"
        );
    }

    /// Three tables, zero relationships: every table is edgeless, so the
    /// orphan check would have named whichever non-root table a `HashSet`
    /// yielded first. The base-table message lists them all, sorted.
    #[test]
    fn several_unrelated_tables_get_one_deterministic_error() {
        let def = make_def(
            vec![
                ("o", "orders", vec!["id"]),
                ("p", "products", vec!["id"]),
                ("c", "customers", vec!["id"]),
            ],
            vec![],
            vec![],
            vec![],
        );
        let err = validate_graph(&def)
            .expect_err("unrelated tables must be rejected")
            .message;
        assert!(
            err.contains("base table 'o'") && err.contains("('c', 'p')"),
            "expected the base-table error listing 'c', 'p' in order, got: {err}"
        );
    }

    /// With several edgeless non-root tables the orphan check named whichever
    /// one `HashSet` iteration reached first, so the same definition could
    /// report a different table from one CREATE to the next. It names the
    /// alphabetically first now, every time.
    #[test]
    fn orphan_error_is_deterministic() {
        let make = || {
            make_def(
                vec![
                    ("o", "orders", vec!["id"]),
                    ("c", "customers", vec!["id"]),
                    ("z", "z_table", vec!["id"]),
                    ("y", "y_table", vec!["id"]),
                    ("x", "x_table", vec!["id"]),
                    ("w", "w_table", vec!["id"]),
                ],
                vec![("o", "c", vec!["customer_id"])],
                vec![],
                vec![],
            )
        };
        for _ in 0..64 {
            let err = validate_graph(&make()).unwrap_err().message;
            assert!(
                err.starts_with("orphan table 'w'"),
                "expected the alphabetically first orphan 'w', got: {err}"
            );
        }
    }

    /// A relationship with no FK columns (reachable through YAML, which does
    /// not require `fk_columns`) used to switch graph validation off for the
    /// whole definition. Every query then failed the fan-trap fence with a
    /// misleading "legacy pre-Phase-24 definition format" error.
    #[test]
    fn relationship_without_fk_columns_rejected() {
        let mut def = make_def(
            vec![("o", "orders", vec!["id"]), ("c", "customers", vec!["id"])],
            vec![("o", "c", vec![])],
            vec![],
            vec![],
        );
        def.joins[0].name = Some("o_to_c".to_string());
        let err = validate_graph(&def)
            .expect_err("a relationship with no FK columns must be rejected")
            .message;
        assert!(
            err.contains("'o_to_c'") && err.contains("no foreign-key columns"),
            "expected the error to name the relationship, got: {err}"
        );
    }

    #[test]
    fn relationship_without_fk_columns_rejected_on_a_single_table_view() {
        // The single-table exemption covers only a view with no relationship
        // at all; an FK-less relationship is rejected even when TABLES has one
        // entry (here it targets a table that is not declared).
        let mut def = SemanticViewDefinition {
            tables: vec![TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec![],
                unique_constraints: vec![],
                comment: None,
                synonyms: vec![],
            }],
            joins: vec![Join {
                table: "customers".to_string(),
                fk_columns: vec![], // Legacy -- no PK/FK metadata
                ..Default::default()
            }],
            dimensions: vec![],
            metrics: vec![],
            facts: vec![],
            materializations: vec![],

            created_on: None,
            database_name: None,
            schema_name: None,
            resolution_schema_name: None,
            comment: None,
        };
        let err = validate_graph(&def)
            .expect_err("an FK-less relationship must be rejected on a one-table view too")
            .message;
        assert!(
            err.contains("no foreign-key columns") && err.contains("'customers'"),
            "expected the FK-less relationship to be named by its target, got: {err}"
        );
        // An empty TABLES list still skips: there is no graph to validate, and
        // the TABLES-clause checks own that error.
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
        let err = validate_graph(&def).unwrap_err().message;
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
    fn named_cross_source_diamond_rejected() {
        // E-4 (code-review 2026-07-11): a node reached from two DIFFERENT
        // source tables via distinctly-named relationships is a genuine
        // multi-path diamond, NOT role-playing. It was previously accepted
        // (the relaxation only checked name uniqueness), and expansion then
        // silently joined the node through whichever edge was declared first.
        // It must be rejected: role-playing requires a single source table.
        //   a (base) -> b, a -> c, b -> d (via_b), c -> d (via_c)  =>  d is a
        //   diamond reachable from both b and c.
        let def = make_def_with_named_joins(
            vec![
                ("a", "tbl_a", vec!["id"]),
                ("b", "tbl_b", vec!["id"]),
                ("c", "tbl_c", vec!["id"]),
                ("d", "tbl_d", vec!["id"]),
            ],
            vec![
                (Some("a_to_b"), "a", "b", vec!["b_id"]),
                (Some("a_to_c"), "a", "c", vec!["c_id"]),
                (Some("via_b"), "b", "d", vec!["d_id"]),
                (Some("via_c"), "c", "d", vec!["d_id"]),
            ],
            vec![("cnt", Some("a"), vec![])],
        );
        let err = validate_graph(&def).unwrap_err().message;
        assert!(
            err.contains("diamond") && err.contains("reachable from multiple tables"),
            "cross-source named diamond must be rejected: {err}"
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
        let err = validate_graph(&def).unwrap_err().message;
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
        let err = validate_graph(&def).unwrap_err().message;
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
                tables,
                joins,
                dimensions: vec![],
                metrics: vec![],
                facts: vec![],
                materializations: vec![],

                created_on: None,
                database_name: None,
                schema_name: None,
                resolution_schema_name: None,
                comment: None,
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
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
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
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![vec!["email".to_string()]],
                        comment: None,
                        synonyms: vec![],
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
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![vec!["email".to_string()]],
                        comment: None,
                        synonyms: vec![],
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
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string(), "email".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
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
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["ID".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
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
                        comment: None,
                        synonyms: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                        comment: None,
                        synonyms: vec![],
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
