//! AS-body clause scanning: split the body into `KEYWORD (...)` clause bounds.

use super::scan::QuoteState;
use crate::errors::ParseError;

/// Known clause keywords for the AS-body scanner.
const CLAUSE_KEYWORDS: &[&str] = &[
    "tables",
    "relationships",
    "facts",
    "dimensions",
    "metrics",
    "materializations",
];

/// Clause ordering — TABLES must be first, then RELATIONSHIPS (optional),
/// FACTS (optional), DIMENSIONS (optional),
/// METRICS (optional), MATERIALIZATIONS (optional).
/// At least one of DIMENSIONS or METRICS is required.
const CLAUSE_ORDER: &[&str] = &[
    "tables",
    "relationships",
    "facts",
    "dimensions",
    "metrics",
    "materializations",
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

/// Internal result of scanning a single clause from the AS-body.
pub(super) struct ClauseBound<'a> {
    pub(super) keyword: &'static str,
    pub(super) content: &'a str,      // text inside the matching parens
    pub(super) content_offset: usize, // byte offset of content[0] relative to the AS-body text
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
pub(super) fn find_clause_bounds<'a>(
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
                    "Unexpected character '{ch}' in AS body; expected a clause keyword (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS).",
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
                format!("Unknown clause keyword '{word}'; did you mean '{sug_upper}'?")
            } else {
                format!(
                    "Unknown clause keyword '{word}'; expected one of TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS.",
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

        // Find matching ')' with depth tracking, skipping quoted regions so
        // a bracket inside `'...'` or `"..."` (e.g. `o AS "tbl)x"`) cannot
        // close the clause early (PA-6).
        let content_start = i;
        let mut depth: i32 = 1;
        let mut st = QuoteState::default();
        while i < bytes.len() {
            let (next, live) = st.step(bytes, i);
            if live {
                match bytes[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            i = next;
        }

        if depth != 0 {
            let kw_upper = keyword.to_ascii_uppercase();
            // Distinguish "a quote never closed, swallowing the rest of the
            // body" from a genuinely missing ')' — the quote-aware scan
            // otherwise reports the misleading unclosed-paren error for
            // unterminated quotes.
            let message = if st.in_ident {
                format!("Unterminated quoted identifier in clause '{kw_upper}'.")
            } else if st.in_string {
                format!("Unterminated string literal in clause '{kw_upper}'.")
            } else {
                format!("Unclosed '(' for clause '{kw_upper}'.")
            };
            return Err(ParseError {
                message,
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
                    "Clause '{kw_upper}' appears out of order; clauses must appear as: TABLES, RELATIONSHIPS (optional), FACTS (optional), DIMENSIONS (optional), METRICS (optional), MATERIALIZATIONS (optional).",
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
