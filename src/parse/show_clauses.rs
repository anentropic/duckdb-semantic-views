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
use crate::errors::ParseError;
use crate::ident::{find_identifier_end, parse_qualified_identifier_with_quoting};
use crate::sql_lit::SqlLit;
use crate::util::{byte_offset_within, is_ident_byte, starts_with_keyword_ci};

/// Peel a (possibly double-quoted) identifier off the front of `rest`,
/// returning `(name, remaining)` with `remaining` already left-trimmed.
///
/// The scan is [`find_identifier_end`], the same quote-aware helper the
/// CREATE / DROP / DESCRIBE name slots use, so whitespace and `;` inside
/// `"..."` are part of the name rather than terminators. These slots
/// previously split on the first whitespace, which truncated a quoted name
/// mid-quote and left its tail to surface as "Unexpected tokens"
/// (TECH-DEBT #25 residual, the last of the whitespace-tokeniser family).
///
/// `allow_paren` is false, matching `extract_name_only`: no SHOW name slot is
/// followed by a parenthesised list, so `(` is an ordinary name byte here.
fn take_identifier(rest: &str) -> (&str, &str) {
    let end = find_identifier_end(rest, false);
    (&rest[..end], rest[end..].trim_start())
}

/// Split a captured scope name into its unquoted identifier parts.
///
/// Only the schema / database slots need this. Their value is emitted into a
/// `lower(schema_name) = lower('<literal>')` comparison and nothing downstream
/// unquotes it, so a surviving quote character is a **silent no-match** rather
/// than an error — `IN SCHEMA "main"` matched nothing at all. The view and
/// metric slots deliberately keep their raw text instead: their read table
/// function normalizes once at the catalog-read boundary (FF-4), and stripping
/// here as well would double-fold.
///
/// The parts are returned rather than rejoined because a dot is **significant**
/// here: `IN SCHEMA <db>.<schema>` is Snowflake's qualified form and has to
/// become two predicates. Rejoining produced `schema_name = 'memory.main'`
/// against a column holding a bare schema name — matching nothing, silently.
/// A quoted dot (`"a.b"`) is a single part and stays one, which is why this
/// goes through `parse_qualified_identifier_with_quoting` rather than
/// `split('.')`.
///
/// Case is **not** folded here. It is folded in the emitted SQL instead, on
/// both sides (see [`build_filter_suffix`]), so quoting still means nothing
/// extra — `"Main"` and `Main` produce identical predicates.
fn scope_name_parts(
    raw: &str,
    label: &str,
    position: Option<usize>,
) -> Result<Vec<String>, ParseError> {
    parse_qualified_identifier_with_quoting(raw)
        .map(|parts| parts.into_iter().map(|(part, _quoted)| part).collect())
        .map_err(|e| ParseError {
            message: format!("Invalid {label} name '{raw}': {e}"),
            position,
        })
}

