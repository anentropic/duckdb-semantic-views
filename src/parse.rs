// Parse detection and rewriting for semantic view DDL statements.
//
// This module provides two layers:
// 1. Pure detection/rewrite functions (`detect_semantic_view_ddl`,
//    `extract_ddl_name`, `validate_and_rewrite`) testable under `cargo test`
//    without the extension feature.
// 2. FFI entry points (`sv_parser_override_rust`, `sv_free_buffer`)
//    feature-gated on `extension`, with `catch_unwind` for panic safety.

use std::collections::HashSet;

use crate::body_parser::parse_keyword_body;
use crate::errors::ParseError;
use crate::model::{Cardinality, Join, TableRef};

// ---------------------------------------------------------------------------
// Catalog handle for parser_override DDL rewrites (v0.8.0; Phase 62 direct-attach).
// ---------------------------------------------------------------------------
//
// CREATE/DROP/ALTER need to know whether a view exists (and for SET/UNSET
// COMMENT, what its current JSON definition is) before emitting native SQL
// with friendly errors. The parser_override callback runs in a context
// without access to the caller's catalog, so we stash a dedicated
// `CatalogReader` (populated at extension load) and hand it to the C++ shim
// as an opaque `Box<OverrideContext>`. The shim attaches the boxed pointer
// to its `SemanticViewsParserInfo` (the `parser_info` value DuckDB passes
// back into the override callback for every parse). Lifetime is tied to the
// `DBConfig`, so destruction happens on DB unload.
//
// Phase 62 (Wave 1) replaced the v0.8.1 16-entry `db_token` LRU with this
// direct-attachment design — see TECH-DEBT item 20. The LRU's silent-
// eviction error class is gone because there is no global map any more;
// each `parser_info` carries its own `OverrideContext`.
//
// The reader sees committed state only — by design. Same-transaction
// CREATE-then-ALTER is the documented v0.8.0 limitation.

/// Catalog handle plus an `is_file_backed` flag that gates DDL-time
/// type inference. `LIMIT 0` probes used for type inference depend on
/// user tables having been committed; for in-memory DBs we follow the
/// v0.7.1 behaviour and skip inference entirely.
///
/// Owned by the C++ shim as `Box<OverrideContext>` (one per
/// `SemanticViewsParserInfo`, i.e. one per extension-LOAD-per-DB).
#[cfg(feature = "extension")]
pub struct OverrideContext {
    pub catalog: crate::catalog::CatalogReader,
    pub is_file_backed: bool,
}

#[cfg(feature = "extension")]
impl Drop for OverrideContext {
    fn drop(&mut self) {
        // Phase 62 Q2 — INTENTIONAL LEAK of self.catalog.conn (the duckdb_connection).
        //
        // ~SemanticViewsParserInfo (and therefore Drop for OverrideContext) fires
        // during ~DBConfig, AFTER ~DatabaseInstance has already reset
        // connection_manager (duckdb.cpp:276819). Calling duckdb_disconnect here
        // would invoke ~Connection() → ConnectionManager::RemoveConnection() on
        // the destroyed manager — use-after-free.
        //
        // The leak is bounded at ONE duckdb_connection per DB ever opened in this
        // process (a few KB each). This matches v0.8.0 commit 680a967 which shipped
        // successfully with the same leak. The Rust-side Box<OverrideContext>
        // allocation itself IS reclaimed (this Drop runs and the Box dealloc fires).
        //
        // See: .planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md §Q2.
        // Resolves TECH-DEBT item 20 (silent LRU eviction class) by removing the LRU.
    }
}

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
    ShowMaterializations,
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

/// Return the byte offset of the first character that is neither ASCII whitespace
/// nor part of a SQL comment. Recognises:
///   - `-- ... \n` line comments (terminated by newline or end-of-input)
///   - `/* ... */` block comments (NOT nested -- matches PostgreSQL/DuckDB behaviour)
///
/// Designed for prefix-matching: never errors. An unterminated `/* ...` consumes to
/// end of input (so the keyword match below it will simply fail and fall through
/// to `PARSE_NOT_OURS`, matching today's behaviour for malformed queries).
///
/// Returns the byte offset where real SQL begins, in the *original* slice. Callers
/// substitute this for the `query.len() - query.trim_start().len()` whitespace
/// offset so that v0.5.1 error-caret positions continue to reference the original
/// query string after a leading comment is consumed.
///
/// Quick task 260430-vdz: fixes parser hook compatibility with dbt-duckdb (and
/// any other tool that prepends a query annotation comment).
fn skip_leading_whitespace_and_comments(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut i = 0;
    loop {
        // ASCII whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        // Line comment: -- ... \n
        if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue; // re-enter loop to consume more whitespace/comments
        }
        // Block comment: /* ... */ (non-nesting, Postgres semantics)
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2; // consume "*/"
            } else {
                i = bytes.len(); // unterminated -- consume to end
            }
            continue;
        }
        break;
    }
    i
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
    // SHOW SEMANTIC MATERIALIZATIONS (3 keywords) -- before SHOW SEMANTIC VIEWS
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"semantic", b"materializations"]) {
        return Some((DdlKind::ShowMaterializations, n));
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
    let lead = skip_leading_whitespace_and_comments(query);
    let trimmed = query[lead..].trim_end().trim_end_matches(';').trim();
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
        DdlKind::ShowMaterializations => "show_semantic_materializations",
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
    let lead = skip_leading_whitespace_and_comments(query);
    let trimmed = query[lead..].trim_end();
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
        | DdlKind::ShowFacts
        | DdlKind::ShowMaterializations => {
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
                    DdlKind::ShowMaterializations => "show_semantic_materializations_all",
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
    let lead = skip_leading_whitespace_and_comments(query);
    let trimmed = query[lead..].trim_end();
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
        DdlKind::ShowDimensions
        | DdlKind::ShowMetrics
        | DdlKind::ShowFacts
        | DdlKind::ShowMaterializations => {
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
    "show semantic materializations",
];

/// Detect near-miss DDL prefixes using fuzzy matching.
///
/// If the beginning of the query is close (Levenshtein distance <= 3) to one
/// of the 7 known DDL prefixes, returns a `ParseError` suggesting the correct
/// prefix. Returns `None` if no near-miss is found.
#[must_use]
pub fn detect_near_miss(query: &str) -> Option<ParseError> {
    let lead = skip_leading_whitespace_and_comments(query);
    let trimmed = query[lead..].trim_end();
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
        let trim_offset = lead;
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
    let lead = skip_leading_whitespace_and_comments(query);
    let trimmed = query[lead..].trim_end();
    let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
    let trim_offset = lead;

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
        // SHOW SEMANTIC DIMENSIONS/METRICS/FACTS/MATERIALIZATIONS: optional IN view_name
        DdlKind::ShowDimensions
        | DdlKind::ShowMetrics
        | DdlKind::ShowFacts
        | DdlKind::ShowMaterializations => rewrite_ddl(query).map(Some).map_err(|e| ParseError {
            message: e,
            position: Some(trim_offset + plen),
        }),
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

    // --- FROM YAML body path (Phase 52 + Phase 53) ---
    let is_yaml_body = after_name_trimmed
        .get(..9)
        .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
        && (after_name_trimmed.len() == 9
            || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());
    if is_yaml_body {
        let yaml_text = after_name_trimmed[9..].trim_start();

        // Phase 53: FROM YAML FILE '/path' sub-branch
        let is_file = yaml_text
            .get(..4)
            .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
            && (yaml_text.len() == 4 || yaml_text.as_bytes()[4].is_ascii_whitespace());
        if is_file {
            let file_text = yaml_text[4..].trim_start();
            return rewrite_ddl_yaml_file_body(kind, name, file_text, view_comment);
        }

        // Phase 52: FROM YAML $$...$$ inline sub-branch (existing)
        return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment);
    }
    // --- End FROM YAML body path ---

    // Non-AS/FROM-YAML syntax rejected -- AS keyword or FROM YAML required after view name.
    let pos_in_trimmed = plen + (trimmed_no_semi.len() - plen - after_prefix.len()) + name_end;
    Err(ParseError {
        message: "Expected 'AS' or 'FROM YAML' after view name. Use: CREATE SEMANTIC VIEW name \
                  AS TABLES (...) DIMENSIONS (...) METRICS (...) or: CREATE SEMANTIC VIEW name \
                  FROM YAML $$ ... $$ or: CREATE SEMANTIC VIEW name FROM YAML FILE \
                  '/path/to/file.yaml'"
            .to_string(),
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
    let def = crate::model::SemanticViewDefinition {
        tables: keyword_body.tables,
        dimensions: keyword_body.dimensions,
        metrics: keyword_body.metrics,
        joins: keyword_body.relationships,
        facts: keyword_body.facts,
        materializations: keyword_body.materializations,
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
// Phase 53: Single-quoted file path extraction and YAML FILE sentinel
// ---------------------------------------------------------------------------

/// Extract a single-quoted string literal from the input.
///
/// Returns `(unescaped_content, bytes_consumed)` on success.
/// Handles SQL-standard escaped single quotes (`''` -> `'`).
fn extract_single_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('\'') {
        return Err(ParseError {
            message: "Expected single-quoted file path after FILE keyword. \
                      Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            position: None,
        });
    }
    let mut result = String::new();
    let mut i = 1; // skip opening quote
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                result.push('\'');
                i += 2;
            } else {
                return Ok((result, i + 1));
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Err(ParseError {
        message: "Unterminated file path string (missing closing single quote)".to_string(),
        position: None,
    })
}

/// Generate a sentinel string for C++ shim to intercept and read the file.
///
/// Sentinel format: `__SV_YAML_FILE__<path>\x01<kind>\x01<name>\x01<comment>`
/// Uses `\x01` (SOH) as field separator instead of `\x00` (NUL) because the
/// sentinel is passed through C string APIs that treat NUL as a terminator.
/// `rewrite_to_native_sql` strips the prefix and dispatches to
/// `rewrite_yaml_file_create`, which calls `read_text()` on the catalog
/// connection (Rust-side) and then routes through `emit_native_create_sql`.
fn rewrite_ddl_yaml_file_body(
    kind: DdlKind,
    name: &str,
    file_text: &str,
    view_comment: Option<String>,
) -> Result<Option<String>, ParseError> {
    let (file_path, consumed) = extract_single_quoted(file_text)?;

    let trailing = file_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after file path: '{trailing}'"),
            position: None,
        });
    }

    if file_path.is_empty() {
        return Err(ParseError {
            message: "File path cannot be empty. \
                      Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            position: None,
        });
    }

    let kind_num = match kind {
        DdlKind::Create => 0,
        DdlKind::CreateOrReplace => 1,
        DdlKind::CreateIfNotExists => 2,
        _ => unreachable!("rewrite_ddl_yaml_file_body only called for CREATE forms"),
    };
    let comment = view_comment.unwrap_or_default();
    Ok(Some(format!(
        "__SV_YAML_FILE__{file_path}\x01{kind_num}\x01{name}\x01{comment}"
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

/// Write an error message into a fixed-size, caller-owned byte buffer.
/// Null-terminated, truncated to `len - 1` bytes.
///
/// Use only for short, bounded strings (error messages). For unboundedly
/// large outputs (rewritten SQL) use `leak_string_to_c_buffer` +
/// `sv_free_buffer` instead — silently truncating SQL produced confusing
/// downstream parser errors (see v0.8.0 buffer-truncation fix).
///
/// # Safety
///
/// `buf` must point to a writable buffer of at least `len` bytes.
///
/// Phase 62 Plan 03 made this the live error-emit path for
/// `sv_parse_function_rust` (rc=1 / rc=3). It used to be dead under the
/// v0.8.1 `FALLBACK_OVERRIDE` synthesised-`SELECT error` workaround, which
/// has been deleted now that `parse_function` re-renders the caret.
#[cfg(any(feature = "extension", test))]
unsafe fn write_error_to_buffer(buf: *mut u8, len: usize, s: &str) {
    if buf.is_null() || len == 0 {
        return;
    }
    let max_copy = len - 1; // reserve space for null terminator
    let copy_len = s.len().min(max_copy);
    std::ptr::copy_nonoverlapping(s.as_ptr(), buf, copy_len);
    *buf.add(copy_len) = 0; // null terminate
}

/// Convert an owned `String` into a heap-allocated byte buffer that the C++
/// caller takes ownership of. Caller must release via `sv_free_buffer`.
///
/// Returns `(ptr, len)`. The buffer is **not** NUL-terminated — the C++
/// side reads exactly `len` bytes. This avoids any silent truncation cap
/// regardless of how large the rewritten SQL becomes.
///
/// Uses `Box<[u8]>` rather than a leaked `Vec` because `Vec::shrink_to_fit`
/// is only a hint — the allocator may keep excess capacity, which would
/// make the matching `Vec::from_raw_parts(ptr, len, len)` in `reclaim_c_buffer`
/// undefined behaviour in release builds. `into_boxed_slice` actually
/// guarantees `len == capacity`.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
fn leak_string_to_c_buffer(s: String) -> (*mut u8, usize) {
    let boxed: Box<[u8]> = s.into_bytes().into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed).cast::<u8>();
    (ptr, len)
}

/// Reclaim a buffer produced by `leak_string_to_c_buffer`.
///
/// # Safety
///
/// `ptr`/`len` must be the exact pair returned by an earlier call to
/// `leak_string_to_c_buffer` (or its FFI exports), and may only be released
/// once.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
unsafe fn reclaim_c_buffer(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(ptr, len);
    drop(Box::from_raw(slice));
}

/// FFI export: free a heap buffer produced by an earlier
/// `sv_parser_override_rust` success return.
///
/// Safe to call with a null pointer (no-op).
///
/// # Safety
///
/// `ptr`/`len` must be the exact pair the Rust side returned via its
/// `sql_out_ptr` / `sql_out_len` out-parameters. Calling with any other
/// pair (or twice on the same pair) is undefined behaviour.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_free_buffer(ptr: *mut u8, len: usize) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reclaim_c_buffer(ptr, len);
    }));
}

