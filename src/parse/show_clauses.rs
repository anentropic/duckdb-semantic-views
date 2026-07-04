//! Filter-clause parsing for `SHOW SEMANTIC ...` DDL statements.
//!
//! Extracted from `parse` (AR-1) so the god-module stays focused on
//! detection and rewrite dispatch. These functions parse the optional
//! `LIKE` / `IN` / `FOR METRIC` / `STARTS WITH` / `LIMIT` suffix of a
//! `SHOW SEMANTIC {VIEWS,DIMENSIONS,METRICS,FACTS}` command into a
//! [`ShowClauses`] struct, and render that back into a SQL `WHERE`/`LIMIT`
//! suffix ([`build_filter_suffix`]).
//!
//! `plan_ddl` in the parent module is the sole caller: it invokes
//! [`parse_show_filter_clauses`] then [`build_filter_suffix`] to produce
//! the rewritten catalog query. Single-quoted argument extraction is shared
//! with the rest of the parser, so it stays in the parent module and is
//! referenced here via `super::extract_quoted_string`.

use super::extract_quoted_string;
use super::DdlKind;
use crate::util::{is_ident_byte, starts_with_keyword_ci};

/// Build optional WHERE and LIMIT suffix for a SHOW rewrite.
///
/// LIKE maps to `name ILIKE '<escaped>'` (case-insensitive).
/// STARTS WITH maps to `name LIKE '<escaped>%'` (case-sensitive).
/// IN SCHEMA maps to `schema_name = '<escaped>'`.
/// IN DATABASE maps to `database_name = '<escaped>'`.
/// All conditions combined with AND. LIMIT appended last.
pub(crate) fn build_filter_suffix(
    like_pattern: Option<&str>,
    starts_with: Option<&str>,
    limit: Option<u64>,
    in_schema: Option<&str>,
    in_database: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(pattern) = like_pattern {
        let escaped = pattern.replace('\'', "''");
        parts.push(format!("name ILIKE '{escaped}'"));
    }
    if let Some(prefix) = starts_with {
        let escaped = prefix.replace('\'', "''");
        parts.push(format!("name LIKE '{escaped}%'"));
    }
    if let Some(schema) = in_schema {
        let escaped = schema.replace('\'', "''");
        parts.push(format!("schema_name = '{escaped}'"));
    }
    if let Some(db) = in_database {
        let escaped = db.replace('\'', "''");
        parts.push(format!("database_name = '{escaped}'"));
    }
    let mut suffix = String::new();
    if !parts.is_empty() {
        suffix.push_str(" WHERE ");
        suffix.push_str(&parts.join(" AND "));
    }
    if let Some(n) = limit {
        use std::fmt::Write;
        let _ = write!(suffix, " LIMIT {n}");
    }
    suffix
}

/// Parsed filter clauses from a SHOW SEMANTIC command.
pub(crate) struct ShowClauses<'a> {
    pub(crate) like_pattern: Option<String>,
    pub(crate) in_view: Option<&'a str>,
    pub(crate) in_schema: Option<&'a str>,
    pub(crate) in_database: Option<&'a str>,
    pub(crate) for_metric: Option<&'a str>,
    pub(crate) starts_with: Option<String>,
    pub(crate) limit: Option<u64>,
}

/// Parse a keyword + identifier pair from text starting with IN.
///
/// Checks for `IN SCHEMA <name>` or `IN DATABASE <name>`.
/// Returns `(remaining_text, in_schema, in_database)`.
fn parse_in_scope(rest: &str) -> Result<(&str, Option<&str>, Option<&str>), String> {
    let after_in = rest[2..].trim_start();

    // Try to match a keyword (SCHEMA or DATABASE) followed by an identifier.
    let (keyword, kw_len, label) = if starts_with_keyword_ci(after_in, "SCHEMA")
        && (after_in.len() == 6 || after_in.as_bytes()[6].is_ascii_whitespace())
    {
        ("SCHEMA", 6, "schema")
    } else if starts_with_keyword_ci(after_in, "DATABASE")
        && (after_in.len() == 8 || after_in.as_bytes()[8].is_ascii_whitespace())
    {
        ("DATABASE", 8, "database")
    } else {
        return Err(
            "SHOW SEMANTIC VIEWS requires IN SCHEMA <name> or IN DATABASE <name>".to_string(),
        );
    };

    let after_kw = after_in[kw_len..].trim_start();
    if after_kw.is_empty() {
        return Err(format!("Missing {label} name after IN {keyword}"));
    }
    let name_end = after_kw
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_kw.len());
    let name = &after_kw[..name_end];
    let remaining = after_kw[name_end..].trim_start();

    if keyword == "SCHEMA" {
        Ok((remaining, Some(name), None))
    } else {
        Ok((remaining, None, Some(name)))
    }
}

/// Parse FOR METRIC clause (only valid for `ShowDimensions`).
///
/// Returns `(remaining_text, metric_name)`.
fn parse_for_metric<'a>(rest: &'a str, entity: &str) -> Result<(&'a str, &'a str), String> {
    let after_for = rest[3..].trim_start();
    // Word boundary after METRIC: `FOR METRICS x` must not parse as the
    // METRIC keyword followed by a metric named `s x` (PR #50 review).
    let metric_boundary_ok = starts_with_keyword_ci(after_for, "METRIC")
        && (after_for.len() == 6 || after_for.as_bytes()[6].is_ascii_whitespace());
    if !metric_boundary_ok {
        return Err("Expected FOR METRIC after view name. \
             Usage: SHOW SEMANTIC DIMENSIONS [LIKE '<pattern>'] [IN view_name] \
             [FOR METRIC metric_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            .to_string());
    }
    let _ = entity;
    let after_metric = after_for[6..].trim_start();
    if after_metric.is_empty() {
        return Err("Missing metric name after FOR METRIC".to_string());
    }
    let name_end = after_metric
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_metric.len());
    Ok((
        after_metric[name_end..].trim_start(),
        &after_metric[..name_end],
    ))
}

