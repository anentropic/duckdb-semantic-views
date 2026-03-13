// Parse detection and rewriting for semantic view DDL statements.
//
// This module provides two layers:
// 1. Pure detection/rewrite functions (`detect_semantic_view_ddl`, `rewrite_ddl`,
//    `extract_ddl_name`, `validate_and_rewrite`) testable under `cargo test`
//    without the extension feature.
// 2. FFI entry points (`sv_validate_ddl_rust`, `sv_rewrite_ddl_rust`)
//    feature-gated on `extension`, with `catch_unwind` for panic safety.

use crate::body_parser::parse_keyword_body;

/// Not our statement -- return `DISPLAY_ORIGINAL_ERROR`.
pub const PARSE_NOT_OURS: u8 = 0;
/// Detected a semantic view DDL statement -- return `PARSE_SUCCESSFUL`.
pub const PARSE_DETECTED: u8 = 1;

// ---------------------------------------------------------------------------
// DdlKind enum and detection
// ---------------------------------------------------------------------------

/// The 7 supported DDL statement forms for semantic views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdlKind {
    Create,
    CreateOrReplace,
    CreateIfNotExists,
    Drop,
    DropIfExists,
    Describe,
    Show,
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
    // DESCRIBE SEMANTIC VIEW (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"describe", b"semantic", b"view"]) {
        return Some((DdlKind::Describe, n));
    }
    // SHOW SEMANTIC VIEWS (3 keywords)
    if let Some(n) = match_keyword_prefix(b, &[b"show", b"semantic", b"views"]) {
        return Some((DdlKind::Show, n));
    }

    None
}

/// Detect the DDL kind from a query string.
///
/// Returns `Some(DdlKind)` if the query matches one of the 7 semantic view
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
/// Returns `PARSE_DETECTED` for all 7 DDL forms, `PARSE_NOT_OURS` otherwise.
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

/// Parse a CREATE-with-body DDL statement, extracting the view name and body.
///
/// `prefix` is the DDL prefix (e.g. "create semantic view") already known to
/// match. Returns `(name, body)` where `body` is everything between the first
/// `(` and last `)` after the name.
fn parse_create_body(trimmed: &str, prefix_len: usize) -> Result<(&str, &str), String> {
    let after_prefix = &trimmed[prefix_len..];
    let after_prefix = after_prefix.trim_start();
    if after_prefix.is_empty() {
        return Err("Missing view name".to_string());
    }

    // View name ends at whitespace or '('
    let name_end = after_prefix
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(after_prefix.len());
    let name = &after_prefix[..name_end];
    if name.is_empty() {
        return Err("Missing view name".to_string());
    }

    // Find the opening paren
    let after_name = &after_prefix[name_end..];
    let open_paren_offset = after_name
        .find('(')
        .ok_or_else(|| "Expected '(' after view name".to_string())?;

    // Find the body: everything between first '(' and last ')' in the remaining text
    let from_open = &after_name[open_paren_offset..];
    let close_paren = from_open
        .rfind(')')
        .ok_or_else(|| "Expected ')' to close DDL body".to_string())?;

    // Body is between '(' and ')' (exclusive of both)
    let body = &from_open[1..close_paren];
    Ok((name, body))
}

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

/// Parse a `CREATE SEMANTIC VIEW` DDL statement, extracting the view name and body.
///
/// This is the original function, now delegating to `parse_create_body`.
/// Kept for backward compatibility with existing code paths.
pub fn parse_ddl_text(query: &str) -> Result<(&str, &str), String> {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();

    let (kind, prefix_len) = detect_ddl_prefix(trimmed)
        .ok_or_else(|| "Not a CREATE SEMANTIC VIEW statement".to_string())?;

    if !matches!(
        kind,
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists
    ) {
        return Err("Not a CREATE SEMANTIC VIEW statement".to_string());
    }

    parse_create_body(trimmed, prefix_len)
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
    }
}

