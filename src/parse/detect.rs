//! Prefix detection for semantic-view DDL statements.
//!
//! Extracted from `parse` (AR-1). Everything here answers "is this one of our
//! statements, and if so which kind?" without rewriting anything: keyword
//! prefix matching ([`match_keyword_prefix`]), comment/whitespace skipping
//! ([`skip_leading_whitespace_and_comments`]), the longest-first prefix table
//! ([`detect_ddl_prefix`]), the public detection entry points
//! ([`detect_ddl_kind`], [`detect_semantic_view_ddl`]), and fuzzy near-miss
//! suggestion ([`detect_near_miss`]).

use super::DdlKind;
use super::{PARSE_DETECTED, PARSE_NOT_OURS};
use crate::errors::ParseError;
use crate::util::is_ident_byte;

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
pub(crate) fn match_keyword_prefix(input: &[u8], keywords: &[&[u8]]) -> Option<usize> {
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
    // Require a word boundary after the FINAL keyword. Inter-keyword
    // boundaries are already enforced by the mandatory whitespace above,
    // but without this check `CREATE SEMANTIC VIEWfoo` matched, and the
    // plural typo `DROP SEMANTIC VIEWS` matched the `DROP SEMANTIC VIEW`
    // prefix and dropped a view named `s` (PA-4, code-review 2026-07-02).
    // Non-ASCII bytes (>= 0x80) are identifier continuation in DuckDB, so
    // they are NOT boundaries either; ASCII punctuation (whitespace, `(`,
    // `;`, `"`) is a legitimate token boundary and stays accepted.
    if pos < input.len() && is_ident_byte(input[pos]) {
        return None;
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
pub(crate) fn skip_leading_whitespace_and_comments(input: &str) -> usize {
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
pub(crate) fn detect_ddl_prefix(trimmed: &str) -> Option<(DdlKind, usize)> {
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
/// Returns `Some(DdlKind)` if the query matches one of the semantic view
/// DDL prefixes, `None` otherwise. Uses longest-first ordering to avoid
/// prefix overlap (e.g. "create or replace semantic view" before
/// "create semantic view").
///
/// Tolerates arbitrary ASCII whitespace (spaces, tabs, newlines, carriage
/// returns, vertical tabs, form feeds) between prefix keywords.
#[must_use]
pub fn detect_ddl_kind(query: &str) -> Option<DdlKind> {
    // PA-7: comment-blind detection — also lets a comment sit between
    // prefix keywords (`CREATE /* x */ SEMANTIC VIEW`).
    let blanked = crate::util::blank_sql_comments(query);
    let query = blanked.as_ref();
    let lead = skip_leading_whitespace_and_comments(query);
    let trimmed = query[lead..].trim_end().trim_end_matches(';').trim();
    detect_ddl_prefix(trimmed).map(|(kind, _)| kind)
}

/// Detect whether a query is any semantic view DDL statement.
///
/// Returns `PARSE_DETECTED` for any semantic view DDL form, `PARSE_NOT_OURS`
/// otherwise. Handles case variations, leading/trailing whitespace, and
/// trailing semicolons.
#[must_use]
pub fn detect_semantic_view_ddl(query: &str) -> u8 {
    if detect_ddl_kind(query).is_some() {
        PARSE_DETECTED
    } else {
        PARSE_NOT_OURS
    }
}

/// Known DDL prefixes for fuzzy near-miss matching.
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
/// of the known DDL prefixes (see `DDL_PREFIXES`), returns a `ParseError`
/// suggesting the correct prefix. Returns `None` if no near-miss is found.
#[must_use]
pub fn detect_near_miss(query: &str) -> Option<ParseError> {
    // PA-7: comment-blind near-miss detection.
    let blanked = crate::util::blank_sql_comments(query);
    let query = blanked.as_ref();
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
