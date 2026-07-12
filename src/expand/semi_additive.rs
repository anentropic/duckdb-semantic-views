//! CTE-based expansion for semi-additive metrics.
//!
//! When any requested metric has a non-empty `non_additive_by` field AND at least
//! one of its NA dims is NOT in the queried dimensions, the expansion wraps the
//! base query in a CTE that uses `RANK()` to select snapshot rows before
//! aggregation. `RANK()` — not `ROW_NUMBER()` — so that when the fact grain is
//! finer than the queried dims (e.g. several accounts per customer, each with a
//! row at the same latest date), ALL rows tied at the snapshot ordering value
//! share rank 1 and aggregate together, deterministically (SG-4).
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
//! - `RANK() OVER (PARTITION BY non-NA-dims ORDER BY NA-dims) AS __sv_rn`
//!   (the `__sv_rn` column name predates the RANK switch and is pinned by
//!   sqllogictests -- keep it)
//! - Outer SELECT: regular/effectively-regular metrics use plain aggregation,
//!   active semi-additive metrics use `SUM(CASE WHEN __sv_rn = 1 THEN raw_val END)`
//!
//! Because the CTE decomposes every metric into an inner-expression capture plus
//! an outer re-aggregation, every metric in the query must be a single bare
//! aggregate call `FUNC(args)`. Anything else (arithmetic-wrapped, `COUNT(*)`,
//! DISTINCT, `COALESCE`-wrapped, multi-aggregate derived metrics) cannot be
//! decomposed and is rejected with a clear error instead of silently mangled
//! (SG-5).
//!
//! When multiple semi-additive metrics have different NON ADDITIVE BY dimensions,
//! each gets its own `__sv_rn_N` column in the CTE.

use std::collections::{HashMap, HashSet};

use crate::model::{Metric, NonAdditiveDim, NullsOrder, SemanticViewDefinition, SortOrder};
use crate::util::replace_word_boundary;

use super::join_resolver::{push_join_clauses, resolve_joins_pkfk};
use super::resolution::{qualify_and_quote_table_ref, quote_ident};
use super::types::ExpandError;

/// Returns true when `met` is an ACTIVE semi-additive metric for a query over
/// `queried_dim_names` (lowercased): it has a NON ADDITIVE BY clause and at
/// least one of its NA dims is NOT in the queried dimension set, so it takes
/// the `RANK`-CTE snapshot path. When ALL NA dims are queried, the
/// metric is "effectively regular" (Snowflake semantics) and takes the
/// standard aggregation path.
///
/// This is THE routing predicate — shared by `expand()` (CTE dispatch),
/// `expand_semi_additive` (per-metric classification), and the fan-trap check
/// (which must skip exactly the metrics that take the CTE path, SG-6) so the
/// three cannot drift.
pub(super) fn is_active_semi_additive(met: &Metric, queried_dim_names: &HashSet<String>) -> bool {
    !met.non_additive_by.is_empty()
        && met
            .non_additive_by
            .iter()
            .any(|na| !queried_dim_names.contains(&na.dimension.to_ascii_lowercase()))
}