/// Internal helper: publish an owned `String` to the FFI out-parameters.
/// On null out-pointers the buffer is dropped instead of leaked, so a
/// misbehaving C++ caller cannot induce a memory leak through us.
///
/// # Safety
///
/// Either both `sql_out_ptr` and `sql_out_len` must point to writable
/// `*mut u8` / `usize` slots, or both must be null. Mixed null is treated
/// as "drop and skip writing."
#[cfg(feature = "extension")]
unsafe fn publish_owned_sql(sql: String, sql_out_ptr: *mut *mut u8, sql_out_len: *mut usize) {
    if sql_out_ptr.is_null() || sql_out_len.is_null() {
        return; // dropping `sql` here releases the heap allocation
    }
    let (ptr, len) = leak_string_to_c_buffer(sql);
    *sql_out_ptr = ptr;
    *sql_out_len = len;
}

// ---------------------------------------------------------------------------
// v0.8.x: native-SQL rewrite for parser_override (transactional DDL)
// ---------------------------------------------------------------------------
//
// `parser_override` is the sole semantic-view DDL entry point. Every recognised
// statement is rewritten here and re-executed on the caller's connection by
// DuckDB — the legacy parse_function / sv_ddl_internal fallback was retired
// in v0.8.1.
//
// Rewriting is dispatched by shape:
//
//   * CREATE / CREATE OR REPLACE / CREATE IF NOT EXISTS
//     CREATE ... FROM YAML FILE '/path/...'
//     DROP / DROP IF EXISTS
//     ALTER ... RENAME TO / SET COMMENT / UNSET COMMENT
//       → emitted as native INSERT / DELETE / UPDATE against
//         `semantic_layer._definitions`, so writes participate in the
//         caller's transaction (the v0.8.0 ADBC autocommit=false fix).
//
//   * DESCRIBE / SHOW SEMANTIC * / GET_DDL / READ_YAML_FROM_SEMANTIC_VIEW
//       → passed through as `SELECT * FROM <existing_read_side_table_function>(...)`
//         (or the same SQL with WHERE/LIMIT clauses appended). DuckDB re-parses
//         and executes on the caller's connection. The read-side table functions
//         themselves still query via `catalog_conn` (committed state); making
//         DESCRIBE/SHOW transactional w.r.t. the caller's snapshot would require
//         exposing the executing connection from BindInfo — a separate refactor.
//
//   * Anything else (`validate_and_rewrite` returns None)
//       → `Ok(None)`; the C++ shim returns DISPLAY_ORIGINAL_ERROR and DuckDB's
//         default parser handles it.
//
// CREATE/DROP/ALTER need a `CatalogReader` for existence checks; CREATE also
// runs catalog-side queries during enrichment (PK lookup, LIMIT 0 type
// inference, fact typing). Phase 62 attaches the `OverrideContext` directly
// to the C++ `SemanticViewsParserInfo` via an opaque `Box`, so the rewriters
// take `&OverrideContext` here instead of looking up by token. Under
// `cargo test` (no extension feature) these code paths are excluded entirely
// (this entry point itself is feature-gated; its sole caller —
// `sv_parser_override_rust` — is `extension`-only).
#[cfg(feature = "extension")]
pub fn rewrite_to_native_sql(
    ctx: &OverrideContext,
    query: &str,
) -> Result<Option<String>, ParseError> {
    let Some(tf_sql) = validate_and_rewrite(query)? else {
        return Ok(None);
    };

    // YAML FILE produces a sentinel string starting with `__SV_YAML_FILE__`
    // (path + kind + name + comment). Read the file and route through the
    // shared CREATE emission path so the INSERT runs on the caller's
    // connection.
    if let Some(payload) = tf_sql.strip_prefix("__SV_YAML_FILE__") {
        return rewrite_yaml_file_create(ctx, payload);
    }

    // Read-side DDL (DESCRIBE / SHOW with WHERE/LIMIT clauses) emits SQL that
    // doesn't match the `SELECT * FROM fn('arg', ...)` shape. Pass through
    // unchanged; DuckDB executes the read-side table function on the caller's
    // connection.
    let Some(call) = parse_table_function_call(&tf_sql) else {
        return Ok(Some(tf_sql));
    };

    // The args returned by parse_table_function_call retain the SQL escaping
    // produced by validate_and_rewrite (single quotes doubled). They can be
    // re-embedded as single-quoted strings without further processing.
    let args = &call.args;
    match call.fn_name.as_str() {
        // CREATE forms: enrich the JSON (metadata + type inference + graph
        // validation) against the catalog connection, then emit native INSERT
        // on the caller's connection. See `rewrite_create` (extension-only).
        "create_semantic_view_from_json"
        | "create_or_replace_semantic_view_from_json"
        | "create_semantic_view_if_not_exists_from_json" => {
            rewrite_create(ctx, call.fn_name.as_str(), args)
        }
        // DROP / ALTER: existence check + JSON read-modify-write + native
        // DELETE/UPDATE. See `rewrite_drop_or_alter` (extension-only).
        "drop_semantic_view"
        | "drop_semantic_view_if_exists"
        | "alter_semantic_view_rename"
        | "alter_semantic_view_rename_if_exists"
        | "alter_semantic_view_set_comment"
        | "alter_semantic_view_set_comment_if_exists"
        | "alter_semantic_view_unset_comment"
        | "alter_semantic_view_unset_comment_if_exists" => {
            rewrite_drop_or_alter(ctx, call.fn_name.as_str(), args)
        }
        // Read-side table functions (describe_semantic_view, show_*, list_*,
        // get_ddl, read_yaml_from_semantic_view): pass through.
        _ => Ok(Some(tf_sql)),
    }
}

/// DROP / ALTER native-SQL emission. Phase 62 takes `&OverrideContext`
/// directly — the LRU `db_token` indirection is gone (TECH-DEBT 20).
#[cfg(feature = "extension")]
fn rewrite_drop_or_alter(
    ctx: &OverrideContext,
    fn_name: &str,
    args: &[String],
) -> Result<Option<String>, ParseError> {
    let catalog = ctx.catalog;

    match (fn_name, args.len()) {
        ("drop_semantic_view", 1) => rewrite_drop(&catalog, &args[0], false),
        ("drop_semantic_view_if_exists", 1) => rewrite_drop(&catalog, &args[0], true),
        ("alter_semantic_view_rename", 2) => {
            rewrite_alter_rename(&catalog, &args[0], &args[1], false)
        }
        ("alter_semantic_view_rename_if_exists", 2) => {
            rewrite_alter_rename(&catalog, &args[0], &args[1], true)
        }
        ("alter_semantic_view_set_comment", 2) => {
            rewrite_alter_comment(&catalog, &args[0], Some(&args[1]), false)
        }
        ("alter_semantic_view_set_comment_if_exists", 2) => {
            rewrite_alter_comment(&catalog, &args[0], Some(&args[1]), true)
        }
        ("alter_semantic_view_unset_comment", 1) => {
            rewrite_alter_comment(&catalog, &args[0], None, false)
        }
        ("alter_semantic_view_unset_comment_if_exists", 1) => {
            rewrite_alter_comment(&catalog, &args[0], None, true)
        }
        // Caller pre-filtered to known names; this is unreachable. If hit, it
        // is an internal dispatch bug — surface as an error instead of
        // silently producing wrong SQL.
        _ => Err(ParseError {
            message: format!(
                "internal error: rewrite_drop_or_alter dispatched with unknown \
                 fn_name='{fn_name}' arity={}",
                args.len()
            ),
            position: None,
        }),
    }
}

