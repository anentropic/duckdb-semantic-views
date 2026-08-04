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
//! Each substituted expression is parenthesized, since the splice is textual and
//! the predicate is an operator expression — see [`resolve_where_clause`].

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
    //
    // Each replacement is PARENTHESIZED, because the splice is textual and the
    // predicate is an operator expression: a member whose expression binds
    // looser than its surrounding context would otherwise have its grouping
    // silently destroyed. `us_or_eu AND is_large` spliced bare yields
    // `… = 'US' OR … = 'EU' AND amount > 100`, which SQL reads as
    // `US OR (EU AND large)` — wrong rows, and no error to notice it by. The
    // other two `inline_references` call sites (fact chaining in
    // `expand::facts`, derived metrics in `expand::per_grain`) already wrap for
    // exactly this reason. Wrapping unconditionally keeps the three consistent;
    // a redundant `(o.region)` around a plain column is harmless.
    // EXP-14: every member is keyed BOTH bare (`order_date`) and by its own
    // `source_table.name` (`o.order_date`), because `scan_references` keys a
    // dotted chain by the whole chain. Keying bare only meant a qualified
    // reference matched nothing: it survived into the SQL as a raw column —
    // silently filtering on different semantics than the member wherever a
    // dimension's expression differs from a same-named physical column — and
    // skipped the metric check entirely, so `o.revenue > 5` slipped past
    // `WhereClauseReferencesMetric`. This mirrors `expand::facts::insert_fact_keys`
    // and the dotted-reference handling at the NA-dim and window sites
    // (TECH-DEBT #28/#30). Keying by the member's OWN table is what keeps a
    // *foreign* qualifier (`c.order_date`) an ordinary column, per E-3.
    let keys_for = |source_table: Option<&str>, name: &str| {
        let mut keys = Vec::with_capacity(2);
        if let Some(st) = source_table {
            keys.push(normalize_ident_part(&format!("{st}.{name}")));
        }
        keys.push(normalize_ident_part(name));
        keys
    };

    let mut dim_exprs: HashMap<String, String> = HashMap::new();
    let mut member_tables: HashMap<String, Option<&str>> = HashMap::new();
    for dim in &def.dimensions {
        // Dimensions are never PRIVATE — `PRIVATE` on a dimension is rejected at
        // CREATE (`body_parser::entries`), which is why `sql_gen`'s `Resolvable`
        // impl has `unreachable!("dimensions cannot be private")`. So there is no
        // access check to make here, only on facts below.
        for key in keys_for(dim.source_table.as_deref(), &dim.name) {
            dim_exprs.insert(key.clone(), format!("({})", dim.expr));
            member_tables.insert(key, dim.source_table.as_deref());
        }
    }
    // EXP-13: a PRIVATE fact is deliberately NOT added to the lookup. It is
    // recorded separately so a reference to one is rejected by name rather than
    // falling through to "unknown reference", which would leave it in the SQL as
    // a raw column and leak the very member PRIVATE withholds.
    let mut private_facts: HashMap<String, &str> = HashMap::new();
    for fact in &def.facts {
        for key in keys_for(fact.source_table.as_deref(), &fact.name) {
            if fact.access == crate::model::AccessModifier::Private {
                private_facts.entry(key).or_insert(fact.name.as_str());
                continue;
            }
            // A dimension and a fact may share a name; the dimension wins,
            // matching the precedence the select-list resolution already uses.
            dim_exprs
                .entry(key.clone())
                .or_insert_with(|| format!("({})", fact.expr));
            member_tables
                .entry(key)
                .or_insert(fact.source_table.as_deref());
        }
    }
    let mut metric_names: HashMap<String, &str> = HashMap::new();
    for m in &def.metrics {
        for key in keys_for(m.source_table.as_deref(), &m.name) {
            metric_names.entry(key).or_insert(m.name.as_str());
        }
    }

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
        // EXP-13: a PRIVATE fact the predicate names. Checked after the member
        // lookup so a public dimension sharing its name still resolves, and
        // before the metric check for the same reason the metric check sits
        // after the member lookup — the most specific match wins.
        if let Some(fact_name) = private_facts.get(&key) {
            return Err(ExpandError::PrivateFact {
                view_name: view_name.to_string(),
                name: (*fact_name).to_string(),
            });
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

    let borrowed: HashMap<String, &str> = dim_exprs
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str()))
        .collect();
    Ok(ResolvedWhere {
        sql: inline_references(raw, &borrowed),
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
        assert_eq!(r.sql, "(o.ordered_at) > DATE '1995-01-01'");
        assert_eq!(r.source_tables, vec!["o"]);
    }

    #[test]
    fn substitutes_a_fact_expression() {
        let r = resolve_where_clause("v", &def(), "net_price > 100").unwrap();
        assert_eq!(r.sql, "(o.price * (1 - o.discount)) > 100");
    }

    // EXP-13: the member lookup was built from *all* facts with no
    // `AccessModifier` check, so a PRIVATE fact that `resolve_names` refuses to
    // let you select could still be filtered on — and its values probed by
    // varying the predicate. The queried-member path has enforced PRIVATE since
    // Phase 43; the predicate path did not. (Only facts are affected: `PRIVATE`
    // on a dimension is rejected at CREATE — see PAR-4.)

    #[test]
    fn a_private_fact_is_rejected_in_the_predicate() {
        let def = SemanticViewDefinition::default()
            .with_table("o", "orders", &["id"])
            .with_private_fact("secret_margin", "o.price - o.cost", "o");
        let err = resolve_where_clause("v", &def, "secret_margin > 0").unwrap_err();
        assert!(
            matches!(err, ExpandError::PrivateFact { ref name, .. } if name == "secret_margin"),
            "expected PrivateFact, got {err:?}"
        );
    }

    #[test]
    fn a_private_fact_is_rejected_through_a_qualified_reference_too() {
        let def = SemanticViewDefinition::default()
            .with_table("o", "orders", &["id"])
            .with_private_fact("secret_margin", "o.price - o.cost", "o");
        let err = resolve_where_clause("v", &def, "o.secret_margin > 0").unwrap_err();
        assert!(
            matches!(err, ExpandError::PrivateFact { ref name, .. } if name == "secret_margin"),
            "expected PrivateFact, got {err:?}"
        );
    }

    /// Control for EXP-13: a non-private fact of the same shape still resolves,
    /// so the guard rejects on access rather than on being a fact at all.
    #[test]
    fn a_public_fact_still_resolves_in_the_predicate() {
        let r = resolve_where_clause("v", &def(), "net_price > 100").unwrap();
        assert_eq!(r.sql, "(o.price * (1 - o.discount)) > 100");
    }

    // EXP-14: `scan_references` keys a dotted chain as `o.order_date`, but the
    // member lookup was keyed by bare name only. A qualified reference therefore
    // matched nothing: it was left as a raw column (silently filtering on
    // different semantics than the member wherever the two differ) and skipped
    // the metric check entirely. Every other member-reference site resolves
    // dotted references (#30/#28); this one did not.

    #[test]
    fn a_qualified_dimension_reference_is_substituted() {
        let r = resolve_where_clause("v", &def(), "o.order_date > DATE '1995-01-01'").unwrap();
        assert_eq!(r.sql, "(o.ordered_at) > DATE '1995-01-01'");
        assert_eq!(r.source_tables, vec!["o"]);
    }

    #[test]
    fn a_qualified_fact_reference_is_substituted() {
        let r = resolve_where_clause("v", &def(), "o.net_price > 100").unwrap();
        assert_eq!(r.sql, "(o.price * (1 - o.discount)) > 100");
    }

    #[test]
    fn a_qualified_metric_reference_is_still_rejected() {
        let err = resolve_where_clause("v", &def(), "o.revenue > 5").unwrap_err();
        assert!(
            matches!(err, ExpandError::WhereClauseReferencesMetric { ref metric_name, .. }
                if metric_name == "revenue"),
            "expected WhereClauseReferencesMetric, got {err:?}"
        );
    }

    /// Control for EXP-14: a qualifier that is *not* the member's own table is a
    /// column on another relation, not a member reference (the E-3 rule), so it
    /// must stay untouched rather than being substituted.
    #[test]
    fn a_foreign_qualified_reference_is_left_as_a_raw_column() {
        let r = resolve_where_clause("v", &def(), "c.order_date > DATE '1995-01-01'").unwrap();
        assert_eq!(r.sql, "c.order_date > DATE '1995-01-01'");
    }

    /// A member whose expression is a compound of LOOSER-binding operators than
    /// the context it lands in must keep its grouping. `us_or_eu` is an `OR`;
    /// composing it with `AND` must not let the tighter `AND` capture only the
    /// `OR`'s right operand.
    ///
    /// Spliced bare this yielded
    /// `o.country = 'US' OR o.country = 'EU' AND o.amount > 100`, which SQL
    /// reads as `US OR (EU AND large)` — silently WRONG ROWS, not an error. The
    /// other two `inline_references` call sites (fact chaining in `facts.rs`,
    /// derived metrics in `per_grain.rs`) already wrap each replacement in
    /// parentheses for exactly this reason; this path did not.
    ///
    /// Named filters make this the common case rather than a corner: a filter's
    /// expression is boolean by construction, so it is far more likely to be a
    /// compound than an ordinary dimension's column reference.
    #[test]
    fn a_compound_member_keeps_its_grouping_when_spliced() {
        let def = SemanticViewDefinition::default()
            .with_table("o", "orders", &["id"])
            .with_dimension(
                "us_or_eu",
                "o.country = 'US' OR o.country = 'EU'",
                Some("o"),
            )
            .with_fact("is_large", "o.amount > 100", "o");
        let r = resolve_where_clause("v", &def, "us_or_eu AND is_large").unwrap();
        assert_eq!(
            r.sql,
            "(o.country = 'US' OR o.country = 'EU') AND (o.amount > 100)"
        );
    }

    /// `NOT` over a compound member has the same hazard: `NOT` binds tighter
    /// than `OR`, so a bare splice negates only the first operand.
    #[test]
    fn negating_a_compound_member_negates_the_whole_expression() {
        let def = SemanticViewDefinition::default()
            .with_table("o", "orders", &["id"])
            .with_dimension(
                "us_or_eu",
                "o.country = 'US' OR o.country = 'EU'",
                Some("o"),
            );
        let r = resolve_where_clause("v", &def, "NOT us_or_eu").unwrap();
        assert_eq!(r.sql, "NOT (o.country = 'US' OR o.country = 'EU')");
    }

    #[test]
    fn member_lookup_is_case_and_quote_insensitive() {
        let r = resolve_where_clause("v", &def(), "\"REGION\" = 'EU'").unwrap();
        assert_eq!(r.sql, "(c.region) = 'EU'");
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
        assert_eq!(r.sql, "(c.region) = 'region'");
    }

    #[test]
    fn a_metric_name_inside_a_string_literal_does_not_trigger_rejection() {
        let r = resolve_where_clause("v", &def(), "region = 'revenue'").unwrap();
        assert_eq!(r.sql, "(c.region) = 'revenue'");
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
        assert_eq!(r.sql, "lower((c.region)) = 'eu'");
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