/// Parse optional SHOW SEMANTIC filter clauses from text after the prefix.
///
/// Clause order (Snowflake): LIKE, IN, FOR METRIC, STARTS WITH, LIMIT.
#[allow(clippy::too_many_lines)]
pub(crate) fn parse_show_filter_clauses<'a>(
    after_prefix: &'a str,
    kind: DdlKind,
) -> Result<ShowClauses<'a>, String> {
    let mut rest = after_prefix.trim();
    let mut like_pattern: Option<String> = None;
    let mut in_view: Option<&'a str> = None;
    let mut in_schema: Option<&'a str> = None;
    let mut in_database: Option<&'a str> = None;
    let mut for_metric: Option<&'a str> = None;
    let mut starts_with: Option<String> = None;
    let mut limit: Option<u64> = None;

    let entity = match kind {
        DdlKind::Show | DdlKind::ShowTerse => "VIEWS",
        DdlKind::ShowDimensions => "DIMENSIONS",
        DdlKind::ShowMetrics => "METRICS",
        _ => "FACTS",
    };

    // 1. Check for LIKE keyword
    if starts_with_keyword_ci(rest, "LIKE") {
        // Ensure it's followed by whitespace (not just a prefix match)
        if rest.len() == 4 || rest.as_bytes()[4].is_ascii_whitespace() {
            rest = rest[4..].trim_start();
            let (pattern, consumed) = extract_quoted_string(rest)?;
            like_pattern = Some(pattern);
            rest = rest[consumed..].trim_start();
        }
    }

    // 2. Check for IN keyword
    if starts_with_keyword_ci(rest, "IN")
        && (rest.len() == 2 || rest.as_bytes()[2].is_ascii_whitespace())
    {
        if kind == DdlKind::Show || kind == DdlKind::ShowTerse {
            let (remaining, schema, database) = parse_in_scope(rest)?;
            rest = remaining;
            in_schema = schema;
            in_database = database;
        } else {
            rest = rest[2..].trim_start();
            if rest.is_empty() {
                return Err("Missing view name after IN".to_string());
            }
            let name_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
            in_view = Some(&rest[..name_end]);
            rest = rest[name_end..].trim_start();
        }
    }

    // 3. Check for FOR METRIC (only for ShowDimensions). Word boundary
    // enforced so e.g. FOREIGN does not match FOR (PR #50 review).
    if starts_with_keyword_ci(rest, "FOR")
        && (rest.len() == 3 || rest.as_bytes()[3].is_ascii_whitespace())
    {
        if kind != DdlKind::ShowDimensions {
            return Err(format!(
                "FOR METRIC is only valid for SHOW SEMANTIC DIMENSIONS, not SHOW SEMANTIC {entity}"
            ));
        }
        let (remaining, metric_name) = parse_for_metric(rest, entity)?;
        rest = remaining;
        for_metric = Some(metric_name);
    }

    // 4. Check for STARTS WITH. Word boundaries enforced (PA-10:
    // `STARTSWITH 'a'` used to be accepted).
    if starts_with_keyword_ci(rest, "STARTS")
        && (rest.len() == 6 || rest.as_bytes()[6].is_ascii_whitespace())
    {
        rest = rest[6..].trim_start();
        // Word boundary after WITH: `_` and non-ASCII bytes are identifier
        // continuation (mirrors match_keyword_prefix), so WITH_x / WITHé do
        // not match the keyword.
        let with_boundary_ok = starts_with_keyword_ci(rest, "WITH")
            && (rest.len() == 4 || !is_ident_byte(rest.as_bytes()[4]));
        if !with_boundary_ok {
            return Err(format!(
                "Expected STARTS WITH. \
                 Usage: SHOW SEMANTIC {entity} [LIKE '<pattern>'] [IN view_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            ));
        }
        rest = rest[4..].trim_start();
        let (prefix, consumed) = extract_quoted_string(rest)?;
        starts_with = Some(prefix);
        rest = rest[consumed..].trim_start();
    }

    // 5. Check for LIMIT. Word boundary enforced (PA-10: `LIMIT5` used to
    // be accepted).
    if starts_with_keyword_ci(rest, "LIMIT")
        && (rest.len() == 5 || rest.as_bytes()[5].is_ascii_whitespace())
    {
        rest = rest[5..].trim_start();
        let token_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        let token = &rest[..token_end];
        let n: u64 = token
            .parse()
            .map_err(|_| format!("LIMIT must be a positive integer, got: '{token}'"))?;
        limit = Some(n);
        rest = rest[token_end..].trim_start();
    }

    // 6. If any text remains, error with usage hint
    if !rest.is_empty() {
        let usage = if kind == DdlKind::ShowDimensions {
            format!(
                "Unexpected tokens: '{rest}'. \
                 Usage: SHOW SEMANTIC DIMENSIONS [LIKE '<pattern>'] [IN view_name] [FOR METRIC metric_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            )
        } else {
            format!(
                "Unexpected tokens: '{rest}'. \
                 Usage: SHOW SEMANTIC {entity} [LIKE '<pattern>'] [IN view_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
            )
        };
        return Err(usage);
    }

    Ok(ShowClauses {
        like_pattern,
        in_view,
        in_schema,
        in_database,
        for_metric,
        starts_with,
        limit,
    })
}
