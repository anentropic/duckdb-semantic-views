//! Trailing COMMENT / WITH SYNONYMS annotations and leading access modifiers.

use super::scan::{extract_paren_prefix, is_ident_continuation, QuoteState};
use super::split_at_depth0_commas;
use crate::errors::ParseError;
use crate::model::AccessModifier;

/// Trailing metadata annotations parsed from a DDL entry.
/// Used internally to collect COMMENT and SYNONYMS from entry text.
#[derive(Debug, Default)]
pub(super) struct ParsedAnnotations {
    pub(super) comment: Option<String>,
    pub(super) synonyms: Vec<String>,
}

/// Extract a single-quoted string value, handling '' escape sequences.
/// Input starts with the opening quote: 'text here'
/// Returns the unescaped string content. Text after the closing quote is
/// ignored (COMMENT extraction hands this function the whole annotation
/// tail, e.g. `'x' WITH SYNONYMS = (...)`).
///
/// Thin adapter over the shared UTF-8-correct extractor: this used to be
/// the byte-wise copy that the WR-04 fix missed, Latin-1-izing every
/// non-ASCII codepoint in COMMENT/SYNONYMS payloads (PA-2, code-review
/// 2026-07-02).
fn extract_single_quoted_string(s: &str) -> Result<String, ParseError> {
    match crate::util::extract_single_quoted_prefix(s) {
        Ok((content, _consumed)) => Ok(content),
        Err(crate::util::SingleQuoteError::NotQuoted) => Err(ParseError {
            message: "Expected single-quoted string.".to_string(),
            position: None,
        }),
        Err(crate::util::SingleQuoteError::Unterminated) => Err(ParseError {
            message: "Unclosed single-quoted string.".to_string(),
            position: None,
        }),
    }
}

/// Parse comma-separated single-quoted strings from inside parentheses.
/// Input: "'syn1', 'syn2'" (already extracted from parens)
fn parse_synonym_list(content: &str) -> Result<Vec<String>, ParseError> {
    let entries = split_at_depth0_commas(content);
    let mut result = Vec::new();
    for (_, entry) in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        result.push(extract_single_quoted_string(trimmed)?);
    }
    Ok(result)
}

