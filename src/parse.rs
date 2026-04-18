// Parse detection and rewriting for semantic view DDL statements.
//
// This module provides two layers:
// 1. Pure detection/rewrite functions (`detect_semantic_view_ddl`,
//    `extract_ddl_name`, `validate_and_rewrite`) testable under `cargo test`
//    without the extension feature.
// 2. FFI entry points (`sv_validate_ddl_rust`, `sv_rewrite_ddl_rust`)
//    feature-gated on `extension`, with `catch_unwind` for panic safety.

use std::collections::HashSet;

use crate::body_parser::parse_keyword_body;
use crate::errors::ParseError;
use crate::model::{Cardinality, Join, TableRef};

/// Not our statement -- return `DISPLAY_ORIGINAL_ERROR`.
pub const PARSE_NOT_OURS: u8 = 0;
/// Detected a semantic view DDL statement -- return `PARSE_SUCCESSFUL`.
pub const PARSE_DETECTED: u8 = 1;

// ---------------------------------------------------------------------------
// DdlKind enum and detection
// ---------------------------------------------------------------------------

/// The supported DDL statement forms for semantic views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdlKind {
    Create,
    CreateOrReplace,
    CreateIfNotExists,
    Drop,
    DropIfExists,
    Describe,
    Show,
    ShowTerse,
    ShowColumns,
    Alter,
    AlterIfExists,
    ShowDimensions,
    ShowMetrics,
    ShowFacts,
}

/// Match a fixed sequence of keyword tokens at the start of `input`, tolerating
/// arbitrary ASCII whitespace between tokens.
///
/// Returns `Some(bytes_consumed)` if all keywords matched (case-insensitively),
/// where `bytes_consumed` is the number of bytes consumed by the keyword prefix
/// (including inter-keyword whitespace). Returns `None` otherwise.
///
/// The match anchors at position 0. Leading whitespace in `input` is consumed
/// as part of the match (counted in the returned byte count). If the caller has
/// already trimmed leading whitespace, the returned count is from offset 0 of
/// the trimmed slice.
///
/// Anti-pattern avoided: does NOT scan at increasing offsets (no O(n^2) behavior).
/// If keyword[0] doesn't match at the start (after whitespace), returns None.
///
/// Note: only handles ASCII whitespace (0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20).
/// Unicode whitespace is handled by `DuckDB`'s `StripUnicodeSpaces` before the hook fires.
fn match_keyword_prefix(input: &[u8], keywords: &[&[u8]]) -> Option<usize> {
    let mut pos = 0;
    for (i, &kw) in keywords.iter().enumerate() {
        // Skip ASCII whitespace (but not before the first keyword -- caller is
        // responsible for leading whitespace; we skip INTER-keyword whitespace
        // only for i > 0).
        if i > 0 {
            // Require at least one whitespace character between keywords.
            if pos >= input.len() || !input[pos].is_ascii_whitespace() {
                return None;
            }
            while pos < input.len() && input[pos].is_ascii_whitespace() {
                pos += 1;
            }
        }
        // Match keyword case-insensitively.
        if input.len() < pos + kw.len() {
            return None;
        }
        if !input[pos..pos + kw.len()].eq_ignore_ascii_case(kw) {
            return None;
        }
        pos += kw.len();
    }
    Some(pos)
}

/// Detect the DDL kind and consumed prefix byte count from a query string.
///
/// The input must already be trimmed of leading/trailing whitespace and
/// trailing semicolons. Returns `Some((DdlKind, consumed_bytes))` where
/// `consumed_bytes` is the number of bytes consumed by the matched prefix
/// (including any inter-keyword whitespace in the input). Returns `None`
/// if no prefix matches.
///
/// Longest-first ordering prevents prefix overlap.
fn detect_ddl_prefix(trimmed: &str) -> Option<(DdlKind, usize)> {
    let b = trimmed.as_bytes();

    // CREATE OR REPLACE SEMANTIC VIEW (5 keywords) -- before CREATE SEMANTIC VIEW
    if let Some(n) = match_keyword_prefix(b, &[b"create", b"or", b"replace", b"semantic", b"view"])
    {
        return Some((DdlKind::CreateOrReplace, n));
    }
    // CREATE SEMANTIC VIEW IF NOT EXISTS (6 keywords) -- before CREATE SEMANTIC VIEW
    if let Some(n) = match_keyword_prefix(
        b,
        &[b"create", b"semantic", b"view", b"if", b"not", b"exists"],
    ) {
        return Some((DdlKind::CreateIfNotExists, n));
    }
    // CREATE SEMANTIC VIEW (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"create", b"semantic", b"view"]) {
        return Some((DdlKind::Create, n));
    }
    // DROP SEMANTIC VIEW IF EXISTS (5 keywords) -- before DROP SEMANTIC VIEW
    if let Some(n) = match_keyword_prefix(b, &[b"drop", b"semantic", b"view", b"if", b"exists"]) {
        return Some((DdlKind::DropIfExists, n));
    }
    // DROP SEMANTIC VIEW (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"drop", b"semantic", b"view"]) {
        return Some((DdlKind::Drop, n));
    }
    // ALTER SEMANTIC VIEW IF EXISTS (5 keywords) -- before ALTER SEMANTIC VIEW
    if let Some(n) = match_keyword_prefix(b, &[b"alter", b"semantic", b"view", b"if", b"exists"]) {
        return Some((DdlKind::AlterIfExists, n));
    }
    // ALTER SEMANTIC VIEW (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"alter", b"semantic", b"view"]) {
        return Some((DdlKind::Alter, n));
    }
    // DESCRIBE SEMANTIC VIEW (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"describe", b"semantic", b"view"]) {
        return Some((DdlKind::Describe, n));
    }
    // SHOW COLUMNS IN SEMANTIC VIEW (5 keywords) -- before all SHOW SEMANTIC matches
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"columns", b"in", b"semantic", b"view"]) {
        return Some((DdlKind::ShowColumns, n));
    }
    // SHOW TERSE SEMANTIC VIEWS (4 keywords) -- before SHOW SEMANTIC VIEWS
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"terse", b"semantic", b"views"]) {
        return Some((DdlKind::ShowTerse, n));
    }
    // SHOW SEMANTIC DIMENSIONS (3 keywords) -- before SHOW SEMANTIC VIEWS
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"semantic", b"dimensions"]) {
        return Some((DdlKind::ShowDimensions, n));
    }
    // SHOW SEMANTIC METRICS (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"semantic", b"metrics"]) {
        return Some((DdlKind::ShowMetrics, n));
    }
    // SHOW SEMANTIC FACTS (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"semantic", b"facts"]) {
        return Some((DdlKind::ShowFacts, n));
    }
    // SHOW SEMANTIC VIEWS (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"semantic", b"views"]) {
        return Some((DdlKind::Show, n));
    }

    None
}

/// Detect the DDL kind from a query string.
///
/// Returns `Some(DdlKind)` if the query matches one of the 9 semantic view
/// DDL prefixes, `None` otherwise. Uses longest-first ordering to avoid
/// prefix overlap (e.g. "create or replace semantic view" before
/// "create semantic view").
///
/// Tolerates arbitrary ASCII whitespace (spaces, tabs, newlines, carriage
/// returns, vertical tabs, form feeds) between prefix keywords.
#[must_use]
pub fn detect_ddl_kind(query: &str) -> Option<DdlKind> {
    let trimmed = query.trim().trim_end_matches(';').trim();
    detect_ddl_prefix(trimmed).map(|(kind, _)| kind)
}

/// Detect whether a query is any semantic view DDL statement.
///
/// Returns `PARSE_DETECTED` for all 9 DDL forms, `PARSE_NOT_OURS` otherwise.
/// Handles case variations, leading/trailing whitespace, and trailing semicolons.
#[must_use]
pub fn detect_semantic_view_ddl(query: &str) -> u8 {
    if detect_ddl_kind(query).is_some() {
        PARSE_DETECTED
    } else {
        PARSE_NOT_OURS
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Extract just the view name from a name-only DDL statement (DROP, DESCRIBE).
///
/// `prefix_len` is the byte length of the already-matched prefix.
fn extract_name_only(trimmed: &str, prefix_len: usize) -> Result<String, String> {
    let after_prefix = trimmed[prefix_len..].trim();
    if after_prefix.is_empty() {
        return Err("Missing view name".to_string());
    }
    // Name is everything up to whitespace (or end)
    let name_end = after_prefix
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_prefix.len());
    let name = &after_prefix[..name_end];
    if name.is_empty() {
        return Err("Missing view name".to_string());
    }
    Ok(name.to_string())
}

// ---------------------------------------------------------------------------
// Rewrite: DDL -> function call
// ---------------------------------------------------------------------------

/// Map a `DdlKind` to its target function name.
fn function_name(kind: DdlKind) -> &'static str {
    match kind {
        DdlKind::Create => "create_semantic_view",
        DdlKind::CreateOrReplace => "create_or_replace_semantic_view",
        DdlKind::CreateIfNotExists => "create_semantic_view_if_not_exists",
        DdlKind::Drop => "drop_semantic_view",
        DdlKind::DropIfExists => "drop_semantic_view_if_exists",
        DdlKind::Describe => "describe_semantic_view",
        DdlKind::Show => "list_semantic_views",
        DdlKind::ShowTerse => "list_terse_semantic_views",
        DdlKind::ShowColumns => "show_columns_in_semantic_view",
        DdlKind::Alter | DdlKind::AlterIfExists => "alter_semantic_view",
        DdlKind::ShowDimensions => "show_semantic_dimensions",
        DdlKind::ShowMetrics => "show_semantic_metrics",
        DdlKind::ShowFacts => "show_semantic_facts",
    }
}

// rewrite_show_dims_for_metric removed in Phase 34.1.1 -- absorbed into parse_show_filter_clauses.

// ---------------------------------------------------------------------------
// SHOW SEMANTIC filter clause helpers (Phase 34.1.1)
// ---------------------------------------------------------------------------

/// Extract a single-quoted string from `input`, starting at position 0.
/// Returns `(extracted_content, bytes_consumed)` where `bytes_consumed` includes
/// the opening and closing quotes.
///
/// Handles SQL-style escaping: `''` inside quotes represents a literal `'`.
fn extract_quoted_string(input: &str) -> Result<(String, usize), String> {
    let bytes = input.as_bytes();
    if bytes.is_empty() || bytes[0] != b'\'' {
        return Err("Expected single-quoted string".to_string());
    }
    let mut pos = 1;
    let mut result = String::new();
    while pos < bytes.len() {
        if bytes[pos] == b'\'' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'\'' {
                // Escaped quote: '' -> '
                result.push('\'');
                pos += 2;
            } else {
                // End of string
                return Ok((result, pos + 1));
            }
        } else {
            result.push(bytes[pos] as char);
            pos += 1;
        }
    }
    Err("Unterminated single-quoted string".to_string())
}

/// Build optional WHERE and LIMIT suffix for a SHOW rewrite.
///
/// LIKE maps to `name ILIKE '<escaped>'` (case-insensitive).
/// STARTS WITH maps to `name LIKE '<escaped>%'` (case-sensitive).
/// IN SCHEMA maps to `schema_name = '<escaped>'`.
/// IN DATABASE maps to `database_name = '<escaped>'`.
/// All conditions combined with AND. LIMIT appended last.
fn build_filter_suffix(
    like_pattern: Option<&str>,
    starts_with: Option<&str>,
    limit: Option<u64>,
    in_schema: Option<&str>,
    in_database: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(pattern) = like_pattern {
        let escaped = pattern.replace('\'', "''");
        parts.push(format!("name ILIKE '{escaped}'"));
    }
    if let Some(prefix) = starts_with {
        let escaped = prefix.replace('\'', "''");
        parts.push(format!("name LIKE '{escaped}%'"));
    }
    if let Some(schema) = in_schema {
        let escaped = schema.replace('\'', "''");
        parts.push(format!("schema_name = '{escaped}'"));
    }
    if let Some(db) = in_database {
        let escaped = db.replace('\'', "''");
        parts.push(format!("database_name = '{escaped}'"));
    }
    let mut suffix = String::new();
    if !parts.is_empty() {
        suffix.push_str(" WHERE ");
        suffix.push_str(&parts.join(" AND "));
    }
    if let Some(n) = limit {
        use std::fmt::Write;
        let _ = write!(suffix, " LIMIT {n}");
    }
    suffix
}

