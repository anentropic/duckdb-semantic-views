//! `SHOW SEMANTIC DIMENSIONS ... FOR METRIC` dispatcher (ST-2).
//!
//! A two-argument TF `(view_name, metric_name)` returning 3 VARCHAR + 1 BOOL
//! per row `(table_name, name, data_type, required)`. It doesn't fit the
//! shared 8-column `EntityRow` (extra metric arg, fan-trap reachability
//! filter, trailing BOOL column), so it keeps its own body — but routes the
//! scaffold through [`crate::ddl::read_ffi::run_dispatcher`] like every other
//! read-side function.

#![cfg(feature = "extension")]

use std::collections::{HashMap, HashSet};

use crate::catalog::CatalogReader;
use crate::ddl::read_ffi::{
    probe_catalog_table_present, read_str_arg, run_dispatcher, serialize_varchar_bool_rows,
    BorrowedConnection,
};
use crate::expand::{ancestors_to_root, collect_derived_metric_source_tables};
use crate::graph::RelationshipGraph;
use crate::model::{Cardinality, Dimension, SemanticViewDefinition};
use crate::util::suggest_closest;

/// # Safety
///
/// `conn` is a borrowed handle; both name pointers must point to valid UTF-8
/// bytes of the matching length. The caller releases the returned buffer via
/// `sv_free_buffer`.
#[no_mangle]
pub unsafe extern "C" fn sv_show_semantic_dimensions_for_metric_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    view_name_ptr: *const u8,
    view_name_len: usize,
    metric_name_ptr: *const u8,
    metric_name_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    run_dispatcher(
        conn,
        out_ptr,
        out_len,
        error_buf,
        error_buf_len,
        "sv_show_semantic_dimensions_for_metric_bind_rust",
        |borrowed| unsafe {
            show_dims_for_metric(
                borrowed,
                view_name_ptr,
                view_name_len,
                metric_name_ptr,
                metric_name_len,
            )
        },
    )
}

/// Body for [`sv_show_semantic_dimensions_for_metric_bind_rust`]: resolve the
/// view + metric, then emit each dimension with a `required` flag, filtered to
/// those reachable from the metric's source table(s) without a fan trap.
///
/// # Safety
///
/// `view_name_ptr` / `metric_name_ptr` must each be null or point to the
/// matching number of readable bytes.
unsafe fn show_dims_for_metric(
    borrowed: &BorrowedConnection,
    view_name_ptr: *const u8,
    view_name_len: usize,
    metric_name_ptr: *const u8,
    metric_name_len: usize,
) -> Result<Vec<u8>, String> {
    let view_name = read_str_arg(view_name_ptr, view_name_len, "view name")?;
    let metric_name = read_str_arg(metric_name_ptr, metric_name_len, "metric name")?;

    let present = probe_catalog_table_present(borrowed);
    let reader = CatalogReader::new(borrowed, present);
    let json = match reader.lookup(&view_name)? {
        Some(j) => j,
        None => {
            let available = reader.list_names().unwrap_or_default();
            let not_found = crate::catalog::view_not_found_msg(&view_name);
            return Err(match suggest_closest(&view_name, &available) {
                Some(suggestion) => format!("{not_found}. Did you mean '{suggestion}'?"),
                None => not_found,
            });
        }
    };
    let def = SemanticViewDefinition::from_json(&view_name, &json)?;

    let metric_lower = metric_name.to_ascii_lowercase();
    let met = match def
        .metrics
        .iter()
        .find(|m| m.name.to_ascii_lowercase() == metric_lower)
    {
        Some(m) => m,
        None => {
            let available: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();
            return Err(match suggest_closest(&metric_name, &available) {
                Some(suggestion) => format!(
                    "metric '{metric_name}' not found in semantic view '{view_name}'. \
                     Did you mean '{suggestion}'?"
                ),
                None => format!("metric '{metric_name}' not found in semantic view '{view_name}'"),
            });
        }
    };

    let required_dim_names: HashSet<String> = if let Some(ref ws) = met.window_spec {
        let mut names = HashSet::new();
        for dn in &ws.excluding_dims {
            names.insert(dn.to_ascii_lowercase());
        }
        for dn in &ws.partition_dims {
            names.insert(dn.to_ascii_lowercase());
        }
        for ob in &ws.order_by {
            names.insert(ob.expr.to_ascii_lowercase());
        }
        names
    } else {
        HashSet::new()
    };

    let met_tables: Vec<String> = if let Some(ref st) = met.source_table {
        vec![st.to_ascii_lowercase()]
    } else {
        collect_derived_metric_source_tables(met, &def.metrics)
            .into_iter()
            .map(|s| s.to_ascii_lowercase())
            .collect()
    };

    let alias_map = def.alias_to_table_map();
    let mut rows: Vec<(Vec<String>, bool)> = if def.joins.is_empty() {
        def.dimensions
            .iter()
            .map(|d| {
                let table_name = d
                    .source_table
                    .as_ref()
                    .and_then(|a| alias_map.get(a).cloned())
                    .unwrap_or_default();
                (
                    vec![
                        table_name,
                        d.name.clone(),
                        d.output_type.clone().unwrap_or_default(),
                    ],
                    required_dim_names.contains(&d.name.to_ascii_lowercase()),
                )
            })
            .collect()
    } else {
        let graph =
            RelationshipGraph::from_definition(&def).map_err(|e| format!("graph error: {e}"))?;
        let mut parent_map: HashMap<String, String> = HashMap::new();
        for (child, parents) in &graph.reverse {
            if let Some(parent) = parents.first() {
                parent_map.insert(child.clone(), parent.clone());
            }
        }
        let card_map: HashMap<(String, String), Cardinality> = def
            .joins
            .iter()
            .filter(|j| !j.fk_columns.is_empty())
            .map(|j| {
                (
                    (
                        j.from_alias.to_ascii_lowercase(),
                        j.table.to_ascii_lowercase(),
                    ),
                    j.cardinality,
                )
            })
            .collect();
        def.dimensions
            .iter()
            .filter(|d| is_dimension_reachable_for_metric(d, &met_tables, &parent_map, &card_map))
            .map(|d| {
                let table_name = d
                    .source_table
                    .as_ref()
                    .and_then(|a| alias_map.get(a).cloned())
                    .unwrap_or_default();
                (
                    vec![
                        table_name,
                        d.name.clone(),
                        d.output_type.clone().unwrap_or_default(),
                    ],
                    required_dim_names.contains(&d.name.to_ascii_lowercase()),
                )
            })
            .collect()
    };
    rows.sort_by(|a, b| a.0[1].cmp(&b.0[1]));
    Ok(serialize_varchar_bool_rows(&rows))
}

