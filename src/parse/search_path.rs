//! Rewrite-time injection of the caller's schema resolution order (TECH-DEBT
//! #25 / #19).
//!
//! # Why this exists
//!
//! `DuckDB` resolves an unqualified object name through the session's search
//! path. The read-side table functions cannot: each binds on a fresh
//! `Connection(*context.db)` that carries neither the caller's search path nor
//! its current schema, and neither is reachable from the bind callback — the
//! caller's `ClientContext` cannot be queried re-entrantly (its `context_lock`
//! is not recursive) and `ClientData::catalog_search_path` is not exposed by
//! the amalgamation. Entry #19 records both walls.
//!
//! What *is* available is the caller's connection at **bind time**: `DuckDB`
//! constant-folds table-function arguments there, on the caller's connection,
//! before the bind callback runs. So the path can be handed in as an ordinary
//! argument, and the parser override is the one place that sees every
//! statement and can add it.
//!
//! This module finds read-table-function calls in a statement and appends
//! `search_path := <expression>` to each. The expression is evaluated by
//! `DuckDB`, not by us — we only splice the text in.
//!
//! # What is deliberately NOT rewritten
//!
//! `get_ddl` and `read_yaml_from_semantic_view` are **scalar** functions, which
//! have no named parameters; giving them the path needs a second registered
//! arity. They keep the no-path fallback (the unique match, else an ambiguity
//! error) — conservative rather than wrong. See TECH-DEBT #25.

use std::borrow::Cow;

/// SQL yielding the caller's schema resolution order, most significant first.
///
/// `current_schemas(false)` is what `SET search_path` / `USE` set, and is
/// **empty** on a fresh connection — so `current_schema()` is appended to cover
/// the default. Both are ordinary scalar functions, which is what makes this
/// foldable in a table-function argument.
///
/// Duplicates are possible (`USE a` makes `current_schemas(false)` already
/// contain `a`) and harmless: resolution takes the first match in order, so a
/// repeat never changes the answer.
///
/// `current_schemas(true)` is deliberately not used. It prepends the *temp*
/// schema, which is also called `main` — so with a real schema named `main`
/// later on the path, the temp entry would win and silently resolve to the
/// wrong one.
pub(crate) const SEARCH_PATH_SQL: &str = "list_concat(current_schemas(false), [current_schema()])";

/// The read table functions that take a view name and therefore need the path.
///
/// The `_all` variants (`list_semantic_views`, `show_semantic_*_all`) list
/// every view rather than resolving one, so they are absent: injecting into
/// them would be harmless but pointless noise in the emitted SQL.
const RESOLVING_READ_FUNCTIONS: &[&str] = &[
    "semantic_view",
    "explain_semantic_view",
    "describe_semantic_view",
    "show_columns_in_semantic_view",
    "show_semantic_dimensions",
    "show_semantic_metrics",
    "show_semantic_facts",
    "show_semantic_materializations",
    "show_semantic_dimensions_for_metric",
];

/// Cheap reject for the overwhelming majority of statements.
///
/// The parser override runs on **every** statement, so the common path has to
/// be a substring scan and nothing more. Every name in
/// [`RESOLVING_READ_FUNCTIONS`] contains `semantic_view` except the four
/// `show_semantic_*` entities, which all contain `semantic_`; `semantic_` is a
/// prefix of `semantic_view`, so it alone covers the set.
///
/// Matched case-insensitively — SQL function names are, and a plain
/// `contains("semantic_")` silently skipped `SEMANTIC_VIEW('v')` entirely,
/// which would have left uppercase call sites resolving without a path. The
/// byte-window compare keeps the reject path allocation-free.
fn might_contain_read_call(sql: &str) -> bool {
    const NEEDLE: &[u8] = b"semantic_";
    sql.as_bytes()
        .windows(NEEDLE.len())
        .any(|w| w.eq_ignore_ascii_case(NEEDLE))
}

