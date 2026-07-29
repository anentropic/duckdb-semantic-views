//! Resolution of the pre-aggregation `where_clause` predicate.
//!
//! Snowflake's `SEMANTIC_VIEW( … WHERE <predicate> )` filters the rows that feed
//! the metrics, *before* they are aggregated ("this filter condition is applied
//! before the metrics are computed"). That is not expressible by wrapping the
//! generated query in an outer `WHERE`: by then the aggregation has already run
//! over every row, and the members the predicate names are not in the output.
//!
//! Our query surface is a table function rather than a SQL construct, so the
//! predicate arrives as the `where_clause := '…'` named parameter. (`where :=`
//! is a parse error — `DuckDB` reserves the keyword in named-parameter position —
//! and `"where" :=` would force quoting at every call site. If the
//! `parser_override` hook ever accepts the real `SEMANTIC_VIEW(…)` construct,
//! the genuine `WHERE` keyword arrives there, inside our own syntax.)
//!
//! The predicate names declared *members*, not raw columns, so it is rewritten
//! into their expressions before emission — the same splice the derived-metric
//! path uses ([`crate::expr_tokens::inline_references`]), which is
//! quote/literal-aware, so a member name inside a string literal is untouched.

use std::collections::{HashMap, HashSet};

use crate::expr_tokens::{inline_references, scan_references};
use crate::ident::normalize_ident_part;
use crate::model::SemanticViewDefinition;

use super::types::ExpandError;

/// A `where_clause` rewritten into raw SQL, plus what it touched.
#[derive(Debug)]
pub(super) struct ResolvedWhere {
    /// The predicate with every declared dimension/fact reference replaced by
    /// that member's expression. Ready to splice after `WHERE`.
    pub(super) sql: String,
    /// Lowercased source-table aliases of the members the predicate referenced.
    ///
    /// Snowflake counts `WHERE`-clause members in its same-logical-table rule,
    /// so these participate in the same reachability and fan-out checks as
    /// queried dimensions — a filter on a table that would fan out against a
    /// metric's grain is the same hazard as grouping by one. Filtering on a
    /// table at all requires joining it, and that join multiplies the metric's
    /// rows exactly as a grouping join would.
    pub(super) source_tables: Vec<String>,
    /// `(member name, source table)` for each declared member the predicate
    /// named, in first-reference order. Fed to the fan-trap check so its error
    /// can name the member the user actually wrote.
    pub(super) members: Vec<(String, Option<String>)>,
}

