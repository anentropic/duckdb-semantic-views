//! CTE-based expansion for semi-additive metrics.
//!
//! When any requested metric has a non-empty `non_additive_by` field AND at least
//! one of its NA dims is NOT in the queried dimensions, the expansion wraps the
//! base query in a CTE that uses `ROW_NUMBER()` to select snapshot rows before
//! aggregation.
//!
//! **Effectively-regular classification (Snowflake semantics):**
//! A semi-additive metric whose ALL NA dims are present in the queried dimensions
//! is treated as "effectively regular" for that query -- no CTE needed, standard
//! aggregation applies. This matches Snowflake's behavior: "When the non-additive
//! dimension is included in the query, the metric is calculated as a standard
//! additive metric."
//!
//! The strategy for active semi-additive metrics:
//! - Single CTE (`__sv_snapshot`) containing all raw columns needed
//! - `ROW_NUMBER() OVER (PARTITION BY non-NA-dims ORDER BY NA-dims) AS __sv_rn`
//! - Outer SELECT: regular/effectively-regular metrics use plain aggregation,
//!   active semi-additive metrics use `SUM(CASE WHEN __sv_rn = 1 THEN raw_val END)`
//!
//! When multiple semi-additive metrics have different NON ADDITIVE BY dimensions,
//! each gets its own `__sv_rn_N` column in the CTE.

use std::collections::{HashMap, HashSet};

use crate::model::{Metric, NonAdditiveDim, NullsOrder, SemanticViewDefinition, SortOrder};
use crate::util::replace_word_boundary;

use super::join_resolver::{resolve_joins_pkfk, synthesize_on_clause, synthesize_on_clause_scoped};
use super::resolution::{quote_ident, quote_table_ref};
use super::types::ExpandError;

