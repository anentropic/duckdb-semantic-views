//! SQL keyword body parser for CREATE SEMANTIC VIEW.
//!
//! Parses: `AS TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)`
//! into a `SemanticViewDefinition`.

mod annotations;
mod clause_bounds;
mod entries;
mod materializations;
mod metrics;
mod relationships;
mod scan;
mod tables;
mod window;

use crate::errors::ParseError;
use crate::model::{
    AccessModifier, Dimension, Fact, Join, Materialization, Metric, NonAdditiveDim, TableRef,
    WindowSpec,
};

use clause_bounds::find_clause_bounds;
use scan::split_qualified_identifier;

pub(crate) use entries::parse_qualified_entries;
pub(crate) use materializations::parse_materializations_clause;
pub(crate) use metrics::parse_metrics_clause;
pub(crate) use relationships::parse_relationships_clause;
pub(crate) use scan::split_at_depth0_commas;
pub(crate) use tables::parse_tables_clause;

/// Parsed DIMENSIONS / FACTS entry (R-4: named fields, was a 6-tuple).
///
/// Shared shape for both clauses; DIMENSIONS ignores `access` (a leading
/// PRIVATE/PUBLIC is rejected earlier for dimensions). Fields map onto
/// [`Fact`] / [`Dimension`] in `parse_keyword_body`.
#[derive(Debug)]
pub(super) struct ParsedQualifiedEntry {
    pub(super) source_alias: String,
    pub(super) name: String,
    pub(super) expr: String,
    pub(super) comment: Option<String>,
    pub(super) synonyms: Vec<String>,
    pub(super) access: AccessModifier,
}

/// Parsed METRICS entry (R-4: named fields, was a 9-tuple with `// tuple
/// index N` comments and a 9-way closure destructuring).
///
/// Fields map 1:1 onto [`Metric`]; `output_type` is assigned during
/// expansion, not at parse time, so it has no field here.
#[derive(Debug)]
pub(super) struct ParsedMetric {
    pub(super) source_alias: Option<String>,
    pub(super) name: String,
    pub(super) expr: String,
    pub(super) using_relationships: Vec<String>,
    pub(super) comment: Option<String>,
    pub(super) synonyms: Vec<String>,
    pub(super) access: AccessModifier,
    pub(super) non_additive_by: Vec<NonAdditiveDim>,
    pub(super) window_spec: Option<WindowSpec>,
}

/// Result of parsing the keyword body (everything after "AS").
#[derive(Debug)]
pub struct KeywordBody {
    pub tables: Vec<TableRef>,
    pub relationships: Vec<Join>,
    pub facts: Vec<Fact>,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
    pub materializations: Vec<Materialization>,
}

/// Parse the keyword body after "AS" into structured clause data.
///
/// `text` is the full text starting with "AS", trimmed.
/// `base_offset` is the byte offset of `text[0]` in the original query string.
#[allow(clippy::too_many_lines)]
pub fn parse_keyword_body(text: &str, base_offset: usize) -> Result<KeywordBody, ParseError> {
    // Strip leading "AS" (case-insensitive)
    let trimmed = text.trim();
    let after_as = if trimmed
        .get(..2)
        .is_some_and(|s| s.eq_ignore_ascii_case("AS"))
    {
        trimmed[2..].trim_start()
    } else {
        return Err(ParseError {
            message: "Expected 'AS' keyword at start of semantic view body.".to_string(),
            position: Some(base_offset),
        });
    };

    // Offset of after_as within the original query
    let as_offset = base_offset + (text.len() - text.trim_start().len()) + 2;
    let after_as_offset = as_offset + (text.trim_start()[2..].len() - after_as.len());

    let bounds = find_clause_bounds(after_as, after_as_offset)?;

    let mut tables: Vec<TableRef> = Vec::new();
    let mut relationships: Vec<Join> = Vec::new();
    let mut facts_raw: Vec<ParsedQualifiedEntry> = Vec::new();
    let mut dimensions_raw: Vec<ParsedQualifiedEntry> = Vec::new();
    let mut metrics_raw: Vec<ParsedMetric> = Vec::new();
    let mut materializations: Vec<Materialization> = Vec::new();

    for bound in &bounds {
        match bound.keyword {
            "tables" => {
                tables = parse_tables_clause(bound.content, bound.content_offset)?;
            }
            "relationships" => {
                relationships = parse_relationships_clause(bound.content, bound.content_offset)?;
            }
            "facts" => {
                facts_raw =
                    parse_qualified_entries(bound.content, bound.content_offset, true, "facts")?;
            }
            "dimensions" => {
                dimensions_raw = parse_qualified_entries(
                    bound.content,
                    bound.content_offset,
                    false,
                    "dimensions",
                )?;
            }
            "metrics" => {
                metrics_raw = parse_metrics_clause(bound.content, bound.content_offset)?;
            }
            "materializations" => {
                materializations =
                    parse_materializations_clause(bound.content, bound.content_offset)?;
            }
            _ => {}
        }
    }

    // Map parsed clause entries onto the Fact / Dimension / Metric model types.
    let facts = facts_raw
        .into_iter()
        .map(|e| Fact {
            name: e.name,
            expr: e.expr,
            source_table: Some(e.source_alias),
            output_type: None,
            comment: e.comment,
            synonyms: e.synonyms,
            access: e.access,
        })
        .collect();

    let dimensions: Vec<Dimension> = dimensions_raw
        .into_iter()
        // Dimensions carry no access modifier — `e.access` is intentionally
        // dropped (a leading PRIVATE/PUBLIC is rejected earlier for DIMENSIONS).
        .map(|e| Dimension {
            name: e.name,
            expr: e.expr,
            source_table: Some(e.source_alias),
            output_type: None,
            comment: e.comment,
            synonyms: e.synonyms,
        })
        .collect();

    let metrics: Vec<Metric> = metrics_raw
        .into_iter()
        .map(|m| Metric {
            name: m.name,
            expr: m.expr,
            source_table: m.source_alias,
            output_type: None,
            using_relationships: m.using_relationships,
            comment: m.comment,
            synonyms: m.synonyms,
            access: m.access,
            non_additive_by: m.non_additive_by,
            window_spec: m.window_spec,
        })
        .collect();

    // Phase 47: Validate NON ADDITIVE BY dimension references
    // Phase 68 B1 / D-08: accept dotted-path qualifier `alias.dim_name` in
    // addition to the bare `dim_name` form. The dotted form is split at the
    // first depth-0 dot OUTSIDE a quoted region (so `"a.b"` stays atomic but
    // `o."order date"` splits into `o` + `"order date"`).
    for metric in &metrics {
        for na in &metric.non_additive_by {
            let dim_exists = dimensions.iter().any(|d| {
                if d.name.eq_ignore_ascii_case(&na.dimension) {
                    return true;
                }
                // D-08 dotted-path acceptance: if NA dim is `alias.name`,
                // match against (source_table, name).
                if let Some((alias_part, name_part)) = split_qualified_identifier(&na.dimension) {
                    if let Some(ref src) = d.source_table {
                        return src.eq_ignore_ascii_case(alias_part)
                            && d.name.eq_ignore_ascii_case(name_part);
                    }
                }
                false
            });
            if !dim_exists {
                let available_dims: Vec<String> =
                    dimensions.iter().map(|d| d.name.clone()).collect();
                let suggestion = crate::util::suggest_closest(&na.dimension, &available_dims);
                let mut msg = format!(
                    "NON ADDITIVE BY dimension '{}' on metric '{}' does not match any declared dimension.",
                    na.dimension, metric.name
                );
                if let Some(closest) = suggestion {
                    use std::fmt::Write;
                    let _ = write!(msg, " Did you mean '{closest}'?");
                }
                return Err(ParseError {
                    message: msg,
                    position: None,
                });
            }
        }
    }

    // Phase 48: Validate window metric EXCLUDING dimension and inner metric references
    let metric_names: Vec<String> = metrics.iter().map(|m| m.name.clone()).collect();
    for metric in &metrics {
        if let Some(ref ws) = metric.window_spec {
            // Validate EXCLUDING dimension references
            for dim in &ws.excluding_dims {
                let dim_exists = dimensions.iter().any(|d| d.name.eq_ignore_ascii_case(dim));
                if !dim_exists {
                    let available_dims: Vec<String> =
                        dimensions.iter().map(|d| d.name.clone()).collect();
                    let suggestion = crate::util::suggest_closest(dim, &available_dims);
                    let mut msg = format!(
                        "Window metric '{}': EXCLUDING dimension '{}' not found in semantic view dimensions.",
                        metric.name, dim
                    );
                    if let Some(closest) = suggestion {
                        use std::fmt::Write;
                        let _ = write!(msg, " Did you mean '{closest}'?");
                    }
                    return Err(ParseError {
                        message: msg,
                        position: None,
                    });
                }
            }
            // Validate PARTITION BY dimension references
            for dim in &ws.partition_dims {
                let dim_exists = dimensions.iter().any(|d| d.name.eq_ignore_ascii_case(dim));
                if !dim_exists {
                    let available_dims: Vec<String> =
                        dimensions.iter().map(|d| d.name.clone()).collect();
                    let suggestion = crate::util::suggest_closest(dim, &available_dims);
                    let mut msg = format!(
                        "Window metric '{}': PARTITION BY dimension '{}' not found in semantic view dimensions.",
                        metric.name, dim
                    );
                    if let Some(closest) = suggestion {
                        use std::fmt::Write;
                        let _ = write!(msg, " Did you mean '{closest}'?");
                    }
                    return Err(ParseError {
                        message: msg,
                        position: None,
                    });
                }
            }
            // Validate ORDER BY dimension references
            // Phase 68 B2 / D-08: accept dotted-path qualifier `alias.dim_name`
            // in addition to the bare `dim_name` form (mirrors NAB resolver).
            for ob in &ws.order_by {
                let dim_exists = dimensions.iter().any(|d| {
                    if d.name.eq_ignore_ascii_case(&ob.expr) {
                        return true;
                    }
                    if let Some((alias_part, name_part)) = split_qualified_identifier(&ob.expr) {
                        if let Some(ref src) = d.source_table {
                            return src.eq_ignore_ascii_case(alias_part)
                                && d.name.eq_ignore_ascii_case(name_part);
                        }
                    }
                    false
                });
                if !dim_exists {
                    let available_dims: Vec<String> =
                        dimensions.iter().map(|d| d.name.clone()).collect();
                    let suggestion = crate::util::suggest_closest(&ob.expr, &available_dims);
                    let mut msg = format!(
                        "Window metric '{}': ORDER BY dimension '{}' not found in semantic view dimensions.",
                        metric.name, ob.expr
                    );
                    if let Some(closest) = suggestion {
                        use std::fmt::Write;
                        let _ = write!(msg, " Did you mean '{closest}'?");
                    }
                    return Err(ParseError {
                        message: msg,
                        position: None,
                    });
                }
            }
            // Validate inner metric reference
            let inner_exists = metric_names
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&ws.inner_metric));
            if !inner_exists {
                let suggestion = crate::util::suggest_closest(&ws.inner_metric, &metric_names);
                let mut msg = format!(
                    "Window metric '{}': inner metric '{}' not found in semantic view metrics.",
                    metric.name, ws.inner_metric
                );
                if let Some(closest) = suggestion {
                    use std::fmt::Write;
                    let _ = write!(msg, " Did you mean '{closest}'?");
                }
                return Err(ParseError {
                    message: msg,
                    position: None,
                });
            }
        }
    }

    // Phase 54: Validate materialization references
    // Duplicate name check
    {
        let mut seen_names: Vec<String> = Vec::new();
        for mat in &materializations {
            let lower = mat.name.to_ascii_lowercase();
            if seen_names.iter().any(|n| n == &lower) {
                return Err(ParseError {
                    message: format!("Duplicate materialization name '{}'.", mat.name),
                    position: None,
                });
            }
            seen_names.push(lower);
        }
    }
    // Dimension reference check
    for mat in &materializations {
        for dim_name in &mat.dimensions {
            let dim_exists = dimensions
                .iter()
                .any(|d| d.name.eq_ignore_ascii_case(dim_name));
            if !dim_exists {
                let available_dims: Vec<String> =
                    dimensions.iter().map(|d| d.name.clone()).collect();
                let suggestion = crate::util::suggest_closest(dim_name, &available_dims);
                let mut msg = format!(
                    "Materialization '{}': dimension '{}' not found in semantic view dimensions.",
                    mat.name, dim_name
                );
                if let Some(closest) = suggestion {
                    use std::fmt::Write;
                    let _ = write!(msg, " Did you mean '{closest}'?");
                }
                return Err(ParseError {
                    message: msg,
                    position: None,
                });
            }
        }
        // Metric reference check
        for met_name in &mat.metrics {
            let met_exists = metrics
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(met_name));
            if !met_exists {
                let suggestion = crate::util::suggest_closest(met_name, &metric_names);
                let mut msg = format!(
                    "Materialization '{}': metric '{}' not found in semantic view metrics.",
                    mat.name, met_name
                );
                if let Some(closest) = suggestion {
                    use std::fmt::Write;
                    let _ = write!(msg, " Did you mean '{closest}'?");
                }
                return Err(ParseError {
                    message: msg,
                    position: None,
                });
            }
        }
    }

    Ok(KeywordBody {
        tables,
        relationships,
        facts,
        dimensions,
        metrics,
        materializations,
    })
}

