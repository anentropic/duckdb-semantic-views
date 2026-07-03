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

/// Double-quote `ident` only when a bare emission would not round-trip.
///
/// A name is bare-safe iff it matches `[a-z_][a-z0-9_]*` — anything else
/// (uppercase letters, whitespace, dots, quotes, non-ASCII, leading digits,
/// empty) must be quoted: bare view names fold to lowercase on re-parse
/// (PA-8), so an unquoted mixed-case or special-character name silently
/// resolves to a different view or fails to parse (RT-2, code-review
/// 2026-07-02).
///
/// Used by `render_ddl` for the stored (bare, quote-stripped) view name so
/// `get_ddl` output re-parses to the same catalog key while common
/// lowercase names keep rendering unquoted.
#[must_use]
pub fn quote_ident_if_needed(ident: &str) -> String {
    let bytes = ident.as_bytes();
    let bare_safe = !bytes.is_empty()
        && !bytes[0].is_ascii_digit()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
    if bare_safe {
        ident.to_string()
    } else {
        quote_ident(ident)
    }
}

/// Quote a potentially dot-qualified table reference, normalising already-quoted input.
///
/// Delegates to [`crate::ident::parse_qualified_identifier`] so we operate on the
/// UNQUOTED logical parts of the identifier, then re-emit each part via
/// [`quote_ident`]. This makes the function **idempotent** on already-quoted
/// input — repeated application produces the same canonical form.
///
/// Behaviour:
/// - Bare:                `orders`               -> `"orders"`
/// - Two-part bare:       `jaffle.raw_orders`    -> `"jaffle"."raw_orders"`
/// - Three-part bare:     `catalog.schema.table` -> `"catalog"."schema"."table"`
/// - Already quoted:      `"memory"."main"."v"`  -> `"memory"."main"."v"`  (idempotent)
/// - Mixed quoting:       `main."v"`             -> `"main"."v"`
/// - Embedded `""` escape: `"with""q"`           -> `"with""q"`            (preserved)
/// - Dot inside quoted part: `"a.b"`             -> `"a.b"`                (single part)
///
/// Inputs that fail to parse as a SQL identifier (legacy / malformed strings)
/// are emitted verbatim wrapped in a single pair of quotes via [`quote_ident`];
/// this preserves the bare-name fallback behaviour and never produces
/// double-quoting.
#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    match crate::ident::parse_qualified_identifier(table) {
        Ok(parts) => parts
            .iter()
            .map(|p| quote_ident(p))
            .collect::<Vec<_>>()
            .join("."),
        Err(_) => quote_ident(table),
    }
}

/// Qualify a table name with the definition's catalog/schema, then quote it.
///
/// If the table name is already dot-qualified (more than one structural part),
/// it is used as-is to avoid double-qualification. Otherwise, `database_name`
/// and `schema_name` from the definition are prepended as available.
///
/// This ensures the expanded SQL uses fully-qualified table references, which
/// is required for execution contexts (e.g. ADBC) that don't inherit the
/// connection's default catalog/schema search path.
///
/// We use structural part-count from [`crate::ident::parse_qualified_identifier`]
/// rather than a raw substring-dot heuristic, so quoted parts that contain
/// a literal `.` (e.g. `"a.b"`) are correctly recognised as single-part bare
/// names that should receive the db/schema prefix. If the input fails to parse
/// (legacy / malformed strings), we fall through to the prepend path with
/// `quote_ident(table)` — the safest option since prepending the catalog
/// context cannot cause downstream re-quote bugs.
#[must_use]
pub fn qualify_and_quote_table_ref(table: &str, def: &SemanticViewDefinition) -> String {
    // Structural "is already qualified" test: a parsed identifier with more
    // than one part means the user already wrote `db.t` / `db.schema.t` /
    // `"db"."schema"."t"` etc. and we must not prepend a second qualifier.
    let is_qualified = matches!(
        crate::ident::parse_qualified_identifier(table),
        Ok(ref parts) if parts.len() > 1
    );
    if is_qualified {
        return quote_table_ref(table);
    }

    let mut parts = Vec::new();
    if let Some(db) = &def.database_name {
        parts.push(quote_ident(db));
    }
    if let Some(schema) = &def.schema_name {
        parts.push(quote_ident(schema));
    }
    // `table` here is logically single-part. If it parses cleanly we emit
    // its unquoted form via quote_ident; if not (malformed) we fall back to
    // quote_ident on the raw string, which is the same shape quote_table_ref
    // uses for its Err branch.
    let last = match crate::ident::parse_qualified_identifier(table) {
        Ok(p) if p.len() == 1 => quote_ident(&p[0]),
        _ => quote_ident(table),
    };
    parts.push(last);
    parts.join(".")
}

