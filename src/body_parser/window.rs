//! Window-metric OVER clause parsing and the shared ORDER-BY modifier loop.

use super::scan::{
    extract_paren_content, find_depth0_keyword, find_keyword_ci, is_ident_continuation,
    is_quoting_balanced,
};
use super::split_at_depth0_commas;
use crate::errors::ParseError;
use crate::ident::find_identifier_end;
use crate::model::{NullsOrder, SortOrder, WindowOrderBy, WindowSpec};

/// Parse a window function OVER clause from the expression text.
///
/// Detects `FUNC(metric[, args...]) OVER (PARTITION BY EXCLUDING d1, d2 [ORDER BY ...] [frame])`.
/// Returns the raw expression and an optional parsed `WindowSpec`.
///
/// The OVER keyword must be at depth-0 (not inside parens or string literals) and at a word
/// boundary. If found, the function call part is parsed into `window_function`, `inner_metric`,
/// and `extra_args`, and the OVER clause content is parsed for EXCLUDING dims, ORDER BY entries,
/// and a frame clause.
pub(super) fn parse_window_over_clause(
    expr: &str,
    base_offset: usize,
) -> Result<(String, Option<WindowSpec>), ParseError> {
    let expr = expr.trim();
    let upper = expr.to_ascii_uppercase();

    // Scan for OVER keyword at depth-0 with word boundaries
    let Some(over_pos) = find_depth0_keyword(&upper, expr, "OVER") else {
        return Ok((expr.to_string(), None));
    };

    // The part before OVER is the function call: e.g., "AVG(total_qty)" or "LAG(total_qty, 30)"
    let func_part = expr[..over_pos].trim();
    let after_over = expr[over_pos + 4..].trim();

    // Extract the OVER clause's parenthesized content
    if !after_over.starts_with('(') {
        return Err(ParseError {
            message: format!("Expected '(' after OVER in expression '{expr}'."),
            position: Some(base_offset + over_pos + 4),
        });
    }
    let over_content = extract_paren_content(after_over).ok_or_else(|| ParseError {
        message: format!("Unclosed '(' after OVER in expression '{expr}'."),
        position: Some(base_offset + over_pos + 4),
    })?;

    // Parse function call part: FUNC(inner_metric[, extra_args...])
    let paren_start = func_part.find('(').ok_or_else(|| ParseError {
        message: format!(
            "Window function before OVER must have parenthesized arguments: '{func_part}'."
        ),
        position: Some(base_offset),
    })?;
    let window_function = func_part[..paren_start].trim().to_string();
    let func_args_content =
        extract_paren_content(&func_part[paren_start..]).ok_or_else(|| ParseError {
            message: format!("Unclosed '(' in window function call '{func_part}'."),
            position: Some(base_offset + paren_start),
        })?;

    // Split function arguments: first is inner_metric, rest are extra_args
    let func_args: Vec<&str> = split_at_depth0_commas(func_args_content)
        .into_iter()
        .map(|(_, s)| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if func_args.is_empty() {
        return Err(ParseError {
            message: format!("Window function '{window_function}' has no arguments."),
            position: Some(base_offset),
        });
    }
    let inner_metric = func_args[0].to_string();
    let extra_args: Vec<String> = func_args[1..]
        .iter()
        .map(std::string::ToString::to_string)
        .collect();

    // Parse OVER clause content: PARTITION BY [EXCLUDING] ..., ORDER BY ..., frame clause
    let over_upper = over_content.to_ascii_uppercase();
    let (excluding_dims, partition_dims, order_by, frame_clause) =
        parse_over_content(over_content, &over_upper, base_offset + over_pos)?;

    Ok((
        expr.to_string(),
        Some(WindowSpec {
            window_function,
            inner_metric,
            extra_args,
            excluding_dims,
            partition_dims,
            order_by,
            frame_clause,
        }),
    ))
}

/// Parsed components of an OVER clause.
/// (`excluding_dims`, `partition_dims`, `order_by`, `frame_clause`)
type OverContent = (Vec<String>, Vec<String>, Vec<WindowOrderBy>, Option<String>);

/// Parse the content inside the OVER (...) clause.
/// Returns (`excluding_dims`, `order_by`, `frame_clause`).
#[allow(clippy::too_many_lines)]
fn parse_over_content(
    content: &str,
    upper_content: &str,
    base_offset: usize,
) -> Result<OverContent, ParseError> {
    let content = content.trim();
    let upper_content = upper_content.trim();

    if content.is_empty() {
        return Ok((vec![], vec![], vec![], None));
    }

    // Look for PARTITION BY EXCLUDING or plain PARTITION BY at the start
    let mut excluding_dims: Vec<String> = Vec::new();
    let mut partition_dims: Vec<String> = Vec::new();
    let mut remaining = content;
    let mut remaining_upper = upper_content;

    if let Some(pbe_pos) = find_partition_by_excluding(upper_content) {
        let after_pbe = &content[pbe_pos..];
        let after_pbe_upper = &upper_content[pbe_pos..];

        // Find end of EXCLUDING dims list: either ORDER BY, or a frame keyword, or end of string
        let end_of_dims =
            find_keyword_ci(after_pbe_upper, "ORDER").or_else(|| find_frame_start(after_pbe_upper));
        let dims_text = match end_of_dims {
            Some(end) => after_pbe[..end].trim(),
            None => after_pbe.trim(),
        };

        // Parse comma-separated dimension names
        excluding_dims = split_at_depth0_commas(dims_text)
            .into_iter()
            .map(|(_, s)| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Slice BOTH content and its ASCII-uppercased twin (identical byte
        // offsets) from the same absolute bounds. Deriving the start via
        // `content.len() - remaining.len()` assumed `remaining` was a strict
        // suffix; `.trim()` also strips trailing whitespace, so that start
        // would skip the remainder's leading bytes if any trailing
        // whitespace were present. Today `content` is pre-trimmed so no live
        // bug, but offset-based slicing is correct independent of that
        // invariant (PR #50 review).
        let (rem_start, rem_end) = match end_of_dims {
            Some(end) => trimmed_bounds(content, pbe_pos + end),
            None => (content.len(), content.len()),
        };
        remaining = &content[rem_start..rem_end];
        remaining_upper = &upper_content[rem_start..rem_end];
    } else if let Some(pb_pos) = find_partition_by(upper_content) {
        // Plain PARTITION BY (without EXCLUDING)
        let after_pb = &content[pb_pos..];
        let after_pb_upper = &upper_content[pb_pos..];

        let end_of_dims =
            find_keyword_ci(after_pb_upper, "ORDER").or_else(|| find_frame_start(after_pb_upper));
        let dims_text = match end_of_dims {
            Some(end) => after_pb[..end].trim(),
            None => after_pb.trim(),
        };

        partition_dims = split_at_depth0_commas(dims_text)
            .into_iter()
            .map(|(_, s)| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Offset-based slicing (see the EXCLUDING branch above).
        let (rem_start, rem_end) = match end_of_dims {
            Some(end) => trimmed_bounds(content, pb_pos + end),
            None => (content.len(), content.len()),
        };
        remaining = &content[rem_start..rem_end];
        remaining_upper = &upper_content[rem_start..rem_end];
    }

    // Look for ORDER BY
    let mut order_by: Vec<WindowOrderBy> = Vec::new();
    let order_pos = find_keyword_ci(remaining_upper, "ORDER");
    if let Some(opos) = order_pos {
        // P-3 (code-review 2026-07-11): text before ORDER at this point is
        // not valid OVER-clause content — it was previously skipped silently.
        if !remaining[..opos].trim().is_empty() {
            return Err(ParseError {
                message: format!(
                    "Unexpected text '{}' before ORDER BY in OVER clause.",
                    remaining[..opos].trim()
                ),
                position: Some(base_offset),
            });
        }
        let after_order = &remaining[opos + 5..];
        let after_order_upper = &remaining_upper[opos + 5..];
        // P-3: ORDER must be immediately followed by BY. Previously BY was
        // searched for ANYWHERE after ORDER (junk between them was skipped),
        // and when BY was absent the whole tail fell through silently and
        // `ORDER d` was stored as the frame clause — accepted with no
        // ordering applied.
        let ws_len = after_order.len() - after_order.trim_start().len();
        let at_by = &after_order_upper.as_bytes()[ws_len..];
        let by_ok =
            at_by.starts_with(b"BY") && (at_by.len() == 2 || !is_ident_continuation(at_by[2]));
        if !by_ok {
            return Err(ParseError {
                message: "Expected BY immediately after ORDER in OVER clause.".to_string(),
                position: Some(base_offset),
            });
        }
        {
            let after_order_by = after_order[ws_len + 2..].trim();
            let after_order_by_upper = after_order_upper[ws_len + 2..].trim();

            // Find end of ORDER BY: frame clause or end of string
            let frame_start = find_frame_start(after_order_by_upper);
            let order_text = match frame_start {
                Some(fpos) => after_order_by[..fpos].trim(),
                None => after_order_by.trim(),
            };

            // Parse ORDER BY entries using same pattern as non_additive_by
            // Phase 68 Plan 03 (B2) / TECH-DEBT #25: identifier-aware
            // tokenisation of the column-reference slot. The post-port
            // contract narrows the slot from "any expression" to
            // "identifier (possibly quoted, possibly dotted)" — RESEARCH §B2
            // confirmed no existing fixture uses function-call expressions
            // here, so this is a defense-in-depth narrowing, not a regression.
            let entries = split_at_depth0_commas(order_text);
            for (start, entry_text) in entries {
                let entry_text = entry_text.trim();
                if entry_text.is_empty() {
                    continue;
                }
                let name_end = find_identifier_end(entry_text, /* allow_paren = */ false);
                if name_end == 0 {
                    continue;
                }
                if !is_quoting_balanced(&entry_text[..name_end]) {
                    return Err(ParseError {
                        message: format!(
                            "Unterminated quoted identifier in OVER ORDER BY entry '{entry_text}'."
                        ),
                        position: Some(base_offset + start),
                    });
                }
                let dim_name = entry_text[..name_end].trim().to_string();
                let suffix = entry_text[name_end..].trim();
                let parts: Vec<&str> = suffix.split_whitespace().collect();
                let (sort, nulls) = parse_order_by_modifiers(
                    &parts,
                    OrderModifierContext::OverOrderBy { entry_text },
                    base_offset + start,
                )?;
                order_by.push(WindowOrderBy {
                    expr: dim_name,
                    order: sort,
                    nulls,
                });
            }

            // P-3: ORDER BY must yield at least one parsed entry. Without
            // this, an unquoted reference named `range`/`rows`/`groups` was
            // claimed by `find_frame_start`, leaving zero entries and a
            // bogus frame clause with no diagnostics.
            if order_by.is_empty() {
                return Err(ParseError {
                    message: format!(
                        "Expected column reference after ORDER BY in OVER clause, found '{after_order_by}'. (Quote the reference if it is named like a frame keyword.)"
                    ),
                    position: Some(base_offset),
                });
            }

            // Frame clause is everything after ORDER BY entries
            remaining = match frame_start {
                Some(fpos) => after_order_by[fpos..].trim(),
                None => "",
            };
        }
    }

    // Whatever is left is the frame clause. P-3: validate it actually IS one
    // — previously any residue was stored verbatim as `frame_clause`.
    let frame_clause = if remaining.is_empty() {
        None
    } else {
        let upper_rem = remaining.to_ascii_uppercase();
        let is_frame = [&b"ROWS"[..], b"RANGE", b"GROUPS"].iter().any(|kw| {
            upper_rem.as_bytes().starts_with(kw)
                && (upper_rem.len() == kw.len()
                    || !is_ident_continuation(upper_rem.as_bytes()[kw.len()]))
        });
        if !is_frame {
            return Err(ParseError {
                message: format!(
                    "Expected frame clause starting with ROWS, RANGE, or GROUPS in OVER clause, found '{remaining}'."
                ),
                position: Some(base_offset),
            });
        }
        Some(remaining.to_string())
    };

    Ok((excluding_dims, partition_dims, order_by, frame_clause))
}

/// Absolute `[start, end)` byte offsets of `s[base..].trim()` within `s`.
///
/// Lets a caller slice `s` and its ASCII-uppercased twin (identical byte
/// offsets) from the same bounds, avoiding the `len()`-subtraction pitfall:
/// `trim()` strips both ends, so `s.len() - s[base..].trim().len()` skips
/// the remainder's leading bytes whenever trailing whitespace is present.
fn trimmed_bounds(s: &str, base: usize) -> (usize, usize) {
    let raw = &s[base..];
    let start = base + (raw.len() - raw.trim_start().len());
    let end = start + raw.trim().len();
    (start, end)
}

/// Find "PARTITION BY" (without EXCLUDING) in uppercase text.
/// Returns byte offset past "BY" (the start of the dims list).
/// Only matches if NOT followed by EXCLUDING.
fn find_partition_by(upper_text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(pos) = find_keyword_ci(&upper_text[search_from..], "PARTITION") {
        let abs_pos = search_from + pos;
        let after_partition = upper_text[abs_pos + 9..].trim_start();
        if let Some(rest) = after_partition.strip_prefix("BY") {
            // Word boundary after BY: `_` and non-ASCII bytes continue an
            // identifier (BY_foo is not the keyword BY).
            if rest.is_empty() || !is_ident_continuation(rest.as_bytes()[0]) {
                let rest = rest.trim_start();
                // Make sure this is NOT PARTITION BY EXCLUDING
                if rest.starts_with("EXCLUDING")
                    && (rest.len() == 9 || !is_ident_continuation(rest.as_bytes()[9]))
                {
                    // This is PARTITION BY EXCLUDING, skip
                    search_from = abs_pos + 9;
                    continue;
                }
                // Return offset past "BY"
                let by_end = upper_text.len() - rest.len();
                return Some(by_end);
            }
        }
        search_from = abs_pos + 9;
    }
    None
}

/// Find "PARTITION BY EXCLUDING" in uppercase text.
/// Returns byte offset past "EXCLUDING" (the start of the dims list).
fn find_partition_by_excluding(upper_text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(pos) = find_keyword_ci(&upper_text[search_from..], "PARTITION") {
        let abs_pos = search_from + pos;
        let after_partition = upper_text[abs_pos + 9..].trim_start();
        if let Some(rest) = after_partition.strip_prefix("BY") {
            // Word boundary after BY / EXCLUDING: `_` and non-ASCII bytes
            // continue an identifier.
            if rest.is_empty() || !is_ident_continuation(rest.as_bytes()[0]) {
                let rest = rest.trim_start();
                if let Some(rest2) = rest.strip_prefix("EXCLUDING") {
                    if rest2.is_empty() || !is_ident_continuation(rest2.as_bytes()[0]) {
                        // Return offset past EXCLUDING
                        let excluding_end = upper_text.len() - rest2.len();
                        return Some(excluding_end);
                    }
                }
            }
        }
        search_from = abs_pos + 9;
    }
    None
}

/// Find the start of a frame clause keyword (ROWS, RANGE, GROUPS) in uppercase text.
fn find_frame_start(upper_text: &str) -> Option<usize> {
    // Try each frame keyword
    let frame_keywords = ["ROWS", "RANGE", "GROUPS"];
    let mut earliest: Option<usize> = None;
    for kw in &frame_keywords {
        if let Some(pos) = find_keyword_ci(upper_text, kw) {
            match earliest {
                None => earliest = Some(pos),
                Some(e) if pos < e => earliest = Some(pos),
                _ => {}
            }
        }
    }
    earliest
}

/// Sort-modifier parsing context: which clause the `ASC|DESC|NULLS FIRST|LAST`
/// modifier suffix belongs to. The two call sites share the token loop below
/// but differ deliberately in two behaviours (kept byte-for-byte identical to
/// the pre-extraction loops):
///
/// - **Unknown tokens**: a hard `ParseError` for NON ADDITIVE BY entries; a
///   silent stop for OVER ORDER BY entries (trailing text belongs to the
///   frame clause).
/// - **DESC nulls default**: NON ADDITIVE BY applies the `DESC => NULLS
///   FIRST` default after the loop, only when no explicit NULLS was given
///   (so `NULLS LAST DESC` keeps LAST); OVER ORDER BY sets NULLS FIRST the
///   moment DESC is seen (matches DuckDB/Snowflake), so a later explicit
///   NULLS wins but an earlier one is overridden.
#[derive(Clone, Copy)]
pub(super) enum OrderModifierContext<'a> {
    /// `NON ADDITIVE BY (dim [ASC|DESC] [NULLS FIRST|LAST], ...)` entry.
    NonAdditiveBy,
    /// `OVER (... ORDER BY dim [ASC|DESC] [NULLS FIRST|LAST] ...)` entry;
    /// carries the entry text for error messages.
    OverOrderBy { entry_text: &'a str },
}

impl OrderModifierContext<'_> {
    /// Error message when the token after NULLS is neither FIRST nor LAST.
    fn nulls_bad_follower_message(self, follower: &str) -> String {
        match self {
            Self::NonAdditiveBy => {
                format!("Expected FIRST or LAST after NULLS, got '{follower}'")
            }
            Self::OverOrderBy { entry_text } => {
                format!("Expected FIRST or LAST after NULLS in OVER ORDER BY entry '{entry_text}'.")
            }
        }
    }

    /// Error message when NULLS is the final token.
    fn nulls_missing_message(self) -> String {
        match self {
            Self::NonAdditiveBy => "Expected FIRST or LAST after NULLS".to_string(),
            Self::OverOrderBy { entry_text } => {
                format!("Expected FIRST or LAST after NULLS in OVER ORDER BY entry '{entry_text}'.")
            }
        }
    }
}

/// Parse a whitespace-tokenised `[ASC|DESC] [NULLS FIRST|LAST]` modifier
/// suffix. This is the ONE shared implementation of the modifier loop that
/// previously appeared in near-identical form in the NON ADDITIVE BY dim
/// parser and the OVER ORDER BY parser (ST-3, code-review 2026-07-02).
/// Returns the resolved `(SortOrder, NullsOrder)` pair. Defaults are
/// ASC / NULLS LAST; the DESC => NULLS FIRST default and the unknown-token
/// policy vary by `context` (see `OrderModifierContext`).
pub(super) fn parse_order_by_modifiers(
    parts: &[&str],
    context: OrderModifierContext<'_>,
    err_position: usize,
) -> Result<(SortOrder, NullsOrder), ParseError> {
    let mut order = SortOrder::Asc;
    let mut nulls = NullsOrder::Last;
    let mut has_explicit_nulls = false;
    let mut i = 0;
    while i < parts.len() {
        match parts[i].to_ascii_uppercase().as_str() {
            "ASC" => {
                order = SortOrder::Asc;
                i += 1;
            }
            "DESC" => {
                order = SortOrder::Desc;
                if matches!(context, OrderModifierContext::OverOrderBy { .. }) {
                    // DESC defaults to NULLS FIRST (matches DuckDB/Snowflake)
                    nulls = NullsOrder::First;
                }
                i += 1;
            }
            "NULLS" => {
                if i + 1 < parts.len() {
                    match parts[i + 1].to_ascii_uppercase().as_str() {
                        "FIRST" => {
                            nulls = NullsOrder::First;
                            has_explicit_nulls = true;
                            i += 2;
                        }
                        "LAST" => {
                            nulls = NullsOrder::Last;
                            has_explicit_nulls = true;
                            i += 2;
                        }
                        _ => {
                            return Err(ParseError {
                                message: context.nulls_bad_follower_message(parts[i + 1]),
                                position: Some(err_position),
                            });
                        }
                    }
                } else {
                    return Err(ParseError {
                        message: context.nulls_missing_message(),
                        position: Some(err_position),
                    });
                }
            }
            other => match context {
                OrderModifierContext::NonAdditiveBy => {
                    return Err(ParseError {
                        message: format!(
                            "Unexpected token '{other}' in NON ADDITIVE BY dimension entry",
                        ),
                        position: Some(err_position),
                    });
                }
                OrderModifierContext::OverOrderBy { .. } => {
                    // Unexpected token, stop parsing ORDER BY modifiers
                    break;
                }
            },
        }
    }
    // Adjust default nulls based on sort order (DESC defaults to NULLS FIRST)
    // Only if user did not explicitly specify NULLS
    if matches!(context, OrderModifierContext::NonAdditiveBy)
        && !has_explicit_nulls
        && order == SortOrder::Desc
    {
        nulls = NullsOrder::First;
    }
    Ok((order, nulls))
}

#[cfg(test)]
mod tests {
    use super::trimmed_bounds;

    #[test]
    fn trimmed_bounds_no_whitespace() {
        let s = "ORDER BY d";
        assert_eq!(trimmed_bounds(s, 0), (0, s.len()));
    }

    #[test]
    fn trimmed_bounds_leading_only() {
        // base points before "   ORDER BY d"; start skips the 3 leading
        // spaces, end is the full string length (no trailing ws).
        let s = "xyz   ORDER BY d";
        let (start, end) = trimmed_bounds(s, 3);
        assert_eq!(&s[start..end], "ORDER BY d");
    }

    #[test]
    fn trimmed_bounds_leading_and_trailing() {
        // The old `len() - remaining.len()` start computation skipped the
        // two trailing spaces' worth of leading bytes here (PR #50 review).
        let s = "xyz  ORDER BY d  ";
        let (start, end) = trimmed_bounds(s, 3);
        assert_eq!(&s[start..end], "ORDER BY d");
    }

    #[test]
    fn trimmed_bounds_all_whitespace_suffix() {
        let s = "abc   ";
        let (start, end) = trimmed_bounds(s, 3);
        assert_eq!(start, end, "empty trimmed remainder -> zero-length span");
    }
}
