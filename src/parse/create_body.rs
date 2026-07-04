//! CREATE-body parsing for semantic-view DDL (AR-1).
//!
//! Turns the text after `CREATE SEMANTIC VIEW <name>` into a structured
//! [`RewriteAction`]. Three body forms are handled:
//!   - `AS TABLES (...) DIMENSIONS (...) METRICS (...)` — the keyword body,
//!     parsed via [`crate::body_parser::parse_keyword_body`].
//!   - `FROM YAML $$ ... $$` — inline dollar-quoted YAML.
//!   - `FROM YAML FILE '<path>'` — a file reference resolved at execution.
//!
//! An optional view-level `COMMENT = '...'` between the name and the body is
//! also extracted here. Cardinality inference runs on the parsed definition
//! before it is carried structurally to the emission stage.
//!
//! `validate_create_body` is the entry point (called by `plan_rewrite`); the
//! remaining functions are cluster-internal. The quote/body helpers are
//! re-exported under `#[cfg(test)]` so the parent module's test suite can
//! exercise them directly.

use super::{CreateMode, DdlKind, RewriteAction};
use crate::body_parser::parse_keyword_body;
use crate::errors::ParseError;
use crate::ident::{find_identifier_end, normalize_view_name};
use crate::util::{extract_single_quoted_prefix, is_ident_byte, SingleQuoteError};

/// Extract an optional COMMENT = '...' between the view name and the AS keyword.
/// Returns (`comment_option`, `remaining_text_after_comment`).
///
/// Phase 43: Supports `CREATE SEMANTIC VIEW my_view COMMENT = 'desc' AS ...`
pub(crate) fn extract_view_comment(text: &str) -> Result<(Option<String>, &str), ParseError> {
    let upper = text.to_ascii_uppercase();
    if upper.starts_with("COMMENT") {
        // Verify word boundary (not e.g. COMMENTARY, COMMENT_x, COMMENTé —
        // `_` and non-ASCII bytes are identifier continuation).
        if text.len() > 7 && is_ident_byte(text.as_bytes()[7]) {
            return Ok((None, text));
        }
        let after_kw = text[7..].trim_start();
        if !after_kw.starts_with('=') {
            return Err(ParseError {
                message: "Expected '=' after COMMENT keyword.".to_string(),
                position: None,
            });
        }
        let after_eq = after_kw[1..].trim_start();
        if !after_eq.starts_with('\'') {
            return Err(ParseError {
                message: "Expected single-quoted string after COMMENT =.".to_string(),
                position: None,
            });
        }
        // Extract the quoted string handling '' escaping.
        //
        // Phase 65.1 WR-04: walk as `char` stream (UTF-8 scalar values)
        // rather than raw bytes — the previous `bytes[i] as char` cast
        // silently mangled non-ASCII characters in the comment body.
        // We skip the opening quote (one ASCII byte) and iterate from
        // byte offset 1 via `char_indices()` against `&after_eq[1..]`,
        // adjusting reported offsets back to `after_eq` space.
        let mut chars = after_eq[1..].char_indices();
        let mut value = String::new();
        while let Some((rel_i, ch)) = chars.next() {
            if ch == '\'' {
                let mut peek = chars.clone();
                if matches!(peek.next(), Some((_, '\''))) {
                    value.push('\'');
                    chars = peek;
                    continue;
                }
                // Closing quote found. `rel_i` is offset into `after_eq[1..]`;
                // absolute offset of the closing quote in `after_eq` is
                // `rel_i + 1`; the slice after the closing quote starts at
                // `rel_i + 2` (closing quote is one ASCII byte).
                let remaining = &after_eq[rel_i + 2..];
                return Ok((Some(value), remaining));
            }
            value.push(ch);
        }
        Err(ParseError {
            message: "Unclosed single-quoted string in view-level COMMENT.".to_string(),
            position: None,
        })
    } else {
        Ok((None, text))
    }
}

