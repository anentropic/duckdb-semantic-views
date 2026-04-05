use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogState;
use crate::expand::{ancestors_to_root, collect_derived_metric_source_tables};
use crate::graph::RelationshipGraph;
use crate::model::{Cardinality, Dimension, SemanticViewDefinition};
use crate::util::suggest_closest;

/// A single row in the SHOW SEMANTIC DIMENSIONS FOR METRIC output.
///
/// 4 Snowflake-aligned columns: table_name, name, data_type, required.
/// The `required` column is a constant FALSE (BOOLEAN), emitted separately.
struct ShowDimForMetricRow {
    table_name: String,
    name: String,
    data_type: String,
}

/// Bind-time data: pre-collected dimension rows (fan-trap-filtered).
pub struct ShowDimsForMetricBindData {
    rows: Vec<ShowDimForMetricRow>,
}

// SAFETY: all fields are owned `Vec<ShowDimForMetricRow>` (String fields only), which is `Send + Sync`.
unsafe impl Send for ShowDimsForMetricBindData {}
unsafe impl Sync for ShowDimsForMetricBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct ShowDimsForMetricInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ShowDimsForMetricInitData {}
unsafe impl Sync for ShowDimsForMetricInitData {}

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

/// Table function: `SHOW SEMANTIC DIMENSIONS IN view_name FOR METRIC metric_name`
///
/// Takes two VARCHAR parameters (view name, metric name).
/// Returns only dimensions that are fan-trap-safe for the given metric.
///
/// Output schema: table_name VARCHAR, name VARCHAR, data_type VARCHAR, required BOOLEAN.
pub struct ShowDimensionsForMetricVTab;

impl VTab for ShowDimensionsForMetricVTab {
    type BindData = ShowDimsForMetricBindData;
    type InitData = ShowDimsForMetricInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Declare 4-column output schema: table_name, name, data_type, required (BOOLEAN)
        bind.add_result_column(
            "table_name",
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        );
        bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column("data_type", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column("required", LogicalTypeHandle::from(LogicalTypeId::Boolean));

        let view_name = bind.get_parameter(0).to_string();
        let metric_name = bind.get_parameter(1).to_string();

        let state_ptr = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state_ptr).read().expect("catalog RwLock poisoned") };

        let json = guard.get(&view_name).ok_or_else(|| {
            let available: Vec<String> = guard.keys().cloned().collect();
            if let Some(suggestion) = suggest_closest(&view_name, &available) {
                format!(
                    "semantic view '{}' does not exist. Did you mean '{}'?",
                    view_name, suggestion
                )
            } else {
                format!("semantic view '{}' does not exist", view_name)
            }
        })?;

        let def = SemanticViewDefinition::from_json(&view_name, json)?;

        // Find the metric (case-insensitive)
        let metric_lower = metric_name.to_ascii_lowercase();
        let met = def
            .metrics
            .iter()
            .find(|m| m.name.to_ascii_lowercase() == metric_lower)
            .ok_or_else(|| {
                let available: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();
                if let Some(suggestion) = suggest_closest(&metric_name, &available) {
                    format!(
                        "metric '{}' not found in semantic view '{}'. Did you mean '{}'?",
                        metric_name, view_name, suggestion
                    )
                } else {
                    format!(
                        "metric '{}' not found in semantic view '{}'",
                        metric_name, view_name
                    )
                }
            })?;

        // Get metric source tables
        let met_tables: Vec<String> = if let Some(ref st) = met.source_table {
            vec![st.to_ascii_lowercase()]
        } else {
            // Derived metric: resolve source tables transitively
            collect_derived_metric_source_tables(met, &def.metrics)
                .into_iter()
                .map(|s| s.to_ascii_lowercase())
                .collect()
        };

        let alias_map = def.alias_to_table_map();

        // If no joins, all dimensions are reachable (single-table view)
        let rows = if def.joins.is_empty() {
            def.dimensions
                .iter()
                .map(|d| {
                    let table_name = d
                        .source_table
                        .as_ref()
                        .and_then(|a| alias_map.get(a).cloned())
                        .unwrap_or_default();
                    ShowDimForMetricRow {
                        table_name,
                        name: d.name.clone(),
                        data_type: d.output_type.clone().unwrap_or_default(),
                    }
                })
                .collect()
        } else {
            // Build graph structures for fan-trap checking
            let graph = RelationshipGraph::from_definition(&def)
                .map_err(|e| format!("graph error: {e}"))?;

            // Build parent map: each non-root node's parent in the tree
            let mut parent_map: HashMap<String, String> = HashMap::new();
            for (child, parents) in &graph.reverse {
                if let Some(parent) = parents.first() {
                    parent_map.insert(child.clone(), parent.clone());
                }
            }

            // Build cardinality map: (from_alias_lower, to_alias_lower) -> Cardinality
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

            // Filter dimensions: keep only those reachable without fan-out
            def.dimensions
                .iter()
                .filter(|d| {
                    is_dimension_reachable_for_metric(d, &met_tables, &parent_map, &card_map)
                })
                .map(|d| {
                    let table_name = d
                        .source_table
                        .as_ref()
                        .and_then(|a| alias_map.get(a).cloned())
                        .unwrap_or_default();
                    ShowDimForMetricRow {
                        table_name,
                        name: d.name.clone(),
                        data_type: d.output_type.clone().unwrap_or_default(),
                    }
                })
                .collect()
        };

        let mut rows: Vec<ShowDimForMetricRow> = rows;
        rows.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(ShowDimsForMetricBindData { rows })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowDimsForMetricInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        if init_data.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let bind_data = func.get_bind_data();
        let n = bind_data.rows.len();

        let table_vec = output.flat_vector(0);
        let name_vec = output.flat_vector(1);
        let type_vec = output.flat_vector(2);
        let mut req_vec = output.flat_vector(3);

        for (i, row) in bind_data.rows.iter().enumerate() {
            table_vec.insert(i, row.table_name.as_str());
            name_vec.insert(i, row.name.as_str());
            type_vec.insert(i, row.data_type.as_str());
        }

        // Write boolean column: constant FALSE for all rows
        let req_slice = req_vec.as_mut_slice::<bool>();
        for i in 0..n {
            req_slice[i] = false;
        }

        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        ])
    }
}