/// Generate CTE-based expansion SQL for queries containing semi-additive metrics.
///
/// Called from `expand()` when `has_active_semi_additive` is true.
/// Receives already-resolved dims, metrics, expressions, and scoped aliases.
#[allow(
    clippy::too_many_lines,
    clippy::result_large_err,
    clippy::unnecessary_wraps
)]
pub(super) fn expand_semi_additive(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&Metric],
    resolved_exprs: &HashMap<String, String>,
    dim_scoped_aliases: &[Option<String>],
) -> Result<String, ExpandError> {
    let _ = view_name; // reserved for future error messages
    let mut sql = String::with_capacity(512);

    // Build set of queried dimension names for classification
    let queried_dim_names: HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();

    // Classify each metric as active semi-additive
    let is_active_semi = |met: &Metric| -> bool {
        !met.non_additive_by.is_empty()
            && met
                .non_additive_by
                .iter()
                .any(|na| !queried_dim_names.contains(&na.dimension.to_ascii_lowercase()))
    };

    // 1. Identify distinct NON ADDITIVE BY dimension sets for ACTIVE metrics only.
    let na_groups = collect_na_groups(resolved_mets, &queried_dim_names);

    // === CTE ===
    sql.push_str("WITH __sv_snapshot AS (\n    SELECT\n");

    let mut cte_select_items: Vec<String> = Vec::new();

    // Dimension columns in CTE
    for (i, dim) in resolved_dims.iter().enumerate() {
        let mut base_expr = dim.expr.clone();
        if let Some(ref scoped) = dim_scoped_aliases[i] {
            if let Some(ref st) = dim.source_table {
                base_expr = replace_word_boundary(&base_expr, st, scoped);
            }
        }
        let final_expr = if let Some(ref type_str) = dim.output_type {
            format!("CAST({base_expr} AS {type_str})")
        } else {
            base_expr
        };
        cte_select_items.push(format!(
            "        {} AS {}",
            final_expr,
            quote_ident(&dim.name)
        ));
    }

    // Metric raw columns in CTE -- extract inner expression from aggregate
    for (met_idx, met) in resolved_mets.iter().enumerate() {
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| met.expr.clone());

        if is_active_semi(met) {
            // Active semi-additive: extract inner expression from aggregate
            let inner =
                extract_aggregate_inner(&resolved_expr).unwrap_or_else(|| resolved_expr.clone());
            cte_select_items.push(format!("        {inner} AS \"__sv_semi_{met_idx}\""));
        } else {
            // Regular or effectively-regular: include raw column reference
            let inner =
                extract_aggregate_inner(&resolved_expr).unwrap_or_else(|| resolved_expr.clone());
            cte_select_items.push(format!("        {inner} AS \"__sv_reg_{met_idx}\""));
        }
    }

    // ROW_NUMBER columns (one per active NA group)
    for (group_idx, group) in na_groups.iter().enumerate() {
        let rn_alias = if na_groups.len() == 1 {
            "__sv_rn".to_string()
        } else {
            format!("__sv_rn_{}", group_idx + 1)
        };

        // PARTITION BY: all queried dims (NA dims not in query are not in
        // resolved_dims, so this naturally partitions by all queried dims)
        let na_dim_names: Vec<String> = group
            .na_dims
            .iter()
            .map(|nd| nd.dimension.to_ascii_lowercase())
            .collect();

        let partition_dims: Vec<String> = resolved_dims
            .iter()
            .filter(|d| !na_dim_names.contains(&d.name.to_ascii_lowercase()))
            .map(|d| quote_ident(&d.name))
            .collect();

        let partition_clause = if partition_dims.is_empty() {
            String::new()
        } else {
            format!("PARTITION BY {}", partition_dims.join(", "))
        };

        // ORDER BY: the NA dims with their specified sort order.
        // Use the dimension's raw expression when the NA dim is NOT in the
        // queried dimensions (it won't have a CTE alias).
        let order_items: Vec<String> = group
            .na_dims
            .iter()
            .map(|nd| {
                // Try to find the dimension in resolved (queried) dims first
                let dim_expr = resolved_dims
                    .iter()
                    .find(|d| d.name.eq_ignore_ascii_case(&nd.dimension))
                    .map_or_else(
                        || {
                            // NA dim not in queried dims -- find it in the view definition
                            // and use its raw expression
                            def.dimensions
                                .iter()
                                .find(|d| d.name.eq_ignore_ascii_case(&nd.dimension))
                                .map_or_else(|| quote_ident(&nd.dimension), |d| d.expr.clone())
                        },
                        |d| quote_ident(&d.name),
                    );
                let dir = match nd.order {
                    SortOrder::Asc => "ASC",
                    SortOrder::Desc => "DESC",
                };
                let nulls = match nd.nulls {
                    NullsOrder::First => "NULLS FIRST",
                    NullsOrder::Last => "NULLS LAST",
                };
                format!("{dim_expr} {dir} {nulls}")
            })
            .collect();

        let order_clause = order_items.join(", ");

        let window_spec = if partition_clause.is_empty() {
            format!("ORDER BY {order_clause}")
        } else {
            format!("{partition_clause} ORDER BY {order_clause}")
        };

        cte_select_items.push(format!(
            "        ROW_NUMBER() OVER ({window_spec}) AS \"{rn_alias}\""
        ));
    }

    sql.push_str(&cte_select_items.join(",\n"));

    // CTE FROM clause (same logic as expand())
    sql.push_str("\n    FROM ");
    sql.push_str(&quote_table_ref(&def.base_table));
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // CTE JOINs
    let ordered_aliases = resolve_joins_pkfk(def, resolved_dims, resolved_mets);
    for alias in &ordered_aliases {
        if let Some(sep_pos) = alias.find("__") {
            let rel_name = &alias[sep_pos + 2..];
            let Some(join) = def.joins.iter().find(|j| {
                j.name
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(rel_name))
            }) else {
                continue;
            };
            let bare_alias = &alias[..sep_pos];
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == bare_alias);
            let physical_table = table_ref.map_or(bare_alias, |t| t.table.as_str());
            sql.push_str("\n    LEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause_scoped(join, &def.tables, alias));
        } else {
            let Some(join) = def.joins.iter().find(|j| {
                j.table.to_ascii_lowercase() == *alias
                    || j.from_alias.to_ascii_lowercase() == *alias
            }) else {
                continue;
            };
            let table_ref = def
                .tables
                .iter()
                .find(|t| t.alias.to_ascii_lowercase() == *alias);
            let physical_table = table_ref.map_or(alias.as_str(), |t| t.table.as_str());
            sql.push_str("\n    LEFT JOIN ");
            sql.push_str(&quote_table_ref(physical_table));
            sql.push_str(" AS ");
            sql.push_str(&quote_ident(alias));
            sql.push_str(" ON ");
            sql.push_str(&synthesize_on_clause(join, &def.tables));
        }
    }

    sql.push_str("\n)\n");

    // === Outer SELECT ===
    sql.push_str("SELECT\n");

    let mut outer_select_items: Vec<String> = Vec::new();

    // Dimension columns: reference CTE aliases
    for dim in resolved_dims {
        outer_select_items.push(format!(
            "    {} AS {}",
            quote_ident(&dim.name),
            quote_ident(&dim.name)
        ));
    }

    // Metric columns
    for (met_idx, met) in resolved_mets.iter().enumerate() {
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| met.expr.clone());
        let agg_func = extract_aggregate_func(&resolved_expr).unwrap_or("SUM");

        if is_active_semi(met) {
            // Active semi-additive: wrap in CASE WHEN __sv_rn = 1
            let rn_col = get_rn_column_for_metric(met_idx, &na_groups);
            let final_expr =
                format!("{agg_func}(CASE WHEN \"{rn_col}\" = 1 THEN \"__sv_semi_{met_idx}\" END)");
            let cast_expr = if let Some(ref type_str) = met.output_type {
                format!("CAST({final_expr} AS {type_str})")
            } else {
                final_expr
            };
            outer_select_items.push(format!("    {} AS {}", cast_expr, quote_ident(&met.name)));
        } else {
            // Regular or effectively-regular: aggregate normally over all rows
            let final_expr = format!("{agg_func}(\"__sv_reg_{met_idx}\")");
            let cast_expr = if let Some(ref type_str) = met.output_type {
                format!("CAST({final_expr} AS {type_str})")
            } else {
                final_expr
            };
            outer_select_items.push(format!("    {} AS {}", cast_expr, quote_ident(&met.name)));
        }
    }

    sql.push_str(&outer_select_items.join(",\n"));

    // FROM the CTE
    sql.push_str("\nFROM __sv_snapshot");

    // GROUP BY (when dims are present)
    if !resolved_dims.is_empty() {
        sql.push_str("\nGROUP BY\n");
        let group_items: Vec<String> = (1..=resolved_dims.len())
            .map(|i| format!("    {i}"))
            .collect();
        sql.push_str(&group_items.join(",\n"));
    }

    Ok(sql)
}

