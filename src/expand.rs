use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::graph::RelationshipGraph;
use crate::model::{Cardinality, Fact, Join, SemanticViewDefinition, TableRef};

/// Suggest the closest matching name from `available` using Levenshtein distance.
///
/// Returns `Some(name)` (with original casing) if the best match has an edit
/// distance of 3 or fewer characters. Returns `None` if no candidate is close
/// enough. Both the query and candidates are lowercased for comparison.
#[must_use]
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let query = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let dist = strsim::levenshtein(&query, &candidate.to_ascii_lowercase());
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, candidate));
                }
            } else {
                best = Some((dist, candidate));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// A request to expand a semantic view into SQL.
///
/// Contains the names of dimensions and metrics to include in the query.
/// At least one dimension or one metric must be specified. Supported modes:
/// - Dimensions only: `SELECT DISTINCT` (no aggregation)
/// - Metrics only: global aggregate (no `GROUP BY`)
/// - Both: grouped aggregation with `GROUP BY`
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
}

/// Errors that can occur during semantic view expansion.
#[derive(Debug)]
pub enum ExpandError {
    /// The request contained neither dimensions nor metrics.
    EmptyRequest { view_name: String },
    /// A requested dimension name does not exist in the view definition.
    UnknownDimension {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A requested metric name does not exist in the view definition.
    UnknownMetric {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A dimension name was requested more than once.
    DuplicateDimension { view_name: String, name: String },
    /// A metric name was requested more than once.
    DuplicateMetric { view_name: String, name: String },
    /// A metric aggregates across a one-to-many boundary, risking inflated results.
    FanTrap {
        view_name: String,
        metric_name: String,
        metric_table: String,
        dimension_name: String,
        dimension_table: String,
        relationship_name: String,
    },
    /// A dimension from a role-playing table is ambiguous because multiple
    /// relationships reach that table and no co-queried metric provides USING
    /// context to disambiguate.
    AmbiguousPath {
        view_name: String,
        dimension_name: String,
        dimension_table: String,
        available_relationships: Vec<String>,
    },
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequest { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': specify at least dimensions := [...] or metrics := [...]"
                )
            }
            Self::UnknownDimension {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown dimension '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::UnknownMetric {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown metric '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::DuplicateDimension { view_name, name } => {
                write!(
                    f,
                    "semantic view '{view_name}': duplicate dimension '{name}'"
                )
            }
            Self::DuplicateMetric { view_name, name } => {
                write!(f, "semantic view '{view_name}': duplicate metric '{name}'")
            }
            Self::FanTrap {
                view_name,
                metric_name,
                metric_table,
                dimension_name,
                dimension_table,
                relationship_name,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': fan trap detected -- metric '{metric_name}' \
                     (table '{metric_table}') would be duplicated when joined to dimension \
                     '{dimension_name}' (table '{dimension_table}') via relationship \
                     '{relationship_name}' (many-to-one cardinality, inferred: FK is not PK/UNIQUE). \
                     This would inflate aggregation results. \
                     Remove the dimension, use a metric from the same table, or restructure the \
                     relationship."
                )
            }
            Self::AmbiguousPath {
                view_name,
                dimension_name,
                dimension_table,
                available_relationships,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': dimension '{dimension_name}' is ambiguous -- \
                     table '{dimension_table}' is reached via multiple relationships: [{}]. \
                     Specify a metric with USING to disambiguate, or use a dimension from a \
                     non-ambiguous table.",
                    available_relationships.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ExpandError {}

/// Double-quote a SQL identifier, escaping embedded double quotes.
///
/// `DuckDB` uses `"` for identifier quoting. Internal `"` must be escaped
/// as `""` per the SQL standard.
///
/// # Examples
///
/// ```
/// # use semantic_views::expand::quote_ident;
/// assert_eq!(quote_ident("orders"), "\"orders\"");
/// assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
/// ```
#[must_use]
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Quote a potentially dot-qualified table reference.
///
/// Splits on `.` and quotes each part individually. This handles:
/// - Simple names: `orders` -> `"orders"`
/// - Catalog-qualified: `jaffle.raw_orders` -> `"jaffle"."raw_orders"`
/// - Fully qualified: `catalog.schema.table` -> `"catalog"."schema"."table"`
///
/// Each part is quoted via `quote_ident`, so embedded double quotes are escaped.
#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    table
        .split('.')
        .map(quote_ident)
        .collect::<Vec<_>>()
        .join(".")
}

/// Look up a dimension by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.region"),
/// splits into (alias, `bare_name`) and also matches `source_table == alias`.
/// Falls back to `bare_name` lookup if no qualified match is found.
fn find_dimension<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Dimension> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        // Try qualified lookup: bare_name match AND source_table == alias
        if let Some(d) = def.dimensions.iter().find(|d| {
            d.name.eq_ignore_ascii_case(bare)
                && d.source_table
                    .as_deref()
                    .is_some_and(|st| st.eq_ignore_ascii_case(alias))
        }) {
            return Some(d);
        }
        // Fall back to bare_name only (backward compat)
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(bare))
    } else {
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }
}

/// Look up a metric by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.revenue"),
/// splits into (alias, `bare_name`) and also matches `source_table == alias`.
/// Falls back to `bare_name` lookup if no qualified match is found.
fn find_metric<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Metric> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        if let Some(m) = def.metrics.iter().find(|m| {
            m.name.eq_ignore_ascii_case(bare)
                && m.source_table
                    .as_deref()
                    .is_some_and(|st| st.eq_ignore_ascii_case(alias))
        }) {
            return Some(m);
        }
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(bare))
    } else {
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

/// Synthesize an ON clause from PK/FK column declarations (Phase 26).
///
/// Zips `join.fk_columns` with the referenced table's `pk_columns` to produce
/// `from_alias.fk = to_alias.pk` pairs, joined by ` AND `.
/// Uses `join.from_alias` for the FROM side and `join.table` for the TO side.
fn synthesize_on_clause(join: &Join, tables: &[TableRef]) -> String {
    synthesize_on_clause_scoped(join, tables, &join.table)
}

/// Synthesize an ON clause with a potentially scoped alias on the PK (target) side.
///
/// Like `synthesize_on_clause`, but uses `to_alias` instead of `join.table` in the
/// generated SQL. This supports role-playing dimensions where the same physical table
/// appears multiple times with different scoped aliases (e.g., `a__dep_airport`).
///
/// Phase 33: Prefers `join.ref_columns` (resolved during inference) over looking up
/// `pk_columns` from the target table. Falls back to target PK for backward compat
/// (legacy joins without `ref_columns`).
fn synthesize_on_clause_scoped(join: &Join, tables: &[TableRef], to_alias: &str) -> String {
    // Phase 33: Prefer ref_columns (resolved during inference).
    // Fall back to target PK for backward compat (legacy joins without ref_columns).
    let ref_cols: &[String] = if join.ref_columns.is_empty() {
        let to_alias_lower = join.table.to_ascii_lowercase();
        tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower)
            .map_or(&[] as &[String], |t| &t.pk_columns)
    } else {
        &join.ref_columns
    };

    let pairs: Vec<String> = join
        .fk_columns
        .iter()
        .zip(ref_cols.iter())
        .map(|(fk, pk)| {
            format!(
                "{}.{} = {}.{}",
                quote_ident(&join.from_alias),
                quote_ident(fk),
                quote_ident(to_alias),
                quote_ident(pk),
            )
        })
        .collect();
    pairs.join(" AND ")
}

/// Determine which relationships point to a given table alias in the definition.
///
/// Returns a list of relationship names that have `to_alias` as their target.
/// Used for ambiguity detection: if a table is reached by multiple named relationships,
/// dimensions from that table require USING context to disambiguate.
fn relationships_to_table(def: &SemanticViewDefinition, target_alias: &str) -> Vec<String> {
    let target_lower = target_alias.to_ascii_lowercase();
    def.joins
        .iter()
        .filter(|j| !j.fk_columns.is_empty() && j.table.to_ascii_lowercase() == target_lower)
        .filter_map(|j| j.name.clone())
        .collect()
}

/// Determine the scoped alias for a dimension from a role-playing table.
///
/// Checks whether the dimension's `source_table` is reached by multiple relationships.
/// If so, looks at co-queried metrics' `using_relationships` to determine which
/// relationship (and thus which scoped alias) to use for the dimension.
///
/// Returns:
/// - `Ok(None)` if the dimension's table is not a role-playing table (single or no relationship)
/// - `Ok(Some(scoped_alias))` if exactly one USING path disambiguates
/// - `Err(ExpandError::AmbiguousPath)` if ambiguous with no single USING context
#[allow(clippy::result_large_err)]
fn find_using_context(
    view_name: &str,
    def: &SemanticViewDefinition,
    dim: &crate::model::Dimension,
    resolved_mets: &[&crate::model::Metric],
) -> Result<Option<String>, ExpandError> {
    let Some(ref dim_table) = dim.source_table else {
        return Ok(None); // No source table -> base table, no scoping needed
    };
    let dim_table_lower = dim_table.to_ascii_lowercase();

    // Find all relationships pointing to this table
    let rels = relationships_to_table(def, &dim_table_lower);
    if rels.len() <= 1 {
        return Ok(None); // Single or no relationship -> unambiguous, use bare alias
    }

    // Multiple relationships -> role-playing table. Look for USING context.
    // Collect all USING relationships from co-queried metrics that target this table.
    let mut using_rels_for_table: Vec<String> = Vec::new();
    for met in resolved_mets {
        for using_rel in &met.using_relationships {
            // Check if this USING relationship targets our dimension's table
            let using_rel_lower = using_rel.to_ascii_lowercase();
            let targets_our_table = def.joins.iter().any(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.to_ascii_lowercase() == using_rel_lower)
                    && j.table.to_ascii_lowercase() == dim_table_lower
            });
            if targets_our_table && !using_rels_for_table.contains(&using_rel_lower) {
                using_rels_for_table.push(using_rel_lower);
            }
        }
        // Also check derived metrics: walk their transitive dependencies
        if met.source_table.is_none() {
            let transitive_using = collect_derived_metric_using(met, &def.metrics);
            for using_rel in transitive_using {
                let using_rel_lower = using_rel.to_ascii_lowercase();
                let targets_our_table = def.joins.iter().any(|j| {
                    j.name
                        .as_ref()
                        .is_some_and(|n| n.to_ascii_lowercase() == using_rel_lower)
                        && j.table.to_ascii_lowercase() == dim_table_lower
                });
                if targets_our_table && !using_rels_for_table.contains(&using_rel_lower) {
                    using_rels_for_table.push(using_rel_lower);
                }
            }
        }
    }

    if using_rels_for_table.len() == 1 {
        // Exactly one USING path disambiguates -> return scoped alias
        let scoped = format!("{dim_table_lower}__{}", using_rels_for_table[0]);
        Ok(Some(scoped))
    } else {
        // Zero or multiple USING paths -> ambiguous
        Err(ExpandError::AmbiguousPath {
            view_name: view_name.to_string(),
            dimension_name: dim.name.clone(),
            dimension_table: dim_table_lower,
            available_relationships: rels,
        })
    }
}