/// Parsed filter clauses from a SHOW SEMANTIC command.
struct ShowClauses<'a> {
    like_pattern: Option<String>,
    in_view: Option<&'a str>,
    in_schema: Option<&'a str>,
    in_database: Option<&'a str>,
    for_metric: Option<&'a str>,
    starts_with: Option<String>,
    limit: Option<u64>,
}

/// Parse a keyword + identifier pair from text starting with IN.
///
/// Checks for `IN SCHEMA <name>` or `IN DATABASE <name>`.
/// Returns `(remaining_text, in_schema, in_database)`.
fn parse_in_scope(rest: &str) -> Result<(&str, Option<&str>, Option<&str>), String> {
    let after_in = rest[2..].trim_start();

    // Try to match a keyword (SCHEMA or DATABASE) followed by an identifier.
    let (keyword, kw_len, label) = if after_in.len() >= 6
        && after_in[..6].eq_ignore_ascii_case("SCHEMA")
        && (after_in.len() == 6 || after_in.as_bytes()[6].is_ascii_whitespace())
    {
        ("SCHEMA", 6, "schema")
    } else if after_in.len() >= 8
        && after_in[..8].eq_ignore_ascii_case("DATABASE")
        && (after_in.len() == 8 || after_in.as_bytes()[8].is_ascii_whitespace())
    {
        ("DATABASE", 8, "database")
    } else {
        return Err(
            "SHOW SEMANTIC VIEWS requires IN SCHEMA <name> or IN DATABASE <name>".to_string(),
        );
    };

    let after_kw = after_in[kw_len..].trim_start();
    if after_kw.is_empty() {
        return Err(format!("Missing {label} name after IN {keyword}"));
    }
    let name_end = after_kw
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_kw.len());
    let name = &after_kw[..name_end];
    let remaining = after_kw[name_end..].trim_start();

    if keyword == "SCHEMA" {
        Ok((remaining, Some(name), None))
    } else {
        Ok((remaining, None, Some(name)))
    }
}

/// Parse FOR METRIC clause (only valid for `ShowDimensions`).
///
/// Returns `(remaining_text, metric_name)`.
fn parse_for_metric<'a>(rest: &'a str, entity: &str) -> Result<(&'a str, &'a str), String> {
    let after_for = rest[3..].trim_start();
    if after_for.len() < 6 || !after_for[..6].eq_ignore_ascii_case("METRIC") {
        return Err("Expected FOR METRIC after view name. \
             Usage: SHOW SEMANTIC DIMENSIONS [LIKE '<pattern>'] [IN view_name] \
             [FOR METRIC metric_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            .to_string());
    }
    let _ = entity;
    let after_metric = after_for[6..].trim_start();
    if after_metric.is_empty() {
        return Err("Missing metric name after FOR METRIC".to_string());
    }
    let name_end = after_metric
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_metric.len());
    Ok((
        after_metric[name_end..].trim_start(),
        &after_metric[..name_end],
    ))
}