#[cfg(test)]
mod tests {
    use super::annotations::parse_trailing_annotations;
    use super::scan::{find_keyword_ci, find_primary_key};
    use super::*;
    use crate::model::{Cardinality, NullsOrder, SortOrder};

    // -----------------------------------------------------------------------
    // split_at_depth0_commas tests
    // -----------------------------------------------------------------------

    #[test]
    fn split_simple_three_entries() {
        let result = split_at_depth0_commas("a, b, c");
        assert_eq!(result.len(), 3, "Expected 3 entries, got {:?}", result);
        assert_eq!(result[0].1, "a");
        assert_eq!(result[1].1, "b");
        assert_eq!(result[2].1, "c");
    }

    #[test]
    fn split_nested_parens_not_split() {
        // The comma inside SUM(a, b) is at depth 1 — must not split
        let result = split_at_depth0_commas("SUM(a, b), COUNT(*)");
        assert_eq!(result.len(), 2, "Expected 2 entries, got {:?}", result);
        assert_eq!(result[0].1, "SUM(a, b)");
        assert_eq!(result[1].1, "COUNT(*)");
    }

    #[test]
    fn split_string_literal_comma_not_split() {
        // Comma inside single-quoted string must not split
        let result = split_at_depth0_commas("a, 'x, y', b");
        assert_eq!(result.len(), 3, "Expected 3 entries, got {:?}", result);
        assert_eq!(result[0].1, "a");
        assert_eq!(result[1].1, "'x, y'");
        assert_eq!(result[2].1, "b");
    }

    #[test]
    fn split_trailing_comma_discarded() {
        let result = split_at_depth0_commas("a,");
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
        assert_eq!(result[0].1, "a");
    }

    #[test]
    fn split_empty_body() {
        let result = split_at_depth0_commas("");
        assert_eq!(result.len(), 0, "Empty body must produce 0 entries");
    }

    // -----------------------------------------------------------------------
    // find_clause_bounds tests (via parse_keyword_body integration)
    // -----------------------------------------------------------------------

