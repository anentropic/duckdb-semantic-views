//! METRICS clause parsing, including USING and NON ADDITIVE BY.

use super::annotations::{parse_leading_access_modifier, parse_trailing_annotations};
use super::scan::{
    extract_paren_content, find_keyword_ci, find_live_byte, is_ident_continuation,
    is_quoting_balanced, unterminated_quote_error,
};
use super::window::{parse_order_by_modifiers, parse_window_over_clause, OrderModifierContext};
use super::{split_at_depth0_commas, MetricEntry};
use crate::errors::ParseError;
use crate::ident::find_identifier_end;
use crate::model::NonAdditiveDim;

/// Parse the content inside METRICS (...) supporting both qualified and unqualified entries.
///
/// Qualified entries have the form: `alias.name AS expr` (base metric).
/// Qualified entries may include: `alias.name USING (rel1, rel2) AS expr` (Phase 32).
/// Unqualified entries have the form: `name AS expr` (derived metric).
///
/// Returns `Vec<(Option<source_alias>, bare_name, expr, using_relationships, comment, synonyms, access)>` where:
/// - Option is `Some(alias)` for qualified entries and `None` for unqualified (derived) entries
/// - `using_relationships` is a `Vec<String>` of named relationships (empty if no USING clause)
/// - Phase 43: comment, synonyms, and access modifier are parsed from trailing annotations and leading keyword
#[allow(clippy::type_complexity)]
pub(crate) fn parse_metrics_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<MetricEntry>, ParseError> {
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

/// Find the keyword sequence "NON ADDITIVE BY" with word boundaries.
/// Returns `(start, end)` byte offsets — `start` at 'N' of NON, `end` one
/// past 'Y' of BY. Returning the real end kills the hardcoded
/// `start + 16` slice (PA-10, code-review 2026-07-02): it assumed exactly
/// one space between the keywords, rejecting `NON  ADDITIVE BY` and the
/// no-space `BY(d)` form.
fn find_non_additive_by_keyword(upper_text: &str) -> Option<(usize, usize)> {
    let mut search_from = 0;
    while let Some(pos) = find_keyword_ci(&upper_text[search_from..], "NON") {
        let abs_pos = search_from + pos;
        let after_non = upper_text[abs_pos + 3..].trim_start();
        if let Some(rest) = after_non.strip_prefix("ADDITIVE") {
            let after_additive = rest.trim_start();
            if let Some(after_by) = after_additive.strip_prefix("BY") {
                // Verify BY has a word boundary: `_` and non-ASCII bytes
                // continue an identifier (BY_foo is not the keyword BY).
                if after_by.is_empty() || !is_ident_continuation(after_by.as_bytes()[0]) {
                    // `after_by` is a suffix slice of `upper_text`, so the
                    // offset one past "BY" falls out of the lengths.
                    let end = upper_text.len() - after_by.len();
                    return Some((abs_pos, end));
                }
            }
        }
        search_from = abs_pos + 3;
    }
    None
}

/// Parse the dimension entries inside a NON ADDITIVE BY (...) clause.
/// Each entry: `dim_name [ASC|DESC] [NULLS FIRST|LAST]`
///
/// Phase 68 Plan 03 (B1) / TECH-DEBT #25: `dim_name` is captured via
/// identifier-aware tokenisation (`find_identifier_end`) so quoted identifiers
/// containing literal whitespace AND dotted paths (`table.col`, D-08) survive
/// intact. The modifier suffix (`ASC|DESC|NULLS FIRST|LAST`) is then
/// `split_whitespace`-tokenised since the suffix has no quoted identifiers.
fn parse_non_additive_dims(
    content: &str,
    base_offset: usize,
) -> Result<Vec<NonAdditiveDim>, ParseError> {
    let entries = split_at_depth0_commas(content);
    let mut result = Vec::new();
    for (start, entry_text) in entries {
        let entry_text = entry_text.trim();
        if entry_text.is_empty() {
            continue; // trailing comma
        }
        // Phase 68 B1: identifier-aware capture of dim_name. `allow_paren=false`
        // because NAB entries have no parens inside identifiers.
        let name_end = find_identifier_end(entry_text, /* allow_paren = */ false);
        if name_end == 0 {
            return Err(ParseError {
                message: "Empty dimension in NON ADDITIVE BY clause".to_string(),
                position: Some(base_offset + start),
            });
        }
        // Phase 68 B1: reject unterminated quoted identifiers — mirrors the
        // TABLES-clause A4 check. `find_identifier_end` saturates at
        // `input.len()` on an unterminated `"`, so without this guard the
        // malformed name would flow downstream.
        if !is_quoting_balanced(&entry_text[..name_end]) {
            return Err(ParseError {
                message: format!(
                    "Unterminated quoted identifier in NON ADDITIVE BY dimension entry '{entry_text}'."
                ),
                position: Some(base_offset + start),
            });
        }
        let dim_name = entry_text[..name_end].trim().to_string();
        let suffix = entry_text[name_end..].trim();
        let parts: Vec<&str> = suffix.split_whitespace().collect();
        let (order, nulls) = parse_order_by_modifiers(
            &parts,
            OrderModifierContext::NonAdditiveBy,
            base_offset + start,
        )?;
        result.push(NonAdditiveDim {
            dimension: dim_name,
            order,
            nulls,
        });
    }
    Ok(result)
}

/// Parse one METRICS entry: either `alias.name [USING (...)] [NON ADDITIVE BY (...)] AS expr` (qualified)
/// or `name AS expr` (derived).
///
/// Phase 32: If a USING clause is present, it must be on a qualified entry (has dot).
/// USING on a derived metric (no dot) produces a `ParseError`.
/// Phase 47: If a NON ADDITIVE BY clause is present, it must be on a qualified entry (has dot).
/// Phase 48: If an OVER clause is present, it must be on a qualified entry (has dot).
#[allow(clippy::too_many_lines)]
fn parse_single_metric_entry(entry: &str, entry_offset: usize) -> Result<MetricEntry, ParseError> {
    let entry = entry.trim();

    // Unterminated quoting swallows the rest of the entry under the
    // quote-aware scanners — reject it up front with a precise error.
    if let Some(noun) = unterminated_quote_error(entry) {
        return Err(ParseError {
            message: format!("{noun} in metric entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Phase 43: Check for leading PRIVATE/PUBLIC keyword
    let (access, entry_after_access) = parse_leading_access_modifier(entry);

    // Check if entry contains a dot BEFORE the AS keyword -- if so, it's qualified.
    // Find "AS" keyword first (case-insensitive, word boundary).
    let upper = entry_after_access.to_ascii_uppercase();
    let as_pos = find_keyword_ci(&upper, "AS").ok_or_else(|| ParseError {
        message: format!(
            "Expected 'AS' keyword in metric entry '{entry}'. Form: 'alias.name AS expr' or 'name AS expr'.",
        ),
        position: Some(entry_offset),
    })?;

    let before_as = entry_after_access[..as_pos].trim();
    let raw_expr = entry_after_access[as_pos + 2..].trim();

    if raw_expr.is_empty() {
        return Err(ParseError {
            message: format!("Missing expression after 'AS' in metric entry '{entry}'."),
            position: Some(entry_offset + as_pos + 2),
        });
    }

    // Phase 43: Parse trailing annotations from expression
    let (expr, annotations) = parse_trailing_annotations(raw_expr)?;

    // Phase 48: Detect and parse OVER clause from the expression text.
    // The OVER clause is part of the expression for window metrics, e.g.:
    //   AVG(total_qty) OVER (PARTITION BY EXCLUDING d1, d2 ORDER BY d1)
    // Base the reported positions at the expression's own offset within the
    // entry (leading access modifier + AS + whitespace), not the entry start
    // — otherwise OVER-clause error carets point at the metric name
    // (PR #50 review).
    let after_as_slice = &entry_after_access[as_pos + 2..];
    let expr_offset = (entry.len() - entry_after_access.len())
        + as_pos
        + 2
        + (after_as_slice.len() - after_as_slice.trim_start().len());
    let (expr, window_spec) = parse_window_over_clause(&expr, entry_offset + expr_offset)?;

    if before_as.is_empty() {
        return Err(ParseError {
            message: format!("Missing metric name before 'AS' in entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Phase 47: Check for NON ADDITIVE BY in before_as first (it appears after USING if both present)
    let upper_before = before_as.to_ascii_uppercase();
    let na_pos = find_non_additive_by_keyword(&upper_before);
    let mut non_additive_by: Vec<NonAdditiveDim> = Vec::new();
    let before_na = if let Some((na_start, na_end)) = na_pos {
        let after_na = before_as[na_end..].trim();
        if !after_na.starts_with('(') {
            return Err(ParseError {
                message: format!("Expected '(' after NON ADDITIVE BY in metric entry '{entry}'."),
                position: Some(entry_offset + na_end),
            });
        }
        let paren_content = extract_paren_content(after_na).ok_or_else(|| ParseError {
            message: format!("Unclosed '(' after NON ADDITIVE BY in metric entry '{entry}'."),
            position: Some(entry_offset + na_end),
        })?;
        non_additive_by = parse_non_additive_dims(paren_content, entry_offset + na_end + 1)?;
        before_as[..na_start].trim()
    } else {
        before_as
    };

    // Phase 48: OVER clause combined with NON ADDITIVE BY produces error (mutually exclusive)
    if window_spec.is_some() && !non_additive_by.is_empty() {
        let name_part = before_na.trim();
        return Err(ParseError {
            message: format!(
                "Cannot combine OVER clause with NON ADDITIVE BY on metric '{name_part}'. \
                 Use one or the other.",
            ),
            position: Some(entry_offset),
        });
    }

    // Check for USING keyword in the portion before NON ADDITIVE BY (or full before_as)
    let upper_before_na = before_na.to_ascii_uppercase();
    let using_pos = find_keyword_ci(&upper_before_na, "USING");
    let mut using_relationships: Vec<String> = Vec::new();

    // The name portion is before USING (or all of before_na if no USING)
    let final_name_portion = if let Some(upos) = using_pos {
        // Extract the parenthesized relationship list after USING
        let after_using = before_na[upos + 5..].trim();
        if !after_using.starts_with('(') {
            return Err(ParseError {
                message: format!("Expected '(' after USING in metric entry '{entry}'."),
                position: Some(entry_offset + upos + 5),
            });
        }
        let paren_content = extract_paren_content(after_using).ok_or_else(|| ParseError {
            message: format!("Unclosed '(' after USING in metric entry '{entry}'."),
            position: Some(entry_offset + upos + 5),
        })?;
        using_relationships = split_at_depth0_commas(paren_content)
            .into_iter()
            .map(|(_, entry)| entry.to_string())
            .collect();
        before_na[..upos].trim()
    } else {
        before_na
    };

    // Check for dot to distinguish qualified vs unqualified. Quote-aware
    // (PA-6): a dot inside a quoted name (`"a.b"`) is not a qualifier
    // separator.
    if let Some(dot_pos) = find_live_byte(final_name_portion, b'.') {
        // Qualified: alias.name
        let source_alias = final_name_portion[..dot_pos].trim().to_string();
        let bare_name = final_name_portion[dot_pos + 1..].trim().to_string();

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

        Ok((
            Some(source_alias),
            bare_name,
            expr,
            using_relationships,
            annotations.comment,
            annotations.synonyms,
            access,
            non_additive_by,
            window_spec,
        ))
    } else {
        // Unqualified: just name (derived metric)
        // USING is not allowed on derived metrics
        if !using_relationships.is_empty() {
            return Err(ParseError {
                message: format!(
                    "USING clause not allowed on derived metric '{final_name_portion}'. \
                     Only qualified metrics (alias.name) can use USING.",
                ),
                position: Some(entry_offset),
            });
        }
        // NON ADDITIVE BY is not allowed on derived metrics
        if !non_additive_by.is_empty() {
            return Err(ParseError {
                message: format!(
                    "NON ADDITIVE BY clause not allowed on derived metric '{final_name_portion}'. \
                     Only qualified metrics (alias.name) can use NON ADDITIVE BY.",
                ),
                position: Some(entry_offset),
            });
        }
        // OVER clause is not allowed on derived metrics
        if window_spec.is_some() {
            return Err(ParseError {
                message: format!(
                    "OVER clause not allowed on derived metric '{final_name_portion}'. \
                     Only qualified metrics (alias.name) can use OVER.",
                ),
                position: Some(entry_offset),
            });
        }
        let bare_name = final_name_portion.to_string();
        Ok((
            None,
            bare_name,
            expr,
            vec![],
            annotations.comment,
            annotations.synonyms,
            access,
            vec![],
            None,
        ))
    }
}