/// Parse optional SHOW SEMANTIC filter clauses from text after the prefix.
///
/// Clause order (Snowflake): LIKE, IN, FOR METRIC, STARTS WITH, LIMIT.
fn parse_show_filter_clauses<'a>(
    after_prefix: &'a str,
    kind: DdlKind,
) -> Result<ShowClauses<'a>, String> {
    let mut rest = after_prefix.trim();
    let mut like_pattern: Option<String> = None;
    let mut in_view: Option<&'a str> = None;
    let mut in_schema: Option<&'a str> = None;
    let mut in_database: Option<&'a str> = None;
    let mut for_metric: Option<&'a str> = None;
    let mut starts_with: Option<String> = None;
    let mut limit: Option<u64> = None;

    let entity = match kind {
        DdlKind::Show | DdlKind::ShowTerse => "VIEWS",
        DdlKind::ShowDimensions => "DIMENSIONS",
        DdlKind::ShowMetrics => "METRICS",
        _ => "FACTS",
    };

    // 1. Check for LIKE keyword
    if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("LIKE") {
        // Ensure it's followed by whitespace (not just a prefix match)
        if rest.len() == 4 || rest.as_bytes()[4].is_ascii_whitespace() {
            rest = rest[4..].trim_start();
            let (pattern, consumed) = extract_quoted_string(rest)?;
            like_pattern = Some(pattern);
            rest = rest[consumed..].trim_start();
        }
    }

    // 2. Check for IN keyword
    if rest.len() >= 2
        && rest[..2].eq_ignore_ascii_case("IN")
        && (rest.len() == 2 || rest.as_bytes()[2].is_ascii_whitespace())
    {
        if kind == DdlKind::Show || kind == DdlKind::ShowTerse {
            let (remaining, schema, database) = parse_in_scope(rest)?;
            rest = remaining;
            in_schema = schema;
            in_database = database;
        } else {
            rest = rest[2..].trim_start();
            if rest.is_empty() {
                return Err("Missing view name after IN".to_string());
            }
            let name_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
            in_view = Some(&rest[..name_end]);
            rest = rest[name_end..].trim_start();
        }
    }

    // 3. Check for FOR METRIC (only for ShowDimensions)
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case("FOR") {
        if kind != DdlKind::ShowDimensions {
            return Err(format!(
                "FOR METRIC is only valid for SHOW SEMANTIC DIMENSIONS, not SHOW SEMANTIC {entity}"
            ));
        }
        let (remaining, metric_name) = parse_for_metric(rest, entity)?;
        rest = remaining;
        for_metric = Some(metric_name);
    }

    // 4. Check for STARTS WITH
    if rest.len() >= 6 && rest[..6].eq_ignore_ascii_case("STARTS") {
        rest = rest[6..].trim_start();
        if rest.len() < 4 || !rest[..4].eq_ignore_ascii_case("WITH") {
            return Err(format!(
                "Expected STARTS WITH. \
                 Usage: SHOW SEMANTIC {entity} [LIKE '<pattern>'] [IN view_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            ));
        }
        rest = rest[4..].trim_start();
        let (prefix, consumed) = extract_quoted_string(rest)?;
        starts_with = Some(prefix);
        rest = rest[consumed..].trim_start();
    }

    // 5. Check for LIMIT
    if rest.len() >= 5 && rest[..5].eq_ignore_ascii_case("LIMIT") {
        rest = rest[5..].trim_start();
        let token_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        let token = &rest[..token_end];
        let n: u64 = token
            .parse()
            .map_err(|_| format!("LIMIT must be a positive integer, got: '{token}'"))?;
        limit = Some(n);
        rest = rest[token_end..].trim_start();
    }

    // 6. If any text remains, error with usage hint
    if !rest.is_empty() {
        let usage = if kind == DdlKind::ShowDimensions {
            format!(
                "Unexpected tokens: '{rest}'. \
                 Usage: SHOW SEMANTIC DIMENSIONS [LIKE '<pattern>'] [IN view_name] [FOR METRIC metric_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            )
        } else {
            format!(
                "Unexpected tokens: '{rest}'. \
                 Usage: SHOW SEMANTIC {entity} [LIKE '<pattern>'] [IN view_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            )
        };
        return Err(usage);
    }

    Ok(ShowClauses {
        like_pattern,
        in_view,
        in_schema,
        in_database,
        for_metric,
        starts_with,
        limit,
    })
}

/// Rewrite an ALTER SEMANTIC VIEW sub-operation to a table function call.
///
/// Dispatches on RENAME TO, SET COMMENT, and UNSET COMMENT.
fn rewrite_alter(trimmed: &str, plen: usize, kind: DdlKind) -> Result<String, String> {
    let after_prefix = trimmed[plen..].trim();
    let name_end = after_prefix
        .find(|c: char| c.is_whitespace())
        .ok_or("Missing view name after ALTER SEMANTIC VIEW")?;
    let view_name = &after_prefix[..name_end];
    let rest = after_prefix[name_end..].trim();
    let rest_upper = rest.to_ascii_uppercase();
    let safe_name = view_name.replace('\'', "''");

    let if_exists_suffix = if kind == DdlKind::AlterIfExists {
        "_if_exists"
    } else {
        ""
    };

    if rest_upper.starts_with("RENAME TO") {
        let new_name = rest["RENAME TO".len()..].trim();
        if new_name.is_empty() {
            return Err("Missing new name after RENAME TO".to_string());
        }
        let safe_new = new_name.replace('\'', "''");
        let alter_fn = format!("alter_semantic_view_rename{if_exists_suffix}");
        Ok(format!(
            "SELECT * FROM {alter_fn}('{safe_name}', '{safe_new}')"
        ))
    } else if rest_upper.starts_with("SET COMMENT") {
        let after_set_comment = rest["SET COMMENT".len()..].trim_start();
        if !after_set_comment.starts_with('=') {
            return Err("Expected '=' after SET COMMENT".to_string());
        }
        let after_eq = after_set_comment[1..].trim_start();
        if !after_eq.starts_with('\'') {
            return Err("Expected single-quoted string after SET COMMENT =".to_string());
        }
        // Extract the quoted string handling '' escaping
        let (comment_value, _consumed) =
            extract_quoted_string(after_eq).map_err(|e| format!("Invalid comment string: {e}"))?;
        // Re-escape for SQL embedding
        let safe_comment = comment_value.replace('\'', "''");
        let alter_fn = format!("alter_semantic_view_set_comment{if_exists_suffix}");
        Ok(format!(
            "SELECT * FROM {alter_fn}('{safe_name}', '{safe_comment}')"
        ))
    } else if rest_upper.starts_with("UNSET COMMENT") {
        let alter_fn = format!("alter_semantic_view_unset_comment{if_exists_suffix}");
        Ok(format!("SELECT * FROM {alter_fn}('{safe_name}')"))
    } else {
        Err(
            "Unsupported ALTER operation. Supported: RENAME TO, SET COMMENT, UNSET COMMENT."
                .to_string(),
        )
    }
}

/// Rewrite a name-only or SHOW semantic view DDL statement to its function call.
///
/// Handles only:
/// - Name-only (DROP, DESCRIBE): `SELECT * FROM fn('name')`
/// - SHOW forms: `SELECT * FROM list_semantic_views()` with optional LIKE/STARTS WITH/LIMIT
///
/// CREATE forms must go through `validate_and_rewrite` -> `rewrite_ddl_keyword_body`.
fn rewrite_ddl(query: &str) -> Result<String, String> {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();

    let (kind, plen) = detect_ddl_prefix(trimmed)
        .ok_or_else(|| "Not a semantic view DDL statement".to_string())?;

    let fn_name = function_name(kind);

    match kind {
        // CREATE forms no longer supported via rewrite_ddl -- use validate_and_rewrite
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists => {
            Err("CREATE forms must use validate_and_rewrite".to_string())
        }
        // Name-only forms (DROP, DESCRIBE, SHOW COLUMNS IN SEMANTIC VIEW)
        DdlKind::Drop | DdlKind::DropIfExists | DdlKind::Describe | DdlKind::ShowColumns => {
            let name = extract_name_only(trimmed, plen)?;
            let safe_name = name.replace('\'', "''");
            Ok(format!("SELECT * FROM {fn_name}('{safe_name}')"))
        }
        // SHOW SEMANTIC VIEWS/DIMENSIONS/METRICS/FACTS: optional LIKE/IN/FOR METRIC/STARTS WITH/LIMIT
        DdlKind::Show
        | DdlKind::ShowTerse
        | DdlKind::ShowDimensions
        | DdlKind::ShowMetrics
        | DdlKind::ShowFacts => {
            let after_prefix = trimmed[plen..].trim();
            let clauses = parse_show_filter_clauses(after_prefix, kind)?;

            // Validate FOR METRIC requires IN
            if clauses.for_metric.is_some() && clauses.in_view.is_none() {
                return Err("FOR METRIC requires IN view_name".to_string());
            }

            // Build base SELECT
            let base = if let Some(view_name) = clauses.in_view {
                let safe_name = view_name.replace('\'', "''");
                if let Some(metric_name) = clauses.for_metric {
                    let safe_metric = metric_name.replace('\'', "''");
                    format!(
                        "SELECT * FROM show_semantic_dimensions_for_metric('{safe_name}', '{safe_metric}')"
                    )
                } else {
                    format!("SELECT * FROM {fn_name}('{safe_name}')")
                }
            } else {
                let all_fn = match kind {
                    DdlKind::Show => "list_semantic_views",
                    DdlKind::ShowTerse => "list_terse_semantic_views",
                    DdlKind::ShowDimensions => "show_semantic_dimensions_all",
                    DdlKind::ShowMetrics => "show_semantic_metrics_all",
                    DdlKind::ShowFacts => "show_semantic_facts_all",
                    _ => unreachable!(),
                };
                format!("SELECT * FROM {all_fn}()")
            };

            // Append filter suffix
            let suffix = build_filter_suffix(
                clauses.like_pattern.as_deref(),
                clauses.starts_with.as_deref(),
                clauses.limit,
                clauses.in_schema,
                clauses.in_database,
            );
            Ok(format!("{base}{suffix}"))
        }
        // ALTER: sub-operation dispatch (RENAME TO, SET COMMENT, UNSET COMMENT)
        DdlKind::Alter | DdlKind::AlterIfExists => rewrite_alter(trimmed, plen, kind),
    }
}

// ---------------------------------------------------------------------------
// Name extraction
// ---------------------------------------------------------------------------

/// Extract the view name from a semantic view DDL statement.
///
/// Returns `Ok(Some(name))` for DDL forms that have a view name (CREATE, DROP,
/// DESCRIBE), and `Ok(None)` for SHOW (no name). Returns `Err` if the query
/// is not a semantic view DDL statement or is malformed.
pub fn extract_ddl_name(query: &str) -> Result<Option<String>, String> {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();

    let (kind, plen) = detect_ddl_prefix(trimmed)
        .ok_or_else(|| "Not a semantic view DDL statement".to_string())?;

    match kind {
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists => {
            // Extract name directly: after prefix, trim whitespace, take up to
            // whitespace or '(' (same logic as validate_create_body).
            let after_prefix = trimmed[plen..].trim_start();
            if after_prefix.is_empty() {
                return Err("Missing view name".to_string());
            }
            let name_end = after_prefix
                .find(|c: char| c.is_whitespace() || c == '(')
                .unwrap_or(after_prefix.len());
            let name = &after_prefix[..name_end];
            if name.is_empty() {
                return Err("Missing view name".to_string());
            }
            Ok(Some(name.to_string()))
        }
        DdlKind::Drop
        | DdlKind::DropIfExists
        | DdlKind::Describe
        | DdlKind::ShowColumns
        | DdlKind::Alter
        | DdlKind::AlterIfExists => {
            let name = extract_name_only(trimmed, plen)?;
            Ok(Some(name))
        }
        DdlKind::Show | DdlKind::ShowTerse => Ok(None),
        DdlKind::ShowDimensions | DdlKind::ShowMetrics | DdlKind::ShowFacts => {
            let after_prefix = trimmed[plen..].trim();
            if after_prefix.is_empty() {
                return Ok(None); // Cross-view form, no specific name
            }
            let mut rest = after_prefix;
            // Skip LIKE clause if present (LIKE appears before IN)
            if rest.len() >= 4
                && rest[..4].eq_ignore_ascii_case("LIKE")
                && (rest.len() == 4 || rest.as_bytes()[4].is_ascii_whitespace())
            {
                rest = rest[4..].trim_start();
                // Skip the quoted string
                if let Ok((_pattern, consumed)) = extract_quoted_string(rest) {
                    rest = rest[consumed..].trim_start();
                } else {
                    return Ok(None);
                }
            }
            // Check for IN keyword
            if rest.len() >= 2
                && rest[..2].eq_ignore_ascii_case("IN")
                && (rest.len() == 2 || rest.as_bytes()[2].is_ascii_whitespace())
            {
                let after_in = rest[2..].trim();
                if after_in.is_empty() {
                    return Ok(None);
                }
                let name_end = after_in
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(after_in.len());
                Ok(Some(after_in[..name_end].to_string()))
            } else {
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Validation layer: ParseError, detect_near_miss, validate_and_rewrite
// ---------------------------------------------------------------------------

/// The DDL prefixes used for near-miss detection.
const DDL_PREFIXES: &[&str] = &[
    "create semantic view",
    "create or replace semantic view",
    "create semantic view if not exists",
    "drop semantic view",
    "drop semantic view if exists",
    "describe semantic view",
    "show semantic views",
    "show terse semantic views",
    "show columns in semantic view",
    "alter semantic view",
    "alter semantic view if exists",
    "show semantic dimensions",
    "show semantic dimensions for metric",
    "show semantic metrics",
    "show semantic facts",
];

/// Detect near-miss DDL prefixes using fuzzy matching.
///
/// If the beginning of the query is close (Levenshtein distance <= 3) to one
/// of the 7 known DDL prefixes, returns a `ParseError` suggesting the correct
/// prefix. Returns `None` if no near-miss is found.
#[must_use]
pub fn detect_near_miss(query: &str) -> Option<ParseError> {
    let trimmed = query.trim();
    let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
    let lower = trimmed_no_semi.to_ascii_lowercase();

    let mut best: Option<(usize, &str)> = None;

    for &prefix in DDL_PREFIXES {
        // Extract the first N words from the query where N is the number of
        // words in this DDL prefix. This ensures we compare apples-to-apples
        // regardless of what follows the prefix in the query.
        let prefix_word_count = prefix.split_whitespace().count();
        let query_words: Vec<&str> = lower.split_whitespace().collect();
        let query_slice_words = &query_words[..query_words.len().min(prefix_word_count)];
        let query_slice = query_slice_words.join(" ");

        let dist = strsim::levenshtein(&query_slice, prefix);
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, prefix));
                }
            } else {
                best = Some((dist, prefix));
            }
        }
    }

    best.map(|(_, prefix)| {
        let trim_offset = query.len() - query.trim_start().len();
        ParseError {
            message: format!(
                "Unknown statement. Did you mean '{}'?",
                prefix.to_uppercase()
            ),
            position: Some(trim_offset),
        }
    })
}

/// Validate a DDL statement and rewrite it if valid.
///
/// This is the main entry point for the validation layer. CREATE forms go through
/// the AS-body keyword parser. DROP/DESCRIBE/SHOW forms are rewritten directly.
///
/// Returns:
/// - `Ok(Some(sql))` -- DDL detected and validated, rewritten SQL returned
/// - `Ok(None)` -- not a semantic view DDL statement
/// - `Err(ParseError)` -- validation error with message and optional position
pub fn validate_and_rewrite(query: &str) -> Result<Option<String>, ParseError> {
    let trimmed = query.trim();
    let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
    let trim_offset = query.len() - query.trim_start().len();

    let Some((kind, plen)) = detect_ddl_prefix(trimmed_no_semi) else {
        return Ok(None);
    };

    match kind {
        // CREATE-with-body forms: validate clauses before rewriting
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists => {
            validate_create_body(query, trimmed_no_semi, trim_offset, plen, kind)
        }
        // Name-only forms: validate name is present
        DdlKind::Drop | DdlKind::DropIfExists | DdlKind::Describe => {
            let after_prefix = trimmed_no_semi[plen..].trim();
            if after_prefix.is_empty() {
                return Err(ParseError {
                    message: "Missing view name.".to_string(),
                    position: Some(trim_offset + plen),
                });
            }
            rewrite_ddl(query).map(Some).map_err(|e| ParseError {
                message: e,
                position: Some(trim_offset + plen),
            })
        }
        // SHOW [TERSE] SEMANTIC VIEWS: optional filter/scope clauses
        DdlKind::Show | DdlKind::ShowTerse => {
            rewrite_ddl(query).map(Some).map_err(|e| ParseError {
                message: e,
                position: Some(trim_offset + plen),
            })
        }
        // SHOW COLUMNS IN SEMANTIC VIEW: name-only form
        DdlKind::ShowColumns => {
            let after_prefix = trimmed_no_semi[plen..].trim();
            if after_prefix.is_empty() {
                return Err(ParseError {
                    message: "Missing view name.".to_string(),
                    position: Some(trim_offset + plen),
                });
            }
            rewrite_ddl(query).map(Some).map_err(|e| ParseError {
                message: e,
                position: Some(trim_offset + plen),
            })
        }
        // SHOW SEMANTIC DIMENSIONS/METRICS/FACTS: optional IN view_name
        DdlKind::ShowDimensions | DdlKind::ShowMetrics | DdlKind::ShowFacts => {
            rewrite_ddl(query).map(Some).map_err(|e| ParseError {
                message: e,
                position: Some(trim_offset + plen),
            })
        }
        // ALTER forms: validate sub-operation (RENAME TO, SET COMMENT, UNSET COMMENT)
        DdlKind::Alter | DdlKind::AlterIfExists => {
            validate_alter(trimmed_no_semi, trim_offset, plen)?;
            rewrite_ddl(query).map(Some).map_err(|e| ParseError {
                message: e,
                position: Some(trim_offset + plen),
            })
        }
    }
}

/// Validate an ALTER SEMANTIC VIEW statement's sub-operation before rewriting.
///
/// Checks that the view name and a valid sub-operation (RENAME TO, SET COMMENT,
/// UNSET COMMENT) are present, returning a `ParseError` on validation failure.
fn validate_alter(
    trimmed_no_semi: &str,
    trim_offset: usize,
    plen: usize,
) -> Result<(), ParseError> {
    let after_prefix = trimmed_no_semi[plen..].trim();
    if after_prefix.is_empty() {
        return Err(ParseError {
            message: "Missing view name after ALTER SEMANTIC VIEW.".to_string(),
            position: Some(trim_offset + plen),
        });
    }
    let name_end = after_prefix
        .find(|c: char| c.is_whitespace())
        .ok_or_else(|| ParseError {
            message: "Missing ALTER operation after view name. Supported: RENAME TO, SET COMMENT, UNSET COMMENT.".to_string(),
            position: Some(trim_offset + plen + after_prefix.len()),
        })?;
    let rest = after_prefix[name_end..].trim();
    let rest_upper = rest.to_ascii_uppercase();

    if rest_upper.starts_with("RENAME TO") {
        let new_name_str = rest["RENAME TO".len()..].trim();
        if new_name_str.is_empty() {
            return Err(ParseError {
                message: "Missing new name after RENAME TO.".to_string(),
                position: Some(trim_offset + plen + after_prefix.len()),
            });
        }
    } else if rest_upper.starts_with("SET COMMENT") {
        let after_set_comment = rest["SET COMMENT".len()..].trim_start();
        if !after_set_comment.starts_with('=') {
            return Err(ParseError {
                message: "Expected '=' after SET COMMENT.".to_string(),
                position: Some(trim_offset + plen + name_end),
            });
        }
        let after_eq = after_set_comment[1..].trim_start();
        if !after_eq.starts_with('\'') {
            return Err(ParseError {
                message: "Expected single-quoted string after SET COMMENT =.".to_string(),
                position: Some(trim_offset + plen + name_end),
            });
        }
        let _ = extract_quoted_string(after_eq).map_err(|e| ParseError {
            message: format!("Invalid comment string: {e}"),
            position: Some(trim_offset + plen + name_end),
        })?;
    } else if rest_upper.starts_with("UNSET COMMENT") {
        // Valid -- no further arguments needed
    } else {
        return Err(ParseError {
            message:
                "Unsupported ALTER operation. Supported: RENAME TO, SET COMMENT, UNSET COMMENT."
                    .to_string(),
            position: Some(trim_offset + plen + name_end),
        });
    }
    Ok(())
}

/// Extract an optional COMMENT = '...' between the view name and the AS keyword.
/// Returns (`comment_option`, `remaining_text_after_comment`).
///
/// Phase 43: Supports `CREATE SEMANTIC VIEW my_view COMMENT = 'desc' AS ...`
fn extract_view_comment(text: &str) -> Result<(Option<String>, &str), ParseError> {
    let upper = text.to_ascii_uppercase();
    if upper.starts_with("COMMENT") {
        // Verify word boundary (not e.g. COMMENTARY)
        if text.len() > 7 && text.as_bytes()[7].is_ascii_alphanumeric() {
            return Ok((None, text));
        }
        let after_kw = text[7..].trim_start();
        if !after_kw.starts_with('=') {
            return Err(ParseError {
                message: "Expected '=' after COMMENT keyword.".to_string(),
                position: None,
            });
        }
        let after_eq = after_kw[1..].trim_start();
        if !after_eq.starts_with('\'') {
            return Err(ParseError {
                message: "Expected single-quoted string after COMMENT =.".to_string(),
                position: None,
            });
        }
        // Extract the quoted string handling '' escaping
        let bytes = after_eq.as_bytes();
        let mut i = 1; // skip opening quote
        let mut value = String::new();
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    value.push('\'');
                    i += 2;
                    continue;
                }
                // Closing quote found
                let remaining = &after_eq[i + 1..];
                return Ok((Some(value), remaining));
            }
            value.push(bytes[i] as char);
            i += 1;
        }
        Err(ParseError {
            message: "Unclosed single-quoted string in view-level COMMENT.".to_string(),
            position: None,
        })
    } else {
        Ok((None, text))
    }
}