    #[test]
    fn find_clause_bounds_basic_tables_dimensions_metrics() {
        // Smoke test: parsing a well-formed AS body finds all 3 clauses
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result.map(|_| ()));
        let kb = result.unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn find_clause_bounds_with_relationships() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id), c AS customers PRIMARY KEY (id)) RELATIONSHIPS (o_to_c AS o(customer_id) REFERENCES c) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.map(|_| ()));
        let kb = result.unwrap();
        assert_eq!(kb.relationships.len(), 1);
    }

    #[test]
    fn find_clause_bounds_rejects_unknown_keyword() {
        // "TABLSE" is close to "TABLES" — should get "did you mean?" error
        let body = "AS TABLSE (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.x AS x)";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Expected error for unknown keyword TABLSE");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("TABLES") || err.message.contains("TABLSE"),
            "Error should mention TABLES or TABLSE: {}",
            err.message
        );
    }

    #[test]
    fn find_clause_bounds_rejects_missing_tables() {
        let body = "AS DIMENSIONS (o.x AS x) METRICS (o.y AS SUM(y))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Expected error for missing TABLES clause");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("TABLES"),
            "Error should mention TABLES: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // T-2 (code-review 2026-07-11): the duplicate-clause-keyword branch in
    // clause_bounds.rs had zero test coverage. Pin the message and that the
    // caret points at the SECOND occurrence.
    // -----------------------------------------------------------------------

    #[test]
    fn find_clause_bounds_rejects_duplicate_dimensions() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.a AS o.a) DIMENSIONS (o.b AS o.b)";
        let err = parse_keyword_body(body, 0).unwrap_err();
        assert!(
            err.message
                .contains("Duplicate clause keyword 'DIMENSIONS'"),
            "got: {}",
            err.message
        );
        // Caret must point at the second DIMENSIONS keyword.
        assert_eq!(err.position, Some(body.rfind("DIMENSIONS").unwrap()));
    }

    #[test]
    fn find_clause_bounds_rejects_duplicate_tables() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) TABLES (c AS customers PRIMARY KEY (id)) DIMENSIONS (o.a AS o.a)";
        let err = parse_keyword_body(body, 0).unwrap_err();
        assert!(
            err.message.contains("Duplicate clause keyword 'TABLES'"),
            "got: {}",
            err.message
        );
        assert_eq!(err.position, Some(body.rfind("TABLES").unwrap()));
    }

    // -----------------------------------------------------------------------
    // T-3 (code-review 2026-07-11): pin the behaviour of empty clause bodies.
    // TABLES () satisfies the presence check but yields zero tables — the
    // "at least one of DIMENSIONS/METRICS" and downstream validation decide
    // the outcome. These tests document what actually happens so a future
    // refactor can't silently change it.
    // -----------------------------------------------------------------------

    #[test]
    fn empty_dimensions_and_metrics_both_present_is_ok() {
        // Both clauses present but empty: the "at least one of D/M" rule is
        // about clause PRESENCE, so this parses to an empty-dims/empty-mets
        // body (a dimensionless, metricless view — degenerate but legal at
        // the parse layer).
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS () METRICS ()";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert!(kb.dimensions.is_empty());
        assert!(kb.metrics.is_empty());
    }

    #[test]
    fn empty_tables_clause_parses_to_zero_tables() {
        // TABLES () satisfies the presence check; downstream expansion is
        // what rejects a zero-table view, not the clause scanner.
        let body = "AS TABLES () DIMENSIONS (o.a AS o.a)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(kb.tables.is_empty());
        assert_eq!(kb.dimensions.len(), 1);
    }

    #[test]
    fn empty_metrics_only_still_requires_tables_and_d_or_m_by_presence() {
        // METRICS () alone (no DIMENSIONS) satisfies the presence rule.
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS ()";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(kb.metrics.is_empty());
        // Neither DIMENSIONS nor METRICS present at all is the actual error.
        let body_none = "AS TABLES (o AS orders PRIMARY KEY (id))";
        let err = parse_keyword_body(body_none, 0).unwrap_err();
        assert!(
            err.message
                .contains("At least one of 'DIMENSIONS' or 'METRICS' is required"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn empty_materializations_clause_parses() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.a AS o.a) MATERIALIZATIONS ()";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(kb.materializations.is_empty());
    }

    // -----------------------------------------------------------------------
    // T-4 (code-review 2026-07-11): comments INSIDE the AS body work only by
    // construction (whole-query blanking runs before the body parser). Pin
    // that block comments between clauses and inside clause parens are inert,
    // and that a typo after an in-body comment still carets correctly.
    // -----------------------------------------------------------------------

    #[test]
    fn block_comment_between_clauses_is_inert() {
        // The parse layer sees comments already blanked to spaces (length
        // preserving), so this must parse identically to the comment-free
        // form. NOTE: parse_keyword_body itself does not blank — callers
        // (plan_rewrite) do — so exercise the blank-then-parse pipeline.
        let raw = "AS TABLES (o AS orders PRIMARY KEY (id)) /* between */ DIMENSIONS (o.a AS o.a)";
        let blanked = crate::util::blank_sql_comments(raw);
        let kb = parse_keyword_body(&blanked, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
    }

    #[test]
    fn block_comment_inside_clause_parens_is_inert() {
        let raw =
            "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.a AS o.a /* c, ) inside */)";
        let blanked = crate::util::blank_sql_comments(raw);
        let kb = parse_keyword_body(&blanked, 0).unwrap();
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.dimensions[0].name, "a");
    }

    #[test]
    fn caret_after_in_body_comment_is_honest() {
        // blank_sql_comments is length-preserving, so a typo AFTER an in-body
        // comment must still caret at its true byte offset. Here `TABLSE`
        // follows a block comment; its reported position must equal the raw
        // byte index of `TABLSE`, not be shifted by the comment's length.
        let raw =
            "AS /* leading note */ TABLSE (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.a AS o.a)";
        let blanked = crate::util::blank_sql_comments(raw);
        let err = parse_keyword_body(&blanked, 0).unwrap_err();
        assert!(err.message.contains("TABLSE") || err.message.contains("TABLES"));
        assert_eq!(
            err.position,
            Some(raw.find("TABLSE").unwrap()),
            "caret must point at the raw offset of TABLSE despite the preceding comment"
        );
    }

    #[test]
    fn find_clause_bounds_nested_pk_not_confused() {
        // PRIMARY KEY (...) creates nested depth — closing paren must not end TABLES clause early
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (o_orderkey, o_custkey)) DIMENSIONS (o.x AS x)";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.map(|_| ()));
        let kb = result.unwrap();
        assert_eq!(kb.tables[0].pk_columns.len(), 2);
    }

    // -----------------------------------------------------------------------
    // parse_tables_clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_tables_single_pk() {
        let result = parse_tables_clause("o AS orders PRIMARY KEY (o_orderkey)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert_eq!(result[0].pk_columns, vec!["o_orderkey"]);
    }

    #[test]
    fn parse_tables_schema_qualified() {
        let result = parse_tables_clause("o AS main.orders PRIMARY KEY (o_orderkey)", 0).unwrap();
        assert_eq!(result[0].table, "main.orders");
        assert_eq!(result[0].alias, "o");
    }

    #[test]
    fn parse_tables_composite_pk() {
        let result =
            parse_tables_clause("l AS lineitem PRIMARY KEY (l_orderkey, l_linenumber)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["l_orderkey", "l_linenumber"]);
    }

    #[test]
    fn parse_tables_error_missing_as() {
        let result = parse_tables_clause("o orders PRIMARY KEY (id)", 0);
        assert!(result.is_err(), "Expected error for missing AS");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("AS"),
            "Error should mention AS: {}",
            err.message
        );
    }

    #[test]
    fn parse_tables_without_primary_key_is_valid() {
        // Phase 33: PRIMARY KEY is optional (fact tables)
        let result = parse_tables_clause("o AS orders", 0).unwrap();
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert!(result[0].pk_columns.is_empty());
    }

    // -----------------------------------------------------------------------
    // Phase 67 Plan 02 / TECH-DEBT #24: identifier-aware tokenisation of the
    // source-table-name slot. Quoted identifiers with internal whitespace —
    // including ones that contain the literal `PRIMARY KEY` substring — must
    // survive intact through `parse_single_table_entry`.
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_single_table_entry_quoted_with_internal_whitespace() {
        // Quoted name with embedded space; trailing PRIMARY KEY clause.
        let result = parse_tables_clause("o AS \"my orders\" PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "\"my orders\"");
        assert_eq!(result[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn test_parse_single_table_entry_quoted_containing_primary_key_substring() {
        // Canonical TECH-DEBT #24 bug: a quoted source-table name that
        // contains the literal `PRIMARY KEY` substring must NOT be split by
        // the case-insensitive PRIMARY-KEY substring search. The fix is to
        // capture the identifier FIRST using `find_identifier_end`, then run
        // PRIMARY KEY detection only on the post-name slice.
        let result =
            parse_tables_clause("o AS \"weird PRIMARY KEY name\" PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "\"weird PRIMARY KEY name\"");
        assert_eq!(result[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn test_parse_single_table_entry_3part_quoted_fqn_with_whitespace() {
        // 3-part fully-qualified name with internal whitespace in two
        // segments. The dot-separated walk in the new identifier-aware
        // tokeniser must traverse all three segments and preserve the
        // verbatim byte sequence.
        let result =
            parse_tables_clause("o AS \"my db\".\"schema\".\"my table\" PRIMARY KEY (id)", 0)
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "\"my db\".\"schema\".\"my table\"");
        assert_eq!(result[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn test_parse_single_table_entry_regression_no_whitespace() {
        // Regression baseline for the happy path: unquoted dot-qualified
        // name with no whitespace anywhere in the source-table slot.
        // Byte-for-byte identical to the pre-fix `parse_tables_schema_qualified`
        // assertion shape.
        let result = parse_tables_clause("o AS schema.t PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "schema.t");
        assert_eq!(result[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn test_parse_single_table_entry_quoted_with_unique_no_pk() {
        // No PK, trailing UNIQUE clause after a quoted-with-whitespace name.
        // Exercises the no-PK branch of the post-name keyword search.
        let result = parse_tables_clause("f AS \"fact stage\" UNIQUE (email)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "f");
        assert_eq!(result[0].table, "\"fact stage\"");
        assert!(result[0].pk_columns.is_empty());
        assert_eq!(
            result[0].unique_constraints,
            vec![vec!["email".to_string()]]
        );
    }

    // -----------------------------------------------------------------------
    // P-1 (code-review 2026-07-11): text between the source-table name and a
    // PRIMARY KEY / UNIQUE constraint must be rejected, not silently
    // discarded. The anywhere-scan for the constraint keywords previously
    // dropped everything before the match — including a naturally-misplaced
    // COMMENT annotation (silent data loss).
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_single_table_entry_comment_before_pk_rejected() {
        let err = parse_tables_clause(
            "o AS orders COMMENT = 'load-bearing doc' PRIMARY KEY (id)",
            0,
        )
        .unwrap_err();
        assert!(
            err.message
                .contains("between source table name and PRIMARY KEY"),
            "got: {}",
            err.message
        );
        assert!(err.position.is_some(), "error must carry a position");
    }

    #[test]
    fn test_parse_single_table_entry_junk_before_pk_rejected() {
        let err = parse_tables_clause("o AS orders banana PRIMARY KEY (id)", 0).unwrap_err();
        assert!(
            err.message.contains("Unexpected text 'banana'"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_parse_single_table_entry_junk_between_pk_and_unique_rejected() {
        let entry = "o AS orders PRIMARY KEY (id) junk UNIQUE (email)";
        let err = parse_tables_clause(entry, 0).unwrap_err();
        assert!(
            err.message.contains("before UNIQUE"),
            "got: {}",
            err.message
        );
        // Caret must land on the junk token, not the entry start (Copilot
        // review, PR #71).
        assert_eq!(
            err.position,
            Some(entry.find("junk").unwrap()),
            "position should point at 'junk'"
        );
    }

    #[test]
    fn test_parse_single_table_entry_junk_before_pk_caret_position() {
        // Sibling assertion for the PRIMARY KEY guard's caret.
        let entry = "o AS orders banana PRIMARY KEY (id)";
        let err = parse_tables_clause(entry, 0).unwrap_err();
        assert_eq!(
            err.position,
            Some(entry.find("banana").unwrap()),
            "position should point at 'banana'"
        );
    }

    #[test]
    fn test_parse_single_table_entry_annotation_before_unique_rejected() {
        let err = parse_tables_clause("o AS orders COMMENT = 'doc' UNIQUE (email)", 0).unwrap_err();
        assert!(
            err.message.contains("before UNIQUE"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_parse_single_table_entry_full_form_still_parses() {
        // The complete legal ordering is unaffected by the P-1 guards.
        let result = parse_tables_clause(
            "o AS orders PRIMARY KEY (id) UNIQUE (email) COMMENT = 'doc' WITH SYNONYMS = ('ord')",
            0,
        )
        .unwrap();
        assert_eq!(result[0].pk_columns, vec!["id"]);
        assert_eq!(
            result[0].unique_constraints,
            vec![vec!["email".to_string()]]
        );
        assert_eq!(result[0].comment.as_deref(), Some("doc"));
        assert_eq!(result[0].synonyms, vec!["ord".to_string()]);
    }

    #[test]
    fn test_parse_single_table_entry_unique_inside_comment_string_ok() {
        // The constraint scans are quote-aware: keyword text INSIDE the
        // annotation string must not trip the P-1 guards.
        let result = parse_tables_clause(
            "o AS orders PRIMARY KEY (id) COMMENT = 'has UNIQUE and PRIMARY KEY inside'",
            0,
        )
        .unwrap();
        assert_eq!(
            result[0].comment.as_deref(),
            Some("has UNIQUE and PRIMARY KEY inside")
        );
    }

    // -----------------------------------------------------------------------
    // Phase 68 A1 (D-03): bare reserved keywords (PRIMARY, UNIQUE, FOREIGN,
    // REFERENCES, NOT) appearing in the source-table-name slot must surface
    // the pre-Phase-67 literal error message. The keyword set is authoritative
    // per Phase 68 CONTEXT.md D-03; the REVIEW.md draft list is informational.
    // -----------------------------------------------------------------------
    const A1_EXPECTED_MESSAGE: &str =
        "Missing physical table name after AS for alias 'o' in TABLES clause.";

    #[test]
    fn test_parse_single_table_entry_reserved_keyword_after_as_primary() {
        let err = parse_tables_clause("o AS PRIMARY KEY (id)", 0).unwrap_err();
        assert_eq!(err.message, A1_EXPECTED_MESSAGE);
    }

    #[test]
    fn test_parse_single_table_entry_reserved_keyword_after_as_unique() {
        let err = parse_tables_clause("o AS UNIQUE (id)", 0).unwrap_err();
        assert_eq!(err.message, A1_EXPECTED_MESSAGE);
    }

    #[test]
    fn test_parse_single_table_entry_reserved_keyword_after_as_foreign() {
        let err = parse_tables_clause("o AS FOREIGN KEY (id)", 0).unwrap_err();
        assert_eq!(err.message, A1_EXPECTED_MESSAGE);
    }

    #[test]
    fn test_parse_single_table_entry_reserved_keyword_after_as_references() {
        let err = parse_tables_clause("o AS REFERENCES other(id)", 0).unwrap_err();
        assert_eq!(err.message, A1_EXPECTED_MESSAGE);
    }

    #[test]
    fn test_parse_single_table_entry_reserved_keyword_after_as_not() {
        let err = parse_tables_clause("o AS NOT NULL", 0).unwrap_err();
        assert_eq!(err.message, A1_EXPECTED_MESSAGE);
    }

    #[test]
    fn test_parse_single_table_entry_reserved_keyword_after_as_lowercase() {
        // Guard is case-insensitive — `primary` must trigger it just like
        // `PRIMARY` (Phase 68 D-03).
        let err = parse_tables_clause("o AS primary KEY (id)", 0).unwrap_err();
        assert_eq!(err.message, A1_EXPECTED_MESSAGE);
    }

    // -----------------------------------------------------------------------
    // Phase 68 A4: unterminated quoted source-table identifier in TABLES
    // clause must surface a structured ParseError, never silently flow
    // through as a malformed name. Doubled-quote `""` is an escape and must
    // NOT trip the balanced-quote check.
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_single_table_entry_unterminated_quote() {
        let err = parse_tables_clause("o AS \"unclosed", 0).unwrap_err();
        assert!(
            err.message.contains("Unterminated quoted identifier"),
            "expected unterminated-quote error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_parse_single_table_entry_quoted_with_doubled_escape_balanced() {
        // `"a""b"` is balanced — doubled-quote is an escape inside the quoted
        // region, so this must parse successfully.
        let result = parse_tables_clause("o AS \"a\"\"b\" PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "\"a\"\"b\"");
        assert_eq!(result[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn test_parse_single_table_entry_unbalanced_after_doubled_escape() {
        // `"a""b` opens, escapes, then never closes — odd unescaped quote
        // count, must be rejected.
        let err = parse_tables_clause("o AS \"a\"\"b PRIMARY KEY (id)", 0).unwrap_err();
        assert!(
            err.message.contains("Unterminated quoted identifier"),
            "expected unterminated-quote error, got: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // Phase 68 A7: `find_primary_key`'s three word-boundary checks align with
    // `find_unique`'s `_`-exclusion pattern. Identifiers like `my_PRIMARY`
    // (prefix) or `PRIMARY KEY_extra` (suffix) must NOT match.
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_primary_key_word_boundary_underscore() {
        // Underscore-prefixed: `_PRIMARY` should not match because `_` is now
        // excluded from the before-boundary set.
        assert!(find_primary_key(&"my_PRIMARY KEY".to_ascii_uppercase()).is_none());
        // Underscore-suffixed on KEY: `KEY_extra` should not match.
        assert!(find_primary_key(&"PRIMARY KEY_extra".to_ascii_uppercase()).is_none());
    }

    // -----------------------------------------------------------------------
    // PA-1 / PA-2 regressions (code-review 2026-07-02): byte-indexed keyword
    // scanning panicked mid-codepoint on non-ASCII input, and the local
    // single-quote extractor Latin-1-ized non-ASCII COMMENT / SYNONYMS
    // payloads (`'café'` → `cafÃ©`).
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_keyword_ci_non_ascii_no_panic() {
        // `é` uppercases to `É` (both 2 bytes); scanning must not panic and
        // must still find the keyword after the multi-byte run.
        let upper = "ÉÉÉ AS x".to_string();
        assert_eq!(find_keyword_ci(&upper, "AS"), Some(7));
        // No match at all — pure multi-byte text.
        assert_eq!(find_keyword_ci("東京東京", "AS"), None);
    }

    #[test]
    fn test_find_primary_key_non_ascii_no_panic() {
        assert!(find_primary_key("ΩΩ NO PK HERE Ω").is_none());
        let upper = "\"CAFÉ\" PRIMARY KEY (ID)".to_string();
        let (start, end) = find_primary_key(&upper).expect("PRIMARY KEY found");
        assert_eq!(&upper[start..end], "PRIMARY KEY");
    }

    #[test]
    fn test_comment_annotation_non_ascii_payload_survives() {
        // PA-2: the pre-fix extractor stored 'café et plus' as mojibake.
        let (expr, ann) =
            parse_trailing_annotations("SUM(o.amount) COMMENT = 'café et plus'").unwrap();
        assert_eq!(expr, "SUM(o.amount)");
        assert_eq!(ann.comment.as_deref(), Some("café et plus"));
    }

    #[test]
    fn test_synonyms_annotation_non_ascii_payload_survives() {
        let (expr, ann) =
            parse_trailing_annotations("o.city WITH SYNONYMS = ('ciudad', 'stadt', '都市')")
                .unwrap();
        assert_eq!(expr, "o.city");
        assert_eq!(ann.synonyms, vec!["ciudad", "stadt", "都市"]);
    }

    #[test]
    fn test_annotation_scan_non_ascii_expression_no_panic() {
        // Multi-byte chars ahead of the annotation keywords exercise the
        // depth-0 scanner's byte loop.
        let (expr, ann) = parse_trailing_annotations("concat(city, ' – ') COMMENT = 'ok'").unwrap();
        assert_eq!(expr, "concat(city, ' – ')");
        assert_eq!(ann.comment.as_deref(), Some("ok"));
    }

    // -----------------------------------------------------------------------
    // P-2 (code-review 2026-07-11): the annotation region must be tiled
    // exactly — duplicate clauses, malformed clauses, and trailing junk are
    // rejected instead of being silently dropped/accepted.
    // -----------------------------------------------------------------------

    #[test]
    fn test_annotation_both_orders_still_parse() {
        // Regression: valid single-clause and both-order forms are unaffected.
        let (_, a) = parse_trailing_annotations("x COMMENT = 'c' WITH SYNONYMS = ('s')").unwrap();
        assert_eq!(a.comment.as_deref(), Some("c"));
        assert_eq!(a.synonyms, vec!["s"]);
        let (_, b) = parse_trailing_annotations("x WITH SYNONYMS = ('s') COMMENT = 'c'").unwrap();
        assert_eq!(b.comment.as_deref(), Some("c"));
        assert_eq!(b.synonyms, vec!["s"]);
    }

    #[test]
    fn test_annotation_duplicate_comment_rejected() {
        // Previously the second COMMENT was silently dropped.
        let err = parse_trailing_annotations("x COMMENT = 'a' COMMENT = 'b'").unwrap_err();
        assert!(
            err.message.contains("Duplicate COMMENT"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_annotation_duplicate_synonyms_rejected() {
        let err = parse_trailing_annotations("x WITH SYNONYMS = ('a') WITH SYNONYMS = ('b')")
            .unwrap_err();
        assert!(
            err.message.contains("Duplicate WITH SYNONYMS"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_annotation_trailing_garbage_rejected() {
        // Previously `banana` was silently accepted and discarded.
        let err = parse_trailing_annotations("x COMMENT = 'a' banana").unwrap_err();
        assert!(
            err.message.contains("Unexpected text in annotations"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_annotation_keyword_word_boundary_preserved() {
        // A column-ish token that merely starts with COMMENT must not be
        // mistaken for the keyword — it stays part of the expression.
        let (expr, ann) = parse_trailing_annotations("commentary_col").unwrap();
        assert_eq!(expr, "commentary_col");
        assert!(ann.comment.is_none() && ann.synonyms.is_empty());
    }

    #[test]
    fn test_annotation_with_without_synonyms_rejected() {
        // A second WITH clause that isn't WITH SYNONYMS is an error, not junk.
        let err = parse_trailing_annotations("x COMMENT = 'a' WITH FOO").unwrap_err();
        assert!(
            err.message.contains("Expected SYNONYMS after WITH"),
            "got: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // PA-3 (code-review 2026-07-02): keyword scanners must not match inside
    // string literals. Pre-fix, a COMMENT payload mentioning PRIMARY KEY
    // fabricated pk_columns from comment text and discarded the comment.
    // -----------------------------------------------------------------------

    #[test]
    fn test_primary_key_inside_comment_string_not_fabricated() {
        let result =
            parse_tables_clause("o AS orders COMMENT = 'the PRIMARY KEY (id) lives here'", 0)
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pk_columns, Vec::<String>::new());
        assert_eq!(
            result[0].comment.as_deref(),
            Some("the PRIMARY KEY (id) lives here")
        );
    }

    #[test]
    fn test_unique_inside_comment_string_not_fabricated() {
        let result = parse_tables_clause(
            "o AS orders PRIMARY KEY (id) COMMENT = 'a UNIQUE (x) mention'",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pk_columns, vec!["id"]);
        assert!(result[0].unique_constraints.is_empty());
        assert_eq!(result[0].comment.as_deref(), Some("a UNIQUE (x) mention"));
    }

    // -----------------------------------------------------------------------
    // PA-9 (code-review 2026-07-02): table-level COMMENT / WITH SYNONYMS on a
    // table with no PK/UNIQUE used to be silently dropped (remainder was
    // hard-set to ""), and trailing junk was silently ignored.
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_pk_table_comment_and_synonyms_preserved() {
        let result = parse_tables_clause(
            "li AS line_items COMMENT = 'fact rows' WITH SYNONYMS = ('items', 'lines')",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pk_columns, Vec::<String>::new());
        assert_eq!(result[0].comment.as_deref(), Some("fact rows"));
        assert_eq!(result[0].synonyms, vec!["items", "lines"]);
    }

    #[test]
    fn test_table_entry_trailing_garbage_errors() {
        let err = parse_tables_clause("o AS orders garbage COMMENT = 'x'", 0).unwrap_err();
        assert!(
            err.message.contains("Unexpected text"),
            "expected trailing-garbage error, got: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // PA-6 (code-review 2026-07-02): depth-0 scanners must honour
    // double-quoted identifiers.
    // -----------------------------------------------------------------------

    #[test]
    fn test_split_commas_ignores_comma_inside_quoted_ident() {
        let entries = split_at_depth0_commas("o.x AS o.\"a,b\", o.y AS o.c");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, "o.x AS o.\"a,b\"");
        assert_eq!(entries[1].1, "o.y AS o.c");
    }

    #[test]
    fn test_quoted_ident_with_paren_does_not_close_clause() {
        // `"tbl)x"` inside TABLES must not close the TABLES clause early.
        let def = parse_keyword_body(
            "AS TABLES (o AS \"tbl)x\" PRIMARY KEY (id)) DIMENSIONS (o.d AS o.c)",
            0,
        )
        .unwrap();
        assert_eq!(def.tables.len(), 1);
        assert_eq!(def.tables[0].table, "\"tbl)x\"");
        assert_eq!(def.dimensions.len(), 1);
    }

    #[test]
    fn test_dot_inside_quoted_name_not_a_qualifier() {
        // Dimension bare name `"a.b"` — the inner dot is not a separator.
        let entries = parse_qualified_entries("o.\"a.b\" AS o.c", 0, false, "dimensions").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_alias, "o");
        assert_eq!(entries[0].name, "\"a.b\"");
    }

    #[test]
    fn test_keyword_not_matched_before_non_ascii_continuation() {
        // PR #50 review: keyword boundary checks treated non-ASCII bytes as
        // boundaries, so COMMENT matched inside the identifier `commenté`
        // and the annotation scanner truncated the expression.
        let entries = parse_qualified_entries("o.x AS o.commenté", 0, false, "dimensions").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].expr, "o.commenté");
        assert_eq!(entries[0].comment, None);
    }

    #[test]
    fn test_quoted_comment_column_usable_in_expression() {
        // PA-9 companion: a column literally named `comment` is usable at
        // depth 0 when quoted — the annotation scanner must not treat the
        // quoted identifier as the COMMENT keyword.
        let entries =
            parse_qualified_entries("o.note AS o.\"comment\"", 0, false, "dimensions").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].expr, "o.\"comment\"");
        assert_eq!(entries[0].comment, None);
    }

    #[test]
    fn test_unterminated_quote_in_dimension_entry_errors() {
        let err =
            parse_qualified_entries("o.x AS o.\"unclosed", 0, false, "dimensions").unwrap_err();
        assert!(
            err.message.contains("Unterminated quoted identifier"),
            "got: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // Phase 68 A5: mixed bare/quoted dot-qualified source-table names must
    // parse correctly. The dot-walk inside `find_identifier_end` already
    // handles this case (its `fqn_with_quoted_parts_runs_to_whitespace`
    // doctest covers it at the helper level); these tests pin the
    // parse_tables_clause contract end-to-end.
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_single_table_entry_mixed_quoted_and_bare() {
        // Bare schema segment followed by quoted-with-whitespace table segment.
        let result = parse_tables_clause("o AS staging.\"my orders\" PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "staging.\"my orders\"");
        assert_eq!(result[0].pk_columns, vec!["id"]);

        // Symmetric case: quoted-with-whitespace database segment, then bare
        // schema + bare table.
        let result = parse_tables_clause("o AS \"my db\".sch.t PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "\"my db\".sch.t");
        assert_eq!(result[0].pk_columns, vec!["id"]);
    }

    // -----------------------------------------------------------------------
    // Phase 68 Plan 03 (B1) / TECH-DEBT #25: identifier-aware tokenisation of
    // the dim_name slot inside `NON ADDITIVE BY (...)`. Quoted identifiers with
    // internal whitespace AND dotted paths (`table.col`, D-08) must survive
    // intact through `parse_non_additive_dims`. `parse_non_additive_dims` is
    // private — exercise via the public `parse_metrics_clause` entry.
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_non_additive_dims_quoted_identifier_with_whitespace() {
        // Quoted dim_name with embedded space. DESC defaults to NULLS FIRST.
        let result = parse_metrics_clause(
            "a.balance NON ADDITIVE BY (\"my dim\" DESC) AS SUM(a.balance)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let na_vec = &result[0].non_additive_by;
        assert_eq!(na_vec.len(), 1);
        let na = &na_vec[0];
        assert_eq!(na.dimension, "\"my dim\"");
        assert_eq!(na.order, SortOrder::Desc);
        assert_eq!(na.nulls, NullsOrder::First); // DESC default
    }

    #[test]
    fn test_parse_non_additive_dims_dotted_path() {
        // Dotted-path dim_name (D-08 contract extension): `o."my dim"` must be
        // captured as a single identifier including the dot. Two-entry clause
        // confirms comma splitting still works.
        let result = parse_metrics_clause(
            "a.balance NON ADDITIVE BY (o.\"my dim\" ASC NULLS LAST, c2 DESC) AS SUM(a.balance)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let na_vec = &result[0].non_additive_by;
        assert_eq!(na_vec.len(), 2);

        let na0 = &na_vec[0];
        assert_eq!(na0.dimension, "o.\"my dim\"");
        assert_eq!(na0.order, SortOrder::Asc);
        assert_eq!(na0.nulls, NullsOrder::Last);

        let na1 = &na_vec[1];
        assert_eq!(na1.dimension, "c2");
        assert_eq!(na1.order, SortOrder::Desc);
        assert_eq!(na1.nulls, NullsOrder::First); // DESC default
    }

    #[test]
    fn test_keyword_boundaries_reject_underscore_and_non_ascii_continuation() {
        // PR #50 review: post-keyword boundary checks after BY / EXCLUDING
        // accepted `_` and non-ASCII bytes as boundaries, so `BY_foo` /
        // `EXCLUDING_foo` mis-tokenized as the keyword ending early. Such
        // identifiers must NOT activate the keyword.
        let r = parse_metrics_clause("a.bal NON ADDITIVE BY_x (d) AS SUM(a.bal)", 0);
        if let Ok(v) = r {
            assert!(
                v[0].non_additive_by.is_empty(),
                "BY_x must not match the NON ADDITIVE BY keyword"
            );
        }

        let r = parse_metrics_clause(
            "s.r AS SUM(t) OVER (PARTITION BY_x ORDER BY d ASC NULLS LAST)",
            0,
        );
        if let Ok(v) = r {
            let ws = v[0].window_spec.as_ref().expect("window spec");
            assert!(
                ws.partition_dims.is_empty(),
                "PARTITION BY_x must not match PARTITION BY: {:?}",
                ws.partition_dims
            );
        }

        let r = parse_metrics_clause(
            "s.r AS SUM(t) OVER (PARTITION BY EXCLUDING_x ORDER BY d ASC NULLS LAST)",
            0,
        );
        if let Ok(v) = r {
            let ws = v[0].window_spec.as_ref().expect("window spec");
            assert!(
                ws.excluding_dims.is_empty(),
                "EXCLUDING_x must not match EXCLUDING: {:?}",
                ws.excluding_dims
            );
        }

        // The legitimate forms keep working.
        let v = parse_metrics_clause(
            "s.r AS SUM(t) OVER (PARTITION BY EXCLUDING region ORDER BY d ASC NULLS LAST)",
            0,
        )
        .unwrap();
        let ws = v[0].window_spec.as_ref().expect("window spec");
        assert_eq!(ws.excluding_dims, vec!["region"]);
    }

    #[test]
    fn test_as_keyword_requires_boundary_in_tables_and_materializations() {
        // PR #50 review: `AS` was matched as a raw 2-byte prefix, so
        // `ASorders` / `ASx` were treated as the AS keyword ending early.
        let err = parse_tables_clause("o ASorders PRIMARY KEY (id)", 0).unwrap_err();
        assert!(
            err.message.contains("Expected 'AS'"),
            "got: {}",
            err.message
        );
        let err = parse_materializations_clause("m1 ASx (TABLE t, DIMENSIONS (d))", 0).unwrap_err();
        assert!(
            err.message.contains("Expected 'AS'"),
            "got: {}",
            err.message
        );
        // Punctuation stays a legal boundary: AS"quoted" and AS( work.
        let result = parse_tables_clause("o AS\"my tbl\" PRIMARY KEY (id)", 0).unwrap();
        assert_eq!(result[0].table, "\"my tbl\"");
        let result = parse_materializations_clause("m1 AS(TABLE t, DIMENSIONS (d))", 0).unwrap();
        assert_eq!(result[0].table, "t");
    }

    #[test]
    fn test_over_clause_error_position_is_expression_relative() {
        // PR #50 review: OVER-clause errors were based at the entry start,
        // so carets pointed at the metric name instead of the expression.
        // Entry: "s.r AS SUM(t) OVER bad" — expr starts at byte 7, OVER at
        // byte 7 within the expr, error points just past "OVER" (byte 18).
        let err = parse_metrics_clause("s.r AS SUM(t) OVER bad", 0).unwrap_err();
        assert!(
            err.message.contains("Expected '(' after OVER"),
            "got: {}",
            err.message
        );
        assert_eq!(err.position, Some(18), "caret must sit after OVER");
    }

    #[test]
    fn test_over_partition_and_order_by_with_interior_whitespace() {
        // PR #50 review: the remainder after PARTITION BY dims was located
        // by len() subtraction over a trim()med slice. Exercise the full
        // PARTITION BY -> ORDER BY -> frame path with generous interior
        // whitespace so ORDER BY / frame detection must survive the
        // remainder-offset computation.
        let result = parse_metrics_clause(
            "s.r AS SUM(qty) OVER (PARTITION BY region   ORDER BY d ASC NULLS LAST   ROWS BETWEEN 1 PRECEDING AND CURRENT ROW)",
            0,
        )
        .unwrap();
        let ws = result[0].window_spec.as_ref().expect("window spec");
        assert_eq!(ws.partition_dims, vec!["region"]);
        assert_eq!(ws.order_by.len(), 1, "ORDER BY must survive: {ws:?}");
        assert_eq!(ws.order_by[0].expr, "d");
        assert_eq!(
            ws.frame_clause.as_deref(),
            Some("ROWS BETWEEN 1 PRECEDING AND CURRENT ROW")
        );

        // EXCLUDING branch, same shape.
        let result = parse_metrics_clause(
            "s.r AS SUM(qty) OVER (PARTITION BY EXCLUDING region   ORDER BY d DESC)",
            0,
        )
        .unwrap();
        let ws = result[0].window_spec.as_ref().expect("window spec");
        assert_eq!(ws.excluding_dims, vec!["region"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "d");
    }

    // -----------------------------------------------------------------------
    // P-3 (code-review 2026-07-11): the OVER-clause parser must not silently
    // degrade malformed content. `ORDER` without an adjacent `BY` previously
    // became the frame clause; junk between ORDER and BY was skipped; any
    // residue was stored verbatim as a frame clause.
    // -----------------------------------------------------------------------

    #[test]
    fn test_over_order_without_by_rejected() {
        let err = parse_metrics_clause("s.r AS SUM(t) OVER (PARTITION BY region ORDER d)", 0)
            .unwrap_err();
        assert!(
            err.message.contains("Expected BY immediately after ORDER"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_over_order_junk_before_by_rejected() {
        let err = parse_metrics_clause(
            "s.r AS SUM(t) OVER (PARTITION BY region ORDER banana BY d)",
            0,
        )
        .unwrap_err();
        assert!(
            err.message.contains("Expected BY immediately after ORDER"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_over_junk_content_rejected_as_frame() {
        let err = parse_metrics_clause("s.r AS SUM(t) OVER (banana)", 0).unwrap_err();
        assert!(
            err.message
                .contains("Expected frame clause starting with ROWS, RANGE, or GROUPS"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_over_order_by_frame_keyword_name_rejected() {
        // An unquoted reference named like a frame keyword is claimed by
        // find_frame_start, leaving zero ORDER BY entries — must error, not
        // silently produce an orderless window with a bogus frame.
        let err = parse_metrics_clause("s.r AS SUM(t) OVER (ORDER BY range)", 0).unwrap_err();
        assert!(
            err.message
                .contains("Expected column reference after ORDER BY"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_over_frame_only_still_parses() {
        let result = parse_metrics_clause(
            "s.r AS SUM(t) OVER (ROWS BETWEEN 1 PRECEDING AND CURRENT ROW)",
            0,
        )
        .unwrap();
        let ws = result[0].window_spec.as_ref().expect("window spec");
        assert!(ws.order_by.is_empty());
        assert_eq!(
            ws.frame_clause.as_deref(),
            Some("ROWS BETWEEN 1 PRECEDING AND CURRENT ROW")
        );
    }

    #[test]
    fn test_non_additive_by_flexible_spacing() {
        // PA-10: the keyword offset was hardcoded as 16 ("NON ADDITIVE BY"
        // + one space), rejecting the no-space `BY(d)` form and extra
        // inter-keyword whitespace.
        for entry in [
            "a.balance NON ADDITIVE BY(report_date) AS SUM(a.balance)",
            "a.balance NON  ADDITIVE   BY (report_date) AS SUM(a.balance)",
        ] {
            let result = parse_metrics_clause(entry, 0)
                .unwrap_or_else(|e| panic!("{entry} failed: {}", e.message));
            assert_eq!(result.len(), 1);
            let na_vec = &result[0].non_additive_by;
            assert_eq!(na_vec.len(), 1, "for {entry}");
            assert_eq!(na_vec[0].dimension, "report_date");
        }
    }

    #[test]
    fn test_parse_non_additive_dims_unterminated_quote() {
        // Unterminated quoted dim_name must surface a structured ParseError
        // with the expected wording. Mirrors the A4 TABLES-clause contract.
        let err = parse_metrics_clause(
            "a.balance NON ADDITIVE BY (\"unclosed DESC) AS SUM(a.balance)",
            0,
        )
        .unwrap_err();
        assert!(
            err.message.contains("Unterminated quoted identifier"),
            "Expected unterminated-quote error, got: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // Phase 68 Plan 03 (B2) / TECH-DEBT #25: identifier-aware tokenisation of
    // the OVER ORDER BY column-reference slot. Quoted identifiers with
    // internal whitespace AND dotted paths (`table.col`, D-08) must survive
    // intact through `parse_over_content` / `parse_window_over_clause`.
    // `parse_window_spec` / `parse_over_content` is private — exercise via the
    // public `parse_metrics_clause` entry which routes a window-metric expression
    // through `parse_window_over_clause`. The parsed `WindowSpec` lives in the
    // `ParsedMetric.window_spec` field.
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_window_spec_quoted_order_by() {
        // Quoted identifier with embedded space in OVER ORDER BY entry.
        let result = parse_metrics_clause(
            "s.running AS AVG(qty) OVER (PARTITION BY EXCLUDING r ORDER BY \"order date\" ASC NULLS LAST)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0]
            .window_spec
            .as_ref()
            .expect("window_spec must be Some");
        assert_eq!(ws.order_by.len(), 1);
        let ob = &ws.order_by[0];
        assert_eq!(ob.expr, "\"order date\"");
        assert_eq!(ob.order, SortOrder::Asc);
        assert_eq!(ob.nulls, NullsOrder::Last);
    }

    #[test]
    fn test_parse_window_spec_dotted_order_by() {
        // Dotted-path column ref (D-08): `o."order date"` must be captured as a
        // single identifier including the dot.
        let result = parse_metrics_clause(
            "s.running AS AVG(qty) OVER (PARTITION BY EXCLUDING r ORDER BY o.\"order date\" DESC)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0]
            .window_spec
            .as_ref()
            .expect("window_spec must be Some");
        assert_eq!(ws.order_by.len(), 1);
        let ob = &ws.order_by[0];
        assert_eq!(ob.expr, "o.\"order date\"");
        assert_eq!(ob.order, SortOrder::Desc);
        // DESC defaults to NULLS FIRST in window ORDER BY arm (matches NAB).
        assert_eq!(ob.nulls, NullsOrder::First);
    }

    #[test]
    fn test_parse_window_spec_unterminated_quote_order_by() {
        // Unterminated quoted column ref surfaces structured ParseError.
        let err = parse_metrics_clause(
            "s.running AS AVG(qty) OVER (PARTITION BY EXCLUDING r ORDER BY \"unclosed ASC)",
            0,
        )
        .unwrap_err();
        assert!(
            err.message.contains("Unterminated quoted identifier"),
            "Expected unterminated-quote error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_parse_window_spec_regression_bare_order_by() {
        // Regression baseline: bare unquoted column ref. Mirrors phase48.
        let result = parse_metrics_clause(
            "s.running AS AVG(qty) OVER (PARTITION BY EXCLUDING r ORDER BY order_date ASC NULLS LAST)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0]
            .window_spec
            .as_ref()
            .expect("window_spec must be Some");
        assert_eq!(ws.order_by.len(), 1);
        let ob = &ws.order_by[0];
        assert_eq!(ob.expr, "order_date");
        assert_eq!(ob.order, SortOrder::Asc);
        assert_eq!(ob.nulls, NullsOrder::Last);
    }

    #[test]
    fn test_parse_non_additive_dims_regression_bare_no_whitespace() {
        // Regression baseline for the pre-existing happy path: bare unquoted
        // dim_name with ASC/DESC modifier. Mirrors phase47_semi_additive.test.
        let result = parse_metrics_clause(
            "a.balance NON ADDITIVE BY (report_date DESC) AS SUM(a.balance)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let na_vec = &result[0].non_additive_by;
        assert_eq!(na_vec.len(), 1);
        let na = &na_vec[0];
        assert_eq!(na.dimension, "report_date");
        assert_eq!(na.order, SortOrder::Desc);
        assert_eq!(na.nulls, NullsOrder::First); // DESC default
    }

    // -----------------------------------------------------------------------
    // parse_relationships_clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_relationships_empty_body() {
        let result = parse_relationships_clause("", 0).unwrap();
        assert_eq!(result.len(), 0, "Empty body must return empty vec");
    }

    #[test]
    fn parse_relationships_single_entry() {
        let result =
            parse_relationships_clause("order_to_customer AS o(customer_id) REFERENCES c", 0)
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name.as_deref(), Some("order_to_customer"));
        assert_eq!(result[0].from_alias, "o");
        assert_eq!(result[0].fk_columns, vec!["customer_id"]);
        assert_eq!(result[0].table, "c");
    }

    #[test]
    fn parse_relationships_composite_fk() {
        let result = parse_relationships_clause("rel AS o(fk1, fk2) REFERENCES c", 0).unwrap();
        assert_eq!(result[0].fk_columns, vec!["fk1", "fk2"]);
    }

    #[test]
    fn parse_relationships_quoted_paren_in_fk_column() {
        // PA-6 (PR #50 review): the close paren after the FK list was
        // located with a naive find(')'), so a quoted FK column containing
        // ')' truncated the list and mis-parsed the REFERENCES clause.
        let result =
            parse_relationships_clause("rel AS o(\"x)y\") REFERENCES c(\"a)b\")", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].fk_columns, vec!["\"x)y\""]);
        assert_eq!(result[0].table, "c");
        assert_eq!(result[0].ref_columns, vec!["\"a)b\""]);
    }

    #[test]
    fn parse_materializations_quoted_specials_do_not_split() {
        // PA-6 (PR #50 review): the sub-body paren scan and TABLE /
        // DIMENSIONS / METRICS keyword scan were not quote- or depth-aware —
        // a quoted name containing ')' closed the sub-body early, and
        // keyword text inside quotes or nested parens split it at the wrong
        // places.
        let result = parse_materializations_clause(
            "m1 AS (TABLE \"pre)agg\", DIMENSIONS (\"metrics\", region), METRICS (total))",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "m1");
        assert_eq!(result[0].table, "\"pre)agg\"");
        assert_eq!(result[0].dimensions, vec!["\"metrics\"", "region"]);
        assert_eq!(result[0].metrics, vec!["total"]);
    }

    #[test]
    fn parse_relationships_error_missing_name() {
        // Entry starts with "AS" — no preceding relationship name
        let result = parse_relationships_clause("AS o(customer_id) REFERENCES c", 0);
        assert!(
            result.is_err(),
            "Expected error for missing relationship name"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("name") || err.message.contains("required"),
            "Error should mention name or required: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // Phase 33: Cardinality keyword rejection + REFERENCES(cols) + UNIQUE tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_relationship_without_cardinality_defaults() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(
            result[0].cardinality,
            Cardinality::ManyToOne,
            "Cardinality should default to ManyToOne"
        );
    }

    #[test]
    fn old_cardinality_keywords_rejected() {
        // Phase 33: All cardinality keywords are rejected
        for input in [
            "rel AS a(fk) REFERENCES b MANY TO ONE",
            "rel AS a(fk) REFERENCES b ONE TO ONE",
            "rel AS a(fk) REFERENCES b ONE TO MANY",
        ] {
            let result = parse_relationships_clause(input, 0);
            assert!(
                result.is_err(),
                "Cardinality keyword should be rejected: {input}"
            );
            let err = result.unwrap_err();
            assert!(
                err.message.contains("no longer supported"),
                "Error should mention no longer supported for '{input}': {}",
                err.message
            );
        }
    }

    #[test]
    fn trailing_text_after_references_rejected() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b garbage", 0);
        assert!(result.is_err(), "Trailing text should be rejected");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Unexpected tokens")
                || err.message.contains("no longer supported"),
            "Error should mention unexpected tokens: {}",
            err.message
        );
    }

    #[test]
    fn references_with_column_list() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b(id)", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(result[0].ref_columns, vec!["id"]);
    }

    #[test]
    fn references_without_column_list() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert!(
            result[0].ref_columns.is_empty(),
            "ref_columns should be empty when no explicit column list"
        );
    }

    #[test]
    fn references_multi_column_list() {
        let result =
            parse_relationships_clause("rel AS a(fk1, fk2) REFERENCES b(col1, col2)", 0).unwrap();
        assert_eq!(result[0].ref_columns, vec!["col1", "col2"]);
    }

    #[test]
    fn references_target_no_space_before_paren() {
        // target(col) with no space between alias and paren
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b(id)", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(result[0].ref_columns, vec!["id"]);
    }

    #[test]
    fn references_target_space_before_paren() {
        // target (col) with space between alias and paren
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b (id)", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(result[0].ref_columns, vec!["id"]);
    }

    #[test]
    fn unique_constraint_parsing() {
        let result = parse_tables_clause("o AS orders PRIMARY KEY (id) UNIQUE (email)", 0).unwrap();
        assert_eq!(result[0].unique_constraints.len(), 1);
        assert_eq!(result[0].unique_constraints[0], vec!["email"]);
    }

    #[test]
    fn multiple_unique_constraints() {
        let result = parse_tables_clause(
            "o AS orders PRIMARY KEY (id) UNIQUE (email) UNIQUE (first_name, last_name)",
            0,
        )
        .unwrap();
        assert_eq!(result[0].unique_constraints.len(), 2);
        assert_eq!(result[0].unique_constraints[0], vec!["email"]);
        assert_eq!(
            result[0].unique_constraints[1],
            vec!["first_name", "last_name"]
        );
    }

    #[test]
    fn table_without_primary_key() {
        let result = parse_tables_clause("f AS fact_table", 0).unwrap();
        assert_eq!(result[0].alias, "f");
        assert_eq!(result[0].table, "fact_table");
        assert!(result[0].pk_columns.is_empty());
        assert!(result[0].unique_constraints.is_empty());
    }

    #[test]
    fn table_with_unique_no_pk() {
        let result = parse_tables_clause("f AS fact_table UNIQUE (email)", 0).unwrap();
        assert_eq!(result[0].alias, "f");
        assert_eq!(result[0].table, "fact_table");
        assert!(result[0].pk_columns.is_empty());
        assert_eq!(result[0].unique_constraints.len(), 1);
        assert_eq!(result[0].unique_constraints[0], vec!["email"]);
    }

    // -----------------------------------------------------------------------
    // parse_qualified_entries tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_qualified_entries_simple() {
        let result =
            parse_qualified_entries("o.revenue AS SUM(amount)", 0, false, "dimensions").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, "o"); // source_alias
        assert_eq!(result[0].name, "revenue"); // bare_name
        assert_eq!(result[0].expr, "SUM(amount)"); // expr
    }

    #[test]
    fn parse_qualified_entries_nested_parens() {
        let result = parse_qualified_entries(
            "o.disc_price AS SUM(l_extendedprice * (1 - l_discount))",
            0,
            false,
            "dimensions",
        )
        .unwrap();
        assert_eq!(result[0].expr, "SUM(l_extendedprice * (1 - l_discount))");
    }

    #[test]
    fn parse_qualified_entries_trailing_comma() {
        let result =
            parse_qualified_entries("o.revenue AS SUM(amount),", 0, false, "dimensions").unwrap();
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_qualified_entries_multiple_with_trailing_comma() {
        let result =
            parse_qualified_entries("o.a AS x, o.b AS y,", 0, false, "dimensions").unwrap();
        assert_eq!(result.len(), 2, "Expected 2 entries, got {:?}", result);
        assert_eq!(result[0].name, "a");
        assert_eq!(result[1].name, "b");
    }

    #[test]
    fn parse_qualified_entries_error_missing_dot() {
        let result = parse_qualified_entries("revenue AS SUM(amount)", 0, false, "dimensions");
        assert!(result.is_err(), "Expected error for missing alias prefix");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("alias") || err.message.contains("qualified"),
            "Error should mention alias or qualified: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // parse_keyword_body end-to-end tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_basic() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.relationships.len(), 0);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
        assert_eq!(kb.tables[0].alias, "o");
        assert_eq!(kb.dimensions[0].name, "region");
        assert_eq!(kb.dimensions[0].source_table.as_deref(), Some("o"));
        assert_eq!(kb.metrics[0].name, "rev");
        assert_eq!(kb.metrics[0].expr, "SUM(amount)");
    }

    #[test]
    fn parse_keyword_body_with_relationships() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id), c AS customers PRIMARY KEY (id)) RELATIONSHIPS (o_to_c AS o(cust_id) REFERENCES c) DIMENSIONS (o.reg AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.relationships.len(), 1);
        assert_eq!(kb.relationships[0].name.as_deref(), Some("o_to_c"));
        assert_eq!(kb.relationships[0].from_alias, "o");
        assert_eq!(kb.relationships[0].fk_columns, vec!["cust_id"]);
        assert_eq!(kb.relationships[0].table, "c");
    }

    #[test]
    fn parse_keyword_body_empty_relationships() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) RELATIONSHIPS () DIMENSIONS (o.x AS x)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(
            kb.relationships.len(),
            0,
            "Empty RELATIONSHIPS must be empty vec"
        );
    }

    #[test]
    fn parse_keyword_body_error_missing_tables() {
        let body = "AS DIMENSIONS (o.x AS x)";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Expected error for missing TABLES clause");
    }

    // -----------------------------------------------------------------------
    // Whitespace tolerance tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_newlines_between_clauses() {
        let body = "AS\nTABLES (\n  o AS orders PRIMARY KEY (id)\n)\nDIMENSIONS (\n  o.region AS region\n)\nMETRICS (\n  o.rev AS SUM(amount)\n)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn parse_keyword_body_tabs_between_tokens() {
        let body = "AS\tTABLES\t(\to\tAS\torders\tPRIMARY\tKEY\t(id)\t)\tDIMENSIONS\t(\to.region\tAS\tregion\t)\tMETRICS\t(\to.rev\tAS\tSUM(amount)\t)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions[0].name, "region");
    }

    #[test]
    fn parse_keyword_body_extra_spaces() {
        let body = "AS  TABLES  (  o  AS  orders  PRIMARY  KEY  (  id  )  )  DIMENSIONS  (  o.region  AS  region  )  METRICS  (  o.rev  AS  SUM(amount)  )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables[0].alias, "o");
        assert_eq!(kb.tables[0].table, "orders");
        assert_eq!(kb.tables[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn parse_keyword_body_mixed_whitespace_multientry() {
        // Multiple entries with newline+indent separation
        let body = "AS TABLES (\n    o AS orders PRIMARY KEY (o_id),\n    c AS customers PRIMARY KEY (c_id)\n) DIMENSIONS (\n    o.region AS region,\n    c.name AS customer_name\n) METRICS (\n    o.rev AS SUM(amount)\n)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 2);
        assert_eq!(kb.dimensions.len(), 2);
    }

    #[test]
    fn parse_tables_extra_whitespace_around_tokens() {
        // Extra whitespace inside the clause body
        let result = parse_tables_clause(
            "  o   AS   main.orders   PRIMARY   KEY   ( o_id ,  o_seq )  ",
            0,
        )
        .unwrap();
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "main.orders");
        assert_eq!(result[0].pk_columns, vec!["o_id", "o_seq"]);
    }

    #[test]
    fn parse_relationships_newline_separated() {
        let body = "\n  order_to_customer AS o(customer_id) REFERENCES c\n";
        let result = parse_relationships_clause(body, 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name.as_deref(), Some("order_to_customer"));
    }

    #[test]
    fn parse_qualified_entries_newline_separated() {
        let body = "\n  o.revenue AS SUM(amount),\n  o.count AS COUNT(*)\n";
        let result = parse_qualified_entries(body, 0, false, "dimensions").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "revenue");
        assert_eq!(result[1].name, "count");
    }

    // -----------------------------------------------------------------------
    // Case insensitivity tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_lowercase_clause_keywords() {
        let body = "as tables (o AS orders primary key (id)) dimensions (o.region AS region) metrics (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn parse_keyword_body_mixedcase_clause_keywords() {
        let body = "As Tables (o AS orders Primary Key (id)) Dimensions (o.region AS region) Metrics (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions[0].name, "region");
    }

    #[test]
    fn parse_tables_lowercase_as_and_primary_key() {
        let result = parse_tables_clause("o as orders primary key (o_id)", 0).unwrap();
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert_eq!(result[0].pk_columns, vec!["o_id"]);
    }

    #[test]
    fn parse_tables_primary_key_with_newline() {
        // PRIMARY\nKEY — newline between the two words
        let result = parse_tables_clause("o AS orders PRIMARY\nKEY (o_id)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["o_id"]);
    }

    #[test]
    fn parse_tables_primary_key_with_tab() {
        let result = parse_tables_clause("o AS orders PRIMARY\tKEY (o_id)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["o_id"]);
    }

    #[test]
    fn parse_relationships_lowercase_as_and_references() {
        let result =
            parse_relationships_clause("ord_to_cust as o(cust_id) references c", 0).unwrap();
        assert_eq!(result[0].name.as_deref(), Some("ord_to_cust"));
        assert_eq!(result[0].from_alias, "o");
        assert_eq!(result[0].table, "c");
    }

    #[test]
    fn parse_qualified_entries_lowercase_as() {
        let result =
            parse_qualified_entries("o.revenue as SUM(amount)", 0, false, "dimensions").unwrap();
        assert_eq!(result[0].name, "revenue");
        assert_eq!(result[0].expr, "SUM(amount)");
    }

    #[test]
    fn parse_keyword_body_all_lowercase_full_round_trip() {
        // Fully lowercase: every keyword, operator, and token in lowercase
        let body = "as\ntables (\n    o as main.orders primary\nkey (o_id)\n)\nrelationships (\n    ord_to_cust as o(o_cust_id) references c\n)\ndimensions (\n    o.region as region\n)\nmetrics (\n    o.revenue as sum(amount)\n)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.tables[0].table, "main.orders");
        assert_eq!(kb.relationships.len(), 1);
        assert_eq!(kb.relationships[0].name.as_deref(), Some("ord_to_cust"));
        assert_eq!(kb.dimensions[0].name, "region");
        assert_eq!(kb.metrics[0].expr, "sum(amount)");
    }

    // -----------------------------------------------------------------------
    // FACTS clause tests (Phase 29)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_with_facts_single() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts.len(), 1);
        assert_eq!(kb.facts[0].name, "net_price");
        assert_eq!(kb.facts[0].expr, "o.price * (1 - o.discount)");
        assert_eq!(kb.facts[0].source_table.as_deref(), Some("o"));
    }

    #[test]
    fn parse_keyword_body_with_facts_multiple() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount), o.tax_amount AS o.price * o.tax_rate) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts.len(), 2);
        assert_eq!(kb.facts[0].name, "net_price");
        assert_eq!(kb.facts[1].name, "tax_amount");
    }

    #[test]
    fn parse_keyword_body_with_facts_trailing_comma() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount),) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(
            kb.facts.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_keyword_body_with_empty_facts() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS () DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(
            kb.facts.is_empty(),
            "Empty FACTS clause must produce empty vec"
        );
    }

    #[test]
    fn parse_keyword_body_without_facts_still_works() {
        // Backward compat: DDL without FACTS clause must still work
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(kb.facts.is_empty());
    }

    #[test]
    fn parse_keyword_body_fact_without_source_table_rejected() {
        // Facts reuse parse_qualified_entries which requires alias.name format
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (net_price AS price * discount) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(
            result.is_err(),
            "Fact without source table prefix must be rejected"
        );
    }

    #[test]
    fn parse_keyword_body_facts_after_dimensions_rejected() {
        // FACTS must come before DIMENSIONS (order: tables, relationships, facts, dimensions, metrics)
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) FACTS (o.net_price AS price) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(
            result.is_err(),
            "FACTS after DIMENSIONS must be rejected (wrong order)"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("out of order"),
            "Error should mention out of order: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // parse_metrics_clause tests (Phase 30 -- derived metrics)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_clause_qualified_entry() {
        let result = parse_metrics_clause("li.revenue AS SUM(li.amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("li".to_string())); // source alias
        assert_eq!(result[0].name, "revenue"); // bare_name
        assert_eq!(result[0].expr, "SUM(li.amount)"); // expr
    }

    #[test]
    fn parse_metrics_clause_unqualified_entry() {
        let result = parse_metrics_clause("profit AS revenue - cost", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, None); // no source alias (derived metric)
        assert_eq!(result[0].name, "profit"); // bare_name
        assert_eq!(result[0].expr, "revenue - cost"); // expr
    }

    #[test]
    fn parse_metrics_clause_mixed_entries() {
        let result = parse_metrics_clause(
            "li.revenue AS SUM(li.amount), profit AS revenue - cost, li.cost AS SUM(li.unit_cost)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        // First: qualified
        assert_eq!(result[0].source_alias, Some("li".to_string()));
        assert_eq!(result[0].name, "revenue");
        assert_eq!(result[0].expr, "SUM(li.amount)");
        // Second: unqualified (derived)
        assert_eq!(result[1].source_alias, None);
        assert_eq!(result[1].name, "profit");
        assert_eq!(result[1].expr, "revenue - cost");
        // Third: qualified
        assert_eq!(result[2].source_alias, Some("li".to_string()));
        assert_eq!(result[2].name, "cost");
        assert_eq!(result[2].expr, "SUM(li.unit_cost)");
    }

    #[test]
    fn parse_metrics_clause_trailing_comma() {
        let result = parse_metrics_clause("profit AS revenue - cost,", 0).unwrap();
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_metrics_clause_newline_separated() {
        let result = parse_metrics_clause(
            "\n  li.revenue AS SUM(li.amount),\n  profit AS revenue - cost\n",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_alias, Some("li".to_string()));
        assert_eq!(result[1].source_alias, None);
        assert_eq!(result[1].name, "profit");
    }

    #[test]
    fn parse_keyword_body_with_derived_metrics() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.revenue AS SUM(o.amount), profit AS revenue - cost)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        // First: qualified metric -> source_table: Some("o")
        assert_eq!(kb.metrics[0].name, "revenue");
        assert_eq!(kb.metrics[0].source_table.as_deref(), Some("o"));
        assert_eq!(kb.metrics[0].expr, "SUM(o.amount)");
        // Second: derived metric -> source_table: None
        assert_eq!(kb.metrics[1].name, "profit");
        assert!(kb.metrics[1].source_table.is_none());
        assert_eq!(kb.metrics[1].expr, "revenue - cost");
    }

    #[test]
    fn parse_keyword_body_only_derived_metrics() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (profit AS revenue - cost, margin AS profit / revenue)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        assert!(kb.metrics[0].source_table.is_none());
        assert!(kb.metrics[1].source_table.is_none());
        assert_eq!(kb.metrics[0].name, "profit");
        assert_eq!(kb.metrics[1].name, "margin");
    }

    #[test]
    fn parse_qualified_entries_still_rejects_unqualified() {
        // FACTS and DIMENSIONS still use parse_qualified_entries which requires alias.name
        let result = parse_qualified_entries("revenue AS SUM(amount)", 0, false, "dimensions");
        assert!(
            result.is_err(),
            "parse_qualified_entries must still reject unqualified entries (missing dot)"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("alias") || err.message.contains("qualified"),
            "Error should mention alias or qualified: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_clause_empty_body() {
        let result = parse_metrics_clause("", 0).unwrap();
        assert_eq!(result.len(), 0, "Empty body must return empty vec");
    }

    // -----------------------------------------------------------------------
    // parse_metrics_clause USING tests (Phase 32 -- role-playing dimensions)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_using_single_relationship() {
        let result =
            parse_metrics_clause("f.departure_count USING (dep_airport) AS COUNT(*)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("f".to_string())); // source alias
        assert_eq!(result[0].name, "departure_count"); // bare_name
        assert_eq!(result[0].expr, "COUNT(*)"); // expr
        assert_eq!(result[0].using_relationships, vec!["dep_airport"]); // using_relationships
    }

    #[test]
    fn parse_metrics_using_multiple_relationships() {
        let result = parse_metrics_clause("f.met USING (rel1, rel2) AS SUM(x)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("f".to_string()));
        assert_eq!(result[0].name, "met");
        assert_eq!(result[0].expr, "SUM(x)");
        assert_eq!(result[0].using_relationships, vec!["rel1", "rel2"]);
    }

    #[test]
    fn parse_metrics_using_on_derived_produces_error() {
        // Derived metric (no dot prefix) with USING -> ParseError
        let result = parse_metrics_clause("derived_met USING (rel1) AS revenue - cost", 0);
        assert!(
            result.is_err(),
            "USING on derived metric must produce error"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("USING") && err.message.contains("derived"),
            "Error should mention USING and derived: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_without_using_backward_compat() {
        // Metric without USING still parses correctly
        let result = parse_metrics_clause("o.revenue AS SUM(o.amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("o".to_string()));
        assert_eq!(result[0].name, "revenue");
        assert_eq!(result[0].expr, "SUM(o.amount)");
        assert!(
            result[0].using_relationships.is_empty(),
            "No USING -> empty relationships"
        );
    }

    #[test]
    fn parse_metrics_using_case_insensitive() {
        let result =
            parse_metrics_clause("f.departure_count using (dep_airport) AS COUNT(*)", 0).unwrap();
        assert_eq!(result[0].using_relationships, vec!["dep_airport"]);

        let result2 =
            parse_metrics_clause("f.departure_count UsInG (dep_airport) AS COUNT(*)", 0).unwrap();
        assert_eq!(result2[0].using_relationships, vec!["dep_airport"]);
    }

    #[test]
    fn parse_keyword_body_with_using_metrics() {
        let body = "AS TABLES (f AS flights PRIMARY KEY (id), a AS airports PRIMARY KEY (id)) RELATIONSHIPS (dep_airport AS f(dep_id) REFERENCES a, arr_airport AS f(arr_id) REFERENCES a) DIMENSIONS (a.name AS airport_name) METRICS (f.departure_count USING (dep_airport) AS COUNT(*))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 1);
        assert_eq!(kb.metrics[0].name, "departure_count");
        assert_eq!(kb.metrics[0].using_relationships, vec!["dep_airport"]);
    }

    // -----------------------------------------------------------------------
    // Phase 43: COMMENT, SYNONYMS, PRIVATE/PUBLIC annotation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dimension_with_comment() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at COMMENT = 'Order date') METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.dimensions[0].comment.as_deref(), Some("Order date"));
    }

    #[test]
    fn test_dimension_with_synonyms() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at WITH SYNONYMS = ('purchase_date', 'order_date')) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(
            kb.dimensions[0].synonyms,
            vec!["purchase_date", "order_date"]
        );
    }

    #[test]
    fn test_comment_and_synonyms_either_order() {
        // COMMENT then SYNONYMS
        let body1 = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at COMMENT = 'test' WITH SYNONYMS = ('a')) METRICS (o.rev AS SUM(o.amount))";
        let kb1 = parse_keyword_body(body1, 0).unwrap();
        assert_eq!(kb1.dimensions[0].comment.as_deref(), Some("test"));
        assert_eq!(kb1.dimensions[0].synonyms, vec!["a"]);

        // SYNONYMS then COMMENT
        let body2 = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at WITH SYNONYMS = ('a') COMMENT = 'test') METRICS (o.rev AS SUM(o.amount))";
        let kb2 = parse_keyword_body(body2, 0).unwrap();
        assert_eq!(kb2.dimensions[0].comment.as_deref(), Some("test"));
        assert_eq!(kb2.dimensions[0].synonyms, vec!["a"]);
    }

    #[test]
    fn test_private_metric() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (PRIVATE o.cost AS SUM(o.cost))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Private);
    }

    #[test]
    fn test_explicit_public_metric() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (PUBLIC o.revenue AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Public);
    }

    #[test]
    fn test_default_public_metric() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (o.revenue AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Public);
    }

    #[test]
    fn test_private_fact() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (PRIVATE o.raw_cost AS o.cost) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts[0].access, AccessModifier::Private);
    }

    #[test]
    fn test_private_dimension_rejected() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (PRIVATE o.date AS o.created_at) METRICS (o.rev AS SUM(o.amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "PRIVATE on dimension must produce error");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("PRIVATE") || err.message.contains("not supported"),
            "Error should mention PRIVATE or not supported: {}",
            err.message
        );
        assert!(
            err.message.to_lowercase().contains("dimension"),
            "Error should mention dimension: {}",
            err.message
        );
    }

    #[test]
    fn test_escaped_quotes_in_comment() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at COMMENT = 'It''s a test') METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.dimensions[0].comment.as_deref(), Some("It's a test"));
    }

    #[test]
    fn test_comment_identifier_not_confused() {
        // Expression contains "comment_count" as an identifier -- must NOT trigger COMMENT annotation
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.flag AS CASE WHEN comment_count > 0 THEN 1 ELSE 0 END) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(
            kb.dimensions[0].comment.is_none(),
            "comment_count in expr must not trigger COMMENT annotation"
        );
        assert_eq!(
            kb.dimensions[0].expr,
            "CASE WHEN comment_count > 0 THEN 1 ELSE 0 END"
        );
    }

    #[test]
    fn test_table_with_comment_and_synonyms() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id) COMMENT = 'Orders table' WITH SYNONYMS = ('sales')) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables[0].comment.as_deref(), Some("Orders table"));
        assert_eq!(kb.tables[0].synonyms, vec!["sales"]);
    }

    #[test]
    fn test_metric_with_using_and_comment() {
        let body = "AS TABLES (f AS flights PRIMARY KEY (id), a AS airports PRIMARY KEY (id)) RELATIONSHIPS (dep AS f(dep_id) REFERENCES a) DIMENSIONS (a.name AS a.name) METRICS (f.dep_count USING (dep) AS COUNT(*) COMMENT = 'Departures')";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].using_relationships, vec!["dep"]);
        assert_eq!(kb.metrics[0].comment.as_deref(), Some("Departures"));
    }

    #[test]
    fn test_private_keyword_disambiguation() {
        // Entry starting with table alias "private_schema.metric_name" should NOT be treated as PRIVATE
        // because "private_schema" is followed by "."
        let body = "AS TABLES (private_schema AS my_table PRIMARY KEY (id)) METRICS (private_schema.metric_name AS SUM(private_schema.value))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Public);
        assert_eq!(
            kb.metrics[0].source_table.as_deref(),
            Some("private_schema")
        );
        assert_eq!(kb.metrics[0].name, "metric_name");
    }

    #[test]
    fn test_full_keyword_body_with_all_metadata() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id) COMMENT = 'Order table') FACTS (o.net_price AS o.amount * 0.9 COMMENT = 'Net price' WITH SYNONYMS = ('discounted_price')) DIMENSIONS (o.region AS o.region WITH SYNONYMS = ('territory')) METRICS (o.revenue AS SUM(o.amount) COMMENT = 'Total revenue', PRIVATE o.cost AS SUM(o.cost))";
        let kb = parse_keyword_body(body, 0).unwrap();

        assert_eq!(kb.tables[0].comment.as_deref(), Some("Order table"));
        assert_eq!(kb.facts[0].comment.as_deref(), Some("Net price"));
        assert_eq!(kb.facts[0].synonyms, vec!["discounted_price"]);
        assert_eq!(kb.dimensions[0].synonyms, vec!["territory"]);
        assert_eq!(kb.metrics[0].comment.as_deref(), Some("Total revenue"));
        assert_eq!(kb.metrics[1].access, AccessModifier::Private);
    }

    // -----------------------------------------------------------------------
    // Phase 47: NON ADDITIVE BY tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_non_additive_by_single_dim_defaults() {
        let result = parse_metrics_clause("a.bal NON ADDITIVE BY (d1) AS SUM(x)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("a".to_string())); // source alias
        assert_eq!(result[0].name, "bal"); // bare name
        assert_eq!(result[0].expr, "SUM(x)"); // expr
        assert_eq!(result[0].non_additive_by.len(), 1);
        assert_eq!(result[0].non_additive_by[0].dimension, "d1");
        assert_eq!(result[0].non_additive_by[0].order, SortOrder::Asc);
        assert_eq!(result[0].non_additive_by[0].nulls, NullsOrder::Last);
    }

    #[test]
    fn parse_metrics_non_additive_by_desc_nulls_first() {
        let result = parse_metrics_clause(
            "a.balance NON ADDITIVE BY (date_dim DESC) AS SUM(a.balance)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].non_additive_by.len(), 1);
        assert_eq!(result[0].non_additive_by[0].dimension, "date_dim");
        assert_eq!(result[0].non_additive_by[0].order, SortOrder::Desc);
        // DESC defaults to NULLS FIRST
        assert_eq!(result[0].non_additive_by[0].nulls, NullsOrder::First);
    }

    #[test]
    fn parse_metrics_non_additive_by_multiple_dims() {
        let result = parse_metrics_clause(
            "a.bal NON ADDITIVE BY (d1 DESC NULLS FIRST, d2 ASC NULLS LAST) AS SUM(x)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].non_additive_by.len(), 2);
        assert_eq!(result[0].non_additive_by[0].dimension, "d1");
        assert_eq!(result[0].non_additive_by[0].order, SortOrder::Desc);
        assert_eq!(result[0].non_additive_by[0].nulls, NullsOrder::First);
        assert_eq!(result[0].non_additive_by[1].dimension, "d2");
        assert_eq!(result[0].non_additive_by[1].order, SortOrder::Asc);
        assert_eq!(result[0].non_additive_by[1].nulls, NullsOrder::Last);
    }

    #[test]
    fn parse_metrics_non_additive_by_on_derived_produces_error() {
        let result = parse_metrics_clause("profit NON ADDITIVE BY (d1) AS revenue - cost", 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("NON ADDITIVE BY"),
            "Error should mention NON ADDITIVE BY: {}",
            err.message
        );
        assert!(
            err.message.contains("derived"),
            "Error should mention derived: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_non_additive_by_with_using() {
        let result =
            parse_metrics_clause("a.bal USING (rel1) NON ADDITIVE BY (d1 DESC) AS SUM(x)", 0)
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("a".to_string()));
        assert_eq!(result[0].name, "bal");
        assert_eq!(result[0].using_relationships, vec!["rel1"]); // using_relationships
        assert_eq!(result[0].non_additive_by.len(), 1);
        assert_eq!(result[0].non_additive_by[0].dimension, "d1");
        assert_eq!(result[0].non_additive_by[0].order, SortOrder::Desc);
    }

    #[test]
    fn parse_keyword_body_non_additive_by_integration() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.date_dim AS a.date) \
                     METRICS (a.balance NON ADDITIVE BY (date_dim DESC) AS SUM(a.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 1);
        assert_eq!(kb.metrics[0].name, "balance");
        assert_eq!(kb.metrics[0].non_additive_by.len(), 1);
        assert_eq!(kb.metrics[0].non_additive_by[0].dimension, "date_dim");
        assert_eq!(kb.metrics[0].non_additive_by[0].order, SortOrder::Desc);
    }

    #[test]
    fn parse_keyword_body_non_additive_by_invalid_dim_error() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.date_dim AS a.date) \
                     METRICS (a.balance NON ADDITIVE BY (nonexistent DESC) AS SUM(a.amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("does not match any declared dimension"),
            "Error should mention dimension mismatch: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_non_additive_by_valid_dim_success() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.date_dim AS a.date, a.account AS a.account_id) \
                     METRICS (a.balance NON ADDITIVE BY (date_dim DESC, account) AS SUM(a.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].non_additive_by.len(), 2);
        assert_eq!(kb.metrics[0].non_additive_by[0].dimension, "date_dim");
        assert_eq!(kb.metrics[0].non_additive_by[1].dimension, "account");
    }

    #[test]
    fn parse_keyword_body_non_additive_by_case_insensitive_dim_match() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.Date_Dim AS a.date) \
                     METRICS (a.balance NON ADDITIVE BY (date_dim DESC) AS SUM(a.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].non_additive_by.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Phase 48: Window function OVER clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_window_over_basic() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1, d2 ORDER BY d1)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_alias, Some("o".to_string())); // source alias
        assert_eq!(result[0].name, "avg_qty"); // bare name
        let ws = result[0]
            .window_spec
            .as_ref()
            .expect("window_spec should be Some");
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert!(ws.extra_args.is_empty());
        assert_eq!(ws.excluding_dims, vec!["d1", "d2"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "d1");
        assert_eq!(ws.order_by[0].order, SortOrder::Asc);
        assert_eq!(ws.order_by[0].nulls, NullsOrder::Last);
        assert!(ws.frame_clause.is_none());
    }

    #[test]
    fn parse_metrics_window_over_with_frame_clause() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1 ORDER BY d1 RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0]
            .window_spec
            .as_ref()
            .expect("window_spec should be Some");
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert_eq!(ws.excluding_dims, vec!["d1"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "d1");
        assert_eq!(
            ws.frame_clause.as_deref(),
            Some("RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW")
        );
    }

    #[test]
    fn parse_metrics_window_lag_with_extra_args() {
        let result = parse_metrics_clause(
            "o.prev_qty AS LAG(total_qty, 30) OVER (PARTITION BY EXCLUDING d1 ORDER BY d1)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0]
            .window_spec
            .as_ref()
            .expect("window_spec should be Some");
        assert_eq!(ws.window_function, "LAG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert_eq!(ws.extra_args, vec!["30"]);
        assert_eq!(ws.excluding_dims, vec!["d1"]);
    }

    #[test]
    fn parse_metrics_window_over_on_derived_produces_error() {
        let result = parse_metrics_clause(
            "avg_ratio AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1)",
            0,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("OVER clause not allowed on derived metric"),
            "Error should mention OVER on derived: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_window_over_with_non_additive_by_produces_error() {
        let result = parse_metrics_clause(
            "o.met NON ADDITIVE BY (d1) AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d2)",
            0,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("Cannot combine OVER clause with NON ADDITIVE BY"),
            "Error should mention mutual exclusion: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_window_over_no_over_returns_none() {
        let result = parse_metrics_clause("o.revenue AS SUM(o.amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            result[0].window_spec.is_none(),
            "Regular metric should have no window_spec"
        );
    }

    #[test]
    fn parse_metrics_window_over_order_by_desc_nulls() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1 ORDER BY d1 DESC NULLS LAST)",
            0,
        )
        .unwrap();
        let ws = result[0].window_spec.as_ref().unwrap();
        assert_eq!(ws.order_by[0].order, SortOrder::Desc);
        assert_eq!(ws.order_by[0].nulls, NullsOrder::Last);
    }

    #[test]
    fn parse_keyword_body_window_metric_integration() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region, o.month AS date_trunc('month', o.created_at)) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month)\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        assert!(kb.metrics[0].window_spec.is_none());
        let ws = kb.metrics[1]
            .window_spec
            .as_ref()
            .expect("second metric should have window_spec");
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert_eq!(ws.excluding_dims, vec!["region"]);
        assert_eq!(ws.order_by[0].expr, "month");
    }

    #[test]
    fn parse_keyword_body_window_excluding_invalid_dim_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING nonexistent ORDER BY region)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("EXCLUDING dimension"),
            "Error should mention EXCLUDING dimension: {}",
            err.message
        );
        assert!(
            err.message.contains("nonexistent"),
            "Error should mention the invalid dim: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_window_inner_metric_invalid_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(nonexistent_metric) OVER (PARTITION BY EXCLUDING region ORDER BY region)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("inner metric"),
            "Error should mention inner metric: {}",
            err.message
        );
        assert!(
            err.message.contains("nonexistent_metric"),
            "Error should mention the invalid metric: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_window_order_by_invalid_dim_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY bad_dim)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("ORDER BY dimension"),
            "Error should mention ORDER BY dimension: {}",
            err.message
        );
        assert!(
            err.message.contains("bad_dim"),
            "Error should mention the invalid dim: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_window_valid_refs_succeed() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region, o.month AS date_trunc('month', o.created_at)) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month)\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        assert!(kb.metrics[1].window_spec.is_some());
    }

    #[test]
    fn parse_metrics_window_partition_by_explicit() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store ORDER BY date)",
            0,
        )
        .unwrap();
        let ws = result[0].window_spec.as_ref().unwrap();
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "date");
    }

    #[test]
    fn parse_metrics_window_partition_by_multiple_dims() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store, region ORDER BY date DESC NULLS FIRST)",
            0,
        )
        .unwrap();
        let ws = result[0].window_spec.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store", "region"]);
        assert_eq!(ws.order_by[0].order, SortOrder::Desc);
        assert_eq!(ws.order_by[0].nulls, NullsOrder::First);
    }

    #[test]
    fn parse_metrics_window_partition_by_no_order() {
        let result =
            parse_metrics_clause("o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store)", 0)
                .unwrap();
        let ws = result[0].window_spec.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store"]);
        assert!(ws.order_by.is_empty());
    }

    #[test]
    fn parse_keyword_body_window_partition_by_integration() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.store AS o.store, o.date AS o.sale_date) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store ORDER BY date)\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        let ws = kb.metrics[1].window_spec.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store"]);
        assert_eq!(ws.order_by[0].expr, "date");
    }

    #[test]
    fn parse_keyword_body_window_partition_by_invalid_dim_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY nonexistent ORDER BY region)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("PARTITION BY dimension"),
            "Error should mention PARTITION BY dimension: {}",
            err.message
        );
        assert!(
            err.message.contains("nonexistent"),
            "Error should mention the invalid dim: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // Phase 54: MATERIALIZATIONS clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_materializations_clause_single_entry() {
        let body = "daily_rev AS (\n\
                         TABLE daily_revenue_agg,\n\
                         DIMENSIONS (region),\n\
                         METRICS (revenue, order_count)\n\
                     )";
        let result = parse_materializations_clause(body, 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "daily_rev");
        assert_eq!(result[0].table, "daily_revenue_agg");
        assert_eq!(result[0].dimensions, vec!["region"]);
        assert_eq!(result[0].metrics, vec!["revenue", "order_count"]);
    }

    #[test]
    fn parse_materializations_clause_multiple_entries() {
        let body = "daily_rev AS (\n\
                         TABLE daily_agg,\n\
                         DIMENSIONS (region),\n\
                         METRICS (revenue)\n\
                     ),\n\
                     monthly_rev AS (\n\
                         TABLE monthly_agg,\n\
                         DIMENSIONS (region),\n\
                         METRICS (revenue, order_count)\n\
                     )";
        let result = parse_materializations_clause(body, 0).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "daily_rev");
        assert_eq!(result[0].table, "daily_agg");
        assert_eq!(result[1].name, "monthly_rev");
        assert_eq!(result[1].table, "monthly_agg");
        assert_eq!(result[1].metrics, vec!["revenue", "order_count"]);
    }

    #[test]
    fn parse_materializations_clause_dimensions_only() {
        let body = "dims_only AS (\n\
                         TABLE t1,\n\
                         DIMENSIONS (region, date_dim)\n\
                     )";
        let result = parse_materializations_clause(body, 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].dimensions, vec!["region", "date_dim"]);
        assert!(result[0].metrics.is_empty());
    }

    #[test]
    fn parse_materializations_clause_metrics_only() {
        let body = "mets_only AS (\n\
                         TABLE t2,\n\
                         METRICS (revenue)\n\
                     )";
        let result = parse_materializations_clause(body, 0).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].dimensions.is_empty());
        assert_eq!(result[0].metrics, vec!["revenue"]);
    }

    #[test]
    fn parse_materializations_clause_rejects_empty() {
        let body = "empty_mat AS (\n\
                         TABLE some_table\n\
                     )";
        let result = parse_materializations_clause(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("must specify at least one of DIMENSIONS or METRICS"),
            "Expected error about empty dims/metrics: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_with_materializations() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (o.revenue AS SUM(o.amount)) \
                     MATERIALIZATIONS (\
                         daily_rev AS (\
                             TABLE daily_agg,\
                             DIMENSIONS (region),\
                             METRICS (revenue)\
                         )\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.materializations.len(), 1);
        assert_eq!(kb.materializations[0].name, "daily_rev");
        assert_eq!(kb.materializations[0].table, "daily_agg");
        assert_eq!(kb.materializations[0].dimensions, vec!["region"]);
        assert_eq!(kb.materializations[0].metrics, vec!["revenue"]);
    }

    #[test]
    fn parse_keyword_body_materializations_before_metrics_rejected() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     MATERIALIZATIONS (\
                         mat AS (TABLE t, DIMENSIONS (region))\
                     ) \
                     METRICS (o.revenue AS SUM(o.amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("out of order"),
            "Expected ordering error: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_materialization_bad_dim_ref() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (o.revenue AS SUM(o.amount)) \
                     MATERIALIZATIONS (\
                         mat AS (\
                             TABLE t,\
                             DIMENSIONS (nonexistent_dim),\
                             METRICS (revenue)\
                         )\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("dimension 'nonexistent_dim' not found"),
            "Expected dim ref error: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_materialization_bad_dim_ref_with_suggestion() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (o.revenue AS SUM(o.amount)) \
                     MATERIALIZATIONS (\
                         mat AS (\
                             TABLE t,\
                             DIMENSIONS (regin),\
                             METRICS (revenue)\
                         )\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("dimension 'regin' not found"),
            "Expected dim ref error: {}",
            err.message
        );
        assert!(
            err.message.contains("Did you mean 'region'"),
            "Expected suggestion: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_materialization_bad_metric_ref() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (o.revenue AS SUM(o.amount)) \
                     MATERIALIZATIONS (\
                         mat AS (\
                             TABLE t,\
                             DIMENSIONS (region),\
                             METRICS (nonexistent_metric)\
                         )\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("metric 'nonexistent_metric' not found"),
            "Expected metric ref error: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_materialization_duplicate_name() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (o.revenue AS SUM(o.amount)) \
                     MATERIALIZATIONS (\
                         same_name AS (\
                             TABLE t1,\
                             DIMENSIONS (region)\
                         ),\
                         same_name AS (\
                             TABLE t2,\
                             METRICS (revenue)\
                         )\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Duplicate materialization name"),
            "Expected duplicate name error: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_without_materializations_has_empty_vec() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (o.revenue AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(
            kb.materializations.is_empty(),
            "No MATERIALIZATIONS clause should produce empty vec"
        );
    }

    #[test]
    fn parse_materializations_qualified_table_name() {
        let body = "qual_mat AS (\n\
                         TABLE catalog.schema.daily_revenue_agg,\n\
                         DIMENSIONS (region),\n\
                         METRICS (revenue)\n\
                     )";
        let result = parse_materializations_clause(body, 0).unwrap();
        assert_eq!(result[0].table, "catalog.schema.daily_revenue_agg");
    }
}