/// True when a qualified request's table part matches an item's declared
/// `source_table` (case-insensitive). Items declared WITHOUT a table
/// qualifier (`source_table == None`) are base-table items everywhere else in
/// the expansion layer, so they match when the requested alias is the
/// base/root (first declared) table's alias.
fn source_table_matches(
    source_table: Option<&str>,
    alias: &str,
    def: &SemanticViewDefinition,
) -> bool {
    match source_table {
        Some(st) => st.eq_ignore_ascii_case(alias),
        None => def
            .tables
            .first()
            .is_some_and(|t| t.alias.eq_ignore_ascii_case(alias)),
    }
}

/// Look up a dimension by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.region"),
/// splits into (alias, `bare_name`) and matches only dimensions whose
/// `source_table` is that alias (unqualified declarations count as the base
/// table). There is deliberately NO fallback to a bare-name match when the
/// table part doesn't match (SG-14): `x.region` must not silently resolve to
/// a `region` dimension on some other table — the caller surfaces the
/// standard unknown-dimension error (with suggestions) instead.
pub(super) fn find_dimension<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Dimension> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        def.dimensions.iter().find(|d| {
            d.name.eq_ignore_ascii_case(bare)
                && source_table_matches(d.source_table.as_deref(), alias, def)
        })
    } else {
        def.dimensions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }
}

