//! SQL keyword body parser for CREATE SEMANTIC VIEW.
//!
//! Parses: `AS TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)`
//! into a `SemanticViewDefinition`.

use crate::errors::ParseError;
use crate::model::{
    AccessModifier, Cardinality, Dimension, Fact, Join, Metric, NonAdditiveDim, NullsOrder,
    SortOrder, TableRef, WindowOrderBy, WindowSpec,
};

/// Parsed qualified entry tuple.
type QualifiedEntry = (
    String,
    String,
    String,
    Option<String>,
    Vec<String>,
    AccessModifier,
);

/// Parsed metric entry tuple.
type MetricEntry = (
    Option<String>,      // source_alias
    String,              // bare_name
    String,              // expr
    Vec<String>,         // using_relationships
    Option<String>,      // comment
    Vec<String>,         // synonyms
    AccessModifier,      // access
    Vec<NonAdditiveDim>, // non_additive_by
    Option<WindowSpec>,  // window_spec
);

/// Result of parsing the keyword body (everything after "AS").
#[derive(Debug)]
pub struct KeywordBody {
    pub tables: Vec<TableRef>,
    pub relationships: Vec<Join>,
    pub facts: Vec<Fact>,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
}

/// Trailing metadata annotations parsed from a DDL entry.
/// Used internally to collect COMMENT and SYNONYMS from entry text.
#[derive(Debug, Default)]
struct ParsedAnnotations {
    comment: Option<String>,
    synonyms: Vec<String>,
}

/// Known clause keywords for the AS-body scanner.
const CLAUSE_KEYWORDS: &[&str] = &["tables", "relationships", "facts", "dimensions", "metrics"];