/// Resolve `raw` against the view's declared members.
///
/// Every identifier chain in the predicate is looked up as a dimension name,
/// then a fact name, using the same case- and quote-insensitive key the
/// `dimensions := [...]` list resolves through, so `region`, `REGION`, and
/// `"region"` all name the same member.
///
/// A chain that resolves to a **metric** is rejected
/// ([`ExpandError::WhereClauseReferencesMetric`]) — Snowflake's rule, and
/// structurally necessary: the predicate runs before aggregation, so an
/// aggregate has no value yet.
///
/// A chain that resolves to nothing is left **verbatim**. That is deliberate:
/// the scan cannot tell a mistyped member from ordinary SQL that must survive
/// (`DATE '1995-01-01'`, `NULL`, `TRUE`, `CURRENT_DATE`, a raw column). Passing
/// it through means `DuckDB` validates it and reports an unknown column itself,
/// rather than this layer guessing. A raw column on a table the query does not
/// otherwise join fails loudly at bind time; one on a table already joined
/// filters correctly, since that table's grain is already accounted for.
pub(super) fn resolve_where_clause(
    view_name: &str,
    def: &SemanticViewDefinition,
    raw: &str,
) -> Result<ResolvedWhere, ExpandError> {
    // Member lookup tables, keyed the same way `inline_references` keys its
    // replacements so FIND and SPLICE agree.
    let mut dim_exprs: HashMap<String, &str> = HashMap::new();
    let mut member_tables: HashMap<String, Option<&str>> = HashMap::new();
    for dim in &def.dimensions {
        let key = normalize_ident_part(&dim.name);
        dim_exprs.insert(key.clone(), dim.expr.as_str());
        member_tables.insert(key, dim.source_table.as_deref());
    }
    for fact in &def.facts {
        let key = normalize_ident_part(&fact.name);
        // A dimension and a fact may share a name; the dimension wins, matching
        // the precedence the select-list resolution already uses.
        dim_exprs.entry(key.clone()).or_insert(fact.expr.as_str());
        member_tables
            .entry(key)
            .or_insert(fact.source_table.as_deref());
    }
    let metric_names: HashMap<String, &str> = def
        .metrics
        .iter()
        .map(|m| (normalize_ident_part(&m.name), m.name.as_str()))
        .collect();

    let mut source_tables: Vec<String> = Vec::new();
    let mut members: Vec<(String, Option<String>)> = Vec::new();
    let mut seen_members: HashSet<String> = HashSet::new();
    for r in scan_references(raw) {
        let key = r.key();
        if let Some(table) = member_tables.get(&key) {
            if let Some(t) = table {
                let lowered = t.to_ascii_lowercase();
                if !source_tables.contains(&lowered) {
                    source_tables.push(lowered);
                }
            }
            if seen_members.insert(key.clone()) {
                members.push((r.raw.to_string(), table.map(str::to_ascii_lowercase)));
            }
            continue;
        }
        // Only reject a metric the predicate genuinely names. Checked after the
        // member lookup so a dimension sharing a metric's name still resolves.
        if let Some(metric_name) = metric_names.get(&key) {
            return Err(ExpandError::WhereClauseReferencesMetric {
                view_name: view_name.to_string(),
                metric_name: (*metric_name).to_string(),
            });
        }
    }

    Ok(ResolvedWhere {
        sql: inline_references(raw, &dim_exprs),
        source_tables,
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::test_helpers::TestFixtureExt;

    fn def() -> SemanticViewDefinition {
        SemanticViewDefinition::default()
            .with_table("o", "orders", &["id"])
            .with_table("c", "customers", &["id"])
            .with_dimension("region", "c.region", Some("c"))
            .with_dimension("order_date", "o.ordered_at", Some("o"))
            .with_fact("net_price", "o.price * (1 - o.discount)", "o")
            .with_metric("revenue", "sum(o.price)", Some("o"))
            .with_pkfk_join("o_to_c", "o", "c", &["customer_id"], &["id"])
    }

    #[test]
    fn substitutes_dimension_and_fact_expressions() {
        let r = resolve_where_clause("v", &def(), "order_date > DATE '1995-01-01'").unwrap();
        assert_eq!(r.sql, "o.ordered_at > DATE '1995-01-01'");
        assert_eq!(r.source_tables, vec!["o"]);
    }

    #[test]
    fn substitutes_a_fact_expression() {
        let r = resolve_where_clause("v", &def(), "net_price > 100").unwrap();
        assert_eq!(r.sql, "o.price * (1 - o.discount) > 100");
    }

    #[test]
    fn member_lookup_is_case_and_quote_insensitive() {
        let r = resolve_where_clause("v", &def(), "\"REGION\" = 'EU'").unwrap();
        assert_eq!(r.sql, "c.region = 'EU'");
        assert_eq!(r.source_tables, vec!["c"]);
    }

    #[test]
    fn collects_every_referenced_source_table() {
        let r = resolve_where_clause("v", &def(), "region = 'EU' AND net_price > 10").unwrap();
        assert_eq!(r.source_tables, vec!["c", "o"]);
    }

    #[test]
    fn rejects_a_metric_reference() {
        let err = resolve_where_clause("v", &def(), "revenue > 1000").unwrap_err();
        match err {
            ExpandError::WhereClauseReferencesMetric { metric_name, .. } => {
                assert_eq!(metric_name, "revenue");
            }
            other => panic!("expected WhereClauseReferencesMetric, got: {other:?}"),
        }
    }

    #[test]
    fn a_member_name_inside_a_string_literal_is_not_substituted() {
        // The splice is literal-aware: 'region' is data, not a reference.
        let r = resolve_where_clause("v", &def(), "region = 'region'").unwrap();
        assert_eq!(r.sql, "c.region = 'region'");
    }

    #[test]
    fn a_metric_name_inside_a_string_literal_does_not_trigger_rejection() {
        let r = resolve_where_clause("v", &def(), "region = 'revenue'").unwrap();
        assert_eq!(r.sql, "c.region = 'revenue'");
    }

    #[test]
    fn unknown_references_pass_through_untouched() {
        // Not a declared member -- left for DuckDB to validate rather than
        // guessed at here. `DATE`/`NULL` must survive for the same reason.
        let r = resolve_where_clause("v", &def(), "o.raw_col IS NOT NULL").unwrap();
        assert_eq!(r.sql, "o.raw_col IS NOT NULL");
        assert!(
            r.source_tables.is_empty(),
            "a raw column declares no member source table: {:?}",
            r.source_tables
        );
    }

    #[test]
    fn function_calls_are_left_alone() {
        let r = resolve_where_clause("v", &def(), "lower(region) = 'eu'").unwrap();
        assert_eq!(r.sql, "lower(c.region) = 'eu'");
    }

    #[test]
    fn a_blank_predicate_emits_no_where_clause() {
        // Found by `fuzz_where_predicate` on its first CI run: a `Some("")`
        // predicate rendered a bare `WHERE ` with no condition -- invalid SQL.
        // The FFI layer maps an empty parameter to None, but `expand()` is a
        // public API and was reachable directly with `Some("")`.
        use crate::expand::{expand, MetricName, QueryRequest};
        for blank in ["", "   ", "\t\n "] {
            let req = QueryRequest {
                dimensions: vec![],
                metrics: vec![MetricName::new("revenue")],
                facts: vec![],
                where_clause: Some(blank.to_string()),
            };
            let sql = expand("v", &def(), &req).unwrap();
            assert!(
                !sql.contains("WHERE"),
                "blank predicate {blank:?} must emit no WHERE: {sql}"
            );
        }
    }
}