/// CREATE-side native-SQL emission. Phase 62 takes `&OverrideContext`
/// directly: needs the runtime `CatalogReader` for existence checks AND for
/// catalog-side queries performed by `enrich_definition_for_create` (PK
/// lookup, type inference, fact typing).
#[cfg(feature = "extension")]
fn rewrite_create(
    ctx: &OverrideContext,
    fn_name: &str,
    args: &[String],
) -> Result<Option<String>, ParseError> {
    if args.len() != 2 {
        return Err(ParseError {
            message: format!(
                "internal error: rewrite_create dispatched with arity={}",
                args.len()
            ),
            position: None,
        });
    }
    let name = unescape_sql_arg(&args[0]);
    let json = unescape_sql_arg(&args[1]);

    let (or_replace, if_not_exists) = match fn_name {
        "create_semantic_view_from_json" => (false, false),
        "create_or_replace_semantic_view_from_json" => (true, false),
        "create_semantic_view_if_not_exists_from_json" => (false, true),
        _ => {
            return Err(ParseError {
                message: format!("internal error: rewrite_create unknown fn_name='{fn_name}'"),
                position: None,
            });
        }
    };

    let def =
        crate::model::SemanticViewDefinition::from_json(&name, &json).map_err(|e| ParseError {
            message: e,
            position: None,
        })?;

    emit_native_create_sql(ctx, &name, def, or_replace, if_not_exists)
}

/// Shared CREATE-emission helper used by both the table-function-style CREATE
/// path (`rewrite_create`) and the FROM YAML FILE path (`rewrite_yaml_file_create`).
///
/// Steps:
/// 1. Existence pre-check on committed state (skipped for OR REPLACE).
/// 2. Run `enrich_definition_for_create` against the catalog connection (PK
///    resolution, validation, metadata capture, type inference, fact typing).
/// 3. Emit `INSERT [OR REPLACE] INTO semantic_layer._definitions ...
///    RETURNING name AS view_name` so DuckDB executes the write on the
///    caller's connection inside the caller's transaction.
///
/// Returns the legacy 0-row `SELECT ... WHERE 1 = 0` shape for IF NOT EXISTS
/// when the view already exists; errors plain CREATE on an existing view.
#[cfg(feature = "extension")]
fn emit_native_create_sql(
    ctx: &OverrideContext,
    name: &str,
    def: crate::model::SemanticViewDefinition,
    or_replace: bool,
    if_not_exists: bool,
) -> Result<Option<String>, ParseError> {
    let name_escaped = escape_sql_arg(name);

    // Parse-time existence check: fast path for the committed-state case.
    // Same-txn CREATE-then-CREATE slips past this (the catalog connection
    // only sees committed rows), so the generated SQL below also guards
    // against the in-flight case via a CASE+error() / WHERE NOT EXISTS
    // pattern that runs on the caller's transaction.
    let exists = ctx.catalog.exists(name).map_err(|e| ParseError {
        message: format!("catalog lookup failed: {e}"),
        position: None,
    })?;

    if exists && !or_replace {
        if if_not_exists {
            return Ok(Some(format!(
                "SELECT '{name_escaped}'::VARCHAR AS view_name WHERE 1 = 0"
            )));
        }
        return Err(ParseError {
            message: format!(
                "semantic view '{name}' already exists; use CREATE OR REPLACE \
                 SEMANTIC VIEW to overwrite"
            ),
            position: None,
        });
    }

    // `is_file_backed` matches the legacy `DefineState::persist_conn.is_some()`
    // behaviour: type inference runs only for file-backed DBs (v0.7.1 design).
    let enriched_json = crate::ddl::define::enrich_definition_for_create(
        name,
        def,
        ctx.catalog.raw(),
        ctx.is_file_backed,
    )
    .map_err(|e| ParseError {
        message: e,
        position: None,
    })?;
    let enriched_escaped = escape_sql_arg(&enriched_json);

    // The generated SQL runs on the caller's connection, so its EXISTS
    // subqueries see in-flight INSERTs from the same transaction. Three
    // shapes:
    //   - OR REPLACE: straight INSERT OR REPLACE, no guard needed.
    //   - IF NOT EXISTS: INSERT OR IGNORE absorbs same-snapshot duplicates
    //     (the same-txn duplicate path, mirroring the SELECT WHERE 1=0
    //     fast path on committed-state hits). It does *not* paper over
    //     a cross-connection committer race: two transactions that each
    //     see no row will both INSERT, and DuckDB's PK constraint raises
    //     a write-write conflict on the second commit. That matches plain
    //     CREATE concurrency semantics — see TECH-DEBT item 23.
    //   - Plain CREATE: CASE+error() raises the friendly "already exists"
    //     message before the INSERT can fire, replacing what would
    //     otherwise be a generic PK constraint violation.
    let sql = if or_replace {
        format!(
            "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) \
             VALUES ('{name_escaped}', '{enriched_escaped}') \
             RETURNING name AS view_name"
        )
    } else if if_not_exists {
        format!(
            "INSERT OR IGNORE INTO semantic_layer._definitions (name, definition) \
             VALUES ('{name_escaped}', '{enriched_escaped}') \
             RETURNING name AS view_name"
        )
    } else {
        format!(
            "INSERT INTO semantic_layer._definitions (name, definition) \
             SELECT \
               CASE WHEN EXISTS (SELECT 1 FROM semantic_layer._definitions \
                                 WHERE name = '{name_escaped}') \
                    THEN error('semantic view ''{name_escaped}'' already exists; \
                                use CREATE OR REPLACE SEMANTIC VIEW to overwrite') \
                    ELSE '{name_escaped}' \
               END, \
               '{enriched_escaped}' \
             RETURNING name AS view_name"
        )
    };
    Ok(Some(sql))
}

/// Read the FROM YAML FILE sentinel produced by `rewrite_ddl_yaml_file_body`,
/// fetch the file via `read_text()` on the catalog connection (preserves
/// `DuckDB`'s filesystem support — local paths, `https://`, S3 via httpfs,
/// etc.), parse the YAML into a `SemanticViewDefinition`, then emit a
/// transactional INSERT through `emit_native_create_sql` so the write
/// participates in the caller's transaction.
///
/// Returns a `ParseError` when no `parser_override` context is installed for
/// this `OverrideContext`. Phase 62 takes `&OverrideContext` directly —
/// the LRU `db_token` indirection is gone (TECH-DEBT 20).
#[cfg(feature = "extension")]
fn rewrite_yaml_file_create(
    ctx: &OverrideContext,
    payload: &str,
) -> Result<Option<String>, ParseError> {
    use std::ffi::CStr;
    use std::os::raw::c_void;

    use libduckdb_sys as ffi;

    // Sentinel format: `<path>\x01<kind>\x01<name>\x01<comment>` (the
    // `__SV_YAML_FILE__` prefix has already been stripped by the caller).
    let mut parts = payload.splitn(4, '\x01');
    let file_path = parts.next().ok_or_else(|| ParseError {
        message: "Internal error: malformed YAML FILE sentinel (missing path)".to_string(),
        position: None,
    })?;
    let kind_str = parts.next().ok_or_else(|| ParseError {
        message: "Internal error: malformed YAML FILE sentinel (missing kind)".to_string(),
        position: None,
    })?;
    let name = parts.next().ok_or_else(|| ParseError {
        message: "Internal error: malformed YAML FILE sentinel (missing name)".to_string(),
        position: None,
    })?;
    let comment = parts.next().unwrap_or("");

    let (or_replace, if_not_exists) = match kind_str {
        "0" => (false, false),
        "1" => (true, false),
        "2" => (false, true),
        _ => {
            return Err(ParseError {
                message: format!("Internal error: unknown YAML FILE kind '{kind_str}'"),
                position: None,
            });
        }
    };

    // Read file via read_text(); this is one DuckDB statement on the catalog
    // connection so the user's transaction state is untouched.
    let path_escaped = file_path.replace('\'', "''");
    let read_sql = format!("SELECT content FROM read_text('{path_escaped}')");
    let mut result =
        unsafe { crate::query::table_function::execute_sql_raw(ctx.catalog.raw(), &read_sql) }
            .map_err(|e| ParseError {
                message: format!("FROM YAML FILE failed: {e}"),
                position: None,
            })?;
    let row_count = unsafe { ffi::duckdb_row_count(&mut result) };
    if row_count == 0 {
        unsafe { ffi::duckdb_destroy_result(&mut result) };
        return Err(ParseError {
            message: format!("FROM YAML FILE failed: no content returned from '{file_path}'"),
            position: None,
        });
    }
    let yaml_content = unsafe {
        let val_ptr = ffi::duckdb_value_varchar(&mut result, 0, 0);
        if val_ptr.is_null() {
            ffi::duckdb_destroy_result(&mut result);
            return Err(ParseError {
                message: format!("FROM YAML FILE failed: NULL content from '{file_path}'"),
                position: None,
            });
        }
        let s = CStr::from_ptr(val_ptr).to_string_lossy().into_owned();
        ffi::duckdb_free(val_ptr.cast::<c_void>());
        ffi::duckdb_destroy_result(&mut result);
        s
    };

    let mut def =
        crate::model::SemanticViewDefinition::from_yaml_with_size_cap(name, &yaml_content)
            .map_err(|e| ParseError {
                message: e,
                position: None,
            })?;

    if !comment.is_empty() {
        def.comment = Some(comment.to_string());
    }

    // Cardinality inference runs against PK declarations in the YAML; the
    // shared enrichment helper re-runs it after catalog PK resolution but
    // we still need this initial pass to populate cardinality where the YAML
    // already specifies PKs.
    infer_cardinality(&def.tables, &mut def.joins)?;

    emit_native_create_sql(ctx, name, def, or_replace, if_not_exists)
}

// SQL-string escape helpers (round-trip pair).
//
// `escape_sql_arg` doubles single quotes so the input can be embedded inside
// a single-quoted SQL string literal: `O'Brien` → `O''Brien`. `unescape_sql_arg`
// reverses the doubling for values that arrived already-escaped (typically
// from `parse_table_function_call::args`, which retains the SQL form so its
// callers can re-embed without re-escaping).
//
// The pair is unconditionally compiled (no `#[cfg(feature = "extension")]`)
// so unit tests under `cargo test` can exercise the escaping rules without
// linking the loadable-extension stubs. They have no FFI dependencies.