/// Generate CTE-based expansion SQL for queries containing semi-additive metrics.
///
/// Called from `expand()` when `has_active_semi_additive` is true.
/// Receives already-resolved dims, metrics, expressions, and scoped aliases.
#[allow(clippy::too_many_lines)]
pub(super) fn expand_semi_additive(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&Metric],
    resolved_exprs: &HashMap<String, String>,
    dim_scoped_aliases: &[Option<String>],
) -> Result<String, ExpandError> {
    let mut sql = String::with_capacity(512);

    // Build set of queried dimension names for classification
    let queried_dim_names: HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();

    // Classify each metric as active semi-additive (shared routing predicate)
    let is_active_semi =
        |met: &Metric| -> bool { is_active_semi_additive(met, &queried_dim_names) };

    // 1. Identify distinct NON ADDITIVE BY dimension sets for ACTIVE metrics only.
    let na_groups = collect_na_groups(resolved_mets, &queried_dim_names);

    // 2. SG-5: validate every metric expression BEFORE emitting any SQL. The
    //    snapshot CTE decomposes each metric into an inner-expression capture
    //    (CTE column) plus an outer re-aggregation, which is only sound for a
    //    single bare aggregate call. Anything else was previously mangled
    //    silently (dropped arithmetic, star/DISTINCT arguments emitted as
    //    broken CTE columns) -- reject it with a clear error instead.
    let semi_metric_name = resolved_mets
        .iter()
        .find(|m| is_active_semi(m))
        .map_or_else(String::new, |m| m.name.clone());
    let mut decomposed: Vec<(String, String)> = Vec::with_capacity(resolved_mets.len());
    for met in resolved_mets {
        let resolved_expr = resolved_exprs
            .get(&met.name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| met.expr.clone());
        match parse_snapshot_aggregate(&resolved_expr) {
            Ok(parts) => decomposed.push(parts),
            Err(reason) => {
                return Err(if is_active_semi(met) {
                    ExpandError::SemiAdditiveUnsupportedExpression {
                        view_name: view_name.to_string(),
                        metric_name: met.name.clone(),
                        metric_expr: resolved_expr,
                        reason,
                    }
                } else {
                    ExpandError::SemiAdditiveCoQueryUnsupported {
                        view_name: view_name.to_string(),
                        metric_name: met.name.clone(),
                        metric_expr: resolved_expr,
                        semi_metric_name: semi_metric_name.clone(),
                        reason,
                    }
                });
            }
        }
    }

    // === CTE ===
    sql.push_str("WITH __sv_snapshot AS (\n    SELECT\n");

    let mut cte_select_items: Vec<String> = Vec::new();

    // Dimension columns in CTE. Keep each dimension's rendered expression:
    // the RANK() window clauses below must repeat the EXPRESSION, never the
    // select alias — inside the CTE's own SELECT, DuckDB resolves a
    // window-clause identifier to a same-named physical FROM-clause column
    // before the lateral select alias, so `PARTITION BY "region"` with
    // `upper(o.region) AS region` silently partitioned on the raw column
    // and produced wrong snapshot sums (E-1, code-review 2026-07-11). The
    // standard path defends against the same shadowing with GROUP BY
    // ordinals; this is the CTE-path equivalent.
    let mut dim_cte_exprs: Vec<String> = Vec::with_capacity(resolved_dims.len());
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
        dim_cte_exprs.push(final_expr);
    }

    // Metric raw columns in CTE -- the validated inner expression of each
    // metric's aggregate call (decomposed above).
    for (met_idx, met) in resolved_mets.iter().enumerate() {
        let inner = &decomposed[met_idx].1;
        if is_active_semi(met) {
            // Active semi-additive: snapshot-filtered in the outer SELECT
            cte_select_items.push(format!("        {inner} AS \"__sv_semi_{met_idx}\""));
        } else {
            // Regular or effectively-regular: aggregated over all rows
            cte_select_items.push(format!("        {inner} AS \"__sv_reg_{met_idx}\""));
        }
    }

    // Snapshot rank columns (one per active NA group). RANK() so that rows
    // tied on all NA ordering keys share rank 1 and ALL aggregate (SG-4);
    // ROW_NUMBER() would keep one arbitrary tied row per partition.
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

        // Partition by the dimension EXPRESSIONS, not the CTE aliases (E-1).
        let partition_dims: Vec<String> = resolved_dims
            .iter()
            .zip(&dim_cte_exprs)
            .filter(|(d, _)| !na_dim_names.contains(&d.name.to_ascii_lowercase()))
            .map(|(_, expr)| expr.clone())
            .collect();

        let partition_clause = if partition_dims.is_empty() {
            String::new()
        } else {
            format!("PARTITION BY {}", partition_dims.join(", "))
        };

        // ORDER BY: the NA dims with their specified sort order, always as
        // EXPRESSIONS (E-1: a queried NA dim previously ordered by its CTE
        // alias, which DuckDB binds to a same-named physical column first).
        // A queried NA dim uses its rendered CTE expression; an unqueried NA
        // dim uses its raw definition expression (it has no CTE column). That
        // raw expression may reference a non-base table -- its join is
        // guaranteed below (SG-9).
        let order_items: Vec<String> = group
            .na_dims
            .iter()
            .map(|nd| {
                // Try to find the dimension in resolved (queried) dims first
                let dim_expr = resolved_dims
                    .iter()
                    .position(|d| d.name.eq_ignore_ascii_case(&nd.dimension))
                    .map_or_else(
                        || {
                            // NA dim not in queried dims -- find it in the view definition
                            // and use its raw expression
                            def.dimensions
                                .iter()
                                .find(|d| d.name.eq_ignore_ascii_case(&nd.dimension))
                                .map_or_else(|| quote_ident(&nd.dimension), |d| d.expr.clone())
                        },
                        |idx| dim_cte_exprs[idx].clone(),
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
            "        RANK() OVER ({window_spec}) AS \"{rn_alias}\""
        ));
    }

    sql.push_str(&cte_select_items.join(",\n"));

    // CTE FROM clause (same logic as expand())
    sql.push_str("\n    FROM ");
    sql.push_str(&qualify_and_quote_table_ref(def.base_table(), def));
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }

    // CTE JOINs. SG-9: the snapshot ORDER BY references each active NA dim's
    // raw expression even when that dim is not queried, so the NA dims'
    // source tables must be joined too. They are passed through the
    // resolver's extra-alias parameter, which appends each with its path
    // intermediaries (aliases already joined for dims/metrics are skipped).
    let na_dim_sources = collect_na_dim_source_tables(def, &na_groups);
    let resolved_joins = resolve_joins_pkfk(def, resolved_dims, resolved_mets, &na_dim_sources);
    push_join_clauses(&mut sql, &resolved_joins, def, "\n    LEFT JOIN ");

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
        let agg_func = &decomposed[met_idx].0;

        if is_active_semi(met) {
            // Active semi-additive: aggregate only rows at the snapshot rank.
            // All rows tied at rank 1 contribute (RANK semantics, SG-4).
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
        // Skip regular and effectively-regular metrics (shared routing predicate)
        if !is_active_semi_additive(met, queried_dim_names) {
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

/// Collect the (lowercased) source-table aliases of every active NA dim,
/// resolved against the view's declared dimensions (SG-9).
///
/// A NA dim that lives on a non-queried, non-base table contributes its
/// source alias here so the snapshot CTE joins that table (the resolver adds
/// path intermediaries); otherwise its raw expression in the snapshot ORDER
/// BY would reference an unjoined table and fail at bind time. Base-table
/// dims (`source_table == None`) and unresolvable names contribute nothing.
fn collect_na_dim_source_tables(
    def: &SemanticViewDefinition,
    na_groups: &[NaGroup],
) -> Vec<String> {
    let mut sources: Vec<String> = Vec::new();
    for group in na_groups {
        for nd in &group.na_dims {
            let Some(dim) = def
                .dimensions
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(&nd.dimension))
            else {
                continue;
            };
            if let Some(ref st) = dim.source_table {
                let alias = st.to_ascii_lowercase();
                if !sources.contains(&alias) {
                    sources.push(alias);
                }
            }
        }
    }
    sources
}

/// Aggregate functions the snapshot CTE knows how to decompose into an
/// inner-expression capture (CTE column) plus an outer re-aggregation.
const SNAPSHOT_AGG_FUNCS: [&str; 5] = ["SUM", "COUNT", "AVG", "MIN", "MAX"];

/// Validate and decompose a metric expression for the snapshot CTE (SG-5).
///
/// Accepts exactly one top-level aggregate call `FUNC(args)` where `FUNC` is
/// SUM/COUNT/AVG/MIN/MAX (any case), `args` is neither `*` nor
/// DISTINCT-qualified, and no expression text precedes or follows the call.
/// Returns `(FUNC as written, inner argument text)` on success, or a
/// human-readable reason why the expression cannot be decomposed.
///
/// The matching-paren scan is parenthesis-depth and quote aware
/// (single-quoted SQL strings, double-quoted identifiers), so arguments
/// containing parens or quotes classify correctly.
fn parse_snapshot_aggregate(expr: &str) -> Result<(String, String), String> {
    let trimmed = expr.trim();
    let Some(open) = trimmed.find('(') else {
        return Err("the expression is not an aggregate function call".to_string());
    };
    let func_name = trimmed[..open].trim();
    if func_name.is_empty()
        || !func_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(
            "the expression does not start with a single aggregate function call".to_string(),
        );
    }
    if !SNAPSHOT_AGG_FUNCS.contains(&func_name.to_ascii_uppercase().as_str()) {
        return Err(format!(
            "aggregate function '{func_name}' cannot be decomposed for snapshot aggregation \
             (supported: SUM, COUNT, AVG, MIN, MAX)"
        ));
    }
    let Some(close) = find_matching_paren(trimmed, open) else {
        return Err("unbalanced parentheses in the expression".to_string());
    };
    let trailing = trimmed[close + 1..].trim();
    if !trailing.is_empty() {
        return Err(format!(
            "expression text follows the aggregate call: '{trailing}'"
        ));
    }
    let inner = trimmed[open + 1..close].trim();
    if inner.is_empty() {
        return Err("the aggregate call has no argument".to_string());
    }
    if inner == "*" {
        return Err("star aggregates like COUNT(*) are not supported".to_string());
    }
    if starts_with_distinct_keyword(inner) {
        return Err("DISTINCT aggregates are not supported".to_string());
    }
    Ok((func_name.to_string(), inner.to_string()))
}