/// Validate a CREATE-with-body DDL statement and rewrite it if valid.
pub(crate) fn validate_create_body(
    _query: &str,
    trimmed_no_semi: &str,
    trim_offset: usize,
    plen: usize,
    kind: DdlKind,
) -> Result<Option<RewriteAction>, ParseError> {
    let after_prefix = trimmed_no_semi[plen..].trim_start();
    if after_prefix.is_empty() {
        return Err(ParseError {
            message: "Missing view name after DDL prefix.".to_string(),
            position: Some(trim_offset + plen),
        });
    }

    // Quote-aware delimiter scan; honours `"..."` regions so quoted/FQN forms
    // like `"db"."sch"."v"` or `"my view"` are captured intact before
    // normalisation. allow_paren=true: the CREATE form may legally have a `(`
    // for legacy paren-body callers (the AS-keyword body path is the main one
    // today, but `(` remains a safe terminator).
    let name_end = find_identifier_end(after_prefix, true);
    let raw_name = &after_prefix[..name_end];
    if raw_name.is_empty() {
        return Err(ParseError {
            message: "Missing view name after DDL prefix.".to_string(),
            position: Some(trim_offset + plen),
        });
    }
    let name_owned = normalize_view_name(raw_name).map_err(|e| ParseError {
        message: format!("Invalid view name: {e}"),
        position: Some(trim_offset + plen),
    })?;
    let name = name_owned.as_str();

    let after_name = &after_prefix[name_end..];

    // --- Phase 43: View-level COMMENT extraction ---
    // Extract optional COMMENT = '...' between the view name and the AS keyword.
    let after_name_pre = after_name.trim_start();
    let (view_comment, remaining_after_comment) = extract_view_comment(after_name_pre)?;

    // --- AS keyword body path (new in Phase 25) ---
    // If text after the name starts with "AS" (whitespace-delimited), route to the
    // AS-body keyword parser instead of the legacy paren-body path.
    let after_name_trimmed = remaining_after_comment.trim_start();
    let is_as_body = after_name_trimmed
        .get(..2)
        .is_some_and(|s| s.eq_ignore_ascii_case("AS"))
        && (after_name_trimmed.len() == 2
            || after_name_trimmed.as_bytes()[2].is_ascii_whitespace());
    if is_as_body {
        // Compute the byte offset of after_name_trimmed[0] within trimmed_no_semi.
        // after_prefix starts at: plen + whitespace-gap between trimmed_no_semi[plen..] and after_prefix
        let after_prefix_in_tns = plen + (trimmed_no_semi.len() - plen - after_prefix.len());
        // after_name starts at name_end within after_prefix
        let after_name_in_tns = after_prefix_in_tns + name_end;
        // Calculate the byte offset of after_name_trimmed relative to trimmed_no_semi
        // after_name_trimmed is a slice within after_name, so compute by pointer arithmetic
        let trimmed_start_in_after_name = after_name.len() - remaining_after_comment.len()
            + (remaining_after_comment.len() - after_name_trimmed.len());
        let body_offset_in_tns = after_name_in_tns + trimmed_start_in_after_name;
        let body_offset = trim_offset + body_offset_in_tns;
        return rewrite_ddl_keyword_body(kind, name, after_name_trimmed, body_offset, view_comment)
            .map(Some);
    }
    // --- End AS keyword body path ---

    // --- FROM YAML body path (Phase 52 + Phase 53) ---
    let is_yaml_body = after_name_trimmed
        .get(..9)
        .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
        && (after_name_trimmed.len() == 9
            || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());
    if is_yaml_body {
        let yaml_text = after_name_trimmed[9..].trim_start();

        // Phase 53: FROM YAML FILE '/path' sub-branch
        let is_file = yaml_text
            .get(..4)
            .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
            && (yaml_text.len() == 4 || yaml_text.as_bytes()[4].is_ascii_whitespace());
        if is_file {
            let file_text = yaml_text[4..].trim_start();
            return rewrite_ddl_yaml_file_body(kind, name, file_text, view_comment).map(Some);
        }

        // Phase 52: FROM YAML $$...$$ inline sub-branch (existing)
        return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment).map(Some);
    }
    // --- End FROM YAML body path ---

    // Non-AS/FROM-YAML syntax rejected -- AS keyword or FROM YAML required after view name.
    let pos_in_trimmed = plen + (trimmed_no_semi.len() - plen - after_prefix.len()) + name_end;
    Err(ParseError {
        message: "Expected 'AS' or 'FROM YAML' after view name. Use: CREATE SEMANTIC VIEW name \
                  AS TABLES (...) DIMENSIONS (...) METRICS (...) or: CREATE SEMANTIC VIEW name \
                  FROM YAML $$ ... $$ or: CREATE SEMANTIC VIEW name FROM YAML FILE \
                  '/path/to/file.yaml'"
            .to_string(),
        position: Some(trim_offset + pos_in_trimmed),
    })
}

/// Parse an AS-body CREATE statement into a [`RewriteAction::Create`].
///
/// Called when `validate_create_body` detects the `AS` keyword path. Parses the
/// keyword body via `parse_keyword_body`, infers cardinality, and carries the
/// resulting `SemanticViewDefinition` structurally to the emission stage.
fn rewrite_ddl_keyword_body(
    kind: DdlKind,
    name: &str,
    body_text: &str,              // text starting at "AS" (inclusive)
    body_offset: usize,           // byte offset of body_text[0] in original query
    view_comment: Option<String>, // Phase 43: optional view-level COMMENT
) -> Result<RewriteAction, ParseError> {
    // 1. Call parse_keyword_body (body_text starts at "AS"; pass body_offset)
    let mut keyword_body = parse_keyword_body(body_text, body_offset)?;

    // Phase 33: Infer cardinality and resolve ref_columns.
    crate::graph::infer_cardinality(&keyword_body.tables, &mut keyword_body.relationships)?;

    // 2. Construct SemanticViewDefinition from KeywordBody
    let def = crate::model::SemanticViewDefinition {
        tables: keyword_body.tables,
        dimensions: keyword_body.dimensions,
        metrics: keyword_body.metrics,
        joins: keyword_body.relationships,
        facts: keyword_body.facts,
        materializations: keyword_body.materializations,
        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: view_comment,
    };

    // 3. Carry the definition structurally — `rewrite_to_native_sql` hands it
    //    straight to `emit_native_create_sql` (AR-2: no JSON serialize / re-parse
    //    / re-deserialize round-trip).
    Ok(RewriteAction::Create {
        name: name.to_string(),
        def: Box::new(def),
        mode: CreateMode::from_kind(kind),
    })
}