/// Undo the SQL `''`-escaping retained in `parse_table_function_call`'s args.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
fn unescape_sql_arg(s: &str) -> String {
    s.replace("''", "'")
}

/// Re-escape a string for embedding in single-quoted SQL.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
fn escape_sql_arg(s: &str) -> String {
    s.replace('\'', "''")
}

/// Build the race-guard SELECT for non-IF-EXISTS DROP/ALTER (B1, v0.8.1).
///
/// `name_escaped` is the view name with single quotes already SQL-doubled
/// (matches the form returned by `parse_table_function_call::args`).
///
/// The emitted statement errors with `semantic view '<name>' was concurrently
/// dropped` when the row is missing from `semantic_layer._definitions`.
/// Caller appends `;` and the actual DELETE/UPDATE; both run on the caller's
/// connection in the same transaction so the guard's NOT EXISTS check is
/// snapshot-consistent with the DML that follows.
///
/// The CTE form `WITH op AS (DELETE ... RETURNING)` is rejected by `DuckDB`
/// 1.10.502 with `Parser Error: A CTE needs a SELECT`, so we use a
/// two-statement string instead. See the smoke test
/// `catalog::tests::two_statement_guard_then_dml_smoke` for the working shape.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
fn race_guard_select(name_escaped: &str) -> String {
    format!(
        "SELECT CASE WHEN NOT EXISTS \
                   (SELECT 1 FROM semantic_layer._definitions WHERE name = '{name_escaped}') \
                THEN error('semantic view ''{name_escaped}'' was concurrently dropped') \
                ELSE TRUE END"
    )
}

#[cfg(feature = "extension")]
fn rewrite_drop(
    catalog: &crate::catalog::CatalogReader,
    name_escaped: &str,
    if_exists: bool,
) -> Result<Option<String>, ParseError> {
    let name = unescape_sql_arg(name_escaped);
    let exists = catalog.exists(&name).map_err(|e| ParseError {
        message: format!("catalog lookup failed: {e}"),
        position: None,
    })?;

    if !exists {
        if if_exists {
            // Silent no-op, but emit a SELECT that returns the same one-row
            // schema (`view_name VARCHAR`) the legacy path produces.
            return Ok(Some(format!(
                "SELECT '{name_escaped}'::VARCHAR AS view_name WHERE 1 = 0"
            )));
        }
        return Err(ParseError {
            message: format!("semantic view '{name}' does not exist"),
            position: None,
        });
    }

    if if_exists {
        // IF EXISTS keeps its silent no-op contract on race: the catalog
        // pre-check saw the row, but if a concurrent DROP commits before our
        // DELETE runs, the DELETE simply affects 0 rows and that is fine.
        return Ok(Some(format!(
            "DELETE FROM semantic_layer._definitions WHERE name = '{name_escaped}' \
             RETURNING name AS view_name"
        )));
    }

    // Race guard: catalog.exists() reads committed state via the catalog
    // connection — a separate connection from the caller's. Between that
    // pre-check and the DELETE running on the caller's connection, another
    // session can commit a DROP of the same view. Without a guard the DELETE
    // would silently affect 0 rows. The guard SELECT runs on the caller's
    // connection in the same transaction as the DELETE, so its NOT EXISTS
    // check is snapshot-consistent. RETURNING from the DELETE is the
    // user-visible result.
    let guard = race_guard_select(name_escaped);
    Ok(Some(format!(
        "{guard}; \
         DELETE FROM semantic_layer._definitions WHERE name = '{name_escaped}' \
         RETURNING name AS view_name"
    )))
}

#[cfg(feature = "extension")]
fn rewrite_alter_rename(
    catalog: &crate::catalog::CatalogReader,
    old_escaped: &str,
    new_escaped: &str,
    if_exists: bool,
) -> Result<Option<String>, ParseError> {
    let old_name = unescape_sql_arg(old_escaped);
    let new_name = unescape_sql_arg(new_escaped);

    let exists = catalog.exists(&old_name).map_err(|e| ParseError {
        message: format!("catalog lookup failed: {e}"),
        position: None,
    })?;

    if !exists {
        if if_exists {
            // Silent no-op with the legacy two-column schema.
            return Ok(Some(format!(
                "SELECT '{old_escaped}'::VARCHAR AS old_name, \
                 '{new_escaped}'::VARCHAR AS new_name WHERE 1 = 0"
            )));
        }
        return Err(ParseError {
            message: format!("semantic view '{old_name}' does not exist"),
            position: None,
        });
    }

    if catalog.exists(&new_name).map_err(|e| ParseError {
        message: format!("catalog lookup failed: {e}"),
        position: None,
    })? {
        return Err(ParseError {
            message: format!("semantic view '{new_name}' already exists"),
            position: None,
        });
    }

    if if_exists {
        // IF EXISTS preserves its silent contract on race: pre-check saw the
        // old name; if a concurrent DROP commits before our UPDATE, the
        // UPDATE simply affects 0 rows.
        return Ok(Some(format!(
            "UPDATE semantic_layer._definitions SET name = '{new_escaped}' \
             WHERE name = '{old_escaped}' \
             RETURNING '{old_escaped}'::VARCHAR AS old_name, name AS new_name"
        )));
    }

    // Race guard (see rewrite_drop for rationale). PK uniqueness on the new
    // name is still validated by DuckDB during UPDATE; the pre-check above
    // only gives a friendlier error in the non-race case.
    let guard = race_guard_select(old_escaped);
    Ok(Some(format!(
        "{guard}; \
         UPDATE semantic_layer._definitions SET name = '{new_escaped}' \
         WHERE name = '{old_escaped}' \
         RETURNING '{old_escaped}'::VARCHAR AS old_name, name AS new_name"
    )))
}

#[cfg(feature = "extension")]
fn rewrite_alter_comment(
    catalog: &crate::catalog::CatalogReader,
    name_escaped: &str,
    new_comment_escaped: Option<&str>,
    if_exists: bool,
) -> Result<Option<String>, ParseError> {
    let name = unescape_sql_arg(name_escaped);

    let json_str = catalog.lookup(&name).map_err(|e| ParseError {
        message: format!("catalog lookup failed: {e}"),
        position: None,
    })?;

    let Some(json_str) = json_str else {
        if if_exists {
            // Silent no-op with the legacy (name, status) schema.
            return Ok(Some(format!(
                "SELECT '{name_escaped}'::VARCHAR AS name, 'no-op'::VARCHAR AS status \
                 WHERE 1 = 0"
            )));
        }
        return Err(ParseError {
            message: format!("semantic view '{name}' does not exist"),
            position: None,
        });
    };

    let mut def: crate::model::SemanticViewDefinition =
        serde_json::from_str(&json_str).map_err(|e| ParseError {
            message: format!("failed to parse stored definition: {e}"),
            position: None,
        })?;

    let status_label = if new_comment_escaped.is_some() {
        "comment set"
    } else {
        "comment unset"
    };
    def.comment = new_comment_escaped.map(unescape_sql_arg);

    let new_json = serde_json::to_string(&def).map_err(|e| ParseError {
        message: format!("failed to serialize updated definition: {e}"),
        position: None,
    })?;
    let new_json_escaped = escape_sql_arg(&new_json);

    if if_exists {
        // IF EXISTS preserves its silent contract on race: pre-check saw the
        // row; if a concurrent DROP commits before our UPDATE, the UPDATE
        // simply affects 0 rows.
        return Ok(Some(format!(
            "UPDATE semantic_layer._definitions SET definition = '{new_json_escaped}' \
             WHERE name = '{name_escaped}' \
             RETURNING name, '{status_label}'::VARCHAR AS status"
        )));
    }

    // Race guard (see rewrite_drop for rationale). Note: this only guards
    // against concurrent DROP. A concurrent ALTER ... SET COMMENT or
    // ALTER ... RENAME against the same row could still cause a lost-update
    // (we serialized our new JSON from the lookup snapshot). That broader
    // optimistic-concurrency story is out of scope for v0.8.1.
    let guard = race_guard_select(name_escaped);
    Ok(Some(format!(
        "{guard}; \
         UPDATE semantic_layer._definitions SET definition = '{new_json_escaped}' \
         WHERE name = '{name_escaped}' \
         RETURNING name, '{status_label}'::VARCHAR AS status"
    )))
}

/// Result of parsing a `SELECT * FROM <fn>('arg1'[, 'arg2'])` SQL string.
///
/// `args` retains the original SQL escaping (single quotes doubled), so they
/// can be substituted back into a new single-quoted SQL string verbatim.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
struct TableFunctionCall {
    fn_name: String,
    args: Vec<String>,
}