/// Look up a metric by name using case-insensitive matching.
///
/// Supports table-qualified names: if `name` contains a '.' (e.g., "o.revenue"),
/// splits into (alias, `bare_name`) and matches only metrics whose
/// `source_table` is that alias (unqualified declarations count as the base
/// table). As with [`find_dimension`], there is NO fallback to a bare-name
/// match when the table part doesn't match (SG-14).
pub(super) fn find_metric<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a crate::model::Metric> {
    if let Some(dot_pos) = name.find('.') {
        let alias = &name[..dot_pos];
        let bare = &name[dot_pos + 1..];
        def.metrics.iter().find(|m| {
            m.name.eq_ignore_ascii_case(bare)
                && source_table_matches(m.source_table.as_deref(), alias, def)
        })
    } else {
        def.metrics
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        find_dimension, find_metric, qualify_and_quote_table_ref, quote_ident, quote_table_ref,
    };
    use crate::model::SemanticViewDefinition;

    /// Minimal SemanticViewDefinition fixture with optional db_name / schema_name.
    ///
    /// All other vectors are empty — `qualify_and_quote_table_ref` only reads
    /// `database_name` and `schema_name`, so we don't need a full view.
    fn def_with_db_schema(db: Option<&str>, schema: Option<&str>) -> SemanticViewDefinition {
        SemanticViewDefinition {
            tables: vec![],
            dimensions: vec![],
            metrics: vec![],
            joins: vec![],
            facts: vec![],
            materializations: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
            created_on: None,
            database_name: db.map(str::to_string),
            schema_name: schema.map(str::to_string),
            comment: None,
        }
    }

    mod find_lookup_tests {
        use super::*;
        use crate::model::{Dimension, Metric, TableRef};

        /// Base table `o` + joined table `c`. `region` lives on `c`;
        /// `status` is declared without a source table (base-table item);
        /// metric `revenue` lives on `c`, metric `order_count` is
        /// unqualified (None).
        fn lookup_def() -> SemanticViewDefinition {
            let mut def = def_with_db_schema(None, None);
            def.tables = vec![
                TableRef {
                    alias: "o".to_string(),
                    table: "orders".to_string(),
                    ..Default::default()
                },
                TableRef {
                    alias: "c".to_string(),
                    table: "customers".to_string(),
                    ..Default::default()
                },
            ];
            def.dimensions = vec![
                Dimension {
                    name: "region".to_string(),
                    expr: "c.region".to_string(),
                    source_table: Some("c".to_string()),
                    ..Default::default()
                },
                Dimension {
                    name: "status".to_string(),
                    expr: "status".to_string(),
                    source_table: None,
                    ..Default::default()
                },
            ];
            def.metrics = vec![
                Metric {
                    name: "revenue".to_string(),
                    expr: "sum(c.amount)".to_string(),
                    source_table: Some("c".to_string()),
                    ..Default::default()
                },
                Metric {
                    name: "order_count".to_string(),
                    expr: "count(*)".to_string(),
                    source_table: None,
                    ..Default::default()
                },
            ];
            def
        }

        #[test]
        fn qualified_dimension_matching_table_resolves() {
            let def = lookup_def();
            let d = find_dimension(&def, "c.region").expect("c.region must resolve");
            assert_eq!(d.name, "region");
        }

        #[test]
        fn qualified_dimension_wrong_table_returns_none() {
            // SG-14: no silent fallback to a dimension on another table.
            let def = lookup_def();
            assert!(find_dimension(&def, "o.region").is_none());
            assert!(find_dimension(&def, "warehouse.region").is_none());
        }

        #[test]
        fn base_alias_matches_unqualified_dimension() {
            // `status` has source_table == None (base-table item), so the
            // base alias qualifies it; a non-base alias does not.
            let def = lookup_def();
            assert!(find_dimension(&def, "o.status").is_some());
            assert!(find_dimension(&def, "O.STATUS").is_some());
            assert!(find_dimension(&def, "c.status").is_none());
        }

        #[test]
        fn qualified_metric_wrong_table_returns_none() {
            let def = lookup_def();
            assert!(find_metric(&def, "c.revenue").is_some());
            assert!(find_metric(&def, "o.revenue").is_none());
            assert!(find_metric(&def, "x.revenue").is_none());
        }

        #[test]
        fn base_alias_matches_unqualified_metric() {
            let def = lookup_def();
            assert!(find_metric(&def, "o.order_count").is_some());
            assert!(find_metric(&def, "c.order_count").is_none());
        }

        #[test]
        fn bare_lookup_unchanged() {
            let def = lookup_def();
            assert!(find_dimension(&def, "region").is_some());
            assert!(find_dimension(&def, "STATUS").is_some());
            assert!(find_metric(&def, "revenue").is_some());
            assert!(find_dimension(&def, "nonexistent").is_none());
        }
    }

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
            // Input `my"db.my"table` is malformed under the new strict parser
            // (bare parts cannot abut a `"`), so it falls through to the
            // `quote_ident` fallback path: wrap the entire string in a single
            // pair of quotes and escape any internal `"` via `""`.
            assert_eq!(
                quote_table_ref("my\"db.my\"table"),
                "\"my\"\"db.my\"\"table\""
            );
        }

        // -----------------------------------------------------------------
        // Phase 64-03: idempotency / already-quoted input handling.
        // -----------------------------------------------------------------

        #[test]
        fn already_quoted_simple() {
            assert_eq!(quote_table_ref("\"orders\""), "\"orders\"");
        }

        #[test]
        fn already_quoted_two_part() {
            assert_eq!(
                quote_table_ref("\"jaffle\".\"raw_orders\""),
                "\"jaffle\".\"raw_orders\"",
            );
        }

        #[test]
        fn already_quoted_three_part() {
            assert_eq!(
                quote_table_ref("\"memory\".\"main\".\"orders\""),
                "\"memory\".\"main\".\"orders\"",
            );
        }

        #[test]
        fn mixed_quoting_first_quoted() {
            assert_eq!(quote_table_ref("\"main\".orders"), "\"main\".\"orders\"",);
        }

        #[test]
        fn mixed_quoting_last_quoted() {
            assert_eq!(quote_table_ref("main.\"orders\""), "\"main\".\"orders\"",);
        }

        #[test]
        fn mixed_quoting_middle_quoted() {
            assert_eq!(
                quote_table_ref("db.\"schema\".table"),
                "\"db\".\"schema\".\"table\"",
            );
        }

        #[test]
        fn embedded_double_quote_in_quoted_part() {
            assert_eq!(quote_table_ref("\"with\"\"q\""), "\"with\"\"q\"");
        }

        #[test]
        fn dot_inside_quoted_part() {
            // The `.` is data (single quoted part), not a separator.
            assert_eq!(quote_table_ref("\"a.b\""), "\"a.b\"");
        }

        #[test]
        fn whitespace_inside_quoted_part() {
            assert_eq!(quote_table_ref("\"my table\""), "\"my table\"");
        }

        #[test]
        fn idempotent_property_bare() {
            let once = quote_table_ref("orders");
            let twice = quote_table_ref(&once);
            assert_eq!(once, twice);
        }

        #[test]
        fn idempotent_property_fqn() {
            let once = quote_table_ref("memory.main.orders");
            let twice = quote_table_ref(&once);
            assert_eq!(once, twice);
        }

        #[test]
        fn idempotent_property_already_quoted_fqn() {
            // Direct regression coverage for the reported triple-quote bug:
            // re-quoting an already-quoted FQN must not change it.
            let input = "\"memory\".\"main\".\"orders_sv\"";
            assert_eq!(quote_table_ref(input), input);
            let twice = quote_table_ref(&quote_table_ref(input));
            assert_eq!(twice, input);
        }

        #[test]
        fn malformed_falls_back() {
            // `"unterminated` has an unterminated quote → parser returns Err →
            // fallback emits `quote_ident("\"unterminated")` which escapes the
            // lone `"` as `""` and wraps the whole thing in one pair of quotes.
            assert_eq!(quote_table_ref("\"unterminated"), "\"\"\"unterminated\"",);
        }
    }

    mod qualify_and_quote_table_ref_tests {
        use super::*;

        #[test]
        fn bare_name_gets_db_schema_prepended() {
            let def = def_with_db_schema(Some("db"), Some("schema"));
            assert_eq!(
                qualify_and_quote_table_ref("t", &def),
                "\"db\".\"schema\".\"t\"",
            );
        }

        #[test]
        fn bare_name_with_only_schema() {
            let def = def_with_db_schema(None, Some("schema"));
            assert_eq!(qualify_and_quote_table_ref("t", &def), "\"schema\".\"t\"",);
        }

        #[test]
        fn bare_name_no_db_no_schema() {
            let def = def_with_db_schema(None, None);
            assert_eq!(qualify_and_quote_table_ref("t", &def), "\"t\"");
        }

        #[test]
        fn quoted_bare_name_with_dot_inside_treated_as_single_part() {
            // `"a.b"` is a SINGLE quoted part (the `.` is data, not a separator).
            // The old substring-dot heuristic mistakenly treated this as
            // qualified and skipped the prepend. The structural test sees one
            // part, so db/schema are correctly prepended.
            let def = def_with_db_schema(Some("db"), Some("schema"));
            assert_eq!(
                qualify_and_quote_table_ref("\"a.b\"", &def),
                "\"db\".\"schema\".\"a.b\"",
            );
        }

        #[test]
        fn already_qualified_two_part() {
            let def = def_with_db_schema(Some("db"), Some("schema"));
            assert_eq!(
                qualify_and_quote_table_ref("jaffle.raw_orders", &def),
                "\"jaffle\".\"raw_orders\"",
            );
        }

        #[test]
        fn already_qualified_quoted_two_part() {
            let def = def_with_db_schema(Some("db"), Some("schema"));
            assert_eq!(
                qualify_and_quote_table_ref("\"jaffle\".\"raw_orders\"", &def),
                "\"jaffle\".\"raw_orders\"",
            );
        }

        #[test]
        fn already_qualified_three_part() {
            let def = def_with_db_schema(Some("db"), Some("schema"));
            assert_eq!(
                qualify_and_quote_table_ref("a.b.c", &def),
                "\"a\".\"b\".\"c\"",
            );
        }

        #[test]
        fn already_qualified_quoted_three_part_idempotent() {
            // The reported bug shape: already-quoted FQN must not be re-quoted.
            let def = def_with_db_schema(Some("ignored"), Some("ignored"));
            let input = "\"memory\".\"main\".\"orders\"";
            assert_eq!(qualify_and_quote_table_ref(input, &def), input);
        }

        #[test]
        fn malformed_falls_through_to_prepend() {
            // `"unterminated` fails to parse. The structural test returns
            // is_qualified == false, so we fall through to the prepend path.
            // The bare-name slot uses quote_ident("\"unterminated") which
            // escapes the lone `"` and wraps once. Result must not panic and
            // must contain the db/schema prefix.
            let def = def_with_db_schema(Some("db"), Some("schema"));
            let result = qualify_and_quote_table_ref("\"unterminated", &def);
            assert!(
                !result.is_empty(),
                "qualify_and_quote_table_ref must not return empty on malformed input",
            );
            assert!(
                result.starts_with("\"db\".\"schema\"."),
                "expected db/schema prefix on malformed input, got: {result}",
            );
        }
    }
}