/// Clause ordering — TABLES must be first, then RELATIONSHIPS (optional),
/// FACTS (optional), DIMENSIONS (optional),
/// METRICS (optional). At least one of DIMENSIONS or METRICS is required.
const CLAUSE_ORDER: &[&str] = &["tables", "relationships", "facts", "dimensions", "metrics"];

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
                format!("Unknown clause keyword '{word}'; did you mean '{sug_upper}'?",)
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
                    "Clause '{kw_upper}' appears out of order; clauses must appear as: TABLES, RELATIONSHIPS (optional), FACTS (optional), DIMENSIONS (optional), METRICS (optional).",
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
#[allow(clippy::too_many_lines)]
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
    let mut facts_raw: Vec<QualifiedEntry> = Vec::new();
    let mut dimensions_raw: Vec<QualifiedEntry> = Vec::new();
    let mut metrics_raw: Vec<MetricEntry> = Vec::new();

    for bound in &bounds {
        match bound.keyword {
            "tables" => {
                tables = parse_tables_clause(bound.content, bound.content_offset)?;
            }
            "relationships" => {
                relationships = parse_relationships_clause(bound.content, bound.content_offset)?;
            }
            "facts" => {
                facts_raw =
                    parse_qualified_entries(bound.content, bound.content_offset, true, "facts")?;
            }
            "dimensions" => {
                dimensions_raw = parse_qualified_entries(
                    bound.content,
                    bound.content_offset,
                    false,
                    "dimensions",
                )?;
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
        .map(|(alias, bare_name, expr, comment, synonyms, access)| Fact {
            name: bare_name,
            expr,
            source_table: Some(alias),
            output_type: None,
            comment,
            synonyms,
            access,
        })
        .collect();

    let dimensions: Vec<Dimension> = dimensions_raw
        .into_iter()
        .map(
            |(alias, bare_name, expr, comment, synonyms, _access)| Dimension {
                name: bare_name,
                expr,
                source_table: Some(alias),
                output_type: None,
                comment,
                synonyms,
            },
        )
        .collect();

    let metrics: Vec<Metric> = metrics_raw
        .into_iter()
        .map(
            |(
                source_alias,
                bare_name,
                expr,
                using_rels,
                comment,
                synonyms,
                access,
                non_additive_by,
                window_spec,
            )| Metric {
                name: bare_name,
                expr,
                source_table: source_alias,
                output_type: None,
                using_relationships: using_rels,
                comment,
                synonyms,
                access,
                non_additive_by,
                window_spec,
            },
        )
        .collect();

    // Phase 47: Validate NON ADDITIVE BY dimension references
    for metric in &metrics {
        for na in &metric.non_additive_by {
            let dim_exists = dimensions
                .iter()
                .any(|d| d.name.eq_ignore_ascii_case(&na.dimension));
            if !dim_exists {
                let available_dims: Vec<String> =
                    dimensions.iter().map(|d| d.name.clone()).collect();
                let suggestion = crate::util::suggest_closest(&na.dimension, &available_dims);
                let mut msg = format!(
                    "NON ADDITIVE BY dimension '{}' on metric '{}' does not match any declared dimension.",
                    na.dimension, metric.name
                );
                if let Some(closest) = suggestion {
                    use std::fmt::Write;
                    let _ = write!(msg, " Did you mean '{closest}'?");
                }
                return Err(ParseError {
                    message: msg,
                    position: None,
                });
            }
        }
    }

    // Phase 48: Validate window metric EXCLUDING dimension and inner metric references
    let metric_names: Vec<String> = metrics.iter().map(|m| m.name.clone()).collect();
    for metric in &metrics {
        if let Some(ref ws) = metric.window_spec {
            // Validate EXCLUDING dimension references
            for dim in &ws.excluding_dims {
                let dim_exists = dimensions.iter().any(|d| d.name.eq_ignore_ascii_case(dim));
                if !dim_exists {
                    let available_dims: Vec<String> =
                        dimensions.iter().map(|d| d.name.clone()).collect();
                    let suggestion = crate::util::suggest_closest(dim, &available_dims);
                    let mut msg = format!(
                        "Window metric '{}': EXCLUDING dimension '{}' not found in semantic view dimensions.",
                        metric.name, dim
                    );
                    if let Some(closest) = suggestion {
                        use std::fmt::Write;
                        let _ = write!(msg, " Did you mean '{closest}'?");
                    }
                    return Err(ParseError {
                        message: msg,
                        position: None,
                    });
                }
            }
            // Validate PARTITION BY dimension references
            for dim in &ws.partition_dims {
                let dim_exists = dimensions.iter().any(|d| d.name.eq_ignore_ascii_case(dim));
                if !dim_exists {
                    let available_dims: Vec<String> =
                        dimensions.iter().map(|d| d.name.clone()).collect();
                    let suggestion = crate::util::suggest_closest(dim, &available_dims);
                    let mut msg = format!(
                        "Window metric '{}': PARTITION BY dimension '{}' not found in semantic view dimensions.",
                        metric.name, dim
                    );
                    if let Some(closest) = suggestion {
                        use std::fmt::Write;
                        let _ = write!(msg, " Did you mean '{closest}'?");
                    }
                    return Err(ParseError {
                        message: msg,
                        position: None,
                    });
                }
            }
            // Validate ORDER BY dimension references
            for ob in &ws.order_by {
                let dim_exists = dimensions
                    .iter()
                    .any(|d| d.name.eq_ignore_ascii_case(&ob.expr));
                if !dim_exists {
                    let available_dims: Vec<String> =
                        dimensions.iter().map(|d| d.name.clone()).collect();
                    let suggestion = crate::util::suggest_closest(&ob.expr, &available_dims);
                    let mut msg = format!(
                        "Window metric '{}': ORDER BY dimension '{}' not found in semantic view dimensions.",
                        metric.name, ob.expr
                    );
                    if let Some(closest) = suggestion {
                        use std::fmt::Write;
                        let _ = write!(msg, " Did you mean '{closest}'?");
                    }
                    return Err(ParseError {
                        message: msg,
                        position: None,
                    });
                }
            }
            // Validate inner metric reference
            let inner_exists = metric_names
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&ws.inner_metric));
            if !inner_exists {
                let suggestion = crate::util::suggest_closest(&ws.inner_metric, &metric_names);
                let mut msg = format!(
                    "Window metric '{}': inner metric '{}' not found in semantic view metrics.",
                    metric.name, ws.inner_metric
                );
                if let Some(closest) = suggestion {
                    use std::fmt::Write;
                    let _ = write!(msg, " Did you mean '{closest}'?");
                }
                return Err(ParseError {
                    message: msg,
                    position: None,
                });
            }
        }
    }

    Ok(KeywordBody {
        tables,
        relationships,
        facts,
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

    let (table_name, pk_columns, after_pk_text) = if let Some((pk_start, pk_end)) = pk_pos {
        let table_name = after_as[..pk_start].trim();
        if table_name.is_empty() {
            return Err(ParseError {
                message: format!(
                    "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
                ),
                position: Some(after_as_offset),
            });
        }
        let after_pk = after_as[pk_end..].trim_start();
        if !after_pk.starts_with('(') {
            return Err(ParseError {
                message: "Expected '(' after PRIMARY KEY in TABLES clause.".to_string(),
                position: Some(after_as_offset + pk_end),
            });
        }
        let pk_body = extract_paren_content(after_pk).ok_or_else(|| ParseError {
            message: "Unclosed '(' in PRIMARY KEY column list.".to_string(),
            position: Some(after_as_offset + pk_end),
        })?;
        let pk_columns: Vec<String> = pk_body
            .split(',')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        // SAFETY: extract_paren_content succeeded above (returned Some), confirming
        // balanced parens exist. The closing ')' must be present in after_pk.
        let close = after_pk.find(')').unwrap();
        let remainder = &after_pk[close + 1..];
        (table_name, pk_columns, remainder)
    } else {
        // No PRIMARY KEY -- fact table. Table name is before UNIQUE keyword (if any).
        let unique_pos = find_unique(&upper);
        let table_name = if let Some((u_start, _)) = unique_pos {
            after_as[..u_start].trim()
        } else {
            after_as.trim()
        };
        if table_name.is_empty() {
            return Err(ParseError {
                message: format!(
                    "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
                ),
                position: Some(after_as_offset),
            });
        }
        let remainder = if let Some((u_start, _)) = unique_pos {
            &after_as[u_start..]
        } else {
            ""
        };
        (table_name, vec![], remainder)
    };

    // Step 4: parse zero or more UNIQUE constraints from after_pk_text
    let mut unique_constraints: Vec<Vec<String>> = Vec::new();
    let mut remaining = after_pk_text;
    loop {
        let upper_remaining = remaining.to_ascii_uppercase();
        if let Some((_u_start, u_end)) = find_unique(&upper_remaining) {
            let after_unique_kw = remaining[u_end..].trim_start();
            if !after_unique_kw.starts_with('(') {
                return Err(ParseError {
                    message: format!(
                        "Expected '(' after UNIQUE keyword for table alias '{alias}'."
                    ),
                    position: Some(entry_offset),
                });
            }
            let cols_str = extract_paren_content(after_unique_kw).ok_or_else(|| ParseError {
                message: format!("Unclosed '(' in UNIQUE column list for table alias '{alias}'."),
                position: Some(entry_offset),
            })?;
            let cols: Vec<String> = cols_str
                .split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            unique_constraints.push(cols);
            // SAFETY: extract_paren_content succeeded above (returned Some), confirming
            // balanced parens exist. The closing ')' must be present in after_unique_kw.
            let close = after_unique_kw.find(')').unwrap();
            remaining = &after_unique_kw[close + 1..];
        } else {
            break;
        }
    }

    // Phase 43: Parse trailing COMMENT / WITH SYNONYMS annotations after constraints
    let (_, annotations) = parse_trailing_annotations(remaining)?;

    Ok(TableRef {
        alias: alias.to_string(),
        table: table_name.to_string(),
        pk_columns,
        unique_constraints,
        comment: annotations.comment,
        synonyms: annotations.synonyms,
    })
}

/// Find "UNIQUE" keyword with word-boundary matching in `upper_text`.
/// Returns `(start, end)` byte offsets where start points at 'U' and end is past 'E'.
fn find_unique(upper_text: &str) -> Option<(usize, usize)> {
    let bytes = upper_text.as_bytes();
    let kw = b"UNIQUE";
    let kw_len = kw.len(); // 6
    let mut i = 0;
    while i + kw_len <= bytes.len() {
        if &bytes[i..i + kw_len] == kw {
            let before_ok =
                i == 0 || { !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_' };
            let after_ok = i + kw_len == bytes.len() || {
                !bytes[i + kw_len].is_ascii_alphanumeric() && bytes[i + kw_len] != b'_'
            };
            if before_ok && after_ok {
                return Some((i, i + kw_len));
            }
        }
        i += 1;
    }
    None
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
            // Check boundary: preceded by non-identifier char (or start), followed by non-identifier char (or end).
            // Underscore is a valid identifier character, so it must NOT count as a word boundary.
            let before_ok = i == 0 || {
                let c = upper_text.as_bytes()[i - 1];
                !c.is_ascii_alphanumeric() && c != b'_'
            };
            let after_ok = i + kw_len == text_len || {
                let c = upper_text.as_bytes()[i + kw_len];
                !c.is_ascii_alphanumeric() && c != b'_'
            };
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Extract a single-quoted string value, handling '' escape sequences.
/// Input starts with the opening quote: 'text here'
/// Returns the unescaped string content.
fn extract_single_quoted_string(s: &str) -> Result<String, ParseError> {
    if !s.starts_with('\'') {
        return Err(ParseError {
            message: "Expected single-quoted string.".to_string(),
            position: None,
        });
    }
    let bytes = s.as_bytes();
    let mut result = String::new();
    let mut i = 1; // skip opening quote
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                result.push('\'');
                i += 2;
                continue;
            }
            // Closing quote
            return Ok(result);
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    Err(ParseError {
        message: "Unclosed single-quoted string.".to_string(),
        position: None,
    })
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
#[allow(clippy::too_many_lines)]
fn parse_trailing_annotations(text: &str) -> Result<(String, ParsedAnnotations), ParseError> {
    let text = text.trim();
    let upper = text.to_ascii_uppercase();

    // Find the FIRST occurrence of COMMENT or WITH SYNONYMS at depth-0 with word boundaries.
    // Scan forward tracking depth to find annotation region start.
    let mut depth: i32 = 0;
    let mut in_string = false;
    let bytes = text.as_bytes();
    let upper_bytes = upper.as_bytes();
    let mut annotation_start: Option<usize> = None;
    let mut i = 0;

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
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }

        // At depth 0, outside string, check for COMMENT or WITH keyword
        if depth == 0 && !in_string {
            // Check for COMMENT keyword with word boundaries
            if i + 7 <= bytes.len() && &upper_bytes[i..i + 7] == b"COMMENT" {
                let before_ok = i == 0 || {
                    let c = bytes[i - 1];
                    !c.is_ascii_alphanumeric() && c != b'_'
                };
                let after_ok = i + 7 == bytes.len() || {
                    let c = bytes[i + 7];
                    !c.is_ascii_alphanumeric() && c != b'_'
                };
                if before_ok && after_ok && annotation_start.is_none() {
                    annotation_start = Some(i);
                }
            }
            // Check for WITH keyword (for WITH SYNONYMS)
            if i + 4 <= bytes.len() && &upper_bytes[i..i + 4] == b"WITH" {
                let before_ok = i == 0 || {
                    let c = bytes[i - 1];
                    !c.is_ascii_alphanumeric() && c != b'_'
                };
                let after_ok = i + 4 == bytes.len() || {
                    let c = bytes[i + 4];
                    !c.is_ascii_alphanumeric() && c != b'_'
                };
                if before_ok && after_ok {
                    // Verify it's WITH SYNONYMS, not just any WITH
                    let after_with = upper[i + 4..].trim_start();
                    if after_with.starts_with("SYNONYMS") && annotation_start.is_none() {
                        annotation_start = Some(i);
                    }
                }
            }
        }
        i += 1;
    }

    let (expr_text, annotation_text) = if let Some(start) = annotation_start {
        (text[..start].trim(), &text[start..])
    } else {
        return Ok((text.to_string(), ParsedAnnotations::default()));
    };

    // Parse annotation_text for COMMENT = '...' and WITH SYNONYMS = ('...', '...')
    let mut comment: Option<String> = None;
    let mut synonyms: Vec<String> = Vec::new();
    let ann_upper = annotation_text.to_ascii_uppercase();

    // Extract COMMENT = '...'
    if let Some(comment_pos) = find_keyword_ci(&ann_upper, "COMMENT") {
        let after_comment = annotation_text[comment_pos + 7..].trim_start();
        if !after_comment.starts_with('=') {
            return Err(ParseError {
                message: "Expected '=' after COMMENT keyword.".to_string(),
                position: None,
            });
        }
        let after_eq = after_comment[1..].trim_start();
        if !after_eq.starts_with('\'') {
            return Err(ParseError {
                message: "Expected single-quoted string after COMMENT =.".to_string(),
                position: None,
            });
        }
        comment = Some(extract_single_quoted_string(after_eq)?);
    }

    // Extract WITH SYNONYMS = ('...', '...')
    if let Some(with_pos) = find_keyword_ci(&ann_upper, "WITH") {
        let after_with = annotation_text[with_pos + 4..].trim_start();
        let aw_upper = after_with.to_ascii_uppercase();
        if aw_upper.starts_with("SYNONYMS") {
            let after_syn = after_with[8..].trim_start();
            if !after_syn.starts_with('=') {
                return Err(ParseError {
                    message: "Expected '=' after WITH SYNONYMS keyword.".to_string(),
                    position: None,
                });
            }
            let after_eq = after_syn[1..].trim_start();
            let content = extract_paren_content(after_eq).ok_or_else(|| ParseError {
                message: "Expected parenthesized list after WITH SYNONYMS =.".to_string(),
                position: None,
            })?;
            synonyms = parse_synonym_list(content)?;
        }
    }

    Ok((
        expr_text.to_string(),
        ParsedAnnotations { comment, synonyms },
    ))
}

/// Check for a leading PRIVATE or PUBLIC keyword on an entry.
/// Returns (`AccessModifier`, `remaining_entry_text`).
/// Disambiguates table aliases starting with "private" or "public" by checking
/// if the next non-whitespace character is '.' (indicating a qualified identifier).
fn parse_leading_access_modifier(entry: &str) -> (AccessModifier, &str) {
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

    let remaining_after_refs = after_paren[refs_pos + "REFERENCES".len()..].trim();

    // Get target alias: may be followed by '(' for explicit ref columns
    let (to_alias, after_to) = if remaining_after_refs.is_empty() {
        return Err(ParseError {
            message: format!(
                "Expected target alias after REFERENCES in relationship '{rel_name}'.",
            ),
            position: Some(entry_offset),
        });
    } else if let Some(paren_idx) = remaining_after_refs.find('(') {
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
        let cols: Vec<String> = cols_str
            .split(',')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        // SAFETY: extract_paren_content succeeded above (returned Some), confirming
        // balanced parens exist. The closing ')' must be present in after_to.
        let close = after_to.find(')').unwrap();
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
/// Returns the byte offset of "NON" if found.
fn find_non_additive_by_keyword(upper_text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(pos) = find_keyword_ci(&upper_text[search_from..], "NON") {
        let abs_pos = search_from + pos;
        let after_non = upper_text[abs_pos + 3..].trim_start();
        if let Some(rest) = after_non.strip_prefix("ADDITIVE") {
            let after_additive = rest.trim_start();
            if let Some(after_by) = after_additive.strip_prefix("BY") {
                // Verify BY has word boundary
                if after_by.is_empty() || !after_by.as_bytes()[0].is_ascii_alphanumeric() {
                    return Some(abs_pos);
                }
            }
        }
        search_from = abs_pos + 3;
    }
    None
}

/// Parse the dimension entries inside a NON ADDITIVE BY (...) clause.
/// Each entry: `dim_name [ASC|DESC] [NULLS FIRST|LAST]`
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
        let parts: Vec<&str> = entry_text.split_whitespace().collect();
        if parts.is_empty() {
            return Err(ParseError {
                message: "Empty dimension in NON ADDITIVE BY clause".to_string(),
                position: Some(base_offset + start),
            });
        }
        let dim_name = parts[0].to_string();
        let upper_parts: Vec<String> = parts.iter().map(|p| p.to_ascii_uppercase()).collect();
        let mut order = SortOrder::Asc;
        let mut nulls = NullsOrder::Last;
        let mut has_explicit_nulls = false;
        let mut i = 1;
        while i < upper_parts.len() {
            match upper_parts[i].as_str() {
                "ASC" => {
                    order = SortOrder::Asc;
                    i += 1;
                }
                "DESC" => {
                    order = SortOrder::Desc;
                    i += 1;
                }
                "NULLS" => {
                    if i + 1 < upper_parts.len() {
                        match upper_parts[i + 1].as_str() {
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
                                    message: format!(
                                        "Expected FIRST or LAST after NULLS, got '{}'",
                                        parts[i + 1]
                                    ),
                                    position: Some(base_offset + start),
                                });
                            }
                        }
                    } else {
                        return Err(ParseError {
                            message: "Expected FIRST or LAST after NULLS".to_string(),
                            position: Some(base_offset + start),
                        });
                    }
                }
                other => {
                    return Err(ParseError {
                        message: format!(
                            "Unexpected token '{other}' in NON ADDITIVE BY dimension entry",
                        ),
                        position: Some(base_offset + start),
                    });
                }
            }
        }
        // Adjust default nulls based on sort order (DESC defaults to NULLS FIRST)
        // Only if user did not explicitly specify NULLS
        if !has_explicit_nulls && order == SortOrder::Desc {
            nulls = NullsOrder::First;
        }
        result.push(NonAdditiveDim {
            dimension: dim_name,
            order,
            nulls,
        });
    }
    Ok(result)
}

