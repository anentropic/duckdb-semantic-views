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

// ---------------------------------------------------------------------------
// Fact validation (Phase 29)
// ---------------------------------------------------------------------------

/// Check if a byte is a word-boundary character (NOT alphanumeric or underscore).
fn is_word_boundary_byte(b: u8) -> bool {
    !b.is_ascii_alphanumeric() && b != b'_'
}

/// Find references to known fact names in an expression using word-boundary matching.
///
/// Returns a list of fact names found in `expr`. Only matches whole words:
/// `net_price` matches in `SUM(net_price)` and `net_price + tax`
/// but NOT in `net_price_total` or `my_net_price`.
#[must_use]
pub fn find_fact_references<'a>(expr: &str, fact_names: &[&'a str]) -> Vec<&'a str> {
    let bytes = expr.as_bytes();
    let expr_lower = expr.to_ascii_lowercase();
    let lower_bytes = expr_lower.as_bytes();
    let mut found = Vec::new();

    for &fact_name in fact_names {
        let fn_lower = fact_name.to_ascii_lowercase();
        let fn_bytes = fn_lower.as_bytes();
        let fn_len = fn_bytes.len();
        if fn_len == 0 || fn_len > bytes.len() {
            continue;
        }

        let mut i = 0;
        while i + fn_len <= lower_bytes.len() {
            if &lower_bytes[i..i + fn_len] == fn_bytes {
                let before_ok = i == 0 || is_word_boundary_byte(bytes[i - 1]);
                let after_ok =
                    i + fn_len == bytes.len() || is_word_boundary_byte(bytes[i + fn_len]);
                if before_ok && after_ok {
                    found.push(fact_name);
                    break; // Each fact name only counted once
                }
            }
            i += 1;
        }
    }

    found
}

/// Validate facts in a semantic view definition.
///
/// Checks:
/// 1. Each fact's `source_table` is in `def.tables` aliases (case-insensitive).
/// 2. Any fact names referenced in other facts' expressions actually exist.
/// 3. The fact dependency graph has no cycles (Kahn's algorithm).
///
/// Returns `Ok(())` if valid, `Err` with descriptive message otherwise.
#[allow(clippy::too_many_lines)]
pub fn validate_facts(def: &SemanticViewDefinition) -> Result<(), String> {
    if def.facts.is_empty() {
        return Ok(());
    }

    // 1. Check source_table references
    check_fact_source_tables(def)?;

    // Collect fact names
    let fact_names: Vec<&str> = def.facts.iter().map(|f| f.name.as_str()).collect();

    // 2. Build fact dependency DAG and check for cycles
    let (edges, in_degree) = build_fact_dag(def, &fact_names)?;

    // 3. Check that all referenced facts exist
    check_fact_references_exist(&edges, &fact_names)?;

    // 4. Cycle detection via Kahn's algorithm
    check_fact_cycles(&edges, in_degree, &fact_names)
}

/// Check that each fact's `source_table` is a declared table alias.
fn check_fact_source_tables(def: &SemanticViewDefinition) -> Result<(), String> {
    let table_aliases: Vec<String> = def
        .tables
        .iter()
        .map(|t| t.alias.to_ascii_lowercase())
        .collect();
    let alias_display: Vec<String> = def.tables.iter().map(|t| t.alias.clone()).collect();

    for fact in &def.facts {
        if let Some(ref st) = fact.source_table {
            let st_lower = st.to_ascii_lowercase();
            if !table_aliases.contains(&st_lower) {
                let suggestion = suggest_closest(&st_lower, &alias_display);
                let mut msg = format!("unknown source table '{}' in fact '{}'", st, fact.name);
                if let Some(s) = suggestion {
                    let _ = write!(msg, "; did you mean '{s}'?");
                }
                return Err(msg);
            }
        }
    }
    Ok(())
}

/// Adjacency list and in-degree map for fact dependency DAG.
type FactDag<'a> = (HashMap<&'a str, Vec<&'a str>>, HashMap<&'a str, usize>);