/// Collect `using_relationships` from all transitive base metrics referenced by a derived metric.
fn collect_derived_metric_using(
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

/// Resolve which joins are needed using graph-based PK/FK resolution (Phase 26+32).
///
/// Builds a `RelationshipGraph` from the definition, collects needed table aliases
/// from resolved dimensions and metrics, walks reverse edges to include transitive
/// intermediaries, and returns aliases in topological order (root-outward).
///
/// Phase 32: When metrics have `using_relationships`, generates scoped aliases
/// (`{to_alias}__{rel_name}`) instead of bare aliases. Scoped aliases are placed
/// after the corresponding bare alias position in topological order.
#[allow(clippy::too_many_lines)]
fn resolve_joins_pkfk(
    def: &SemanticViewDefinition,
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
) -> Vec<String> {
    let Ok(graph) = RelationshipGraph::from_definition(def) else {
        return Vec::new(); // Graph was validated at define time
    };

    let root = &graph.root;

    // Phase 32: Collect scoped aliases from metrics with USING relationships.
    let mut scoped_aliases: Vec<String> = Vec::new();
    // Also track which bare aliases are role-playing (have multiple relationships).
    let mut role_playing_bare_aliases: HashSet<String> = HashSet::new();

    for met in resolved_mets {
        if !met.using_relationships.is_empty() {
            for using_rel in &met.using_relationships {
                // Find the join for this relationship name
                let using_rel_lower = using_rel.to_ascii_lowercase();
                if let Some(join) = def.joins.iter().find(|j| {
                    j.name
                        .as_ref()
                        .is_some_and(|n| n.to_ascii_lowercase() == using_rel_lower)
                }) {
                    let to_alias = join.table.to_ascii_lowercase();
                    let scoped = format!("{to_alias}__{using_rel_lower}");
                    if !scoped_aliases.contains(&scoped) {
                        scoped_aliases.push(scoped);
                    }
                    role_playing_bare_aliases.insert(to_alias);
                }
            }
        } else if met.source_table.is_none() {
            // Derived metric: walk transitive USING relationships
            let transitive_using = collect_derived_metric_using(met, &def.metrics);
            for using_rel in transitive_using {
                let using_rel_lower = using_rel.to_ascii_lowercase();
                if let Some(join) = def.joins.iter().find(|j| {
                    j.name
                        .as_ref()
                        .is_some_and(|n| n.to_ascii_lowercase() == using_rel_lower)
                }) {
                    let to_alias = join.table.to_ascii_lowercase();
                    let scoped = format!("{to_alias}__{using_rel_lower}");
                    if !scoped_aliases.contains(&scoped) {
                        scoped_aliases.push(scoped);
                    }
                    role_playing_bare_aliases.insert(to_alias);
                }
            }
        }
    }

    // Collect needed bare aliases from source_table fields (lowercased).
    let mut needed: HashSet<String> = HashSet::new();
    for dim in resolved_dims {
        if let Some(ref st) = dim.source_table {
            let alias = st.to_ascii_lowercase();
            if alias != *root {
                // If this is a role-playing table, the dimension alias will be resolved
                // via find_using_context() in expand(). Don't add the bare alias here;
                // the scoped aliases are already tracked.
                if !role_playing_bare_aliases.contains(&alias) {
                    needed.insert(alias);
                }
            }
        }
    }
    for met in resolved_mets {
        if met.using_relationships.is_empty() {
            if let Some(ref st) = met.source_table {
                let alias = st.to_ascii_lowercase();
                if alias != *root {
                    needed.insert(alias);
                }
            } else {
                // Derived metric without direct USING: walk dependency graph for bare tables
                let transitive_tables = collect_derived_metric_source_tables(met, &def.metrics);
                for alias in transitive_tables {
                    if alias != *root && !role_playing_bare_aliases.contains(&alias) {
                        needed.insert(alias);
                    }
                }
            }
        }
        // Metrics WITH using_relationships: their source_table is the base table
        // (e.g., "f" for flights). Only add if it's not root.
        if !met.using_relationships.is_empty() {
            if let Some(ref st) = met.source_table {
                let alias = st.to_ascii_lowercase();
                if alias != *root {
                    needed.insert(alias);
                }
            }
        }
    }

    // If no bare aliases or scoped aliases needed, return empty
    if needed.is_empty() && scoped_aliases.is_empty() {
        return Vec::new();
    }

    // Walk reverse edges to include transitive intermediaries for bare aliases.
    let mut all_needed: HashSet<String> = needed.clone();
    let mut to_visit: Vec<String> = needed.into_iter().collect();
    while let Some(current) = to_visit.pop() {
        if let Some(parents) = graph.reverse.get(&current) {
            for parent in parents {
                if parent != root && all_needed.insert(parent.clone()) {
                    to_visit.push(parent.clone());
                }
            }
        }
    }

    // Build the result: bare aliases in topo order, then scoped aliases after.
    let Ok(topo_order) = graph.toposort() else {
        return Vec::new(); // Should not happen — validated at define time
    };

    let mut result: Vec<String> = topo_order
        .into_iter()
        .filter(|alias| all_needed.contains(alias))
        .collect();

    // Sort scoped aliases for deterministic output, then append
    scoped_aliases.sort();
    result.extend(scoped_aliases);

    result
}

/// Replace all word-boundary occurrences of `needle` in `haystack` with `replacement`.
///
/// A word boundary is defined as: the character before the match (if any) is NOT
/// alphanumeric or underscore, AND the character after the match (if any) is NOT
/// alphanumeric or underscore. This prevents `net_price` from matching inside
/// `net_price_total` or `my_net_price`.
///
/// The matching is case-sensitive (fact names are identifiers).
fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || needle.len() > haystack.len() {
        return haystack.to_string();
    }

    let h_bytes = haystack.as_bytes();
    let n_bytes = needle.as_bytes();
    let n_len = n_bytes.len();

    let mut result = String::with_capacity(haystack.len());
    let mut i = 0;

    while i + n_len <= h_bytes.len() {
        if &h_bytes[i..i + n_len] == n_bytes {
            let before_ok = i == 0 || is_word_boundary_char(h_bytes[i - 1]);
            let after_ok = i + n_len == h_bytes.len() || is_word_boundary_char(h_bytes[i + n_len]);
            if before_ok && after_ok {
                result.push_str(replacement);
                i += n_len;
                continue;
            }
        }
        result.push(haystack[i..].chars().next().unwrap());
        i += 1;
    }
    // Append remaining bytes that are shorter than needle
    if i < haystack.len() {
        result.push_str(&haystack[i..]);
    }
    result
}

/// Check if a byte is a word-boundary character (NOT alphanumeric or underscore).
fn is_word_boundary_char(b: u8) -> bool {
    !b.is_ascii_alphanumeric() && b != b'_'
}

