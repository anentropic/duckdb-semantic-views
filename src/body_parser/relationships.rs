//! RELATIONSHIPS clause parsing.

use super::scan::{extract_paren_content, find_keyword_ci, find_live_byte, split_first_token};
use super::split_at_depth0_commas;
use crate::errors::ParseError;
use crate::model::{Cardinality, Join};

/// Parse the content inside RELATIONSHIPS (...). Returns empty vec for empty body.
///
/// Each entry has the form:
///   `rel_name AS from_alias(fk_col1, fk_col2) REFERENCES to_alias`
pub(crate) fn parse_relationships_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<Join>, ParseError> {
    if body.trim().is_empty() {
        return Ok(vec![]);
    }

    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let join = parse_single_relationship_entry(entry, entry_offset)?;
        result.push(join);
    }

    Ok(result)
}

/// Parse one RELATIONSHIPS entry: `rel_name AS from_alias(fk_cols) REFERENCES to_alias[(ref_cols)]`
///
/// Phase 33: Cardinality keywords (MANY TO ONE, etc.) are no longer accepted.
/// Cardinality is inferred from PK/UNIQUE constraints at parse time.
/// Optional `REFERENCES target(col1, col2)` syntax stores explicit `ref_columns`.
#[allow(clippy::too_many_lines)]
fn parse_single_relationship_entry(entry: &str, entry_offset: usize) -> Result<Join, ParseError> {
    let entry = entry.trim();

    // Find "AS" keyword (case-insensitive) -- relationship name is before it
    let upper = entry.to_ascii_uppercase();
    let as_pos = find_keyword_ci(&upper, "AS").ok_or_else(|| ParseError {
        message: format!(
            "Missing relationship name: expected 'rel_name AS from_alias(fk_cols) REFERENCES to_alias', got '{entry}'.",
        ),
        position: Some(entry_offset),
    })?;

    let rel_name = entry[..as_pos].trim();
    if rel_name.is_empty() {
        return Err(ParseError {
            message: "Relationship name is required; found 'AS' without a preceding name."
                .to_string(),
            position: Some(entry_offset),
        });
    }

    let after_as = entry[as_pos + 2..].trim_start();
    let after_as_offset = entry_offset + entry.len() - entry[as_pos + 2..].len();
    let _ = after_as_offset;

    // Next: from_alias, then '(' for fk cols, then REFERENCES, then to_alias
    // Find the '(' for fk cols
    let paren_pos = after_as.find('(').ok_or_else(|| ParseError {
        message: format!(
            "Expected '(' after from_alias in relationship '{rel_name}'. Got: '{after_as}'",
        ),
        position: Some(entry_offset),
    })?;

    let from_alias = after_as[..paren_pos].trim();
    if from_alias.is_empty() {
        return Err(ParseError {
            message: format!("Expected from_alias before '(' in relationship '{rel_name}'."),
            position: Some(entry_offset),
        });
    }

    // Extract fk_columns from parenthesized list
    let paren_content =
        extract_paren_content(&after_as[paren_pos..]).ok_or_else(|| ParseError {
            message: format!("Unclosed '(' in FK column list for relationship '{rel_name}'."),
            position: Some(entry_offset),
        })?;

    let fk_columns: Vec<String> = split_at_depth0_commas(paren_content)
        .into_iter()
        .map(|(_, entry)| entry.to_string())
        .collect();

    // Find REFERENCES after the closing paren. extract_paren_content is
    // quote-aware and requires its input to start with '(', so the matching
    // close is exactly one byte past the content — a naive find(')') would
    // stop at a ')' inside a quoted FK column name (PA-6).
    let close_paren_pos = paren_pos + paren_content.len() + 2;

    let after_paren = after_as[close_paren_pos..].trim_start();
    let upper_after = after_paren.to_ascii_uppercase();
    let refs_pos = find_keyword_ci(&upper_after, "REFERENCES").ok_or_else(|| ParseError {
        message: format!("Expected 'REFERENCES' after FK columns in relationship '{rel_name}'."),
        position: Some(entry_offset),
    })?;

    let remaining_after_refs = after_paren[refs_pos + "REFERENCES".len()..].trim();

    // Get target alias: may be followed by '(' for explicit ref columns
    let (to_alias, after_to) = if remaining_after_refs.is_empty() {
        return Err(ParseError {
            message: format!(
                "Expected target alias after REFERENCES in relationship '{rel_name}'.",
            ),
            position: Some(entry_offset),
        });
    } else if let Some(paren_idx) = find_live_byte(remaining_after_refs, b'(') {
        let before_paren = remaining_after_refs[..paren_idx].trim_end();
        if before_paren.contains(char::is_whitespace) {
            // "target (col)" -- split at first whitespace
            let (alias, rest) = split_first_token(remaining_after_refs);
            (alias, rest.trim_start())
        } else if before_paren.is_empty() {
            return Err(ParseError {
                message: format!(
                    "Expected target alias after REFERENCES in relationship '{rel_name}'.",
                ),
                position: Some(entry_offset),
            });
        } else {
            // "target(col)" -- alias is before '('
            (before_paren, &remaining_after_refs[paren_idx..])
        }
    } else {
        // No paren at all -- target alias is first token
        let (alias, rest) = split_first_token(remaining_after_refs);
        (alias, rest.trim_start())
    };

    // Parse optional ref_columns from REFERENCES target(col1, col2)
    let (ref_columns, after_ref_cols) = if after_to.starts_with('(') {
        let cols_str = extract_paren_content(after_to).ok_or_else(|| ParseError {
            message: format!(
                "Unclosed '(' in REFERENCES column list for relationship '{rel_name}'.",
            ),
            position: Some(entry_offset),
        })?;
        let cols: Vec<String> = split_at_depth0_commas(cols_str)
            .into_iter()
            .map(|(_, entry)| entry.to_string())
            .collect();
        // Quote-aware close (see the FK-list scan above): after_to starts
        // with '(', so the matching ')' is one byte past the content.
        let close = 1 + cols_str.len();
        (cols, after_to[close + 1..].trim())
    } else {
        (vec![], after_to)
    };

    // Reject any remaining tokens (old cardinality keywords or garbage)
    if !after_ref_cols.is_empty() {
        return Err(ParseError {
            message: format!(
                "Unexpected tokens after REFERENCES target in relationship '{rel_name}': '{after_ref_cols}'. \
                 Cardinality is now inferred from PK/UNIQUE constraints; explicit keywords are no longer supported.",
            ),
            position: Some(entry_offset),
        });
    }

    Ok(Join {
        table: to_alias.to_string(),
        from_alias: from_alias.to_string(),
        fk_columns,
        ref_columns,
        name: Some(rel_name.to_string()),
        cardinality: Cardinality::default(), // will be set by inference
        on: String::new(),
        from_cols: vec![],
        join_columns: vec![],
    })
}
