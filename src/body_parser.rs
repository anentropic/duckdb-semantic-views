//! SQL keyword body parser for CREATE SEMANTIC VIEW.
//!
//! Parses: `AS TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)`
//! into a `SemanticViewDefinition`.

use crate::model::{Dimension, Fact, Hierarchy, Join, Metric, TableRef};
use crate::parse::ParseError;

/// Result of parsing the keyword body (everything after "AS").
#[derive(Debug)]
pub struct KeywordBody {
    pub tables: Vec<TableRef>,
    pub relationships: Vec<Join>,
    pub facts: Vec<Fact>,
    pub hierarchies: Vec<Hierarchy>,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
}

/// Known clause keywords for the AS-body scanner.
const CLAUSE_KEYWORDS: &[&str] = &[
    "tables",
    "relationships",
    "facts",
    "hierarchies",
    "dimensions",
    "metrics",
];

/// Clause ordering — TABLES must be first, then RELATIONSHIPS (optional),
/// FACTS (optional), HIERARCHIES (optional), DIMENSIONS (optional),
/// METRICS (optional). At least one of DIMENSIONS or METRICS is required.
const CLAUSE_ORDER: &[&str] = &[
    "tables",
    "relationships",
    "facts",
    "hierarchies",
    "dimensions",
    "metrics",
];

/// Suggest the closest known clause keyword for a near-miss word.
fn suggest_clause_keyword(word: &str) -> Option<&'static str> {
    let lower = word.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for &kw in CLAUSE_KEYWORDS {
        let dist = strsim::levenshtein(&lower, kw);
        if dist <= 3 {
            match best {
                Some((best_dist, _)) if dist < best_dist => best = Some((dist, kw)),
                None => best = Some((dist, kw)),
                _ => {}
            }
        }
    }
    best.map(|(_, kw)| kw)
}

/// Split `body` at depth-0 commas, respecting nested parens and single-quoted strings.
/// Returns `Vec<(start_offset_in_body, trimmed_slice)>`. Trailing empty entries discarded.
pub(crate) fn split_at_depth0_commas(body: &str) -> Vec<(usize, &str)> {
    let mut entries = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut start = 0;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            // Handle escaped single quotes: '' inside a string
            if in_string && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            in_string = !in_string;
        } else if !in_string {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    let entry = body[start..i].trim();
                    if !entry.is_empty() {
                        entries.push((start, entry));
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let tail = body[start..].trim();
    if !tail.is_empty() {
        entries.push((start, tail));
    }
    entries
}

/// Internal result of scanning a single clause from the AS-body.
struct ClauseBound<'a> {
    keyword: &'static str,
    content: &'a str,      // text inside the matching parens
    content_offset: usize, // byte offset of content[0] relative to the AS-body text
}

/// Scan `text` (the text after "AS") at depth 0 to find clause headers of the form
/// `KEYWORD (...)`. Returns all found clause bounds in encounter order.
///
/// Validates:
/// - All keywords must be in `CLAUSE_KEYWORDS` (with "did you mean?" suggestion on error)
/// - No duplicate clauses
/// - Order must be TABLES -> RELATIONSHIPS? -> DIMENSIONS? -> METRICS?
/// - TABLES is required; at least one of DIMENSIONS or METRICS is required
#[allow(clippy::too_many_lines)]
fn find_clause_bounds<'a>(
    text: &'a str,
    base_offset: usize,
) -> Result<Vec<ClauseBound<'a>>, ParseError> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut bounds: Vec<ClauseBound<'a>> = Vec::new();
    let mut seen: Vec<&'static str> = Vec::new();

    while i < bytes.len() {
        // Skip whitespace
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        // Collect identifier word
        if !(bytes[i] as char).is_ascii_alphabetic() {
            // Unexpected character at top level
            let ch = bytes[i] as char;
            return Err(ParseError {
                message: format!(
                    "Unexpected character '{ch}' in AS body; expected a clause keyword (TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS).",
                ),
                position: Some(base_offset + i),
            });
        }

        let word_start = i;
        while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
            i += 1;
        }
        let word = &text[word_start..i];
        let lower = word.to_ascii_lowercase();

        // Find matching static keyword
        let keyword: &'static str = if let Some(&kw) =
            CLAUSE_KEYWORDS.iter().find(|&&kw| kw == lower)
        {
            kw
        } else {
            let msg = if let Some(sug) = suggest_clause_keyword(word) {
                let sug_upper = sug.to_ascii_uppercase();
                format!("Unknown clause keyword '{word}'; did you mean '{sug_upper}'?",)
            } else {
                format!(
                    "Unknown clause keyword '{word}'; expected one of TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS.",
                )
            };
            return Err(ParseError {
                message: msg,
                position: Some(base_offset + word_start),
            });
        };

        // Duplicate check
        if seen.contains(&keyword) {
            let kw_upper = keyword.to_ascii_uppercase();
            return Err(ParseError {
                message: format!("Duplicate clause keyword '{kw_upper}'."),
                position: Some(base_offset + word_start),
            });
        }

        // Skip whitespace after keyword
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }

        // Expect '('
        if i >= bytes.len() || bytes[i] as char != '(' {
            let kw_upper = keyword.to_ascii_uppercase();
            let found = if i < bytes.len() {
                bytes[i] as char
            } else {
                '\0'
            };
            return Err(ParseError {
                message: format!(
                    "Expected '(' after clause keyword '{kw_upper}', found '{found}'.",
                ),
                position: Some(base_offset + i),
            });
        }
        let open_paren_pos = i;
        i += 1; // skip '('

        // Find matching ')' with depth tracking
        let content_start = i;
        let mut depth: i32 = 1;
        let mut in_string = false;
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
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }

        if depth != 0 {
            let kw_upper = keyword.to_ascii_uppercase();
            return Err(ParseError {
                message: format!("Unclosed '(' for clause '{kw_upper}'."),
                position: Some(base_offset + open_paren_pos),
            });
        }

        let content = &text[content_start..i];
        let content_offset = base_offset + content_start;
        i += 1; // skip closing ')'

        seen.push(keyword);
        bounds.push(ClauseBound {
            keyword,
            content,
            content_offset,
        });
    }

    // Validate ordering: TABLES < RELATIONSHIPS < DIMENSIONS < METRICS
    let mut last_order: Option<usize> = None;
    for bound in &bounds {
        let order = CLAUSE_ORDER
            .iter()
            .position(|&k| k == bound.keyword)
            .unwrap_or(999);
        if last_order.is_some_and(|lo| order <= lo) {
            let kw_upper = bound.keyword.to_ascii_uppercase();
            return Err(ParseError {
                message: format!(
                    "Clause '{kw_upper}' appears out of order; clauses must appear as: TABLES, RELATIONSHIPS (optional), FACTS (optional), HIERARCHIES (optional), DIMENSIONS (optional), METRICS (optional).",
                ),
                position: None,
            });
        }
        last_order = Some(order);
    }

    // Required: TABLES must be present
    if !seen.contains(&"tables") {
        return Err(ParseError {
            message: "Missing required clause 'TABLES'.".to_string(),
            position: None,
        });
    }

    // Required: at least one of DIMENSIONS or METRICS
    if !seen.contains(&"dimensions") && !seen.contains(&"metrics") {
        return Err(ParseError {
            message: "At least one of 'DIMENSIONS' or 'METRICS' is required.".to_string(),
            position: None,
        });
    }

    Ok(bounds)
}