/// Rewrite any semantic view DDL statement to its corresponding function call.
///
/// Dispatches to the appropriate rewrite based on the 3 parsing categories:
/// - CREATE-with-body: `SELECT * FROM fn('name', body)`
/// - Name-only (DROP, DESCRIBE): `SELECT * FROM fn('name')`
/// - No-args (SHOW): `SELECT * FROM list_semantic_views()`
pub fn rewrite_ddl(query: &str) -> Result<String, String> {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();

    let (kind, plen) = detect_ddl_prefix(trimmed)
        .ok_or_else(|| "Not a semantic view DDL statement".to_string())?;

    let fn_name = function_name(kind);

    match kind {
        // CREATE-with-body forms
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists => {
            let (name, body) = parse_create_body(trimmed, plen)?;
            let safe_name = name.replace('\'', "''");
            Ok(format!("SELECT * FROM {fn_name}('{safe_name}', {body})"))
        }
        // Name-only forms
        DdlKind::Drop | DdlKind::DropIfExists | DdlKind::Describe => {
            let name = extract_name_only(trimmed, plen)?;
            let safe_name = name.replace('\'', "''");
            Ok(format!("SELECT * FROM {fn_name}('{safe_name}')"))
        }
        // No-args form
        DdlKind::Show => Ok(format!("SELECT * FROM {fn_name}()")),
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
            let (name, _) = parse_create_body(trimmed, plen)?;
            Ok(Some(name.to_string()))
        }
        DdlKind::Drop | DdlKind::DropIfExists | DdlKind::Describe => {
            let name = extract_name_only(trimmed, plen)?;
            Ok(Some(name))
        }
        DdlKind::Show => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Validation layer: ParseError, validate_clauses, detect_near_miss,
//                   validate_and_rewrite
// ---------------------------------------------------------------------------

/// Error from DDL validation with an optional byte offset into the original query.
///
/// The `position` field, when present, is a 0-based byte offset into the
/// original query string (before any trimming). `DuckDB` uses this to render
/// a caret (`^`) under the error location.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the original query string.
    pub position: Option<usize>,
}

/// Known clause keywords for a CREATE SEMANTIC VIEW body.
const CLAUSE_KEYWORDS: &[&str] = &["tables", "relationships", "dimensions", "metrics"];

/// The 7 DDL prefixes used for near-miss detection.
const DDL_PREFIXES: &[&str] = &[
    "create semantic view",
    "create or replace semantic view",
    "create semantic view if not exists",
    "drop semantic view",
    "drop semantic view if exists",
    "describe semantic view",
    "show semantic views",
];

/// Suggest the closest clause keyword using Levenshtein distance.
///
/// Returns `Some(keyword)` if a keyword is within edit distance <= 3 of `word`.
fn suggest_clause_keyword(word: &str) -> Option<&'static str> {
    let lower = word.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for &kw in CLAUSE_KEYWORDS {
        let dist = strsim::levenshtein(&lower, kw);
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, kw));
                }
            } else {
                best = Some((dist, kw));
            }
        }
    }
    best.map(|(_, kw)| kw)
}

/// Return the matching closing bracket for an opening one.
fn matching_close(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => '?',
    }
}

/// Check a closing bracket against the top of the stack, returning an error
/// on mismatch.
fn check_close_bracket(
    expected_open: char,
    close_char: char,
    paren_stack: &mut Vec<(char, usize)>,
    pos: usize,
) -> Result<(), ParseError> {
    if let Some((open, _)) = paren_stack.last() {
        if *open == expected_open {
            paren_stack.pop();
        } else {
            return Err(ParseError {
                message: format!(
                    "Unbalanced bracket: expected closing '{}' but found '{close_char}'.",
                    matching_close(*open)
                ),
                position: Some(pos),
            });
        }
    }
    Ok(())
}

