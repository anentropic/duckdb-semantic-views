use std::collections::{HashMap, HashSet};

use crate::model::{Cardinality, SemanticViewDefinition};

use super::facts::collect_derived_metric_source_tables;
use super::types::ExpandError;

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
pub(super) fn check_fan_traps(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
) -> Result<(), ExpandError> {
    if def.joins.is_empty() {
        return Ok(());
    }

    let Ok(graph) = crate::graph::RelationshipGraph::from_definition(def) else {
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
        // Phase 47: Skip fan trap check for semi-additive metrics.
        // The ROW_NUMBER CTE inherently handles fan-out by selecting one row
        // per partition, making fan trap detection unnecessary.
        if !met.non_additive_by.is_empty() {
            continue;
        }

        // Phase 48: Skip fan trap check for window function metrics.
        // Window metrics operate on pre-aggregated CTE results, so fan-out
        // is handled by the inner aggregation step.
        if met.is_window() {
            continue;
        }

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
                let dim_ancestor_set: std::collections::HashSet<&String> =
                    dim_ancestors.iter().collect();
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

/// Validate that all tables referenced by a fact query are on the same
/// root-to-leaf path in the relationship tree.
///
/// Snowflake constraint: "all facts and dimensions used in the query must be
/// defined in the same logical table." For our multi-table model, this means
/// all `source_table` aliases must be reachable through a single linear path
/// (no fan-out — each pair must have an ancestor/descendant relationship).
///
/// Skips validation when there is only one unique table (trivially valid)
/// or when there are no joins (single-table view).
#[allow(clippy::result_large_err)]
pub(super) fn validate_fact_table_path(
    view_name: &str,
    def: &SemanticViewDefinition,
    fact_tables: &[String],
    dim_tables: &[String],
) -> Result<(), ExpandError> {
    if def.joins.is_empty() {
        return Ok(());
    }

    // Collect all unique table aliases (case-insensitive)
    let mut all_tables: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for t in fact_tables.iter().chain(dim_tables.iter()) {
        let lower = t.to_ascii_lowercase();
        if seen.insert(lower.clone()) {
            all_tables.push(lower);
        }
    }

    if all_tables.len() <= 1 {
        return Ok(()); // Single table or empty — trivially valid
    }

    let Ok(graph) = crate::graph::RelationshipGraph::from_definition(def) else {
        return Ok(()); // Graph was validated at define time
    };

    // Build parent map from reverse adjacency
    let mut parent_map: HashMap<String, String> = HashMap::new();
    for (child, parents) in &graph.reverse {
        if let Some(parent) = parents.first() {
            parent_map.insert(child.clone(), parent.clone());
        }
    }

    // For each pair, verify one is an ancestor of the other
    for i in 0..all_tables.len() {
        for j in (i + 1)..all_tables.len() {
            let a = &all_tables[i];
            let b = &all_tables[j];
            let a_ancestors = ancestors_to_root(a, &parent_map);
            let b_ancestors = ancestors_to_root(b, &parent_map);
            let a_is_ancestor_of_b = b_ancestors.iter().any(|x| x == a);
            let b_is_ancestor_of_a = a_ancestors.iter().any(|x| x == b);
            if !a_is_ancestor_of_b && !b_is_ancestor_of_a {
                return Err(ExpandError::FactPathViolation {
                    view_name: view_name.to_string(),
                    table_a: a.clone(),
                    table_b: b.clone(),
                });
            }
        }
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
    use crate::model::{Cardinality, NullsOrder, SortOrder, WindowSpec};

    #[test]
    fn test_ancestors_to_root_at_root() {
        let parent_map: HashMap<String, String> = HashMap::new();
        let result = ancestors_to_root("root", &parent_map);
        assert_eq!(result, vec!["root"]);
    }

    #[test]
    fn test_ancestors_to_root_single_parent() {
        let mut parent_map: HashMap<String, String> = HashMap::new();
        parent_map.insert("child".to_string(), "root".to_string());
        let result = ancestors_to_root("child", &parent_map);
        assert_eq!(result, vec!["child", "root"]);
    }

    #[test]
    fn test_ancestors_to_root_multi_level() {
        let mut parent_map: HashMap<String, String> = HashMap::new();
        parent_map.insert("leaf".to_string(), "mid".to_string());
        parent_map.insert("mid".to_string(), "root".to_string());
        let result = ancestors_to_root("leaf", &parent_map);
        assert_eq!(result, vec!["leaf", "mid", "root"]);
    }

    #[test]
    fn test_check_fan_traps_no_joins_ok() {
        let def = orders_view();
        let resolved_dims: Vec<&_> = def.dimensions.iter().collect();
        let resolved_mets: Vec<&_> = def.metrics.iter().collect();
        let result = check_fan_traps("test", &def, &resolved_dims, &resolved_mets);
        assert!(result.is_ok(), "No joins should be OK");
    }

    #[test]
    fn test_check_fan_traps_many_to_one_safe_direction() {
        // orders (root) -> customers via FK. Metric on orders, dim on customers.
        // This is ManyToOne from orders->customers (orders has FK to customers).
        // Walking from orders metric to customers dim goes forward on the FK edge = safe.
        let def = minimal_def("orders", "cust_name", "name", "total", "sum(amount)")
            .with_table("orders", "orders", &["id"])
            .with_table("customers", "customers", &["id"])
            .with_dimension("cust_name", "name", Some("customers"))
            .with_metric("total", "sum(amount)", Some("orders"))
            .with_pkfk_join(
                "orders_customers",
                "orders",
                "customers",
                &["customer_id"],
                &["id"],
            );
        // Remove the initial minimal_def dims/metrics (they have no source_table)
        let mut def = def;
        def.dimensions
            .retain(|d| d.source_table.is_some() || d.name == "cust_name");
        def.metrics
            .retain(|m| m.source_table.is_some() || m.name == "total");
        let resolved_dims: Vec<&_> = def.dimensions.iter().collect();
        let resolved_mets: Vec<&_> = def.metrics.iter().collect();
        let result = check_fan_traps("test", &def, &resolved_dims, &resolved_mets);
        assert!(
            result.is_ok(),
            "ManyToOne forward direction should be safe, got: {result:?}"
        );
    }

    #[test]
    fn test_check_fan_traps_many_to_one_fan_out() {
        // orders (root) -> line_items. line_items has FK to orders (ManyToOne from line_items->orders).
        // Metric on orders, dimension on line_items.
        // Walking from orders to line_items reverses the ManyToOne edge = fan-out.
        let def = minimal_def("orders", "item_name", "name", "total", "sum(amount)")
            .with_table("orders", "orders", &["id"])
            .with_table("line_items", "line_items", &["id"])
            .with_dimension("item_name", "name", Some("line_items"))
            .with_metric("total", "sum(amount)", Some("orders"))
            .with_pkfk_join(
                "items_to_orders",
                "line_items",
                "orders",
                &["order_id"],
                &["id"],
            );
        let mut def = def;
        def.dimensions.retain(|d| d.source_table.is_some());
        def.metrics.retain(|m| m.source_table.is_some());
        let resolved_dims: Vec<&_> = def.dimensions.iter().collect();
        let resolved_mets: Vec<&_> = def.metrics.iter().collect();
        let result = check_fan_traps("test", &def, &resolved_dims, &resolved_mets);
        assert!(result.is_err(), "Should detect fan-out");
        if let Err(ExpandError::FanTrap { metric_name, .. }) = &result {
            assert_eq!(metric_name, "total");
        } else {
            panic!("Expected FanTrap error, got: {result:?}");
        }
    }

    #[test]
    fn test_check_fan_traps_one_to_one_safe() {
        // Same fan-out scenario as above, but with OneToOne cardinality.
        let def = minimal_def("orders", "item_name", "name", "total", "sum(amount)")
            .with_table("orders", "orders", &["id"])
            .with_table("line_items", "line_items", &["id"])
            .with_dimension("item_name", "name", Some("line_items"))
            .with_metric("total", "sum(amount)", Some("orders"))
            .with_pkfk_join(
                "items_to_orders",
                "line_items",
                "orders",
                &["order_id"],
                &["id"],
            );
        let mut def = def;
        def.dimensions.retain(|d| d.source_table.is_some());
        def.metrics.retain(|m| m.source_table.is_some());
        // Mutate the join to OneToOne
        def.joins.last_mut().unwrap().cardinality = Cardinality::OneToOne;
        let resolved_dims: Vec<&_> = def.dimensions.iter().collect();
        let resolved_mets: Vec<&_> = def.metrics.iter().collect();
        let result = check_fan_traps("test", &def, &resolved_dims, &resolved_mets);
        assert!(
            result.is_ok(),
            "OneToOne should be safe regardless of direction, got: {result:?}"
        );
    }

    #[test]
    fn test_check_fan_traps_skips_semi_additive() {
        // Same fan-out scenario, but the metric has non_additive_by set.
        let def = minimal_def("orders", "item_name", "name", "total", "sum(amount)")
            .with_table("orders", "orders", &["id"])
            .with_table("line_items", "line_items", &["id"])
            .with_dimension("item_name", "name", Some("line_items"))
            .with_metric("total_sourced", "sum(amount)", Some("orders"))
            .with_non_additive_by(
                "total_sourced",
                &[("item_name", SortOrder::Desc, NullsOrder::First)],
            )
            .with_pkfk_join(
                "items_to_orders",
                "line_items",
                "orders",
                &["order_id"],
                &["id"],
            );
        let mut def = def;
        def.dimensions.retain(|d| d.source_table.is_some());
        def.metrics.retain(|m| m.source_table.is_some());
        let resolved_dims: Vec<&_> = def.dimensions.iter().collect();
        let resolved_mets: Vec<&_> = def.metrics.iter().collect();
        let result = check_fan_traps("test", &def, &resolved_dims, &resolved_mets);
        assert!(
            result.is_ok(),
            "Semi-additive metrics should skip fan trap check, got: {result:?}"
        );
    }

    #[test]
    fn test_check_fan_traps_skips_window() {
        // Same fan-out scenario, but the metric has window_spec set.
        let def = minimal_def("orders", "item_name", "name", "total", "sum(amount)")
            .with_table("orders", "orders", &["id"])
            .with_table("line_items", "line_items", &["id"])
            .with_dimension("item_name", "name", Some("line_items"))
            .with_metric("total_sourced", "sum(amount)", Some("orders"))
            .with_window_spec(
                "total_sourced",
                WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_sourced".to_string(),
                    ..Default::default()
                },
            )
            .with_pkfk_join(
                "items_to_orders",
                "line_items",
                "orders",
                &["order_id"],
                &["id"],
            );
        let mut def = def;
        def.dimensions.retain(|d| d.source_table.is_some());
        def.metrics.retain(|m| m.source_table.is_some());
        let resolved_dims: Vec<&_> = def.dimensions.iter().collect();
        let resolved_mets: Vec<&_> = def.metrics.iter().collect();
        let result = check_fan_traps("test", &def, &resolved_dims, &resolved_mets);
        assert!(
            result.is_ok(),
            "Window metrics should skip fan trap check, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_fact_table_path_single_table_ok() {
        let def = orders_view().with_table("orders", "orders", &["id"]);
        let fact_tables = vec!["orders".to_string()];
        let dim_tables = vec!["orders".to_string()];
        let result = validate_fact_table_path("test", &def, &fact_tables, &dim_tables);
        assert!(result.is_ok(), "Single table should be OK");
    }

    #[test]
    fn test_validate_fact_table_path_ancestor_descendant_ok() {
        let def = orders_view()
            .with_table("orders", "orders", &["id"])
            .with_table("customers", "customers", &["id"])
            .with_pkfk_join(
                "orders_customers",
                "orders",
                "customers",
                &["customer_id"],
                &["id"],
            );
        let fact_tables = vec!["orders".to_string()];
        let dim_tables = vec!["customers".to_string()];
        let result = validate_fact_table_path("test", &def, &fact_tables, &dim_tables);
        assert!(
            result.is_ok(),
            "Ancestor-descendant should be OK, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_fact_table_path_divergent_tables_err() {
        // orders (root) -> customers and orders -> products (siblings)
        let def = orders_view()
            .with_table("orders", "orders", &["id"])
            .with_table("customers", "customers", &["id"])
            .with_table("products", "products", &["id"])
            .with_pkfk_join(
                "orders_customers",
                "orders",
                "customers",
                &["customer_id"],
                &["id"],
            )
            .with_pkfk_join(
                "orders_products",
                "orders",
                "products",
                &["product_id"],
                &["id"],
            );
        // customers and products are siblings (neither ancestor of the other)
        let fact_tables = vec!["customers".to_string()];
        let dim_tables = vec!["products".to_string()];
        let result = validate_fact_table_path("test", &def, &fact_tables, &dim_tables);
        assert!(result.is_err(), "Divergent tables should fail");
        if let Err(ExpandError::FactPathViolation {
            table_a, table_b, ..
        }) = &result
        {
            assert!(
                (table_a == "customers" && table_b == "products")
                    || (table_a == "products" && table_b == "customers"),
                "Should identify the divergent tables"
            );
        } else {
            panic!("Expected FactPathViolation, got: {result:?}");
        }
    }

    #[test]
    fn test_validate_fact_table_path_no_joins_ok() {
        let def = orders_view();
        let fact_tables = vec!["orders".to_string()];
        let dim_tables = vec!["customers".to_string()];
        let result = validate_fact_table_path("test", &def, &fact_tables, &dim_tables);
        assert!(result.is_ok(), "No joins should be OK");
    }
}
