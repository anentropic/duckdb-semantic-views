//! MATERIALIZATIONS clause parsing.
//!
//! §6.1 (phase 6, code-review 2026-07-11): the entry structure — the `name`,
//! the `AS`, the parenthesized sub-body, and the depth-0 TABLE / DIMENSIONS /
//! METRICS keyword scan — is parsed on the shared [`Cursor`]/lexer. Keyword,
//! `AS`-boundary, and paren detection are now quote- and depth-aware by
//! construction: a `TABLE`/`METRICS`/`(`/`)` inside a `"quoted"` / `'string'`
//! token is inert, a keyword-like name nested inside a DIMENSIONS/METRICS list
//! (depth > 0) does not split the sub-body, and `ASx` is a single ident token
//! (never the `AS` keyword). This replaces the `split_first_token` /
//! `extract_paren_content` / local `find_sub_keyword_positions` byte scans. The
//! comma-split into entries and into per-list names is delegated to the shared
//! [`split_at_depth0_commas`] unchanged.

use super::cursor::Cursor;
use super::lexer::TokenKind;
use super::split_at_depth0_commas;
use crate::errors::ParseError;
use crate::model::Materialization;

/// Parse the content inside MATERIALIZATIONS (...).
///
/// Each entry has the form: `name AS (TABLE table_name, DIMENSIONS (d1, d2), METRICS (m1, m2))`
/// At least one of DIMENSIONS or METRICS must be present in each entry.
pub(crate) fn parse_materializations_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<Materialization>, ParseError> {
    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();
    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let mat = parse_single_materialization_entry(entry, entry_offset)?;
        result.push(mat);
    }
    Ok(result)
}

/// Parse a single MATERIALIZATIONS entry: `name AS (TABLE t, DIMENSIONS (...), METRICS (...))`.
fn parse_single_materialization_entry(
    entry: &str,
    entry_offset: usize,
) -> Result<Materialization, ParseError> {
    let entry = entry.trim();
    let mut cur = Cursor::new(entry, entry_offset);

    // The name is the first token, which must be a value token — a bare or
    // quoted identifier, or a string literal (`peek_is_value`) — rather than
    // punctuation. This mirrors the retired first-whitespace scan, which also
    // took a leading `'string'` verbatim as the name; only a leading symbol is
    // rejected here as "missing name". A quoted name keeps its quotes and may
    // contain whitespace (`"my mat"`), since it is one token now rather than a
    // first-whitespace split.
    if !cur.peek_is_value() {
        let message = if cur.peek().is_none() {
            "Empty entry in MATERIALIZATIONS clause.".to_string()
        } else {
            "Expected materialization name in MATERIALIZATIONS entry.".to_string()
        };
        return Err(ParseError {
            message,
            position: Some(entry_offset),
        });
    }
    let name_tok = cur.bump().expect("peek_is_value guaranteed a token");

    // An unterminated `"..."` / `'...'` lexes as a single token spanning the
    // rest of the entry, so without this guard it would be swallowed as the
    // `name` and surface a misleading "Expected 'AS' after name '<whole entry>'"
    // error. Reject it up front — matching how every sibling clause (TABLES via
    // the token kind, entries/metrics via `unterminated_quote_error`) handles an
    // orphan quote (PR #105 review).
    if let TokenKind::Unterminated { ident } = name_tok.kind {
        let noun = if ident {
            "Unterminated quoted identifier"
        } else {
            "Unterminated string literal"
        };
        return Err(cur.err(
            name_tok.start,
            format!("{noun} in materialization entry '{entry}'."),
        ));
    }
    let name = cur.text(name_tok);

    // Expect `AS` immediately after the name (only whitespace between the two
    // tokens). `AS(...)` is legal — `(` is a separate token — while `ASx` is a
    // single `ASx` ident token that is not the `AS` keyword (PR #50 review).
    let as_ok = cur.peek().is_some_and(|t| cur.is_kw(t, "AS"));
    if !as_ok {
        return Err(cur.err(
            name_tok.end,
            format!(
                "Expected 'AS' after materialization name '{name}' in MATERIALIZATIONS clause."
            ),
        ));
    }
    cur.bump(); // consume AS

    // Expect the parenthesized sub-body: (TABLE ..., DIMENSIONS (...), METRICS (...)).
    if !cur.peek_is_symbol(b'(') {
        return Err(ParseError {
            message: format!("Expected '(' after 'AS' for materialization '{name}'."),
            position: None,
        });
    }
    // A `)` inside a quoted identifier or string is part of that one token, so
    // it cannot close the sub-body early (PA-6).
    let Some(sub_body) = cur.take_parens() else {
        return Err(ParseError {
            message: format!("Unclosed '(' for materialization '{name}'."),
            position: None,
        });
    };

    let (table, dim_names, met_names) = parse_materialization_sub_body(name, sub_body)?;

    Ok(Materialization {
        name: name.to_string(),
        table,
        dimensions: dim_names,
        metrics: met_names,
    })
}

