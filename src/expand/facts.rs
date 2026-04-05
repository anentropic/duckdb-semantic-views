use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::Fact;
use crate::util::{is_word_boundary_char, replace_word_boundary};

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
) -> Vec<usize> {
    let n = derived.len();
    if n == 0 {
        return Vec::new();
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

    // If order.len() != n, there is a cycle. This should have been caught at
    // define time by validate_derived_metrics, but as a defensive fallback we
    // just return what we have.
    order
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
) -> HashMap<String, String> {
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
        return resolved;
    }

    // Step 3: Topologically sort derived metrics and inline in order
    let derived_topo = toposort_derived(&derived, &resolved);

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

    resolved
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