// ---------------------------------------------------------------------------
// Phase 53: Single-quoted file path extraction and YAML FILE sentinel
// ---------------------------------------------------------------------------

/// Extract a single-quoted string literal from the input.
///
/// Returns `(unescaped_content, bytes_consumed)` on success.
/// Handles SQL-standard escaped single quotes (`''` -> `'`).
///
/// Thin adapter over the shared UTF-8-correct extractor (ST-4 consolidation;
/// originally fixed here as Phase 65.1 WR-04) mapping errors to this call
/// site's FILE-path message wording.
pub(crate) fn extract_single_quoted(input: &str) -> Result<(String, usize), ParseError> {
    extract_single_quoted_prefix(input).map_err(|e| ParseError {
        message: match e {
            SingleQuoteError::NotQuoted => "Expected single-quoted file path after FILE keyword. \
                 Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            SingleQuoteError::Unterminated => {
                "Unterminated file path string (missing closing single quote)".to_string()
            }
        },
        position: None,
    })
}

/// Parse a `FROM YAML FILE '<path>'` body into a structured
/// [`RewriteAction::CreateFromYamlFile`].
///
/// `rewrite_to_native_sql` hands the fields straight to
/// `emit_native_create_from_yaml_file`, which emits an INSERT selecting from the
/// `__sv_compute_create_from_yaml` helper TF (that reads the file at execution).
/// AR-2 replaced the previous `\x01`-delimited sentinel string (which smuggled
/// path/kind/name/comment through the `String` return of `validate_and_rewrite`).
pub(crate) fn rewrite_ddl_yaml_file_body(
    kind: DdlKind,
    name: &str,
    file_text: &str,
    view_comment: Option<String>,
) -> Result<RewriteAction, ParseError> {
    let (file_path, consumed) = extract_single_quoted(file_text)?;

    let trailing = file_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after file path: '{trailing}'"),
            position: None,
        });
    }

    if file_path.is_empty() {
        return Err(ParseError {
            message: "File path cannot be empty. \
                      Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            position: None,
        });
    }

    Ok(RewriteAction::CreateFromYamlFile {
        file_path,
        name: name.to_string(),
        comment: view_comment.unwrap_or_default(),
        mode: CreateMode::from_kind(kind),
    })
}

// ---------------------------------------------------------------------------
// Phase 52: Dollar-quote extraction and YAML DDL rewrite
// ---------------------------------------------------------------------------

/// Extract content from a dollar-quoted string (`$$...$$` or `$tag$...$tag$`).
///
/// Returns `(content, bytes_consumed)` where `bytes_consumed` includes both
/// opening and closing delimiters. The content does NOT include the delimiters.
pub(crate) fn extract_dollar_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('$') {
        return Err(ParseError {
            message: "Expected '$' to begin dollar-quoted string".to_string(),
            position: None,
        });
    }
    let tag_end = input[1..].find('$').ok_or_else(|| ParseError {
        message: "Unterminated dollar-quote opening delimiter".to_string(),
        position: None,
    })? + 2;
    let delimiter = &input[..tag_end];
    let content_start = tag_end;
    let close_pos = input[content_start..]
        .find(delimiter)
        .ok_or_else(|| ParseError {
            message: format!("Unterminated dollar-quoted string (expected closing '{delimiter}')"),
            position: None,
        })?;
    let content = &input[content_start..content_start + close_pos];
    let total = content_start + close_pos + delimiter.len();
    Ok((content.to_string(), total))
}

/// Parse a `FROM YAML $$..$$` dollar-quoted CREATE body into a
/// [`RewriteAction::Create`].
///
/// Called when `validate_create_body` detects the `FROM YAML` keyword path.
/// Extracts dollar-quoted YAML, deserializes via `from_yaml_with_size_cap()`,
/// infers cardinality, and carries the `SemanticViewDefinition` structurally.
pub(crate) fn rewrite_ddl_yaml_body(
    kind: DdlKind,
    name: &str,
    yaml_text: &str,
    view_comment: Option<String>,
) -> Result<RewriteAction, ParseError> {
    let (yaml_content, consumed) = extract_dollar_quoted(yaml_text)?;

    let trailing = yaml_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after closing dollar-quote: '{trailing}'"),
            position: None,
        });
    }

    let mut def =
        crate::model::SemanticViewDefinition::from_yaml_with_size_cap(name, &yaml_content)
            .map_err(|e| ParseError {
                message: e,
                position: None,
            })?;

    if let Some(c) = view_comment {
        def.comment = Some(c);
    }

    crate::graph::infer_cardinality(&def.tables, &mut def.joins)?;

    Ok(RewriteAction::Create {
        name: name.to_string(),
        def: Box::new(def),
        mode: CreateMode::from_kind(kind),
    })
}