/// Parse the `(TABLE ..., DIMENSIONS (...), METRICS (...))` sub-body of one
/// materialization entry into `(table, dimensions, metrics)`.
///
/// Locates the TABLE / DIMENSIONS / METRICS sub-keywords at depth 0 (outside
/// any nested `(...)` list) and outside quotes, in order. Each keyword's
/// content runs from just past it to the start of the next keyword (or the end
/// of the sub-body) — the same tiling the retired `find_sub_keyword_positions`
/// produced, now quote-aware by construction.
fn parse_materialization_sub_body(
    name: &str,
    sub_body: &str,
) -> Result<(String, Vec<String>, Vec<String>), ParseError> {
    let sub = Cursor::new(sub_body, 0);
    let kw_toks = sub.find_all_kw_depth0(&["TABLE", "DIMENSIONS", "METRICS"]);

    // F-4 (code-review 2026-07-16): reject text before the first sub-keyword.
    // The keyword tiling only examines content *after* each keyword, so a
    // leading `(junk TABLE t, ...)` previously discarded `junk` silently.
    if let Some(first) = kw_toks.first() {
        let before = sub_body[..first.start].trim();
        if !before.is_empty() {
            return Err(ParseError {
                message: format!(
                    "Materialization '{name}': unexpected text '{before}' before the first sub-clause."
                ),
                position: None,
            });
        }
    }

    let mut table_name: Option<String> = None;
    let mut dim_names: Vec<String> = Vec::new();
    let mut met_names: Vec<String> = Vec::new();
    // F-4: a repeated sub-keyword previously overwrote silently (last wins).
    let (mut dim_seen, mut met_seen) = (false, false);

    for (i, &kw_tok) in kw_toks.iter().enumerate() {
        let end = if i + 1 < kw_toks.len() {
            kw_toks[i + 1].start
        } else {
            sub_body.len()
        };
        let content = sub_body[kw_tok.end..end].trim();
        // Strip a single trailing comma (the separator to the next sub-clause).
        let content = content.strip_suffix(',').unwrap_or(content).trim();

        if sub.is_kw(kw_tok, "TABLE") {
            if table_name.is_some() {
                return Err(duplicate_sub_clause(name, "TABLE"));
            }
            if content.is_empty() {
                return Err(ParseError {
                    message: format!(
                        "Materialization '{name}': TABLE sub-clause has no table name."
                    ),
                    position: None,
                });
            }
            table_name = Some(content.to_string());
        } else if sub.is_kw(kw_tok, "DIMENSIONS") {
            if dim_seen {
                return Err(duplicate_sub_clause(name, "DIMENSIONS"));
            }
            dim_seen = true;
            dim_names = extract_paren_list(content)?;
        } else if sub.is_kw(kw_tok, "METRICS") {
            if met_seen {
                return Err(duplicate_sub_clause(name, "METRICS"));
            }
            met_seen = true;
            met_names = extract_paren_list(content)?;
        }
    }

    let table = table_name.ok_or_else(|| ParseError {
        message: format!("Materialization '{name}': missing TABLE sub-clause."),
        position: None,
    })?;

    if dim_names.is_empty() && met_names.is_empty() {
        return Err(ParseError {
            message: format!(
                "Materialization '{name}': must specify at least one of DIMENSIONS or METRICS."
            ),
            position: None,
        });
    }

    Ok((table, dim_names, met_names))
}

/// Build the "duplicate sub-clause" error for a repeated TABLE / DIMENSIONS /
/// METRICS keyword within one materialization entry (F-4).
fn duplicate_sub_clause(name: &str, kw: &str) -> ParseError {
    ParseError {
        message: format!("Materialization '{name}': duplicate {kw} sub-clause."),
        position: None,
    }
}

/// Extract a parenthesized comma-separated name list: `(name1, name2, ...)`.
/// Strips whitespace from each name. A bare, unparenthesized `content` is
/// treated as a single-element list (unchanged tolerance from the pre-cursor
/// scanner).
fn extract_paren_list(content: &str) -> Result<Vec<String>, ParseError> {
    let content = content.trim();
    if content.is_empty() {
        return Ok(Vec::new());
    }
    let inner = if content.starts_with('(') {
        // Quote-aware balanced `(...)`: a `)` inside a string / quoted ident
        // cannot close the list early (PA-6).
        let mut c = Cursor::new(content, 0);
        let inner = c.take_parens().ok_or_else(|| ParseError {
            message: "Unclosed parenthesis in MATERIALIZATIONS sub-clause.".to_string(),
            position: None,
        })?;
        // F-4 (code-review 2026-07-16): reject trailing text after the `(...)`
        // list (`DIMENSIONS (d) junk`) instead of silently dropping it.
        if let Some(tok) = c.peek() {
            let residue = content[tok.start..].trim();
            return Err(ParseError {
                message: format!(
                    "Unexpected text '{residue}' after a MATERIALIZATIONS sub-clause list."
                ),
                position: None,
            });
        }
        inner
    } else {
        content
    };
    Ok(split_at_depth0_commas(inner)
        .into_iter()
        .map(|(_, entry)| entry.to_string())
        .collect())
}
