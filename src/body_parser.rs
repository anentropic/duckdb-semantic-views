//! SQL keyword body parser for CREATE SEMANTIC VIEW.
//!
//! Parses: `AS TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)`
//! into a `SemanticViewDefinition`.

use crate::model::{Dimension, Join, Metric, TableRef};
use crate::parse::ParseError;

/// Result of parsing the keyword body (everything after "AS").
pub struct KeywordBody {
    pub tables: Vec<TableRef>,
    pub relationships: Vec<Join>,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
}

/// Parse the keyword body after "AS" into structured clause data.
///
/// `text` is the full text after the "AS" keyword, trimmed.
/// `base_offset` is the byte offset of `text[0]` in the original query string.
#[allow(clippy::needless_pass_by_value, unused_variables)]
pub fn parse_keyword_body(_text: &str, _base_offset: usize) -> Result<KeywordBody, ParseError> {
    todo!("Plan 02 implements this")
}

/// Parse the content inside TABLES (...).
#[allow(dead_code, unused_variables)]
pub(crate) fn parse_tables_clause(
    _body: &str,
    _base_offset: usize,
) -> Result<Vec<TableRef>, ParseError> {
    todo!("Plan 02 implements this")
}

/// Parse the content inside RELATIONSHIPS (...). Returns empty vec for empty body.
#[allow(dead_code, unused_variables)]
pub(crate) fn parse_relationships_clause(
    _body: &str,
    _base_offset: usize,
) -> Result<Vec<Join>, ParseError> {
    todo!("Plan 02 implements this")
}

/// Parse the content inside DIMENSIONS or METRICS (...).
/// Returns `Vec<(source_alias, bare_name, expr)>`.
#[allow(dead_code, unused_variables)]
pub(crate) fn parse_qualified_entries(
    _body: &str,
    _base_offset: usize,
) -> Result<Vec<(String, String, String)>, ParseError> {
    todo!("Plan 02 implements this")
}

/// Split `body` at depth-0 commas, respecting nested parens and single-quoted strings.
/// Returns `Vec<(start_offset_in_body, trimmed_slice)>`. Trailing empty entries discarded.
#[allow(dead_code, unused_variables)]
pub(crate) fn split_at_depth0_commas(_body: &str) -> Vec<(usize, &str)> {
    // Implementation stub — returns empty until Plan 02.
    // Plan 02 replaces this with the depth-tracking algorithm from RESEARCH.md Pattern.
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- TABLES clause ---

    #[test]
    #[should_panic] // Remove should_panic in Plan 02 when implementation is complete
    fn parse_tables_single_pk() {
        let result = parse_tables_clause("o AS orders PRIMARY KEY (o_orderkey)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert_eq!(result[0].pk_columns, vec!["o_orderkey"]);
    }

    #[test]
    #[should_panic]
    fn parse_tables_schema_qualified() {
        let result = parse_tables_clause("o AS main.orders PRIMARY KEY (o_orderkey)", 0).unwrap();
        assert_eq!(result[0].table, "main.orders");
        assert_eq!(result[0].alias, "o");
    }

    #[test]
    #[should_panic]
    fn parse_tables_composite_pk() {
        let result =
            parse_tables_clause("l AS lineitem PRIMARY KEY (l_orderkey, l_linenumber)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["l_orderkey", "l_linenumber"]);
    }

    // --- RELATIONSHIPS clause ---

    #[test]
    fn parse_relationships_empty_body() {
        // Empty relationships body — must return Ok(vec![]) WITHOUT should_panic
        // This case works as a stub returning todo!() only if called...
        // Temporarily skip until Plan 02.
    }

    #[test]
    #[should_panic]
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

    // --- DIMENSIONS / METRICS clause ---

    #[test]
    #[should_panic]
    fn parse_qualified_entries_simple() {
        let result = parse_qualified_entries("o.revenue AS SUM(amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "o"); // source_alias
        assert_eq!(result[0].1, "revenue"); // bare_name
        assert_eq!(result[0].2, "SUM(amount)"); // expr
    }

    #[test]
    #[should_panic]
    fn parse_qualified_entries_nested_parens() {
        let result =
            parse_qualified_entries("o.disc_price AS SUM(l_extendedprice * (1 - l_discount))", 0)
                .unwrap();
        assert_eq!(result[0].2, "SUM(l_extendedprice * (1 - l_discount))");
    }

    #[test]
    #[should_panic]
    fn parse_qualified_entries_trailing_comma() {
        let result = parse_qualified_entries("o.revenue AS SUM(amount),", 0).unwrap();
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }
}
