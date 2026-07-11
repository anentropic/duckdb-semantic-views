//! TABLES clause parsing.

use super::annotations::parse_trailing_annotations;
use super::scan::{
    extract_paren_content, find_primary_key, find_unique, is_ident_continuation,
    is_quoting_balanced, split_first_token,
};
use super::split_at_depth0_commas;
use crate::errors::ParseError;
use crate::ident::find_identifier_end;
use crate::model::TableRef;

/// Parse the content inside TABLES (...).
///
/// Each entry has the form: `alias AS physical_table PRIMARY KEY (col1, col2, ...)`
pub(crate) fn parse_tables_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<TableRef>, ParseError> {
    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let table_ref = parse_single_table_entry(entry, entry_offset)?;
        result.push(table_ref);
    }

    Ok(result)
}

/// Parse a single TABLES clause entry.
///
/// Supports:
/// - `alias AS physical_table PRIMARY KEY (cols) [UNIQUE (cols)]*`
/// - `alias AS physical_table [UNIQUE (cols)]*`   (no PRIMARY KEY -- fact tables)
/// - `alias AS physical_table`                    (bare -- no PK, no UNIQUE)
#[allow(clippy::too_many_lines)]
fn parse_single_table_entry(entry: &str, entry_offset: usize) -> Result<TableRef, ParseError> {
    // Step 1: get alias (first whitespace-delimited token)
    let entry = entry.trim();
    let (alias, rest) = split_first_token(entry);
    if alias.is_empty() {
        return Err(ParseError {
            message: "Expected table alias in TABLES entry.".to_string(),
            position: Some(entry_offset),
        });
    }
    let rest = rest.trim();
    let rest_offset = entry_offset + entry.len() - rest.len();

    // Step 2: expect "AS" keyword, with a trailing word boundary —
    // `ASschema.table` / `ASX` must not match (PR #50 review). Punctuation
    // like `"` stays a legal boundary (`AS"my table"`).
    let as_ok = rest.get(..2).is_some_and(|s| s.eq_ignore_ascii_case("AS"))
        && (rest.len() == 2 || !is_ident_continuation(rest.as_bytes()[2]));
    if !as_ok {
        return Err(ParseError {
            message: format!("Expected 'AS' after table alias '{alias}' in TABLES clause."),
            position: Some(rest_offset),
        });
    }
    let after_as = rest[2..].trim_start();
    let after_as_offset = rest_offset + 2 + (rest[2..].len() - after_as.len());

    // Step 3: capture the source-table name using identifier-aware tokenisation
    // (Phase 67 Plan 02 / TECH-DEBT #24). `find_identifier_end` natively walks
    // across dots while outside quoted regions (verified by the
    // `fqn_with_quoted_parts_runs_to_whitespace` doctest in `src/ident.rs`), so
    // a single call captures `schema.tbl`, `"my db"."schema"."col"`, etc. The
    // earlier dot-rejoin loop arm was unreachable (Phase 68 A3 collapse).
    // This MUST run before we look for the PRIMARY KEY / UNIQUE trailing
    // keywords — otherwise the case-insensitive scan over the whole `after_as`
    // slice can match a `PRIMARY KEY` substring INSIDE a quoted source-table
    // name.
    let name_end = find_identifier_end(after_as, /* allow_paren = */ true);
    if name_end == 0 {
        return Err(ParseError {
            message: format!(
                "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
            ),
            position: Some(after_as_offset),
        });
    }
    // Phase 68 A1 (D-03): reject bare reserved keywords captured as the
    // source-table name. Without this guard `o AS PRIMARY KEY (id)` would
    // succeed at `find_identifier_end` (capturing `PRIMARY` up to the
    // following whitespace) and the downstream PRIMARY-KEY scan over
    // `" KEY (id)"` would fail to find the keyword (already eaten), producing
    // a confusing "table 'PRIMARY' does not exist" downstream. The literal
    // pre-Phase-67 error message is the contract — see Phase 68 CONTEXT.md
    // D-03 for the authoritative keyword set.
    let captured = after_as[..name_end].trim();
    let upper_captured = captured.to_ascii_uppercase();
    if matches!(
        upper_captured.as_str(),
        "PRIMARY" | "UNIQUE" | "FOREIGN" | "REFERENCES" | "NOT"
    ) {
        return Err(ParseError {
            message: format!(
                "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
            ),
            position: Some(after_as_offset),
        });
    }
    let table_name = captured;
    if table_name.is_empty() {
        return Err(ParseError {
            message: format!(
                "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
            ),
            position: Some(after_as_offset),
        });
    }
    // Phase 68 A4: reject unterminated quoted source-table names. `find_identifier_end`
    // saturates at input.len() on an unterminated quote rather than surfacing an
    // error, so `o AS "unclosed` would otherwise pass through as `table_name =
    // "\"unclosed"` and corrupt downstream catalog lookups. The doubled-quote
    // escape `""` is balanced — mirror src/ident.rs::find_identifier_end's escape
    // rule via the private `is_quoting_balanced` helper.
    if !is_quoting_balanced(&after_as[..name_end]) {
        return Err(ParseError {
            message: format!(
                "Unterminated quoted identifier in source-table name for alias '{alias}' in TABLES clause.",
            ),
            position: Some(after_as_offset),
        });
    }
    let after_name = &after_as[name_end..];
    let after_name_offset = after_as_offset + name_end;

    // Step 3a: now search ONLY the post-name slice for the optional
    // PRIMARY KEY / UNIQUE trailing clauses. `name_end` was computed via
    // identifier-aware tokenisation so we can safely uppercase the rest.
    let upper_after_name = after_name.to_ascii_uppercase();
    let pk_pos = find_primary_key(&upper_after_name);

    // P-1 (code-review 2026-07-11): reject text between the source-table
    // name and PRIMARY KEY instead of silently discarding it. The keyword
    // scan finds PRIMARY KEY anywhere in the post-name slice, so
    // `o AS orders COMMENT = 'doc' PRIMARY KEY (id)` previously parsed
    // "successfully" with the comment destroyed.
    if let Some((pk_start, _)) = pk_pos {
        let pre = &after_name[..pk_start];
        if !pre.trim().is_empty() {
            return Err(ParseError {
                message: format!(
                    "Unexpected text '{}' between source table name and PRIMARY KEY for alias '{alias}' in TABLES clause. Constraints must immediately follow the table name; COMMENT / WITH SYNONYMS come after constraints.",
                    pre.trim()
                ),
                position: Some(after_name_offset + (pre.len() - pre.trim_start().len())),
            });
        }
    }

    // `after_pk_offset` tracks the absolute byte offset of `after_pk_text[0]`
    // so the UNIQUE-loop guards below can point their carets at the actual
    // offending token rather than the start of the entry (Copilot review,
    // PR #71).
    let (pk_columns, after_pk_text, after_pk_offset) = if let Some((_pk_start, pk_end)) = pk_pos {
        let after_pk_raw = &after_name[pk_end..];
        let after_pk = after_pk_raw.trim_start();
        let after_pk_offset = after_name_offset + pk_end + (after_pk_raw.len() - after_pk.len());
        if !after_pk.starts_with('(') {
            return Err(ParseError {
                message: "Expected '(' after PRIMARY KEY in TABLES clause.".to_string(),
                position: Some(after_pk_offset),
            });
        }
        let pk_body = extract_paren_content(after_pk).ok_or_else(|| ParseError {
            message: "Unclosed '(' in PRIMARY KEY column list.".to_string(),
            position: Some(after_pk_offset),
        })?;
        let pk_columns: Vec<String> = split_at_depth0_commas(pk_body)
            .into_iter()
            .map(|(_, entry)| entry.to_string())
            .collect();
        // extract_paren_content requires after_pk to start with '(' and is
        // quote-aware, so the matching close is exactly one byte past the
        // content — a naive find(')') would stop at a ')' inside a quoted
        // column name (PA-6).
        let close = 1 + pk_body.len();
        let remainder = &after_pk[close + 1..];
        (pk_columns, remainder, after_pk_offset + close + 1)
    } else {
        // No PRIMARY KEY -- fact table. Hand the whole post-name slice to
        // the UNIQUE/annotation steps below. (Previously the bare no-PK /
        // no-UNIQUE case hard-set the remainder to "" — silently dropping
        // table-level COMMENT / WITH SYNONYMS annotations, PA-9.)
        (vec![], after_name, after_name_offset)
    };

    // Step 4: parse zero or more UNIQUE constraints from after_pk_text
    let mut unique_constraints: Vec<Vec<String>> = Vec::new();
    let mut remaining = after_pk_text;
    let mut remaining_offset = after_pk_offset;
    loop {
        let upper_remaining = remaining.to_ascii_uppercase();
        if let Some((u_start, u_end)) = find_unique(&upper_remaining) {
            // P-1 companion: text before a UNIQUE constraint is an error,
            // not silently discarded (the same anywhere-scan hole as the
            // PRIMARY KEY slot above).
            let pre = &remaining[..u_start];
            if !pre.trim().is_empty() {
                return Err(ParseError {
                    message: format!(
                        "Unexpected text '{}' before UNIQUE for alias '{alias}' in TABLES clause. Constraints must immediately follow the table name or the preceding constraint; COMMENT / WITH SYNONYMS come after constraints.",
                        pre.trim()
                    ),
                    // Caret on the junk token, not the entry start (skip
                    // leading whitespace within `pre`).
                    position: Some(remaining_offset + (pre.len() - pre.trim_start().len())),
                });
            }
            let after_unique_raw = &remaining[u_end..];
            let after_unique_kw = after_unique_raw.trim_start();
            let after_unique_offset =
                remaining_offset + u_end + (after_unique_raw.len() - after_unique_kw.len());
            if !after_unique_kw.starts_with('(') {
                return Err(ParseError {
                    message: format!(
                        "Expected '(' after UNIQUE keyword for table alias '{alias}'."
                    ),
                    position: Some(after_unique_offset),
                });
            }
            let cols_str = extract_paren_content(after_unique_kw).ok_or_else(|| ParseError {
                message: format!("Unclosed '(' in UNIQUE column list for table alias '{alias}'."),
                position: Some(after_unique_offset),
            })?;
            let cols: Vec<String> = split_at_depth0_commas(cols_str)
                .into_iter()
                .map(|(_, entry)| entry.to_string())
                .collect();
            unique_constraints.push(cols);
            // Quote-aware close (see the PRIMARY KEY branch above).
            let close = 1 + cols_str.len();
            remaining = &after_unique_kw[close + 1..];
            remaining_offset = after_unique_offset + close + 1;
        } else {
            break;
        }
    }

    // Phase 43: Parse trailing COMMENT / WITH SYNONYMS annotations after constraints.
    // Any non-annotation text left over is an error rather than silently
    // discarded (PA-9 companion: `o AS orders garbage COMMENT = 'x'`).
    let (leftover, annotations) = parse_trailing_annotations(remaining)?;
    if !leftover.trim().is_empty() {
        return Err(ParseError {
            message: format!(
                "Unexpected text '{}' after table declaration for alias '{alias}' in TABLES clause.",
                leftover.trim()
            ),
            position: Some(entry_offset),
        });
    }

    Ok(TableRef {
        alias: alias.to_string(),
        table: table_name.to_string(),
        pk_columns,
        unique_constraints,
        comment: annotations.comment,
        synonyms: annotations.synonyms,
    })
}
