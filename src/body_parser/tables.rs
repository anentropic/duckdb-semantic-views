//! TABLES clause parsing.
//!
//! §6.1 (code-review 2026-07-11): this is the first clause migrated onto the
//! shared [`Cursor`]/lexer. Each entry is parsed by consuming tokens in order
//! — alias, `AS`, source-table name, then the optional `PRIMARY KEY` / `UNIQUE`
//! constraints — so "text between the name and a constraint" is a visible
//! unexpected token rather than a region a `find_primary_key`-anywhere scan
//! could silently slice past (the P-1 silent-discard hole). Quote-awareness is
//! structural: a keyword inside a `"quoted"` name or a `'string'` COMMENT is a
//! single token, never a matchable keyword.
//!
//! The trailing `COMMENT` / `WITH SYNONYMS` region is still handed as verbatim
//! source to the shared [`parse_trailing_annotations`], which tiles it (P-2).

use super::annotations::parse_trailing_annotations;
use super::cursor::Cursor;
use super::lexer::TokenKind;
use super::split_at_depth0_commas;
use crate::errors::ParseError;
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

/// The "physical table name is missing" error is emitted from three guards
/// (no name token, a bare reserved keyword in the name slot, ...) so the exact
/// message stays single-sourced.
fn missing_name_msg(alias: &str) -> String {
    format!("Missing physical table name after AS for alias '{alias}' in TABLES clause.")
}

