use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::Fact;
use crate::util::{is_word_boundary_char, replace_word_boundary};

/// Maximum allowed nesting depth for derived metric resolution.
/// Prevents stack overflow from deeply nested metric chains that pass
/// cycle detection (linear chains: a->b->c->d->... up to 64 levels).
const MAX_DERIVATION_DEPTH: usize = 64;

/// Collect `using_relationships` from all transitive base metrics referenced by a derived metric.
pub(super) fn collect_derived_metric_using(
    met: &crate::model::Metric,
    all_metrics: &[crate::model::Metric],
) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = vec![met.name.to_ascii_lowercase()];

    let name_map: HashMap<String, &crate::model::Metric> = all_metrics
        .iter()
        .map(|m| (m.name.to_ascii_lowercase(), m))
        .collect();

    let all_names: Vec<String> = all_metrics
        .iter()
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    while let Some(current_name) = stack.pop() {
        if !visited.insert(current_name.clone()) {
            continue;
        }
        let Some(current_met) = name_map.get(&current_name) else {
            continue;
        };

        if current_met.source_table.is_some() {
            // Base metric: collect its USING relationships
            for rel in &current_met.using_relationships {
                if !result.contains(rel) {
                    result.push(rel.clone());
                }
            }
        } else {
            // Derived metric: find referenced metric names and push to stack
            let expr_lower = current_met.expr.to_ascii_lowercase();
            for name in &all_names {
                if *name == current_name {
                    continue;
                }
                let expr_bytes = expr_lower.as_bytes();
                let name_bytes = name.as_bytes();
                let name_len = name_bytes.len();
                let mut pos = 0;
                while pos + name_len <= expr_bytes.len() {
                    if &expr_bytes[pos..pos + name_len] == name_bytes {
                        let before_ok = pos == 0 || is_word_boundary_char(expr_bytes[pos - 1]);
                        let after_ok = pos + name_len == expr_bytes.len()
                            || is_word_boundary_char(expr_bytes[pos + name_len]);
                        if before_ok && after_ok {
                            stack.push(name.clone());
                            break;
                        }
                    }
                    pos += 1;
                }
            }
        }
    }

    result
}