/// Topologically sort facts by their inter-dependencies (leaf facts first).
///
/// Uses Kahn's algorithm. Returns indices into the `facts` slice in topological
/// order (facts with no dependencies on other facts come first).
///
/// Returns `Err` if a cycle is detected (defensive — `validate_facts` should
/// have already rejected cycles at CREATE time).
fn toposort_facts(facts: &[Fact]) -> Result<Vec<usize>, String> {
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
fn inline_facts(expr: &str, facts: &[Fact], topo_order: &[usize]) -> String {
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
fn inline_derived_metrics(
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

/// Check for fan traps: a metric aggregating across a one-to-many boundary.
///
/// Walks the join path between each (metric source, dimension source) pair
/// and checks whether any edge is traversed in the fan-out direction.
/// Returns `Err(ExpandError::FanTrap)` with details if a fan-out is detected.
///
/// # Fan-out direction
///
/// For an edge `(from_alias, to_alias)` with cardinality:
/// - `ManyToOne`: from->to is safe (many go to one), to->from is fan-out
/// - `OneToOne`: both directions are safe
#[allow(clippy::result_large_err)]
fn check_fan_traps(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
) -> Result<(), ExpandError> {
    if def.joins.is_empty() {
        return Ok(());
    }

    let Ok(graph) = RelationshipGraph::from_definition(def) else {
        return Ok(()); // Graph was validated at define time
    };

    // Build cardinality map: (from_lower, to_lower) -> (Cardinality, relationship_name)
    let card_map: HashMap<(String, String), (Cardinality, String)> = def
        .joins
        .iter()
        .filter(|j| !j.fk_columns.is_empty())
        .map(|j| {
            let rel_name = j.name.as_deref().unwrap_or(&j.from_alias).to_string();
            (
                (
                    j.from_alias.to_ascii_lowercase(),
                    j.table.to_ascii_lowercase(),
                ),
                (j.cardinality, rel_name),
            )
        })
        .collect();

    // Build parent map for tree path finding.
    // In a validated tree, each non-root node has exactly one parent via the reverse map.
    let mut parent_map: HashMap<String, String> = HashMap::new();
    for (child, parents) in &graph.reverse {
        if let Some(parent) = parents.first() {
            parent_map.insert(child.clone(), parent.clone());
        }
    }

    // For each metric + dimension pair, check for fan-out on the join path.
    for met in resolved_mets {
        // Get source tables for this metric
        let met_tables: Vec<String> = if let Some(ref st) = met.source_table {
            vec![st.to_ascii_lowercase()]
        } else {
            // Derived metric: walk dependency graph to find transitive base metric source tables
            collect_derived_metric_source_tables(met, &def.metrics)
                .into_iter()
                .map(|s| s.to_ascii_lowercase())
                .collect()
        };

        for dim in resolved_dims {
            let Some(ref dim_table_raw) = dim.source_table else {
                continue; // No source table -> base table dim, skip
            };
            let dim_table = dim_table_raw.to_ascii_lowercase();

            for met_table in &met_tables {
                if *met_table == dim_table {
                    continue; // Same table, no fan-out possible
                }

                // Find path from met_table to dim_table through the tree.
                // Walk both up to root to get ancestor chains, then derive path.
                let met_ancestors = ancestors_to_root(met_table, &parent_map);
                let dim_ancestors = ancestors_to_root(&dim_table, &parent_map);

                // Find the lowest common ancestor (LCA)
                let dim_ancestor_set: HashSet<&String> = dim_ancestors.iter().collect();
                let lca = met_ancestors
                    .iter()
                    .find(|a| dim_ancestor_set.contains(a))
                    .cloned();

                let Some(lca) = lca else {
                    continue; // No common ancestor (shouldn't happen in a tree)
                };

                // Build path: met_table -> ... -> LCA -> ... -> dim_table
                // Check edges from met_table up to LCA
                if let Some(err) =
                    check_path_up(met_table, &lca, &parent_map, &card_map, view_name, met, dim)
                {
                    return Err(err);
                }
                // Check edges from LCA down to dim_table
                // We need the path from LCA down to dim_table. Build it from dim_ancestors.
                let path_down = path_from_ancestor_to_node(&lca, &dim_table, &dim_ancestors);
                if let Some(err) = check_path_down(&path_down, &card_map, view_name, met, dim) {
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

/// Walk from `node` to the root through the parent map, returning the chain
/// including `node` itself. The last element is the root.
pub(crate) fn ancestors_to_root(node: &str, parent_map: &HashMap<String, String>) -> Vec<String> {
    let mut chain = vec![node.to_string()];
    let mut current = node.to_string();
    while let Some(parent) = parent_map.get(&current) {
        chain.push(parent.clone());
        current = parent.clone();
    }
    chain
}

/// Build the path from an ancestor down to a target node, given the target's ancestor chain.
/// Returns a vec starting at `ancestor` and ending at `target`.
fn path_from_ancestor_to_node(
    ancestor: &str,
    target: &str,
    target_ancestors: &[String],
) -> Vec<String> {
    // target_ancestors is [target, parent, grandparent, ..., root]
    // Find ancestor in this chain and take the sub-chain, reversed.
    if let Some(pos) = target_ancestors.iter().position(|a| a == ancestor) {
        let mut path: Vec<String> = target_ancestors[..=pos].to_vec();
        path.reverse();
        path
    } else {
        vec![ancestor.to_string(), target.to_string()]
    }
}

/// Check edges going UP from `start` to `ancestor` (toward root).
/// Walking up means: at each step, current -> parent. The actual edge in the
/// graph might be current->parent (forward edge) or parent->current (forward edge).
///
/// Returns `None` if no fan-out, `Some(ExpandError)` if fan-out detected.
fn check_path_up(
    start: &str,
    ancestor: &str,
    parent_map: &HashMap<String, String>,
    card_map: &HashMap<(String, String), (Cardinality, String)>,
    view_name: &str,
    met: &crate::model::Metric,
    dim: &crate::model::Dimension,
) -> Option<ExpandError> {
    let mut current = start.to_string();
    while current != ancestor {
        let Some(parent) = parent_map.get(&current) else {
            break;
        };
        // Determine which direction this edge goes in the card_map.
        // The graph stores edges as from_alias -> to_alias (FK -> PK).
        // So either (current, parent) or (parent, current) is in the map.
        if let Some((_card, _rel_name)) = card_map.get(&(current.clone(), parent.clone())) {
            // Edge is current -> parent (current has FK pointing to parent)
            // Walking current -> parent: this is the forward direction of the edge.
            // ManyToOne forward = safe, OneToOne = safe -- no fan-out possible going forward
        } else if let Some((card, rel_name)) = card_map.get(&(parent.clone(), current.clone())) {
            // Edge is parent -> current (parent has FK pointing to current)
            // Walking current -> parent means traversing this edge in REVERSE.
            // ManyToOne reverse = fan-out, OneToOne = safe
            if *card == Cardinality::ManyToOne {
                let met_table = met.source_table.as_deref().unwrap_or(&current);
                return Some(ExpandError::FanTrap {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    metric_table: met_table.to_string(),
                    dimension_name: dim.name.clone(),
                    dimension_table: dim.source_table.as_deref().unwrap_or("").to_string(),
                    relationship_name: rel_name.clone(),
                });
            }
        }
        current = parent.clone();
    }
    None
}

/// Check edges going DOWN a path (from ancestor toward target).
/// The path is [ancestor, ..., target]. For each consecutive pair (a, b),
/// check the traversal direction vs cardinality.
///
/// Returns `None` if no fan-out, `Some(ExpandError)` if fan-out detected.
fn check_path_down(
    path: &[String],
    card_map: &HashMap<(String, String), (Cardinality, String)>,
    view_name: &str,
    met: &crate::model::Metric,
    dim: &crate::model::Dimension,
) -> Option<ExpandError> {
    for window in path.windows(2) {
        let a = &window[0];
        let b = &window[1];
        // Walking a -> b (downward in the tree, away from root)
        if let Some((_card, _rel_name)) = card_map.get(&(a.clone(), b.clone())) {
            // Edge is a -> b (a has FK pointing to b)
            // Walking a -> b: forward direction
            // ManyToOne forward = safe, OneToOne = safe -- no fan-out possible going forward
        } else if let Some((card, rel_name)) = card_map.get(&(b.clone(), a.clone())) {
            // Edge is b -> a (b has FK pointing to a)
            // Walking a -> b means traversing this edge in REVERSE.
            // ManyToOne reverse = fan-out, OneToOne = safe
            if *card == Cardinality::ManyToOne {
                let met_table = met.source_table.as_deref().unwrap_or("").to_string();
                return Some(ExpandError::FanTrap {
                    view_name: view_name.to_string(),
                    metric_name: met.name.clone(),
                    metric_table: met_table,
                    dimension_name: dim.name.clone(),
                    dimension_table: dim.source_table.as_deref().unwrap_or("").to_string(),
                    relationship_name: rel_name.clone(),
                });
            }
        }
    }
    None
}

/// Expand a semantic view definition into a SQL query string.
///
/// Takes a view name (for error messages), its definition, and a query request
/// specifying which dimensions and metrics to include. Returns the generated SQL
/// or an `ExpandError` if the request is invalid.
///
/// # Errors
///
/// Returns `ExpandError` if:
/// - Neither dimensions nor metrics are requested (`EmptyRequest`)
/// - A requested dimension or metric name is not found (`UnknownDimension`, `UnknownMetric`)
/// - A dimension or metric name is duplicated (`DuplicateDimension`, `DuplicateMetric`)
#[allow(clippy::too_many_lines, clippy::result_large_err)]
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate: at least one dimension or metric is required.
    if req.dimensions.is_empty() && req.metrics.is_empty() {
        return Err(ExpandError::EmptyRequest {
            view_name: view_name.to_string(),
        });
    }

    // 2. Resolve requested dimensions to their definitions.
    let mut resolved_dims = Vec::with_capacity(req.dimensions.len());
    let mut seen_dims = std::collections::HashSet::new();
    for name in &req.dimensions {
        if !seen_dims.insert(name.to_ascii_lowercase()) {
            return Err(ExpandError::DuplicateDimension {
                view_name: view_name.to_string(),
                name: name.clone(),
            });
        }
        let dim = find_dimension(def, name).ok_or_else(|| {
            let available: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();
            let suggestion = suggest_closest(name, &available);
            ExpandError::UnknownDimension {
                view_name: view_name.to_string(),
                name: name.clone(),
                available,
                suggestion,
            }
        })?;
        resolved_dims.push(dim);
    }

    // 3. Resolve requested metrics to their definitions.
    let mut resolved_mets = Vec::with_capacity(req.metrics.len());
    let mut seen_mets = std::collections::HashSet::new();
    for name in &req.metrics {
        if !seen_mets.insert(name.to_ascii_lowercase()) {
            return Err(ExpandError::DuplicateMetric {
                view_name: view_name.to_string(),
                name: name.clone(),
            });
        }
        let met = find_metric(def, name).ok_or_else(|| {
            let available: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();
            let suggestion = suggest_closest(name, &available);
            ExpandError::UnknownMetric {
                view_name: view_name.to_string(),
                name: name.clone(),
                available,
                suggestion,
            }
        })?;
        resolved_mets.push(met);
    }

    // 4. Pre-compute all metric expressions: inline facts into base metrics,
    //    then inline metric references into derived metrics.
    let topo_order = toposort_facts(&def.facts).unwrap_or_default();
    let resolved_exprs = inline_derived_metrics(&def.metrics, &def.facts, &topo_order);

    // Phase 31: Check for fan traps before generating SQL.
    check_fan_traps(view_name, def, &resolved_dims, &resolved_mets)?;

    // Phase 32: Pre-compute dimension scoped aliases for role-playing tables.
    // Maps dimension index -> scoped alias (e.g., "a__dep_airport").
    let mut dim_scoped_aliases: Vec<Option<String>> = Vec::with_capacity(resolved_dims.len());
    for dim in &resolved_dims {
        let scoped = find_using_context(view_name, def, dim, &resolved_mets)?;
        dim_scoped_aliases.push(scoped);
    }

    // 5. Build the SELECT clause.
    //    Dimensions-only (no metrics): SELECT DISTINCT, no GROUP BY.
    //    Metrics-only (no dimensions): SELECT (global aggregate), no GROUP BY.
    //    Both: SELECT with GROUP BY.
    let mut sql = String::with_capacity(256);
    if !resolved_dims.is_empty() && resolved_mets.is_empty() {
        sql.push_str("SELECT DISTINCT\n");
    } else {
        sql.push_str("SELECT\n");
    }

    let mut select_items: Vec<String> = Vec::new();
    for (i, dim) in resolved_dims.iter().enumerate() {
        let mut base_expr = dim.expr.clone();
        // Phase 32: If this dimension has a scoped alias, rewrite the expression.
        if let Some(ref scoped) = dim_scoped_aliases[i] {
            if let Some(ref st) = dim.source_table {
                // Replace bare alias with scoped alias in expression
                // e.g., "a.city" -> "a__dep_airport.city"
                base_expr = replace_word_boundary(&base_expr, st, scoped);
            }
        }
        // If output_type is set, wrap the expression in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = dim.output_type {
            format!("CAST({base_expr} AS {type_str})")
        } else {
            base_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&dim.name)));
    }
    for met in &resolved_mets {
        // Look up the pre-computed resolved expression (handles both base + derived metrics)
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| met.expr.clone());
        // If output_type is set, wrap the aggregate in CAST(... AS <type>).
        let final_expr = if let Some(ref type_str) = met.output_type {
            format!("CAST({resolved_expr} AS {type_str})")
        } else {
            resolved_expr
        };
        select_items.push(format!("    {} AS {}", final_expr, quote_ident(&met.name)));
    }
    sql.push_str(&select_items.join(",\n"));

    // 6. FROM clause with base table.
    sql.push_str("\nFROM ");
    sql.push_str(&quote_table_ref(&def.base_table));

    // If tables aliases are declared (Phase 11.1), emit AS "alias" after the base table.
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // Join resolution via PK/FK graph (legacy resolve_joins removed in Phase 27).
    // Phase 32: ordered_aliases may contain scoped aliases like "a__dep_airport".
    let ordered_aliases = resolve_joins_pkfk(def, &resolved_dims, &resolved_mets);
    for alias in &ordered_aliases {
        // Phase 32: Check if this is a scoped alias (contains "__").
        if let Some(sep_pos) = alias.find("__") {
            let rel_name = &alias[sep_pos + 2..];
            // Find the Join by relationship name.
            let Some(join) = def.joins.iter().find(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(rel_name))
            }) else {
                continue;
            };
            // Find physical table name from the bare alias (before __).
            let bare_alias = &alias[..sep_pos];
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == bare_alias);
            let physical_table = table_ref.map_or(bare_alias, |t| t.table.as_str());
            sql.push_str("\nLEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause_scoped(join, &def.tables, alias));
        } else {
            // Standard bare alias join (non-role-playing).
            let Some(join) = def.joins.iter().find(|j| {
                j.table.to_ascii_lowercase() == *alias
                    || j.from_alias.to_ascii_lowercase() == *alias
            }) else {
                continue;
            };
            // Find the TableRef for this alias to get the physical table name.
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == *alias);
            let physical_table = table_ref.map_or(alias.as_str(), |t| t.table.as_str());
            sql.push_str("\nLEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause(join, &def.tables));
        }
    }

    // 7. GROUP BY (only when both dimensions and metrics are present).
    //    Use ordinal positions (GROUP BY 1, 2, ...) instead of expressions to avoid
    //    ambiguity when an expression matches its alias (e.g., `status AS "status"`).
    if !resolved_dims.is_empty() && !resolved_mets.is_empty() {
        sql.push_str("\nGROUP BY\n");
        let group_items: Vec<String> = (1..=resolved_dims.len())
            .map(|i| format!("    {i}"))
            .collect();
        sql.push_str(&group_items.join(",\n"));
    }

    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod quote_ident_tests {
        use super::*;

        #[test]
        fn simple_identifier() {
            assert_eq!(quote_ident("orders"), "\"orders\"");
        }

        #[test]
        fn reserved_word() {
            assert_eq!(quote_ident("select"), "\"select\"");
        }

        #[test]
        fn embedded_double_quote() {
            assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
        }

        #[test]
        fn identifier_with_spaces() {
            assert_eq!(quote_ident("my table"), "\"my table\"");
        }
    }

    mod quote_table_ref_tests {
        use super::*;

        #[test]
        fn simple_table_name() {
            assert_eq!(quote_table_ref("orders"), "\"orders\"");
        }

        #[test]
        fn catalog_qualified() {
            assert_eq!(
                quote_table_ref("jaffle.raw_orders"),
                "\"jaffle\".\"raw_orders\""
            );
        }

        #[test]
        fn fully_qualified() {
            assert_eq!(
                quote_table_ref("catalog.schema.table"),
                "\"catalog\".\"schema\".\"table\""
            );
        }

        #[test]
        fn reserved_word_parts() {
            assert_eq!(quote_table_ref("select.from"), "\"select\".\"from\"");
        }

        #[test]
        fn embedded_quotes_in_parts() {
            assert_eq!(
                quote_table_ref("my\"db.my\"table"),
                "\"my\"\"db\".\"my\"\"table\""
            );
        }
    }

    mod expand_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, SemanticViewDefinition};

        /// Helper to build a simple orders view definition.
        fn orders_view() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "status".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "total_revenue".to_string(),
                        expr: "sum(amount)".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "count(*)".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        #[test]
        fn test_basic_single_dimension_single_metric() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    region AS \"region\",
    sum(amount) AS \"total_revenue\"
FROM \"orders\"
GROUP BY
    1";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_multiple_dimensions_multiple_metrics() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "status".to_string()],
                metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    region AS \"region\",
    status AS \"status\",
    sum(amount) AS \"total_revenue\",
    count(*) AS \"order_count\"
FROM \"orders\"
GROUP BY
    1,
    2";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_global_aggregate_no_dimensions() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    sum(amount) AS \"total_revenue\"
FROM \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_identifier_quoting() {
            let def = SemanticViewDefinition {
                base_table: "select".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "col".to_string(),
                    expr: "col".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["col".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            // Base table "select" must be quoted
            assert!(sql.contains("FROM \"select\""));
        }

        #[test]
        fn test_dimension_expression_not_quoted() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "month".to_string(),
                    expr: "date_trunc('month', created_at)".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["month".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Expression appears verbatim in SELECT; GROUP BY uses ordinal position
            assert!(sql.contains("date_trunc('month', created_at) AS \"month\""));
            assert!(sql.contains("GROUP BY\n    1"));
        }

        #[test]
        fn test_empty_request_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec![],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::EmptyRequest { view_name } => {
                    assert_eq!(view_name, "orders");
                }
                other => panic!("Expected EmptyRequest, got: {other}"),
            }
        }

        #[test]
        fn test_dimensions_only_generates_distinct() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "status".to_string()],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT DISTINCT
    region AS \"region\",
    status AS \"status\"
