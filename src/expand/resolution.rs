use crate::model::SemanticViewDefinition;

/// Double-quote a SQL identifier, escaping embedded double quotes.
///
/// `DuckDB` uses `"` for identifier quoting. Internal `"` must be escaped
/// as `""` per the SQL standard.
///
/// # Examples
///
/// ```
/// # use semantic_views::expand::quote_ident;
/// assert_eq!(quote_ident("orders"), "\"orders\"");
/// assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
/// ```
#[must_use]
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Quote a potentially dot-qualified table reference.
///
/// Splits on `.` and quotes each part individually. This handles:
/// - Simple names: `orders` -> `"orders"`
/// - Catalog-qualified: `jaffle.raw_orders` -> `"jaffle"."raw_orders"`
/// - Fully qualified: `catalog.schema.table` -> `"catalog"."schema"."table"`
///
/// Each part is quoted via `quote_ident`, so embedded double quotes are escaped.
#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    table
        .split('.')
        .map(quote_ident)
        .collect::<Vec<_>>()
        .join(".")
}

/// Look up a dimension by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.region"),
/// splits into (alias, `bare_name`) and also matches `source_table == alias`.
/// Falls back to `bare_name` lookup if no qualified match is found.
pub(super) fn find_dimension<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Dimension> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        // Try qualified lookup: bare_name match AND source_table == alias
        if let Some(d) = def.dimensions.iter().find(|d| {
            d.name.eq_ignore_ascii_case(bare)
                && d.source_table
                    .as_deref()
                    .is_some_and(|st| st.eq_ignore_ascii_case(alias))
        }) {
            return Some(d);
        }
        // Fall back to bare_name only (backward compat)
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(bare))
    } else {
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }
}

/// Look up a metric by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.revenue"),
/// splits into (alias, `bare_name`) and also matches `source_table == alias`.
/// Falls back to `bare_name` lookup if no qualified match is found.
pub(super) fn find_metric<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Metric> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        if let Some(m) = def.metrics.iter().find(|m| {
            m.name.eq_ignore_ascii_case(bare)
                && m.source_table
                    .as_deref()
                    .is_some_and(|st| st.eq_ignore_ascii_case(alias))
        }) {
            return Some(m);
        }
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(bare))
    } else {
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::{quote_ident, quote_table_ref};

    mod quote_ident_tests {
        use super::*;

        #[test]
        fn simple_identifier() {
            assert_eq!(quote_ident("orders"), "\"orders\"");
        }

        #[test]
        fn reserved_word() {
            assert_eq!(quote_ident("select"), "\"select\"");
        }

        #[test]
        fn embedded_double_quote() {
            assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
        }

        #[test]
        fn identifier_with_spaces() {
            assert_eq!(quote_ident("my table"), "\"my table\"");
        }
    }

    mod quote_table_ref_tests {
        use super::*;

        #[test]
        fn simple_table_name() {
            assert_eq!(quote_table_ref("orders"), "\"orders\"");
        }

        #[test]
        fn catalog_qualified() {
            assert_eq!(
                quote_table_ref("jaffle.raw_orders"),
                "\"jaffle\".\"raw_orders\""
            );
        }

        #[test]
        fn fully_qualified() {
            assert_eq!(
                quote_table_ref("catalog.schema.table"),
                "\"catalog\".\"schema\".\"table\""
            );
        }

        #[test]
        fn reserved_word_parts() {
            assert_eq!(quote_table_ref("select.from"), "\"select\".\"from\"");
        }

        #[test]
        fn embedded_quotes_in_parts() {
            assert_eq!(
                quote_table_ref("my\"db.my\"table"),
                "\"my\"\"db\".\"my\"\"table\""
            );
        }
    }
}
