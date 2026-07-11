use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{Fact, TableRef};
use crate::util::{is_word_boundary_char, replace_word_boundary_any, replace_word_boundary_pairs};

use super::resolution::quote_ident;

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

        // Inline any already-resolved facts into this fact's expression.
        // Qualified (`alias.name`) and unqualified (`name`) forms are replaced in a
        // single pass so a replacement containing the unqualified name is not
        // re-scanned (see `replace_word_boundary_any`).
        for (name, replacement) in &resolved {
            let qualified = fact.source_table.as_ref().map(|st| format!("{st}.{name}"));
            let mut needles: Vec<&str> = Vec::with_capacity(2);
            if let Some(ref q) = qualified {
                needles.push(q);
            }
            needles.push(name);
            resolved_expr = replace_word_boundary_any(&resolved_expr, &needles, replacement);
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
            // Replace qualified (`alias.name`) and unqualified (`name`) forms in a
            // single pass. Sequential calls would double-substitute an identity
            // fact whose replacement contains its own unqualified name.
            let qualified = fact
                .source_table
                .as_ref()
                .map(|st| format!("{st}.{}", fact.name));
            let mut needles: Vec<&str> = Vec::with_capacity(2);
            if let Some(ref q) = qualified {
                needles.push(q);
            }
            needles.push(&fact.name);
            result = replace_word_boundary_any(&result, &needles, replacement);
        }
    }

    result
}

/// Replace every `COUNT(*)` call in `expr` with `COUNT(<replacement_arg>)`.
///
/// Matches `count` case-insensitively at a word boundary, followed by
/// optional whitespace, `(`, optional whitespace, `*`, optional whitespace,
/// `)` — i.e. `COUNT(*)`, `count( * )`, etc. The original casing of the
/// function name is preserved; only the argument is replaced. Occurrences
/// inside single-quoted SQL string literals are left untouched.
///
/// Returns `None` when the expression contains no `COUNT(*)` call.
pub(super) fn rewrite_count_star(expr: &str, replacement_arg: &str) -> Option<String> {
    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len() + replacement_arg.len());
    let mut copied = 0usize; // byte offset copied into `out` so far
    let mut pos = 0usize;
    let mut in_string = false;
    let mut changed = false;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte == b'\'' {
            in_string = !in_string;
            pos += 1;
            continue;
        }
        if in_string {
            pos += 1;
            continue;
        }
        if (byte == b'c' || byte == b'C')
            && pos + 5 <= bytes.len()
            && bytes[pos..pos + 5].eq_ignore_ascii_case(b"count")
            && (pos == 0 || is_word_boundary_char(bytes[pos - 1]))
        {
            let mut open_paren = pos + 5;
            while open_paren < bytes.len() && bytes[open_paren].is_ascii_whitespace() {
                open_paren += 1;
            }
            if open_paren < bytes.len() && bytes[open_paren] == b'(' {
                let mut star = open_paren + 1;
                while star < bytes.len() && bytes[star].is_ascii_whitespace() {
                    star += 1;
                }
                if star < bytes.len() && bytes[star] == b'*' {
                    let mut close_paren = star + 1;
                    while close_paren < bytes.len() && bytes[close_paren].is_ascii_whitespace() {
                        close_paren += 1;
                    }
                    if close_paren < bytes.len() && bytes[close_paren] == b')' {
                        // All scanned offsets sit on ASCII bytes, so slicing
                        // is char-boundary safe. Copy through the `(`, swap
                        // the `*` (and its padding) for the replacement.
                        out.push_str(&expr[copied..=open_paren]);
                        out.push_str(replacement_arg);
                        out.push(')');
                        copied = close_paren + 1;
                        pos = close_paren + 1;
                        changed = true;
                        continue;
                    }
                }
            }
        }
        pos += 1;
    }
    if !changed {
        return None;
    }
    out.push_str(&expr[copied..]);
    Some(out)
}