FROM \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_metrics_only_still_works() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            let expected = "\
SELECT
    sum(amount) AS \"total_revenue\",
    count(*) AS \"order_count\"
FROM \"orders\"";
            assert_eq!(sql, expected);
        }

        #[test]
        fn test_case_insensitive_dimension_lookup() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "Region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            // Request uses lowercase "region" but definition has "Region"
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's expression
            assert!(sql.contains("region AS \"Region\""));
            assert!(sql.contains("GROUP BY\n    1"));
        }

        #[test]
        fn test_unknown_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["reigon".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownDimension {
                    view_name,
                    name,
                    available,
                    suggestion,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "reigon");
                    assert!(available.contains(&"region".to_string()));
                    assert_eq!(suggestion, Some("region".to_string()));
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_unknown_metric_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["totl_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownMetric {
                    view_name,
                    name,
                    available,
                    suggestion,
                } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "totl_revenue");
                    assert!(available.contains(&"total_revenue".to_string()));
                    assert_eq!(suggestion, Some("total_revenue".to_string()));
                }
                other => panic!("Expected UnknownMetric, got: {other}"),
            }
        }

        #[test]
        fn test_unknown_dimension_no_suggestion() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["xyzzy".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::UnknownDimension { suggestion, .. } => {
                    assert_eq!(suggestion, None);
                }
                other => panic!("Expected UnknownDimension, got: {other}"),
            }
        }

        #[test]
        fn test_duplicate_dimension_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec!["region".to_string(), "region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::DuplicateDimension { view_name, name } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "region");
                }
                other => panic!("Expected DuplicateDimension, got: {other}"),
            }
        }

        #[test]
        fn test_duplicate_metric_error() {
            let def = orders_view();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string(), "total_revenue".to_string()],
            };
            let result = expand("orders", &def, &req);
            assert!(result.is_err());
            match result.unwrap_err() {
                ExpandError::DuplicateMetric { view_name, name } => {
                    assert_eq!(view_name, "orders");
                    assert_eq!(name, "total_revenue");
                }
                other => panic!("Expected DuplicateMetric, got: {other}"),
            }
        }

        #[test]
        fn test_case_insensitive_metric_lookup() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "Total_Revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            // Request uses lowercase "total_revenue" but definition has "Total_Revenue"
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            // Should succeed and use the definition's name casing in the alias
            assert!(sql.contains("sum(amount) AS \"Total_Revenue\""));
        }

        #[test]
        fn test_error_display_messages() {
            // EmptyRequest
            let err = ExpandError::EmptyRequest {
                view_name: "orders".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("specify at least dimensions"));

            // UnknownDimension with suggestion
            let err = ExpandError::UnknownDimension {
                view_name: "orders".to_string(),
                name: "reigon".to_string(),
                available: vec!["region".to_string(), "status".to_string()],
                suggestion: Some("region".to_string()),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("reigon"));
            assert!(msg.contains("region, status"));
            assert!(msg.contains("Did you mean 'region'?"));

            // UnknownDimension without suggestion
            let err = ExpandError::UnknownDimension {
                view_name: "orders".to_string(),
                name: "xyzzy".to_string(),
                available: vec!["region".to_string()],
                suggestion: None,
            };
            let msg = format!("{err}");
            assert!(msg.contains("xyzzy"));
            assert!(!msg.contains("Did you mean"));

            // UnknownMetric with suggestion
            let err = ExpandError::UnknownMetric {
                view_name: "orders".to_string(),
                name: "totl_revenue".to_string(),
                available: vec!["total_revenue".to_string()],
                suggestion: Some("total_revenue".to_string()),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("totl_revenue"));
            assert!(msg.contains("Did you mean 'total_revenue'?"));

            // DuplicateDimension
            let err = ExpandError::DuplicateDimension {
                view_name: "orders".to_string(),
                name: "region".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("duplicate dimension 'region'"));

            // DuplicateMetric
            let err = ExpandError::DuplicateMetric {
                view_name: "orders".to_string(),
                name: "total_revenue".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("orders"));
            assert!(msg.contains("duplicate metric 'total_revenue'"));
        }

        // Legacy join tests (test_join_included_when_dimension_needs_it,
        // test_join_included_when_metric_needs_it, test_transitive_join_resolution,
        // test_joins_emitted_in_declaration_order, test_mixed_base_and_joined_dimensions,
        // test_dot_qualified_join_table) removed in Phase 27 -- legacy resolve_joins deleted.
        // PK/FK join resolution is now the only path; see phase26_pkfk_expand_tests.

        #[test]
        fn test_join_excluded_when_not_needed() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "region".to_string(),
                        source_table: None,

                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "customers.name".to_string(),
                        source_table: Some("customers".to_string()),

                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "customers".to_string(),
                    on: "orders.customer_id = customers.id".to_string(),
                    from_cols: vec![],
                    join_columns: vec![],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            // Request only "region" which comes from base table
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "JOIN should not appear when only base-table dims/metrics requested"
            );
        }

        #[test]
        fn test_no_joins_declared_no_error() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "no JOIN clauses when no joins declared"
            );
        }

        #[test]
        fn test_dot_qualified_base_table() {
            let def = SemanticViewDefinition {
                base_table: "jaffle.raw_orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "status".to_string(),
                    expr: "status".to_string(),
                    source_table: None,

                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "order_count".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["order_count".to_string()],
            };
            let sql = expand("jaffle_orders", &def, &req).unwrap();
            // Must produce "jaffle"."raw_orders" not "jaffle.raw_orders"
            assert!(
                sql.contains("FROM \"jaffle\".\"raw_orders\""),
                "dot-qualified base_table must be split and quoted: {sql}"
            );
        }
    }

    mod phase11_1_expand_tests {
        use super::*;
        use crate::model::{JoinColumn, TableRef};

        fn def_with_join_columns() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        ..Default::default()
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "customers".to_string(),
                        ..Default::default()
                    },
                ],
                dimensions: vec![
                    crate::model::Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),

                        output_type: None,
                    },
                    crate::model::Dimension {
                        name: "tier".to_string(),
                        expr: "c.tier".to_string(),
                        source_table: Some("c".to_string()),

                        output_type: None,
                    },
                ],
                metrics: vec![crate::model::Metric {
                    name: "revenue".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![crate::model::Join {
                    table: "customers".to_string(),
                    on: String::new(),
                    from_cols: vec![],
                    join_columns: vec![JoinColumn {
                        from: "customer_id".to_string(),
                        to: "id".to_string(),
                    }],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        // Legacy join_columns tests (join_columns_generates_on_clause,
        // multi_column_join_generates_and_joined_on_clause,
        // join_with_empty_join_columns_falls_back_to_on_string)
        // removed in Phase 27 -- legacy join resolution deleted.
        // PK/FK ON clause synthesis is tested in phase26_pkfk_expand_tests.

        #[test]
        fn table_qualified_dimension_lookup_with_matching_source_table() {
            // Test E: 'o.region' resolves to dimension named 'region' with source_table='o'
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["o.region".to_string()],
                metrics: vec![],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("o.region"),
                "Must include the dimension expr: {sql}"
            );
            assert!(
                sql.contains("AS \"region\""),
                "Must alias as bare name: {sql}"
            );
        }

        #[test]
        fn bare_dimension_name_still_resolves() {
            // Test F: 'region' (no prefix) resolves by bare name (backward compat)
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            assert!(
                result.is_ok(),
                "Bare name lookup must succeed: {:?}",
                result.err()
            );
        }

        #[test]
        fn table_qualified_unknown_dimension_returns_error() {
            // Test G: 'o.nosuch' returns UnknownDimension with full 'o.nosuch' as name
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec!["o.nosuch".to_string()],
                metrics: vec![],
            };
            let result = expand("sales_view", &def, &req);
            match result {
                Err(ExpandError::UnknownDimension { name, .. }) => {
                    // The error name may be the bare 'nosuch' (after fallback) — that's fine
                    // What matters is it returns an error
                    let _ = name;
                }
                other => panic!("Expected UnknownDimension error, got: {:?}", other),
            }
        }

        #[test]
        fn table_qualified_metric_lookup_with_matching_source_table() {
            // Test I: 'o.revenue' resolves to metric named 'revenue' with source_table='o'
            let def = def_with_join_columns();
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["o.revenue".to_string()],
            };
            let sql = expand("sales_view", &def, &req).unwrap();
            assert!(
                sql.contains("sum(o.amount)"),
                "Must include metric expr: {sql}"
            );
        }
    }

    mod phase12_cast_tests {
        use super::*;
        use crate::model::{Dimension, Metric};

        #[test]
        fn output_type_on_metric_emits_cast() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: Some("BIGINT".to_string()),
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(sum(amount) AS BIGINT)"),
                "output_type BIGINT must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn output_type_on_dimension_emits_cast() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region_id".to_string(),
                    expr: "region_id".to_string(),
                    source_table: None,

                    output_type: Some("INTEGER".to_string()),
                }],
                metrics: vec![],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region_id".to_string()],
                metrics: vec![],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                sql.contains("CAST(region_id AS INTEGER)"),
                "output_type INTEGER on dimension must generate CAST wrapper: {sql}"
            );
        }

        #[test]
        fn no_output_type_no_cast() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "sum(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("orders", &def, &req).unwrap();
            assert!(
                !sql.contains("CAST(sum(amount) AS"),
                "No output_type must not generate CAST: {sql}"
            );
            assert!(
                sql.contains("sum(amount) AS"),
                "Bare expr must be present: {sql}"
            );
        }
    }

    mod phase26_pkfk_expand_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, TableRef};

        /// Helper: build a 2-table PK/FK definition (orders -> customers).
        fn pkfk_two_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
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
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        /// Helper: build a 3-table PK/FK definition (li -> o -> c).
        fn pkfk_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "line_items".to_string(),
                tables: vec![
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
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
                dimensions: vec![
                    Dimension {
                        name: "product".to_string(),
                        expr: "li.product".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_qty".to_string(),
                    expr: "sum(li.qty)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![
                    Join {
                        table: "o".to_string(),
                        from_alias: "li".to_string(),
                        fk_columns: vec!["order_id".to_string()],
                        ..Default::default()
                    },
                    Join {
                        table: "c".to_string(),
                        from_alias: "o".to_string(),
                        fk_columns: vec!["customer_id".to_string()],
                        ..Default::default()
                    },
                ],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        #[test]
        fn test_pkfk_on_clause_simple() {
            // Single FK->PK: o.customer_id = c.id
            let def = pkfk_two_table_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("\"o\".\"customer_id\" = \"c\".\"id\""),
                "PK/FK ON clause must use from_alias.fk = to_alias.pk: {sql}"
            );
        }

        #[test]
        fn test_pkfk_on_clause_composite() {
            // Multi-column FK->PK: a.fk1 = b.pk1 AND a.fk2 = b.pk2
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "a".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "b".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["pk1".to_string(), "pk2".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "b.detail".to_string(),
                    source_table: Some("b".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: Some("a".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "b".to_string(),
                    from_alias: "a".to_string(),
                    fk_columns: vec!["fk1".to_string(), "fk2".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["detail".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("\"a\".\"fk1\" = \"b\".\"pk1\""),
                "First FK/PK pair must appear: {sql}"
            );
            assert!(sql.contains("AND"), "Composite ON must use AND: {sql}");
            assert!(
                sql.contains("\"a\".\"fk2\" = \"b\".\"pk2\""),
                "Second FK/PK pair must appear: {sql}"
            );
        }

        #[test]
        fn test_pkfk_left_join_emitted() {
            let def = pkfk_two_table_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN"),
                "PK/FK path must emit LEFT JOIN: {sql}"
            );
            // Must NOT have bare " JOIN " (without LEFT prefix) for the actual join
            let join_lines: Vec<&str> = sql
                .lines()
                .filter(|l| l.trim().starts_with("LEFT JOIN") || l.trim().starts_with("JOIN"))
                .collect();
            for line in &join_lines {
                assert!(
                    line.trim().starts_with("LEFT JOIN"),
                    "All joins must be LEFT JOIN, got: {line}"
                );
            }
        }

        #[test]
        fn test_pkfk_transitive_join_inclusion() {
            // A(li)->B(o)->C(c): request dim from C, must include B(o) join too
            let def = pkfk_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_qty".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"orders\" AS \"o\""),
                "Transitive intermediate join (o) must be included: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"customers\" AS \"c\""),
                "Target join (c) must be included: {sql}"
            );
        }

        #[test]
        fn test_pkfk_pruning() {
            // Request only base-table dims: no joins needed
            let def = pkfk_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["product".to_string()],
                metrics: vec!["total_qty".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "No joins needed when only base-table dims requested: {sql}"
            );
        }

        #[test]
        fn test_pkfk_topological_order() {
            // Joins must be emitted root-outward (li first, then o, then c)
            // regardless of declaration order
            let mut def = pkfk_three_table_def();
            // Reverse declaration order of joins to test that topo sort overrides it
            def.joins.reverse();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_qty".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            let o_pos = sql
                .find("LEFT JOIN \"orders\"")
                .expect("orders join missing");
            let c_pos = sql
                .find("LEFT JOIN \"customers\"")
                .expect("customers join missing");
            assert!(
                o_pos < c_pos,
                "orders (closer to root) must appear before customers (further from root) in topo order: {sql}"
            );
        }

        // Legacy compat tests (test_legacy_join_columns_still_works,
        // test_legacy_on_string_still_works) removed in Phase 27 --
        // legacy join resolution deleted per no-backward-compat policy.
    }

    mod phase27_qualified_refs_tests {
        use super::*;
        use crate::model::{Dimension, Join, Metric, TableRef};

        /// Build a 2-table PK/FK definition for qualified column ref testing.
        fn qualified_ref_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "p27_orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        #[test]
        fn test_expand_qualified_column_refs_verbatim() {
            // EXP-05: qualified column references (alias.column) in dimension/metric
            // expressions must appear verbatim in generated SQL, not stripped or rewritten.
            let def = qualified_ref_def();
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("p27_test", &def, &req).unwrap();

            // The dimension expr "c.name" must appear verbatim in SELECT
            assert!(
                sql.contains("c.name AS"),
                "Qualified dim expr 'c.name' must appear verbatim in SQL: {sql}"
            );

            // The metric expr "sum(o.amount)" must appear verbatim in SELECT
            assert!(
                sql.contains("sum(o.amount) AS"),
                "Qualified metric expr 'sum(o.amount)' must appear verbatim in SQL: {sql}"
            );
        }

        #[test]
        fn test_expand_multiple_qualified_refs_different_tables() {
            // Multiple qualified refs from different tables resolve correctly
            let def = SemanticViewDefinition {
                base_table: "p27_orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "p27_orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "c".to_string(),
                        table: "p27_customers".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "customer_name".to_string(),
                        expr: "c.name".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "order_region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![Metric {
                    name: "total_amount".to_string(),
                    expr: "sum(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string(), "order_region".to_string()],
                metrics: vec!["total_amount".to_string()],
            };
            let sql = expand("p27_test", &def, &req).unwrap();

            // Both qualified dim exprs must appear verbatim
            assert!(
                sql.contains("c.name AS"),
                "Qualified dim expr 'c.name' must appear verbatim: {sql}"
            );
            assert!(
                sql.contains("o.region AS"),
                "Qualified dim expr 'o.region' must appear verbatim: {sql}"
            );
            assert!(
                sql.contains("sum(o.amount) AS"),
                "Qualified metric expr 'sum(o.amount)' must appear verbatim: {sql}"
            );
        }
    }

    mod phase29_fact_inlining_tests {
        use super::*;
        use crate::model::{Dimension, Fact, Metric};

        // -------------------------------------------------------------------
        // replace_word_boundary tests
        // -------------------------------------------------------------------

        #[test]
        fn replace_word_boundary_no_match() {
            let result = replace_word_boundary("SUM(total)", "net_price", "(x)");
            assert_eq!(result, "SUM(total)");
        }

        #[test]
        fn replace_word_boundary_exact_match_in_function() {
            let result =
                replace_word_boundary("SUM(net_price)", "net_price", "(price * (1 - discount))");
            assert_eq!(result, "SUM((price * (1 - discount)))");
        }

        #[test]
        fn replace_word_boundary_no_substring_match_suffix() {
            // "net_price" should NOT match in "net_price_total"
            let result = replace_word_boundary("SUM(net_price_total)", "net_price", "(x)");
            assert_eq!(result, "SUM(net_price_total)");
        }

        #[test]
        fn replace_word_boundary_no_substring_match_prefix() {
            // "net_price" should NOT match in "total_net_price_x"
            let result = replace_word_boundary("total_net_price_x + 1", "net_price", "(x)");
            assert_eq!(result, "total_net_price_x + 1");
        }

        #[test]
        fn replace_word_boundary_match_with_addition() {
            let result = replace_word_boundary("net_price + tax", "net_price", "(a + b)");
            assert_eq!(result, "(a + b) + tax");
        }

        #[test]
        fn replace_word_boundary_match_in_parens() {
            let result = replace_word_boundary("(net_price)", "net_price", "(a)");
            assert_eq!(result, "((a))");
        }

        #[test]
        fn replace_word_boundary_entire_string() {
            let result = replace_word_boundary("net_price", "net_price", "(a + b)");
            assert_eq!(result, "(a + b)");
        }

        #[test]
        fn replace_word_boundary_at_start() {
            let result = replace_word_boundary("net_price * 2", "net_price", "(x)");
            assert_eq!(result, "(x) * 2");
        }

        #[test]
        fn replace_word_boundary_at_end() {
            let result = replace_word_boundary("2 * net_price", "net_price", "(x)");
            assert_eq!(result, "2 * (x)");
        }

        #[test]
        fn replace_word_boundary_multiple_occurrences() {
            let result = replace_word_boundary("net_price + net_price", "net_price", "(x)");
            assert_eq!(result, "(x) + (x)");
        }

        #[test]
        fn replace_word_boundary_empty_needle() {
            let result = replace_word_boundary("abc", "", "x");
            assert_eq!(result, "abc");
        }

        // -------------------------------------------------------------------
        // toposort_facts tests
        // -------------------------------------------------------------------

        #[test]
        fn toposort_facts_empty() {
            let order = toposort_facts(&[]).unwrap();
            assert!(order.is_empty());
        }

        #[test]
        fn toposort_facts_independent() {
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "x + 1".to_string(),
                    source_table: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "y + 2".to_string(),
                    source_table: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 2);
            // Both are independent, order should contain both indices
            assert!(order.contains(&0));
            assert!(order.contains(&1));
        }

        #[test]
        fn toposort_facts_chain() {
            // b depends on a: a must come before b in topo order
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price * qty".to_string(),
                    source_table: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * (1 - discount)".to_string(),
                    source_table: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 2);
            let a_pos = order.iter().position(|&x| x == 0).unwrap();
            let b_pos = order.iter().position(|&x| x == 1).unwrap();
            assert!(a_pos < b_pos, "a (leaf) must come before b (depends on a)");
        }

        #[test]
        fn toposort_facts_three_level_chain() {
            // c depends on b, b depends on a
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price".to_string(),
                    source_table: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * qty".to_string(),
                    source_table: None,
                },
                Fact {
                    name: "c".to_string(),
                    expr: "b * tax".to_string(),
                    source_table: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            assert_eq!(order.len(), 3);
            let a_pos = order.iter().position(|&x| x == 0).unwrap();
            let b_pos = order.iter().position(|&x| x == 1).unwrap();
            let c_pos = order.iter().position(|&x| x == 2).unwrap();
            assert!(a_pos < b_pos);
            assert!(b_pos < c_pos);
        }

        // -------------------------------------------------------------------
        // inline_facts tests
        // -------------------------------------------------------------------

        #[test]
        fn inline_facts_no_facts() {
            let result = inline_facts("SUM(price)", &[], &[]);
            assert_eq!(result, "SUM(price)");
        }

        #[test]
        fn inline_facts_single_fact() {
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "price * (1 - discount)".to_string(),
                source_table: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(net_price)", &facts, &order);
            assert_eq!(result, "SUM((price * (1 - discount)))");
        }

        #[test]
        fn inline_facts_multi_level() {
            // fact a = price * qty
            // fact b = a * (1 - discount)  -- depends on a
            // metric expr: SUM(b)
            // Expected: SUM(((price * qty) * (1 - discount)))
            let facts = vec![
                Fact {
                    name: "a".to_string(),
                    expr: "price * qty".to_string(),
                    source_table: None,
                },
                Fact {
                    name: "b".to_string(),
                    expr: "a * (1 - discount)".to_string(),
                    source_table: None,
                },
            ];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(b)", &facts, &order);
            assert_eq!(result, "SUM(((price * qty) * (1 - discount)))");
        }

        #[test]
        fn inline_facts_preserves_parenthesization() {
            // fact = a + b, inlined into expr * fact -> expr * (a + b)
            let facts = vec![Fact {
                name: "total".to_string(),
                expr: "a + b".to_string(),
                source_table: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("x * total", &facts, &order);
            assert_eq!(result, "x * (a + b)");
        }

        #[test]
        fn inline_facts_word_boundary_prevents_collision() {
            // fact named "net_price" should NOT match "net_price_total"
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "p * q".to_string(),
                source_table: None,
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(net_price_total)", &facts, &order);
            assert_eq!(
                result, "SUM(net_price_total)",
                "Word boundary must prevent matching"
            );
        }

        #[test]
        fn inline_facts_with_qualified_name_in_metric() {
            // fact has source_table = "li", name = "net_price"
            // metric expr references "li.net_price"
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "li.price * (1 - li.discount)".to_string(),
                source_table: Some("li".to_string()),
            }];
            let order = toposort_facts(&facts).unwrap();
            let result = inline_facts("SUM(li.net_price)", &facts, &order);
            assert_eq!(result, "SUM((li.price * (1 - li.discount)))");
        }

        // -------------------------------------------------------------------
        // End-to-end expand() with facts
        // -------------------------------------------------------------------

        #[test]
        fn expand_with_facts_inlines_into_metric() {
            let def = SemanticViewDefinition {
                base_table: "line_items".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "total_net".to_string(),
                    expr: "SUM(net_price)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![Fact {
                    name: "net_price".to_string(),
                    expr: "price * (1 - discount)".to_string(),
                    source_table: None,
                }],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["total_net".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM((price * (1 - discount)))"),
                "Fact inlining must resolve net_price in metric expr: {sql}"
            );
        }

        #[test]
        fn expand_without_facts_unchanged() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "total".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM(amount) AS"),
                "Without facts, metric expr unchanged: {sql}"
            );
        }

        #[test]
        fn expand_multi_level_facts() {
            // net_price = extended_price * (1 - discount)
            // tax_amount = net_price * tax_rate
            // Metric: SUM(tax_amount) -> SUM(((extended_price * (1 - discount)) * tax_rate))
            let def = SemanticViewDefinition {
                base_table: "line_items".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![Metric {
                    name: "total_tax".to_string(),
                    expr: "SUM(tax_amount)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![
                    Fact {
                        name: "net_price".to_string(),
                        expr: "extended_price * (1 - discount)".to_string(),
                        source_table: None,
                    },
                    Fact {
                        name: "tax_amount".to_string(),
                        expr: "net_price * tax_rate".to_string(),
                        source_table: None,
                    },
                ],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["total_tax".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("SUM(((extended_price * (1 - discount)) * tax_rate))"),
                "Multi-level fact chain must resolve correctly: {sql}"
            );
        }
    }

    mod phase30_derived_metric_tests {
        use super::*;
        use crate::model::{Dimension, Fact, Join, Metric, TableRef};

        // -------------------------------------------------------------------
        // inline_derived_metrics unit tests
        // -------------------------------------------------------------------

        #[test]
        fn inline_derived_one_base_one_derived() {
            // Base: revenue = SUM(amount), cost = SUM(unit_cost)
            // Derived: profit = revenue - cost
            // Expected: profit -> (SUM(amount)) - (SUM(unit_cost))
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("profit").unwrap(),
                "(SUM(amount)) - (SUM(unit_cost))"
            );
        }

        #[test]
        fn inline_derived_stacked() {
            // Base: revenue, cost
            // Derived: profit = revenue - cost
            // Derived: margin = profit / revenue * 100
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "cost".to_string(),
                    expr: "SUM(unit_cost)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "revenue - cost".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / revenue * 100".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("profit").unwrap(),
                "(SUM(amount)) - (SUM(unit_cost))"
            );
            assert_eq!(
                resolved.get("margin").unwrap(),
                "((SUM(amount)) - (SUM(unit_cost))) / (SUM(amount)) * 100"
            );
        }

        #[test]
        fn inline_derived_with_facts() {
            // Fact: net_price = extended_price * (1 - discount)
            // Base: revenue = SUM(net_price)
            // Derived: double_rev = revenue * 2
            // Chain: fact -> base metric -> derived metric
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(net_price)".to_string(),
                    source_table: Some("li".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "double_rev".to_string(),
                    expr: "revenue * 2".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let facts = vec![Fact {
                name: "net_price".to_string(),
                expr: "extended_price * (1 - discount)".to_string(),
                source_table: Some("li".to_string()),
            }];
            let topo_order = toposort_facts(&facts).unwrap();
            let resolved = inline_derived_metrics(&metrics, &facts, &topo_order);
            assert_eq!(
                resolved.get("revenue").unwrap(),
                "SUM((extended_price * (1 - discount)))"
            );
            assert_eq!(
                resolved.get("double_rev").unwrap(),
                "(SUM((extended_price * (1 - discount)))) * 2"
            );
        }

        #[test]
        fn inline_derived_parenthesization_prevents_precedence_error() {
            // profit = a - b, margin = profit / a
            // Without parens: a - b / a (division before subtraction!)
            // With parens: (a - b) / (a)
            let metrics = vec![
                Metric {
                    name: "a".to_string(),
                    expr: "SUM(x)".to_string(),
                    source_table: Some("t".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "b".to_string(),
                    expr: "SUM(y)".to_string(),
                    source_table: Some("t".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "profit".to_string(),
                    expr: "a - b".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "margin".to_string(),
                    expr: "profit / a".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            // margin should be ((SUM(x)) - (SUM(y))) / (SUM(x))
            // NOT (SUM(x)) - (SUM(y)) / (SUM(x))
            assert_eq!(
                resolved.get("margin").unwrap(),
                "((SUM(x)) - (SUM(y))) / (SUM(x))"
            );
        }

        #[test]
        fn inline_derived_word_boundary_safety() {
            // Metric named "revenue" must not match "revenue_total"
            let metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "revenue_total".to_string(),
                    expr: "SUM(total)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                },
                Metric {
                    name: "derived".to_string(),
                    expr: "revenue + revenue_total".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                },
            ];
            let resolved = inline_derived_metrics(&metrics, &[], &[]);
            assert_eq!(
                resolved.get("derived").unwrap(),
                "(SUM(amount)) + (SUM(total))"
            );
        }

        // -------------------------------------------------------------------
        // expand() with derived metrics
        // -------------------------------------------------------------------

        #[test]
        fn expand_derived_metric_correct_sql() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(amount)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(unit_cost)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["profit".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("(SUM(amount)) - (SUM(unit_cost)) AS \"profit\""),
                "Derived metric must expand to inlined expression: {sql}"
            );
            // Derived metric should NOT add extra GROUP BY entries
            assert!(
                sql.contains("GROUP BY\n    1"),
                "GROUP BY should reference only the dimension: {sql}"
            );
        }

        #[test]
        fn expand_derived_only_no_base_metrics_requested() {
            // Only request the derived metric -- JOINs for referenced base metrics
            // must still be included.
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(li.unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["profit".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"line_items\" AS \"li\""),
                "JOIN to li must be included for derived metric referencing li-based metrics: {sql}"
            );
            assert!(
                sql.contains("(SUM(li.amount)) - (SUM(li.unit_cost)) AS \"profit\""),
                "Derived metric expression must be inlined: {sql}"
            );
        }

        #[test]
        fn resolve_joins_includes_transitive_deps_from_derived() {
            // Derived metric references base metrics from different tables --
            // all those tables' joins must be included.
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.amount)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(DISTINCT o.id)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "avg_order_value".to_string(),
                        expr: "revenue / order_count".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![Join {
                    table: "o".to_string(),
                    from_alias: "li".to_string(),
                    fk_columns: vec!["order_id".to_string()],
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["avg_order_value".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            // li must be joined because revenue (source_table=li) is referenced by avg_order_value
            assert!(
                sql.contains("LEFT JOIN \"line_items\" AS \"li\""),
                "JOIN to li must be included for derived metric avg_order_value: {sql}"
            );
        }

        #[test]
        fn expand_derived_metric_with_facts_chain() {
            // Fact: net_price = extended_price * (1 - discount)
            // Base: revenue = SUM(net_price), cost = SUM(unit_cost)
            // Derived: profit = revenue - cost
            // Chain: fact -> base metric -> derived metric
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(net_price)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "cost".to_string(),
                        expr: "SUM(unit_cost)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "profit".to_string(),
                        expr: "revenue - cost".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![],
                facts: vec![Fact {
                    name: "net_price".to_string(),
                    expr: "extended_price * (1 - discount)".to_string(),
                    source_table: Some("li".to_string()),
                }],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec!["profit".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            // profit = revenue - cost
            // revenue = SUM(net_price) -> SUM((extended_price * (1 - discount)))
            // cost = SUM(unit_cost) (unchanged)
            // profit -> (SUM((extended_price * (1 - discount)))) - (SUM(unit_cost))
            assert!(
                sql.contains(
                    "(SUM((extended_price * (1 - discount)))) - (SUM(unit_cost)) AS \"profit\""
                ),
                "Fact->base->derived chain must resolve correctly: {sql}"
            );
        }
    }

    mod phase31_fan_trap_tests {
        use super::*;
        use crate::model::{Cardinality, Dimension, Join, Metric, TableRef};

        /// Helper: build a 3-table definition (o root, li->o, o->c)
        /// where li->o is MANY TO ONE and o->c is MANY TO ONE.
        fn fan_trap_three_table_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "li".to_string(),
                        table: "line_items".to_string(),
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
                dimensions: vec![
                    Dimension {
                        name: "region".to_string(),
                        expr: "o.region".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "status".to_string(),
                        expr: "li.status".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "segment".to_string(),
                        expr: "c.segment".to_string(),
                        source_table: Some("c".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "revenue".to_string(),
                        expr: "SUM(li.extended_price)".to_string(),
                        source_table: Some("li".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                    Metric {
                        name: "order_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("o".to_string()),
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![
                    Join {
                        table: "o".to_string(),
                        from_alias: "li".to_string(),
                        fk_columns: vec!["order_id".to_string()],
                        ref_columns: vec!["id".to_string()],
                        name: Some("li_to_order".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                    Join {
                        table: "c".to_string(),
                        from_alias: "o".to_string(),
                        fk_columns: vec!["customer_id".to_string()],
                        ref_columns: vec!["id".to_string()],
                        name: Some("order_to_customer".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                ],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        #[test]
        fn fan_trap_one_to_many_blocked() {
            // Metric from o (order_count), dimension from li (status).
            // Path: o -> li. Edge is li->o ManyToOne. Reverse traversal o->li is fan-out.
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["order_count".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Fan trap must block the query");
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    view_name,
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(view_name, "sales");
                    assert_eq!(metric_name, "order_count");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_many_to_one_safe() {
            // Metric from li (revenue), dimension from o (region).
            // Path: li -> o. Edge is li->o ManyToOne. Forward traversal = safe.
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_ok(),
                "MANY TO ONE direction must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_one_to_one_safe() {
            // ONE TO ONE: both directions are safe
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
                    TableRef {
                        alias: "o".to_string(),
                        table: "orders".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "d".to_string(),
                        table: "details".to_string(),
                        pk_columns: vec!["id".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "detail".to_string(),
                    expr: "d.detail".to_string(),
                    source_table: Some("d".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "d".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["detail_id".to_string()],
                    ref_columns: vec!["id".to_string()],
                    name: Some("order_to_detail".to_string()),
                    cardinality: Cardinality::OneToOne,
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["detail".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(
                result.is_ok(),
                "ONE TO ONE must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_same_table_safe() {
            // Metric and dimension from the same table -> no fan-out
            let def = fan_trap_three_table_def();
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            // Both from li -> same table, should be OK
            let result = expand("sales", &def, &req);
            assert!(
                result.is_ok(),
                "Same table must be safe: {:?}",
                result.err()
            );
        }

        #[test]
        fn fan_trap_no_joins_safe() {
            // Single-table view, no joins -> check_fan_traps returns Ok immediately
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "region".to_string(),
                    source_table: None,
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: None,
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(result.is_ok(), "No joins must be safe: {:?}", result.err());
        }

        #[test]
        fn fan_trap_transitive_chain() {
            // 3-table chain: metric from c (customer_count), dimension from li (status).
            // Path: c -> o -> li. Edge o->c is o(customer_id) REFERENCES c ManyToOne.
            // Traversing c->o is reverse of o->c ManyToOne = fan-out!
            // But also: edge li->o is ManyToOne. Traversing o->li is fan-out too.
            // Either should trigger the fan trap.
            let mut def = fan_trap_three_table_def();
            def.metrics.push(Metric {
                name: "customer_count".to_string(),
                expr: "COUNT(DISTINCT c.id)".to_string(),
                source_table: Some("c".to_string()),
                output_type: None,
                using_relationships: vec![],
            });
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["customer_count".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(
                result.is_err(),
                "Transitive chain fan trap must be detected"
            );
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(metric_name, "customer_count");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_derived_metric_blocked() {
            // Derived metric that references a base metric from o.
            // Queried with dimension from li -> fan-out through the derived metric's
            // transitive source table (o).
            let mut def = fan_trap_three_table_def();
            def.metrics.push(Metric {
                name: "avg_order".to_string(),
                expr: "order_count / 1".to_string(),
                source_table: None, // derived
                output_type: None,
                using_relationships: vec![],
            });
            let req = QueryRequest {
                dimensions: vec!["status".to_string()],
                metrics: vec!["avg_order".to_string()],
            };
            let result = expand("sales", &def, &req);
            assert!(result.is_err(), "Derived metric fan trap must be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap {
                    metric_name,
                    dimension_name,
                    ..
                } => {
                    assert_eq!(metric_name, "avg_order");
                    assert_eq!(dimension_name, "status");
                }
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn fan_trap_error_message_format() {
            let err = ExpandError::FanTrap {
                view_name: "sales".to_string(),
                metric_name: "order_count".to_string(),
                metric_table: "o".to_string(),
                dimension_name: "status".to_string(),
                dimension_table: "li".to_string(),
                relationship_name: "li_to_order".to_string(),
            };
            let msg = format!("{err}");
            assert!(msg.contains("sales"), "Must contain view name");
            assert!(msg.contains("order_count"), "Must contain metric name");
            assert!(msg.contains("status"), "Must contain dimension name");
            assert!(
                msg.contains("li_to_order"),
                "Must contain relationship name"
            );
            assert!(
                msg.contains("fan trap detected"),
                "Must contain 'fan trap detected'"
            );
            assert!(
                msg.contains("many-to-one cardinality"),
                "Must describe the cardinality direction"
            );
        }
    }

    mod phase32_role_playing_tests {
        use super::*;
        use crate::model::{Cardinality, Dimension, Join, Metric, TableRef};

        /// Helper: build the flights/airports definition with two role-playing
        /// relationships (dep_airport, arr_airport) to the same airports table.
        fn flights_airports_def() -> SemanticViewDefinition {
            SemanticViewDefinition {
                base_table: "flights".to_string(),
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![
                    Dimension {
                        name: "city".to_string(),
                        expr: "a.city".to_string(),
                        source_table: Some("a".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "country".to_string(),
                        expr: "a.country".to_string(),
                        source_table: Some("a".to_string()),
                        output_type: None,
                    },
                    Dimension {
                        name: "carrier".to_string(),
                        expr: "f.carrier".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                    },
                ],
                metrics: vec![
                    Metric {
                        name: "departure_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        using_relationships: vec!["dep_airport".to_string()],
                    },
                    Metric {
                        name: "arrival_count".to_string(),
                        expr: "COUNT(*)".to_string(),
                        source_table: Some("f".to_string()),
                        output_type: None,
                        using_relationships: vec!["arr_airport".to_string()],
                    },
                    Metric {
                        name: "total_flights".to_string(),
                        expr: "departure_count + arrival_count".to_string(),
                        source_table: None,
                        output_type: None,
                        using_relationships: vec![],
                    },
                ],

                joins: vec![
                    Join {
                        table: "a".to_string(),
                        from_alias: "f".to_string(),
                        fk_columns: vec!["departure_code".to_string()],
                        ref_columns: vec!["airport_code".to_string()],
                        name: Some("dep_airport".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                    Join {
                        table: "a".to_string(),
                        from_alias: "f".to_string(),
                        fk_columns: vec!["arrival_code".to_string()],
                        ref_columns: vec!["airport_code".to_string()],
                        name: Some("arr_airport".to_string()),
                        cardinality: Cardinality::ManyToOne,
                        ..Default::default()
                    },
                ],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            }
        }

        #[test]
        fn using_metric_generates_scoped_join_alias() {
            // Metric with USING (dep_airport) generates JOIN with alias "a__dep_airport"
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("a__dep_airport"),
                "Scoped alias a__dep_airport must appear: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "LEFT JOIN with scoped alias must appear: {sql}"
            );
        }

        #[test]
        fn two_using_metrics_generate_two_scoped_joins() {
            // Two metrics with different USING generate two separate LEFT JOINs
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["departure_count".to_string(), "arrival_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "dep_airport scoped JOIN must appear: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
                "arr_airport scoped JOIN must appear: {sql}"
            );
        }

        #[test]
        fn dimension_rewritten_to_scoped_alias() {
            // Dimension expression "a.city" rewritten to "a__dep_airport.city"
            // when co-queried metric uses dep_airport
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("a__dep_airport.city"),
                "Dimension must be rewritten to scoped alias: {sql}"
            );
        }

        #[test]
        fn ambiguous_dimension_without_using_produces_error() {
            // Dimension from ambiguous table without any metric USING
            // produces AmbiguousPath error
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec![],
            };
            let result = expand("test_flights", &def, &req);
            assert!(result.is_err(), "Ambiguous dimension must produce error");
            match result.unwrap_err() {
                ExpandError::AmbiguousPath {
                    view_name,
                    dimension_name,
                    dimension_table,
                    available_relationships,
                } => {
                    assert_eq!(view_name, "test_flights");
                    assert_eq!(dimension_name, "city");
                    assert_eq!(dimension_table, "a");
                    assert!(available_relationships.contains(&"dep_airport".to_string()));
                    assert!(available_relationships.contains(&"arr_airport".to_string()));
                }
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn ambiguous_path_error_lists_relationships() {
            let err = ExpandError::AmbiguousPath {
                view_name: "test_flights".to_string(),
                dimension_name: "city".to_string(),
                dimension_table: "a".to_string(),
                available_relationships: vec!["dep_airport".to_string(), "arr_airport".to_string()],
            };
            let msg = format!("{err}");
            assert!(msg.contains("test_flights"));
            assert!(msg.contains("city"));
            assert!(msg.contains("ambiguous"));
            assert!(msg.contains("dep_airport"));
            assert!(msg.contains("arr_airport"));
        }

        #[test]
        fn non_ambiguous_single_relationship_works_without_using() {
            // Single relationship to a table (no role-playing) works fine
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
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
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    name: Some("order_to_customer".to_string()),
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(
                result.is_ok(),
                "Single relationship must work without USING: {:?}",
                result.err()
            );
        }

        #[test]
        fn base_table_dimension_works_unchanged() {
            // Dimension from base table (no relationship needed) works unchanged
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("f.carrier AS \"carrier\""),
                "Base table dimension must appear unchanged: {sql}"
            );
        }

        #[test]
        fn fan_trap_detection_works_with_using_paths() {
            // Fan trap detection should still work with USING-scoped paths.
            // Create a scenario with a ManyToOne edge traversed in reverse (fan-out).
            // flights(dep_airport_code) -> airports: ManyToOne
            // Metric on airports, dimension on flights -> traverses ManyToOne in reverse = fan-out
            let def = SemanticViewDefinition {
                base_table: "flights".to_string(),
                tables: vec![
                    TableRef {
                        alias: "f".to_string(),
                        table: "flights".to_string(),
                        pk_columns: vec!["flight_id".to_string()],
                        unique_constraints: vec![],
                    },
                    TableRef {
                        alias: "a".to_string(),
                        table: "airports".to_string(),
                        pk_columns: vec!["airport_code".to_string()],
                        unique_constraints: vec![],
                    },
                ],
                dimensions: vec![Dimension {
                    name: "carrier".to_string(),
                    expr: "f.carrier".to_string(),
                    source_table: Some("f".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "airport_count".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("a".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "a".to_string(),
                    from_alias: "f".to_string(),
                    fk_columns: vec!["dep_airport_code".to_string()],
                    ref_columns: vec!["airport_code".to_string()],
                    name: Some("dep_flights".to_string()),
                    cardinality: Cardinality::ManyToOne,
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["airport_count".to_string()],
            };
            let result = expand("test", &def, &req);
            assert!(result.is_err(), "Fan trap must still be detected");
            match result.unwrap_err() {
                ExpandError::FanTrap { .. } => {}
                other => panic!("Expected FanTrap, got: {other}"),
            }
        }

        #[test]
        fn derived_metric_with_two_using_resolves_both_joins() {
            // Derived metric referencing two base metrics with different USING paths
            // resolves both join paths
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["carrier".to_string()],
                metrics: vec!["total_flights".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
                "Derived metric must resolve dep_airport join: {sql}"
            );
            assert!(
                sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
                "Derived metric must resolve arr_airport join: {sql}"
            );
        }

        #[test]
        fn metric_using_from_base_table_no_unnecessary_join() {
            // Metric with USING from base table (no join needed) does not emit JOIN
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![TableRef {
                    alias: "o".to_string(),
                    table: "orders".to_string(),
                    pk_columns: vec!["id".to_string()],
                    unique_constraints: vec![],
                }],
                dimensions: vec![Dimension {
                    name: "region".to_string(),
                    expr: "o.region".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "cnt".to_string(),
                    expr: "COUNT(*)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["region".to_string()],
                metrics: vec!["cnt".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                !sql.contains("JOIN"),
                "No JOIN needed when everything is on base table: {sql}"
            );
        }

        #[test]
        fn backward_compat_no_using_expands_as_before() {
            // Definition without any USING expands exactly as before (backward compat)
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![
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
                dimensions: vec![Dimension {
                    name: "customer_name".to_string(),
                    expr: "c.name".to_string(),
                    source_table: Some("c".to_string()),
                    output_type: None,
                }],
                metrics: vec![Metric {
                    name: "revenue".to_string(),
                    expr: "SUM(o.amount)".to_string(),
                    source_table: Some("o".to_string()),
                    output_type: None,
                    using_relationships: vec![],
                }],

                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    name: Some("order_to_customer".to_string()),
                    ..Default::default()
                }],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
            };
            let req = QueryRequest {
                dimensions: vec!["customer_name".to_string()],
                metrics: vec!["revenue".to_string()],
            };
            let sql = expand("test", &def, &req).unwrap();
            assert!(
                sql.contains("LEFT JOIN \"customers\" AS \"c\""),
                "Non-USING definition must use bare alias: {sql}"
            );
            assert!(
                sql.contains("c.name AS"),
                "Dimension expr must use bare alias: {sql}"
            );
        }

        #[test]
        fn ambiguous_dimension_with_derived_metric_using_both_paths() {
            // Derived metric total_flights uses both USING paths.
            // City dimension from airports is ambiguous because both paths exist.
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["total_flights".to_string()],
            };
            let result = expand("test_flights", &def, &req);
            assert!(
                result.is_err(),
                "City dimension must be ambiguous when derived metric uses both paths"
            );
            match result.unwrap_err() {
                ExpandError::AmbiguousPath { .. } => {}
                other => panic!("Expected AmbiguousPath, got: {other}"),
            }
        }

        #[test]
        fn scoped_join_on_clause_uses_correct_fk_pk() {
            // Verify the ON clause of scoped joins uses the correct FK/PK columns
            let def = flights_airports_def();
            let req = QueryRequest {
                dimensions: vec!["city".to_string()],
                metrics: vec!["departure_count".to_string()],
            };
            let sql = expand("test_flights", &def, &req).unwrap();
            // ON clause for dep_airport: f.departure_code = a__dep_airport.airport_code
            assert!(
                sql.contains("\"f\".\"departure_code\" = \"a__dep_airport\".\"airport_code\""),
                "Scoped JOIN ON clause must use correct FK/PK: {sql}"
            );
        }
    }
}