/// Parse a window function OVER clause from the expression text.
///
/// Detects `FUNC(metric[, args...]) OVER (PARTITION BY EXCLUDING d1, d2 [ORDER BY ...] [frame])`.
/// Returns the raw expression and an optional parsed `WindowSpec`.
///
/// The OVER keyword must be at depth-0 (not inside parens or string literals) and at a word
/// boundary. If found, the function call part is parsed into `window_function`, `inner_metric`,
/// and `extra_args`, and the OVER clause content is parsed for EXCLUDING dims, ORDER BY entries,
/// and a frame clause.
fn parse_window_over_clause(
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

/// Find a keyword at depth-0 in a string (not inside parens or string literals).
/// Returns byte offset of the keyword start.
fn find_depth0_keyword(upper_text: &str, raw_text: &str, keyword: &str) -> Option<usize> {
    let bytes = upper_text.as_bytes();
    let kw_len = keyword.len();
    if bytes.len() < kw_len {
        return None;
    }
    let raw_bytes = raw_text.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        let ch = raw_bytes[i] as char;
        if ch == '\'' {
            if in_string && i + 1 < bytes.len() && raw_bytes[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            in_string = !in_string;
        } else if !in_string {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        if depth == 0
            && !in_string
            && i + kw_len <= bytes.len()
            && &upper_text[i..i + kw_len] == keyword
        {
            let before_ok = i == 0 || {
                let c = bytes[i - 1];
                !c.is_ascii_alphanumeric() && c != b'_'
            };
            let after_ok = i + kw_len == bytes.len() || {
                let c = bytes[i + kw_len];
                !c.is_ascii_alphanumeric() && c != b'_'
            };
            if before_ok && after_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
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

        remaining = match end_of_dims {
            Some(end) => after_pbe[end..].trim(),
            None => "",
        };
        let content_start = content.len() - remaining.len();
        remaining_upper = upper_content[content_start..].trim();
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

        remaining = match end_of_dims {
            Some(end) => after_pb[end..].trim(),
            None => "",
        };
        let content_start = content.len() - remaining.len();
        remaining_upper = upper_content[content_start..].trim();
    }

    // Look for ORDER BY
    let mut order_by: Vec<WindowOrderBy> = Vec::new();
    let order_pos = find_keyword_ci(remaining_upper, "ORDER");
    if let Some(opos) = order_pos {
        let after_order = &remaining[opos..];
        let after_order_upper = &remaining_upper[opos..];
        // Skip "ORDER BY" (consume ORDER, then BY)
        let by_pos = find_keyword_ci(&after_order_upper[5..], "BY");
        if let Some(bp) = by_pos {
            let after_order_by = &after_order[5 + bp + 2..].trim();
            let after_order_by_upper = after_order_upper[5 + bp + 2..].trim();

            // Find end of ORDER BY: frame clause or end of string
            let frame_start = find_frame_start(after_order_by_upper);
            let order_text = match frame_start {
                Some(fpos) => after_order_by[..fpos].trim(),
                None => after_order_by.trim(),
            };

            // Parse ORDER BY entries using same pattern as non_additive_by
            let entries = split_at_depth0_commas(order_text);
            for (start, entry_text) in entries {
                let entry_text = entry_text.trim();
                if entry_text.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = entry_text.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let dim_name = parts[0].to_string();
                let mut sort = SortOrder::Asc;
                let mut nulls = NullsOrder::Last;
                let mut idx = 1;
                while idx < parts.len() {
                    match parts[idx].to_ascii_uppercase().as_str() {
                        "ASC" => {
                            sort = SortOrder::Asc;
                            idx += 1;
                        }
                        "DESC" => {
                            sort = SortOrder::Desc;
                            // DESC defaults to NULLS FIRST (matches DuckDB/Snowflake)
                            nulls = NullsOrder::First;
                            idx += 1;
                        }
                        "NULLS" => {
                            if idx + 1 < parts.len() {
                                match parts[idx + 1].to_ascii_uppercase().as_str() {
                                    "FIRST" => {
                                        nulls = NullsOrder::First;
                                        idx += 2;
                                    }
                                    "LAST" => {
                                        nulls = NullsOrder::Last;
                                        idx += 2;
                                    }
                                    _ => {
                                        return Err(ParseError {
                                            message: format!(
                                                "Expected FIRST or LAST after NULLS in OVER ORDER BY entry '{entry_text}'."
                                            ),
                                            position: Some(base_offset + start),
                                        });
                                    }
                                }
                            } else {
                                return Err(ParseError {
                                    message: format!(
                                        "Expected FIRST or LAST after NULLS in OVER ORDER BY entry '{entry_text}'."
                                    ),
                                    position: Some(base_offset + start),
                                });
                            }
                        }
                        _ => {
                            // Unexpected token, stop parsing ORDER BY modifiers
                            break;
                        }
                    }
                }
                order_by.push(WindowOrderBy {
                    expr: dim_name,
                    order: sort,
                    nulls,
                });
            }

            // Frame clause is everything after ORDER BY entries
            remaining = match frame_start {
                Some(fpos) => after_order_by[fpos..].trim(),
                None => "",
            };
        }
    }

    // Whatever is left is the frame clause
    let frame_clause = if remaining.is_empty() {
        None
    } else {
        Some(remaining.to_string())
    };

    Ok((excluding_dims, partition_dims, order_by, frame_clause))
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
            if rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphanumeric() {
                let rest = rest.trim_start();
                // Make sure this is NOT PARTITION BY EXCLUDING
                if rest.starts_with("EXCLUDING")
                    && (rest.len() == 9 || !rest.as_bytes()[9].is_ascii_alphanumeric())
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
            if rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphanumeric() {
                let rest = rest.trim_start();
                if let Some(rest2) = rest.strip_prefix("EXCLUDING") {
                    if rest2.is_empty() || !rest2.as_bytes()[0].is_ascii_alphanumeric() {
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
    let (expr, window_spec) = parse_window_over_clause(&expr, entry_offset)?;

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
    let before_na = if let Some(na_start) = na_pos {
        let after_na = before_as[na_start + 16..].trim(); // "NON ADDITIVE BY" = 16 chars
        if !after_na.starts_with('(') {
            return Err(ParseError {
                message: format!("Expected '(' after NON ADDITIVE BY in metric entry '{entry}'."),
                position: Some(entry_offset + na_start + 16),
            });
        }
        let paren_content = extract_paren_content(after_na).ok_or_else(|| ParseError {
            message: format!("Unclosed '(' after NON ADDITIVE BY in metric entry '{entry}'."),
            position: Some(entry_offset + na_start + 16),
        })?;
        non_additive_by = parse_non_additive_dims(paren_content, entry_offset + na_start + 17)?;
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
        using_relationships = paren_content
            .split(',')
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .collect();
        before_na[..upos].trim()
    } else {
        before_na
    };

    // Check for dot to distinguish qualified vs unqualified
    if let Some(dot_pos) = final_name_portion.find('.') {
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

/// Parse the content inside DIMENSIONS or FACTS (...).
/// Returns `Vec<(source_alias, bare_name, expr, comment, synonyms, access)>`.
///
/// Each entry has the form: `[PRIVATE|PUBLIC] alias.name AS sql_expression [COMMENT = '...'] [WITH SYNONYMS = ('...')]`
///
/// `allow_access_modifier`: if false, PRIVATE/PUBLIC keywords produce a `ParseError` (used for DIMENSIONS).
/// `clause_name`: human-readable name for error messages ("dimensions" or "facts").
pub(crate) fn parse_qualified_entries(
    body: &str,
    base_offset: usize,
    allow_access_modifier: bool,
    clause_name: &str,
) -> Result<Vec<QualifiedEntry>, ParseError> {
    if body.trim().is_empty() {
        return Ok(vec![]);
    }

    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();

    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let parsed =
            parse_single_qualified_entry(entry, entry_offset, allow_access_modifier, clause_name)?;
        result.push(parsed);
    }

    Ok(result)
}

/// Parse one DIMENSIONS/FACTS entry: `[PRIVATE|PUBLIC] alias.bare_name AS expr [COMMENT = '...'] [WITH SYNONYMS = ('...')]`
fn parse_single_qualified_entry(
    entry: &str,
    entry_offset: usize,
    allow_access_modifier: bool,
    clause_name: &str,
) -> Result<QualifiedEntry, ParseError> {
    let entry = entry.trim();

    // Phase 43: Check for leading PRIVATE/PUBLIC keyword
    let (access, entry_after_access) = parse_leading_access_modifier(entry);
    if access == AccessModifier::Private && !allow_access_modifier {
        return Err(ParseError {
            message: format!(
                "PRIVATE is not supported on {clause_name}. Only facts and metrics can have access modifiers."
            ),
            position: Some(entry_offset),
        });
    }
    // Also reject explicit PUBLIC on dimensions (for consistency)
    if !allow_access_modifier
        && entry_after_access.len() != entry.trim().len()
        && access == AccessModifier::Public
    {
        // Check if it was an explicit PUBLIC keyword (entry was modified)
        let entry_trimmed = entry.trim();
        let entry_upper = entry_trimmed.to_ascii_uppercase();
        if entry_upper.starts_with("PUBLIC") {
            let after = &entry_trimmed["PUBLIC".len()..];
            if after.starts_with(|c: char| c.is_ascii_whitespace())
                && !after.trim_start().starts_with('.')
            {
                return Err(ParseError {
                    message: format!(
                        "PUBLIC is not supported on {clause_name}. Only facts and metrics can have access modifiers."
                    ),
                    position: Some(entry_offset),
                });
            }
        }
    }

    // Find first '.' to split alias.bare_name
    let dot_pos = entry_after_access.find('.').ok_or_else(|| ParseError {
        message: format!(
            "Expected 'alias.name' qualified identifier, got '{entry}'. Each dimension/metric entry must have the form 'alias.name AS expr'.",
        ),
        position: Some(entry_offset),
    })?;

    let source_alias = entry_after_access[..dot_pos].trim().to_string();
    if source_alias.is_empty() {
        return Err(ParseError {
            message: format!("Source alias before '.' is empty in entry '{entry}'."),
            position: Some(entry_offset),
        });
    }

    // Everything from dot+1 forward; find "AS" (case-insensitive, word boundary)
    let after_dot = &entry_after_access[dot_pos + 1..];
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

    let raw_expr = after_dot[as_pos + 2..].trim();
    if raw_expr.is_empty() {
        return Err(ParseError {
            message: format!("Missing expression after 'AS' in entry '{entry}'."),
            position: Some(entry_offset + dot_pos + 1 + as_pos + 2),
        });
    }

    // Phase 43: Parse trailing annotations from expression
    let (expr, annotations) = parse_trailing_annotations(raw_expr)?;

    Ok((
        source_alias,
        bare_name,
        expr,
        annotations.comment,
        annotations.synonyms,
        access,
    ))
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
    fn parse_tables_without_primary_key_is_valid() {
        // Phase 33: PRIMARY KEY is optional (fact tables)
        let result = parse_tables_clause("o AS orders", 0).unwrap();
        assert_eq!(result[0].alias, "o");
        assert_eq!(result[0].table, "orders");
        assert!(result[0].pk_columns.is_empty());
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
    // Phase 33: Cardinality keyword rejection + REFERENCES(cols) + UNIQUE tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_relationship_without_cardinality_defaults() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(
            result[0].cardinality,
            Cardinality::ManyToOne,
            "Cardinality should default to ManyToOne"
        );
    }

    #[test]
    fn old_cardinality_keywords_rejected() {
        // Phase 33: All cardinality keywords are rejected
        for input in [
            "rel AS a(fk) REFERENCES b MANY TO ONE",
            "rel AS a(fk) REFERENCES b ONE TO ONE",
            "rel AS a(fk) REFERENCES b ONE TO MANY",
        ] {
            let result = parse_relationships_clause(input, 0);
            assert!(
                result.is_err(),
                "Cardinality keyword should be rejected: {input}"
            );
            let err = result.unwrap_err();
            assert!(
                err.message.contains("no longer supported"),
                "Error should mention no longer supported for '{input}': {}",
                err.message
            );
        }
    }

    #[test]
    fn trailing_text_after_references_rejected() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b garbage", 0);
        assert!(result.is_err(), "Trailing text should be rejected");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Unexpected tokens")
                || err.message.contains("no longer supported"),
            "Error should mention unexpected tokens: {}",
            err.message
        );
    }

    #[test]
    fn references_with_column_list() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b(id)", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(result[0].ref_columns, vec!["id"]);
    }

    #[test]
    fn references_without_column_list() {
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert!(
            result[0].ref_columns.is_empty(),
            "ref_columns should be empty when no explicit column list"
        );
    }

    #[test]
    fn references_multi_column_list() {
        let result =
            parse_relationships_clause("rel AS a(fk1, fk2) REFERENCES b(col1, col2)", 0).unwrap();
        assert_eq!(result[0].ref_columns, vec!["col1", "col2"]);
    }

    #[test]
    fn references_target_no_space_before_paren() {
        // target(col) with no space between alias and paren
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b(id)", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(result[0].ref_columns, vec!["id"]);
    }

    #[test]
    fn references_target_space_before_paren() {
        // target (col) with space between alias and paren
        let result = parse_relationships_clause("rel AS a(fk) REFERENCES b (id)", 0).unwrap();
        assert_eq!(result[0].table, "b");
        assert_eq!(result[0].ref_columns, vec!["id"]);
    }

    #[test]
    fn unique_constraint_parsing() {
        let result = parse_tables_clause("o AS orders PRIMARY KEY (id) UNIQUE (email)", 0).unwrap();
        assert_eq!(result[0].unique_constraints.len(), 1);
        assert_eq!(result[0].unique_constraints[0], vec!["email"]);
    }

    #[test]
    fn multiple_unique_constraints() {
        let result = parse_tables_clause(
            "o AS orders PRIMARY KEY (id) UNIQUE (email) UNIQUE (first_name, last_name)",
            0,
        )
        .unwrap();
        assert_eq!(result[0].unique_constraints.len(), 2);
        assert_eq!(result[0].unique_constraints[0], vec!["email"]);
        assert_eq!(
            result[0].unique_constraints[1],
            vec!["first_name", "last_name"]
        );
    }

    #[test]
    fn table_without_primary_key() {
        let result = parse_tables_clause("f AS fact_table", 0).unwrap();
        assert_eq!(result[0].alias, "f");
        assert_eq!(result[0].table, "fact_table");
        assert!(result[0].pk_columns.is_empty());
        assert!(result[0].unique_constraints.is_empty());
    }

    #[test]
    fn table_with_unique_no_pk() {
        let result = parse_tables_clause("f AS fact_table UNIQUE (email)", 0).unwrap();
        assert_eq!(result[0].alias, "f");
        assert_eq!(result[0].table, "fact_table");
        assert!(result[0].pk_columns.is_empty());
        assert_eq!(result[0].unique_constraints.len(), 1);
        assert_eq!(result[0].unique_constraints[0], vec!["email"]);
    }

    // -----------------------------------------------------------------------
    // parse_qualified_entries tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_qualified_entries_simple() {
        let result =
            parse_qualified_entries("o.revenue AS SUM(amount)", 0, false, "dimensions").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "o"); // source_alias
        assert_eq!(result[0].1, "revenue"); // bare_name
        assert_eq!(result[0].2, "SUM(amount)"); // expr
    }

    #[test]
    fn parse_qualified_entries_nested_parens() {
        let result = parse_qualified_entries(
            "o.disc_price AS SUM(l_extendedprice * (1 - l_discount))",
            0,
            false,
            "dimensions",
        )
        .unwrap();
        assert_eq!(result[0].2, "SUM(l_extendedprice * (1 - l_discount))");
    }

    #[test]
    fn parse_qualified_entries_trailing_comma() {
        let result =
            parse_qualified_entries("o.revenue AS SUM(amount),", 0, false, "dimensions").unwrap();
        assert_eq!(
            result.len(),
            1,
            "Trailing comma must not produce extra entry"
        );
    }

    #[test]
    fn parse_qualified_entries_multiple_with_trailing_comma() {
        let result =
            parse_qualified_entries("o.a AS x, o.b AS y,", 0, false, "dimensions").unwrap();
        assert_eq!(result.len(), 2, "Expected 2 entries, got {:?}", result);
        assert_eq!(result[0].1, "a");
        assert_eq!(result[1].1, "b");
    }

    #[test]
    fn parse_qualified_entries_error_missing_dot() {
        let result = parse_qualified_entries("revenue AS SUM(amount)", 0, false, "dimensions");
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
        let result = parse_qualified_entries(body, 0, false, "dimensions").unwrap();
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
        let result =
            parse_qualified_entries("o.revenue as SUM(amount)", 0, false, "dimensions").unwrap();
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

    #[test]
    fn parse_keyword_body_facts_after_dimensions_rejected() {
        // FACTS must come before DIMENSIONS (order: tables, relationships, facts, dimensions, metrics)
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
        let result = parse_qualified_entries("revenue AS SUM(amount)", 0, false, "dimensions");
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

    // -----------------------------------------------------------------------
    // parse_metrics_clause USING tests (Phase 32 -- role-playing dimensions)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_using_single_relationship() {
        let result =
            parse_metrics_clause("f.departure_count USING (dep_airport) AS COUNT(*)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("f".to_string())); // source alias
        assert_eq!(result[0].1, "departure_count"); // bare_name
        assert_eq!(result[0].2, "COUNT(*)"); // expr
        assert_eq!(result[0].3, vec!["dep_airport"]); // using_relationships
    }

    #[test]
    fn parse_metrics_using_multiple_relationships() {
        let result = parse_metrics_clause("f.met USING (rel1, rel2) AS SUM(x)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("f".to_string()));
        assert_eq!(result[0].1, "met");
        assert_eq!(result[0].2, "SUM(x)");
        assert_eq!(result[0].3, vec!["rel1", "rel2"]);
    }

    #[test]
    fn parse_metrics_using_on_derived_produces_error() {
        // Derived metric (no dot prefix) with USING -> ParseError
        let result = parse_metrics_clause("derived_met USING (rel1) AS revenue - cost", 0);
        assert!(
            result.is_err(),
            "USING on derived metric must produce error"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("USING") && err.message.contains("derived"),
            "Error should mention USING and derived: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_without_using_backward_compat() {
        // Metric without USING still parses correctly
        let result = parse_metrics_clause("o.revenue AS SUM(o.amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("o".to_string()));
        assert_eq!(result[0].1, "revenue");
        assert_eq!(result[0].2, "SUM(o.amount)");
        assert!(result[0].3.is_empty(), "No USING -> empty relationships");
    }

    #[test]
    fn parse_metrics_using_case_insensitive() {
        let result =
            parse_metrics_clause("f.departure_count using (dep_airport) AS COUNT(*)", 0).unwrap();
        assert_eq!(result[0].3, vec!["dep_airport"]);

        let result2 =
            parse_metrics_clause("f.departure_count UsInG (dep_airport) AS COUNT(*)", 0).unwrap();
        assert_eq!(result2[0].3, vec!["dep_airport"]);
    }

    #[test]
    fn parse_keyword_body_with_using_metrics() {
        let body = "AS TABLES (f AS flights PRIMARY KEY (id), a AS airports PRIMARY KEY (id)) RELATIONSHIPS (dep_airport AS f(dep_id) REFERENCES a, arr_airport AS f(arr_id) REFERENCES a) DIMENSIONS (a.name AS airport_name) METRICS (f.departure_count USING (dep_airport) AS COUNT(*))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 1);
        assert_eq!(kb.metrics[0].name, "departure_count");
        assert_eq!(kb.metrics[0].using_relationships, vec!["dep_airport"]);
    }

    // -----------------------------------------------------------------------
    // Phase 43: COMMENT, SYNONYMS, PRIVATE/PUBLIC annotation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dimension_with_comment() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at COMMENT = 'Order date') METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.dimensions[0].comment.as_deref(), Some("Order date"));
    }

    #[test]
    fn test_dimension_with_synonyms() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at WITH SYNONYMS = ('purchase_date', 'order_date')) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(
            kb.dimensions[0].synonyms,
            vec!["purchase_date", "order_date"]
        );
    }

    #[test]
    fn test_comment_and_synonyms_either_order() {
        // COMMENT then SYNONYMS
        let body1 = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at COMMENT = 'test' WITH SYNONYMS = ('a')) METRICS (o.rev AS SUM(o.amount))";
        let kb1 = parse_keyword_body(body1, 0).unwrap();
        assert_eq!(kb1.dimensions[0].comment.as_deref(), Some("test"));
        assert_eq!(kb1.dimensions[0].synonyms, vec!["a"]);

        // SYNONYMS then COMMENT
        let body2 = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at WITH SYNONYMS = ('a') COMMENT = 'test') METRICS (o.rev AS SUM(o.amount))";
        let kb2 = parse_keyword_body(body2, 0).unwrap();
        assert_eq!(kb2.dimensions[0].comment.as_deref(), Some("test"));
        assert_eq!(kb2.dimensions[0].synonyms, vec!["a"]);
    }

    #[test]
    fn test_private_metric() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (PRIVATE o.cost AS SUM(o.cost))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Private);
    }

    #[test]
    fn test_explicit_public_metric() {
        let body =
            "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (PUBLIC o.revenue AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Public);
    }

    #[test]
    fn test_default_public_metric() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) METRICS (o.revenue AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Public);
    }

    #[test]
    fn test_private_fact() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) FACTS (PRIVATE o.raw_cost AS o.cost) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.facts[0].access, AccessModifier::Private);
    }

    #[test]
    fn test_private_dimension_rejected() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (PRIVATE o.date AS o.created_at) METRICS (o.rev AS SUM(o.amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err(), "PRIVATE on dimension must produce error");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("PRIVATE") || err.message.contains("not supported"),
            "Error should mention PRIVATE or not supported: {}",
            err.message
        );
        assert!(
            err.message.to_lowercase().contains("dimension"),
            "Error should mention dimension: {}",
            err.message
        );
    }

    #[test]
    fn test_escaped_quotes_in_comment() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.date AS o.created_at COMMENT = 'It''s a test') METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.dimensions[0].comment.as_deref(), Some("It's a test"));
    }

    #[test]
    fn test_comment_identifier_not_confused() {
        // Expression contains "comment_count" as an identifier -- must NOT trigger COMMENT annotation
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.flag AS CASE WHEN comment_count > 0 THEN 1 ELSE 0 END) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert!(
            kb.dimensions[0].comment.is_none(),
            "comment_count in expr must not trigger COMMENT annotation"
        );
        assert_eq!(
            kb.dimensions[0].expr,
            "CASE WHEN comment_count > 0 THEN 1 ELSE 0 END"
        );
    }

    #[test]
    fn test_table_with_comment_and_synonyms() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id) COMMENT = 'Orders table' WITH SYNONYMS = ('sales')) DIMENSIONS (o.region AS o.region) METRICS (o.rev AS SUM(o.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.tables[0].comment.as_deref(), Some("Orders table"));
        assert_eq!(kb.tables[0].synonyms, vec!["sales"]);
    }

    #[test]
    fn test_metric_with_using_and_comment() {
        let body = "AS TABLES (f AS flights PRIMARY KEY (id), a AS airports PRIMARY KEY (id)) RELATIONSHIPS (dep AS f(dep_id) REFERENCES a) DIMENSIONS (a.name AS a.name) METRICS (f.dep_count USING (dep) AS COUNT(*) COMMENT = 'Departures')";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].using_relationships, vec!["dep"]);
        assert_eq!(kb.metrics[0].comment.as_deref(), Some("Departures"));
    }

    #[test]
    fn test_private_keyword_disambiguation() {
        // Entry starting with table alias "private_schema.metric_name" should NOT be treated as PRIVATE
        // because "private_schema" is followed by "."
        let body = "AS TABLES (private_schema AS my_table PRIMARY KEY (id)) METRICS (private_schema.metric_name AS SUM(private_schema.value))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].access, AccessModifier::Public);
        assert_eq!(
            kb.metrics[0].source_table.as_deref(),
            Some("private_schema")
        );
        assert_eq!(kb.metrics[0].name, "metric_name");
    }

    #[test]
    fn test_full_keyword_body_with_all_metadata() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id) COMMENT = 'Order table') FACTS (o.net_price AS o.amount * 0.9 COMMENT = 'Net price' WITH SYNONYMS = ('discounted_price')) DIMENSIONS (o.region AS o.region WITH SYNONYMS = ('territory')) METRICS (o.revenue AS SUM(o.amount) COMMENT = 'Total revenue', PRIVATE o.cost AS SUM(o.cost))";
        let kb = parse_keyword_body(body, 0).unwrap();

        assert_eq!(kb.tables[0].comment.as_deref(), Some("Order table"));
        assert_eq!(kb.facts[0].comment.as_deref(), Some("Net price"));
        assert_eq!(kb.facts[0].synonyms, vec!["discounted_price"]);
        assert_eq!(kb.dimensions[0].synonyms, vec!["territory"]);
        assert_eq!(kb.metrics[0].comment.as_deref(), Some("Total revenue"));
        assert_eq!(kb.metrics[1].access, AccessModifier::Private);
    }

    // -----------------------------------------------------------------------
    // Phase 47: NON ADDITIVE BY tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_non_additive_by_single_dim_defaults() {
        let result = parse_metrics_clause("a.bal NON ADDITIVE BY (d1) AS SUM(x)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("a".to_string())); // source alias
        assert_eq!(result[0].1, "bal"); // bare name
        assert_eq!(result[0].2, "SUM(x)"); // expr
                                           // 8th element: non_additive_by
        assert_eq!(result[0].7.len(), 1);
        assert_eq!(result[0].7[0].dimension, "d1");
        assert_eq!(result[0].7[0].order, SortOrder::Asc);
        assert_eq!(result[0].7[0].nulls, NullsOrder::Last);
    }

    #[test]
    fn parse_metrics_non_additive_by_desc_nulls_first() {
        let result = parse_metrics_clause(
            "a.balance NON ADDITIVE BY (date_dim DESC) AS SUM(a.balance)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].7.len(), 1);
        assert_eq!(result[0].7[0].dimension, "date_dim");
        assert_eq!(result[0].7[0].order, SortOrder::Desc);
        // DESC defaults to NULLS FIRST
        assert_eq!(result[0].7[0].nulls, NullsOrder::First);
    }

    #[test]
    fn parse_metrics_non_additive_by_multiple_dims() {
        let result = parse_metrics_clause(
            "a.bal NON ADDITIVE BY (d1 DESC NULLS FIRST, d2 ASC NULLS LAST) AS SUM(x)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].7.len(), 2);
        assert_eq!(result[0].7[0].dimension, "d1");
        assert_eq!(result[0].7[0].order, SortOrder::Desc);
        assert_eq!(result[0].7[0].nulls, NullsOrder::First);
        assert_eq!(result[0].7[1].dimension, "d2");
        assert_eq!(result[0].7[1].order, SortOrder::Asc);
        assert_eq!(result[0].7[1].nulls, NullsOrder::Last);
    }

    #[test]
    fn parse_metrics_non_additive_by_on_derived_produces_error() {
        let result = parse_metrics_clause("profit NON ADDITIVE BY (d1) AS revenue - cost", 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("NON ADDITIVE BY"),
            "Error should mention NON ADDITIVE BY: {}",
            err.message
        );
        assert!(
            err.message.contains("derived"),
            "Error should mention derived: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_non_additive_by_with_using() {
        let result =
            parse_metrics_clause("a.bal USING (rel1) NON ADDITIVE BY (d1 DESC) AS SUM(x)", 0)
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("a".to_string()));
        assert_eq!(result[0].1, "bal");
        assert_eq!(result[0].3, vec!["rel1"]); // using_relationships
        assert_eq!(result[0].7.len(), 1);
        assert_eq!(result[0].7[0].dimension, "d1");
        assert_eq!(result[0].7[0].order, SortOrder::Desc);
    }

    #[test]
    fn parse_keyword_body_non_additive_by_integration() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.date_dim AS a.date) \
                     METRICS (a.balance NON ADDITIVE BY (date_dim DESC) AS SUM(a.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 1);
        assert_eq!(kb.metrics[0].name, "balance");
        assert_eq!(kb.metrics[0].non_additive_by.len(), 1);
        assert_eq!(kb.metrics[0].non_additive_by[0].dimension, "date_dim");
        assert_eq!(kb.metrics[0].non_additive_by[0].order, SortOrder::Desc);
    }

    #[test]
    fn parse_keyword_body_non_additive_by_invalid_dim_error() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.date_dim AS a.date) \
                     METRICS (a.balance NON ADDITIVE BY (nonexistent DESC) AS SUM(a.amount))";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("does not match any declared dimension"),
            "Error should mention dimension mismatch: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_non_additive_by_valid_dim_success() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.date_dim AS a.date, a.account AS a.account_id) \
                     METRICS (a.balance NON ADDITIVE BY (date_dim DESC, account) AS SUM(a.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].non_additive_by.len(), 2);
        assert_eq!(kb.metrics[0].non_additive_by[0].dimension, "date_dim");
        assert_eq!(kb.metrics[0].non_additive_by[1].dimension, "account");
    }

    #[test]
    fn parse_keyword_body_non_additive_by_case_insensitive_dim_match() {
        let body = "AS TABLES (a AS accounts PRIMARY KEY (id)) \
                     DIMENSIONS (a.Date_Dim AS a.date) \
                     METRICS (a.balance NON ADDITIVE BY (date_dim DESC) AS SUM(a.amount))";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics[0].non_additive_by.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Phase 48: Window function OVER clause tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_metrics_window_over_basic() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1, d2 ORDER BY d1)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, Some("o".to_string())); // source alias
        assert_eq!(result[0].1, "avg_qty"); // bare name
                                            // 9th element: window_spec
        let ws = result[0].8.as_ref().expect("window_spec should be Some");
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert!(ws.extra_args.is_empty());
        assert_eq!(ws.excluding_dims, vec!["d1", "d2"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "d1");
        assert_eq!(ws.order_by[0].order, SortOrder::Asc);
        assert_eq!(ws.order_by[0].nulls, NullsOrder::Last);
        assert!(ws.frame_clause.is_none());
    }

    #[test]
    fn parse_metrics_window_over_with_frame_clause() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1 ORDER BY d1 RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0].8.as_ref().expect("window_spec should be Some");
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert_eq!(ws.excluding_dims, vec!["d1"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "d1");
        assert_eq!(
            ws.frame_clause.as_deref(),
            Some("RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW")
        );
    }

    #[test]
    fn parse_metrics_window_lag_with_extra_args() {
        let result = parse_metrics_clause(
            "o.prev_qty AS LAG(total_qty, 30) OVER (PARTITION BY EXCLUDING d1 ORDER BY d1)",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        let ws = result[0].8.as_ref().expect("window_spec should be Some");
        assert_eq!(ws.window_function, "LAG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert_eq!(ws.extra_args, vec!["30"]);
        assert_eq!(ws.excluding_dims, vec!["d1"]);
    }

    #[test]
    fn parse_metrics_window_over_on_derived_produces_error() {
        let result = parse_metrics_clause(
            "avg_ratio AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1)",
            0,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("OVER clause not allowed on derived metric"),
            "Error should mention OVER on derived: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_window_over_with_non_additive_by_produces_error() {
        let result = parse_metrics_clause(
            "o.met NON ADDITIVE BY (d1) AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d2)",
            0,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("Cannot combine OVER clause with NON ADDITIVE BY"),
            "Error should mention mutual exclusion: {}",
            err.message
        );
    }

    #[test]
    fn parse_metrics_window_over_no_over_returns_none() {
        let result = parse_metrics_clause("o.revenue AS SUM(o.amount)", 0).unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            result[0].8.is_none(),
            "Regular metric should have no window_spec"
        );
    }

    #[test]
    fn parse_metrics_window_over_order_by_desc_nulls() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING d1 ORDER BY d1 DESC NULLS LAST)",
            0,
        )
        .unwrap();
        let ws = result[0].8.as_ref().unwrap();
        assert_eq!(ws.order_by[0].order, SortOrder::Desc);
        assert_eq!(ws.order_by[0].nulls, NullsOrder::Last);
    }

    #[test]
    fn parse_keyword_body_window_metric_integration() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region, o.month AS date_trunc('month', o.created_at)) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month)\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        assert!(kb.metrics[0].window_spec.is_none());
        let ws = kb.metrics[1]
            .window_spec
            .as_ref()
            .expect("second metric should have window_spec");
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert_eq!(ws.excluding_dims, vec!["region"]);
        assert_eq!(ws.order_by[0].expr, "month");
    }

    #[test]
    fn parse_keyword_body_window_excluding_invalid_dim_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING nonexistent ORDER BY region)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("EXCLUDING dimension"),
            "Error should mention EXCLUDING dimension: {}",
            err.message
        );
        assert!(
            err.message.contains("nonexistent"),
            "Error should mention the invalid dim: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_window_inner_metric_invalid_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(nonexistent_metric) OVER (PARTITION BY EXCLUDING region ORDER BY region)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("inner metric"),
            "Error should mention inner metric: {}",
            err.message
        );
        assert!(
            err.message.contains("nonexistent_metric"),
            "Error should mention the invalid metric: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_window_order_by_invalid_dim_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY bad_dim)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("ORDER BY dimension"),
            "Error should mention ORDER BY dimension: {}",
            err.message
        );
        assert!(
            err.message.contains("bad_dim"),
            "Error should mention the invalid dim: {}",
            err.message
        );
    }

    #[test]
    fn parse_keyword_body_window_valid_refs_succeed() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region, o.month AS date_trunc('month', o.created_at)) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month)\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        assert!(kb.metrics[1].window_spec.is_some());
    }

    #[test]
    fn parse_metrics_window_partition_by_explicit() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store ORDER BY date)",
            0,
        )
        .unwrap();
        let ws = result[0].8.as_ref().unwrap();
        assert_eq!(ws.window_function, "AVG");
        assert_eq!(ws.inner_metric, "total_qty");
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store"]);
        assert_eq!(ws.order_by.len(), 1);
        assert_eq!(ws.order_by[0].expr, "date");
    }

    #[test]
    fn parse_metrics_window_partition_by_multiple_dims() {
        let result = parse_metrics_clause(
            "o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store, region ORDER BY date DESC NULLS FIRST)",
            0,
        )
        .unwrap();
        let ws = result[0].8.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store", "region"]);
        assert_eq!(ws.order_by[0].order, SortOrder::Desc);
        assert_eq!(ws.order_by[0].nulls, NullsOrder::First);
    }

    #[test]
    fn parse_metrics_window_partition_by_no_order() {
        let result =
            parse_metrics_clause("o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store)", 0)
                .unwrap();
        let ws = result[0].8.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store"]);
        assert!(ws.order_by.is_empty());
    }

    #[test]
    fn parse_keyword_body_window_partition_by_integration() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.store AS o.store, o.date AS o.sale_date) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY store ORDER BY date)\
                     )";
        let kb = parse_keyword_body(body, 0).unwrap();
        assert_eq!(kb.metrics.len(), 2);
        let ws = kb.metrics[1].window_spec.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["store"]);
        assert_eq!(ws.order_by[0].expr, "date");
    }

    #[test]
    fn parse_keyword_body_window_partition_by_invalid_dim_error() {
        let body = "AS TABLES (o AS orders PRIMARY KEY (id)) \
                     DIMENSIONS (o.region AS o.region) \
                     METRICS (\
                         o.total_qty AS SUM(o.qty), \
                         o.avg_qty AS AVG(total_qty) OVER (PARTITION BY nonexistent ORDER BY region)\
                     )";
        let result = parse_keyword_body(body, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("PARTITION BY dimension"),
            "Error should mention PARTITION BY dimension: {}",
            err.message
        );
        assert!(
            err.message.contains("nonexistent"),
            "Error should mention the invalid dim: {}",
            err.message
        );
    }
}