/// Topologically sort facts by their inter-dependencies (leaf facts first).
///
/// Uses Kahn's algorithm. Returns indices into the `facts` slice in topological
/// order (facts with no dependencies on other facts come first).
///
/// Returns `Err` if a cycle is detected (defensive -- `validate_facts` should
/// have already rejected cycles at CREATE time).
pub(super) fn toposort_facts(facts: &[Fact]) -> Result<Vec<usize>, String> {
    let n = facts.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    // Build name -> index map (case-insensitive)
    let name_to_idx: HashMap<String, usize> = facts
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.to_ascii_lowercase(), i))
        .collect();

    // Build adjacency: edges[i] = set of indices that fact i depends on
    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n]; // dependents[dep] = facts that depend on dep

    for (i, fact) in facts.iter().enumerate() {
        let expr_lower = fact.expr.to_ascii_lowercase();
        for (name, &dep_idx) in &name_to_idx {
            if dep_idx == i {
                continue; // skip self
            }
            // Check if this fact's expr references the other fact name using word boundary
            let expr_bytes = expr_lower.as_bytes();
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len();
            let mut pos = 0;
            while pos + name_len <= expr_bytes.len() {
                if &expr_bytes[pos..pos + name_len] == name_bytes {
                    let before_ok = pos == 0 || is_word_boundary_char(expr_bytes[pos - 1]);
                    let after_ok = pos + name_len == expr_bytes.len()
                        || is_word_boundary_char(expr_bytes[pos + name_len]);
                    if before_ok && after_ok {
                        in_degree[i] += 1;
                        dependents[dep_idx].push(i);
                        break;
                    }
                }
                pos += 1;
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for &dep in &dependents[idx] {
            in_degree[dep] -= 1;
            if in_degree[dep] == 0 {
                queue.push_back(dep);
            }
        }
    }

    if order.len() != n {
        return Err("cycle detected in facts".to_string());
    }

    Ok(order)
}

/// Inline fact expressions into a metric expression.
///
/// Processes facts in topological order (leaf facts first), resolving each fact's
/// expression by inlining any previously-resolved facts. Then applies all resolved
/// facts to the input `expr`.
///
/// Each inlined fact expression is parenthesized to preserve operator precedence:
/// `net_price = price * (1 - discount)` inlined into `SUM(net_price)` becomes
/// `SUM((price * (1 - discount)))`.
pub(super) fn inline_facts(expr: &str, facts: &[Fact], topo_order: &[usize]) -> String {
    if facts.is_empty() || topo_order.is_empty() {
        return expr.to_string();
    }

    // Build resolved expressions in topological order
    let mut resolved: HashMap<String, String> = HashMap::new();

    for &idx in topo_order {
        let fact = &facts[idx];
        let mut resolved_expr = fact.expr.clone();

        // Inline any already-resolved facts into this fact's expression
        for (name, replacement) in &resolved {
            // Try qualified form: source_table.name
            if let Some(ref st) = fact.source_table {
                let qualified = format!("{st}.{name}");
                resolved_expr = replace_word_boundary(&resolved_expr, &qualified, replacement);
            }
            // Unqualified form
            resolved_expr = replace_word_boundary(&resolved_expr, name, replacement);
        }

        // Store as parenthesized
        let parenthesized = format!("({resolved_expr})");
        resolved.insert(fact.name.clone(), parenthesized);
    }

    // Apply all resolved facts to the input expression
    let mut result = expr.to_string();
    // Process in topo order to ensure consistent replacement
    for &idx in topo_order {
        let fact = &facts[idx];
        if let Some(replacement) = resolved.get(&fact.name) {
            // Try qualified form first: source_table.name
            if let Some(ref st) = fact.source_table {
                let qualified = format!("{st}.{}", fact.name);
                result = replace_word_boundary(&result, &qualified, replacement);
            }
            // Unqualified form
            result = replace_word_boundary(&result, &fact.name, replacement);
        }
    }

    result
}

/// Topologically sort derived metrics by their inter-dependencies.
///
/// Uses Kahn's algorithm. Only derived-to-derived edges are considered;
/// references to base metrics are external and do not contribute to in-degree.
/// Returns indices into the `derived` slice in resolution order.
fn toposort_derived(
    derived: &[(usize, &crate::model::Metric)],
    _resolved_names: &HashMap<String, String>,
) -> Result<Vec<usize>, String> {
    let n = derived.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    // Build name -> index-in-derived-slice map (lowercased)
    let name_to_idx: HashMap<String, usize> = derived
        .iter()
        .enumerate()
        .map(|(i, (_, m))| (m.name.to_ascii_lowercase(), i))
        .collect();

    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, (_, met)) in derived.iter().enumerate() {
        let expr_lower = met.expr.to_ascii_lowercase();
        for (name, &dep_idx) in &name_to_idx {
            if dep_idx == i {
                continue; // skip self
            }
            // Check word-boundary reference
            let expr_bytes = expr_lower.as_bytes();
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len();
            let mut pos = 0;
            while pos + name_len <= expr_bytes.len() {
                if &expr_bytes[pos..pos + name_len] == name_bytes {
                    let before_ok = pos == 0 || is_word_boundary_char(expr_bytes[pos - 1]);
                    let after_ok = pos + name_len == expr_bytes.len()
                        || is_word_boundary_char(expr_bytes[pos + name_len]);
                    if before_ok && after_ok {
                        in_degree[i] += 1;
                        dependents[dep_idx].push(i);
                        break;
                    }
                }
                pos += 1;
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for &dep in &dependents[idx] {
            in_degree[dep] -= 1;
            if in_degree[dep] == 0 {
                queue.push_back(dep);
            }
        }
    }

    if order.len() != n {
        // Build cycle description from remaining nodes
        let remaining: Vec<&str> = derived
            .iter()
            .enumerate()
            .filter(|(i, _)| !order.contains(i))
            .map(|(_, (_, m))| m.name.as_str())
            .collect();
        return Err(format!(
            "cycle in derived metrics: [{}]",
            remaining.join(", ")
        ));
    }
    Ok(order)
}

/// Resolve all metric expressions: inline facts into base metrics, then inline
/// base/derived metric references into derived metrics in topological order.
///
/// Returns a map from lowercased metric name to its fully-resolved expression.
///
/// Processing order:
/// 1. Base metrics (`source_table.is_some()`): inline facts, store resolved expression
/// 2. Derived metrics (`source_table.is_none()`): topologically sort by inter-metric deps,
///    then for each derived metric, replace all known metric name references with
///    parenthesized resolved expressions
pub(super) fn inline_derived_metrics(
    metrics: &[crate::model::Metric],
    facts: &[Fact],
    fact_topo_order: &[usize],
) -> Result<HashMap<String, String>, String> {
    let mut resolved: HashMap<String, String> = HashMap::new();

    // Step 1: Resolve base metrics (have source_table) with fact inlining
    for met in metrics.iter().filter(|m| m.source_table.is_some()) {
        let expr = if facts.is_empty() {
            met.expr.clone()
        } else {
            inline_facts(&met.expr, facts, fact_topo_order)
        };
        resolved.insert(met.name.to_ascii_lowercase(), expr);
    }

    // Step 2: Collect derived metrics (no source_table)
    let derived: Vec<(usize, &crate::model::Metric)> = metrics
        .iter()
        .enumerate()
        .filter(|(_, m)| m.source_table.is_none())
        .collect();

    if derived.is_empty() {
        return Ok(resolved);
    }

    // Step 3: Topologically sort derived metrics and inline in order
    let derived_topo = toposort_derived(&derived, &resolved)?;

    // Step 3b: Enforce depth limit to prevent stack overflow from long chains
    if derived_topo.len() > MAX_DERIVATION_DEPTH {
        return Err(format!(
            "derived metric nesting depth {} exceeds maximum of {}",
            derived_topo.len(),
            MAX_DERIVATION_DEPTH
        ));
    }

    for idx in derived_topo {
        let met = derived[idx].1;
        // Start with the raw expression, with facts inlined first
        let mut expr = if facts.is_empty() {
            met.expr.clone()
        } else {
            inline_facts(&met.expr, facts, fact_topo_order)
        };
        // Replace each known metric name with its resolved expression (parenthesized)
        for (name, replacement) in &resolved {
            expr = replace_word_boundary(&expr, name, &format!("({replacement})"));
        }
        resolved.insert(met.name.to_ascii_lowercase(), expr);
    }

    Ok(resolved)
}

/// Collect source tables needed by a derived metric by walking the metric
/// dependency graph transitively.
pub(crate) fn collect_derived_metric_source_tables(
    met: &crate::model::Metric,
    all_metrics: &[crate::model::Metric],
) -> Vec<String> {
    let mut sources: HashSet<String> = HashSet::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = vec![met.name.to_ascii_lowercase()];

    // Build name -> metric lookup
    let name_map: HashMap<String, &crate::model::Metric> = all_metrics
        .iter()
        .map(|m| (m.name.to_ascii_lowercase(), m))
        .collect();

    // Collect all metric names for word-boundary scanning
    let all_names: Vec<String> = all_metrics
        .iter()
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    while let Some(current_name) = stack.pop() {
        if !visited.insert(current_name.clone()) {
            continue;
        }
        let Some(current_met) = name_map.get(&current_name) else {
            continue;
        };

        if let Some(ref st) = current_met.source_table {
            // Base metric: add its source table
            sources.insert(st.to_ascii_lowercase());
        } else {
            // Derived metric: find referenced metric names and push to stack
            let expr_lower = current_met.expr.to_ascii_lowercase();
            for name in &all_names {
                if *name == current_name {
                    continue;
                }
                // Word-boundary check
                let expr_bytes = expr_lower.as_bytes();
                let name_bytes = name.as_bytes();
                let name_len = name_bytes.len();
                let mut pos = 0;
                while pos + name_len <= expr_bytes.len() {
                    if &expr_bytes[pos..pos + name_len] == name_bytes {
                        let before_ok = pos == 0 || is_word_boundary_char(expr_bytes[pos - 1]);
                        let after_ok = pos + name_len == expr_bytes.len()
                            || is_word_boundary_char(expr_bytes[pos + name_len]);
                        if before_ok && after_ok {
                            stack.push(name.clone());
                            break;
                        }
                    }
                    pos += 1;
                }
            }
        }
    }

    sources.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccessModifier, Metric};

    fn make_metric(name: &str, expr: &str, source_table: Option<&str>) -> Metric {
        Metric {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: source_table.map(|s| s.to_string()),
            output_type: None,
            using_relationships: vec![],
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
            non_additive_by: vec![],
            window_spec: None,
        }
    }

    #[test]
    fn toposort_derived_detects_cycle() {
        let met_a = make_metric("a", "b + 1", None);
        let met_b = make_metric("b", "a + 1", None);
        let derived: Vec<(usize, &Metric)> = vec![(0, &met_a), (1, &met_b)];
        let resolved = HashMap::new();
        let result = toposort_derived(&derived, &resolved);
        assert!(result.is_err(), "Expected cycle error");
        assert!(
            result.unwrap_err().contains("cycle"),
            "Error should mention cycle"
        );
    }

    #[test]
    fn toposort_derived_no_cycle_succeeds() {
        let _met_a = make_metric("a", "SUM(x)", Some("t"));
        let met_b = make_metric("b", "a + 1", None);
        // Only derived metrics go into toposort_derived; 'a' is base
        let derived: Vec<(usize, &Metric)> = vec![(1, &met_b)];
        let resolved = HashMap::new();
        let result = toposort_derived(&derived, &resolved);
        assert!(result.is_ok(), "Non-cyclic should succeed");
    }

    #[test]
    fn max_derivation_depth_constant() {
        assert_eq!(MAX_DERIVATION_DEPTH, 64);
    }

    #[test]
    fn inline_derived_metrics_cycle_returns_err() {
        let metrics = vec![
            make_metric("a", "b + 1", None),
            make_metric("b", "a + 1", None),
        ];
        let result = inline_derived_metrics(&metrics, &[], &[]);
        assert!(result.is_err(), "Cycle should produce error");
        let err = result.unwrap_err();
        assert!(err.contains("cycle"), "Error should mention cycle: {err}");
    }

    #[test]
    fn inline_derived_metrics_normal_succeeds() {
        let metrics = vec![
            make_metric("revenue", "SUM(amount)", Some("o")),
            make_metric("cost", "SUM(unit_cost)", Some("o")),
            make_metric("profit", "revenue - cost", None),
        ];
        let result = inline_derived_metrics(&metrics, &[], &[]);
        assert!(result.is_ok(), "Non-cyclic should succeed");
        let resolved = result.unwrap();
        assert_eq!(
            resolved.get("profit").unwrap(),
            "(SUM(amount)) - (SUM(unit_cost))"
        );
    }

    #[test]
    fn inline_derived_metrics_depth_limit_exceeded() {
        // Create a chain of 65 derived metrics: m0 -> m1 -> ... -> m64
        // m0 is base, m1..m64 are derived (64 derived exceeds the limit since
        // MAX_DERIVATION_DEPTH == 64 and we check > not >=)
        let mut metrics = vec![make_metric("m0", "SUM(x)", Some("t"))];
        for i in 1..=MAX_DERIVATION_DEPTH + 1 {
            metrics.push(make_metric(
                &format!("m{i}"),
                &format!("m{} + 1", i - 1),
                None,
            ));
        }
        let result = inline_derived_metrics(&metrics, &[], &[]);
        assert!(result.is_err(), "Depth exceeding limit should error");
        let err = result.unwrap_err();
        assert!(
            err.contains("nesting depth") && err.contains("maximum"),
            "Error should mention depth limit: {err}"
        );
    }

    // --- Helper for metrics with using_relationships ---

    fn make_metric_with_using(
        name: &str,
        expr: &str,
        source_table: Option<&str>,
        using: &[&str],
    ) -> Metric {
        let mut m = make_metric(name, expr, source_table);
        m.using_relationships = using.iter().map(|s| s.to_string()).collect();
        m
    }

    fn make_fact(name: &str, expr: &str) -> Fact {
        Fact {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: Some("t".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
        }
    }

    // --- collect_derived_metric_using tests ---

    #[test]
    fn test_collect_derived_metric_using_base_with_using() {
        let met = make_metric_with_using(
            "flight_count",
            "count(*)",
            Some("flights"),
            &["dep_airport"],
        );
        let result = collect_derived_metric_using(&met, &[met.clone()]);
        assert!(
            result.contains(&"dep_airport".to_string()),
            "Should contain dep_airport, got: {result:?}"
        );
    }

    #[test]
    fn test_collect_derived_metric_using_derived_transitive() {
        let base_met = make_metric_with_using("base_count", "count(*)", Some("flights"), &["rel1"]);
        let derived = make_metric("derived_total", "base_count + 1", None);
        let all = vec![base_met, derived.clone()];
        let result = collect_derived_metric_using(&derived, &all);
        assert!(
            result.contains(&"rel1".to_string()),
            "Should transitively contain rel1, got: {result:?}"
        );
    }

    #[test]
    fn test_collect_derived_metric_using_no_using() {
        let met = make_metric("revenue", "sum(amount)", Some("orders"));
        let result = collect_derived_metric_using(&met, &[met.clone()]);
        assert!(
            result.is_empty(),
            "No using_relationships should return empty"
        );
    }

    #[test]
    fn test_collect_derived_metric_using_multiple_transitive() {
        // Derived metric references two base metrics each with different USING
        let base1 =
            make_metric_with_using("dep_count", "count(*)", Some("flights"), &["dep_airport"]);
        let base2 =
            make_metric_with_using("arr_count", "count(*)", Some("flights"), &["arr_airport"]);
        let derived = make_metric("total_count", "dep_count + arr_count", None);
        let all = vec![base1, base2, derived.clone()];
        let result = collect_derived_metric_using(&derived, &all);
        assert!(
            result.contains(&"dep_airport".to_string()),
            "Should contain dep_airport, got: {result:?}"
        );
        assert!(
            result.contains(&"arr_airport".to_string()),
            "Should contain arr_airport, got: {result:?}"
        );
    }

    // --- toposort_facts tests ---

    #[test]
    fn test_toposort_facts_empty() {
        let result = toposort_facts(&[]);
        assert_eq!(result.unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn test_toposort_facts_single() {
        let facts = vec![make_fact("net_price", "price * (1 - discount)")];
        let result = toposort_facts(&facts).unwrap();
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn test_toposort_facts_chain() {
        let facts = vec![
            make_fact("net_price", "price * (1 - discount)"),
            make_fact("total", "net_price * quantity"),
        ];
        let result = toposort_facts(&facts).unwrap();
        // net_price (index 0) must come before total (index 1)
        let pos_net = result.iter().position(|&x| x == 0).unwrap();
        let pos_total = result.iter().position(|&x| x == 1).unwrap();
        assert!(
            pos_net < pos_total,
            "net_price should come before total in topo order"
        );
    }

    #[test]
    fn test_toposort_facts_independent() {
        let facts = vec![
            make_fact("tax_amount", "price * tax_rate"),
            make_fact("discount_amount", "price * discount_rate"),
        ];
        let result = toposort_facts(&facts).unwrap();
        assert_eq!(result.len(), 2, "Both facts should appear");
        assert!(result.contains(&0));
        assert!(result.contains(&1));
    }

    #[test]
    fn test_toposort_facts_cycle() {
        let facts = vec![make_fact("a", "b + 1"), make_fact("b", "a + 1")];
        let result = toposort_facts(&facts);
        assert!(result.is_err(), "Cycle should be detected");
        assert!(
            result.unwrap_err().contains("cycle"),
            "Error should mention cycle"
        );
    }

    // --- inline_facts tests ---

    #[test]
    fn test_inline_facts_empty_facts() {
        let result = inline_facts("SUM(amount)", &[], &[]);
        assert_eq!(
            result, "SUM(amount)",
            "Empty facts should return expr unchanged"
        );
    }

    #[test]
    fn test_inline_facts_single_substitution() {
        let facts = vec![make_fact("net_price", "price * (1 - discount)")];
        let topo = vec![0];
        let result = inline_facts("SUM(net_price)", &facts, &topo);
        assert_eq!(
            result, "SUM((price * (1 - discount)))",
            "Should inline the fact expression parenthesized"
        );
    }

    #[test]
    fn test_inline_facts_chained_substitution() {
        let facts = vec![
            make_fact("net_price", "price * (1 - discount)"),
            make_fact("total", "net_price * quantity"),
        ];
        // topo order: net_price first (index 0), then total (index 1)
        let topo = vec![0, 1];
        let result = inline_facts("SUM(total)", &facts, &topo);
        // total resolves to ((price * (1 - discount)) * quantity)
        assert!(
            result.contains("price * (1 - discount)"),
            "Should resolve inner fact first, got: {result}"
        );
        assert!(
            result.contains("quantity"),
            "Should contain quantity, got: {result}"
        );
    }

    #[test]
    fn test_inline_facts_qualified_form() {
        let mut fact = make_fact("net_price", "price * (1 - discount)");
        fact.source_table = Some("o".to_string());
        let facts = vec![fact];
        let topo = vec![0];
        let result = inline_facts("SUM(o.net_price)", &facts, &topo);
        assert!(
            result.contains("price * (1 - discount)"),
            "Should replace qualified form o.net_price, got: {result}"
        );
    }

    // --- collect_derived_metric_source_tables tests ---

    #[test]
    fn test_collect_source_tables_base_metric() {
        let met = make_metric("revenue", "sum(amount)", Some("orders"));
        let result = collect_derived_metric_source_tables(&met, &[met.clone()]);
        assert!(
            result.contains(&"orders".to_string()),
            "Should contain orders, got: {result:?}"
        );
    }

    #[test]
    fn test_collect_source_tables_derived_transitive() {
        let base = make_metric("revenue", "sum(amount)", Some("orders"));
        let derived = make_metric("profit", "revenue - cost", None);
        let cost = make_metric("cost", "sum(unit_cost)", Some("items"));
        let all = vec![base, derived.clone(), cost];
        let result = collect_derived_metric_source_tables(&derived, &all);
        assert!(
            result.contains(&"orders".to_string()),
            "Should transitively contain orders, got: {result:?}"
        );
        assert!(
            result.contains(&"items".to_string()),
            "Should transitively contain items, got: {result:?}"
        );
    }

    #[test]
    fn test_collect_source_tables_cycle_handling() {
        // Two metrics referencing each other (defensive: visited set prevents infinite loop)
        let met_a = make_metric("a", "b + 1", None);
        let met_b = make_metric("b", "a + 1", None);
        let all = vec![met_a.clone(), met_b];
        // Should terminate without hanging
        let result = collect_derived_metric_source_tables(&met_a, &all);
        // No source tables found (both are derived with no base)
        assert!(
            result.is_empty(),
            "Cycle with no base metrics should return empty, got: {result:?}"
        );
    }
}