/// Byte index of the `)` matching the `(` at byte offset `open`, or `None`
/// when unbalanced. Parens inside single-quoted SQL strings and double-quoted
/// identifiers are ignored; the doubled-quote escapes (`''`, `""`) fall out
/// naturally from toggling the in-quote state on every quote character.
fn find_matching_paren(s: &str, open: usize) -> Option<usize> {
    enum Mode {
        Normal,
        SingleQuote,
        DoubleQuote,
    }
    let mut mode = Mode::Normal;
    let mut depth = 0usize;
    for (i, c) in s[open..].char_indices() {
        match mode {
            Mode::Normal => match c {
                '(' => depth += 1,
                ')' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(open + i);
                    }
                }
                '\'' => mode = Mode::SingleQuote,
                '"' => mode = Mode::DoubleQuote,
                _ => {}
            },
            Mode::SingleQuote => {
                if c == '\'' {
                    mode = Mode::Normal;
                }
            }
            Mode::DoubleQuote => {
                if c == '"' {
                    mode = Mode::Normal;
                }
            }
        }
    }
    None
}

/// True when `inner` begins with the `DISTINCT` keyword at a word boundary
/// (case-insensitive). UTF-8 safe: `get(..8)` returns `None` rather than
/// panicking when byte 8 is not a char boundary.
fn starts_with_distinct_keyword(inner: &str) -> bool {
    let Some(prefix) = inner.get(..8) else {
        return false;
    };
    prefix.eq_ignore_ascii_case("DISTINCT")
        && inner[8..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
}

/// Get the snapshot rank column name (`__sv_rn` / `__sv_rn_N`) for a given
/// metric index.
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

    use super::parse_snapshot_aggregate;

    /// Single table, one semi-additive metric with NA dim NOT in query.
    /// Expects CTE with RANK and conditional aggregation.
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
        assert!(sql.contains("RANK() OVER"), "Should contain RANK: {sql}");
        assert!(
            !sql.contains("ROW_NUMBER"),
            "ROW_NUMBER drops snapshot ties (SG-4): {sql}"
        );
        // E-1: the window clause repeats the dimension EXPRESSION, never the
        // CTE alias (an alias matching a physical column binds to the column).
        assert!(
            sql.contains("PARTITION BY customer_id"),
            "Should partition by queried dim expression: {sql}"
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

    /// E-1 regression (code-review 2026-07-11): a dimension whose expression
    /// differs from its bare column must be repeated as an EXPRESSION in the
    /// RANK() PARTITION BY. Referencing the CTE alias instead binds to the
    /// same-named physical column (DuckDB resolves window-clause identifiers
    /// to FROM-clause columns before lateral select aliases), silently
    /// partitioning on the raw column: `upper(region) AS region` over rows
    /// 'us'/'US' produced two partitions and summed both snapshot rows.
    #[test]
    fn test_semi_additive_expr_dim_repeats_expression_not_alias() {
        let def = minimal_def(
            "accounts",
            "region",
            "upper(region)",
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
            dimensions: vec![DimensionName::new("region")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("PARTITION BY upper(region)"),
            "PARTITION BY must repeat the dimension expression: {sql}"
        );
        assert!(
            !sql.contains("PARTITION BY \"region\""),
            "PARTITION BY must not reference the CTE alias: {sql}"
        );
    }

    /// E-1 regression, ORDER BY arm: a QUERIED NA dim (mixed NA group where
    /// another NA dim is absent from the query, keeping the metric active)
    /// must also be ordered by its expression, not its CTE alias.
    #[test]
    fn test_semi_additive_queried_na_dim_orders_by_expression() {
        let def = minimal_def(
            "accounts",
            "region",
            "upper(region)",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_non_additive_by(
            "balance",
            &[
                ("report_date", SortOrder::Desc, NullsOrder::First),
                ("region", SortOrder::Asc, NullsOrder::Last),
            ],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("region")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("upper(region) ASC NULLS LAST"),
            "queried NA dim must order by its expression, not its alias: {sql}"
        );
        assert!(
            !sql.contains("\"region\" ASC"),
            "queried NA dim must not order by the CTE alias: {sql}"
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
        assert!(!sql.contains("RANK"), "Should NOT have RANK: {sql}");
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
            sql.contains("PARTITION BY customer_id"),
            "Should partition by queried dim expression (E-1): {sql}"
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
        assert!(!sql.contains("RANK"), "Regular-only should NOT rank: {sql}");
    }

    /// SG-5: shapes the snapshot CTE can decompose.
    #[test]
    fn test_parse_snapshot_aggregate_accepts_single_aggregate_calls() {
        assert_eq!(
            parse_snapshot_aggregate("SUM(a.balance)"),
            Ok(("SUM".to_string(), "a.balance".to_string()))
        );
        // Function case is preserved so previously-valid emissions stay
        // byte-identical.
        assert_eq!(
            parse_snapshot_aggregate("sum(amount)"),
            Ok(("sum".to_string(), "amount".to_string()))
        );
        assert_eq!(
            parse_snapshot_aggregate("AVG( amount )"),
            Ok(("AVG".to_string(), "amount".to_string()))
        );
        assert_eq!(
            parse_snapshot_aggregate("COUNT(id)"),
            Ok(("COUNT".to_string(), "id".to_string()))
        );
        assert_eq!(
            parse_snapshot_aggregate("MIN(price)"),
            Ok(("MIN".to_string(), "price".to_string()))
        );
        assert_eq!(
            parse_snapshot_aggregate("MAX(price)"),
            Ok(("MAX".to_string(), "price".to_string()))
        );
        // Parens inside a single-quoted string literal do not fool the scan.
        assert_eq!(
            parse_snapshot_aggregate("SUM(CASE WHEN note = ')' THEN amount ELSE 0 END)"),
            Ok((
                "SUM".to_string(),
                "CASE WHEN note = ')' THEN amount ELSE 0 END".to_string()
            ))
        );
        // Parens inside a double-quoted identifier do not fool the scan.
        assert_eq!(
            parse_snapshot_aggregate("SUM(\"weird)col\")"),
            Ok(("SUM".to_string(), "\"weird)col\"".to_string()))
        );
        // A column merely named like DISTINCT is not the DISTINCT keyword.
        assert_eq!(
            parse_snapshot_aggregate("SUM(distinctive_col)"),
            Ok(("SUM".to_string(), "distinctive_col".to_string()))
        );
    }

    /// SG-5: every previously-silently-mangled shape must now be rejected
    /// with a reason.
    #[test]
    fn test_parse_snapshot_aggregate_rejects_undecomposable_shapes() {
        // Arithmetic-wrapped: the `* 0.1` was silently DROPPED before.
        let err = parse_snapshot_aggregate("SUM(amount) * 0.1").unwrap_err();
        assert!(err.contains("follows the aggregate call"), "got: {err}");
        // COUNT(*): the `*` was emitted as a broken star-alias CTE column.
        let err = parse_snapshot_aggregate("COUNT(*)").unwrap_err();
        assert!(err.contains("star aggregates"), "got: {err}");
        // DISTINCT (either case).
        let err = parse_snapshot_aggregate("COUNT(DISTINCT x)").unwrap_err();
        assert!(err.contains("DISTINCT"), "got: {err}");
        let err = parse_snapshot_aggregate("count(distinct x)").unwrap_err();
        assert!(err.contains("DISTINCT"), "got: {err}");
        // COALESCE-wrapped: only the outermost call is considered.
        let err = parse_snapshot_aggregate("COALESCE(SUM(x), 0)").unwrap_err();
        assert!(err.contains("cannot be decomposed"), "got: {err}");
        // Multi-aggregate (inlined derived metric shape).
        let err = parse_snapshot_aggregate("SUM(revenue) - SUM(cost)").unwrap_err();
        assert!(err.contains("follows the aggregate call"), "got: {err}");
        // Leading expression text.
        let err = parse_snapshot_aggregate("1 + SUM(x)").unwrap_err();
        assert!(
            err.contains("does not start with a single aggregate"),
            "got: {err}"
        );
        // Not an aggregate at all (previously fell back to a hardcoded SUM).
        let err = parse_snapshot_aggregate("revenue - cost").unwrap_err();
        assert!(err.contains("not an aggregate function call"), "got: {err}");
        let err = parse_snapshot_aggregate("42").unwrap_err();
        assert!(err.contains("not an aggregate function call"), "got: {err}");
        // Unsupported aggregate function.
        let err = parse_snapshot_aggregate("STRING_AGG(x, ',')").unwrap_err();
        assert!(err.contains("cannot be decomposed"), "got: {err}");
        // Malformed input never panics.
        let err = parse_snapshot_aggregate("SUM(").unwrap_err();
        assert!(err.contains("unbalanced parentheses"), "got: {err}");
        let err = parse_snapshot_aggregate("SUM()").unwrap_err();
        assert!(err.contains("no argument"), "got: {err}");
    }

    /// SG-5: a regular metric with an arithmetic-wrapped aggregate co-queried
    /// with an active semi-additive metric errors instead of silently
    /// dropping the arithmetic.
    #[test]
    fn test_co_query_arithmetic_wrapped_metric_errors() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_metric("discounted", "SUM(amount) * 0.1", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("discounted"), MetricName::new("balance")],
        };

        let result = expand("test_view", &def, &req);
        match result {
            Err(ExpandError::SemiAdditiveCoQueryUnsupported {
                metric_name,
                semi_metric_name,
                ..
            }) => {
                assert_eq!(metric_name, "discounted");
                assert_eq!(semi_metric_name, "balance");
            }
            other => panic!("expected SemiAdditiveCoQueryUnsupported, got: {other:?}"),
        }
    }

    /// SG-5: `COUNT(*)` co-queried with an active semi-additive metric errors
    /// instead of emitting a star-alias CTE column that COUNTs an arbitrary
    /// (NULL-sensitive) join column.
    #[test]
    fn test_co_query_count_star_errors() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_metric("row_count", "COUNT(*)", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("row_count"), MetricName::new("balance")],
        };

        let result = expand("test_view", &def, &req);
        assert!(
            matches!(
                result,
                Err(ExpandError::SemiAdditiveCoQueryUnsupported { .. })
            ),
            "COUNT(*) co-query must error, got: {result:?}"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("cannot be co-queried with semi-additive metric 'balance'"),
            "message should name both metrics: {msg}"
        );
        assert!(
            msg.contains("separately"),
            "message should suggest querying separately: {msg}"
        );
    }

    /// SG-5: DISTINCT aggregate co-query errors.
    #[test]
    fn test_co_query_count_distinct_errors() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_metric("uniq_customers", "COUNT(DISTINCT customer_id)", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![
                MetricName::new("uniq_customers"),
                MetricName::new("balance"),
            ],
        };

        let result = expand("test_view", &def, &req);
        assert!(
            matches!(
                result,
                Err(ExpandError::SemiAdditiveCoQueryUnsupported { .. })
            ),
            "COUNT(DISTINCT ...) co-query must error, got: {result:?}"
        );
    }

    /// SG-5: COALESCE-wrapped aggregate co-query errors.
    #[test]
    fn test_co_query_coalesce_wrapped_errors() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_metric("safe_total", "COALESCE(SUM(amount), 0)", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("safe_total"), MetricName::new("balance")],
        };

        let result = expand("test_view", &def, &req);
        assert!(
            matches!(
                result,
                Err(ExpandError::SemiAdditiveCoQueryUnsupported { .. })
            ),
            "COALESCE-wrapped co-query must error, got: {result:?}"
        );
    }

    /// SG-5: a derived metric (inlines to a multi-aggregate expression)
    /// co-queried with an active semi-additive metric errors instead of
    /// falling back to a hardcoded SUM over a mangled column.
    #[test]
    fn test_co_query_derived_metric_errors() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance)",
        )
        .with_dimension("report_date", "report_date", None)
        .with_metric("revenue", "SUM(amount)", None)
        .with_metric("cost_total", "SUM(cost)", None)
        .with_metric("profit", "revenue - cost_total", None)
        .with_non_additive_by(
            "balance",
            &[("report_date", SortOrder::Desc, NullsOrder::First)],
        );

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("profit"), MetricName::new("balance")],
        };

        let result = expand("test_view", &def, &req);
        match result {
            Err(ExpandError::SemiAdditiveCoQueryUnsupported {
                metric_name,
                metric_expr,
                ..
            }) => {
                assert_eq!(metric_name, "profit");
                // The error carries the INLINED expression the CTE would
                // have had to decompose.
                assert!(
                    metric_expr.contains("SUM(amount)") && metric_expr.contains("SUM(cost)"),
                    "expected inlined derived expression, got: {metric_expr}"
                );
            }
            other => panic!("expected SemiAdditiveCoQueryUnsupported, got: {other:?}"),
        }
    }

    /// SG-5: the active semi-additive metric's OWN expression is validated
    /// too — an arithmetic-wrapped NON ADDITIVE BY metric errors instead of
    /// silently dropping the arithmetic.
    #[test]
    fn test_semi_additive_metric_own_expression_validated() {
        let def = minimal_def(
            "accounts",
            "customer_id",
            "customer_id",
            "balance",
            "SUM(balance) * 2",
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

        let result = expand("test_view", &def, &req);
        match result {
            Err(ExpandError::SemiAdditiveUnsupportedExpression { metric_name, .. }) => {
                assert_eq!(metric_name, "balance");
            }
            other => panic!("expected SemiAdditiveUnsupportedExpression, got: {other:?}"),
        }
    }

    /// SG-9: a NON ADDITIVE BY dim living on a non-queried, non-base table
    /// must get its source table joined into the snapshot CTE — otherwise the
    /// snapshot ORDER BY references an unjoined alias and fails at bind time.
    /// Topology: base `a AS accounts` with `a(date_id) REFERENCES d`, NA dim
    /// `report_date = d.report_date`, query by a BASE dim only.
    #[test]
    fn test_na_dim_on_non_queried_table_joins_its_source() {
        let mut def = minimal_def(
            "accounts",
            "customer_id",
            "a.customer_id",
            "balance",
            "SUM(a.balance)",
        );
        def.tables[0].alias = "a".to_string();
        def.tables[0].pk_columns = vec!["id".to_string()];
        def.metrics[0].source_table = Some("a".to_string());
        let def = def
            .with_table("d", "dates", &["id"])
            .with_dimension("report_date", "d.report_date", Some("d"))
            .with_non_additive_by(
                "balance",
                &[("report_date", SortOrder::Desc, NullsOrder::First)],
            )
            .with_pkfk_join("acct_date", "a", "d", &["date_id"], &["id"]);

        let req = QueryRequest {
            facts: vec![],
            dimensions: vec![DimensionName::new("customer_id")],
            metrics: vec![MetricName::new("balance")],
        };

        let sql = expand("test_view", &def, &req).unwrap();
        assert!(
            sql.contains("LEFT JOIN \"dates\" AS \"d\" ON \"a\".\"date_id\" = \"d\".\"id\""),
            "Snapshot CTE must join the NA dim's source table: {sql}"
        );
        assert!(
            sql.contains("ORDER BY d.report_date DESC NULLS FIRST"),
            "Snapshot ORDER BY must reference the joined alias: {sql}"
        );
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

    /// SG-6 (code review 2026-07-02): a semi-additive metric whose NA dims
    /// are ALL in the queried dimensions is "effectively regular" — it takes
    /// the standard aggregation path (never the snapshot CTE), so it MUST
    /// get the standard fan-trap check. The previous unconditional
    /// `non_additive_by`-non-empty skip let this query silently inflate.
    /// (The CTE-path skip is pinned by
    /// `fan_trap::tests::test_check_fan_traps_semi_additive_cte_path_skipped`.)
    #[test]
    fn test_fan_trap_checks_effectively_regular_semi_additive() {
        // Multi-table view where the metric on table c queried with a dim on
        // table a causes a fan trap (a -> c is many-to-one; the dim on a
        // means traversing c -> a, the fan-out direction). The metric's only
        // NA dim (acct_name) IS queried, so it acts as a regular aggregate.
        let def = orders_view()
            .with_table("customers", "customers", &[])
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

        // Effectively-regular semi-additive metrics get the standard check.
        let result = expand("test_view", &def, &req);
        assert!(
            matches!(result, Err(ExpandError::FanTrap { .. })),
            "Effectively-regular semi-additive metric over a fanning join \
             must be a fan trap error, got: {result:?}"
        );
    }

    /// Multi-table JOIN in CTE -- semi-additive metric on joined table.
    #[test]
    fn test_semi_additive_multi_table_join() {
        let def = orders_view()
            .with_table("accounts", "accounts", &[])
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
            sql.contains("PARTITION BY c.name"),
            "Should partition by the customer_name expression (E-1): {sql}"
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

    /// Data-level tests: execute the generated snapshot SQL against an
    /// in-memory `DuckDB`. The `extension` feature swaps the bundled API for
    /// loadable-extension stubs, so these are gated like catalog.rs's tests.
    #[cfg(not(feature = "extension"))]
    mod execution {
        use super::*;

        fn run_by_first_col(sql: &str) -> Vec<(String, f64)> {
            let con = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
            con.execute_batch(
                "CREATE TABLE accounts (customer_id VARCHAR, report_date DATE, balance DOUBLE);
                 INSERT INTO accounts VALUES
                     ('alice', DATE '2024-01-01', 999.0),
                     ('alice', DATE '2024-01-02', 100.0),
                     ('alice', DATE '2024-01-02', 150.0),
                     ('bob',   NULL,              40.0),
                     ('bob',   DATE '2024-01-01', 300.0);",
            )
            .expect("setup");
            let wrapped = format!("SELECT * FROM ({sql}) ORDER BY 1");
            let mut stmt = con.prepare(&wrapped).expect("prepare generated SQL");
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })
                .expect("query");
            rows.collect::<Result<Vec<_>, _>>().expect("rows")
        }

        fn snapshot_def(nulls: NullsOrder) -> crate::model::SemanticViewDefinition {
            minimal_def(
                "accounts",
                "customer_id",
                "customer_id",
                "balance",
                "SUM(balance)",
            )
            .with_dimension("report_date", "report_date", None)
            .with_non_additive_by("balance", &[("report_date", SortOrder::Desc, nulls)])
        }

        fn snapshot_req() -> QueryRequest {
            QueryRequest {
                facts: vec![],
                dimensions: vec![DimensionName::new("customer_id")],
                metrics: vec![MetricName::new("balance")],
            }
        }

        /// SG-4 data-level: alice has TWO rows tied at the latest date
        /// (2024-01-02: 100.0 and 150.0). RANK gives both rows rank 1, so
        /// the snapshot sum is 250.0 — deterministically. `ROW_NUMBER` kept
        /// one arbitrary tied row (100.0 or 150.0, run-to-run
        /// nondeterministic). bob (NULLS LAST variant): the latest non-NULL
        /// date wins -> 300.0.
        #[test]
        fn test_ties_at_snapshot_all_aggregate() {
            let sql = expand(
                "test_view",
                &snapshot_def(NullsOrder::Last),
                &snapshot_req(),
            )
            .expect("expand");
            let rows = run_by_first_col(&sql);
            assert_eq!(rows.len(), 2, "two customers: {rows:?}");
            assert_eq!(rows[0].0, "alice");
            assert!(
                (rows[0].1 - 250.0).abs() < 1e-9,
                "both tied rows must aggregate (100 + 150), got: {rows:?}"
            );
            assert_eq!(rows[1].0, "bob");
            assert!(
                (rows[1].1 - 300.0).abs() < 1e-9,
                "NULLS LAST: latest non-NULL date wins for bob, got: {rows:?}"
            );
        }

        /// TC-7 data-level: with DESC NULLS FIRST (the parser default for
        /// DESC), bob's NULL-dated row outranks the dated row -> 40.0.
        #[test]
        fn test_nulls_first_null_row_wins() {
            let sql = expand(
                "test_view",
                &snapshot_def(NullsOrder::First),
                &snapshot_req(),
            )
            .expect("expand");
            let rows = run_by_first_col(&sql);
            assert_eq!(rows[1].0, "bob");
            assert!(
                (rows[1].1 - 40.0).abs() < 1e-9,
                "NULLS FIRST: the NULL-dated row must win for bob, got: {rows:?}"
            );
        }

        /// SG-9 data-level: NA dim on a non-queried, non-base table. The
        /// snapshot CTE joins `dates`, the ORDER BY binds to `d.report_date`,
        /// and each customer's balance snapshots at their latest date.
        /// customer 10: `date_id` 2 (2024-01-02) is latest -> 150.0
        /// customer 20: only `date_id` 1 -> 300.0
        #[test]
        fn test_na_dim_join_executes() {
            let mut def = minimal_def(
                "accounts",
                "customer_id",
                "a.customer_id",
                "balance",
                "SUM(a.balance)",
            );
            def.tables[0].alias = "a".to_string();
            def.tables[0].pk_columns = vec!["id".to_string()];
            def.metrics[0].source_table = Some("a".to_string());
            let def = def
                .with_table("d", "dates", &["id"])
                .with_dimension("report_date", "d.report_date", Some("d"))
                .with_non_additive_by(
                    "balance",
                    &[("report_date", SortOrder::Desc, NullsOrder::First)],
                )
                .with_pkfk_join("acct_date", "a", "d", &["date_id"], &["id"]);

            let sql = expand("test_view", &def, &snapshot_req()).expect("expand");

            let con = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
            con.execute_batch(
                "CREATE TABLE dates (id INTEGER, report_date DATE);
                 INSERT INTO dates VALUES (1, DATE '2024-01-01'), (2, DATE '2024-01-02');
                 CREATE TABLE accounts (id INTEGER, customer_id INTEGER, date_id INTEGER, balance DOUBLE);
                 INSERT INTO accounts VALUES
                     (1, 10, 1, 100.0),
                     (2, 10, 2, 150.0),
                     (3, 20, 1, 300.0);",
            )
            .expect("setup");
            let wrapped = format!("SELECT * FROM ({sql}) ORDER BY 1");
            let mut stmt = con.prepare(&wrapped).expect("prepare generated SQL");
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, f64>(1)?)))
                .expect("query")
                .collect::<Result<Vec<_>, _>>()
                .expect("rows");
            assert_eq!(rows.len(), 2, "rows: {rows:?}");
            assert_eq!(rows[0].0, 10);
            assert!((rows[0].1 - 150.0).abs() < 1e-9, "rows: {rows:?}");
            assert_eq!(rows[1].0, 20);
            assert!((rows[1].1 - 300.0).abs() < 1e-9, "rows: {rows:?}");
        }
    }
}