/// Byte offset of the `)` matching the `(` at `open`, or `None` when the call
/// is unterminated.
///
/// Skips single-quoted and `$tag$` literal content so a `)` inside a string —
/// `semantic_view('a)b')` — is not mistaken for the close. Reuses the
/// tokenizer's own literal skippers so this agrees with how the rest of the
/// parser reads the same bytes.
fn matching_close_paren(sql: &str, open: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    debug_assert_eq!(bytes.get(open), Some(&b'('));
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = crate::expr_tokens::skip_single_quoted(bytes, i),
            b'$' => {
                if let Some(next) = crate::expr_tokens::try_skip_dollar_quoted(bytes, i) {
                    i = next;
                    continue;
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// True when this call's own argument list already names `search_path` as a
/// named parameter.
///
/// A plain `contains("search_path")` over the argument text was wrong twice
/// over. It missed a caller's `SEARCH_PATH := …` — SQL named parameters are
/// case-insensitive, so that *is* the same argument, and appending a second one
/// yields a duplicate-parameter error rather than the resolution asked for. And
/// it treated a view whose name merely contains the text —
/// `semantic_view('my_search_path_view')` — as if the argument were present,
/// silently skipping injection for exactly those views.
///
/// Looking for the identifier followed by `:=` gets both right:
/// [`crate::expr_tokens::scan_references`] already skips literal content and
/// folds case through the project's one identifier-match rule, so a
/// `where_clause` predicate mentioning `search_path` is literal text here, not
/// an argument.
fn has_search_path_argument(args: &str) -> bool {
    crate::expr_tokens::scan_references(args)
        .into_iter()
        .any(|r| r.key() == "search_path" && args[r.end..].trim_start().starts_with(":="))
}

/// Append `search_path := …` to every resolving read-function call in `sql`.
///
/// Returns the input untouched (as `Cow::Borrowed`) when there is nothing to
/// do, which is the common case for arbitrary SQL.
///
/// A call that already carries a `search_path` argument is left alone, so an
/// explicit one the caller wrote wins and re-running this is idempotent — the
/// override can see the same text twice (`plan_rewrite` is invoked a second
/// time to recover error positions, AR-5).
pub(crate) fn inject_search_path(sql: &str) -> Cow<'_, str> {
    if !might_contain_read_call(sql) {
        return Cow::Borrowed(sql);
    }
    // Collect insertion points first: rewriting as we scan would invalidate the
    // offsets the scanner reports.
    let mut insert_at: Vec<usize> = Vec::new();
    for head in crate::expr_tokens::scan_function_heads(sql) {
        let name = crate::ident::normalize_ident_part(head.raw);
        if !RESOLVING_READ_FUNCTIONS.contains(&name.as_str()) {
            continue;
        }
        let bytes = sql.as_bytes();
        let mut open = head.end;
        while open < bytes.len() && bytes[open].is_ascii_whitespace() {
            open += 1;
        }
        if bytes.get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = matching_close_paren(sql, open) else {
            continue;
        };
        // An explicit `search_path` named argument in this call's own list
        // means the caller (or an earlier pass) already supplied one.
        if has_search_path_argument(&sql[open..close]) {
            continue;
        }
        insert_at.push(close);
    }
    if insert_at.is_empty() {
        return Cow::Borrowed(sql);
    }
    let mut out = String::with_capacity(sql.len() + insert_at.len() * 64);
    let mut prev = 0usize;
    for close in insert_at {
        out.push_str(&sql[prev..close]);
        out.push_str(", search_path := ");
        out.push_str(SEARCH_PATH_SQL);
        prev = close;
    }
    out.push_str(&sql[prev..]);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The injected suffix, for expectations that assert on the whole string.
    fn suffix() -> String {
        format!(", search_path := {SEARCH_PATH_SQL}")
    }

    #[test]
    fn a_statement_with_no_read_call_is_untouched() {
        for sql in [
            "SELECT 1",
            "SELECT * FROM orders WHERE region = 'north'",
            "CREATE TABLE t (a INT)",
        ] {
            assert!(
                matches!(inject_search_path(sql), Cow::Borrowed(_)),
                "must not allocate for {sql:?}"
            );
        }
    }

    #[test]
    fn a_read_call_gains_the_named_parameter() {
        let got = inject_search_path("SELECT * FROM semantic_view('v')");
        assert_eq!(got, format!("SELECT * FROM semantic_view('v'{})", suffix()));
    }

    #[test]
    fn existing_named_parameters_are_preserved() {
        let got = inject_search_path("SELECT * FROM semantic_view('v', metrics := ['total'])");
        assert_eq!(
            got,
            format!(
                "SELECT * FROM semantic_view('v', metrics := ['total']{})",
                suffix()
            )
        );
    }

    #[test]
    fn every_call_in_one_statement_is_injected() {
        let got = inject_search_path(
            "SELECT * FROM semantic_view('a') UNION ALL SELECT * FROM semantic_view('b')",
        );
        assert_eq!(got.matches("search_path :=").count(), 2, "{got}");
    }

    #[test]
    fn a_nested_call_argument_does_not_confuse_the_paren_match() {
        // The `)` of `upper(...)` must not be taken for the call's own close.
        let got = inject_search_path("SELECT * FROM semantic_view(upper('v'))");
        assert_eq!(
            got,
            format!("SELECT * FROM semantic_view(upper('v'){})", suffix())
        );
    }

    #[test]
    fn a_paren_inside_a_string_literal_is_not_the_close() {
        // Without literal-aware scanning the `)` inside `'a)b'` ends the call
        // early and the injected text lands in the middle of the argument.
        let got = inject_search_path("SELECT * FROM semantic_view('a)b')");
        assert_eq!(
            got,
            format!("SELECT * FROM semantic_view('a)b'{})", suffix())
        );
    }

    #[test]
    fn a_function_name_inside_a_string_literal_is_not_a_call() {
        let sql = "SELECT 'semantic_view(x)' AS lit";
        assert!(
            matches!(inject_search_path(sql), Cow::Borrowed(_)),
            "a literal mentioning the function is not a call: {sql}"
        );
    }

    #[test]
    fn the_all_variants_are_left_alone() {
        // They list every view rather than resolving one name, so the path
        // would be noise.
        for sql in [
            "SELECT * FROM list_semantic_views()",
            "SELECT * FROM show_semantic_metrics_all()",
        ] {
            assert!(
                matches!(inject_search_path(sql), Cow::Borrowed(_)),
                "must not inject into {sql}"
            );
        }
    }

    #[test]
    fn injection_is_idempotent() {
        // `plan_rewrite` runs a second time to recover error positions (AR-5),
        // so the same text can pass through twice.
        let once = inject_search_path("SELECT * FROM semantic_view('v')").into_owned();
        let twice = inject_search_path(&once).into_owned();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_caller_supplied_search_path_wins() {
        let sql = "SELECT * FROM semantic_view('v', search_path := ['analytics'])";
        assert!(
            matches!(inject_search_path(sql), Cow::Borrowed(_)),
            "an explicit path must not be overridden: {sql}"
        );
    }

    #[test]
    fn a_caller_supplied_search_path_wins_whatever_its_case() {
        // SQL named parameters are case-insensitive, so this IS the same
        // argument. Appending a second one produces a duplicate-parameter
        // error rather than the resolution the caller asked for.
        // (Raised by review on PR #187.)
        for sql in [
            "SELECT * FROM semantic_view('v', SEARCH_PATH := ['analytics'])",
            "SELECT * FROM semantic_view('v', Search_Path := ['analytics'])",
        ] {
            assert!(
                matches!(inject_search_path(sql), Cow::Borrowed(_)),
                "an explicit path must not be overridden: {sql}"
            );
        }
    }

    #[test]
    fn a_view_name_that_merely_contains_the_parameter_text_is_still_injected() {
        // The argument-present check must look for the named parameter, not for
        // the text anywhere in the call: a view legitimately named
        // `my_search_path_view` would otherwise resolve with no path at all —
        // silently, and only for views whose names contain that substring.
        // (Raised by review on PR #187.)
        let got = inject_search_path("SELECT * FROM semantic_view('my_search_path_view')");
        assert!(
            got.contains("search_path :="),
            "a literal is not a named argument: {got}"
        );
    }

    #[test]
    fn a_where_clause_mentioning_the_parameter_is_not_an_argument() {
        // Same rule from the other side: `search_path` inside a predicate
        // string is literal content, not this call's named parameter.
        let got = inject_search_path(
            "SELECT * FROM semantic_view('v', where_clause := 'search_path = 1')",
        );
        assert!(
            got.contains("search_path := list_concat"),
            "a predicate string is not a named argument: {got}"
        );
    }

    #[test]
    fn the_function_name_matches_case_insensitively() {
        let got = inject_search_path("SELECT * FROM SEMANTIC_VIEW('v')");
        assert!(got.contains("search_path :="), "{got}");
    }

    #[test]
    fn an_unterminated_call_is_left_alone() {
        // Malformed SQL is DuckDB's to reject; splicing into it would only
        // produce a more confusing error.
        let sql = "SELECT * FROM semantic_view('v'";
        assert!(matches!(inject_search_path(sql), Cow::Borrowed(_)), "{sql}");
    }

    #[test]
    fn the_expression_uses_the_explicit_search_path_not_the_implicit_one() {
        // `current_schemas(true)` prepends the temp schema, itself named
        // `main`; with a real `main` later on the path the temp entry would
        // win. Pinned because the two spellings differ by one character and
        // the wrong one fails only in a configuration tests rarely set up.
        assert!(
            SEARCH_PATH_SQL.contains("current_schemas(false)"),
            "{SEARCH_PATH_SQL}"
        );
        assert!(
            !SEARCH_PATH_SQL.contains("current_schemas(true)"),
            "{SEARCH_PATH_SQL}"
        );
        assert!(
            SEARCH_PATH_SQL.contains("current_schema()"),
            "{SEARCH_PATH_SQL}"
        );
    }
}