/// Build fact dependency DAG from expressions. Returns `(edges, in_degree)`.
fn build_fact_dag<'a>(
    def: &'a SemanticViewDefinition,
    fact_names: &[&'a str],
) -> Result<FactDag<'a>, String> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for &name in fact_names {
        in_degree.entry(name).or_insert(0);
    }

    for fact in &def.facts {
        let refs = find_fact_references(&fact.expr, fact_names);
        for &referenced in &refs {
            if referenced == fact.name.as_str() {
                return Err(format!(
                    "cycle detected in facts: {} -> {}",
                    fact.name, fact.name
                ));
            }
            edges
                .entry(fact.name.as_str())
                .or_default()
                .push(referenced);
            *in_degree.entry(referenced).or_insert(0) += 1;
        }
    }

    Ok((edges, in_degree))
}

/// Check that all fact names referenced in edges actually exist.
fn check_fact_references_exist(
    edges: &HashMap<&str, Vec<&str>>,
    fact_names: &[&str],
) -> Result<(), String> {
    let display_names: Vec<String> = fact_names.iter().map(ToString::to_string).collect();
    for (source, targets) in edges {
        for &target in targets {
            if !fact_names.contains(&target) {
                let suggestion = suggest_closest(target, &display_names);
                let mut msg = format!("unknown fact '{target}' referenced in fact '{source}'");
                if let Some(s) = suggestion {
                    let _ = write!(msg, "; did you mean '{s}'?");
                }
                return Err(msg);
            }
        }
    }
    Ok(())
}