/// Resolved metric expressions plus SG-8 rewrite failures.
///
/// Produced by [`inline_derived_metrics`]. `count_star_no_pk` is keyed by
/// lowercased metric name and holds the lowercased source-table alias of each
/// base metric whose `COUNT(*)` could NOT be rewritten (non-base source table
/// with no PRIMARY KEY declared). Erroring is the caller's job so that only
/// queries which actually use such a metric fail — unrelated metrics on the
/// same view keep working.
#[derive(Debug)]
pub(super) struct ResolvedMetricExprs {
    /// Lowercased metric name -> fully-resolved expression.
    pub exprs: HashMap<String, String>,
    /// Lowercased metric name -> lowercased source-table alias for metrics
    /// with an unrewritable `COUNT(*)` (SG-8).
    pub count_star_no_pk: HashMap<String, String>,
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
/// Returns a map from lowercased metric name to its fully-resolved expression,
/// plus the SG-8 rewrite failures (see [`ResolvedMetricExprs`]).
///
/// Processing order:
/// 1. Base metrics (`source_table.is_some()`): inline facts, apply the SG-8
///    `COUNT(*)` rewrite (below), store resolved expression
/// 2. Derived metrics (`source_table.is_none()`): topologically sort by inter-metric deps,
///    then for each derived metric, replace all known metric name references with
///    parenthesized resolved expressions
///
/// # SG-8: `COUNT(*)` rewrite for non-base source tables
///
/// All synthesized joins are LEFT JOINs, so a metric sourced on a table other
/// than the base/root table sees one NULL-extended row per base row with no
/// match — `COUNT(*)` silently over-counts by one per childless parent. For
/// every base metric whose `source_table` is not the first declared table,
/// `COUNT(*)` is rewritten to `COUNT("<alias>"."<pk>")` using the FIRST
/// PRIMARY KEY column declared for that table (NULL-extended rows have a NULL
/// PK and are excluded from the count). The alias is lowercased to match the
/// alias emitted in the JOIN clause. Metrics on the base table keep plain
/// `COUNT(*)` — the base table is never NULL-extended. Because the rewrite
/// runs here, at the shared base-metric resolution step, it propagates to
/// every emission path that consumes resolved expressions: the main
/// aggregation path, derived-metric inlining, semi-additive co-query
/// decomposition, and window-metric inner aggregates. If the source table has
/// no PRIMARY KEY declared the rewrite is impossible; the metric is recorded
/// in `count_star_no_pk` and the caller errors when (and only when) a query
/// actually uses it.
pub(super) fn inline_derived_metrics(
    metrics: &[crate::model::Metric],
    facts: &[Fact],
    fact_topo_order: &[usize],
    tables: &[TableRef],
) -> Result<ResolvedMetricExprs, String> {
    let mut resolved: HashMap<String, String> = HashMap::new();
    let mut count_star_no_pk: HashMap<String, String> = HashMap::new();
    let base_alias = tables.first().map(|t| t.alias.to_ascii_lowercase());

    // Step 1: Resolve base metrics (have source_table) with fact inlining
    for met in metrics.iter().filter(|m| m.source_table.is_some()) {
        let mut expr = if facts.is_empty() {
            met.expr.clone()
        } else {
            inline_facts(&met.expr, facts, fact_topo_order)
        };
        // SG-8: COUNT(*) on a non-base source table (see doc comment above).
        if let Some(ref st) = met.source_table {
            let st_lower = st.to_ascii_lowercase();
            if base_alias.as_deref() != Some(st_lower.as_str()) {
                let pk = tables
                    .iter()
                    .find(|t| t.alias.to_ascii_lowercase() == st_lower)
                    .and_then(|t| t.pk_columns.first());
                if let Some(pk) = pk {
                    let qualified_pk = format!("{}.{}", quote_ident(&st_lower), quote_ident(pk));
                    if let Some(rewritten) = rewrite_count_star(&expr, &qualified_pk) {
                        expr = rewritten;
                    }
                } else if rewrite_count_star(&expr, "*").is_some() {
                    // No PK declared (or unknown alias): rewrite impossible.
                    count_star_no_pk.insert(met.name.to_ascii_lowercase(), st_lower);
                }
            }
        }
        resolved.insert(met.name.to_ascii_lowercase(), expr);
    }

    // Step 2: Collect derived metrics (no source_table)
    let derived: Vec<(usize, &crate::model::Metric)> = metrics
        .iter()
        .enumerate()
        .filter(|(_, m)| m.source_table.is_none())
        .collect();

    if derived.is_empty() {
        return Ok(ResolvedMetricExprs {
            exprs: resolved,
            count_star_no_pk,
        });
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
        let raw_expr = if facts.is_empty() {
            met.expr.clone()
        } else {
            inline_facts(&met.expr, facts, fact_topo_order)
        };
        // Replace every known metric name with its resolved expression
        // (parenthesized) in ONE combined left-to-right pass. Sequential
        // per-name replace_word_boundary calls iterated the HashMap in
        // nondeterministic order and re-scanned earlier substitutions: a
        // metric named like a column used in another metric's expression
        // (`revenue` vs `SUM(o.revenue)` — `.` is a word boundary) was
        // double-substituted into invalid nested-aggregate SQL on a
        // hash-seed-dependent fraction of runs (SG-3, code-review
        // 2026-07-02). Pair order is deterministic (longest needle first,
        // then lexicographic), mirroring `inline_facts`.
        let expr = {
            let mut entries: Vec<(&str, String)> = resolved
                .iter()
                .map(|(name, replacement)| (name.as_str(), format!("({replacement})")))
                .collect();
            entries.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
            let pairs: Vec<(&str, &str)> = entries.iter().map(|(n, r)| (*n, r.as_str())).collect();
            replace_word_boundary_pairs(&raw_expr, &pairs)
        };
        resolved.insert(met.name.to_ascii_lowercase(), expr);
    }

    Ok(ResolvedMetricExprs {
        exprs: resolved,
        count_star_no_pk,
    })
}

/// True when `expr_lower` references `name` at a word boundary.
///
/// Both arguments must already be lowercased. Shared byte-scan used by the
/// transitive metric walks.
fn references_name(expr_lower: &str, name: &str) -> bool {
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
                return true;
            }
        }
        pos += 1;
    }
    false
}