/// Validate a CREATE-with-body DDL statement and rewrite it if valid.
fn validate_create_body(
    _query: &str,
    trimmed_no_semi: &str,
    trim_offset: usize,
    plen: usize,
    kind: DdlKind,
) -> Result<Option<String>, ParseError> {
    let after_prefix = trimmed_no_semi[plen..].trim_start();
    if after_prefix.is_empty() {
        return Err(ParseError {
            message: "Missing view name after DDL prefix.".to_string(),
            position: Some(trim_offset + plen),
        });
    }

    let name_end = after_prefix
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(after_prefix.len());
    let name = &after_prefix[..name_end];
    if name.is_empty() {
        return Err(ParseError {
            message: "Missing view name after DDL prefix.".to_string(),
            position: Some(trim_offset + plen),
        });
    }

    let after_name = &after_prefix[name_end..];

    // --- Phase 43: View-level COMMENT extraction ---
    // Extract optional COMMENT = '...' between the view name and the AS keyword.
    let after_name_pre = after_name.trim_start();
    let (view_comment, remaining_after_comment) = extract_view_comment(after_name_pre)?;

    // --- AS keyword body path (new in Phase 25) ---
    // If text after the name starts with "AS" (whitespace-delimited), route to the
    // AS-body keyword parser instead of the legacy paren-body path.
    let after_name_trimmed = remaining_after_comment.trim_start();
    let is_as_body = after_name_trimmed
        .get(..2)
        .is_some_and(|s| s.eq_ignore_ascii_case("AS"))
        && (after_name_trimmed.len() == 2
            || after_name_trimmed.as_bytes()[2].is_ascii_whitespace());
    if is_as_body {
        // Compute the byte offset of after_name_trimmed[0] within trimmed_no_semi.
        // after_prefix starts at: plen + whitespace-gap between trimmed_no_semi[plen..] and after_prefix
        let after_prefix_in_tns = plen + (trimmed_no_semi.len() - plen - after_prefix.len());
        // after_name starts at name_end within after_prefix
        let after_name_in_tns = after_prefix_in_tns + name_end;
        // Calculate the byte offset of after_name_trimmed relative to trimmed_no_semi
        // after_name_trimmed is a slice within after_name, so compute by pointer arithmetic
        let trimmed_start_in_after_name = after_name.len() - remaining_after_comment.len()
            + (remaining_after_comment.len() - after_name_trimmed.len());
        let body_offset_in_tns = after_name_in_tns + trimmed_start_in_after_name;
        let body_offset = trim_offset + body_offset_in_tns;
        return rewrite_ddl_keyword_body(kind, name, after_name_trimmed, body_offset, view_comment);
    }
    // --- End AS keyword body path ---

    // --- FROM YAML body path (Phase 52) ---
    let is_yaml_body = after_name_trimmed
        .get(..9)
        .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
        && (after_name_trimmed.len() == 9
            || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());
    if is_yaml_body {
        let yaml_text = after_name_trimmed[9..].trim_start();
        return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment);
    }
    // --- End FROM YAML body path ---

    // Non-AS/FROM-YAML syntax rejected -- AS keyword or FROM YAML required after view name.
    let pos_in_trimmed = plen + (trimmed_no_semi.len() - plen - after_prefix.len()) + name_end;
    Err(ParseError {
        message: "Expected 'AS' or 'FROM YAML' after view name. Use: CREATE SEMANTIC VIEW name AS TABLES (...) DIMENSIONS (...) METRICS (...) or: CREATE SEMANTIC VIEW name FROM YAML $$ ... $$".to_string(),
        position: Some(trim_offset + pos_in_trimmed),
    })
}

/// Rewrite an AS-body CREATE DDL statement to a JSON-parameterized function call.
///
/// Called when `validate_create_body` detects the `AS` keyword path.
/// Parses the keyword body via `parse_keyword_body`, serializes to JSON, and embeds in
/// a `SELECT * FROM create_semantic_view_from_json('name', 'json')` call.
fn rewrite_ddl_keyword_body(
    kind: DdlKind,
    name: &str,
    body_text: &str,              // text starting at "AS" (inclusive)
    body_offset: usize,           // byte offset of body_text[0] in original query
    view_comment: Option<String>, // Phase 43: optional view-level COMMENT
) -> Result<Option<String>, ParseError> {
    // 1. Call parse_keyword_body (body_text starts at "AS"; pass body_offset)
    let mut keyword_body = parse_keyword_body(body_text, body_offset)?;

    // Phase 33: Infer cardinality and resolve ref_columns before serialization.
    infer_cardinality(&keyword_body.tables, &mut keyword_body.relationships)?;

    // 2. Construct SemanticViewDefinition from KeywordBody
    //    base_table = first table's physical table name (backward compat)
    let base_table = keyword_body
        .tables
        .first()
        .map(|t| t.table.clone())
        .unwrap_or_default();

    let def = crate::model::SemanticViewDefinition {
        base_table,
        tables: keyword_body.tables,
        dimensions: keyword_body.dimensions,
        metrics: keyword_body.metrics,
        joins: keyword_body.relationships,
        facts: keyword_body.facts,
        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: view_comment,
    };

    // 3. Serialize to JSON
    let json = serde_json::to_string(&def).map_err(|e| ParseError {
        message: format!("Failed to serialize definition: {e}"),
        position: None,
    })?;

    // 4. SQL-escape single quotes in name and JSON
    let safe_name = name.replace('\'', "''");
    let safe_json = json.replace('\'', "''");

    // 5. Pick the correct _from_json function name based on DDL kind
    let fn_name = match kind {
        DdlKind::Create => "create_semantic_view_from_json",
        DdlKind::CreateOrReplace => "create_or_replace_semantic_view_from_json",
        DdlKind::CreateIfNotExists => "create_semantic_view_if_not_exists_from_json",
        _ => unreachable!("rewrite_ddl_keyword_body only called for CREATE forms"),
    };

    Ok(Some(format!(
        "SELECT * FROM {fn_name}('{safe_name}', '{safe_json}')"
    )))
}

// ---------------------------------------------------------------------------
// Phase 52: Dollar-quote extraction and YAML DDL rewrite
// ---------------------------------------------------------------------------

/// Extract content from a dollar-quoted string (`$$...$$` or `$tag$...$tag$`).
///
/// Returns `(content, bytes_consumed)` where `bytes_consumed` includes both
/// opening and closing delimiters. The content does NOT include the delimiters.
fn extract_dollar_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('$') {
        return Err(ParseError {
            message: "Expected '$' to begin dollar-quoted string".to_string(),
            position: None,
        });
    }
    let tag_end = input[1..].find('$').ok_or_else(|| ParseError {
        message: "Unterminated dollar-quote opening delimiter".to_string(),
        position: None,
    })? + 2;
    let delimiter = &input[..tag_end];
    let content_start = tag_end;
    let close_pos = input[content_start..]
        .find(delimiter)
        .ok_or_else(|| ParseError {
            message: format!("Unterminated dollar-quoted string (expected closing '{delimiter}')"),
            position: None,
        })?;
    let content = &input[content_start..content_start + close_pos];
    let total = content_start + close_pos + delimiter.len();
    Ok((content.to_string(), total))
}

/// Rewrite a FROM YAML dollar-quoted DDL statement to a JSON-parameterized function call.
///
/// Called when `validate_create_body` detects the `FROM YAML` keyword path.
/// Extracts dollar-quoted YAML, deserializes via `from_yaml_with_size_cap()`,
/// serializes to JSON, and embeds in a `SELECT * FROM create_semantic_view_from_json('name', 'json')` call.
fn rewrite_ddl_yaml_body(
    kind: DdlKind,
    name: &str,
    yaml_text: &str,
    view_comment: Option<String>,
) -> Result<Option<String>, ParseError> {
    let (yaml_content, consumed) = extract_dollar_quoted(yaml_text)?;

    let trailing = yaml_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after closing dollar-quote: '{trailing}'"),
            position: None,
        });
    }

    let mut def =
        crate::model::SemanticViewDefinition::from_yaml_with_size_cap(name, &yaml_content)
            .map_err(|e| ParseError {
                message: e,
                position: None,
            })?;

    if let Some(c) = view_comment {
        def.comment = Some(c);
    }

    if def.base_table.is_empty() {
        if let Some(first) = def.tables.first() {
            def.base_table = first.table.clone();
        }
    }

    infer_cardinality(&def.tables, &mut def.joins)?;

    let json = serde_json::to_string(&def).map_err(|e| ParseError {
        message: format!("Failed to serialize YAML definition: {e}"),
        position: None,
    })?;

    let safe_name = name.replace('\'', "''");
    let safe_json = json.replace('\'', "''");
    let fn_name = match kind {
        DdlKind::Create => "create_semantic_view_from_json",
        DdlKind::CreateOrReplace => "create_or_replace_semantic_view_from_json",
        DdlKind::CreateIfNotExists => "create_semantic_view_if_not_exists_from_json",
        _ => unreachable!("rewrite_ddl_yaml_body only called for CREATE forms"),
    };
    Ok(Some(format!(
        "SELECT * FROM {fn_name}('{safe_name}', '{safe_json}')"
    )))
}

// ---------------------------------------------------------------------------
// Phase 33: Cardinality inference
// ---------------------------------------------------------------------------

/// Infer cardinality for each relationship based on PK/UNIQUE constraints.
/// Also resolves `ref_columns` (the columns on the target side of the FK reference).
///
/// Two checks per relationship:
/// 1. Resolve `ref_columns`: if empty, use target's PK. If target has no PK, error.
/// 2. Infer cardinality: if FK columns match PK/UNIQUE on the `from_alias` table,
///    the relationship is `OneToOne`; otherwise `ManyToOne`.
pub(crate) fn infer_cardinality(
    tables: &[TableRef],
    relationships: &mut [Join],
) -> Result<(), ParseError> {
    for join in relationships.iter_mut() {
        if join.fk_columns.is_empty() {
            continue;
        }

        let to_alias_lower = join.table.to_ascii_lowercase();
        let from_alias_lower = join.from_alias.to_ascii_lowercase();

        // Find target table (REFERENCES target)
        let target = tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower);

        // Find source table (from_alias side)
        let source = tables
            .iter()
            .find(|t| t.alias.to_ascii_lowercase() == from_alias_lower);

        // Step 1: Resolve ref_columns
        if join.ref_columns.is_empty() {
            // REFERENCES target (no column list) -> use target's PK
            match target {
                Some(t) if !t.pk_columns.is_empty() => {
                    join.ref_columns.clone_from(&t.pk_columns);
                }
                Some(_) => {
                    // Target has no PK declared in DDL -- defer resolution.
                    // At bind time, `resolve_pk_from_catalog` will attempt to
                    // fill in pk_columns from the DuckDB catalog. If that also
                    // fails, `infer_cardinality` is re-run and this branch
                    // will be reached again (now as an error).
                    continue;
                }
                None => {
                    // Target not found -- will be caught by graph validation later
                }
            }
        }
        // When ref_columns was set explicitly (REFERENCES target(cols)),
        // validation against PK/UNIQUE on target happens in graph.rs (CARD-03).

        // Step 2: FK column count must match ref column count
        if !join.ref_columns.is_empty() && join.fk_columns.len() != join.ref_columns.len() {
            let rel_name = join.name.as_deref().unwrap_or("?");
            return Err(ParseError {
                message: format!(
                    "FK column count ({}) does not match referenced column count ({}) \
                     in relationship '{rel_name}'.",
                    join.fk_columns.len(),
                    join.ref_columns.len(),
                ),
                position: None,
            });
        }

        // Step 3: Infer cardinality from FK-side constraints (CARD-04)
        if let Some(source) = source {
            let fk_set: HashSet<String> = join
                .fk_columns
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();

            // Check against source PK
            let pk_set: HashSet<String> = source
                .pk_columns
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();

            if !pk_set.is_empty() && fk_set == pk_set {
                join.cardinality = Cardinality::OneToOne;
            } else {
                // Check against source UNIQUE constraints
                let matches_unique = source.unique_constraints.iter().any(|uc| {
                    let uc_set: HashSet<String> =
                        uc.iter().map(|c| c.to_ascii_lowercase()).collect();
                    fk_set == uc_set
                });
                join.cardinality = if matches_unique {
                    Cardinality::OneToOne
                } else {
                    Cardinality::ManyToOne
                };
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// FFI entry points (extension feature-gated)
// ---------------------------------------------------------------------------

/// FFI entry point for DDL validation with error reporting.
///
/// Validates a semantic view DDL statement and returns a tri-state result:
/// - 0 = success: rewritten SQL written to `sql_out`
/// - 1 = error: error message written to `error_out`, position to `*position_out`
/// - 2 = not ours: no output written
///
/// `position_out` is set to `u32::MAX` when no position is available.
///
/// # Safety
///
/// - `query_ptr` must point to valid UTF-8 bytes of length `query_len`.
/// - `sql_out` must point to a writable buffer of `sql_out_len` bytes.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes.
/// - `position_out` must point to a writable `u32`.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_validate_ddl_rust(
    query_ptr: *const u8,
    query_len: usize,
    sql_out: *mut u8,
    sql_out_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
    position_out: *mut u32,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if query_ptr.is_null() || query_len == 0 {
            return 2_u8; // not ours
        }
        // SAFETY: guaranteed valid UTF-8 by the caller (DuckDB query text)
        let query = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(query_ptr, query_len))
        };

        match validate_and_rewrite(query) {
            Ok(Some(sql)) => {
                unsafe { write_to_buffer(sql_out, sql_out_len, &sql) };
                0 // success
            }
            Ok(None) => {
                // Not a recognized DDL -- check for near-miss
                if let Some(err) = detect_near_miss(query) {
                    unsafe { write_to_buffer(error_out, error_out_len, &err.message) };
                    unsafe {
                        write_position(position_out, err.position);
                    }
                    1 // error (near-miss suggestion)
                } else {
                    2 // not ours
                }
            }
            Err(err) => {
                unsafe { write_to_buffer(error_out, error_out_len, &err.message) };
                unsafe {
                    write_position(position_out, err.position);
                }
                1 // error (validation failure)
            }
        }
    }));

    result.unwrap_or(2) // on panic: not ours
}