/// Parse the keyword body after "AS" into structured clause data.
///
/// `text` is the full text starting with "AS", trimmed.
/// `base_offset` is the byte offset of `text[0]` in the original query string.
pub fn parse_keyword_body(text: &str, base_offset: usize) -> Result<KeywordBody, ParseError> {
    // Strip leading "AS" (case-insensitive)
    let trimmed = text.trim();
    let after_as = if trimmed
        .get(..2)
        .is_some_and(|s| s.eq_ignore_ascii_case("AS"))
    {
        trimmed[2..].trim_start()
    } else {
        return Err(ParseError {
            message: "Expected 'AS' keyword at start of semantic view body.".to_string(),
            position: Some(base_offset),
        });
    };

    // Offset of after_as within the original query
    let as_offset = base_offset + (text.len() - text.trim_start().len()) + 2;
    let after_as_offset = as_offset + (text.trim_start()[2..].len() - after_as.len());

    let bounds = find_clause_bounds(after_as, after_as_offset)?;

    let mut tables: Vec<TableRef> = Vec::new();
    let mut relationships: Vec<Join> = Vec::new();
    let mut facts_raw: Vec<(String, String, String)> = Vec::new();
    let mut hierarchies: Vec<Hierarchy> = Vec::new();
    let mut dimensions_raw: Vec<(String, String, String)> = Vec::new();
    let mut metrics_raw: Vec<(Option<String>, String, String)> = Vec::new();

    for bound in &bounds {
        match bound.keyword {
            "tables" => {
                tables = parse_tables_clause(bound.content, bound.content_offset)?;
            }
            "relationships" => {
                relationships = parse_relationships_clause(bound.content, bound.content_offset)?;
            }
            "facts" => {
                facts_raw = parse_qualified_entries(bound.content, bound.content_offset)?;
            }
            "hierarchies" => {
                hierarchies = parse_hierarchies_clause(bound.content, bound.content_offset)?;
            }
            "dimensions" => {
                dimensions_raw = parse_qualified_entries(bound.content, bound.content_offset)?;
            }
            "metrics" => {
                metrics_raw = parse_metrics_clause(bound.content, bound.content_offset)?;
            }
            _ => {}
        }
    }

    // Map qualified entries to Fact / Dimension / Metric structs
    let facts = facts_raw
        .into_iter()
        .map(|(alias, bare_name, expr)| Fact {
            name: bare_name,
            expr,
            source_table: Some(alias),
        })
        .collect();

    let dimensions = dimensions_raw
        .into_iter()
        .map(|(alias, bare_name, expr)| Dimension {
            name: bare_name,
            expr,
            source_table: Some(alias),
            output_type: None,
        })
        .collect();

    let metrics = metrics_raw
        .into_iter()
        .map(|(source_alias, bare_name, expr)| Metric {
            name: bare_name,
            expr,
            source_table: source_alias,
            output_type: None,
        })
        .collect();

    Ok(KeywordBody {
        tables,
        relationships,
        facts,
        hierarchies,
        dimensions,
        metrics,
    })
}

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

/// Parse a single TABLES clause entry: `alias AS physical_table PRIMARY KEY (col1, ...)`
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

    // Step 2: expect "AS" keyword
    if !rest.get(..2).is_some_and(|s| s.eq_ignore_ascii_case("AS")) {
        return Err(ParseError {
            message: format!("Expected 'AS' after table alias '{alias}' in TABLES clause."),
            position: Some(rest_offset),
        });
    }
    let after_as = rest[2..].trim_start();
    let after_as_offset = rest_offset + 2 + (rest[2..].len() - after_as.len());

    // Step 3: find "PRIMARY KEY" (case-insensitive, any whitespace between words)
    let upper = after_as.to_ascii_uppercase();
    let pk_pos = find_primary_key(&upper);
    if pk_pos.is_none() {
        return Err(ParseError {
            message: format!(
                "Expected 'PRIMARY KEY' after table name in TABLES entry for alias '{alias}'.",
            ),
            position: Some(after_as_offset),
        });
    }
    let (pk_start, pk_end) = pk_pos.unwrap();
    let table_name = after_as[..pk_start].trim();
    if table_name.is_empty() {
        return Err(ParseError {
            message: format!(
                "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
            ),
            position: Some(after_as_offset),
        });
    }

    // Step 4: parse "(col1, col2, ...)" after "PRIMARY KEY"
    let after_pk = after_as[pk_end..].trim_start();
    let after_pk_offset = after_as_offset + pk_end;
    let _ = after_pk_offset; // offset tracked for future use

    if !after_pk.starts_with('(') {
        return Err(ParseError {
            message: "Expected '(' after PRIMARY KEY in TABLES clause.".to_string(),
            position: Some(after_as_offset + pk_end),
        });
    }

    // Find matching closing paren
    let pk_body = extract_paren_content(after_pk).ok_or_else(|| ParseError {
        message: "Unclosed '(' in PRIMARY KEY column list.".to_string(),
        position: Some(after_as_offset + pk_end),
    })?;

    let pk_columns: Vec<String> = pk_body
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();

    Ok(TableRef {
        alias: alias.to_string(),
        table: table_name.to_string(),
        pk_columns,
    })
}