/// Build optional WHERE and LIMIT suffix for a SHOW rewrite.
///
/// LIKE maps to `name ILIKE '<escaped>'` (case-insensitive).
/// STARTS WITH maps to `name LIKE '<escaped>%'` (case-sensitive).
/// IN SCHEMA maps to `lower(schema_name) = lower('<escaped>')`.
/// IN DATABASE maps to `lower(database_name) = lower('<escaped>')`.
/// All conditions combined with AND. LIMIT appended last.
///
/// The last two fold case because `DuckDB` resolves identifiers case-insensitively
/// and this project follows that rule everywhere else (`ident::ident_matches`).
/// They are the two slots where it also has to be *load-bearing*: the stored
/// side is stamped from `current_schema()` / `current_database()` at CREATE
/// time, which `DuckDB` returns as the spelling the caller last wrote in `USE`
/// rather than the catalog's, so two views in one schema can be stamped
/// `MySchema` and `myschema` and an exact match could return at most one.
///
/// The fold is left to SQL rather than applied to the requested name in Rust
/// because the stored side is folded by `DuckDB`'s Unicode-aware `lower()`,
/// while `ident::normalize_ident_part` is `to_ascii_lowercase`. Folding here
/// would emit `'myschÉma'` against a stored `'myschéma'` and break a non-ASCII
/// name that matches exactly today. `lower(x) = lower(y)` puts both sides
/// through the same function.
///
/// It stays an equality rather than becoming `ILIKE`: `SqlLit::escape` only
/// doubles single quotes, so a `%` or `_` in a schema name would turn into a
/// wildcard.
pub(crate) fn build_filter_suffix(
    like_pattern: Option<&str>,
    starts_with: Option<&str>,
    limit: Option<u64>,
    in_schema: Option<&str>,
    in_database: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(pattern) = like_pattern {
        let escaped = SqlLit::escape(pattern);
        parts.push(format!("name ILIKE '{escaped}'"));
    }
    if let Some(prefix) = starts_with {
        let escaped = SqlLit::escape(prefix);
        parts.push(format!("name LIKE '{escaped}%'"));
    }
    if let Some(schema) = in_schema {
        let escaped = SqlLit::escape(schema);
        parts.push(format!("lower(schema_name) = lower('{escaped}')"));
    }
    if let Some(db) = in_database {
        let escaped = SqlLit::escape(db);
        parts.push(format!("lower(database_name) = lower('{escaped}')"));
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
    /// Owned, unlike the other name slots: these two are the only ones whose
    /// value reaches SQL as a plain string literal with no downstream
    /// normalization, so their quotes are stripped here (see
    /// [`scope_name_parts`]) and the result no longer borrows the query.
    ///
    /// A qualified `IN SCHEMA <db>.<schema>` populates **both**, which is how
    /// the database half of Snowflake's qualified form gets applied.
    pub(crate) in_schema: Option<String>,
    pub(crate) in_database: Option<String>,
    pub(crate) for_metric: Option<&'a str>,
    pub(crate) starts_with: Option<String>,
    pub(crate) limit: Option<u64>,
}

/// Parse a keyword + identifier pair from text starting with IN.
///
/// Checks for `IN SCHEMA <name>` or `IN DATABASE <name>`.
/// Returns `(remaining_text, in_schema, in_database)`. `base` is the absolute
/// byte offset of `rest[0]` in the original query, so errors carry a caret
/// position pointing at the offending token (R-2).
fn parse_in_scope(
    rest: &str,
    base: usize,
) -> Result<(&str, Option<String>, Option<String>), ParseError> {
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
        return Err(ParseError {
            message: "SHOW SEMANTIC VIEWS requires IN SCHEMA <name> or IN DATABASE <name>"
                .to_string(),
            position: Some(base + byte_offset_within(rest, after_in)),
        });
    };

    let after_kw = after_in[kw_len..].trim_start();
    if after_kw.is_empty() {
        return Err(ParseError {
            message: format!("Missing {label} name after IN {keyword}"),
            position: Some(base + byte_offset_within(rest, after_kw)),
        });
    }
    let (raw_name, remaining) = take_identifier(after_kw);
    let position = Some(base + byte_offset_within(rest, after_kw));
    let mut parts = scope_name_parts(raw_name, label, position)?;

    // Arity is what distinguishes the two slots. `IN SCHEMA` takes Snowflake's
    // `<schema>` or `<db>.<schema>`; `IN DATABASE` takes one part, because a
    // database has nothing to qualify it with. Anything longer used to be
    // rejoined into a literal that could never match, so it is an error now
    // rather than an empty result set.
    let too_many = |expected: &str| ParseError {
        message: format!("Invalid {label} name '{raw_name}': expected {expected}"),
        position,
    };

    if keyword == "SCHEMA" {
        match parts.len() {
            1 => Ok((remaining, parts.pop(), None)),
            // Pop schema first — it is the LAST part; the database qualifier
            // precedes it. Both predicates are emitted, so a same-named schema
            // in another database does not match.
            2 => {
                let schema = parts.pop();
                let database = parts.pop();
                Ok((remaining, schema, database))
            }
            _ => Err(too_many("<schema> or <database>.<schema>")),
        }
    } else if parts.len() == 1 {
        Ok((remaining, None, parts.pop()))
    } else {
        Err(too_many("a single <database>"))
    }
}