/// Collect the lowercased names of `met` and every metric it transitively
/// depends on: derived metrics contribute the metric names referenced in
/// their expressions; window metrics contribute their inner metric.
///
/// Used by the SG-8 check in `expand()` to decide whether a requested metric
/// reaches a base metric whose `COUNT(*)` could not be rewritten.
pub(super) fn collect_transitive_metric_names(
    met: &crate::model::Metric,
    all_metrics: &[crate::model::Metric],
) -> HashSet<String> {
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
        if let Some(ref ws) = current_met.window_spec {
            stack.push(ws.inner_metric.to_ascii_lowercase());
        }
        if current_met.source_table.is_none() {
            // Derived metric: find referenced metric names and push to stack
            let expr_lower = current_met.expr.to_ascii_lowercase();
            for name in &all_names {
                if *name != current_name && references_name(&expr_lower, name) {
                    stack.push(name.clone());
                }
            }
        }
    }

    visited
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
    fn inline_derived_metrics_name_matching_column_is_not_double_substituted() {
        // SG-3 regression (code-review 2026-07-02): metric `revenue` also
        // appears as the column reference `o.revenue` inside `tax`'s
        // expression (`.` is a word boundary). The old sequential per-name
        // substitution re-scanned inserted text in HashMap iteration order:
        // when `tax` happened to be inlined first, the subsequent `revenue`
        // pass also matched `revenue` inside the freshly inserted
        // `SUM(o.revenue * 0.1)`, corrupting the expression into invalid
        // nested-aggregate SQL on a hash-seed-dependent fraction of runs.
        // The single combined pass must produce this exact expression, every
        // run.
        let metrics = vec![
            make_metric("revenue", "SUM(o.revenue)", Some("o")),
            make_metric("tax", "SUM(o.revenue * 0.1)", Some("o")),
            make_metric("after_tax", "revenue - tax", None),
        ];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
            .unwrap()
            .exprs;
        assert_eq!(
            resolved.get("after_tax").unwrap(),
            "(SUM(o.revenue)) - (SUM(o.revenue * 0.1))"
        );
    }

    #[test]
    fn inline_derived_metrics_chained_derived_not_rescanned() {
        // A derived metric referencing another derived metric: the inner
        // resolution is inserted verbatim and must not be re-scanned even
        // though it contains the names of other metrics.
        let metrics = vec![
            make_metric("revenue", "SUM(o.revenue)", Some("o")),
            make_metric("cost", "SUM(o.cost)", Some("o")),
            make_metric("profit", "revenue - cost", None),
            make_metric("margin", "profit / revenue", None),
        ];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
            .unwrap()
            .exprs;
        assert_eq!(
            resolved.get("margin").unwrap(),
            "((SUM(o.revenue)) - (SUM(o.cost))) / (SUM(o.revenue))"
        );
    }

    #[test]
    fn inline_derived_metrics_cycle_returns_err() {
        let metrics = vec![
            make_metric("a", "b + 1", None),
            make_metric("b", "a + 1", None),
        ];
        let result = inline_derived_metrics(&metrics, &[], &[], &[]);
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
        let result = inline_derived_metrics(&metrics, &[], &[], &[]);
        assert!(result.is_ok(), "Non-cyclic should succeed");
        let resolved = result.unwrap().exprs;
        assert_eq!(
            resolved.get("profit").unwrap(),
            "(SUM(amount)) - (SUM(unit_cost))"
        );
    }

    #[test]
    fn inline_derived_metrics_mixed_case_references_are_inlined() {
        // E-2 regression (code-review 2026-07-11): the CREATE-time validators
        // resolve metric references case-insensitively, but the substitution
        // scanner compared raw bytes — `profit AS REVENUE - Cost` passed
        // validation, skipped inlining, and leaked raw identifiers into the
        // generated SQL (erroring or silently "working" depending on which
        // other metrics were co-queried).
        let metrics = vec![
            make_metric("revenue", "SUM(o.rev)", Some("o")),
            make_metric("cost", "SUM(o.cost)", Some("o")),
            make_metric("profit", "REVENUE - Cost", None),
        ];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
            .unwrap()
            .exprs;
        assert_eq!(
            resolved.get("profit").unwrap(),
            "(SUM(o.rev)) - (SUM(o.cost))"
        );
    }

    #[test]
    fn inline_facts_mixed_case_references_are_inlined() {
        // E-2, facts arm: fact references are validated case-insensitively
        // (graph/facts.rs lowercases both sides), so inlining must match
        // any-case references to an as-declared fact name.
        let facts = vec![Fact {
            name: "net_price".to_string(),
            expr: "price * (1 - discount)".to_string(),
            source_table: Some("o".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
        }];
        let topo = toposort_facts(&facts).unwrap();
        let result = inline_facts("SUM(Net_Price)", &facts, &topo);
        assert_eq!(result, "SUM((price * (1 - discount)))");
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
        let result = inline_derived_metrics(&metrics, &[], &[], &[]);
        assert!(result.is_err(), "Depth exceeding limit should error");
        let err = result.unwrap_err();
        assert!(
            err.contains("nesting depth") && err.contains("maximum"),
            "Error should mention depth limit: {err}"
        );
    }

    // --- rewrite_count_star tests (SG-8) ---

    fn make_table(alias: &str, pk: &[&str]) -> TableRef {
        TableRef {
            alias: alias.to_string(),
            table: alias.to_string(),
            pk_columns: pk.iter().map(|s| (*s).to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn rewrite_count_star_basic() {
        assert_eq!(
            rewrite_count_star("COUNT(*)", "\"li\".\"id\"").as_deref(),
            Some("COUNT(\"li\".\"id\")")
        );
    }

    #[test]
    fn rewrite_count_star_preserves_case_and_handles_spaces() {
        assert_eq!(
            rewrite_count_star("count( * )", "\"li\".\"id\"").as_deref(),
            Some("count(\"li\".\"id\")")
        );
        assert_eq!(
            rewrite_count_star("Count (*)", "x").as_deref(),
            Some("Count (x)")
        );
    }

    #[test]
    fn rewrite_count_star_inside_larger_expression() {
        assert_eq!(
            rewrite_count_star("COUNT(*) * 2 + COUNT(*)", "\"li\".\"id\"").as_deref(),
            Some("COUNT(\"li\".\"id\") * 2 + COUNT(\"li\".\"id\")")
        );
    }

    #[test]
    fn rewrite_count_star_none_when_absent() {
        assert!(rewrite_count_star("COUNT(li.id)", "x").is_none());
        assert!(rewrite_count_star("SUM(amount)", "x").is_none());
        // `*` as multiplication, not a star argument
        assert!(rewrite_count_star("COUNT(a * b)", "x").is_none());
    }

    #[test]
    fn rewrite_count_star_skips_string_literals_and_word_boundaries() {
        // Inside a single-quoted literal: untouched.
        assert!(rewrite_count_star("'COUNT(*)'", "x").is_none());
        // `miscount(*)` is not `count` at a word boundary.
        assert!(rewrite_count_star("miscount(*)", "x").is_none());
    }

    // --- inline_derived_metrics COUNT(*) rewrite tests (SG-8) ---

    #[test]
    fn inline_derived_metrics_rewrites_count_star_on_non_base_table() {
        let tables = vec![make_table("o", &["id"]), make_table("li", &["id"])];
        let metrics = vec![make_metric("item_count", "COUNT(*)", Some("li"))];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &tables).unwrap();
        assert_eq!(
            resolved.exprs.get("item_count").unwrap(),
            "COUNT(\"li\".\"id\")"
        );
        assert!(resolved.count_star_no_pk.is_empty());
    }

    #[test]
    fn inline_derived_metrics_keeps_count_star_on_base_table() {
        let tables = vec![make_table("o", &["id"]), make_table("li", &["id"])];
        let metrics = vec![make_metric("order_count", "COUNT(*)", Some("o"))];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &tables).unwrap();
        assert_eq!(resolved.exprs.get("order_count").unwrap(), "COUNT(*)");
        assert!(resolved.count_star_no_pk.is_empty());
    }

    #[test]
    fn inline_derived_metrics_records_no_pk_failure() {
        // li declares no PRIMARY KEY: the rewrite is impossible and the
        // metric is recorded so the caller can error when it is queried.
        let tables = vec![make_table("o", &["id"]), make_table("li", &[])];
        let metrics = vec![make_metric("item_count", "COUNT(*)", Some("li"))];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &tables).unwrap();
        assert_eq!(resolved.exprs.get("item_count").unwrap(), "COUNT(*)");
        assert_eq!(
            resolved
                .count_star_no_pk
                .get("item_count")
                .map(String::as_str),
            Some("li")
        );
    }

    #[test]
    fn inline_derived_metrics_rewrite_propagates_into_derived() {
        // The rewrite runs at base-metric resolution, BEFORE derived-metric
        // inlining, so derived metrics inherit the rewritten text.
        let tables = vec![make_table("o", &["id"]), make_table("li", &["li_id"])];
        let metrics = vec![
            make_metric("item_count", "COUNT(*)", Some("li")),
            make_metric("double_items", "item_count * 2", None),
        ];
        let resolved = inline_derived_metrics(&metrics, &[], &[], &tables).unwrap();
        assert_eq!(
            resolved.exprs.get("double_items").unwrap(),
            "(COUNT(\"li\".\"li_id\")) * 2"
        );
    }

    // --- collect_transitive_metric_names tests (SG-8 check support) ---

    #[test]
    fn collect_transitive_metric_names_derived_and_window() {
        let mut window_met = make_metric("rolling_items", "AVG(item_count)", None);
        window_met.window_spec = Some(crate::model::WindowSpec {
            window_function: "AVG".to_string(),
            inner_metric: "item_count".to_string(),
            ..Default::default()
        });
        let metrics = vec![
            make_metric("item_count", "COUNT(*)", Some("li")),
            make_metric("double_items", "item_count * 2", None),
            window_met,
        ];
        let via_derived = collect_transitive_metric_names(&metrics[1], &metrics);
        assert!(via_derived.contains("double_items"));
        assert!(via_derived.contains("item_count"));
        let via_window = collect_transitive_metric_names(&metrics[2], &metrics);
        assert!(via_window.contains("rolling_items"));
        assert!(
            via_window.contains("item_count"),
            "window metrics must chase their inner metric: {via_window:?}"
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

    #[test]
    fn test_inline_facts_identity_qualified_no_double_sub() {
        // Identity passthrough: fact `unit_price` whose expression is the qualified
        // column `s.unit_price`. The SELECT path passes the fact's own expr through
        // inline_facts. It must NOT double-substitute into `(s.(s.unit_price))`.
        let mut fact = make_fact("unit_price", "s.unit_price");
        fact.source_table = Some("s".to_string());
        let facts = vec![fact];
        let topo = vec![0];
        let result = inline_facts("s.unit_price", &facts, &topo);
        assert_eq!(
            result, "(s.unit_price)",
            "Identity fact must resolve to its column once, got: {result}"
        );
    }

    #[test]
    fn test_inline_facts_identity_referenced_by_metric() {
        // A metric referencing an identity fact by its qualified column must inline
        // cleanly to a single column reference.
        let mut fact = make_fact("unit_price", "s.unit_price");
        fact.source_table = Some("s".to_string());
        let facts = vec![fact];
        let topo = vec![0];
        let result = inline_facts("SUM(s.unit_price)", &facts, &topo);
        assert_eq!(
            result, "SUM((s.unit_price))",
            "Metric over identity fact must inline once, got: {result}"
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