/// A group of metrics sharing the same NON ADDITIVE BY dimension set.
///
/// Replaces the tuple `(Vec<NonAdditiveDim>, Vec<usize>)` with named fields
/// for readability.
struct NaGroup {
    /// The actual `NonAdditiveDim` entries for this group.
    na_dims: Vec<NonAdditiveDim>,
    /// Indices into `resolved_mets` that belong to this group.
    metric_indices: Vec<usize>,
}

/// Group metrics by their NON ADDITIVE BY dimension sets.
/// Only includes ACTIVE semi-additive metrics (those with at least one NA dim
/// not in the queried dimensions).
fn collect_na_groups(
    resolved_mets: &[&Metric],
    queried_dim_names: &HashSet<String>,
) -> Vec<NaGroup> {
    let mut groups: Vec<(Vec<String>, Vec<NonAdditiveDim>, Vec<usize>)> = Vec::new();
    for (idx, met) in resolved_mets.iter().enumerate() {
        if met.non_additive_by.is_empty() {
            continue;
        }
        // Skip effectively-regular metrics (all NA dims in query)
        let all_na_in_query = met
            .non_additive_by
            .iter()
            .all(|na| queried_dim_names.contains(&na.dimension.to_ascii_lowercase()));
        if all_na_in_query {
            continue;
        }
        let key: Vec<String> = met
            .non_additive_by
            .iter()
            .map(|nd| nd.dimension.to_ascii_lowercase())
            .collect();
        if let Some(group) = groups.iter_mut().find(|(k, _, _)| *k == key) {
            group.2.push(idx);
        } else {
            groups.push((key, met.non_additive_by.clone(), vec![idx]));
        }
    }
    groups
        .into_iter()
        .map(|(_, na_dims, metric_indices)| NaGroup {
            na_dims,
            metric_indices,
        })
        .collect()
}