/// Parse FOR METRIC clause (only valid for `ShowDimensions`).
///
/// Returns `(remaining_text, metric_name)`. `base` is the absolute byte offset
/// of `rest[0]` in the original query (R-2).
fn parse_for_metric(rest: &str, base: usize) -> Result<(&str, &str), ParseError> {
    let after_for = rest[3..].trim_start();
    // Word boundary after METRIC: `FOR METRICS x` must not parse as the
    // METRIC keyword followed by a metric named `s x` (PR #50 review).
    let metric_boundary_ok = starts_with_keyword_ci(after_for, "METRIC")
        && (after_for.len() == 6 || after_for.as_bytes()[6].is_ascii_whitespace());
    if !metric_boundary_ok {
        return Err(ParseError {
            message: "Expected FOR METRIC after view name. \
                 Usage: SHOW SEMANTIC DIMENSIONS [LIKE '<pattern>'] [IN view_name] \
                 [FOR METRIC metric_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
                .to_string(),
            position: Some(base + byte_offset_within(rest, after_for)),
        });
    }
    let after_metric = after_for[6..].trim_start();
    if after_metric.is_empty() {
        return Err(ParseError {
            message: "Missing metric name after FOR METRIC".to_string(),
            position: Some(base + byte_offset_within(rest, after_metric)),
        });
    }
    let (metric_name, remaining) = take_identifier(after_metric);
    Ok((remaining, metric_name))
}