/// Parse a `SELECT * FROM <fn_name>('arg1'[, 'arg2'])` SQL string.
///
/// Returns `None` for SQL that doesn't match this exact shape (e.g. SHOW
/// forms with WHERE/LIMIT, or unrecognized prefixes). Handles SQL `''`
/// escaping inside single-quoted args; preserves the `''` form in the
/// returned strings so callers can re-embed them in new single-quoted SQL
/// without re-escaping.
///
/// v0.8.1 tightened (B4): rejects malformed shapes that earlier silently
/// swallowed — `foo(,)`, `foo('a',)` (trailing comma), `foo('a' 'b')`
/// (missing comma between args).
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
fn parse_table_function_call(sql: &str) -> Option<TableFunctionCall> {
    const PREFIX: &str = "SELECT * FROM ";
    let rest = sql.strip_prefix(PREFIX)?;

    // Read the function name up to '('.
    let paren_pos = rest.find('(')?;
    let fn_name = rest[..paren_pos].trim().to_string();
    if fn_name.is_empty() || fn_name.contains(char::is_whitespace) {
        return None;
    }

    // Body after the opening paren up to the matching closing paren.
    // The body is a comma-separated list of single-quoted strings; we walk
    // it tracking quote state so commas inside strings don't split args.
    let body = &rest[paren_pos + 1..];
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = body.char_indices();
    let mut closing_pos: Option<usize> = None;
    // After the closing `'` of a string literal, only whitespace / `,` / `)`
    // are valid. A second `'` would have been peeked-and-consumed in the
    // in_quote branch (doubled-quote escape).
    let mut just_closed_quote = false;
    // Tracks whether the most recent unquoted-state event was a `,`. Used to
    // reject `foo(,)` / `foo('a',)` where a comma is followed by `)` with no
    // intervening arg.
    let mut expecting_arg_after_comma = false;

    while let Some((i, ch)) = chars.next() {
        if in_quote {
            current.push(ch);
            if ch == '\'' {
                // Lookahead for `''` doubled-quote escape.
                let mut peek = body[i + ch.len_utf8()..].chars();
                if peek.next() == Some('\'') {
                    // Consume the second '
                    current.push('\'');
                    chars.next();
                } else {
                    in_quote = false;
                    just_closed_quote = true;
                }
            }
        } else {
            match ch {
                '\'' => {
                    // Two adjacent string literals like `'a' 'b'` (no comma)
                    // are invalid in our generated SQL — reject.
                    if just_closed_quote {
                        return None;
                    }
                    in_quote = true;
                    just_closed_quote = false;
                    expecting_arg_after_comma = false;
                    current.push(ch);
                }
                ',' => {
                    let trimmed = current.trim();
                    if trimmed.is_empty() {
                        // `foo(,...)` — comma without preceding arg.
                        return None;
                    }
                    args.push(strip_outer_quotes(trimmed)?.to_string());
                    current.clear();
                    just_closed_quote = false;
                    expecting_arg_after_comma = true;
                }
                ')' => {
                    if expecting_arg_after_comma {
                        // `foo('a',)` — trailing comma.
                        return None;
                    }
                    closing_pos = Some(i);
                    break;
                }
                c if c.is_whitespace() => {} // ignore between args
                _ => return None,            // unexpected non-whitespace, non-quote
            }
        }
    }

    let _ = closing_pos?; // must have found a closing paren

    // Push trailing arg if present (handles single-arg and multi-arg cases).
    let trailing = current.trim();
    if !trailing.is_empty() {
        args.push(strip_outer_quotes(trailing)?.to_string());
    }

    // Anything after the closing paren must be empty or whitespace.
    let after = &body[closing_pos? + 1..];
    if !after.trim().is_empty() {
        return None;
    }

    Some(TableFunctionCall { fn_name, args })
}

/// Strip the outer pair of single quotes, leaving doubled-quote escaping intact.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
fn strip_outer_quotes(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner)
}

/// FFI entry point: construct a heap-boxed `OverrideContext` and return its
/// raw pointer to the C++ shim. Phase 62 replaced the per-load `db_token`
/// LRU with this direct ownership: the C++ shim stashes the returned
/// pointer inside its `SemanticViewsParserInfo` and hands it back to
/// `sv_parser_override_rust` on every parse.
///
/// # Safety
///
/// - `conn` must be a valid (or null) `duckdb_connection`. The pointer is
///   stored verbatim inside the boxed `CatalogReader`; it is intentionally
///   NOT closed by the resulting `Drop` (see `Drop for OverrideContext`).
/// - The returned pointer must be passed to `sv_drop_override_context`
///   exactly once when the C++ shim's `SemanticViewsParserInfo` is
///   destroyed. Never call `sv_drop_override_context` more than once on
///   the same pointer.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_make_override_context(
    conn: libduckdb_sys::duckdb_connection,
    is_file_backed: bool,
) -> *mut std::ffi::c_void {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = Box::new(OverrideContext {
            catalog: crate::catalog::CatalogReader::new(conn),
            is_file_backed,
        });
        Box::into_raw(ctx) as *mut std::ffi::c_void
    }));
    result.unwrap_or(std::ptr::null_mut())
}

/// FFI entry point: re-box and drop the `OverrideContext` allocated by
/// `sv_make_override_context`. The Rust-side `Box` allocation is freed.
///
/// # Safety
///
/// - `ctx_ptr` must be a value previously returned by
///   `sv_make_override_context`, or null.
/// - Must not be called more than once on the same pointer (use-after-free).
/// - The `Drop for OverrideContext` impl deliberately does NOT call
///   `duckdb_disconnect` on the inner connection — see Phase 62 RESEARCH §Q2
///   (destruction-order showstopper). The connection is intentionally leaked.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_drop_override_context(ctx_ptr: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if ctx_ptr.is_null() {
            return;
        }
        // Re-box and drop. Drop impl above documents the intentional leak
        // of the inner `duckdb_connection`.
        let _ = Box::from_raw(ctx_ptr as *mut OverrideContext);
    }));
}

/// FFI entry point for parser_override. The sole DDL entry point for the
/// extension as of v0.8.1 — the legacy parse_function/parse_stub path was
/// retired. Rewrites recognized semantic-view DDL into native SQL suitable
/// for re-parsing through DuckDB's own parser and execution on the caller's
/// connection.
///
/// Phase 62: takes an opaque `ctx_ptr` (a `Box<OverrideContext>*` produced
/// by `sv_make_override_context`) instead of the legacy `db_token` LRU
/// lookup — the override context now lives directly inside
/// `SemanticViewsParserInfo` (see `cpp/src/shim.cpp`).
///
/// Returns:
///   0 = success: heap-owned native SQL pointer + length written to
///       `*sql_out_ptr` / `*sql_out_len`. Caller takes ownership and must
///       release via `sv_free_buffer`. The buffer is **not** NUL-terminated;
///       read exactly `*sql_out_len` bytes.
///   1 = validation error / near-miss suggestion: error message written to
///       `error_out`. (Currently unused under FALLBACK_OVERRIDE; kept for
///       Phase 62 Plan 03 once `parse_function` returns to caret rendering.)
///   2 = not ours: defer to default parser. Used both for genuinely
///       non-semantic SQL, for null `ctx_ptr`, and for the early-return on
///       null/empty input or invalid UTF-8.
///
/// # Safety
///
/// - `ctx_ptr` must be a non-null pointer previously returned by
///   `sv_make_override_context`, or null. Null returns rc=2.
/// - `query_ptr` must point to bytes of length `query_len` (validated as
///   UTF-8 here; invalid UTF-8 returns 2 rather than triggering UB).
/// - `sql_out_ptr` must point to a writable `*mut u8` slot, or be null.
/// - `sql_out_len` must point to a writable `usize` slot, or be null.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_parser_override_rust(
    ctx_ptr: *const std::ffi::c_void,
    query_ptr: *const u8,
    query_len: usize,
    sql_out_ptr: *mut *mut u8,
    sql_out_len: *mut usize,
    error_out: *mut u8,
    error_out_len: usize,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if ctx_ptr.is_null() {
            return 2_u8; // no context — defer
        }
        if query_ptr.is_null() || query_len == 0 {
            return 2_u8; // not ours
        }
        // Reject invalid UTF-8 cleanly rather than relying on
        // from_utf8_unchecked (B2 hardening). DuckDB query strings are
        // UTF-8 by spec but a malformed input must not trigger UB.
        let bytes = std::slice::from_raw_parts(query_ptr, query_len);
        let Ok(query) = std::str::from_utf8(bytes) else {
            return 2; // not ours — defer
        };

        let ctx = &*(ctx_ptr as *const OverrideContext);

        match rewrite_to_native_sql(ctx, query) {
            Ok(Some(sql)) => {
                publish_owned_sql(sql, sql_out_ptr, sql_out_len);
                0 // success — native SQL handed to caller
            }
            Ok(None) => {
                // Genuinely not ours — defer to the default parser. If the
                // input is a near-miss for one of our DDL prefixes (e.g.
                // `CRETAE SEMANTIC VIEW`), `parse_function` (registered
                // alongside `parser_override` from Phase 62 Plan 03 onward)
                // will pick this up after the default parser fails on the
                // unrecognised prefix and re-render the suggestion via
                // DISPLAY_EXTENSION_ERROR with caret position.
                let _ = (error_out, error_out_len); // unused under Phase 62
                2 // not ours, defer to default parser
            }
            Err(_err) => {
                // Phase 62: defer to default parser → `sv_parse_stub`
                // (registered as `parse_function`) re-runs validation and
                // returns DISPLAY_EXTENSION_ERROR with caret position. The
                // synthesised `SELECT error('...')` workaround used in
                // v0.8.1 (sql_throwing) has been deleted now that DuckDB's
                // ParserException::SyntaxError caret rendering is reachable
                // again via the parse_function code path. Resolves
                // TECH-DEBT 22.
                let _ = (error_out, error_out_len); // unused under Phase 62
                2
            }
        }
    }));

    result.unwrap_or(2) // on panic: not ours
}