/// Separate the SQL expression from trailing COMMENT / WITH SYNONYMS annotations.
///
/// Input: the text after "AS" in an entry (e.g., "SUM(o.amount) COMMENT = 'test' WITH SYNONYMS = ('a')")
/// Output: (`clean_expression`, `ParsedAnnotations`)
///
/// Handles:
/// - COMMENT = 'string with ''escaped'' quotes'
/// - WITH SYNONYMS = ('syn1', 'syn2')
/// - Either order (COMMENT then SYNONYMS or vice versa)
/// - No annotations at all (returns original expression with empty annotations)
/// - COMMENT as an identifier inside expressions (only matches at depth-0 with word boundaries)
///
/// Once the annotation region begins, it must be tiled exactly by recognized
/// clauses separated by whitespace: a duplicate `COMMENT` / `WITH SYNONYMS`, a
/// malformed clause, or any leftover text is an error (P-2) — none of it is
/// silently dropped.
#[allow(clippy::too_many_lines)]
pub(super) fn parse_trailing_annotations(
    text: &str,
) -> Result<(String, ParsedAnnotations), ParseError> {
    let text = text.trim();
    let upper = text.to_ascii_uppercase();

    // Find the FIRST occurrence of COMMENT or WITH SYNONYMS at depth-0 with word boundaries.
    // Scan forward tracking depth to find annotation region start. Quote-aware
    // (PA-6/PA-9): keyword text inside `'...'` string literals or `"..."`
    // quoted identifiers does not match — a column literally named `comment`
    // is usable at depth 0 when quoted (`o."comment"`).
    let mut depth: i32 = 0;
    let mut st = QuoteState::default();
    let bytes = text.as_bytes();
    let upper_bytes = upper.as_bytes();
    let mut annotation_start: Option<usize> = None;
    let mut i = 0;

    while i < bytes.len() {
        let (next, live) = st.step(bytes, i);
        if live {
            match bytes[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }

            // At depth 0, outside quoted regions, check for COMMENT or WITH keyword
            if depth == 0 {
                // Check for COMMENT keyword with word boundaries
                if i + 7 <= bytes.len() && &upper_bytes[i..i + 7] == b"COMMENT" {
                    let before_ok = i == 0 || !is_ident_continuation(bytes[i - 1]);
                    let after_ok = i + 7 == bytes.len() || !is_ident_continuation(bytes[i + 7]);
                    if before_ok && after_ok && annotation_start.is_none() {
                        annotation_start = Some(i);
                    }
                }
                // Check for WITH keyword (for WITH SYNONYMS)
                if i + 4 <= bytes.len() && &upper_bytes[i..i + 4] == b"WITH" {
                    let before_ok = i == 0 || !is_ident_continuation(bytes[i - 1]);
                    let after_ok = i + 4 == bytes.len() || !is_ident_continuation(bytes[i + 4]);
                    if before_ok && after_ok {
                        // Verify it's WITH SYNONYMS, not just any WITH
                        let after_with = upper[i + 4..].trim_start();
                        if after_with.starts_with("SYNONYMS") && annotation_start.is_none() {
                            annotation_start = Some(i);
                        }
                    }
                }
            }
        }
        i = next;
    }

    let (expr_text, annotation_text) = if let Some(start) = annotation_start {
        (text[..start].trim(), &text[start..])
    } else {
        return Ok((text.to_string(), ParsedAnnotations::default()));
    };

    // Parse the annotation region as a sequence of clauses that must TILE it:
    // each is `COMMENT = '...'` or `WITH SYNONYMS = (...)`, separated only by
    // whitespace. A duplicate clause, a malformed clause, or ANY leftover
    // non-whitespace text is a hard error rather than being silently discarded
    // (P-2, code-review 2026-07-11). Previously only the FIRST COMMENT / first
    // WITH SYNONYMS was read: a second `COMMENT = '...'` was dropped and
    // trailing junk (`COMMENT = 'a' banana`) was accepted.
    let mut comment: Option<String> = None;
    let mut synonyms: Option<Vec<String>> = None;
    let mut rest = annotation_text;

    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let rest_upper = rest.to_ascii_uppercase();

        if starts_with_keyword(&rest_upper, "COMMENT") {
            if comment.is_some() {
                return Err(ParseError {
                    message: "Duplicate COMMENT annotation.".to_string(),
                    position: None,
                });
            }
            // `COMMENT` is 7 ASCII bytes, so slicing at 7 is on a char boundary.
            let Some(after_eq) = rest[7..].trim_start().strip_prefix('=') else {
                return Err(ParseError {
                    message: "Expected '=' after COMMENT keyword.".to_string(),
                    position: None,
                });
            };
            let after_eq = after_eq.trim_start();
            if !after_eq.starts_with('\'') {
                return Err(ParseError {
                    message: "Expected single-quoted string after COMMENT =.".to_string(),
                    position: None,
                });
            }
            let (content, consumed) =
                crate::util::extract_single_quoted_prefix(after_eq).map_err(|e| ParseError {
                    message: match e {
                        crate::util::SingleQuoteError::NotQuoted => {
                            "Expected single-quoted string after COMMENT =.".to_string()
                        }
                        crate::util::SingleQuoteError::Unterminated => {
                            "Unclosed single-quoted string.".to_string()
                        }
                    },
                    position: None,
                })?;
            comment = Some(content);
            rest = &after_eq[consumed..];
        } else if starts_with_keyword(&rest_upper, "WITH") {
            if synonyms.is_some() {
                return Err(ParseError {
                    message: "Duplicate WITH SYNONYMS annotation.".to_string(),
                    position: None,
                });
            }
            // `WITH` is 4 ASCII bytes.
            let after_with = rest[4..].trim_start();
            if !starts_with_keyword(&after_with.to_ascii_uppercase(), "SYNONYMS") {
                return Err(ParseError {
                    message: "Expected SYNONYMS after WITH keyword.".to_string(),
                    position: None,
                });
            }
            // `SYNONYMS` is 8 ASCII bytes. Snowflake makes the `=` optional, so
            // both `WITH SYNONYMS = (...)` and `WITH SYNONYMS (...)` are
            // accepted (F-12, code-review 2026-07-16).
            let after_synonyms = after_with[8..].trim_start();
            let after_eq = after_synonyms
                .strip_prefix('=')
                .unwrap_or(after_synonyms)
                .trim_start();
            let (content, consumed) = extract_paren_prefix(after_eq).ok_or_else(|| ParseError {
                message: "Expected parenthesized list after WITH SYNONYMS.".to_string(),
                position: None,
            })?;
            synonyms = Some(parse_synonym_list(content)?);
            rest = &after_eq[consumed..];
        } else {
            return Err(ParseError {
                message: format!(
                    "Unexpected text in annotations: '{rest}'. Expected COMMENT = '...' or WITH SYNONYMS = (...)."
                ),
                position: None,
            });
        }
    }

    Ok((
        expr_text.to_string(),
        ParsedAnnotations {
            comment,
            synonyms: synonyms.unwrap_or_default(),
        },
    ))
}