/// Check if a dimension is reachable from a given set of metric source tables
/// without causing a fan trap.
///
/// A dimension is reachable if, for at least one metric source table, the path
/// from the metric table to the dimension table does not traverse any edge in
/// the fan-out direction (i.e., from the "one" side to the "many" side).
///
/// Fan-out direction: for edge `(from_alias, to_alias)` with `ManyToOne`,
/// from->to is safe (forward), to->from is fan-out (reverse).
fn is_dimension_reachable_for_metric(
    dim: &Dimension,
    met_tables: &[String],
    parent_map: &HashMap<String, String>,
    card_map: &HashMap<(String, String), Cardinality>,
) -> bool {
    // Base table dimension (no source_table): always reachable
    let Some(ref dim_table_raw) = dim.source_table else {
        return true;
    };
    let dim_table = dim_table_raw.to_ascii_lowercase();

    for met_table in met_tables {
        if *met_table == dim_table {
            return true; // Same table, no fan-out possible
        }

        let met_ancestors = ancestors_to_root(met_table, parent_map);
        let dim_ancestors = ancestors_to_root(&dim_table, parent_map);

        // Find the lowest common ancestor (LCA)
        let dim_ancestor_set: HashSet<&String> = dim_ancestors.iter().collect();
        let lca = met_ancestors
            .iter()
            .find(|a| dim_ancestor_set.contains(a))
            .cloned();

        let Some(lca) = lca else {
            continue; // No common ancestor (disconnected -- shouldn't happen in valid tree)
        };

        let mut fan_out = false;

        // Check path UP from met_table to LCA.
        // Walking current -> parent at each step.
        let mut current = met_table.clone();
        while current != lca {
            let Some(parent) = parent_map.get(&current) else {
                break;
            };

            // The card_map stores edges as (from_alias, to_alias) where
            // from_alias has the FK pointing to to_alias.
            //
            // Walking current -> parent:
            // - If edge is (current, parent): forward direction. ManyToOne forward = safe.
            // - If edge is (parent, current): reverse direction. ManyToOne reverse = fan-out.
            if card_map.get(&(current.clone(), parent.clone())).is_some() {
                // Edge is current->parent (current has FK to parent).
                // Walking current->parent = forward direction = always safe.
            } else if let Some(&card) = card_map.get(&(parent.clone(), current.clone())) {
                // Edge is parent->current (parent has FK to current).
                // Walking current->parent = reverse direction.
                // ManyToOne reverse = fan-out (going from "one" side to "many" side).
                if card == Cardinality::ManyToOne {
                    fan_out = true;
                    break;
                }
            }
            current = parent.clone();
        }

        if fan_out {
            // This metric source table path has fan-out for this dimension.
            // Try next metric source table (for derived metrics with multiple sources).
            continue;
        }

        // Check path DOWN from LCA to dim_table.
        // Build path from LCA to dim_table using dim_ancestors.
        // dim_ancestors is [dim_table, parent, ..., root].
        // Find LCA position and reverse the sub-path.
        if let Some(lca_pos) = dim_ancestors.iter().position(|a| *a == lca) {
            let path_down: Vec<String> = dim_ancestors[..=lca_pos].iter().rev().cloned().collect();
            for window in path_down.windows(2) {
                let a = &window[0]; // closer to LCA
                let b = &window[1]; // closer to dim_table
                                    // Walking a -> b (downward in the tree, away from root)
                if card_map.get(&(a.clone(), b.clone())).is_some() {
                    // Edge is a->b (a has FK to b). Walking forward = safe.
                } else if let Some(&card) = card_map.get(&(b.clone(), a.clone())) {
                    // Edge is b->a (b has FK to a). Walking a->b = reverse direction.
                    // ManyToOne reverse = fan-out.
                    if card == Cardinality::ManyToOne {
                        fan_out = true;
                        break;
                    }
                }
            }
        }

        if !fan_out {
            // Found a valid (no fan-out) path from this metric source to the dimension.
            return true;
        }
        // else: try next metric source table
    }

    // No metric source table found a valid path -- dimension not reachable.
    // Edge case: if met_tables is empty (derived metric with no source tables),
    // the dimension is not reachable.
    false
}