/// FFI entry point for `parse_function` — Phase 62's error-reporting layer.
///
/// Called by DuckDB's `Parser::ParseQuery` after the default parser fails on
/// an unrecognised prefix (e.g. `CREATE SEMANTIC VIEW …` or `CRETAE …`).
/// Re-runs validation against the user's input and returns the validation
/// error message + a byte-offset position so DuckDB's
/// `ParserException::SyntaxError` can render `LINE 1: … ^` (caret) at the
/// offending token.
///
/// Return code (`u8`):
///   * `0` — success / unreachable. `parser_override` should have produced
///     rewritten SQL on the success path; if validation succeeds AND we
///     reach `parse_function`, the override didn't fire. We map this to
///     rc=3 in practice; rc=0 is the defensive "internal error" case.
///   * `1` — recognised prefix, but body is invalid OR a near-miss
///     (`CRETAE` etc.) suggestion was produced. `error_out` gets the
///     message; `position_out` gets the byte offset (or `u32::MAX` if no
///     position is available).
///   * `2` — not ours; defer (`DISPLAY_ORIGINAL_ERROR` on the C++ side).
///   * `3` — valid DDL but `parser_override` didn't fire (override setting
///     is `DEFAULT` or `STRICT`, e.g. after `CALL disable_peg_parser()`
///     reset the setting). `error_out` gets an actionable hint
///     (`SET allow_parser_override_extension='FALLBACK'`); `position_out=0`
///     so the caret lands on the `C` of `CREATE` / `D` of `DROP`.
///
/// `ctx_ptr` is the same `Box<OverrideContext>*` handed to
/// `sv_parser_override_rust`. When non-null we re-run the FULL rewrite
/// (including catalog-aware existence checks in `rewrite_drop` /
/// `rewrite_alter`); when null we fall back to syntax-only validation.
/// This is what lets parse_function reproduce the same error message
/// `parser_override` saw — including "semantic view '…' does not exist"
/// for DROP-of-missing — with caret rendering attached.
///
/// # Safety
///
/// - `query_ptr` must point to bytes of length `query_len`. Invalid UTF-8
///   makes us return rc=2 (defer) rather than triggering UB.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes,
///   or be null. Null is treated as "do not write the message" (rc still
///   computed correctly).
/// - `position_out` must point to a writable `u32`, or be null. Null is
///   treated as "do not write the position".
#[cfg(any(feature = "extension", test))]
#[no_mangle]
pub unsafe extern "C" fn sv_parse_function_rust(
    ctx_ptr: *const std::ffi::c_void,
    query_ptr: *const u8,
    query_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
    position_out: *mut u32,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Initialise position_out to UINT32_MAX (no-position sentinel).
        if !position_out.is_null() {
            *position_out = u32::MAX;
        }

        // UTF-8 check; defer rather than synthesise an error on bad bytes.
        if query_ptr.is_null() || query_len == 0 {
            return 2_u8;
        }
        let bytes = std::slice::from_raw_parts(query_ptr, query_len);
        let Ok(query) = std::str::from_utf8(bytes) else {
            return 2_u8;
        };

        // Recognised DDL prefix?
        if detect_ddl_kind(query).is_none() {
            // Not a recognised prefix — try near-miss detection so the
            // user sees `Did you mean CREATE SEMANTIC VIEW?` instead of
            // a generic default-parser syntax error.
            if let Some(err) = detect_near_miss(query) {
                write_error_to_buffer(error_out, error_out_len, &err.message);
                if !position_out.is_null() {
                    *position_out = err
                        .position
                        .and_then(|p| u32::try_from(p).ok())
                        .unwrap_or(u32::MAX);
                }
                return 1_u8;
            }
            return 2_u8; // genuinely not ours
        }

        // Recognised prefix — re-run validation. When ctx_ptr is non-null
        // (production path) use rewrite_to_native_sql so catalog-level
        // errors (DROP-of-missing, ALTER-renaming-to-existing-name, …) are
        // reproduced here just as parser_override saw them. When ctx_ptr
        // is null (unit tests) fall back to syntax-only validation.
        let result = run_validation_for_parse_function(ctx_ptr, query);

        match result {
            Ok(Some(_rewritten)) => {
                // Valid DDL but we got here — `parser_override` must not have
                // fired. Most common cause: `disable_peg_parser` reset
                // `allow_parser_override_extension` to DEFAULT (TECH-DEBT 21).
                // Position 0 puts the caret on the `C` of CREATE / `D` of
                // DROP / etc.
                let msg = "semantic_views: parser_override is not active for \
                           this connection (allow_parser_override_extension is \
                           'DEFAULT' or 'STRICT'). Re-enable with: \
                           SET allow_parser_override_extension='FALLBACK';";
                write_error_to_buffer(error_out, error_out_len, msg);
                if !position_out.is_null() {
                    *position_out = 0;
                }
                3_u8
            }
            Ok(None) => {
                // detect_ddl_kind matched but validate returned None —
                // unreachable for a matched prefix. Defensive.
                write_error_to_buffer(
                    error_out,
                    error_out_len,
                    "semantic_views: internal error — recognised DDL prefix \
                     produced no rewrite (please report this bug)",
                );
                1_u8
            }
            Err(parse_err) => {
                write_error_to_buffer(error_out, error_out_len, &parse_err.message);
                if !position_out.is_null() {
                    *position_out = parse_err
                        .position
                        .and_then(|p| u32::try_from(p).ok())
                        .unwrap_or(u32::MAX);
                }
                1_u8
            }
        }
    }));

    result.unwrap_or(2) // on panic: not ours
}

/// Re-run validation for the parse_function path. Mirrors what
/// `sv_parser_override_rust` did at parse time: catalog-aware rewrite when a
/// context is available, syntax-only validation when not (unit tests + the
/// future case where the C++ shim has lost its rust_state).
///
/// Returning `Ok(Some(_))` means "validation succeeded" — at the
/// parse_function call site this can only happen when parser_override
/// itself didn't run, so the caller maps it to rc=3 (actionable hint).
#[cfg(feature = "extension")]
unsafe fn run_validation_for_parse_function(
    ctx_ptr: *const std::ffi::c_void,
    query: &str,
) -> Result<Option<String>, ParseError> {
    if ctx_ptr.is_null() {
        return validate_and_rewrite(query);
    }
    let ctx = &*(ctx_ptr as *const OverrideContext);
    rewrite_to_native_sql(ctx, query)
}