/// Parse a single TABLES clause entry.
///
/// Supports:
/// - `alias AS physical_table PRIMARY KEY (cols) [UNIQUE (cols)]*`
/// - `alias AS physical_table [UNIQUE (cols)]*`   (no PRIMARY KEY -- fact tables)
/// - `alias AS physical_table`                    (bare -- no PK, no UNIQUE)
fn parse_single_table_entry(entry: &str, entry_offset: usize) -> Result<TableRef, ParseError> {
    let entry = entry.trim();
    let mut cur = Cursor::new(entry, entry_offset);

    // Step 1: alias — the first token.
    let Some(alias_tok) = cur.bump() else {
        return Err(cur.err(0, "Expected table alias in TABLES entry.".to_string()));
    };
    let alias = cur.text(alias_tok);

    // Step 2: the `AS` keyword. Tokenization gives the word boundary for free —
    // `ASorders` is a single bare token that is not `AS`, and `AS"my table"`
    // splits into `AS` + the quoted name — so no manual boundary check is
    // needed (the PR #50 review's `is_ident_continuation` guard).
    match cur.peek() {
        Some(t) if cur.is_kw(t, "AS") => {
            cur.bump();
        }
        _ => {
            let off = cur.byte_pos();
            return Err(cur.err(
                off,
                format!("Expected 'AS' after table alias '{alias}' in TABLES clause."),
            ));
        }
    }

    // Step 3: the source-table name (see `take_source_table_name`).
    let (table_name, name_end) = take_source_table_name(&mut cur, entry, alias)?;

    // Step 4: optional PRIMARY KEY. Its keyword pair may appear anywhere in the
    // remaining tokens; any token before it is text that does not belong
    // between the name and the constraint (P-1).
    let mut pk_columns: Vec<String> = Vec::new();
    if let Some(pk_tok) = cur.find_kw_pair("PRIMARY", "KEY") {
        let between = entry[name_end..pk_tok.start].trim();
        if !between.is_empty() {
            let off = cur.peek().map_or(name_end, |t| t.start);
            return Err(cur.err(
                off,
                format!(
                    "Unexpected text '{between}' between source table name and PRIMARY KEY for alias '{alias}' in TABLES clause. Constraints must immediately follow the table name; COMMENT / WITH SYNONYMS come after constraints.",
                ),
            ));
        }
        cur.bump(); // PRIMARY
        cur.bump(); // KEY
        pk_columns = take_columns(
            &mut cur,
            "Expected '(' after PRIMARY KEY in TABLES clause.".to_string(),
            "Unclosed '(' in PRIMARY KEY column list.".to_string(),
        )?;
    }

    // Step 5: zero or more UNIQUE constraints, each immediately following the
    // previous constraint. Text before a UNIQUE keyword is rejected, not
    // discarded (P-1 companion).
    let mut unique_constraints: Vec<Vec<String>> = Vec::new();
    while let Some(u_tok) = cur.find_kw("UNIQUE") {
        let between = entry[cur.byte_pos()..u_tok.start].trim();
        if !between.is_empty() {
            let off = cur.peek().map_or(u_tok.start, |t| t.start);
            return Err(cur.err(
                off,
                format!(
                    "Unexpected text '{between}' before UNIQUE for alias '{alias}' in TABLES clause. Constraints must immediately follow the table name or the preceding constraint; COMMENT / WITH SYNONYMS come after constraints.",
                ),
            ));
        }
        cur.bump(); // UNIQUE
        let cols = take_columns(
            &mut cur,
            format!("Expected '(' after UNIQUE keyword for table alias '{alias}'."),
            format!("Unclosed '(' in UNIQUE column list for table alias '{alias}'."),
        )?;
        unique_constraints.push(cols);
    }

    // Step 6: trailing COMMENT / WITH SYNONYMS annotations. The shared parser
    // tiles the region exactly; any non-annotation text left in front of it is
    // reported here rather than silently dropped (PA-9 companion).
    let (leftover, annotations) = parse_trailing_annotations(cur.rest())?;
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

/// Capture the source-table name after `AS` — a maximal run of tokens with no
/// whitespace gap, stopping before a `(` / `;` symbol. This reproduces
/// `find_identifier_end`: a dotted / quoted FQN like `"my db"."sch"."t"` is
/// contiguous and captured whole, while a following ` PRIMARY KEY` is separated
/// by whitespace and left for the constraint parser. Returns the verbatim name
/// slice and its end offset (in `entry`) for the caller's "between" check.
fn take_source_table_name<'a>(
    cur: &mut Cursor<'a>,
    entry: &'a str,
    alias: &str,
) -> Result<(&'a str, usize), ParseError> {
    let Some(first) = cur.peek() else {
        return Err(cur.err(cur.byte_pos(), missing_name_msg(alias)));
    };
    if matches!(first.kind, TokenKind::Symbol(_)) {
        // A leading `(` / `.` / `=` / etc. where the name should be. `find_identifier_end`
        // returned 0 only for `(` / `;` / whitespace, but a leading `.foo` / `=x`
        // is a non-name that is better rejected here than accepted as a bogus
        // table name (pinned by `test_leading_symbol_in_name_slot_is_missing_name`).
        return Err(cur.err(first.start, missing_name_msg(alias)));
    }
    let name_start = first.start;
    let mut name_end = first.end;
    let mut unterminated_ident = matches!(first.kind, TokenKind::Unterminated { ident: true });
    cur.bump();
    while let Some(t) = cur.peek() {
        // A whitespace gap or a `(` / `;` symbol ends the name.
        if t.start != name_end || matches!(t.kind, TokenKind::Symbol(b'(' | b';')) {
            break;
        }
        if matches!(t.kind, TokenKind::Unterminated { ident: true }) {
            unterminated_ident = true;
        }
        name_end = t.end;
        cur.bump();
    }
    let table_name = &entry[name_start..name_end];

    // Bare reserved keywords in the name slot surface the missing-name error
    // (Phase 68 A1 / D-03) — `o AS PRIMARY KEY (id)` has no real table name.
    if matches!(
        table_name.to_ascii_uppercase().as_str(),
        "PRIMARY" | "UNIQUE" | "FOREIGN" | "REFERENCES" | "NOT"
    ) {
        return Err(cur.err(name_start, missing_name_msg(alias)));
    }
    // An unterminated `"..."` in the name slot (Phase 68 A4). A doubled-quote
    // `""` is an escape and stays balanced, so only a genuinely open quote trips
    // this — the lexer already encoded that distinction in the token kind.
    if unterminated_ident {
        return Err(cur.err(
            name_start,
            format!(
                "Unterminated quoted identifier in source-table name for alias '{alias}' in TABLES clause.",
            ),
        ));
    }
    Ok((table_name, name_end))
}

/// Consume the `(col, col, ...)` list that must follow a `PRIMARY KEY` /
/// `UNIQUE` keyword. `expected_msg` fires when no `(` follows; `unclosed_msg`
/// when the `(` never closes. Both carets point at where the `(` was expected.
fn take_columns(
    cur: &mut Cursor,
    expected_msg: String,
    unclosed_msg: String,
) -> Result<Vec<String>, ParseError> {
    let off = cur.byte_pos();
    match cur.peek() {
        Some(t) if t.kind == TokenKind::Symbol(b'(') => {}
        _ => return Err(cur.err(off, expected_msg)),
    }
    let Some(inner) = cur.take_parens() else {
        return Err(cur.err(off, unclosed_msg));
    };
    Ok(split_at_depth0_commas(inner)
        .into_iter()
        .map(|(_, col)| col.to_string())
        .collect())
}
