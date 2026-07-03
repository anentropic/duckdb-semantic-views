//! MATERIALIZATIONS clause parsing.

use super::scan::split_first_token;
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
    if entry.is_empty() {
        return Err(ParseError {
            message: "Empty entry in MATERIALIZATIONS clause.".to_string(),
            position: Some(entry_offset),
        });
    }

    // Extract name before "AS"
    let (name, rest) = split_first_token(entry);
    if name.is_empty() {
        return Err(ParseError {
            message: "Expected materialization name in MATERIALIZATIONS entry.".to_string(),
            position: Some(entry_offset),
        });
    }
    let rest = rest.trim();

    // Expect "AS"
    if !rest.get(..2).is_some_and(|s| s.eq_ignore_ascii_case("AS")) {
        return Err(ParseError {
            message: format!(
                "Expected 'AS' after materialization name '{name}' in MATERIALIZATIONS clause."
            ),
            position: Some(entry_offset + name.len()),
        });
    }
    let after_as = rest[2..].trim();

    // Expect parenthesized sub-body: (TABLE ..., DIMENSIONS (...), METRICS (...))
    if !after_as.starts_with('(') {
        return Err(ParseError {
            message: format!("Expected '(' after 'AS' for materialization '{name}'."),
            position: None,
        });
    }
    // Find matching closing paren
    let sub_body = extract_paren_body(after_as).ok_or_else(|| ParseError {
        message: format!("Unclosed '(' for materialization '{name}'."),
        position: None,
    })?;

    // Parse sub-body keywords: TABLE, DIMENSIONS, METRICS
    let mut table_name: Option<String> = None;
    let mut dim_names: Vec<String> = Vec::new();
    let mut met_names: Vec<String> = Vec::new();

    // Scan for keyword positions (case-insensitive)
    let sub_upper = sub_body.to_ascii_uppercase();
    let kw_positions = find_sub_keyword_positions(&sub_upper);

    for (i, &(kw, start)) in kw_positions.iter().enumerate() {
        let end = if i + 1 < kw_positions.len() {
            kw_positions[i + 1].1
        } else {
            sub_body.len()
        };
        let content = sub_body[start + kw.len()..end].trim();
        // Strip trailing comma
        let content = content.strip_suffix(',').unwrap_or(content).trim();

        match kw {
            "TABLE" => {
                if content.is_empty() {
                    return Err(ParseError {
                        message: format!(
                            "Materialization '{name}': TABLE sub-clause has no table name."
                        ),
                        position: None,
                    });
                }
                table_name = Some(content.to_string());
            }
            "DIMENSIONS" => {
                dim_names = extract_paren_list(content)?;
            }
            "METRICS" => {
                met_names = extract_paren_list(content)?;
            }
            _ => {}
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

    Ok(Materialization {
        name: name.to_string(),
        table,
        dimensions: dim_names,
        metrics: met_names,
    })
}

/// Extract content between matching outer parentheses.
/// Returns the content inside `(...)` (excluding the parens), or None if unmatched.
fn extract_paren_body(s: &str) -> Option<&str> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find positions of TABLE, DIMENSIONS, METRICS keywords in the uppercased sub-body.
/// Returns (keyword, `byte_offset`) pairs sorted by position.
fn find_sub_keyword_positions(upper: &str) -> Vec<(&'static str, usize)> {
    let keywords: &[&str] = &["TABLE", "DIMENSIONS", "METRICS"];
    let mut positions = Vec::new();
    for &kw in keywords {
        let mut search_from = 0;
        while let Some(pos) = upper[search_from..].find(kw) {
            let abs_pos = search_from + pos;
            // Only match at word boundary (not part of a longer identifier)
            let before_ok = abs_pos == 0 || {
                let b = upper.as_bytes()[abs_pos - 1];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            let after_pos = abs_pos + kw.len();
            let after_ok = after_pos >= upper.len() || {
                let b = upper.as_bytes()[after_pos];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            if before_ok && after_ok {
                positions.push((kw, abs_pos));
            }
            search_from = abs_pos + kw.len();
        }
    }
    positions.sort_by_key(|&(_, pos)| pos);
    positions
}

/// Extract a parenthesized comma-separated name list: `(name1, name2, ...)`.
/// Strips whitespace from each name.
fn extract_paren_list(content: &str) -> Result<Vec<String>, ParseError> {
    let content = content.trim();
    if content.is_empty() {
        return Ok(Vec::new());
    }
    let inner = if content.starts_with('(') {
        extract_paren_body(content).ok_or_else(|| ParseError {
            message: "Unclosed parenthesis in MATERIALIZATIONS sub-clause.".to_string(),
            position: None,
        })?
    } else {
        content
    };
    Ok(split_at_depth0_commas(inner)
        .into_iter()
        .map(|(_, entry)| entry.to_string())
        .collect())
}
