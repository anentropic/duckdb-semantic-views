//! DIMENSIONS / FACTS qualified-entry parsing.
//!
//! §6.1 (phase 3, code-review 2026-07-11): the structural `alias.name AS`
//! prefix is parsed on the shared [`Cursor`]/lexer — the qualifier `.` is the
//! first `.` SYMBOL token (quote-aware: a dot inside a quoted `"a.b"` is inert,
//! PA-6) and `AS` is the first keyword token after it. The leading access
//! modifier, the unterminated-quote guard, and the trailing COMMENT / WITH
//! SYNONYMS region continue to use their shared helpers (the annotation tail is
//! handed the post-`AS` source verbatim, as TABLES does).

use super::annotations::{parse_leading_access_modifier, parse_trailing_annotations};
use super::cursor::Cursor;
use super::scan::unterminated_quote_error;
use super::{split_at_depth0_commas, ParsedQualifiedEntry};
use crate::errors::ParseError;
use crate::model::AccessModifier;

/// Parse the content inside DIMENSIONS or FACTS (...).
/// Returns one [`ParsedQualifiedEntry`] per entry.
///
/// Each entry has the form: `[PRIVATE|PUBLIC] alias.name AS sql_expression [COMMENT = '...'] [WITH SYNONYMS = ('...')]`
///
/// `allow_access_modifier`: if false, PRIVATE/PUBLIC keywords produce a `ParseError` (used for DIMENSIONS).
/// `clause_name`: human-readable name for error messages ("dimensions" or "facts").
pub(crate) fn parse_qualified_entries(
    body: &str,
    base_offset: usize,
    allow_access_modifier: bool,
    clause_name: &str,
) -> Result<Vec<ParsedQualifiedEntry>, ParseError> {
    if body.trim().is_empty() {
        return Ok(vec![]);
    }

    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let parsed =
            parse_single_qualified_entry(entry, entry_offset, allow_access_modifier, clause_name)?;
        result.push(parsed);
    }

    Ok(result)
}

/// Parse one DIMENSIONS/FACTS entry: `[PRIVATE|PUBLIC] alias.bare_name AS expr [COMMENT = '...'] [WITH SYNONYMS = ('...')]`
fn parse_single_qualified_entry(
    entry: &str,
    entry_offset: usize,
    allow_access_modifier: bool,
    clause_name: &str,
) -> Result<ParsedQualifiedEntry, ParseError> {
    let entry = entry.trim();

    // Unterminated quoting swallows the rest of the entry under the
    // quote-aware scanners — reject it up front with a precise error.
    if let Some(noun) = unterminated_quote_error(entry) {
        return Err(ParseError {
            message: format!("{noun} in {clause_name} entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Phase 43: Check for leading PRIVATE/PUBLIC keyword
    let (access, entry_after_access) = parse_leading_access_modifier(entry);
    if access == AccessModifier::Private && !allow_access_modifier {
        return Err(ParseError {
            message: format!(
                "PRIVATE is not supported on {clause_name}. Only facts and metrics can have access modifiers."
            ),
            position: Some(entry_offset),
        });
    }
    // Also reject explicit PUBLIC on dimensions (for consistency)
    if !allow_access_modifier
        && entry_after_access.len() != entry.trim().len()
        && access == AccessModifier::Public
    {
        // Check if it was an explicit PUBLIC keyword (entry was modified)
        let entry_trimmed = entry.trim();
        let entry_upper = entry_trimmed.to_ascii_uppercase();
        if entry_upper.starts_with("PUBLIC") {
            let after = &entry_trimmed["PUBLIC".len()..];
            if after.starts_with(|c: char| c.is_ascii_whitespace())
                && !after.trim_start().starts_with('.')
            {
                return Err(ParseError {
                    message: format!(
                        "PUBLIC is not supported on {clause_name}. Only facts and metrics can have access modifiers."
                    ),
                    position: Some(entry_offset),
                });
            }
        }
    }

    // The cursor spans the post-access-modifier text. Its base is `entry_offset`
    // (not the access-modifier offset within `entry`) so error carets reproduce
    // the pre-migration formula exactly.
    let mut cur = Cursor::new(entry_after_access, entry_offset);

    // Split `alias.name` at the first `.` SYMBOL token — quote-aware (PA-6): a
    // dot inside a quoted name (`"a.b"`) is part of that one token, not a
    // qualifier separator. `name` keeps the source-slice form because it may
    // itself contain dots (`o.x.y` → alias `o`, name `x.y`).
    let Some(dot_tok) = cur.find_symbol(b'.') else {
        return Err(cur.err(
            0,
            format!(
                "Expected 'alias.name' qualified identifier, got '{entry}'. Each dimension/metric entry must have the form 'alias.name AS expr'.",
            ),
        ));
    };
    let source_alias = entry_after_access[..dot_tok.start].trim().to_string();
    if source_alias.is_empty() {
        return Err(cur.err(
            0,
            format!("Source alias before '.' is empty in entry '{entry}'."),
        ));
    }
    cur.advance_past_byte(dot_tok.end);

    // `AS` is the first keyword token after the qualifier dot; `name` is the
    // text between them.
    let Some(as_tok) = cur.find_kw("AS") else {
        return Err(cur.err(
            dot_tok.end,
            format!(
                "Expected 'AS' keyword in dimension/metric entry '{entry}'. Form: 'alias.name AS expr'.",
            ),
        ));
    };
    let bare_name = entry_after_access[dot_tok.end..as_tok.start]
        .trim()
        .to_string();
    if bare_name.is_empty() {
        return Err(cur.err(
            dot_tok.end,
            format!("Missing bare name between '.' and 'AS' in entry '{entry}'."),
        ));
    }

    let raw_expr = entry_after_access[as_tok.end..].trim();
    if raw_expr.is_empty() {
        return Err(cur.err(
            as_tok.end,
            format!("Missing expression after 'AS' in entry '{entry}'."),
        ));
    }

    // Phase 43: Parse trailing annotations from expression
    let (expr, annotations) = parse_trailing_annotations(raw_expr)?;

    Ok(ParsedQualifiedEntry {
        source_alias,
        name: bare_name,
        expr,
        comment: annotations.comment,
        synonyms: annotations.synonyms,
        access,
    })
}