/// Validate balanced brackets/parentheses within a body string,
/// respecting single-quoted string literals.
fn validate_brackets(body: &str, body_offset: usize) -> Result<(), ParseError> {
    let mut paren_stack: Vec<(char, usize)> = Vec::new();
    let mut in_string = false;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            if in_string && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            in_string = !in_string;
        } else if !in_string {
            match ch {
                '(' | '[' | '{' => paren_stack.push((ch, body_offset + i)),
                ')' => check_close_bracket('(', ')', &mut paren_stack, body_offset + i)?,
                ']' => check_close_bracket('[', ']', &mut paren_stack, body_offset + i)?,
                '}' => check_close_bracket('{', '}', &mut paren_stack, body_offset + i)?,
                _ => {}
            }
        }
        i += 1;
    }
    if let Some((open_char, open_pos)) = paren_stack.last() {
        return Err(ParseError {
            message: format!("Unbalanced bracket: '{open_char}' opened but never closed."),
            position: Some(*open_pos),
        });
    }
    Ok(())
}

/// Scan for clause keywords at the top level of the body (recognized by ':=' or '(' after the word).
/// Validates them against known clause keywords (outside strings and nested brackets).
fn scan_clause_keywords(body: &str, body_offset: usize) -> Result<Vec<String>, ParseError> {
    let mut found_clauses: Vec<String> = Vec::new();
    let mut in_string = false;
    let mut depth: i32 = 0;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            if in_string && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string {
            i += 1;
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && ch.is_ascii_alphabetic() {
            let word_start = i;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_')
            {
                i += 1;
            }
            let word = &body[word_start..i];
            let after_word = &body[i..];
            let after_trimmed = after_word.trim_start();
            if after_trimmed.starts_with(":=") || after_trimmed.starts_with('(') {
                let lower = word.to_ascii_lowercase();
                if CLAUSE_KEYWORDS.iter().any(|&kw| kw == lower) {
                    found_clauses.push(lower);
                } else {
                    let msg = if let Some(kw) = suggest_clause_keyword(word) {
                        format!("Unknown clause '{word}'. Did you mean '{kw}'?")
                    } else {
                        format!("Unknown clause '{word}'. Expected one of: tables, relationships, dimensions, metrics.")
                    };
                    return Err(ParseError {
                        message: msg,
                        position: Some(body_offset + word_start),
                    });
                }
            }
            continue;
        }
        i += 1;
    }
    Ok(found_clauses)
}

/// Validate clause definitions inside a CREATE body.
///
/// Checks for:
/// 1. Empty body
/// 2. Unbalanced brackets/parentheses
/// 3. Unknown clause keywords (with "Did you mean" suggestions)
/// 4. Missing required clauses (`tables` must be present; at least one of
///    `dimensions` or `metrics`)
///
/// `body_offset` is the byte offset of the body start within the original query.
pub fn validate_clauses(
    body: &str,
    body_offset: usize,
    _original_query: &str,
) -> Result<(), ParseError> {
    let trimmed_body = body.trim();

    if trimmed_body.is_empty() {
        return Err(ParseError {
            message: "Expected clause definitions (tables, dimensions, metrics). Body is empty."
                .to_string(),
            position: Some(body_offset),
        });
    }

    validate_brackets(body, body_offset)?;
    let found_clauses = scan_clause_keywords(body, body_offset)?;

    let has_tables = found_clauses.iter().any(|c| c == "tables");
    let has_dims = found_clauses.iter().any(|c| c == "dimensions");
    let has_metrics = found_clauses.iter().any(|c| c == "metrics");

    if !has_tables {
        return Err(ParseError {
            message: "Missing required clause 'tables'.".to_string(),
            position: Some(body_offset),
        });
    }
    if !has_dims && !has_metrics {
        return Err(ParseError {
            message:
                "Missing required clause: at least one of 'dimensions' or 'metrics' must be specified."
                    .to_string(),
            position: Some(body_offset),
        });
    }

    Ok(())
}

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
/// This is the main entry point for the validation layer. It wraps `rewrite_ddl()`
/// with structural validation that catches errors before rewriting.
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
        // No-args form: no validation needed
        DdlKind::Show => rewrite_ddl(query).map(Some).map_err(|e| ParseError {
            message: e,
            position: None,
        }),
    }
}

