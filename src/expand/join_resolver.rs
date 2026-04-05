use std::collections::HashSet;

use crate::graph::RelationshipGraph;
use crate::model::{Join, SemanticViewDefinition, TableRef};

use super::facts::{collect_derived_metric_source_tables, collect_derived_metric_using};
use super::resolution::quote_ident;

/// Synthesize an ON clause from PK/FK column declarations (Phase 26).
///
/// Zips `join.fk_columns` with the referenced table's `pk_columns` to produce
/// `from_alias.fk = to_alias.pk` pairs, joined by ` AND `.
/// Uses `join.from_alias` for the FROM side and `join.table` for the TO side.
pub(super) fn synthesize_on_clause(join: &Join, tables: &[TableRef]) -> String {
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
pub(super) fn synthesize_on_clause_scoped(
    join: &Join,
    tables: &[TableRef],
    to_alias: &str,
) -> String {
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
pub(super) fn resolve_joins_pkfk(
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
        return Vec::new(); // Should not happen -- validated at define time
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
