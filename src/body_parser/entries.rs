//! DIMENSIONS / FACTS qualified-entry parsing.

use super::annotations::{parse_leading_access_modifier, parse_trailing_annotations};
use super::scan::{find_keyword_ci, find_live_byte, unterminated_quote_error};
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

    // Find first live '.' to split alias.bare_name — quote-aware (PA-6):
    // a dot inside a quoted name (`"a.b"`) is not a qualifier separator.
    let dot_pos = find_live_byte(entry_after_access, b'.').ok_or_else(|| ParseError {
        message: format!(
            "Expected 'alias.name' qualified identifier, got '{entry}'. Each dimension/metric entry must have the form 'alias.name AS expr'.",
        ),
        position: Some(entry_offset),
    })?;

    let source_alias = entry_after_access[..dot_pos].trim().to_string();
    if source_alias.is_empty() {
        return Err(ParseError {
            message: format!("Source alias before '.' is empty in entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Everything from dot+1 forward; find "AS" (case-insensitive, word boundary)
    let after_dot = &entry_after_access[dot_pos + 1..];
    let upper_after = after_dot.to_ascii_uppercase();
    let as_pos = find_keyword_ci(&upper_after, "AS").ok_or_else(|| ParseError {
        message: format!(
            "Expected 'AS' keyword in dimension/metric entry '{entry}'. Form: 'alias.name AS expr'.",
        ),
        position: Some(entry_offset + dot_pos + 1),
    })?;

    let bare_name = after_dot[..as_pos].trim().to_string();
    if bare_name.is_empty() {
        return Err(ParseError {
            message: format!("Missing bare name between '.' and 'AS' in entry '{entry}'."),
            position: Some(entry_offset + dot_pos + 1),
        });
    }

    let raw_expr = after_dot[as_pos + 2..].trim();
    if raw_expr.is_empty() {
        return Err(ParseError {
            message: format!("Missing expression after 'AS' in entry '{entry}'."),
            position: Some(entry_offset + dot_pos + 1 + as_pos + 2),
        });
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