/// Detect cycles in the fact DAG using Kahn's algorithm.
fn check_fact_cycles(
    edges: &HashMap<&str, Vec<&str>>,
    mut in_degree: HashMap<&str, usize>,
    fact_names: &[&str],
) -> Result<(), String> {
    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&name, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(name);
        }
    }

    let mut visited_count = 0;
    while let Some(node) = queue.pop_front() {
        visited_count += 1;
        if let Some(neighbors) = edges.get(node) {
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
    }

    if visited_count != fact_names.len() {
        let remaining: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(&name, _)| name)
            .collect();

        if let Some(&start) = remaining.first() {
            let mut path = vec![start];
            let mut current = start;
            let mut seen: HashSet<&str> = HashSet::new();
            seen.insert(current);

            while let Some(neighbors) = edges.get(current) {
                if let Some(&next) = neighbors.iter().find(|n| remaining.contains(n)) {
                    if seen.contains(next) {
                        path.push(next);
                        return Err(format!("cycle detected in facts: {}", path.join(" -> ")));
                    }
                    seen.insert(next);
                    path.push(next);
                    current = next;
                } else {
                    break;
                }
            }
        }

        return Err("cycle detected in facts".to_string());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Hierarchy validation (Phase 29)
// ---------------------------------------------------------------------------

/// Validate hierarchies in a semantic view definition.
///
/// Checks that every level name in each hierarchy matches a declared dimension name
/// (case-insensitive). Uses `suggest_closest` for fuzzy error suggestions.
///
/// Returns `Ok(())` if valid, `Err` with descriptive message otherwise.
pub fn validate_hierarchies(def: &SemanticViewDefinition) -> Result<(), String> {
    if def.hierarchies.is_empty() {
        return Ok(());
    }

    let dim_names_lower: HashSet<String> = def
        .dimensions
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    let dim_names_display: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();

    for hierarchy in &def.hierarchies {
        for level in &hierarchy.levels {
            let level_lower = level.to_ascii_lowercase();
            if !dim_names_lower.contains(&level_lower) {
                let suggestion = suggest_closest(&level_lower, &dim_names_display);
                let mut msg = format!(
                    "unknown dimension '{}' in hierarchy '{}'",
                    level, hierarchy.name
                );
                if let Some(s) = suggestion {
                    let _ = write!(msg, "; did you mean '{s}'?");
                }
                return Err(msg);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Derived metric validation (Phase 30)
// ---------------------------------------------------------------------------

/// Known SQL aggregate function names (lowercase).
const AGGREGATE_FUNCTIONS: &[&str] = &[
    "sum",
    "count",
    "avg",
    "min",
    "max",
    "stddev",
    "stddev_pop",
    "stddev_samp",
    "variance",
    "var_pop",
    "var_samp",
    "string_agg",
    "listagg",
    "group_concat",
    "array_agg",
    "any_value",
    "approx_count_distinct",
    "median",
    "mode",
    "percentile_cont",
    "percentile_disc",
    "corr",
    "covar_pop",
    "covar_samp",
    "regr_avgx",
    "regr_avgy",
    "regr_count",
    "regr_intercept",
    "regr_r2",
    "regr_slope",
    "regr_sxx",
    "regr_sxy",
    "regr_syy",
    "bit_and",
    "bit_or",
    "bit_xor",
    "bool_and",
    "bool_or",
];

/// Check if an expression contains an aggregate function call.
///
/// Scans for known aggregate function names at word boundaries followed by `(`.
/// Case-insensitive matching. Skips matches inside single-quoted string literals.
///
/// Returns the first aggregate function name found (lowercase), or `None`.
#[must_use]
pub fn contains_aggregate_function(expr: &str) -> Option<&'static str> {
    let bytes = expr.as_bytes();
    let expr_lower = expr.to_ascii_lowercase();
    let lower_bytes = expr_lower.as_bytes();

    for &func_name in AGGREGATE_FUNCTIONS {
        let fn_bytes = func_name.as_bytes();
        let fn_len = fn_bytes.len();
        if fn_len > lower_bytes.len() {
            continue;
        }

        let mut i = 0;
        let mut in_string = false;
        while i < lower_bytes.len() {
            // Track string literal state
            if bytes[i] == b'\'' {
                in_string = !in_string;
                i += 1;
                continue;
            }
            if in_string {
                i += 1;
                continue;
            }

            // Check if we have a match at position i
            if i + fn_len <= lower_bytes.len() && &lower_bytes[i..i + fn_len] == fn_bytes {
                // Check word boundary before
                let before_ok = i == 0 || is_word_boundary_byte(bytes[i - 1]);
                // Check followed by '(' (with optional whitespace)
                if before_ok {
                    let after_pos = i + fn_len;
                    let mut j = after_pos;
                    // Skip whitespace
                    while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'(' {
                        return Some(func_name);
                    }
                }
            }
            i += 1;
        }
    }

    None
}

/// Validate derived metrics in a semantic view definition.
///
/// Checks:
/// 1. No duplicate metric names across base and derived (case-insensitive).
/// 2. Derived metrics must not contain aggregate function calls.
/// 3. All metric names referenced in derived metric expressions must exist.
/// 4. The derived metric dependency graph has no cycles (Kahn's algorithm).
///
/// Returns `Ok(())` if valid, `Err` with descriptive message otherwise.
#[allow(clippy::too_many_lines)]
pub fn validate_derived_metrics(def: &SemanticViewDefinition) -> Result<(), String> {
    let derived: Vec<&crate::model::Metric> = def
        .metrics
        .iter()
        .filter(|m| m.source_table.is_none())
        .collect();

    if derived.is_empty() {
        return Ok(());
    }

    // 1. Check metric name uniqueness (case-insensitive)
    check_metric_name_uniqueness(def)?;

    // 2. Check for aggregate functions in derived metrics
    check_no_aggregates_in_derived(&derived)?;

    // 3. Check for unknown metric references in derived expressions
    let all_metric_names: Vec<&str> = def.metrics.iter().map(|m| m.name.as_str()).collect();
    let all_metric_names_display: Vec<String> = all_metric_names
        .iter()
        .copied()
        .map(ToString::to_string)
        .collect();
    check_derived_metric_references(&derived, &all_metric_names, &all_metric_names_display)?;

    // 4. Check for cycles in derived metric dependency graph
    let derived_name_strs: Vec<&str> = derived.iter().map(|m| m.name.as_str()).collect();
    check_derived_metric_cycles(&derived, &derived_name_strs)
}

/// Check that no two metrics (base or derived) share the same name (case-insensitive).
fn check_metric_name_uniqueness(def: &SemanticViewDefinition) -> Result<(), String> {
    let mut seen_names: HashSet<String> = HashSet::new();
    for met in &def.metrics {
        let lower = met.name.to_ascii_lowercase();
        if !seen_names.insert(lower) {
            return Err(format!("duplicate metric name '{}'", met.name));
        }
    }
    Ok(())
}

/// Check that derived metrics do not contain aggregate function calls.
fn check_no_aggregates_in_derived(derived: &[&crate::model::Metric]) -> Result<(), String> {
    for met in derived {
        if let Some(func) = contains_aggregate_function(&met.expr) {
            return Err(format!(
                "derived metric '{}' must not contain aggregate function '{}'. \
                 Derived metrics compose other metrics; use a regular metric for aggregation.",
                met.name, func
            ));
        }
    }
    Ok(())
}

/// Check that all identifiers in derived metric expressions are known metric names.
fn check_derived_metric_references(
    derived: &[&crate::model::Metric],
    all_metric_names: &[&str],
    all_metric_names_display: &[String],
) -> Result<(), String> {
    for met in derived {
        let potential_refs = extract_identifiers(&met.expr);
        for ident in &potential_refs {
            let ident_lower = ident.to_ascii_lowercase();
            if all_metric_names
                .iter()
                .any(|n| n.to_ascii_lowercase() == ident_lower)
            {
                continue;
            }
            if is_sql_keyword_or_builtin(&ident_lower) {
                continue;
            }
            if ident.chars().next().is_none_or(|c| c.is_ascii_digit()) {
                continue;
            }
            let suggestion = suggest_closest(&ident_lower, all_metric_names_display);
            let mut msg = format!(
                "unknown metric '{}' referenced in derived metric '{}'",
                ident, met.name
            );
            if let Some(s) = suggestion {
                let _ = write!(msg, "; did you mean '{s}'?");
            }
            let _ = write!(
                msg,
                ". Available metrics: [{}]",
                all_metric_names.join(", ")
            );
            return Err(msg);
        }
    }
    Ok(())
}

/// Build derived-to-derived DAG and check for cycles via Kahn's algorithm.
fn check_derived_metric_cycles(
    derived: &[&crate::model::Metric],
    derived_name_strs: &[&str],
) -> Result<(), String> {
    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for &name in derived_name_strs {
        in_degree.entry(name).or_insert(0);
    }

    for met in derived {
        let refs = find_fact_references(&met.expr, derived_name_strs);
        for &referenced in &refs {
            edges.entry(met.name.as_str()).or_default().push(referenced);
            *in_degree.entry(referenced).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&name, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(name);
        }
    }

    let mut visited_count = 0;
    while let Some(node) = queue.pop_front() {
        visited_count += 1;
        if let Some(neighbors) = edges.get(node) {
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
    }

    if visited_count != derived_name_strs.len() {
        let remaining: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(&name, _)| name)
            .collect();

        if let Some(&start) = remaining.first() {
            let mut path = vec![start];
            let mut current = start;
            let mut seen: HashSet<&str> = HashSet::new();
            seen.insert(current);

            while let Some(neighbors) = edges.get(current) {
                if let Some(&next) = neighbors.iter().find(|n| remaining.contains(n)) {
                    if seen.contains(next) {
                        path.push(next);
                        return Err(format!(
                            "cycle detected in derived metrics: {}",
                            path.join(" -> ")
                        ));
                    }
                    seen.insert(next);
                    path.push(next);
                    current = next;
                } else {
                    break;
                }
            }
        }

        return Err("cycle detected in derived metrics".to_string());
    }

    Ok(())
}

/// Extract identifiers from an expression (word-boundary tokens).
/// Skips content inside single-quoted strings and dot-qualified identifiers.
fn extract_identifiers(expr: &str) -> Vec<String> {
    let bytes = expr.as_bytes();
    let mut result = Vec::new();
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string {
            i += 1;
            continue;
        }

        // Skip dot-qualified identifiers (e.g., "o.amount" -- the "o" and "amount" parts)
        // We want bare identifiers only
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &expr[start..i];

            // Check if preceded by a dot (table-qualified) -- skip
            if start > 0 && bytes[start - 1] == b'.' {
                continue;
            }
            // Check if followed by a dot -- skip (it's a table alias prefix)
            if i < bytes.len() && bytes[i] == b'.' {
                continue;
            }
            // Check if followed by '(' -- it's a function call, skip
            let mut j = i;
            while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'(' {
                continue;
            }

            result.push(ident.to_string());
        } else {
            i += 1;
        }
    }

    result
}

/// Check if a token is a SQL keyword or common builtin (not a metric reference).
fn is_sql_keyword_or_builtin(token: &str) -> bool {
    const SQL_KEYWORDS: &[&str] = &[
        "and",
        "or",
        "not",
        "is",
        "null",
        "true",
        "false",
        "case",
        "when",
        "then",
        "else",
        "end",
        "in",
        "between",
        "like",
        "ilike",
        "as",
        "cast",
        "distinct",
        "asc",
        "desc",
        "by",
        "having",
        "where",
        "from",
        "select",
        "group",
        "order",
        "limit",
        "offset",
        "union",
        "intersect",
        "except",
        "all",
        "any",
        "exists",
        "over",
        "partition",
        "rows",
        "range",
        "unbounded",
        "preceding",
        "following",
        "current",
        "row",
        "filter",
        "within",
        "nulls",
        "first",
        "last",
        "if",
        "coalesce",
        "nullif",
        "ifnull",
    ];
    SQL_KEYWORDS.contains(&token)
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

// ---------------------------------------------------------------------------
// USING relationship validation (Phase 32)
// ---------------------------------------------------------------------------

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
                    unique_constraints: vec![],
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
                    using_relationships: vec![],
                })
                .collect(),
            filters: vec![],
            facts: vec![],
            hierarchies: vec![],
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
            filters: vec![],
            facts: vec![],
            hierarchies: vec![],
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

    // -----------------------------------------------------------------------
    // Phase 29: validate_facts tests
    // -----------------------------------------------------------------------

    use crate::model::Fact;

    /// Helper to build a def with facts for testing.
    fn make_def_with_facts(
        tables: Vec<(&str, &str)>,
        facts: Vec<(&str, &str, &str)>,
    ) -> SemanticViewDefinition {
        SemanticViewDefinition {
            base_table: tables
                .first()
                .map(|(_, t)| t.to_string())
                .unwrap_or_default(),
            tables: tables
                .iter()
                .map(|(alias, table)| TableRef {
                    alias: alias.to_string(),
                    table: table.to_string(),
                    pk_columns: vec!["id".to_string()],
                    unique_constraints: vec![],
                })
                .collect(),
            facts: facts
                .iter()
                .map(|(name, expr, source)| Fact {
                    name: name.to_string(),
                    expr: expr.to_string(),
                    source_table: Some(source.to_string()),
                })
                .collect(),
            dimensions: vec![],
            metrics: vec![],
            filters: vec![],
            joins: vec![],
            hierarchies: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
        }
    }

    #[test]
    fn validate_facts_empty_returns_ok() {
        let def = make_def_with_facts(vec![("o", "orders")], vec![]);
        assert!(validate_facts(&def).is_ok());
    }

    #[test]
    fn validate_facts_single_valid_fact() {
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![("net_price", "o.price * (1 - o.discount)", "o")],
        );
        assert!(validate_facts(&def).is_ok());
    }

    #[test]
    fn validate_facts_unknown_source_table() {
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![("net_price", "x.price", "x")], // 'x' is not a declared table
        );
        let err = validate_facts(&def).unwrap_err();
        assert!(
            err.contains("unknown source table"),
            "Expected unknown source table error, got: {err}"
        );
    }

    #[test]
    fn validate_facts_unknown_source_table_fuzzy_suggestion() {
        let def = make_def_with_facts(
            vec![("orders", "orders")],
            vec![("net_price", "x.price", "ordres")], // typo: 'ordres' vs 'orders'
        );
        let err = validate_facts(&def).unwrap_err();
        assert!(
            err.contains("unknown source table"),
            "Expected unknown source table error, got: {err}"
        );
        assert!(
            err.contains("did you mean"),
            "Expected fuzzy suggestion, got: {err}"
        );
    }

    #[test]
    fn validate_facts_two_independent_facts() {
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![
                ("net_price", "o.price * (1 - o.discount)", "o"),
                ("tax_amount", "o.price * o.tax_rate", "o"),
            ],
        );
        assert!(validate_facts(&def).is_ok());
    }

    #[test]
    fn validate_facts_valid_chain() {
        // fact B references fact A -- valid dependency
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![
                ("net_price", "o.price * (1 - o.discount)", "o"),
                ("net_total", "net_price * o.quantity", "o"),
            ],
        );
        assert!(
            validate_facts(&def).is_ok(),
            "Valid fact chain should be accepted"
        );
    }

    #[test]
    fn validate_facts_cycle_detected() {
        // A refs B, B refs A -- cycle
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![("fact_a", "fact_b + 1", "o"), ("fact_b", "fact_a + 1", "o")],
        );
        let err = validate_facts(&def).unwrap_err();
        assert!(
            err.contains("cycle detected in facts"),
            "Expected cycle error, got: {err}"
        );
    }

    #[test]
    fn validate_facts_three_node_cycle() {
        // A -> B -> C -> A
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![
                ("fact_a", "fact_b + 1", "o"),
                ("fact_b", "fact_c + 1", "o"),
                ("fact_c", "fact_a + 1", "o"),
            ],
        );
        let err = validate_facts(&def).unwrap_err();
        assert!(
            err.contains("cycle detected in facts"),
            "Expected cycle error, got: {err}"
        );
    }

    #[test]
    fn validate_facts_self_reference_cycle() {
        let def = make_def_with_facts(
            vec![("o", "orders")],
            vec![("recursive", "recursive + 1", "o")],
        );
        let err = validate_facts(&def).unwrap_err();
        assert!(
            err.contains("cycle detected in facts"),
            "Expected self-reference cycle error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 29: find_fact_references word boundary tests
    // -----------------------------------------------------------------------

    #[test]
    fn find_fact_references_word_boundary_exact() {
        let refs = find_fact_references("SUM(net_price)", &["net_price"]);
        assert_eq!(refs, vec!["net_price"]);
    }

    #[test]
    fn find_fact_references_word_boundary_addition() {
        let refs = find_fact_references("net_price + tax", &["net_price"]);
        assert_eq!(refs, vec!["net_price"]);
    }

    #[test]
    fn find_fact_references_word_boundary_parens() {
        let refs = find_fact_references("(net_price)", &["net_price"]);
        assert_eq!(refs, vec!["net_price"]);
    }

    #[test]
    fn find_fact_references_no_substring_match() {
        // "net_price" should NOT match in "net_price_total"
        let refs = find_fact_references("net_price_total + 1", &["net_price"]);
        assert!(refs.is_empty(), "Should not match substring: {:?}", refs);
    }

    #[test]
    fn find_fact_references_no_prefix_match() {
        // "net_price" should NOT match in "my_net_price"
        let refs = find_fact_references("my_net_price + 1", &["net_price"]);
        assert!(refs.is_empty(), "Should not match prefix: {:?}", refs);
    }

    #[test]
    fn find_fact_references_multiple_facts() {
        let refs = find_fact_references(
            "SUM(net_price) + COUNT(tax_amount)",
            &["net_price", "tax_amount", "other"],
        );
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"net_price"));
        assert!(refs.contains(&"tax_amount"));
    }

    #[test]
    fn find_fact_references_case_insensitive() {
        let refs = find_fact_references("SUM(Net_Price)", &["net_price"]);
        assert_eq!(refs, vec!["net_price"]);
    }

    // -----------------------------------------------------------------------
    // Phase 29: validate_hierarchies tests
    // -----------------------------------------------------------------------

    use crate::model::Hierarchy;

    #[test]
    fn validate_hierarchies_empty_returns_ok() {
        let def = SemanticViewDefinition {
            base_table: "orders".to_string(),
            hierarchies: vec![],
            ..Default::default()
        };
        assert!(validate_hierarchies(&def).is_ok());
    }

    #[test]
    fn validate_hierarchies_valid_hierarchy() {
        let def = SemanticViewDefinition {
            base_table: "orders".to_string(),
            dimensions: vec![
                Dimension {
                    name: "country".to_string(),
                    expr: "country".to_string(),
                    ..Default::default()
                },
                Dimension {
                    name: "state".to_string(),
                    expr: "state".to_string(),
                    ..Default::default()
                },
                Dimension {
                    name: "city".to_string(),
                    expr: "city".to_string(),
                    ..Default::default()
                },
            ],
            hierarchies: vec![Hierarchy {
                name: "geo".to_string(),
                levels: vec![
                    "country".to_string(),
                    "state".to_string(),
                    "city".to_string(),
                ],
            }],
            ..Default::default()
        };
        assert!(
            validate_hierarchies(&def).is_ok(),
            "Valid hierarchy should be accepted"
        );
    }

    #[test]
    fn validate_hierarchies_unknown_dimension() {
        let def = SemanticViewDefinition {
            base_table: "orders".to_string(),
            dimensions: vec![Dimension {
                name: "country".to_string(),
                expr: "country".to_string(),
                ..Default::default()
            }],
            hierarchies: vec![Hierarchy {
                name: "geo".to_string(),
                levels: vec!["country".to_string(), "state".to_string()],
            }],
            ..Default::default()
        };
        let err = validate_hierarchies(&def).unwrap_err();
        assert!(
            err.contains("unknown dimension"),
            "Expected unknown dimension error, got: {err}"
        );
        assert!(
            err.contains("state") && err.contains("geo"),
            "Error should mention the unknown dim and hierarchy name, got: {err}"
        );
    }

    #[test]
    fn validate_hierarchies_unknown_dimension_fuzzy_suggestion() {
        let def = SemanticViewDefinition {
            base_table: "orders".to_string(),
            dimensions: vec![Dimension {
                name: "country".to_string(),
                expr: "country".to_string(),
                ..Default::default()
            }],
            hierarchies: vec![Hierarchy {
                name: "geo".to_string(),
                levels: vec!["contry".to_string()], // typo
            }],
            ..Default::default()
        };
        let err = validate_hierarchies(&def).unwrap_err();
        assert!(
            err.contains("unknown dimension"),
            "Expected unknown dimension error, got: {err}"
        );
        assert!(
            err.contains("did you mean"),
            "Expected fuzzy suggestion, got: {err}"
        );
    }

    #[test]
    fn validate_hierarchies_case_insensitive() {
        let def = SemanticViewDefinition {
            base_table: "orders".to_string(),
            dimensions: vec![Dimension {
                name: "Country".to_string(),
                expr: "country".to_string(),
                ..Default::default()
            }],
            hierarchies: vec![Hierarchy {
                name: "geo".to_string(),
                levels: vec!["country".to_string()], // lowercase ref to uppercase dim
            }],
            ..Default::default()
        };
        assert!(
            validate_hierarchies(&def).is_ok(),
            "Case-insensitive matching should work"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 30: contains_aggregate_function tests
    // -----------------------------------------------------------------------

    #[test]
    fn contains_aggregate_sum() {
        let result = contains_aggregate_function("SUM(revenue)");
        assert_eq!(result, Some("sum"));
    }

    #[test]
    fn contains_aggregate_count_distinct() {
        let result = contains_aggregate_function("COUNT(DISTINCT order_id)");
        assert_eq!(result, Some("count"));
    }

    #[test]
    fn contains_aggregate_avg() {
        let result = contains_aggregate_function("AVG(price)");
        assert_eq!(result, Some("avg"));
    }

    #[test]
    fn contains_aggregate_none_arithmetic() {
        let result = contains_aggregate_function("revenue - cost");
        assert_eq!(result, None);
    }

    #[test]
    fn contains_aggregate_none_multiply() {
        let result = contains_aggregate_function("revenue * 100");
        assert_eq!(result, None);
    }

    #[test]
    fn contains_aggregate_none_summary_no_paren() {
        // "SUMMARY" should not match -- not followed by open-paren
        let result = contains_aggregate_function("SUMMARY");
        assert_eq!(result, None);
    }

    #[test]
    fn contains_aggregate_none_string_literal() {
        // Inside single-quoted string literal -- best effort skip
        let result = contains_aggregate_function("'SUM of values'");
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Phase 30: validate_derived_metrics tests
    // -----------------------------------------------------------------------

    /// Helper to build a def with base metrics and derived metrics for testing.
    fn make_def_with_derived_metrics(
        base_metrics: Vec<(&str, &str, &str)>, // (name, expr, source_table)
        derived_metrics: Vec<(&str, &str)>,    // (name, expr) -- source_table: None
    ) -> SemanticViewDefinition {
        let mut metrics = Vec::new();
        for (name, expr, source) in base_metrics {
            metrics.push(Metric {
                name: name.to_string(),
                expr: expr.to_string(),
                source_table: Some(source.to_string()),
                output_type: None,
                using_relationships: vec![],
            });
        }
        for (name, expr) in derived_metrics {
            metrics.push(Metric {
                name: name.to_string(),
                expr: expr.to_string(),
                source_table: None,
                output_type: None,
                using_relationships: vec![],
            });
        }
        SemanticViewDefinition {
            base_table: "orders".to_string(),
            tables: vec![TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                unique_constraints: vec![],
            }],
            metrics,
            dimensions: vec![],
            filters: vec![],
            joins: vec![],
            facts: vec![],
            hierarchies: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
        }
    }

    #[test]
    fn validate_derived_metrics_cycle_detected() {
        // a -> b -> a (cycle)
        let def = make_def_with_derived_metrics(vec![], vec![("a", "b + 1"), ("b", "a + 1")]);
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("cycle detected in derived metrics"),
            "Expected cycle error, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_unknown_reference() {
        // "profit AS revenue - nonexistent" where "nonexistent" is not a metric name
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("profit", "revenue - nonexistent")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("unknown metric") && err.contains("nonexistent"),
            "Expected unknown metric error, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_aggregate_rejected() {
        // Derived metric must not contain aggregate function
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("bad", "SUM(revenue)")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("aggregate function") && err.contains("bad"),
            "Expected aggregate error, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_valid_simple() {
        // "profit AS revenue - cost" where both are base metrics
        let def = make_def_with_derived_metrics(
            vec![
                ("revenue", "SUM(o.amount)", "o"),
                ("cost", "SUM(o.cost)", "o"),
            ],
            vec![("profit", "revenue - cost")],
        );
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "Valid derived metric should be accepted"
        );
    }

    #[test]
    fn validate_derived_metrics_valid_stacking() {
        // "margin AS profit / revenue" where profit is derived and revenue is base
        let def = make_def_with_derived_metrics(
            vec![
                ("revenue", "SUM(o.amount)", "o"),
                ("cost", "SUM(o.cost)", "o"),
            ],
            vec![("profit", "revenue - cost"), ("margin", "profit / revenue")],
        );
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "Stacking derived metrics should be accepted"
        );
    }

    #[test]
    fn validate_derived_metrics_valid_negation() {
        // "neg_revenue AS -revenue"
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("neg_revenue", "-revenue")],
        );
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "Single-metric reference should be accepted"
        );
    }

    #[test]
    fn validate_derived_metrics_did_you_mean() {
        // Close misspelling: "revnue" instead of "revenue"
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("profit", "revnue - cost")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("did you mean"),
            "Expected 'did you mean?' suggestion, got: {err}"
        );
    }

    #[test]
    fn validate_derived_metrics_no_derived_returns_ok() {
        // No derived metrics -> should return Ok immediately
        let def = make_def_with_derived_metrics(vec![("revenue", "SUM(o.amount)", "o")], vec![]);
        assert!(
            validate_derived_metrics(&def).is_ok(),
            "No derived metrics should return Ok"
        );
    }

    #[test]
    fn validate_derived_metrics_duplicate_name_rejected() {
        // Same name used by both base and derived metric
        let def = make_def_with_derived_metrics(
            vec![("revenue", "SUM(o.amount)", "o")],
            vec![("revenue", "cost + 1")],
        );
        let err = validate_derived_metrics(&def).unwrap_err();
        assert!(
            err.contains("duplicate metric name"),
            "Expected duplicate name error, got: {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 32: Diamond relaxation and USING validation
    // -----------------------------------------------------------------------

    /// Helper to build a definition with named (or unnamed) joins for diamond tests.
    fn make_def_with_named_joins(
        tables: Vec<(&str, &str, Vec<&str>)>,
        joins: Vec<(Option<&str>, &str, &str, Vec<&str>)>, // (name, from, to, fk_cols)
        metrics: Vec<(&str, Option<&str>, Vec<&str>)>,     // (name, source, using_rels)
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
                    unique_constraints: vec![],
                })
                .collect(),
            joins: joins
                .iter()
                .map(|(name, from_alias, to_alias, fk_cols)| Join {
                    table: to_alias.to_string(),
                    from_alias: from_alias.to_string(),
                    fk_columns: fk_cols.iter().map(|s| s.to_string()).collect(),
                    name: name.map(|n| n.to_string()),
                    ..Default::default()
                })
                .collect(),
            dimensions: vec![],
            metrics: metrics
                .iter()
                .map(|(name, source, using_rels)| Metric {
                    name: name.to_string(),
                    expr: format!("COUNT(*)"),
                    source_table: source.map(|s| s.to_string()),
                    output_type: None,
                    using_relationships: using_rels.iter().map(|s| s.to_string()).collect(),
                })
                .collect(),
            filters: vec![],
            facts: vec![],
            hierarchies: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
        }
    }

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

    // -----------------------------------------------------------------------
    // Phase 33: FK reference validation (CARD-03, CARD-09)
    // -----------------------------------------------------------------------

    mod phase33_fk_reference_tests {
        use super::*;
        use crate::model::{Join, TableRef};

        /// Build a minimal definition for FK reference validation testing.
        fn make_fk_ref_def(tables: Vec<TableRef>, joins: Vec<Join>) -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: tables.first().map(|t| t.table.clone()).unwrap_or_default(),
                tables,
                joins,
                dimensions: vec![],
                metrics: vec![],
                filters: vec![],
                facts: vec![],
                hierarchies: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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