/// Validate a CREATE-with-body DDL statement and rewrite it if valid.
fn validate_create_body(
    query: &str,
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

    // --- AS keyword body path (new in Phase 25) ---
    // If text after the name starts with "AS" (whitespace-delimited), route to the
    // AS-body keyword parser instead of the legacy paren-body path.
    let after_name_trimmed = after_name.trim_start();
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
        // after_name_trimmed trims leading whitespace from after_name
        let as_trim_gap = after_name.len() - after_name_trimmed.len();
        let body_offset_in_tns = after_name_in_tns + as_trim_gap;
        let body_offset = trim_offset + body_offset_in_tns;
        return rewrite_ddl_keyword_body(kind, name, after_name_trimmed, body_offset);
    }
    // --- End AS keyword body path ---

    let Some(open_paren_rel) = after_name.find('(') else {
        let pos_in_trimmed = plen + (trimmed_no_semi.len() - plen - after_prefix.len()) + name_end;
        return Err(ParseError {
            message: "Expected '(' after view name.".to_string(),
            position: Some(trim_offset + pos_in_trimmed),
        });
    };

    let after_prefix_offset_in_trimmed = plen + (trimmed_no_semi.len() - plen - after_prefix.len());
    let open_paren_in_trimmed = after_prefix_offset_in_trimmed + name_end + open_paren_rel;

    let from_open = &after_name[open_paren_rel..];
    let Some(close_paren) = from_open.rfind(')') else {
        return Err(ParseError {
            message: "Expected ')' to close DDL body.".to_string(),
            position: Some(trim_offset + open_paren_in_trimmed),
        });
    };
    let body = &from_open[1..close_paren];
    let body_offset = trim_offset + open_paren_in_trimmed + 1;

    validate_clauses(body, body_offset, query)?;

    rewrite_ddl(query).map(Some).map_err(|e| ParseError {
        message: e,
        position: None,
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
    body_text: &str,    // text starting at "AS" (inclusive)
    body_offset: usize, // byte offset of body_text[0] in original query
) -> Result<Option<String>, ParseError> {
    // 1. Call parse_keyword_body (body_text starts at "AS"; pass body_offset)
    let keyword_body = parse_keyword_body(body_text, body_offset)?;

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
        filters: vec![],
        facts: vec![],
        column_type_names: vec![],
        column_types_inferred: vec![],
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

            // Use validate_and_rewrite so both paren-body and AS-body DDL are handled.
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
    // rewrite_ddl tests
    // ===================================================================

    #[test]
    fn test_rewrite_create() {
        let sql = rewrite_ddl("CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])")
            .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM create_semantic_view('sales', tables := [...], dimensions := [...])"
        );
    }

    #[test]
    fn test_rewrite_create_or_replace() {
        let sql = rewrite_ddl(
            "CREATE OR REPLACE SEMANTIC VIEW sales (tables := [...], dimensions := [...])",
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM create_or_replace_semantic_view('sales', tables := [...], dimensions := [...])"
        );
    }

    #[test]
    fn test_rewrite_create_if_not_exists() {
        let sql = rewrite_ddl(
            "CREATE SEMANTIC VIEW IF NOT EXISTS sales (tables := [...], dimensions := [...])",
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM create_semantic_view_if_not_exists('sales', tables := [...], dimensions := [...])"
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

    // -----------------------------------------------------------------------
    // parse_ddl_text tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ddl_basic() {
        let (name, body) = parse_ddl_text("CREATE SEMANTIC VIEW sales (tables := [...])").unwrap();
        assert_eq!(name, "sales");
        assert_eq!(body, "tables := [...]");
    }

    #[test]
    fn test_parse_ddl_case_insensitive() {
        let (name, body) = parse_ddl_text("create semantic view My_View (a := 1)").unwrap();
        assert_eq!(name, "My_View");
        assert_eq!(body, "a := 1");
    }

    #[test]
    fn test_parse_ddl_whitespace_and_semicolon() {
        let (name, body) = parse_ddl_text("  CREATE SEMANTIC VIEW x (a := 1);").unwrap();
        assert_eq!(name, "x");
        assert_eq!(body, "a := 1");
    }

    #[test]
    fn test_parse_ddl_not_our_statement() {
        let err = parse_ddl_text("SELECT 1").unwrap_err();
        assert!(err.contains("Not a CREATE SEMANTIC VIEW"), "got: {err}");
    }

    #[test]
    fn test_parse_ddl_missing_name() {
        let err = parse_ddl_text("CREATE SEMANTIC VIEW").unwrap_err();
        assert!(err.contains("Missing view name"), "got: {err}");
    }

    #[test]
    fn test_parse_ddl_missing_parens() {
        let err = parse_ddl_text("CREATE SEMANTIC VIEW x").unwrap_err();
        assert!(err.contains("Expected '(' after view name"), "got: {err}");
    }

    #[test]
    fn test_parse_ddl_nested_parens() {
        let (name, body) = parse_ddl_text(
            "CREATE SEMANTIC VIEW v (tables := [{alias: 'a', table: 't'}], dimensions := [{name: 'x', expr: 'CAST(y AS INT)', source_table: 'a'}])"
        ).unwrap();
        assert_eq!(name, "v");
        assert!(body.starts_with("tables := ["));
        assert!(body.ends_with("'a'}]"));
    }

    #[test]
    fn test_parse_ddl_name_with_paren_adjacent() {
        let (name, body) = parse_ddl_text("CREATE SEMANTIC VIEW myview(a := 1)").unwrap();
        assert_eq!(name, "myview");
        assert_eq!(body, "a := 1");
    }

    // -----------------------------------------------------------------------
    // Additional rewrite_ddl coverage (legacy test cases)
    // -----------------------------------------------------------------------

    #[test]
    fn test_rewrite_basic() {
        let sql = rewrite_ddl("CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])")
            .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM create_semantic_view('sales', tables := [...], dimensions := [...])"
        );
    }

    #[test]
    fn test_rewrite_escapes_single_quotes() {
        let sql = rewrite_ddl("CREATE SEMANTIC VIEW it's_a_view (tables := [])").unwrap();
        assert!(sql.contains("'it''s_a_view'"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_preserves_body() {
        let sql = rewrite_ddl(
            "CREATE SEMANTIC VIEW v (tables := [{alias: 'sales', table: 'sales'}], dimensions := [{name: 'region', expr: 'region', source_table: 'sales'}], metrics := [{name: 'total', expr: 'SUM(amount)', source_table: 'sales'}])",
        )
        .unwrap();
        assert!(sql.starts_with("SELECT * FROM create_semantic_view('v', tables := ["));
        assert!(sql.contains("metrics := [{name: 'total'"));
    }

    #[test]
    fn test_rewrite_error_propagation() {
        let err = rewrite_ddl("SELECT 1").unwrap_err();
        assert!(err.contains("Not a"), "got: {err}");
    }

    // ===================================================================
    // validate_and_rewrite tests (all use ( syntax — no := syntax)
    // ===================================================================

    #[test]
    fn test_validate_and_rewrite_success() {
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW sales (tables ([{alias: 'a', table: 't'}]), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))"
        );
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(sql.is_some());
        assert!(sql
            .unwrap()
            .starts_with("SELECT * FROM create_semantic_view("));
    }

    #[test]
    fn test_validate_and_rewrite_not_ours() {
        let result = validate_and_rewrite("SELECT 1");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_validate_and_rewrite_empty_body() {
        let result = validate_and_rewrite("CREATE SEMANTIC VIEW x ()");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("tables")
                && err.message.contains("dimensions")
                && err.message.contains("metrics"),
            "Expected empty body error mentioning required clauses, got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_and_rewrite_missing_tables() {
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW x (dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("tables"),
            "Expected missing tables error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_and_rewrite_missing_dims_and_metrics() {
        let result =
            validate_and_rewrite("CREATE SEMANTIC VIEW x (tables ([{alias: 'a', table: 't'}]))");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("dimensions") || err.message.contains("metrics"),
            "Expected missing dimensions/metrics error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_and_rewrite_clause_typo() {
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW x (tbles ([{alias: 'a', table: 't'}]), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))"
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Did you mean") && err.message.contains("tables"),
            "Expected typo suggestion for 'tbles', got: {}",
            err.message
        );
        assert!(err.position.is_some(), "Expected position for clause typo");
    }

    #[test]
    fn test_validate_and_rewrite_unbalanced_brackets() {
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW x (tables ([{alias: 'a', table: 't'}), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))"
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("bracket") || err.message.contains("Unbalanced"),
            "Expected bracket error, got: {}",
            err.message
        );
        assert!(
            err.position.is_some(),
            "Expected position for bracket mismatch"
        );
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

    // ===================================================================
    // validate_and_rewrite coverage gap tests
    // ===================================================================

    #[test]
    fn test_validate_and_rewrite_create_or_replace() {
        let result = validate_and_rewrite(
            "CREATE OR REPLACE SEMANTIC VIEW sv1 (tables ([{alias: 'a', table: 't'}]), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))"
        );
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(sql.is_some(), "Expected Some(rewritten SQL)");
    }

    #[test]
    fn test_validate_and_rewrite_create_if_not_exists() {
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW IF NOT EXISTS sv1 (tables ([{alias: 'a', table: 't'}]), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))"
        );
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(sql.is_some(), "Expected Some(rewritten SQL)");
    }

    #[test]
    fn test_validate_and_rewrite_relationships_clause() {
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW sv1 (tables ([{alias: 'a', table: 't'}]), relationships ([{from: 'a', to: 'b', on: 'a.id = b.id'}]), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))"
        );
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(
            sql.is_some(),
            "Expected Some(rewritten SQL) with relationships clause"
        );
    }

    #[test]
    fn test_validate_and_rewrite_tables_and_metrics_only() {
        // No dimensions, only tables + metrics -- should be valid
        let result = validate_and_rewrite(
            "CREATE SEMANTIC VIEW sv1 (tables ([{alias: 'a', table: 't'}]), metrics ([{name: 'total', expr: 'SUM(x)', source_table: 'a'}]))"
        );
        assert!(result.is_ok());
        let sql = result.unwrap();
        assert!(
            sql.is_some(),
            "Expected Some(rewritten SQL) with tables + metrics only"
        );
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
    // validate_clauses tests (all use ( syntax -- no := syntax)
    // ===================================================================

    #[test]
    fn test_validate_clauses_empty_body() {
        let query = "CREATE SEMANTIC VIEW x ()";
        let body_offset = query.find('(').unwrap() + 1;
        let body = "";
        let result = validate_clauses(body, body_offset, query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("tables")
                && err.message.contains("dimensions")
                && err.message.contains("metrics"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_clauses_unknown_keyword() {
        let query = "CREATE SEMANTIC VIEW x (tbles ([]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Did you mean") && err.message.contains("tables"),
            "got: {}",
            err.message
        );
        // Position should point to the start of "tbles" in the original query
        assert!(err.position.is_some());
        let pos = err.position.unwrap();
        assert_eq!(&query[pos..pos + 5], "tbles");
    }

    #[test]
    fn test_validate_clauses_missing_tables() {
        let query = "CREATE SEMANTIC VIEW x (dimensions ([]), metrics ([]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("tables"), "got: {}", err.message);
    }

    #[test]
    fn test_validate_clauses_missing_dims_and_metrics() {
        let query = "CREATE SEMANTIC VIEW x (tables ([]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("dimensions") || err.message.contains("metrics"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_clauses_valid() {
        let query = "CREATE SEMANTIC VIEW x (tables ([{alias: 'a', table: 't'}]), dimensions ([{name: 'x', expr: 'y', source_table: 'a'}]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(result.is_ok(), "got: {:?}", result.unwrap_err().message);
    }

    #[test]
    fn test_validate_clauses_unbalanced_brackets() {
        let query = "CREATE SEMANTIC VIEW x (tables ([{alias: 'a', table: 't'}), dimensions ([]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("bracket") || err.message.contains("Unbalanced"),
            "got: {}",
            err.message
        );
        assert!(err.position.is_some());
    }

    // ===================================================================
    // validate_clauses coverage gap tests
    // ===================================================================

    #[test]
    fn test_validate_clauses_unknown_keyword_far() {
        // "foobar" is far from any known keyword -- should get "Expected one of" not "Did you mean"
        let query = "CREATE SEMANTIC VIEW x (foobar ([]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Expected one of"),
            "Expected 'Expected one of' for unknown keyword far from any known, got: {}",
            err.message
        );
        assert!(
            !err.message.contains("Did you mean"),
            "Should NOT suggest 'Did you mean' for foobar, got: {}",
            err.message
        );
    }

    #[test]
    fn test_validate_clauses_case_insensitive() {
        // Uppercase clause keywords should be recognized
        let query = "CREATE SEMANTIC VIEW x (TABLES ([{alias: 'a', table: 't'}]), DIMENSIONS ([{name: 'x', expr: 'y', source_table: 'a'}]))";
        let body_start = query.find('(').unwrap() + 1;
        let body = &query[body_start..query.rfind(')').unwrap()];
        let result = validate_clauses(body, body_start, query);
        assert!(
            result.is_ok(),
            "Uppercase clause keywords should be accepted, got: {:?}",
            result.unwrap_err().message
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
    // ParseError position tests (all use ( syntax -- no := syntax)
    // ===================================================================

    #[test]
    fn test_parse_error_position_clause_typo() {
        // Position should point at the clause keyword in the original query
        let query = "CREATE SEMANTIC VIEW x (tbles ([]))";
        let result = validate_and_rewrite(query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.position.is_some());
        let pos = err.position.unwrap();
        // "tbles" should start at the position
        assert_eq!(
            &query[pos..pos + 5],
            "tbles",
            "Position {} doesn't point to 'tbles'",
            pos
        );
    }

    #[test]
    fn test_parse_error_position_with_leading_whitespace() {
        // Leading whitespace should be accounted for in position
        let query = "   CREATE SEMANTIC VIEW x (tbles ([]))";
        let result = validate_and_rewrite(query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.position.is_some());
        let pos = err.position.unwrap();
        assert_eq!(
            &query[pos..pos + 5],
            "tbles",
            "Position {} doesn't point to 'tbles' in query with leading whitespace",
            pos
        );
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
        fn old_paren_body_still_works() {
            let query = "CREATE SEMANTIC VIEW v (tables := [], dimensions := [])";
            let result = validate_and_rewrite(query);
            assert!(result.is_ok(), "Old paren path must still work: {result:?}");
        }

        #[test]
        fn drop_still_rewrites_unchanged() {
            let query = "DROP SEMANTIC VIEW v";
            let result = validate_and_rewrite(query).unwrap().unwrap();
            assert_eq!(result, "SELECT * FROM drop_semantic_view('v')");
        }
    }
}