/// Extract the inner expression from an aggregate function call.
/// e.g., "SUM(a.balance)" -> Some("a.balance"), "COUNT(*)" -> Some("*"),
/// "revenue - cost" -> None (not an aggregate).
fn extract_aggregate_inner(expr: &str) -> Option<String> {
    let trimmed = expr.trim();
    let open = trimmed.find('(')?;
    let close = trimmed.rfind(')')?;
    if close <= open {
        return None;
    }
    // Verify the part before '(' looks like a function name (alphanumeric/underscore)
    let func_name = trimmed[..open].trim();
    if func_name.is_empty()
        || !func_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(trimmed[open + 1..close].trim().to_string())
}

/// Extract the aggregate function name from an expression.
/// e.g., "SUM(a.balance)" -> Some("SUM"), "COUNT(*)" -> Some("COUNT").
fn extract_aggregate_func(expr: &str) -> Option<&str> {
    let trimmed = expr.trim();
    let open = trimmed.find('(')?;
    let func_name = trimmed[..open].trim();
    if func_name.is_empty()
        || !func_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(func_name)
}

/// Get the `ROW_NUMBER` column name for a given metric index.
fn get_rn_column_for_metric(met_idx: usize, na_groups: &[NaGroup]) -> String {
    if na_groups.len() == 1 {
        return "__sv_rn".to_string();
    }
    for (group_idx, group) in na_groups.iter().enumerate() {
        if group.metric_indices.contains(&met_idx) {
            return format!("__sv_rn_{}", group_idx + 1);
        }
    }
    "__sv_rn".to_string() // fallback
}

#[cfg(test)]
mod tests {
    use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
    use crate::expand::{expand, DimensionName, ExpandError, MetricName, QueryRequest};
    use crate::model::{NullsOrder, SortOrder};

    use super::{extract_aggregate_func, extract_aggregate_inner};