/// Parse optional SHOW SEMANTIC filter clauses from text after the prefix.
///
/// Clause order (Snowflake): LIKE, IN, FOR METRIC, STARTS WITH, LIMIT.
///
/// `base` is the absolute byte offset of `after_prefix[0]` in the original
/// query; every error carries a caret position pointing at its offending token
/// (R-2). Errors resolve to absolute offsets via [`byte_offset_within`] against
/// `after_prefix`, so the parser never threads manual byte counters.
#[allow(clippy::too_many_lines)]
pub(crate) fn parse_show_filter_clauses<'a>(
    after_prefix: &'a str,
    kind: DdlKind,
    base: usize,
) -> Result<ShowClauses<'a>, ParseError> {
    // Absolute offset of a subslice of `after_prefix`, for caret positions.
    let abs = |sub: &str| base + byte_offset_within(after_prefix, sub);
    let mut rest = after_prefix.trim();
    let mut like_pattern: Option<String> = None;
    let mut in_view: Option<&'a str> = None;
    let mut in_schema: Option<String> = None;
    let mut in_database: Option<String> = None;
    let mut for_metric: Option<&'a str> = None;
    let mut starts_with: Option<String> = None;
    let mut limit: Option<u64> = None;

    let entity = match kind {
        DdlKind::Show | DdlKind::ShowTerse => "VIEWS",
        DdlKind::ShowDimensions => "DIMENSIONS",
        DdlKind::ShowMetrics => "METRICS",
        DdlKind::ShowMaterializations => "MATERIALIZATIONS",
        // ShowFacts (and any other kind plan_ddl routes here) -> FACTS.
        _ => "FACTS",
    };

    // 1. Check for LIKE keyword
    if starts_with_keyword_ci(rest, "LIKE") {
        // Ensure it's followed by whitespace (not just a prefix match)
        if rest.len() == 4 || rest.as_bytes()[4].is_ascii_whitespace() {
            rest = rest[4..].trim_start();
            let (pattern, consumed) =
                extract_quoted_string(rest).map_err(|message| ParseError {
                    message,
                    position: Some(abs(rest)),
                })?;
            like_pattern = Some(pattern);
            rest = rest[consumed..].trim_start();
        }
    }

    // 2. Check for IN keyword
    if starts_with_keyword_ci(rest, "IN")
        && (rest.len() == 2 || rest.as_bytes()[2].is_ascii_whitespace())
    {
        if kind == DdlKind::Show || kind == DdlKind::ShowTerse {
            let (remaining, schema, database) = parse_in_scope(rest, abs(rest))?;
            rest = remaining;
            in_schema = schema;
            in_database = database;
        } else {
            rest = rest[2..].trim_start();
            if rest.is_empty() {
                return Err(ParseError {
                    message: "Missing view name after IN".to_string(),
                    position: Some(abs(rest)),
                });
            }
            let (view_name, remaining) = take_identifier(rest);
            in_view = Some(view_name);
            rest = remaining;
        }
    }

    // 3. Check for FOR METRIC (only for ShowDimensions). Word boundary
    // enforced so e.g. FOREIGN does not match FOR (PR #50 review).
    if starts_with_keyword_ci(rest, "FOR")
        && (rest.len() == 3 || rest.as_bytes()[3].is_ascii_whitespace())
    {
        if kind != DdlKind::ShowDimensions {
            return Err(ParseError {
                message: format!(
                    "FOR METRIC is only valid for SHOW SEMANTIC DIMENSIONS, not SHOW SEMANTIC {entity}"
                ),
                position: Some(abs(rest)),
            });
        }
        let (remaining, metric_name) = parse_for_metric(rest, abs(rest))?;
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
            return Err(ParseError {
                message: format!(
                    "Expected STARTS WITH. \
                     Usage: SHOW SEMANTIC {entity} [LIKE '<pattern>'] [IN view_name] [STARTS WITH '<prefix>'] [LIMIT <n>]"
                ),
                position: Some(abs(rest)),
            });
        }
        rest = rest[4..].trim_start();
        let (prefix, consumed) = extract_quoted_string(rest).map_err(|message| ParseError {
            message,
            position: Some(abs(rest)),
        })?;
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
        let n: u64 = token.parse().map_err(|_| ParseError {
            message: format!("LIMIT must be a positive integer, got: '{token}'"),
            position: Some(abs(rest)),
        })?;
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
        return Err(ParseError {
            message: usage,
            position: Some(abs(rest)),
        });
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

#[cfg(test)]
mod tests {
    use super::build_filter_suffix;

    // R-1 (code-review 2026-07-11): every user-supplied filter value is
    // embedded in a single-quoted SQL literal via `SqlLit`, so `'` doubles to
    // `''` and no lone quote can break out of the literal. These assertions
    // pin the escaped output the manual `.replace('\'', "''")` used to produce,
    // now routed through the single escaping boundary.
    #[test]
    fn filter_suffix_escapes_single_quotes() {
        assert_eq!(
            build_filter_suffix(Some("O'Brien"), None, None, None, None),
            " WHERE name ILIKE 'O''Brien'"
        );
        assert_eq!(
            build_filter_suffix(None, Some("O'Br"), None, None, None),
            " WHERE name LIKE 'O''Br%'"
        );
        assert_eq!(
            build_filter_suffix(None, None, None, Some("sch'ema"), Some("d'b")),
            " WHERE lower(schema_name) = lower('sch''ema') AND lower(database_name) = lower('d''b')"
        );
    }

    #[test]
    fn filter_suffix_plain_values_and_limit_unchanged() {
        assert_eq!(
            build_filter_suffix(Some("cust%"), None, Some(10), None, None),
            " WHERE name ILIKE 'cust%' LIMIT 10"
        );
        assert_eq!(build_filter_suffix(None, None, None, None, None), "");
    }
}