/// True when `upper` (already ASCII-uppercased) begins with `keyword` (also
/// uppercase) at a word boundary — i.e. the byte after the keyword, if any, is
/// not an identifier-continuation byte. Prevents `COMMENTARY` from matching
/// `COMMENT` / `WITHDRAW` from matching `WITH`.
fn starts_with_keyword(upper: &str, keyword: &str) -> bool {
    let ub = upper.as_bytes();
    let kb = keyword.as_bytes();
    ub.len() >= kb.len()
        && &ub[..kb.len()] == kb
        && (ub.len() == kb.len() || !is_ident_continuation(ub[kb.len()]))
}

/// Check for a leading PRIVATE or PUBLIC keyword on an entry.
/// Returns (`AccessModifier`, `remaining_entry_text`).
/// Disambiguates table aliases starting with "private" or "public" by checking
/// if the next non-whitespace character is '.' (indicating a qualified identifier).
pub(super) fn parse_leading_access_modifier(entry: &str) -> (AccessModifier, &str) {
    let entry_upper = entry.to_ascii_uppercase();
    if entry_upper.starts_with("PRIVATE") {
        let after = &entry["PRIVATE".len()..];
        if after.starts_with(|c: char| c.is_ascii_whitespace()) {
            let trimmed_after = after.trim_start();
            if trimmed_after.starts_with('.') {
                // 'PRIVATE' is a table alias like private_table.metric
                (AccessModifier::Public, entry)
            } else {
                (AccessModifier::Private, trimmed_after)
            }
        } else if after.is_empty() {
            (AccessModifier::Private, after)
        } else {
            // e.g., "PRIVATEMETRIC" or "PRIVATE.x" -- not a keyword
            (AccessModifier::Public, entry)
        }
    } else if entry_upper.starts_with("PUBLIC") {
        let after = &entry["PUBLIC".len()..];
        if after.starts_with(|c: char| c.is_ascii_whitespace()) {
            let trimmed_after = after.trim_start();
            if trimmed_after.starts_with('.') {
                (AccessModifier::Public, entry)
            } else {
                (AccessModifier::Public, trimmed_after)
            }
        } else if after.is_empty() {
            (AccessModifier::Public, after)
        } else {
            (AccessModifier::Public, entry)
        }
    } else {
        (AccessModifier::Public, entry)
    }
}