/// Extract content inside the outermost `(...)` of `s` (which must start with `(`).
/// Returns the content between the first `(` and its matching `)`, or `None` if unbalanced.
fn extract_paren_content(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'(' {
        return None;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        let ch = b as char;
        if ch == '\'' {
            in_string = !in_string;
        } else if !in_string {
            match ch {
                '(' => {
                    depth += 1;
                    if depth == 1 {
                        start = Some(i + 1);
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[start.unwrap()..i]);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Find the byte position of a keyword (already uppercased) in `upper_text`.
/// Find "PRIMARY KEY" with any amount of whitespace between the two words.
/// Returns `(start, end)` byte offsets into `upper_text`, where `upper_text` is already uppercased.
/// `start` points at 'P', `end` points past 'Y' (exclusive).
fn find_primary_key(upper_text: &str) -> Option<(usize, usize)> {
    let bytes = upper_text.as_bytes();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        // Look for "PRIMARY"
        if &upper_text[i..i + 7] == "PRIMARY" {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after_primary = i + 7;
            if before_ok
                && (after_primary == bytes.len() || !bytes[after_primary].is_ascii_alphanumeric())
            {
                // Skip whitespace between PRIMARY and KEY
                let mut j = after_primary;
                while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                    j += 1;
                }
                // Match "KEY"
                if j + 3 <= bytes.len() && &upper_text[j..j + 3] == "KEY" {
                    let after_key = j + 3;
                    let after_ok =
                        after_key == bytes.len() || !bytes[after_key].is_ascii_alphanumeric();
                    if after_ok {
                        return Some((i, after_key));
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Requires the keyword to be preceded by whitespace (or be at start) and
/// followed by whitespace or end-of-string. Returns byte offset into `upper_text`.
fn find_keyword_ci(upper_text: &str, keyword: &str) -> Option<usize> {
    let kw_len = keyword.len();
    let text_len = upper_text.len();
    if text_len < kw_len {
        return None;
    }
    let mut i = 0;
    while i + kw_len <= text_len {
        if &upper_text[i..i + kw_len] == keyword {
            // Check boundary: preceded by non-alpha (or start), followed by non-alpha (or end)
            let before_ok = i == 0 || !upper_text.as_bytes()[i - 1].is_ascii_alphanumeric();
            let after_ok = i + kw_len == text_len
                || !upper_text.as_bytes()[i + kw_len].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Split `s` at the first ASCII whitespace, returning `(first_token, rest)`.
/// If no whitespace found, returns `(s, "")`.
fn split_first_token(s: &str) -> (&str, &str) {
    if let Some(pos) = s.find(|c: char| c.is_ascii_whitespace()) {
        (&s[..pos], &s[pos..])
    } else {
        (s, "")
    }
}

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

/// Parse one RELATIONSHIPS entry: `rel_name AS from_alias(fk_cols) REFERENCES to_alias`
fn parse_single_relationship_entry(entry: &str, entry_offset: usize) -> Result<Join, ParseError> {
    let entry = entry.trim();

    // Find "AS" keyword (case-insensitive) — relationship name is before it
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
            message: format!("Unclosed '(' in FK column list for relationship '{rel_name}'.",),
            position: Some(entry_offset),
        })?;

    let fk_columns: Vec<String> = paren_content
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();

    // Find REFERENCES after the closing paren
    let close_paren_pos = after_as[paren_pos..]
        .find(')')
        .map(|p| paren_pos + p + 1)
        .ok_or_else(|| ParseError {
            message: format!("Unclosed '(' in relationship '{rel_name}'."),
            position: Some(entry_offset),
        })?;

    let after_paren = after_as[close_paren_pos..].trim_start();
    let upper_after = after_paren.to_ascii_uppercase();
    let refs_pos = find_keyword_ci(&upper_after, "REFERENCES").ok_or_else(|| ParseError {
        message: format!("Expected 'REFERENCES' after FK columns in relationship '{rel_name}'.",),
        position: Some(entry_offset),
    })?;

    let to_alias = after_paren[refs_pos + "REFERENCES".len()..].trim();
    if to_alias.is_empty() {
        return Err(ParseError {
            message: format!(
                "Expected target alias after REFERENCES in relationship '{rel_name}'.",
            ),
            position: Some(entry_offset),
        });
    }

    Ok(Join {
        table: to_alias.to_string(),
        from_alias: from_alias.to_string(),
        fk_columns,
        name: Some(rel_name.to_string()),
        on: String::new(),
        from_cols: vec![],
        join_columns: vec![],
    })
}

/// Parse the content inside METRICS (...) supporting both qualified and unqualified entries.
///
/// Qualified entries have the form: `alias.name AS expr` (base metric).
/// Unqualified entries have the form: `name AS expr` (derived metric).
///
/// Returns `Vec<(Option<source_alias>, bare_name, expr)>` where the Option is
/// `Some(alias)` for qualified entries and `None` for unqualified (derived) entries.
pub(crate) fn parse_metrics_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<(Option<String>, String, String)>, ParseError> {
    if body.trim().is_empty() {
        return Ok(vec![]);
    }

    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let parsed = parse_single_metric_entry(entry, entry_offset)?;
        result.push(parsed);
    }

    Ok(result)
}

/// Parse one METRICS entry: either `alias.name AS expr` (qualified) or `name AS expr` (derived).
fn parse_single_metric_entry(
    entry: &str,
    entry_offset: usize,
) -> Result<(Option<String>, String, String), ParseError> {
    let entry = entry.trim();

    // Check if entry contains a dot BEFORE the AS keyword -- if so, it's qualified.
    // Find "AS" keyword first (case-insensitive, word boundary).
    let upper = entry.to_ascii_uppercase();
    let as_pos = find_keyword_ci(&upper, "AS").ok_or_else(|| ParseError {
        message: format!(
            "Expected 'AS' keyword in metric entry '{entry}'. Form: 'alias.name AS expr' or 'name AS expr'.",
        ),
        position: Some(entry_offset),
    })?;

    let before_as = entry[..as_pos].trim();
    let expr = entry[as_pos + 2..].trim().to_string();

    if expr.is_empty() {
        return Err(ParseError {
            message: format!("Missing expression after 'AS' in metric entry '{entry}'."),
            position: Some(entry_offset + as_pos + 2),
        });
    }

    if before_as.is_empty() {
        return Err(ParseError {
            message: format!("Missing metric name before 'AS' in entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Check for dot to distinguish qualified vs unqualified
    if let Some(dot_pos) = before_as.find('.') {
        // Qualified: alias.name
        let source_alias = before_as[..dot_pos].trim().to_string();
        let bare_name = before_as[dot_pos + 1..].trim().to_string();

        if source_alias.is_empty() {
            return Err(ParseError {
                message: format!("Source alias before '.' is empty in metric entry '{entry}'."),
                position: Some(entry_offset),
            });
        }
        if bare_name.is_empty() {
            return Err(ParseError {
                message: format!(
                    "Missing bare name between '.' and 'AS' in metric entry '{entry}'."
                ),
                position: Some(entry_offset + dot_pos + 1),
            });
        }

        Ok((Some(source_alias), bare_name, expr))
    } else {
        // Unqualified: just name (derived metric)
        let bare_name = before_as.to_string();
        Ok((None, bare_name, expr))
    }
}

/// Parse the content inside DIMENSIONS or METRICS (...).
/// Returns `Vec<(source_alias, bare_name, expr)>`.
///
/// Each entry has the form: `alias.name AS sql_expression`
pub(crate) fn parse_qualified_entries(
    body: &str,
    base_offset: usize,
) -> Result<Vec<(String, String, String)>, ParseError> {
    if body.trim().is_empty() {
        return Ok(vec![]);
    }

    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let parsed = parse_single_qualified_entry(entry, entry_offset)?;
        result.push(parsed);
    }

    Ok(result)
}

/// Parse one DIMENSIONS/METRICS entry: `alias.bare_name AS expr`
fn parse_single_qualified_entry(
    entry: &str,
    entry_offset: usize,
) -> Result<(String, String, String), ParseError> {
    let entry = entry.trim();

    // Find first '.' to split alias.bare_name
    let dot_pos = entry.find('.').ok_or_else(|| ParseError {
        message: format!(
            "Expected 'alias.name' qualified identifier, got '{entry}'. Each dimension/metric entry must have the form 'alias.name AS expr'.",
        ),
        position: Some(entry_offset),
    })?;

    let source_alias = entry[..dot_pos].trim().to_string();
    if source_alias.is_empty() {
        return Err(ParseError {
            message: format!("Source alias before '.' is empty in entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Everything from dot+1 forward; find "AS" (case-insensitive, word boundary)
    let after_dot = &entry[dot_pos + 1..];
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

    let expr = after_dot[as_pos + 2..].trim().to_string();
    if expr.is_empty() {
        return Err(ParseError {
            message: format!("Missing expression after 'AS' in entry '{entry}'."),
            position: Some(entry_offset + dot_pos + 1 + as_pos + 2),
        });
    }

    Ok((source_alias, bare_name, expr))
}

/// Parse the content inside HIERARCHIES (...).
/// Returns `Vec<Hierarchy>`.
///
/// Each entry has the form: `name AS (dim1, dim2, dim3)`
pub(crate) fn parse_hierarchies_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<Hierarchy>, ParseError> {
    if body.trim().is_empty() {
        return Ok(vec![]);
    }

    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let hierarchy = parse_single_hierarchy_entry(entry, entry_offset)?;
        result.push(hierarchy);
    }

    Ok(result)
}

/// Parse one HIERARCHIES entry: `name AS (dim1, dim2, dim3)`
fn parse_single_hierarchy_entry(entry: &str, entry_offset: usize) -> Result<Hierarchy, ParseError> {
    let entry = entry.trim();

    // Find "AS" keyword (case-insensitive, word boundary)
    let upper = entry.to_ascii_uppercase();
    let as_pos = find_keyword_ci(&upper, "AS").ok_or_else(|| ParseError {
        message: format!(
            "Expected 'AS' keyword in hierarchy entry '{entry}'. Form: 'name AS (dim1, dim2, ...)'.",
        ),
        position: Some(entry_offset),
    })?;

    let name = entry[..as_pos].trim().to_string();
    if name.is_empty() {
        return Err(ParseError {
            message: format!("Missing hierarchy name before 'AS' in entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    let after_as = entry[as_pos + 2..].trim();
    let after_as_offset = entry_offset + entry.len() - entry[as_pos + 2..].len();
    let _ = after_as_offset;

    // Expect '(' after AS
    if !after_as.starts_with('(') {
        return Err(ParseError {
            message: format!(
                "Expected '(' after AS in hierarchy entry '{name}'. Form: 'name AS (dim1, dim2, ...)'.",
            ),
            position: Some(entry_offset + as_pos + 2),
        });
    }

    // Extract parenthesized content
    let paren_content = extract_paren_content(after_as).ok_or_else(|| ParseError {
        message: format!("Unclosed '(' in hierarchy entry '{name}'."),
        position: Some(entry_offset + as_pos + 2),
    })?;

    // Split levels at commas, trim each
    let levels: Vec<String> = paren_content
        .split(',')
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if levels.is_empty() {
        return Err(ParseError {
            message: format!(
                "Hierarchy '{name}' has no levels; at least one dimension level is required.",
            ),
            position: Some(entry_offset),
        });
    }

    Ok(Hierarchy { name, levels })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // split_at_depth0_commas tests
    // -----------------------------------------------------------------------

    #[test]
    fn split_simple_three_entries() {
        let result = split_at_depth0_commas("a, b, c");
        assert_eq!(result.len(), 3, "Expected 3 entries, got {:?}", result);
        assert_eq!(result[0].1, "a");
        assert_eq!(result[1].1, "b");
        assert_eq!(result[2].1, "c");
    }

    #[test]
    fn split_nested_parens_not_split() {
        // The comma inside SUM(a, b) is at depth 1 — must not split
        let result = split_at_depth0_commas("SUM(a, b), COUNT(*)");
        assert_eq!(result.len(), 2, "Expected 2 entries, got {:?}", result);
        assert_eq!(result[0].1, "SUM(a, b)");
        assert_eq!(result[1].1, "COUNT(*)");
    }

    #[test]
    fn split_string_literal_comma_not_split() {
        // Comma inside single-quoted string must not split
        let result = split_at_depth0_commas("a, 'x, y', b");
        assert_eq!(result.len(), 3, "Expected 3 entries, got {:?}", result);
        assert_eq!(result[0].1, "a");
        assert_eq!(result[1].1, "'x, y'");
        assert_eq!(result[2].1, "b");
    }

    #[test]
    fn split_trailing_comma_discarded() {
        let result = split_at_depth0_commas("a,");
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
        assert_eq!(result[0].1, "a");
    }

    #[test]
    fn split_empty_body() {
        let result = split_at_depth0_commas("");
        assert_eq!(result.len(), 0, "Empty body must produce 0 entries");
    }

    // -----------------------------------------------------------------------
    // find_clause_bounds tests (via parse_keyword_body integration)
    // -----------------------------------------------------------------------

    #[test]
    fn find_clause_bounds_basic_tables_dimensions_metrics() {
        // Smoke test: parsing a well-formed AS body finds all 3 clauses
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result.map(|_| ()));
        let kb = result.unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn find_clause_bounds_with_relationships() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id), c AS customers PRIMARY KEY (id)) RELATIONSHIPS (o_to_c AS o(customer_id) REFERENCES c) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.map(|_| ()));
        let kb = result.unwrap();
        assert_eq!(kb.relationships.len(), 1);
    }

    #[test]
    fn find_clause_bounds_rejects_unknown_keyword() {
        // "TABLSE" is close to "TABLES" — should get "did you mean?" error
        let body = "AS TABLSE (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.x AS x)";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Expected error for unknown keyword TABLSE");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("TABLES") || err.message.contains("TABLSE"),
            "Error should mention TABLES or TABLSE: {}",
            err.message
        );
    }

    #[test]
    fn find_clause_bounds_rejects_missing_tables() {
        let body = "AS DIMENSIONS (o.x AS x) METRICS (o.y AS SUM(y))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Expected error for missing TABLES clause");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("TABLES"),
            "Error should mention TABLES: {}",
            err.message
        );
    }

    #[test]
    fn find_clause_bounds_nested_pk_not_confused() {
        // PRIMARY KEY (...) creates nested depth — closing paren must not end TABLES clause early
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (o_orderkey, o_custkey)) DIMENSIONS (o.x AS x)";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.map(|_| ()));
        let kb = result.unwrap();
        assert_eq!(kb.tables[0].pk_columns.len(), 2);
    }

    // -----------------------------------------------------------------------
    // parse_tables_clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_tables_single_pk() {
        let result = parse_tables_clause("o AS orders PRIMARY KEY (o_orderkey)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert_eq!(result[0].pk_columns, vec!["o_orderkey"]);
    }

    #[test]
    fn parse_tables_schema_qualified() {
        let result = parse_tables_clause("o AS main.orders PRIMARY KEY (o_orderkey)", 0).unwrap();
        assert_eq!(result[0].table, "main.orders");
        assert_eq!(result[0].alias, "o");
    }

    #[test]
    fn parse_tables_composite_pk() {
        let result =
            parse_tables_clause("l AS lineitem PRIMARY KEY (l_orderkey, l_linenumber)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["l_orderkey", "l_linenumber"]);
    }

    #[test]
    fn parse_tables_error_missing_as() {
        let result = parse_tables_clause("o orders PRIMARY KEY (id)", 0);
        assert!(result.is_err(), "Expected error for missing AS");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("AS"),
            "Error should mention AS: {}",
            err.message
        );
    }

    #[test]
    fn parse_tables_error_missing_primary_key() {
        let result = parse_tables_clause("o AS orders", 0);
        assert!(result.is_err(), "Expected error for missing PRIMARY KEY");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("PRIMARY KEY"),
            "Error should mention PRIMARY KEY: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // parse_relationships_clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_relationships_empty_body() {
        let result = parse_relationships_clause("", 0).unwrap();
        assert_eq!(result.len(), 0, "Empty body must return empty vec");
    }

    #[test]
    fn parse_relationships_single_entry() {
        let result =
            parse_relationships_clause("order_to_customer AS o(customer_id) REFERENCES c", 0)
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name.as_deref(), Some("order_to_customer"));
        assert_eq!(result[0].from_alias, "o");
        assert_eq!(result[0].fk_columns, vec!["customer_id"]);
        assert_eq!(result[0].table, "c");
    }

    #[test]
    fn parse_relationships_composite_fk() {
        let result = parse_relationships_clause("rel AS o(fk1, fk2) REFERENCES c", 0).unwrap();
        assert_eq!(result[0].fk_columns, vec!["fk1", "fk2"]);
    }

    #[test]
    fn parse_relationships_error_missing_name() {
        // Entry starts with "AS" — no preceding relationship name
        let result = parse_relationships_clause("AS o(customer_id) REFERENCES c", 0);
        assert!(
            result.is_err(),
            "Expected error for missing relationship name"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("name") || err.message.contains("required"),
            "Error should mention name or required: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // parse_qualified_entries tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_qualified_entries_simple() {
        let result = parse_qualified_entries("o.revenue AS SUM(amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "o"); // source_alias
        assert_eq!(result[0].1, "revenue"); // bare_name
        assert_eq!(result[0].2, "SUM(amount)"); // expr
    }

    #[test]
    fn parse_qualified_entries_nested_parens() {
        let result =
            parse_qualified_entries("o.disc_price AS SUM(l_extendedprice * (1 - l_discount))", 0)
                .unwrap();
        assert_eq!(result[0].2, "SUM(l_extendedprice * (1 - l_discount))");
    }

    #[test]
    fn parse_qualified_entries_trailing_comma() {
        let result = parse_qualified_entries("o.revenue AS SUM(amount),", 0).unwrap();
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_qualified_entries_multiple_with_trailing_comma() {
        let result = parse_qualified_entries("o.a AS x, o.b AS y,", 0).unwrap();
        assert_eq!(result.len(), 2, "Expected 2 entries, got {:?}", result);
        assert_eq!(result[0].1, "a");
        assert_eq!(result[1].1, "b");
    }

    #[test]
    fn parse_qualified_entries_error_missing_dot() {
        let result = parse_qualified_entries("revenue AS SUM(amount)", 0);
        assert!(result.is_err(), "Expected error for missing alias prefix");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("alias") || err.message.contains("qualified"),
            "Error should mention alias or qualified: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // parse_keyword_body end-to-end tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_basic() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.relationships.len(), 0);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
        assert_eq!(kb.tables[0].alias, "o");
        assert_eq!(kb.dimensions[0].name, "region");
        assert_eq!(kb.dimensions[0].source_table.as_deref(), Some("o"));
        assert_eq!(kb.metrics[0].name, "rev");
        assert_eq!(kb.metrics[0].expr, "SUM(amount)");
    }

    #[test]
    fn parse_keyword_body_with_relationships() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id), c AS customers PRIMARY KEY (id)) RELATIONSHIPS (o_to_c AS o(cust_id) REFERENCES c) DIMENSIONS (o.reg AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.relationships.len(), 1);
        assert_eq!(kb.relationships[0].name.as_deref(), Some("o_to_c"));
        assert_eq!(kb.relationships[0].from_alias, "o");
        assert_eq!(kb.relationships[0].fk_columns, vec!["cust_id"]);
        assert_eq!(kb.relationships[0].table, "c");
    }

    #[test]
    fn parse_keyword_body_empty_relationships() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) RELATIONSHIPS () DIMENSIONS (o.x AS x)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(
            kb.relationships.len(),
            0,
            "Empty RELATIONSHIPS must be empty vec"
        );
    }

    #[test]
    fn parse_keyword_body_error_missing_tables() {
        let body = "AS DIMENSIONS (o.x AS x)";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Expected error for missing TABLES clause");
    }

    // -----------------------------------------------------------------------
    // Whitespace tolerance tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_newlines_between_clauses() {
        let body = "AS\nTABLES (\n  o AS orders PRIMARY KEY (id)\n)\nDIMENSIONS (\n  o.region AS region\n)\nMETRICS (\n  o.rev AS SUM(amount)\n)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn parse_keyword_body_tabs_between_tokens() {
        let body = "AS\tTABLES\t(\to\tAS\torders\tPRIMARY\tKEY\t(id)\t)\tDIMENSIONS\t(\to.region\tAS\tregion\t)\tMETRICS\t(\to.rev\tAS\tSUM(amount)\t)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions[0].name, "region");
    }

    #[test]
    fn parse_keyword_body_extra_spaces() {
        let body = "AS  TABLES  (  o  AS  orders  PRIMARY  KEY  (  id  )  )  DIMENSIONS  (  o.region  AS  region  )  METRICS  (  o.rev  AS  SUM(amount)  )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables[0].alias, "o");
        assert_eq!(kb.tables[0].table, "orders");
        assert_eq!(kb.tables[0].pk_columns, vec!["id"]);
    }

    #[test]
    fn parse_keyword_body_mixed_whitespace_multientry() {
        // Multiple entries with newline+indent separation
        let body = "AS TABLES (\n    o AS orders PRIMARY KEY (o_id),\n    c AS customers PRIMARY KEY (c_id)\n) DIMENSIONS (\n    o.region AS region,\n    c.name AS customer_name\n) METRICS (\n    o.rev AS SUM(amount)\n)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 2);
        assert_eq!(kb.dimensions.len(), 2);
    }

    #[test]
    fn parse_tables_extra_whitespace_around_tokens() {
        // Extra whitespace inside the clause body
        let result = parse_tables_clause(
            "  o   AS   main.orders   PRIMARY   KEY   ( o_id ,  o_seq )  ",
            0,
        )
        .unwrap();
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "main.orders");
        assert_eq!(result[0].pk_columns, vec!["o_id", "o_seq"]);
    }

    #[test]
    fn parse_relationships_newline_separated() {
        let body = "\n  order_to_customer AS o(customer_id) REFERENCES c\n";
        let result = parse_relationships_clause(body, 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name.as_deref(), Some("order_to_customer"));
    }

    #[test]
    fn parse_qualified_entries_newline_separated() {
        let body = "\n  o.revenue AS SUM(amount),\n  o.count AS COUNT(*)\n";
        let result = parse_qualified_entries(body, 0).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].1, "revenue");
        assert_eq!(result[1].1, "count");
    }

    // -----------------------------------------------------------------------
    // Case insensitivity tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_lowercase_clause_keywords() {
        let body = "as tables (o AS orders primary key (id)) dimensions (o.region AS region) metrics (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions.len(), 1);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn parse_keyword_body_mixedcase_clause_keywords() {
        let body = "As Tables (o AS orders Primary Key (id)) Dimensions (o.region AS region) Metrics (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.dimensions[0].name, "region");
    }

    #[test]
    fn parse_tables_lowercase_as_and_primary_key() {
        let result = parse_tables_clause("o as orders primary key (o_id)", 0).unwrap();
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert_eq!(result[0].pk_columns, vec!["o_id"]);
    }

    #[test]
    fn parse_tables_primary_key_with_newline() {
        // PRIMARY\nKEY — newline between the two words
        let result = parse_tables_clause("o AS orders PRIMARY\nKEY (o_id)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["o_id"]);
    }

    #[test]
    fn parse_tables_primary_key_with_tab() {
        let result = parse_tables_clause("o AS orders PRIMARY\tKEY (o_id)", 0).unwrap();
        assert_eq!(result[0].pk_columns, vec!["o_id"]);
    }

    #[test]
    fn parse_relationships_lowercase_as_and_references() {
        let result =
            parse_relationships_clause("ord_to_cust as o(cust_id) references c", 0).unwrap();
        assert_eq!(result[0].name.as_deref(), Some("ord_to_cust"));
        assert_eq!(result[0].from_alias, "o");
        assert_eq!(result[0].table, "c");
    }

    #[test]
    fn parse_qualified_entries_lowercase_as() {
        let result = parse_qualified_entries("o.revenue as SUM(amount)", 0).unwrap();
        assert_eq!(result[0].1, "revenue");
        assert_eq!(result[0].2, "SUM(amount)");
    }

    #[test]
    fn parse_keyword_body_all_lowercase_full_round_trip() {
        // Fully lowercase: every keyword, operator, and token in lowercase
        let body = "as\ntables (\n    o as main.orders primary\nkey (o_id)\n)\nrelationships (\n    ord_to_cust as o(o_cust_id) references c\n)\ndimensions (\n    o.region as region\n)\nmetrics (\n    o.revenue as sum(amount)\n)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables.len(), 1);
        assert_eq!(kb.tables[0].table, "main.orders");
        assert_eq!(kb.relationships.len(), 1);
        assert_eq!(kb.relationships[0].name.as_deref(), Some("ord_to_cust"));
        assert_eq!(kb.dimensions[0].name, "region");
        assert_eq!(kb.metrics[0].expr, "sum(amount)");
    }

    // -----------------------------------------------------------------------
    // FACTS clause tests (Phase 29)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_with_facts_single() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts.len(), 1);
        assert_eq!(kb.facts[0].name, "net_price");
        assert_eq!(kb.facts[0].expr, "o.price * (1 - o.discount)");
        assert_eq!(kb.facts[0].source_table.as_deref(), Some("o"));
    }

    #[test]
    fn parse_keyword_body_with_facts_multiple() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount), o.tax_amount AS o.price * o.tax_rate) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts.len(), 2);
        assert_eq!(kb.facts[0].name, "net_price");
        assert_eq!(kb.facts[1].name, "tax_amount");
    }

    #[test]
    fn parse_keyword_body_with_facts_trailing_comma() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount),) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(
            kb.facts.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_keyword_body_with_empty_facts() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS () DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(
            kb.facts.is_empty(),
            "Empty FACTS clause must produce empty vec"
        );
    }

    #[test]
    fn parse_keyword_body_without_facts_still_works() {
        // Backward compat: DDL without FACTS clause must still work
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(kb.facts.is_empty());
        assert!(kb.hierarchies.is_empty());
    }

    #[test]
    fn parse_keyword_body_fact_without_source_table_rejected() {
        // Facts reuse parse_qualified_entries which requires alias.name format
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (net_price AS price * discount) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(
            result.is_err(),
            "Fact without source table prefix must be rejected"
        );
    }

    // -----------------------------------------------------------------------
    // HIERARCHIES clause tests (Phase 29)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_keyword_body_with_hierarchies_single() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) HIERARCHIES (geo AS (country, state, city)) DIMENSIONS (o.country AS country, o.state AS state, o.city AS city) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.hierarchies.len(), 1);
        assert_eq!(kb.hierarchies[0].name, "geo");
        assert_eq!(kb.hierarchies[0].levels, vec!["country", "state", "city"]);
    }

    #[test]
    fn parse_keyword_body_with_hierarchies_single_level() {
        // Hierarchy with just one level must be accepted
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) HIERARCHIES (simple AS (region)) DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.hierarchies.len(), 1);
        assert_eq!(kb.hierarchies[0].levels, vec!["region"]);
    }

    #[test]
    fn parse_keyword_body_with_empty_hierarchies() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) HIERARCHIES () DIMENSIONS (o.region AS region) METRICS (o.rev AS SUM(amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(
            kb.hierarchies.is_empty(),
            "Empty HIERARCHIES clause must produce empty vec"
        );
    }

    #[test]
    fn parse_keyword_body_hierarchy_without_parens_rejected() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) HIERARCHIES (geo AS country) DIMENSIONS (o.country AS country) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "Hierarchy without parens must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("'('"),
            "Error should mention '(': {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_hierarchy_with_empty_parens_rejected() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) HIERARCHIES (geo AS ()) DIMENSIONS (o.country AS country) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(
            result.is_err(),
            "Hierarchy with empty parens must be rejected"
        );
    }

    #[test]
    fn parse_keyword_body_with_facts_and_hierarchies() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (o.net_price AS o.price * (1 - o.discount)) HIERARCHIES (geo AS (country, state, city)) DIMENSIONS (o.country AS country, o.state AS state, o.city AS city) METRICS (o.rev AS SUM(net_price))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts.len(), 1);
        assert_eq!(kb.hierarchies.len(), 1);
        assert_eq!(kb.dimensions.len(), 3);
        assert_eq!(kb.metrics.len(), 1);
    }

    #[test]
    fn parse_keyword_body_facts_after_dimensions_rejected() {
        // FACTS must come before DIMENSIONS (order: tables, relationships, facts, hierarchies, dimensions, metrics)
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) FACTS (o.net_price AS price) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(
            result.is_err(),
            "FACTS after DIMENSIONS must be rejected (wrong order)"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("out of order"),
            "Error should mention out of order: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_hierarchies_after_dimensions_rejected() {
        // HIERARCHIES must come before DIMENSIONS
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) HIERARCHIES (geo AS (region)) METRICS (o.rev AS SUM(amount))";
        let result = parse_keyword_body(body, 0);
        assert!(
            result.is_err(),
            "HIERARCHIES after DIMENSIONS must be rejected (wrong order)"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("out of order"),
            "Error should mention out of order: {}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // parse_hierarchies_clause unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_hierarchies_clause_empty_body() {
        let result = parse_hierarchies_clause("", 0).unwrap();
        assert_eq!(result.len(), 0, "Empty body must return empty vec");
    }

    #[test]
    fn parse_hierarchies_clause_single() {
        let result = parse_hierarchies_clause("geo AS (country, state, city)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "geo");
        assert_eq!(result[0].levels, vec!["country", "state", "city"]);
    }

    #[test]
    fn parse_hierarchies_clause_multiple() {
        let result = parse_hierarchies_clause(
            "geo AS (country, state, city), time AS (year, quarter, month)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "geo");
        assert_eq!(result[1].name, "time");
        assert_eq!(result[1].levels, vec!["year", "quarter", "month"]);
    }

    #[test]
    fn parse_hierarchies_clause_lowercase_as() {
        let result = parse_hierarchies_clause("geo as (country, state)", 0).unwrap();
        assert_eq!(result[0].name, "geo");
        assert_eq!(result[0].levels, vec!["country", "state"]);
    }

    // -----------------------------------------------------------------------
    // parse_metrics_clause tests (Phase 30 -- derived metrics)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_clause_qualified_entry() {
        let result = parse_metrics_clause("li.revenue AS SUM(li.amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("li".to_string())); // source alias
        assert_eq!(result[0].1, "revenue"); // bare_name
        assert_eq!(result[0].2, "SUM(li.amount)"); // expr
    }

    #[test]
    fn parse_metrics_clause_unqualified_entry() {
        let result = parse_metrics_clause("profit AS revenue - cost", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, None); // no source alias (derived metric)
        assert_eq!(result[0].1, "profit"); // bare_name
        assert_eq!(result[0].2, "revenue - cost"); // expr
    }

    #[test]
    fn parse_metrics_clause_mixed_entries() {
        let result = parse_metrics_clause(
            "li.revenue AS SUM(li.amount), profit AS revenue - cost, li.cost AS SUM(li.unit_cost)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        // First: qualified
        assert_eq!(result[0].0, Some("li".to_string()));
        assert_eq!(result[0].1, "revenue");
        assert_eq!(result[0].2, "SUM(li.amount)");
        // Second: unqualified (derived)
        assert_eq!(result[1].0, None);
        assert_eq!(result[1].1, "profit");
        assert_eq!(result[1].2, "revenue - cost");
        // Third: qualified
        assert_eq!(result[2].0, Some("li".to_string()));
        assert_eq!(result[2].1, "cost");
        assert_eq!(result[2].2, "SUM(li.unit_cost)");
    }

    #[test]
    fn parse_metrics_clause_trailing_comma() {
        let result = parse_metrics_clause("profit AS revenue - cost,", 0).unwrap();
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_metrics_clause_newline_separated() {
        let result = parse_metrics_clause(
            "\n  li.revenue AS SUM(li.amount),\n  profit AS revenue - cost\n",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, Some("li".to_string()));
        assert_eq!(result[1].0, None);
        assert_eq!(result[1].1, "profit");
    }

    #[test]
    fn parse_keyword_body_with_derived_metrics() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS region) METRICS (o.revenue AS SUM(o.amount), profit AS revenue - cost)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        // First: qualified metric -> source_table: Some("o")
        assert_eq!(kb.metrics[0].name, "revenue");
        assert_eq!(kb.metrics[0].source_table.as_deref(), Some("o"));
        assert_eq!(kb.metrics[0].expr, "SUM(o.amount)");
        // Second: derived metric -> source_table: None
        assert_eq!(kb.metrics[1].name, "profit");
        assert!(kb.metrics[1].source_table.is_none());
        assert_eq!(kb.metrics[1].expr, "revenue - cost");
    }

    #[test]
    fn parse_keyword_body_only_derived_metrics() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (profit AS revenue - cost, margin AS profit / revenue)";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        assert!(kb.metrics[0].source_table.is_none());
        assert!(kb.metrics[1].source_table.is_none());
        assert_eq!(kb.metrics[0].name, "profit");
        assert_eq!(kb.metrics[1].name, "margin");
    }

    #[test]
    fn parse_qualified_entries_still_rejects_unqualified() {
        // FACTS and DIMENSIONS still use parse_qualified_entries which requires alias.name
        let result = parse_qualified_entries("revenue AS SUM(amount)", 0);
        assert!(
            result.is_err(),
            "parse_qualified_entries must still reject unqualified entries (missing dot)"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("alias") || err.message.contains("qualified"),
            "Error should mention alias or qualified: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_clause_empty_body() {
        let result = parse_metrics_clause("", 0).unwrap();
        assert_eq!(result.len(), 0, "Empty body must return empty vec");
    }
}