/// Write a position value to a raw `u32` pointer, using `u32::MAX` as sentinel
/// for "no position".
///
/// # Safety
///
/// `position_out` must point to a writable `u32`.
#[cfg(feature = "extension")]
unsafe fn write_position(position_out: *mut u32, position: Option<usize>) {
    if !position_out.is_null() {
        match position {
            Some(pos) => *position_out = u32::try_from(pos).unwrap_or(u32::MAX),
            None => *position_out = u32::MAX,
        }
    }
}

/// Write a string into a raw byte buffer, null-terminated and truncated to `len - 1`.
///
/// # Safety
///
/// `buf` must point to a writable buffer of at least `len` bytes.
#[cfg(feature = "extension")]
unsafe fn write_to_buffer(buf: *mut u8, len: usize, s: &str) {
    if buf.is_null() || len == 0 {
        return;
    }
    let max_copy = len - 1; // reserve space for null terminator
    let copy_len = s.len().min(max_copy);
    std::ptr::copy_nonoverlapping(s.as_ptr(), buf, copy_len);
    *buf.add(copy_len) = 0; // null terminate
}

/// FFI entry point for DDL rewriting (no execution), called from C++ `sv_ddl_bind`.
///
/// Rewrites a semantic view DDL statement into the corresponding function call
/// SQL string. The caller (C++) is responsible for executing it.
///
/// On success: writes the rewritten SQL to `sql_out` (null-terminated), returns 0.
/// On failure: writes the error message to `error_out` (null-terminated), returns 1.
///
/// # Safety
///
/// - `query_ptr` must point to valid UTF-8 bytes of length `query_len`.
/// - `sql_out` must point to a writable buffer of `sql_out_len` bytes.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_rewrite_ddl_rust(
    query_ptr: *const u8,
    query_len: usize,
    sql_out: *mut u8,
    sql_out_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<String, String> {
            if query_ptr.is_null() || query_len == 0 {
                return Err("Empty query".to_string());
            }
            // SAFETY: guaranteed valid UTF-8 by the caller (DuckDB query text)
            let query = unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(query_ptr, query_len))
            };

            // Use validate_and_rewrite for all DDL forms.
            // validate_and_rewrite returns:
            //   Ok(Some(sql)) -- DDL detected and rewritten
            //   Ok(None)      -- not our DDL (should not happen here since parse hook already accepted it)
            //   Err(ParseError) -- validation/parse error
            validate_and_rewrite(query)
                .map_err(|e| e.message)
                .and_then(|opt| opt.ok_or_else(|| "DDL not recognized".to_string()))
        },
    ));

    match result {
        Ok(Ok(sql)) => {
            unsafe { write_to_buffer(sql_out, sql_out_len, &sql) };
            0 // success
        }
        Ok(Err(err)) => {
            unsafe { write_to_buffer(error_out, error_out_len, &err) };
            1 // failure
        }
        Err(_panic) => {
            unsafe { write_to_buffer(error_out, error_out_len, "Internal panic in DDL rewrite") };
            1 // failure
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // detect_semantic_view_ddl tests (multi-prefix detection)
    // ===================================================================

    #[test]
    fn test_detect_create() {
        assert_eq!(
            detect_semantic_view_ddl("CREATE SEMANTIC VIEW x (...)"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_create_or_replace() {
        assert_eq!(
            detect_semantic_view_ddl("CREATE OR REPLACE SEMANTIC VIEW x (...)"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_create_if_not_exists() {
        assert_eq!(
            detect_semantic_view_ddl("CREATE SEMANTIC VIEW IF NOT EXISTS x (...)"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_drop() {
        assert_eq!(
            detect_semantic_view_ddl("DROP SEMANTIC VIEW x"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_drop_if_exists() {
        assert_eq!(
            detect_semantic_view_ddl("DROP SEMANTIC VIEW IF EXISTS x"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_describe() {
        assert_eq!(
            detect_semantic_view_ddl("DESCRIBE SEMANTIC VIEW x"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_show() {
        assert_eq!(
            detect_semantic_view_ddl("SHOW SEMANTIC VIEWS"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_case_insensitive_all_forms() {
        assert_eq!(
            detect_semantic_view_ddl("create or replace semantic view x (...)"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("drop semantic view if exists x"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("describe semantic view x"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("show semantic views"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_whitespace_and_semicolon() {
        assert_eq!(
            detect_semantic_view_ddl("  DROP SEMANTIC VIEW x  ;"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("\n\tSHOW SEMANTIC VIEWS;\n"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_detect_non_matching() {
        assert_eq!(detect_semantic_view_ddl("SELECT 1"), PARSE_NOT_OURS);
        assert_eq!(
            detect_semantic_view_ddl("CREATE TABLE t (id INT)"),
            PARSE_NOT_OURS
        );
        assert_eq!(detect_semantic_view_ddl(""), PARSE_NOT_OURS);
    }

    #[test]
    fn test_detect_describe_must_have_view() {
        // "DESCRIBE my_table" must NOT be intercepted
        assert_eq!(
            detect_semantic_view_ddl("DESCRIBE my_table"),
            PARSE_NOT_OURS
        );
    }

    #[test]
    fn test_detect_show_must_have_views() {
        // "SHOW TABLES" must NOT be intercepted
        assert_eq!(detect_semantic_view_ddl("SHOW TABLES"), PARSE_NOT_OURS);
    }

    // ===================================================================
    // detect_ddl_kind tests
    // ===================================================================

    #[test]
    fn test_ddl_kind_create() {
        assert_eq!(
            detect_ddl_kind("CREATE SEMANTIC VIEW x (...)"),
            Some(DdlKind::Create)
        );
    }

    #[test]
    fn test_ddl_kind_create_or_replace() {
        // Must be CreateOrReplace, NOT Create
        assert_eq!(
            detect_ddl_kind("CREATE OR REPLACE SEMANTIC VIEW x (...)"),
            Some(DdlKind::CreateOrReplace)
        );
    }

    #[test]
    fn test_ddl_kind_create_if_not_exists() {
        // Must be CreateIfNotExists, NOT Create
        assert_eq!(
            detect_ddl_kind("CREATE SEMANTIC VIEW IF NOT EXISTS x (...)"),
            Some(DdlKind::CreateIfNotExists)
        );
    }

    #[test]
    fn test_ddl_kind_drop() {
        assert_eq!(detect_ddl_kind("DROP SEMANTIC VIEW x"), Some(DdlKind::Drop));
    }

    #[test]
    fn test_ddl_kind_drop_if_exists() {
        // Must be DropIfExists, NOT Drop
        assert_eq!(
            detect_ddl_kind("DROP SEMANTIC VIEW IF EXISTS x"),
            Some(DdlKind::DropIfExists)
        );
    }

    #[test]
    fn test_ddl_kind_describe() {
        assert_eq!(
            detect_ddl_kind("DESCRIBE SEMANTIC VIEW x"),
            Some(DdlKind::Describe)
        );
    }

    #[test]
    fn test_ddl_kind_show() {
        assert_eq!(detect_ddl_kind("SHOW SEMANTIC VIEWS"), Some(DdlKind::Show));
    }

    #[test]
    fn test_ddl_kind_none() {
        assert_eq!(detect_ddl_kind("SELECT 1"), None);
    }

    // ===================================================================
    // rewrite_ddl tests (name-only and no-args forms only; CREATE rejected)
    // ===================================================================

    #[test]
    fn test_rewrite_create_rejected() {
        let err = rewrite_ddl("CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])")
            .unwrap_err();
        assert!(
            err.contains("validate_and_rewrite"),
            "CREATE forms should be rejected by rewrite_ddl, got: {err}"
        );
    }

    #[test]
    fn test_rewrite_drop() {
        let sql = rewrite_ddl("DROP SEMANTIC VIEW sales").unwrap();
        assert_eq!(sql, "SELECT * FROM drop_semantic_view('sales')");
    }

    #[test]
    fn test_rewrite_drop_if_exists() {
        let sql = rewrite_ddl("DROP SEMANTIC VIEW IF EXISTS sales").unwrap();
        assert_eq!(sql, "SELECT * FROM drop_semantic_view_if_exists('sales')");
    }

    #[test]
    fn test_rewrite_describe() {
        let sql = rewrite_ddl("DESCRIBE SEMANTIC VIEW sales").unwrap();
        assert_eq!(sql, "SELECT * FROM describe_semantic_view('sales')");
    }

    #[test]
    fn test_rewrite_show() {
        let sql = rewrite_ddl("SHOW SEMANTIC VIEWS").unwrap();
        assert_eq!(sql, "SELECT * FROM list_semantic_views()");
    }

    #[test]
    fn test_rewrite_name_with_single_quote() {
        let sql = rewrite_ddl("DROP SEMANTIC VIEW it's_a_view").unwrap();
        assert_eq!(sql, "SELECT * FROM drop_semantic_view('it''s_a_view')");
    }

    #[test]
    fn test_rewrite_drop_missing_name() {
        let err = rewrite_ddl("DROP SEMANTIC VIEW").unwrap_err();
        assert!(err.contains("Missing view name"), "got: {err}");
    }

    #[test]
    fn test_rewrite_not_semantic() {
        let err = rewrite_ddl("SELECT 1").unwrap_err();
        assert!(err.contains("Not a semantic view DDL"), "got: {err}");
    }

    // ===================================================================
    // extract_ddl_name tests
    // ===================================================================

    #[test]
    fn test_extract_name_drop() {
        assert_eq!(
            extract_ddl_name("DROP SEMANTIC VIEW x").unwrap(),
            Some("x".to_string())
        );
    }

    #[test]
    fn test_extract_name_drop_if_exists() {
        assert_eq!(
            extract_ddl_name("DROP SEMANTIC VIEW IF EXISTS x").unwrap(),
            Some("x".to_string())
        );
    }

    #[test]
    fn test_extract_name_describe() {
        assert_eq!(
            extract_ddl_name("DESCRIBE SEMANTIC VIEW x").unwrap(),
            Some("x".to_string())
        );
    }

    #[test]
    fn test_extract_name_show() {
        assert_eq!(extract_ddl_name("SHOW SEMANTIC VIEWS").unwrap(), None);
    }

    #[test]
    fn test_extract_name_create() {
        assert_eq!(
            extract_ddl_name("CREATE SEMANTIC VIEW x (body)").unwrap(),
            Some("x".to_string())
        );
    }

    #[test]
    fn test_extract_name_create_or_replace() {
        assert_eq!(
            extract_ddl_name("CREATE OR REPLACE SEMANTIC VIEW x (body)").unwrap(),
            Some("x".to_string())
        );
    }

    // ===================================================================
    // Additional detect_semantic_view_ddl coverage (legacy test cases)
    // ===================================================================

    #[test]
    fn test_basic_detection() {
        assert_eq!(
            detect_semantic_view_ddl("CREATE SEMANTIC VIEW test (...)"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(
            detect_semantic_view_ddl("create semantic view test"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("Create Semantic View test"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("CREATE semantic VIEW test"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_leading_whitespace() {
        assert_eq!(
            detect_semantic_view_ddl("  CREATE SEMANTIC VIEW test"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("\n\tCREATE SEMANTIC VIEW test"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_trailing_semicolon() {
        assert_eq!(
            detect_semantic_view_ddl("CREATE SEMANTIC VIEW test;"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("CREATE SEMANTIC VIEW test ;"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_semantic_view_ddl("CREATE SEMANTIC VIEW test ;\n"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_non_matching() {
        assert_eq!(detect_semantic_view_ddl("SELECT 1"), PARSE_NOT_OURS);
        assert_eq!(
            detect_semantic_view_ddl("CREATE TABLE test"),
            PARSE_NOT_OURS
        );
        assert_eq!(detect_semantic_view_ddl("CREATE VIEW test"), PARSE_NOT_OURS);
        assert_eq!(detect_semantic_view_ddl(""), PARSE_NOT_OURS);
        assert_eq!(detect_semantic_view_ddl(";"), PARSE_NOT_OURS);
        assert_eq!(detect_semantic_view_ddl("CREATE"), PARSE_NOT_OURS);
    }

    #[test]
    fn test_too_short() {
        assert_eq!(
            detect_semantic_view_ddl("create semantic vie"),
            PARSE_NOT_OURS
        );
    }

    #[test]
    fn test_exact_prefix_only() {
        assert_eq!(
            detect_semantic_view_ddl("create semantic view"),
            PARSE_DETECTED
        );
    }

    // ===================================================================
    // validate_and_rewrite tests
    // ===================================================================

    #[test]
    fn test_validate_and_rewrite_rejects_paren_body() {
        // CLN-01: non-AS-body syntax rejected with clear error
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Expected 'AS' or 'FROM YAML'"),
            "Expected 'Expected AS or FROM YAML' error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_and_rewrite_not_ours() {
        let result = validate_and_rewrite("SELECT 1");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_validate_and_rewrite_drop() {
        // Non-CREATE forms should pass through without clause validation
        let result = validate_and_rewrite("DROP SEMANTIC VIEW x");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_validate_and_rewrite_show() {
        let result = validate_and_rewrite("SHOW SEMANTIC VIEWS");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_validate_and_rewrite_describe() {
        let result = validate_and_rewrite("DESCRIBE SEMANTIC VIEW sv1");
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(sql.is_some(), "Expected Some(rewritten SQL) for DESCRIBE");
    }

    #[test]
    fn test_validate_and_rewrite_drop_if_exists() {
        let result = validate_and_rewrite("DROP SEMANTIC VIEW IF EXISTS sv1");
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(
            sql.is_some(),
            "Expected Some(rewritten SQL) for DROP IF EXISTS"
        );
    }

    // ===================================================================
    // detect_near_miss tests
    // ===================================================================

    #[test]
    fn test_near_miss_creat() {
        let result = detect_near_miss("CREAT SEMANTIC VIEW x (tables := [])");
        assert!(result.is_some());
        let err = result.unwrap();
        assert!(
            err.message.contains("Did you mean")
                && err.message.to_lowercase().contains("create semantic view"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_near_miss_drop_semantc() {
        let result = detect_near_miss("DROP SEMANTC VIEW x");
        assert!(result.is_some());
        let err = result.unwrap();
        assert!(
            err.message.contains("Did you mean")
                && err.message.to_lowercase().contains("drop semantic view"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_near_miss_show_semantic_view() {
        // "SHOW SEMANTIC VIEW" (missing 'S') should suggest "SHOW SEMANTIC VIEWS"
        let result = detect_near_miss("SHOW SEMANTIC VIEW");
        assert!(result.is_some());
        let err = result.unwrap();
        assert!(err.message.contains("Did you mean"), "got: {}", err.message);
    }

    #[test]
    fn test_near_miss_select() {
        // Regular SQL should NOT trigger near-miss
        let result = detect_near_miss("SELECT 1");
        assert!(result.is_none());
    }

    #[test]
    fn test_near_miss_show_tables() {
        // "SHOW TABLES" has too large edit distance from any DDL prefix
        let result = detect_near_miss("SHOW TABLES");
        assert!(result.is_none());
    }

    #[test]
    fn test_near_miss_position_zero() {
        let result = detect_near_miss("CREAT SEMANTIC VIEW x ()");
        assert!(result.is_some());
        let err = result.unwrap();
        assert_eq!(err.position, Some(0));
    }

    // ===================================================================
    // ParseError position tests
    // ===================================================================

    #[test]
    fn test_parse_error_position_paren_body_rejected() {
        // Non-AS-body syntax returns "Expected 'AS' or 'FROM YAML'" error with position
        let query = "CREATE SEMANTIC VIEW x (tables := [])";
        let result = validate_and_rewrite(query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Expected 'AS' or 'FROM YAML'"),
            "got: {}",
            err.message
        );
        assert!(err.position.is_some());
    }

    #[test]
    fn test_parse_error_position_structural() {
        // For missing name, position should point at end of prefix
        let query = "CREATE SEMANTIC VIEW";
        let result = validate_and_rewrite(query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.position.is_some());
    }

    // ===================================================================
    // Phase 25 Plan 03: AS-body dispatch tests
    // ===================================================================

    mod phase25_parse_tests {
        use super::*;

        #[test]
        fn as_body_create_rewrites_to_from_json() {
            let query = "CREATE SEMANTIC VIEW v AS TABLES (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.r AS r) METRICS (t.m AS SUM(1))";
            let result = validate_and_rewrite(query).unwrap().unwrap();
            assert!(
                result.starts_with("SELECT * FROM create_semantic_view_from_json("),
                "Got: {result}"
            );
            assert!(result.contains("'v'"), "Must contain view name: {result}");
        }

        #[test]
        fn as_body_create_or_replace_rewrites_to_from_json() {
            let query = "CREATE OR REPLACE SEMANTIC VIEW v AS TABLES (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.r AS r) METRICS (t.m AS SUM(1))";
            let result = validate_and_rewrite(query).unwrap().unwrap();
            assert!(
                result.starts_with("SELECT * FROM create_or_replace_semantic_view_from_json("),
                "Got: {result}"
            );
        }

        #[test]
        fn as_body_create_if_not_exists_rewrites_to_from_json() {
            let query = "CREATE SEMANTIC VIEW IF NOT EXISTS v AS TABLES (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.r AS r) METRICS (t.m AS SUM(1))";
            let result = validate_and_rewrite(query).unwrap().unwrap();
            assert!(
                result.starts_with("SELECT * FROM create_semantic_view_if_not_exists_from_json("),
                "Got: {result}"
            );
        }

        #[test]
        fn old_paren_body_is_rejected() {
            // CLN-01: non-AS-body syntax rejected with clear error
            let query = "CREATE SEMANTIC VIEW v (tables := [], dimensions := [])";
            let result = validate_and_rewrite(query);
            assert!(result.is_err(), "Paren-body must be rejected: {result:?}");
            let err = result.unwrap_err();
            assert!(
                err.message.contains("Expected 'AS' or 'FROM YAML'"),
                "Expected 'Expected AS or FROM YAML' error, got: {}",
                err.message
            );
        }

        #[test]
        fn drop_still_rewrites_unchanged() {
            let query = "DROP SEMANTIC VIEW v";
            let result = validate_and_rewrite(query).unwrap().unwrap();
            assert_eq!(result, "SELECT * FROM drop_semantic_view('v')");
        }
    }

    // ===================================================================
    // Phase 33: Cardinality inference tests
    // ===================================================================

    mod phase33_inference_tests {
        use super::*;
        use crate::model::{Cardinality, Join, TableRef};

        fn make_table(alias: &str, pk: &[&str], unique: &[&[&str]]) -> TableRef {
            TableRef {
                alias: alias.to_string(),
                table: alias.to_string(),
                pk_columns: pk.iter().map(|s| (*s).to_string()).collect(),
                unique_constraints: unique
                    .iter()
                    .map(|cols| cols.iter().map(|s| (*s).to_string()).collect())
                    .collect(),
                comment: None,
                synonyms: vec![],
            }
        }

        fn make_join(name: &str, from: &str, to: &str, fk: &[&str], ref_cols: &[&str]) -> Join {
            Join {
                name: Some(name.to_string()),
                from_alias: from.to_string(),
                table: to.to_string(),
                fk_columns: fk.iter().map(|s| (*s).to_string()).collect(),
                ref_columns: ref_cols.iter().map(|s| (*s).to_string()).collect(),
                ..Default::default()
            }
        }

        #[test]
        fn resolves_ref_columns_to_target_pk() {
            let tables = vec![
                make_table("orders", &["id"], &[]),
                make_table("customers", &["cust_id"], &[]),
            ];
            let mut rels = vec![make_join("r", "orders", "customers", &["customer_id"], &[])];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert_eq!(rels[0].ref_columns, vec!["cust_id"]);
        }

        #[test]
        fn keeps_explicit_ref_columns() {
            let tables = vec![
                make_table("orders", &["id"], &[]),
                make_table("customers", &["cust_id"], &[&["email"]]),
            ];
            let mut rels = vec![make_join(
                "r",
                "orders",
                "customers",
                &["customer_email"],
                &["email"],
            )];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert_eq!(rels[0].ref_columns, vec!["email"]);
        }

        #[test]
        fn skips_when_target_has_no_pk_and_no_explicit_ref() {
            // When target has no PK, infer_cardinality is tolerant: it skips
            // the join (leaves ref_columns empty) instead of erroring.
            // At bind time, resolve_pk_from_catalog will attempt catalog lookup.
            let tables = vec![
                make_table("orders", &["id"], &[]),
                make_table("events", &[], &[]), // no PK
            ];
            let mut rels = vec![make_join("r", "orders", "events", &["event_id"], &[])];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert!(
                rels[0].ref_columns.is_empty(),
                "ref_columns should remain empty when target has no PK"
            );
        }

        #[test]
        fn infers_one_to_one_from_pk_match() {
            // orders PK is (id), FK is (id) -> OneToOne
            let tables = vec![
                make_table("orders", &["id"], &[]),
                make_table("customers", &["cust_id"], &[]),
            ];
            let mut rels = vec![make_join("r", "orders", "customers", &["id"], &[])];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert_eq!(rels[0].cardinality, Cardinality::OneToOne);
        }

        #[test]
        fn infers_one_to_one_from_unique_match() {
            // orders has UNIQUE(email), FK is (email) -> OneToOne
            let tables = vec![
                make_table("orders", &["id"], &[&["email"]]),
                make_table("customers", &["cust_id"], &[]),
            ];
            let mut rels = vec![make_join("r", "orders", "customers", &["email"], &[])];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert_eq!(rels[0].cardinality, Cardinality::OneToOne);
        }

        #[test]
        fn infers_many_to_one_when_fk_is_bare() {
            // orders PK is (id), FK is (customer_id) -- doesn't match PK or UNIQUE
            let tables = vec![
                make_table("orders", &["id"], &[]),
                make_table("customers", &["cust_id"], &[]),
            ];
            let mut rels = vec![make_join("r", "orders", "customers", &["customer_id"], &[])];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert_eq!(rels[0].cardinality, Cardinality::ManyToOne);
        }

        #[test]
        fn case_insensitive_column_matching() {
            // PK is (ID) uppercase, FK is (id) lowercase -> should still match OneToOne
            let tables = vec![
                make_table("orders", &["ID"], &[]),
                make_table("customers", &["cust_id"], &[]),
            ];
            let mut rels = vec![make_join("r", "orders", "customers", &["id"], &[])];
            infer_cardinality(&tables, &mut rels).unwrap();
            assert_eq!(rels[0].cardinality, Cardinality::OneToOne);
        }

        #[test]
        fn fk_ref_column_count_mismatch_error() {
            let tables = vec![
                make_table("orders", &["id"], &[]),
                make_table("customers", &["a", "b"], &[]),
            ];
            // FK has 1 col, target PK has 2 cols
            let mut rels = vec![make_join("r", "orders", "customers", &["customer_id"], &[])];
            let err = infer_cardinality(&tables, &mut rels).unwrap_err();
            assert!(
                err.message.contains("FK column count"),
                "Expected FK column count error, got: {}",
                err.message
            );
        }

        #[test]
        fn rewrite_produces_json_with_ref_columns_and_cardinality() {
            let query = "CREATE SEMANTIC VIEW v AS \
                         TABLES (o AS orders PRIMARY KEY (id), c AS customers PRIMARY KEY (cust_id)) \
                         RELATIONSHIPS (r AS o(customer_id) REFERENCES c) \
                         DIMENSIONS (o.region AS region) \
                         METRICS (o.revenue AS SUM(amount))";
            let result = validate_and_rewrite(query).unwrap().unwrap();
            // The JSON should contain ref_columns resolved from target PK
            assert!(
                result.contains("ref_columns"),
                "Expected ref_columns in JSON, got: {result}"
            );
            assert!(
                result.contains("cust_id"),
                "Expected target PK 'cust_id' in ref_columns, got: {result}"
            );
        }
    }

    // ===================================================================
    // Phase 34.1.1: SHOW SEMANTIC filter clause tests
    // ===================================================================

    mod phase34_1_1_show_filter_tests {
        use super::*;

        // --- extract_quoted_string tests ---

        #[test]
        fn test_extract_quoted_string_normal() {
            let (s, n) = extract_quoted_string("'hello'").unwrap();
            assert_eq!(s, "hello");
            assert_eq!(n, 7);
        }

        #[test]
        fn test_extract_quoted_string_escaped_quotes() {
            let (s, n) = extract_quoted_string("'O''Brien'").unwrap();
            assert_eq!(s, "O'Brien");
            assert_eq!(n, 10);
        }

        #[test]
        fn test_extract_quoted_string_empty() {
            let (s, n) = extract_quoted_string("''").unwrap();
            assert_eq!(s, "");
            assert_eq!(n, 2);
        }

        #[test]
        fn test_extract_quoted_string_unterminated() {
            let result = extract_quoted_string("'unterminated");
            assert!(result.is_err());
        }

        #[test]
        fn test_extract_quoted_string_no_opening_quote() {
            let result = extract_quoted_string("no_quote");
            assert!(result.is_err());
        }

        // --- build_filter_suffix tests ---

        #[test]
        fn test_build_filter_suffix_like_only() {
            assert_eq!(
                build_filter_suffix(Some("%rev%"), None, None, None, None),
                " WHERE name ILIKE '%rev%'"
            );
        }

        #[test]
        fn test_build_filter_suffix_starts_with_only() {
            assert_eq!(
                build_filter_suffix(None, Some("total"), None, None, None),
                " WHERE name LIKE 'total%'"
            );
        }

        #[test]
        fn test_build_filter_suffix_limit_only() {
            assert_eq!(
                build_filter_suffix(None, None, Some(5), None, None),
                " LIMIT 5"
            );
        }

        #[test]
        fn test_build_filter_suffix_all_three() {
            assert_eq!(
                build_filter_suffix(Some("%x%"), Some("a"), Some(10), None, None),
                " WHERE name ILIKE '%x%' AND name LIKE 'a%' LIMIT 10"
            );
        }

        #[test]
        fn test_build_filter_suffix_none() {
            assert_eq!(build_filter_suffix(None, None, None, None, None), "");
        }

        #[test]
        fn test_build_filter_suffix_reescapes_quotes() {
            assert_eq!(
                build_filter_suffix(Some("O'Brien"), None, None, None, None),
                " WHERE name ILIKE 'O''Brien'"
            );
        }

        #[test]
        fn test_build_filter_suffix_in_schema() {
            assert_eq!(
                build_filter_suffix(None, None, None, Some("main"), None),
                " WHERE schema_name = 'main'"
            );
        }

        #[test]
        fn test_build_filter_suffix_in_database() {
            assert_eq!(
                build_filter_suffix(None, None, None, None, Some("memory")),
                " WHERE database_name = 'memory'"
            );
        }

        #[test]
        fn test_build_filter_suffix_like_and_schema() {
            assert_eq!(
                build_filter_suffix(Some("%x%"), None, None, Some("main"), None),
                " WHERE name ILIKE '%x%' AND schema_name = 'main'"
            );
        }

        // --- rewrite_ddl SHOW with filter clauses ---

        #[test]
        fn test_rewrite_show_dims_like_cross_view() {
            let sql = rewrite_ddl("SHOW SEMANTIC DIMENSIONS LIKE '%rev%'").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM show_semantic_dimensions_all() WHERE name ILIKE '%rev%'"
            );
        }

        #[test]
        fn test_rewrite_show_dims_like_in_starts_with_limit() {
            let sql =
                rewrite_ddl("SHOW SEMANTIC DIMENSIONS LIKE '%c%' IN v STARTS WITH 'cust' LIMIT 2")
                    .unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM show_semantic_dimensions('v') WHERE name ILIKE '%c%' AND name LIKE 'cust%' LIMIT 2"
            );
        }

        #[test]
        fn test_rewrite_show_metrics_starts_with_limit() {
            let sql = rewrite_ddl("SHOW SEMANTIC METRICS STARTS WITH 'total' LIMIT 1").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM show_semantic_metrics_all() WHERE name LIKE 'total%' LIMIT 1"
            );
        }

        #[test]
        fn test_rewrite_show_facts_limit() {
            let sql = rewrite_ddl("SHOW SEMANTIC FACTS LIMIT 10").unwrap();
            assert_eq!(sql, "SELECT * FROM show_semantic_facts_all() LIMIT 10");
        }

        #[test]
        fn test_rewrite_show_dims_for_metric_with_all_clauses() {
            let sql = rewrite_ddl(
                "SHOW SEMANTIC DIMENSIONS LIKE '%x%' IN v FOR METRIC m STARTS WITH 'a' LIMIT 3",
            )
            .unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM show_semantic_dimensions_for_metric('v', 'm') WHERE name ILIKE '%x%' AND name LIKE 'a%' LIMIT 3"
            );
        }

        #[test]
        fn test_rewrite_show_dims_like_after_in_error() {
            let result = rewrite_ddl("SHOW SEMANTIC DIMENSIONS IN v LIKE '%x%'");
            assert!(result.is_err(), "LIKE after IN should error");
        }

        #[test]
        fn test_rewrite_show_metrics_limit_non_numeric() {
            let result = rewrite_ddl("SHOW SEMANTIC METRICS LIMIT abc");
            assert!(result.is_err(), "Non-numeric LIMIT should error");
        }

        #[test]
        fn test_rewrite_show_for_metric_on_metrics_error() {
            let result = rewrite_ddl("SHOW SEMANTIC METRICS IN v FOR METRIC m");
            assert!(result.is_err(), "FOR METRIC on SHOW METRICS should error");
        }

        // --- extract_ddl_name with LIKE ---

        #[test]
        fn test_extract_ddl_name_like_before_in() {
            let result = extract_ddl_name("SHOW SEMANTIC DIMENSIONS LIKE '%x%' IN v").unwrap();
            assert_eq!(result, Some("v".to_string()));
        }

        #[test]
        fn test_extract_ddl_name_like_cross_view() {
            let result = extract_ddl_name("SHOW SEMANTIC DIMENSIONS LIKE '%x%'").unwrap();
            assert_eq!(result, None);
        }

        // --- Case insensitivity ---

        #[test]
        fn test_rewrite_show_case_insensitive() {
            let sql = rewrite_ddl("show semantic dimensions like '%x%' in v").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM show_semantic_dimensions('v') WHERE name ILIKE '%x%'"
            );
        }

        // --- SHOW SEMANTIC VIEWS with filter clauses ---

        #[test]
        fn test_rewrite_show_views_like() {
            let sql = rewrite_ddl("SHOW SEMANTIC VIEWS LIKE '%prod%'").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_semantic_views() WHERE name ILIKE '%prod%'"
            );
        }

        #[test]
        fn test_rewrite_show_views_starts_with_limit() {
            let sql = rewrite_ddl("SHOW SEMANTIC VIEWS STARTS WITH 'sales' LIMIT 5").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_semantic_views() WHERE name LIKE 'sales%' LIMIT 5"
            );
        }

        #[test]
        fn test_rewrite_show_views_all_clauses() {
            let sql =
                rewrite_ddl("SHOW SEMANTIC VIEWS LIKE '%x%' STARTS WITH 'a' LIMIT 3").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_semantic_views() WHERE name ILIKE '%x%' AND name LIKE 'a%' LIMIT 3"
            );
        }

        #[test]
        fn test_rewrite_show_views_in_requires_schema_or_database() {
            let result = rewrite_ddl("SHOW SEMANTIC VIEWS IN some_view");
            assert!(
                result.is_err(),
                "IN without SCHEMA/DATABASE should be rejected for SHOW SEMANTIC VIEWS"
            );
            let err = result.unwrap_err();
            assert!(
                err.contains("SHOW SEMANTIC VIEWS requires IN SCHEMA"),
                "got: {err}"
            );
        }

        #[test]
        fn test_rewrite_show_views_in_schema() {
            let sql = rewrite_ddl("SHOW SEMANTIC VIEWS IN SCHEMA main").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_semantic_views() WHERE schema_name = 'main'"
            );
        }

        #[test]
        fn test_rewrite_show_views_in_database() {
            let sql = rewrite_ddl("SHOW SEMANTIC VIEWS IN DATABASE memory").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_semantic_views() WHERE database_name = 'memory'"
            );
        }

        #[test]
        fn test_rewrite_show_terse() {
            let sql = rewrite_ddl("SHOW TERSE SEMANTIC VIEWS").unwrap();
            assert_eq!(sql, "SELECT * FROM list_terse_semantic_views()");
        }

        #[test]
        fn test_rewrite_show_terse_like() {
            let sql = rewrite_ddl("SHOW TERSE SEMANTIC VIEWS LIKE '%prod%'").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_terse_semantic_views() WHERE name ILIKE '%prod%'"
            );
        }

        #[test]
        fn test_rewrite_show_terse_in_schema() {
            let sql = rewrite_ddl("SHOW TERSE SEMANTIC VIEWS IN SCHEMA main").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_terse_semantic_views() WHERE schema_name = 'main'"
            );
        }

        #[test]
        fn test_rewrite_show_views_in_schema_like() {
            let sql = rewrite_ddl("SHOW SEMANTIC VIEWS LIKE '%x%' IN SCHEMA main").unwrap();
            assert_eq!(
                sql,
                "SELECT * FROM list_semantic_views() WHERE name ILIKE '%x%' AND schema_name = 'main'"
            );
        }

        #[test]
        fn test_rewrite_show_columns_in_semantic_view() {
            let sql = rewrite_ddl("SHOW COLUMNS IN SEMANTIC VIEW sales").unwrap();
            assert_eq!(sql, "SELECT * FROM show_columns_in_semantic_view('sales')");
        }

        #[test]
        fn test_rewrite_show_views_for_metric_error() {
            let result = rewrite_ddl("SHOW SEMANTIC VIEWS FOR METRIC m");
            assert!(
                result.is_err(),
                "FOR METRIC should be rejected for SHOW SEMANTIC VIEWS"
            );
            let err = result.unwrap_err();
            assert!(err.contains("FOR METRIC is only valid"), "got: {err}");
        }

        #[test]
        fn test_rewrite_show_views_no_clauses_regression() {
            let sql = rewrite_ddl("SHOW SEMANTIC VIEWS").unwrap();
            assert_eq!(sql, "SELECT * FROM list_semantic_views()");
        }
    }

    // -----------------------------------------------------------------------
    // Phase 43: View-level COMMENT tests
    // -----------------------------------------------------------------------

    mod phase43_view_comment_tests {
        use crate::parse::validate_and_rewrite;

        #[test]
        fn test_view_comment_parsed() {
            let result = validate_and_rewrite(
                "CREATE SEMANTIC VIEW my_view COMMENT = 'My view' AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))"
            ).unwrap().unwrap();
            // The JSON should contain the comment
            assert!(
                result.contains("My view"),
                "Generated SQL should contain the comment value: {result}"
            );
        }

        #[test]
        fn test_view_without_comment() {
            let result = validate_and_rewrite(
                "CREATE SEMANTIC VIEW my_view AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))"
            ).unwrap().unwrap();
            assert!(
                result.contains("create_semantic_view_from_json"),
                "Should use correct function: {result}"
            );
        }

        #[test]
        fn test_view_comment_escaped_quotes() {
            let result = validate_and_rewrite(
                "CREATE SEMANTIC VIEW my_view COMMENT = 'It''s great' AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))"
            ).unwrap().unwrap();
            assert!(
                result.contains("It''s great") || result.contains("It's great"),
                "Generated SQL should contain the escaped comment: {result}"
            );
        }

        #[test]
        fn test_view_comment_with_create_or_replace() {
            let result = validate_and_rewrite(
                "CREATE OR REPLACE SEMANTIC VIEW my_view COMMENT = 'Updated' AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))"
            ).unwrap().unwrap();
            assert!(
                result.contains("Updated"),
                "Should contain comment: {result}"
            );
            assert!(
                result.contains("create_or_replace"),
                "Should use OR REPLACE function: {result}"
            );
        }
    }

    // ===================================================================
    // ALTER SET/UNSET COMMENT tests (Phase 45)
    // ===================================================================

    #[test]
    fn test_detect_ddl_kind_alter_set_comment() {
        assert_eq!(
            detect_ddl_kind("ALTER SEMANTIC VIEW v SET COMMENT = 'test'"),
            Some(DdlKind::Alter)
        );
    }

    #[test]
    fn test_detect_ddl_kind_alter_if_exists_set_comment() {
        assert_eq!(
            detect_ddl_kind("ALTER SEMANTIC VIEW IF EXISTS v SET COMMENT = 'test'"),
            Some(DdlKind::AlterIfExists)
        );
    }

    #[test]
    fn test_detect_ddl_kind_alter_unset_comment() {
        assert_eq!(
            detect_ddl_kind("ALTER SEMANTIC VIEW v UNSET COMMENT"),
            Some(DdlKind::Alter)
        );
    }

    #[test]
    fn test_detect_ddl_kind_alter_if_exists_unset_comment() {
        assert_eq!(
            detect_ddl_kind("ALTER SEMANTIC VIEW IF EXISTS v UNSET COMMENT"),
            Some(DdlKind::AlterIfExists)
        );
    }

    #[test]
    fn test_detect_ddl_kind_alter_rename_backwards_compat() {
        assert_eq!(
            detect_ddl_kind("ALTER SEMANTIC VIEW v RENAME TO w"),
            Some(DdlKind::Alter)
        );
    }

    #[test]
    fn test_validate_rewrite_alter_set_comment() {
        let result = validate_and_rewrite("ALTER SEMANTIC VIEW v SET COMMENT = 'hello'")
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            "SELECT * FROM alter_semantic_view_set_comment('v', 'hello')"
        );
    }

    #[test]
    fn test_validate_rewrite_alter_unset_comment() {
        let result = validate_and_rewrite("ALTER SEMANTIC VIEW v UNSET COMMENT")
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            "SELECT * FROM alter_semantic_view_unset_comment('v')"
        );
    }

    #[test]
    fn test_validate_rewrite_alter_if_exists_set_comment() {
        let result = validate_and_rewrite("ALTER SEMANTIC VIEW IF EXISTS v SET COMMENT = 'hello'")
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            "SELECT * FROM alter_semantic_view_set_comment_if_exists('v', 'hello')"
        );
    }

    #[test]
    fn test_validate_rewrite_alter_if_exists_unset_comment() {
        let result = validate_and_rewrite("ALTER SEMANTIC VIEW IF EXISTS v UNSET COMMENT")
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            "SELECT * FROM alter_semantic_view_unset_comment_if_exists('v')"
        );
    }

    #[test]
    fn test_validate_rewrite_alter_rename_unchanged() {
        let result = validate_and_rewrite("ALTER SEMANTIC VIEW v RENAME TO w")
            .unwrap()
            .unwrap();
        assert_eq!(result, "SELECT * FROM alter_semantic_view_rename('v', 'w')");
    }

    #[test]
    fn test_validate_rewrite_alter_unsupported_operation() {
        let err = validate_and_rewrite("ALTER SEMANTIC VIEW v TRUNCATE").unwrap_err();
        assert!(
            err.message
                .contains("RENAME TO, SET COMMENT, UNSET COMMENT"),
            "Error should list supported ops, got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_rewrite_alter_set_comment_escaped_quotes() {
        let result = validate_and_rewrite("ALTER SEMANTIC VIEW v SET COMMENT = 'it''s a test'")
            .unwrap()
            .unwrap();
        assert_eq!(
            result,
            "SELECT * FROM alter_semantic_view_set_comment('v', 'it''s a test')"
        );
    }

    #[test]
    fn test_validate_rewrite_alter_missing_operation() {
        let err = validate_and_rewrite("ALTER SEMANTIC VIEW v").unwrap_err();
        assert!(
            err.message
                .contains("RENAME TO, SET COMMENT, UNSET COMMENT"),
            "Error should list supported ops, got: {}",
            err.message
        );
    }

    // ===================================================================
    // Phase 52: Dollar-quote extraction tests
    // ===================================================================

    #[test]
    fn test_extract_dollar_quoted_untagged() {
        let (content, consumed) = extract_dollar_quoted("$$hello world$$").unwrap();
        assert_eq!(content, "hello world");
        assert_eq!(consumed, 15);
    }

    #[test]
    fn test_extract_dollar_quoted_tagged() {
        let (content, consumed) = extract_dollar_quoted("$yaml$my content$yaml$").unwrap();
        assert_eq!(content, "my content");
        assert_eq!(consumed, 22);
    }

    #[test]
    fn test_extract_dollar_quoted_empty_content() {
        let (content, consumed) = extract_dollar_quoted("$$$$").unwrap();
        assert_eq!(content, "");
        assert_eq!(consumed, 4);
    }

    #[test]
    fn test_extract_dollar_quoted_no_leading_dollar() {
        let err = extract_dollar_quoted("not a dollar").unwrap_err();
        assert!(err.message.contains("Expected '$'"));
    }

    #[test]
    fn test_extract_dollar_quoted_unterminated_opening() {
        let err = extract_dollar_quoted("$no_close").unwrap_err();
        assert!(err.message.contains("Unterminated dollar-quote opening"));
    }

    #[test]
    fn test_extract_dollar_quoted_unterminated_body() {
        let err = extract_dollar_quoted("$$no closing").unwrap_err();
        assert!(err.message.contains("Unterminated dollar-quoted string"));
    }

    #[test]
    fn test_extract_dollar_quoted_inner_dollar() {
        // First closing $$ wins — content is "has inner "
        let (content, consumed) = extract_dollar_quoted("$$has inner $$ text$$").unwrap();
        assert_eq!(content, "has inner ");
        assert_eq!(consumed, 14);
    }

    #[test]
    fn test_extract_dollar_quoted_multiline() {
        let input = "$$\ntables:\n  - alias: o\n    table: orders\n$$";
        let (content, _) = extract_dollar_quoted(input).unwrap();
        assert!(content.contains("tables:"));
        assert!(content.contains("alias: o"));
    }

    // ===================================================================
    // Phase 52: YAML DDL rewrite tests
    // ===================================================================

    #[test]
    fn test_yaml_rewrite_basic_create() {
        let yaml_text = r#"$$
base_table: orders
tables:
  - alias: o
    table: orders
    pk_columns:
      - id
dimensions:
  - name: region
    expr: o.region
    source_table: o
metrics:
  - name: total_amount
    expr: SUM(o.amount)
    source_table: o
$$"#;
        let result = rewrite_ddl_yaml_body(DdlKind::Create, "test_view", yaml_text, None).unwrap();
        let sql = result.unwrap();
        assert!(sql.starts_with("SELECT * FROM create_semantic_view_from_json('test_view',"));
    }

    #[test]
    fn test_yaml_rewrite_create_or_replace() {
        let yaml_text = "$$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = rewrite_ddl_yaml_body(DdlKind::CreateOrReplace, "v", yaml_text, None).unwrap();
        let sql = result.unwrap();
        assert!(sql.contains("create_or_replace_semantic_view_from_json"));
    }

    #[test]
    fn test_yaml_rewrite_create_if_not_exists() {
        let yaml_text = "$$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result =
            rewrite_ddl_yaml_body(DdlKind::CreateIfNotExists, "v", yaml_text, None).unwrap();
        let sql = result.unwrap();
        assert!(sql.contains("create_semantic_view_if_not_exists_from_json"));
    }

    #[test]
    fn test_yaml_rewrite_trailing_content_rejected() {
        let yaml_text =
            "$$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$ extra stuff";
        let err = rewrite_ddl_yaml_body(DdlKind::Create, "v", yaml_text, None).unwrap_err();
        assert!(err
            .message
            .contains("Unexpected content after closing dollar-quote"));
    }

    #[test]
    fn test_yaml_rewrite_invalid_yaml() {
        let yaml_text = "$$\n: : : not valid yaml [[[$$";
        let err = rewrite_ddl_yaml_body(DdlKind::Create, "bad_view", yaml_text, None).unwrap_err();
        assert!(err.message.contains("bad_view"));
    }

    #[test]
    fn test_yaml_rewrite_comment_override() {
        let yaml_text =
            "$$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\ncomment: yaml comment\n$$";
        let result = rewrite_ddl_yaml_body(
            DdlKind::Create,
            "v",
            yaml_text,
            Some("ddl comment".to_string()),
        )
        .unwrap();
        let sql = result.unwrap();
        // DDL comment overrides YAML comment
        assert!(sql.contains("ddl comment"));
    }

    #[test]
    fn test_yaml_rewrite_base_table_populated() {
        let yaml_text = r#"$$
base_table: ""
tables:
  - alias: o
    table: orders
    pk_columns: []
dimensions: []
metrics: []
$$"#;
        let result = rewrite_ddl_yaml_body(DdlKind::Create, "v", yaml_text, None).unwrap();
        let sql = result.unwrap();
        // base_table should be populated from first table entry
        assert!(sql.contains("orders"));
    }

    #[test]
    fn test_yaml_rewrite_tagged_dollar_quote() {
        let yaml_text = "$yaml$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$yaml$";
        let result = rewrite_ddl_yaml_body(DdlKind::Create, "v", yaml_text, None).unwrap();
        assert!(result.is_some());
    }

    // ===================================================================
    // Phase 52: FROM YAML detection in validate_create_body
    // ===================================================================

    #[test]
    fn test_from_yaml_detection_via_rewrite_ddl() {
        let query = r#"CREATE SEMANTIC VIEW yaml_test FROM YAML $$
base_table: t
tables: []
dimensions: []
metrics: []
$$"#;
        let result = validate_and_rewrite(query).unwrap();
        assert!(result.is_some());
        let sql = result.unwrap();
        assert!(sql.contains("create_semantic_view_from_json('yaml_test'"));
    }

    #[test]
    fn test_from_yaml_case_insensitive() {
        let query = "CREATE SEMANTIC VIEW v from yaml $$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = validate_and_rewrite(query).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_from_yaml_mixed_case() {
        let query = "CREATE SEMANTIC VIEW v From Yaml $$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = validate_and_rewrite(query).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_error_message_mentions_from_yaml() {
        let query = "CREATE SEMANTIC VIEW v SOMETHING_ELSE";
        let err = validate_and_rewrite(query).unwrap_err();
        assert!(
            err.message.contains("FROM YAML"),
            "Error should mention FROM YAML: {}",
            err.message
        );
    }

    #[test]
    fn test_create_or_replace_from_yaml() {
        let query = "CREATE OR REPLACE SEMANTIC VIEW v FROM YAML $$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = validate_and_rewrite(query).unwrap();
        let sql = result.unwrap();
        assert!(sql.contains("create_or_replace_semantic_view_from_json"));
    }

    #[test]
    fn test_create_if_not_exists_from_yaml() {
        let query = "CREATE SEMANTIC VIEW IF NOT EXISTS v FROM YAML $$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = validate_and_rewrite(query).unwrap();
        let sql = result.unwrap();
        assert!(sql.contains("create_semantic_view_if_not_exists_from_json"));
    }

    #[test]
    fn test_comment_with_from_yaml() {
        let query = "CREATE SEMANTIC VIEW v COMMENT = 'my comment' FROM YAML $$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = validate_and_rewrite(query).unwrap();
        let sql = result.unwrap();
        assert!(sql.contains("my comment"));
    }
}