    /// Single table, one semi-additive metric with NA dim NOT in query.
    /// Expects CTE with ROW_NUMBER and conditional aggregation.
    #[test]
    fn test_semi_additive_single_metric_single_dim() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(sql.contains("WITH __sv_snapshot AS"), "SQL: {sql}");
        assert!(
            sql.contains("ROW_NUMBER() OVER"),
            "Should contain ROW_NUMBER: {sql}"
        );
        assert!(
            sql.contains("PARTITION BY \"customer_id\""),
            "Should partition by queried dim: {sql}"
        );
        assert!(
            sql.contains("DESC NULLS FIRST"),
            "Should have DESC NULLS FIRST: {sql}"
        );
        assert!(
            sql.contains("CASE WHEN \"__sv_rn\" = 1"),
            "Should have CASE WHEN for semi-additive: {sql}"
        );
        assert!(
            sql.contains("\"__sv_semi_0\""),
            "Should have semi-additive CTE alias: {sql}"
        );
    }

    /// Semi-additive metric with ALL NA dims in query -> effectively regular.
    /// No CTE should be generated.
    #[test]
    fn test_effectively_regular_all_na_dims_in_query() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![
                DimensionName::new("customer_id"),
                DimensionName::new("report_date"),
            ],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            !sql.contains("WITH __sv_snapshot"),
            "Should NOT have CTE when all NA dims in query: {sql}"
        );
        assert!(
            !sql.contains("ROW_NUMBER"),
            "Should NOT have ROW_NUMBER: {sql}"
        );
        // Standard expansion path
        assert!(
            sql.contains("GROUP BY"),
            "Should have standard GROUP BY: {sql}"
        );
    }

    /// NA dim NOT in query -> CTE generated with PARTITION BY queried dims.
    #[test]
    fn test_semi_additive_na_dim_not_in_query() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("PARTITION BY \"customer_id\""),
            "Should partition by queried dims: {sql}"
        );
        // report_date is the NA dim, should appear in ORDER BY with its expression
        assert!(
            sql.contains("ORDER BY report_date DESC NULLS FIRST"),
            "Should order by NA dim expression: {sql}"
        );
    }

    /// Mixed regular + semi-additive metrics.
    #[test]
    fn test_mixed_regular_and_semi_additive() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "amount",
            "SUM(amount)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_metric("balance", "SUM(balance)", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("amount"), MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(sql.contains("WITH __sv_snapshot"), "Should have CTE: {sql}");
        // Regular metric gets __sv_reg_0 alias
        assert!(
            sql.contains("\"__sv_reg_0\""),
            "Regular metric should have reg alias: {sql}"
        );
        // Semi-additive metric gets __sv_semi_1 alias
        assert!(
            sql.contains("\"__sv_semi_1\""),
            "Semi-additive metric should have semi alias: {sql}"
        );
        // Outer SELECT: regular uses plain aggregation
        assert!(
            sql.contains("SUM(\"__sv_reg_0\")"),
            "Regular metric should use plain SUM: {sql}"
        );
        // Outer SELECT: semi-additive uses CASE WHEN
        assert!(
            sql.contains("SUM(CASE WHEN \"__sv_rn\" = 1 THEN \"__sv_semi_1\" END)"),
            "Semi-additive should use CASE WHEN: {sql}"
        );
    }

    /// Regular-only metrics -> no CTE, flat SELECT.
    #[test]
    fn test_no_semi_additive_no_cte() {
        let def = orders_view();
        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("region")],
            metrics: vec![MetricName::new("total_revenue")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            !sql.contains("WITH __sv_snapshot"),
            "Regular-only should NOT have CTE: {sql}"
        );
        assert!(
            !sql.contains("ROW_NUMBER"),
            "Regular-only should NOT have ROW_NUMBER: {sql}"
        );
    }

    /// Unit test for extract_aggregate_inner helper.
    #[test]
    fn test_extract_aggregate_inner() {
        assert_eq!(
            extract_aggregate_inner("SUM(a.balance)"),
            Some("a.balance".to_string())
        );
        assert_eq!(extract_aggregate_inner("COUNT(*)"), Some("*".to_string()));
        assert_eq!(
            extract_aggregate_inner("AVG(amount)"),
            Some("amount".to_string())
        );
        assert_eq!(extract_aggregate_inner("revenue - cost"), None);
        assert_eq!(extract_aggregate_inner("42"), None);
    }

    /// Unit test for extract_aggregate_func helper.
    #[test]
    fn test_extract_aggregate_func() {
        assert_eq!(extract_aggregate_func("SUM(a.balance)"), Some("SUM"));
        assert_eq!(extract_aggregate_func("COUNT(*)"), Some("COUNT"));
        assert_eq!(extract_aggregate_func("AVG(amount)"), Some("AVG"));
        assert_eq!(extract_aggregate_func("revenue - cost"), None);
    }

    /// Metrics-only (no dims) semi-additive -> global aggregate with CTE.
    #[test]
    fn test_metrics_only_global_aggregate() {
        let def = minimal_def(
            "accounts",
            "report_date",
            "report_date",
            "balance",
            "SUM(balance)",
        )
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("WITH __sv_snapshot"),
            "Global aggregate should have CTE: {sql}"
        );
        // No PARTITION BY when no dims
        assert!(
            !sql.contains("PARTITION BY"),
            "No dims -> no PARTITION BY: {sql}"
        );
        // No GROUP BY when no dims
        assert!(!sql.contains("GROUP BY"), "No dims -> no GROUP BY: {sql}");
        assert!(
            sql.contains("CASE WHEN \"__sv_rn\" = 1"),
            "Should have CASE WHEN: {sql}"
        );
    }

    /// Fan trap check skips semi-additive metrics.
    #[test]
    fn test_fan_trap_skips_semi_additive() {
        // Create a multi-table view where a regular metric on table c
        // queried with dim on table a would cause fan trap (a -> c is many-to-one,
        // dim on a means traversing c->a which is one-to-many fan-out direction).
        // But with semi-additive, it should be skipped.
        let def = orders_view()
            .with_base_table("customers")
            .clear_dimensions()
            .clear_metrics()
            .with_table("c", "customers", &["id"])
            .with_table("a", "accounts", &["id"])
            .with_dimension("acct_name", "a.name", Some("a"))
            .with_metric("total_balance", "SUM(a.balance)", Some("c"))
            .with_non_additive_by(
                "total_balance",
                &[("acct_name", SortOrder::Desc, NullsOrder::First)],
            )
            .with_pkfk_join("cust_acct", "a", "c", &["customer_id"], &["id"]);

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("acct_name")],
            metrics: vec![MetricName::new("total_balance")],
        };

        // This should NOT return a FanTrap error because semi-additive metrics are skipped
        let result = expand("test_view", &def, &req);
        assert!(
            result.is_ok(),
            "Semi-additive metric should skip fan trap: {:?}",
            result.err()
        );
    }

    /// Multi-table JOIN in CTE -- semi-additive metric on joined table.
    #[test]
    fn test_semi_additive_multi_table_join() {
        let def = orders_view()
            .with_base_table("accounts")
            .clear_dimensions()
            .clear_metrics()
            .with_table("a", "accounts", &["id"])
            .with_table("c", "customers", &["id"])
            .with_dimension("customer_name", "c.name", Some("c"))
            .with_dimension("report_date", "a.report_date", Some("a"))
            .with_metric("total_balance", "SUM(a.balance)", Some("a"))
            .with_non_additive_by(
                "total_balance",
                &[("report_date", SortOrder::Desc, NullsOrder::First)],
            )
            .with_pkfk_join("acct_cust", "a", "c", &["customer_id"], &["id"]);

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_name")],
            metrics: vec![MetricName::new("total_balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(sql.contains("WITH __sv_snapshot"), "Should have CTE: {sql}");
        assert!(sql.contains("LEFT JOIN"), "CTE should include JOIN: {sql}");
        assert!(
            sql.contains("PARTITION BY \"customer_name\""),
            "Should partition by customer_name: {sql}"
        );
    }

    /// DESC NULLS FIRST in non_additive_by -> ORDER BY has "DESC NULLS FIRST".
    #[test]
    fn test_desc_nulls_first_order() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("DESC NULLS FIRST"),
            "Should have DESC NULLS FIRST in ORDER BY: {sql}"
        );
    }

    /// ASC NULLS LAST in non_additive_by -> ORDER BY has "ASC NULLS LAST".
    #[test]
    fn test_asc_nulls_last_order() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Asc, NullsOrder::Last)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("ASC NULLS LAST"),
            "Should have ASC NULLS LAST in ORDER BY: {sql}"
        );
    }
}