/// Test-only sibling of `run_validation_for_parse_function` — pure syntax
/// validation. Under `cargo test` the `extension` feature is OFF (default
/// features = bundled), so `rewrite_to_native_sql` is unavailable; ctx_ptr
/// is always null in tests anyway.
#[cfg(all(not(feature = "extension"), test))]
unsafe fn run_validation_for_parse_function(
    _ctx_ptr: *const std::ffi::c_void,
    query: &str,
) -> Result<Option<String>, ParseError> {
    validate_and_rewrite(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // parse_table_function_call — happy-path + B4 tightening (v0.8.1).
    // Pre-v0.8.1 this silently swallowed `foo(,)` and `foo('a',)` and
    // accepted `foo('a' 'b')` (missing comma). Now they all return None.
    // ===================================================================

    #[test]
    fn parse_tf_call_zero_args() {
        let r = parse_table_function_call("SELECT * FROM foo()").expect("zero-arg parse");
        assert_eq!(r.fn_name, "foo");
        assert!(r.args.is_empty());
    }

    #[test]
    fn parse_tf_call_single_arg() {
        let r = parse_table_function_call("SELECT * FROM foo('a')").expect("single-arg parse");
        assert_eq!(r.fn_name, "foo");
        assert_eq!(r.args, vec!["a".to_string()]);
    }

    #[test]
    fn parse_tf_call_multi_arg() {
        let r = parse_table_function_call("SELECT * FROM foo('a', 'b')").expect("multi-arg parse");
        assert_eq!(r.fn_name, "foo");
        assert_eq!(r.args, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parse_tf_call_doubled_quote_inside_arg() {
        // `'O''Brien'` decodes to `O''Brien` (escaping retained).
        let r = parse_table_function_call("SELECT * FROM foo('O''Brien')")
            .expect("doubled-quote parse");
        assert_eq!(r.args, vec!["O''Brien".to_string()]);
    }

    #[test]
    fn parse_tf_call_rejects_lone_comma() {
        assert!(parse_table_function_call("SELECT * FROM foo(,)").is_none());
    }

    #[test]
    fn parse_tf_call_rejects_trailing_comma() {
        assert!(parse_table_function_call("SELECT * FROM foo('a',)").is_none());
        assert!(parse_table_function_call("SELECT * FROM foo('a', )").is_none());
    }

    #[test]
    fn parse_tf_call_rejects_missing_comma() {
        assert!(parse_table_function_call("SELECT * FROM foo('a' 'b')").is_none());
    }

    #[test]
    fn parse_tf_call_rejects_unknown_prefix() {
        assert!(parse_table_function_call("INSERT INTO t VALUES ('a')").is_none());
        assert!(parse_table_function_call("describe_semantic_view('foo')").is_none());
    }

    // ===================================================================
    // B1 / D6: race-guard SQL shape. Pinned so a future refactor cannot
    // silently drop the snapshot-consistent existence check that protects
    // non-IF-EXISTS DROP / ALTER from a concurrent commit landing between
    // the catalog pre-check (separate connection) and the DML.
    // ===================================================================

    // ===================================================================
    // C2: SQL escape helpers — round-trip pair, no extension feature.
    // ===================================================================

    #[test]
    fn escape_sql_arg_doubles_single_quotes() {
        assert_eq!(escape_sql_arg(""), "");
        assert_eq!(escape_sql_arg("plain"), "plain");
        assert_eq!(escape_sql_arg("O'Brien"), "O''Brien");
        assert_eq!(escape_sql_arg("a'b'c"), "a''b''c");
        assert_eq!(escape_sql_arg("''"), "''''");
    }

    #[test]
    fn unescape_sql_arg_undoes_escape() {
        assert_eq!(unescape_sql_arg(""), "");
        assert_eq!(unescape_sql_arg("plain"), "plain");
        assert_eq!(unescape_sql_arg("O''Brien"), "O'Brien");
        assert_eq!(unescape_sql_arg("a''b''c"), "a'b'c");
    }

    #[test]
    fn escape_unescape_round_trip() {
        for s in [
            "",
            "plain",
            "O'Brien",
            "''already-doubled''",
            "mix 'of' quotes",
            "trailing'",
        ] {
            assert_eq!(unescape_sql_arg(&escape_sql_arg(s)), s);
        }
    }

    #[test]
    fn race_guard_select_emits_not_exists_and_error() {
        let g = race_guard_select("sales");
        assert!(g.contains("NOT EXISTS"), "missing NOT EXISTS: {g}");
        assert!(
            g.contains("FROM semantic_layer._definitions WHERE name = 'sales'"),
            "guard targets wrong table/predicate: {g}"
        );
        assert!(
            g.contains("error('semantic view ''sales'' was concurrently dropped')"),
            "missing error() with friendly message: {g}"
        );
        // Must be a SELECT (so it can run as the first of two statements
        // without affecting catalog state when the row is present).
        assert!(g.trim_start().starts_with("SELECT "), "not a SELECT: {g}");
        // Must not contain a trailing ';' — the caller appends ';' + DML.
        assert!(!g.contains(';'), "guard must not include ';' itself: {g}");
    }

    #[test]
    fn race_guard_select_doubles_quotes_in_name() {
        // name_escaped already has '' for single quotes; embedding it inside
        // an outer SQL string literal preserves correct decoding (DuckDB
        // sees ''X'' as 'X' in the literal). The user-facing error message
        // must read: semantic view 'O'Brien' was concurrently dropped.
        let g = race_guard_select("O''Brien");
        assert!(
            g.contains("WHERE name = 'O''Brien'"),
            "WHERE clause wrong: {g}"
        );
        assert!(
            g.contains("error('semantic view ''O''Brien'' was concurrently dropped')"),
            "error message wrong: {g}"
        );
    }

    // ===================================================================
    // Phase 62: OverrideContext direct-attach (replaces the v0.8.1 LRU).
    // The Drop impl MUST NOT call duckdb_disconnect (RESEARCH §Q2 —
    // destruction-order showstopper). The Box<OverrideContext> Rust
    // allocation IS reclaimed; the inner duckdb_connection leaks.
    // ===================================================================

    /// Sentinel marker for the destructor leak test. Stored at a known
    /// memory location so the test can verify the destructor did NOT
    /// touch `self.catalog.conn` (which would happen if a stray
    /// `duckdb_disconnect` call slipped back in).
    #[cfg(feature = "extension")]
    #[test]
    fn override_context_drop_does_not_disconnect() {
        // Allocate a u64 sentinel on the heap and hand its pointer to the
        // CatalogReader as if it were a duckdb_connection. If Drop calls
        // duckdb_disconnect on that pointer, the test process would
        // segfault (libduckdb would deref it as a Connection*). We only
        // assert that Drop returns cleanly — survival is the contract.
        let sentinel: Box<u64> = Box::new(0xDEAD_BEEF_CAFE_BABE);
        let raw = Box::into_raw(sentinel);
        let ctx = OverrideContext {
            catalog: crate::catalog::CatalogReader::new(raw as libduckdb_sys::duckdb_connection),
            is_file_backed: false,
        };
        // Drop runs here at end of scope. If duckdb_disconnect were
        // called, libduckdb would interpret `raw` as a Connection*, dispatch
        // through ConnectionManager and likely crash. Survival of this
        // function == Drop body did not call duckdb_disconnect.
        drop(ctx);
        // Reclaim the sentinel ourselves (Drop intentionally leaked it).
        unsafe {
            let _ = Box::from_raw(raw);
        }
    }

    #[cfg(feature = "extension")]
    #[test]
    fn sv_make_and_drop_override_context_round_trip() {
        // Construct via FFI ctor with a null connection (intentionally —
        // never dereferenced, just round-tripped through the Box).
        let ptr = unsafe {
            sv_make_override_context(
                std::ptr::null_mut() as libduckdb_sys::duckdb_connection,
                false,
            )
        };
        assert!(!ptr.is_null(), "ctor must return non-null for a valid Box");
        // Destruct via FFI dtor — must not panic, must not call
        // duckdb_disconnect (sentinel-test above pins that contract).
        unsafe { sv_drop_override_context(ptr) };
    }

    #[cfg(feature = "extension")]
    #[test]
    fn sv_drop_override_context_handles_null() {
        // Defensive: null-pointer drop must be a no-op (matches the
        // C++ shim's `if (rust_state) { ... }` guard pattern).
        unsafe { sv_drop_override_context(std::ptr::null_mut()) };
    }

    // ===================================================================
    // Phase 62 Plan 03 — sv_parse_function_rust rc=0/1/2/3 contract.
    // parse_function is reintroduced purely as the error-reporting layer
    // (caret rendering via DISPLAY_EXTENSION_ERROR + error_location).
    // parser_override now defers ALL error cases (rc=2) — the synthesised
    // SELECT error('...') workaround in sql_throwing is gone.
    // ===================================================================

    /// Helper: invoke sv_parse_function_rust with stack buffers and return
    /// (rc, error message, position). Available under default features
    /// because sv_parse_function_rust is a pure-Rust validation layer that
    /// does not touch the DuckDB C API.
    fn call_sv_parse_function(query: &str) -> (u8, String, u32) {
        let mut error_buf = vec![0_u8; 1024];
        let mut position: u32 = u32::MAX;
        let rc = unsafe {
            sv_parse_function_rust(
                std::ptr::null(),
                query.as_ptr(),
                query.len(),
                error_buf.as_mut_ptr(),
                error_buf.len(),
                &mut position as *mut u32,
            )
        };
        // Truncate error_buf at the first NUL.
        let nul = error_buf.iter().position(|&b| b == 0).unwrap_or(0);
        let msg = String::from_utf8_lossy(&error_buf[..nul]).into_owned();
        (rc, msg, position)
    }

    #[test]
    fn sv_parse_function_rust_returns_2_for_select() {
        // Plain SELECT is not ours — defer to default parser (rc=2).
        let (rc, _msg, _pos) = call_sv_parse_function("SELECT 1;");
        assert_eq!(rc, 2, "SELECT must defer with rc=2");
    }

    #[test]
    fn sv_parse_function_rust_returns_2_for_invalid_utf8() {
        // Invalid UTF-8 bytes — defer rather than panic (rc=2).
        let bad: [u8; 5] = [0xFF, 0xFE, 0xFD, 0x00, 0x00];
        let mut error_buf = vec![0_u8; 1024];
        let mut position: u32 = u32::MAX;
        let rc = unsafe {
            sv_parse_function_rust(
                std::ptr::null(),
                bad.as_ptr(),
                4, // exclude trailing nul, just 4 invalid bytes
                error_buf.as_mut_ptr(),
                error_buf.len(),
                &mut position as *mut u32,
            )
        };
        assert_eq!(rc, 2, "invalid UTF-8 must defer with rc=2");
    }

    #[test]
    fn sv_parse_function_rust_returns_1_with_position_for_malformed_create() {
        // CREATE prefix recognised but body mis-spelled — validate_and_rewrite
        // returns Err(ParseError) with position set. rc=1; position non-MAX.
        // We use the proven TABLSE typo (transposition) as in the existing
        // proptest at as_body_position_invariant_clause_typo.
        let query = "CREATE SEMANTIC VIEW v AS TABLSE (t);";
        let (rc, msg, pos) = call_sv_parse_function(query);
        assert_eq!(rc, 1, "malformed CREATE must return rc=1; msg={msg}");
        assert_ne!(
            pos,
            u32::MAX,
            "position must be set for malformed CREATE; msg={msg}"
        );
        assert!(!msg.is_empty(), "error message must be populated for rc=1");
    }

    #[test]
    fn sv_parse_function_rust_returns_1_for_near_miss() {
        // CRETAE is a near-miss for CREATE; detect_ddl_kind returns None,
        // detect_near_miss returns Some with position=0. rc=1; suggestion text.
        let query = "CRETAE SEMANTIC VIEW v AS TABLES (t);";
        let (rc, msg, pos) = call_sv_parse_function(query);
        assert_eq!(rc, 1, "near-miss must return rc=1; msg={msg}");
        assert_eq!(pos, 0, "near-miss position must be 0 (start of CRETAE)");
        assert!(
            msg.contains("Did you mean"),
            "near-miss must contain suggestion text; got: {msg}"
        );
    }

    #[cfg(feature = "extension")]
    #[test]
    fn sv_parser_override_rust_returns_2_for_validation_failure() {
        // Phase 62 contract change: the Err(_) branch of rewrite_to_native_sql
        // now returns rc=2 (defer) rather than synthesising a SELECT error('...')
        // statement via the deleted sql_throwing helper. parse_function picks
        // up the error reporting via caret rendering.
        let ctx_ptr = unsafe {
            sv_make_override_context(
                std::ptr::null_mut() as libduckdb_sys::duckdb_connection,
                false,
            )
        };
        assert!(!ctx_ptr.is_null());

        let query = "CREATE SEMANTIC VIEW v AS TABLSE (t);";
        let mut sql_ptr: *mut u8 = std::ptr::null_mut();
        let mut sql_len: usize = 0;
        let mut error_buf = vec![0_u8; 1024];
        let rc = unsafe {
            sv_parser_override_rust(
                ctx_ptr as *const std::ffi::c_void,
                query.as_ptr(),
                query.len(),
                &mut sql_ptr as *mut *mut u8,
                &mut sql_len as *mut usize,
                error_buf.as_mut_ptr(),
                error_buf.len(),
            )
        };
        assert_eq!(
            rc, 2,
            "parser_override Err branch must defer (rc=2) so parse_function can render caret"
        );
        assert!(
            sql_ptr.is_null(),
            "no rewritten SQL must be published on rc=2"
        );
        assert_eq!(sql_len, 0, "no SQL length on rc=2");

        unsafe { sv_drop_override_context(ctx_ptr) };
    }

    // ===================================================================
    // FFI heap-buffer round-trip — guards against the v0.8.0 silent-
    // truncation regression. Pre-fix the SQL output went through a
    // fixed 64 KB buffer; we now hand the C++ caller an owned heap
    // pointer + length, released via sv_free_buffer.
    // ===================================================================

    #[test]
    fn leak_and_reclaim_round_trips_arbitrary_string() {
        let original = "INSERT INTO _definitions VALUES ('x', '...');".repeat(4096);
        assert!(
            original.len() > 64 * 1024,
            "test input should exceed legacy cap"
        );

        let original_clone = original.clone();
        let (ptr, len) = leak_string_to_c_buffer(original);
        assert!(!ptr.is_null());
        assert_eq!(len, original_clone.len());

        // Read back exactly `len` bytes (no NUL terminator assumption).
        let recovered = unsafe { std::slice::from_raw_parts(ptr.cast_const(), len) };
        assert_eq!(recovered, original_clone.as_bytes());

        // Free.
        unsafe { reclaim_c_buffer(ptr, len) };
    }

    #[test]
    fn reclaim_null_pointer_is_safe() {
        // sv_free_buffer must accept null pointers as a no-op so the C++
        // RAII guard can be unconditionally invoked even when the FFI
        // call returned an error path.
        unsafe { reclaim_c_buffer(std::ptr::null_mut(), 0) };
        unsafe { reclaim_c_buffer(std::ptr::null_mut(), 99) };
    }

    #[test]
    fn leak_handles_empty_string() {
        let (ptr, len) = leak_string_to_c_buffer(String::new());
        assert_eq!(len, 0);
        // Empty Vec may have dangling-but-aligned ptr; reclaim must not crash.
        unsafe { reclaim_c_buffer(ptr, len) };
    }

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
    // Phase 57: SHOW SEMANTIC MATERIALIZATIONS tests (INTR-03)
    // -----------------------------------------------------------------------

    #[test]
    fn detect_show_materializations() {
        assert_eq!(
            detect_ddl_kind("SHOW SEMANTIC MATERIALIZATIONS"),
            Some(DdlKind::ShowMaterializations)
        );
    }

    #[test]
    fn detect_show_materializations_in_view() {
        assert_eq!(
            detect_ddl_kind("SHOW SEMANTIC MATERIALIZATIONS IN my_view"),
            Some(DdlKind::ShowMaterializations)
        );
    }

    #[test]
    fn rewrite_show_materializations_all() {
        let sql = rewrite_ddl("SHOW SEMANTIC MATERIALIZATIONS").unwrap();
        assert_eq!(sql, "SELECT * FROM show_semantic_materializations_all()");
    }

    #[test]
    fn rewrite_show_materializations_in_view() {
        let sql = rewrite_ddl("SHOW SEMANTIC MATERIALIZATIONS IN my_view").unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM show_semantic_materializations('my_view')"
        );
    }

    #[test]
    fn near_miss_show_materialization() {
        // "SHOW SEMANTIC MATERIALIZATION" (missing 'S') should suggest the correct prefix
        let result = detect_near_miss("SHOW SEMANTIC MATERIALIZATION");
        assert!(result.is_some());
        let err = result.unwrap();
        assert!(err.message.contains("Did you mean"), "got: {}", err.message);
    }

    #[test]
    fn extract_ddl_name_show_materializations_in() {
        let result = extract_ddl_name("SHOW SEMANTIC MATERIALIZATIONS IN my_view").unwrap();
        assert_eq!(result, Some("my_view".to_string()));
    }

    #[test]
    fn extract_ddl_name_show_materializations_all() {
        let result = extract_ddl_name("SHOW SEMANTIC MATERIALIZATIONS").unwrap();
        assert_eq!(result, None);
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

    // ===================================================================
    // Phase 53: FROM YAML FILE tests
    // ===================================================================

    #[test]
    fn test_extract_single_quoted_basic() {
        let (content, consumed) = extract_single_quoted("'/path/to/file.yaml'").unwrap();
        assert_eq!(content, "/path/to/file.yaml");
        assert_eq!(consumed, 20);
    }

    #[test]
    fn test_extract_single_quoted_escaped() {
        // '/file''s.yaml' = ' f i l e ' ' s . y a m l ' = 15 chars
        let (content, consumed) = extract_single_quoted("'/file''s.yaml'").unwrap();
        assert_eq!(content, "/file's.yaml");
        assert_eq!(consumed, 15);
    }

    #[test]
    fn test_extract_single_quoted_empty() {
        let (content, consumed) = extract_single_quoted("''").unwrap();
        assert_eq!(content, "");
        assert_eq!(consumed, 2);
    }

    #[test]
    fn test_extract_single_quoted_no_quote() {
        let err = extract_single_quoted("no quote").unwrap_err();
        assert!(
            err.message.contains("Expected single-quoted file path"),
            "Error: {}",
            err.message
        );
    }

    #[test]
    fn test_extract_single_quoted_unterminated() {
        let err = extract_single_quoted("'unterminated").unwrap_err();
        assert!(
            err.message.contains("Unterminated file path string"),
            "Error: {}",
            err.message
        );
    }

    #[test]
    fn test_rewrite_ddl_yaml_file_body_create() {
        let result =
            rewrite_ddl_yaml_file_body(DdlKind::Create, "myview", "'/path/to/def.yaml'", None)
                .unwrap();
        let sentinel = result.unwrap();
        assert!(sentinel.starts_with("__SV_YAML_FILE__"));
        assert!(sentinel.contains("path/to/def.yaml"));
        assert!(sentinel.contains("\x010\x01myview\x01"));
    }

    #[test]
    fn test_rewrite_ddl_yaml_file_body_replace() {
        let result = rewrite_ddl_yaml_file_body(
            DdlKind::CreateOrReplace,
            "v",
            "'/f.yaml'",
            Some("a comment".into()),
        )
        .unwrap();
        let sentinel = result.unwrap();
        assert!(sentinel.contains("\x011\x01v\x01a comment"));
    }

    #[test]
    fn test_rewrite_ddl_yaml_file_body_if_not_exists() {
        let result =
            rewrite_ddl_yaml_file_body(DdlKind::CreateIfNotExists, "v", "'/f.yaml'", None).unwrap();
        let sentinel = result.unwrap();
        assert!(sentinel.contains("\x012\x01v\x01"));
    }

    #[test]
    fn test_rewrite_ddl_yaml_file_body_with_comment() {
        let result = rewrite_ddl_yaml_file_body(
            DdlKind::Create,
            "v",
            "'/f.yaml'",
            Some("my comment".into()),
        )
        .unwrap();
        let sentinel = result.unwrap();
        assert!(sentinel.contains("my comment"));
    }

    #[test]
    fn test_rewrite_ddl_yaml_file_body_empty_path() {
        let err = rewrite_ddl_yaml_file_body(DdlKind::Create, "v", "''", None).unwrap_err();
        assert!(
            err.message.contains("File path cannot be empty"),
            "Error: {}",
            err.message
        );
    }

    #[test]
    fn test_rewrite_ddl_yaml_file_body_trailing_content() {
        let err = rewrite_ddl_yaml_file_body(DdlKind::Create, "v", "'/f.yaml' extra stuff", None)
            .unwrap_err();
        assert!(
            err.message.contains("Unexpected content after file path"),
            "Error: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_and_rewrite_yaml_file() {
        let query = "CREATE SEMANTIC VIEW v FROM YAML FILE '/test.yaml'";
        let result = validate_and_rewrite(query).unwrap();
        let sentinel = result.unwrap();
        assert!(
            sentinel.starts_with("__SV_YAML_FILE__"),
            "Expected sentinel prefix, got: {}",
            sentinel
        );
    }

    #[test]
    fn test_validate_and_rewrite_yaml_file_case_insensitive() {
        let query = "CREATE SEMANTIC VIEW v from yaml file '/test.yaml'";
        let result = validate_and_rewrite(query).unwrap();
        let sentinel = result.unwrap();
        assert!(sentinel.starts_with("__SV_YAML_FILE__"));
    }

    #[test]
    fn test_validate_and_rewrite_yaml_inline_still_works() {
        // Regression: FROM YAML $$...$$ still works after FILE branch is added
        let query = "CREATE SEMANTIC VIEW v FROM YAML $$\nbase_table: t\ntables: []\ndimensions: []\nmetrics: []\n$$";
        let result = validate_and_rewrite(query).unwrap();
        let sql = result.unwrap();
        assert!(sql.contains("create_semantic_view_from_json"));
    }

    #[test]
    fn test_error_message_mentions_from_yaml_file() {
        let query = "CREATE SEMANTIC VIEW v SOMETHING_ELSE";
        let err = validate_and_rewrite(query).unwrap_err();
        assert!(
            err.message.contains("FROM YAML FILE"),
            "Error should mention FROM YAML FILE: {}",
            err.message
        );
    }

    // ===================================================================
    // Quick task 260430-vdz: leading-comment skipping
    //
    // Failing-test-first: these reference `skip_leading_whitespace_and_comments`
    // and rely on the helper being applied at five trimming sites. They will
    // not compile/pass until the fix lands in the next commit.
    // ===================================================================

    #[test]
    fn skip_lws_empty() {
        assert_eq!(skip_leading_whitespace_and_comments(""), 0);
    }

    #[test]
    fn skip_lws_only_whitespace() {
        assert_eq!(skip_leading_whitespace_and_comments("   \n\t"), 5);
    }

    #[test]
    fn skip_lws_line_comment() {
        let q = "-- hi\nCREATE";
        assert_eq!(&q[skip_leading_whitespace_and_comments(q)..], "CREATE");
    }

    #[test]
    fn skip_lws_block_comment() {
        let q = "/* hi */ CREATE";
        assert_eq!(&q[skip_leading_whitespace_and_comments(q)..], "CREATE");
    }

    #[test]
    fn skip_lws_multiple_comments_and_ws() {
        let q = "-- a\n  /* b */\n\t-- c\n/*d*/CREATE";
        assert_eq!(&q[skip_leading_whitespace_and_comments(q)..], "CREATE");
    }

    #[test]
    fn skip_lws_block_does_not_nest() {
        // Outer ends at first */, leaving "trailing */ CREATE"
        let q = "/* outer /* inner */ trailing */ CREATE";
        let rest = &q[skip_leading_whitespace_and_comments(q)..];
        assert!(rest.starts_with("trailing"), "got: {rest:?}");
    }

    #[test]
    fn skip_lws_unterminated_block_consumes_to_eof() {
        let q = "/* never ends";
        assert_eq!(skip_leading_whitespace_and_comments(q), q.len());
    }

    #[test]
    fn skip_lws_no_leading_match() {
        // No comments and no whitespace -> offset 0
        assert_eq!(skip_leading_whitespace_and_comments("CREATE"), 0);
    }

    #[test]
    fn skip_lws_dash_dash_at_eof() {
        let q = "-- no newline at end";
        assert_eq!(skip_leading_whitespace_and_comments(q), q.len());
    }

    #[test]
    fn detect_create_with_leading_block_comment() {
        assert_eq!(
            detect_semantic_view_ddl("/* hi */ CREATE SEMANTIC VIEW x AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn detect_create_with_leading_line_comment() {
        assert_eq!(
            detect_semantic_view_ddl("-- hi\nCREATE SEMANTIC VIEW x AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn detect_create_or_replace_with_dbt_style_annotation() {
        let q = "/* {\"app\": \"dbt\", \"node_id\": \"model.x\"} */ CREATE OR REPLACE SEMANTIC VIEW x AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))";
        assert_eq!(detect_semantic_view_ddl(q), PARSE_DETECTED);
        let kind = detect_ddl_kind(q);
        assert_eq!(kind, Some(DdlKind::CreateOrReplace));
    }

    #[test]
    fn detect_other_ddl_forms_with_leading_comment() {
        for q in [
            "/* x */ DROP SEMANTIC VIEW v",
            "/* x */ ALTER SEMANTIC VIEW v RENAME TO w",
            "/* x */ DESCRIBE SEMANTIC VIEW v",
            "/* x */ SHOW SEMANTIC VIEWS",
            "/* x */ SHOW SEMANTIC METRICS IN v",
            "-- annotation\nDROP SEMANTIC VIEW v",
        ] {
            assert_eq!(detect_semantic_view_ddl(q), PARSE_DETECTED, "failed: {q}");
        }
    }

    #[test]
    fn comment_only_is_not_semantic_view_ddl() {
        assert_eq!(
            detect_semantic_view_ddl("/* just a comment */"),
            PARSE_NOT_OURS
        );
        assert_eq!(
            detect_semantic_view_ddl("-- just a comment\n"),
            PARSE_NOT_OURS
        );
    }

    #[test]
    fn validate_and_rewrite_with_leading_comment_succeeds() {
        let q = "/* annotation */ DROP SEMANTIC VIEW v";
        let result = validate_and_rewrite(q).expect("should not error");
        assert!(result.is_some(), "expected DDL detection");
        let sql = result.unwrap();
        assert!(sql.contains("drop_semantic_view"), "got: {sql}");
    }

    #[test]
    fn extract_ddl_name_with_leading_comment() {
        assert_eq!(
            extract_ddl_name("/* annotation */ DROP SEMANTIC VIEW my_view").unwrap(),
            Some("my_view".to_string())
        );
    }

    #[test]
    fn error_position_accounts_for_leading_comment() {
        // Missing view name -- error position should point at the offset AFTER
        // both the comment AND the prefix, in the ORIGINAL query string.
        let q = "/* hi */ DROP SEMANTIC VIEW";
        let err = validate_and_rewrite(q).expect_err("should error: missing name");
        let pos = err.position.expect("position should be set");
        // Position should be inside the original string (not into the stripped slice).
        // The prefix "DROP SEMANTIC VIEW" starts at byte 9 (after "/* hi */ ").
        // After consuming the prefix (18 bytes), we're at byte 27 == query.len().
        assert_eq!(pos, q.len(), "position should reference original query");
    }
}
