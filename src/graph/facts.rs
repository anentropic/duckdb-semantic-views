//! Fact validation (Phase 29).
//!
//! Validates fact source tables, fact-to-fact references, and detects cycles
//! in the fact dependency DAG.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use crate::model::SemanticViewDefinition;
use crate::util::suggest_closest;

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

#[cfg(test)]
mod tests {
    use crate::graph::{find_fact_references, validate_facts};

    use super::super::test_helpers::*;

    // -----------------------------------------------------------------------
    // Phase 29: validate_facts tests
    // -----------------------------------------------------------------------

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
}
