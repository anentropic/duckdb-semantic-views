//! Shared string utilities for fuzzy matching and word-boundary replacement.
//!
//! Extracted from `expand.rs` to break the expand <-> graph circular dependency.
//! Both `expand` and `graph` modules import from here.

/// Suggest the closest matching name from `available` using Levenshtein distance.
///
/// Returns `Some(name)` (with original casing) if the best match has an edit
/// distance of 3 or fewer characters. Returns `None` if no candidate is close
/// enough. Both the query and candidates are lowercased for comparison.
#[must_use]
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let query = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let dist = strsim::levenshtein(&query, &candidate.to_ascii_lowercase());
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, candidate));
                }
            } else {
                best = Some((dist, candidate));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// Replace all word-boundary occurrences of `needle` in `haystack` with `replacement`.
///
/// A word boundary is defined as: the character before the match (if any) is NOT
/// alphanumeric or underscore, AND the character after the match (if any) is NOT
/// alphanumeric or underscore. This prevents `net_price` from matching inside
/// `net_price_total` or `my_net_price`.
///
/// The matching is case-sensitive (fact names are identifiers).
#[must_use]
pub fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || needle.len() > haystack.len() {
        return haystack.to_string();
    }

    let h_bytes = haystack.as_bytes();
    let n_bytes = needle.as_bytes();
    let n_len = n_bytes.len();

    let mut result = String::with_capacity(haystack.len());
    let mut i = 0;

    while i + n_len <= h_bytes.len() {
        if &h_bytes[i..i + n_len] == n_bytes {
            let before_ok = i == 0 || is_word_boundary_char(h_bytes[i - 1]);
            let after_ok = i + n_len == h_bytes.len() || is_word_boundary_char(h_bytes[i + n_len]);
            if before_ok && after_ok {
                result.push_str(replacement);
                i += n_len;
                continue;
            }
        }
        // Advance by a full UTF-8 char so `i` stays on a char boundary
        // (byte-wise advance both duplicated multi-byte chars in the output
        // and panicked slicing `haystack[i..]` mid-codepoint — MS-3,
        // code-review 2026-07-02; mirrors `replace_word_boundary_any`).
        let ch = haystack[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    // Append remaining bytes that are shorter than needle
    if i < haystack.len() {
        result.push_str(&haystack[i..]);
    }
    result
}

/// Replace word-boundary occurrences of *any* needle in `needles` with `replacement`,
/// in a single left-to-right pass.
///
/// At each position the needles are tried in the given order — put the more specific
/// (e.g. qualified `alias.name`) needle first so it wins over a shorter one (`name`).
/// On a match the replacement is emitted and scanning resumes *after* the matched
/// needle: the inserted replacement text is never re-scanned.
///
/// This matters when the replacement itself contains one of the needles. For an
/// identity fact (`name` whose expression is the qualified column `alias.name`), two
/// sequential [`replace_word_boundary`] calls — qualified then unqualified — would
/// double-substitute (`alias.name` -> `(alias.name)` -> `(alias.(alias.name))`).
/// A single combined pass avoids that.
#[must_use]
pub fn replace_word_boundary_any(haystack: &str, needles: &[&str], replacement: &str) -> String {
    let pairs: Vec<(&str, &str)> = needles.iter().map(|&n| (n, replacement)).collect();
    replace_word_boundary_pairs(haystack, &pairs)
}

/// Replace word-boundary occurrences of each needle with its *own* replacement,
/// in a single left-to-right pass.
///
/// Same scanning semantics as [`replace_word_boundary_any`] — at each position
/// the pairs are tried in the given order, and on a match scanning resumes
/// *after* the matched needle so inserted replacement text is never re-scanned.
/// Callers that need deterministic output for overlapping needles should order
/// the pairs deterministically (e.g. longest needle first, then lexicographic).
///
/// This is the safe substitution primitive for derived-metric inlining (SG-3,
/// code-review 2026-07-02): sequential per-name [`replace_word_boundary`]
/// calls re-scan earlier substitutions, so a metric name that also appears as
/// a column reference inside another metric's resolved expression (`revenue`
/// inside `SUM(o.revenue)` — `.` is a word boundary) got double-substituted
/// into invalid nested-aggregate SQL, dependent on hash-map iteration order.
#[must_use]
pub fn replace_word_boundary_pairs(haystack: &str, pairs: &[(&str, &str)]) -> String {
    let h_bytes = haystack.as_bytes();
    let mut result = String::with_capacity(haystack.len());
    let mut i = 0;

    while i < h_bytes.len() {
        let mut matched = false;
        for &(needle, replacement) in pairs {
            let n_bytes = needle.as_bytes();
            let n_len = n_bytes.len();
            if n_len == 0 || i + n_len > h_bytes.len() {
                continue;
            }
            if &h_bytes[i..i + n_len] == n_bytes {
                let before_ok = i == 0 || is_word_boundary_char(h_bytes[i - 1]);
                let after_ok =
                    i + n_len == h_bytes.len() || is_word_boundary_char(h_bytes[i + n_len]);
                if before_ok && after_ok {
                    result.push_str(replacement);
                    i += n_len;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            // Advance by a full UTF-8 char so `i` stays on a char boundary.
            let ch = haystack[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
    }

    result
}

/// Check if a byte is a word-boundary character (NOT alphanumeric or underscore).
#[must_use]
pub fn is_word_boundary_char(b: u8) -> bool {
    !b.is_ascii_alphanumeric() && b != b'_'
}

/// Does `s` start with the ASCII keyword `kw`, case-insensitively?
///
/// Compares raw *bytes*, so it is safe on any UTF-8 input: the old
/// `s[..kw.len()].eq_ignore_ascii_case(kw)` pattern panicked ("byte index N
/// is not a char boundary") whenever a multi-byte character straddled the
/// keyword length (PA-1, code-review 2026-07-02 — e.g. `SHOW SEMANTIC VIEWS
/// aΩΩ`). A multi-byte character can never byte-match an ASCII keyword, so
/// the comparison is also *correct* on non-ASCII input: it simply fails.
///
/// After a `true` return, slicing `s` at `kw.len()` is guaranteed safe —
/// the matched prefix is pure ASCII, so `kw.len()` lands on a char boundary.
#[must_use]
pub fn starts_with_keyword_ci(s: &str, kw: &str) -> bool {
    let n = kw.len();
    s.len() >= n && s.as_bytes()[..n].eq_ignore_ascii_case(kw.as_bytes())
}

/// Blank SQL comments out of `input`, byte-for-byte length-preserving.
///
/// Every byte of a comment — `-- ...` to end of line (the newline itself is
/// kept), and `/* ... */` including the delimiters — is replaced with a
/// space. Block comments NEST, matching `PostgreSQL`/`DuckDB` semantics (the SQL
/// standard): `/* a /* b */ c */` is one comment. An unterminated block
/// comment blanks to end of input.
///
/// Comment markers inside `'...'` string literals (with `''` escape),
/// `"..."` quoted identifiers (with `""` escape), and `$tag$ ... $tag$`
/// dollar-quoted strings are inert, and quote characters inside comments are
/// inert.
///
/// Because the output length equals the input length and all replaced
/// regions are bounded by ASCII delimiters, every byte offset into the
/// output is valid for the input — error-caret positions computed on the
/// blanked text reference the original query correctly.
///
/// This is the single comment-handling pass for the DDL surface (PA-7,
/// code-review 2026-07-02): applied once at the parse entry points it makes
/// every downstream scanner comment-immune, stops trailing comments being
/// absorbed into stored expressions (`ALTER ... RENAME TO x -- oops` renamed
/// to `x -- oops`), and fixes non-nesting block-comment handling (PA-10).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn blank_sql_comments(input: &str) -> std::borrow::Cow<'_, str> {
    #[derive(PartialEq)]
    enum St {
        Code,
        InString,
        InIdent,
    }

    let bytes = input.as_bytes();
    let mut out: Option<Vec<u8>> = None; // allocated lazily on first comment
    let mut st = St::Code;
    let mut dollar_tag: Option<&[u8]> = None;
    let mut i = 0;

    // Try to read a dollar-quote opener `$tag$` at `i`; returns the tag
    // including both `$` delimiters.
    let read_dollar_tag = |i: usize| -> Option<&[u8]> {
        if bytes[i] != b'$' {
            return None;
        }
        let mut j = i + 1;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
            // A tag may not START with a digit ($1 is a parameter, not a tag).
            if j == i + 1 && bytes[j].is_ascii_digit() {
                return None;
            }
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'$' {
            Some(&bytes[i..=j])
        } else {
            None
        }
    };

    while i < bytes.len() {
        if let Some(tag) = dollar_tag {
            // Inside $tag$ ... $tag$ — scan for the closing tag.
            if bytes[i] == b'$' && bytes[i..].starts_with(tag) {
                i += tag.len();
                dollar_tag = None;
            } else {
                i += 1;
            }
            continue;
        }
        match st {
            St::InString => {
                if bytes[i] == b'\'' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                        i += 2;
                        continue;
                    }
                    st = St::Code;
                }
                i += 1;
            }
            St::InIdent => {
                if bytes[i] == b'"' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    st = St::Code;
                }
                i += 1;
            }
            St::Code => match bytes[i] {
                b'\'' => {
                    st = St::InString;
                    i += 1;
                }
                b'"' => {
                    st = St::InIdent;
                    i += 1;
                }
                b'$' => {
                    if let Some(tag) = read_dollar_tag(i) {
                        i += tag.len();
                        dollar_tag = Some(tag);
                    } else {
                        i += 1;
                    }
                }
                b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                    // Line comment: blank to (not including) the newline.
                    let buf = out.get_or_insert_with(|| bytes.to_vec());
                    while i < bytes.len() && bytes[i] != b'\n' {
                        buf[i] = b' ';
                        i += 1;
                    }
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    // Block comment — nesting; unterminated blanks to end.
                    let buf = out.get_or_insert_with(|| bytes.to_vec());
                    let mut depth = 0usize;
                    while i < bytes.len() {
                        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                            depth += 1;
                            buf[i] = b' ';
                            buf[i + 1] = b' ';
                            i += 2;
                        } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                            depth -= 1;
                            buf[i] = b' ';
                            buf[i + 1] = b' ';
                            i += 2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            buf[i] = b' ';
                            i += 1;
                        }
                    }
                }
                _ => i += 1,
            },
        }
    }

    match out {
        // Only whole comment regions (bounded by ASCII delimiters) were
        // overwritten with ASCII spaces, so the buffer remains valid UTF-8.
        Some(buf) => std::borrow::Cow::Owned(
            String::from_utf8(buf).expect("blanking comment bytes preserves UTF-8 validity"),
        ),
        None => std::borrow::Cow::Borrowed(input),
    }
}

/// Failure modes of [`extract_single_quoted_prefix`]. Callers map these onto
/// their local error types/messages (`ParseError` in the body parser, plain
/// `String` in the SHOW-clause parser).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleQuoteError {
    /// The input does not begin with `'`.
    NotQuoted,
    /// No unescaped closing `'` before end of input.
    Unterminated,
}

/// Extract a single-quoted SQL string literal from the start of `input`.
///
/// Returns `(unescaped_content, bytes_consumed)` where `bytes_consumed`
/// includes both the opening and closing quotes. SQL-standard escaping is
/// honoured: `''` inside the literal is a single literal `'`. Content after
/// the closing quote is not inspected — callers decide what trailing text
/// means.
///
/// Walks the input as a `char` stream (UTF-8 scalar values), never as raw
/// bytes: this is the single shared implementation mandated by ST-4
/// (code-review 2026-07-02). Two earlier per-site copies cast
/// `bytes[i] as char`, silently Latin-1-izing every non-ASCII codepoint
/// (`'café'` → `cafÃ©` — WR-04/PA-2); do not re-inline this logic.
pub fn extract_single_quoted_prefix(input: &str) -> Result<(String, usize), SingleQuoteError> {
    let mut chars = input.char_indices();
    match chars.next() {
        Some((_, '\'')) => {}
        _ => return Err(SingleQuoteError::NotQuoted),
    }
    let mut result = String::new();
    while let Some((i, ch)) = chars.next() {
        if ch == '\'' {
            // Peek without consuming; only advance the real iterator on a hit.
            let mut peek = chars.clone();
            if matches!(peek.next(), Some((_, '\''))) {
                result.push('\'');
                chars = peek;
            } else {
                // `i` is the byte offset of the closing quote; the quote is
                // one ASCII byte, so total consumed = i + 1.
                return Ok((result, i + 1));
            }
        } else {
            result.push(ch);
        }
    }
    Err(SingleQuoteError::Unterminated)
}

/// Wrap a closure in `catch_unwind`, converting panics to `Box<dyn Error>`.
///
/// Used at FFI boundaries to prevent Rust panics from unwinding through C++ frames
/// (which is undefined behavior). The closure must return `Result<T, Box<dyn Error>>`.
///
/// On panic, the payload is inspected for `&str` or `String` messages to produce
/// a descriptive error. Unknown payloads produce a generic "unknown cause" message.
pub fn catch_unwind_to_result<F, T>(f: F) -> Result<T, Box<dyn std::error::Error>>
where
    F: FnOnce() -> Result<T, Box<dyn std::error::Error>> + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                format!("internal error (panic): {s}")
            } else if let Some(s) = payload.downcast_ref::<String>() {
                format!("internal error (panic): {s}")
            } else {
                "internal error (panic): unknown cause".to_string()
            };
            Err(msg.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // -------------------------------------------------------------------
    // replace_word_boundary tests
    // -------------------------------------------------------------------

    #[test]
    fn replace_word_boundary_no_match() {
        let result = replace_word_boundary("SUM(total)", "net_price", "(x)");
        assert_eq!(result, "SUM(total)");
    }

    #[test]
    fn replace_word_boundary_exact_match_in_function() {
        let result =
            replace_word_boundary("SUM(net_price)", "net_price", "(price * (1 - discount))");
        assert_eq!(result, "SUM((price * (1 - discount)))");
    }

    #[test]
    fn replace_word_boundary_no_substring_match_suffix() {
        // "net_price" should NOT match in "net_price_total"
        let result = replace_word_boundary("SUM(net_price_total)", "net_price", "(x)");
        assert_eq!(result, "SUM(net_price_total)");
    }

    #[test]
    fn replace_word_boundary_no_substring_match_prefix() {
        // "net_price" should NOT match in "total_net_price_x"
        let result = replace_word_boundary("total_net_price_x + 1", "net_price", "(x)");
        assert_eq!(result, "total_net_price_x + 1");
    }

    #[test]
    fn replace_word_boundary_match_with_addition() {
        let result = replace_word_boundary("net_price + tax", "net_price", "(a + b)");
        assert_eq!(result, "(a + b) + tax");
    }

    #[test]
    fn replace_word_boundary_non_ascii_haystack_no_panic_no_duplication() {
        // MS-3 regression: byte-wise advance through a multi-byte char
        // panicked on the next `haystack[i..]` slice ("byte index N is not
        // a char boundary") and duplicated the char where it didn't panic.
        // Reachable at query time via fact/derived-metric inlining over any
        // expression containing non-ASCII (string literals, identifiers).
        let result = replace_word_boundary("héllo + net_price", "net_price", "(x)");
        assert_eq!(result, "héllo + (x)");

        let result = replace_word_boundary("concat(city, ' – ') || net_price", "net_price", "(x)");
        assert_eq!(result, "concat(city, ' – ') || (x)");

        // Non-ASCII with no match at all must round-trip unchanged.
        let result = replace_word_boundary("'São Paulo' || 'café'", "net_price", "(x)");
        assert_eq!(result, "'São Paulo' || 'café'");
    }

    #[test]
    fn replace_word_boundary_pairs_distinct_replacements_single_pass() {
        let pairs = [
            ("revenue", "(SUM(o.revenue))"),
            ("tax", "(SUM(o.revenue * 0.1))"),
        ];
        let result = replace_word_boundary_pairs("revenue - tax", &pairs);
        assert_eq!(result, "(SUM(o.revenue)) - (SUM(o.revenue * 0.1))");
    }

    #[test]
    fn replace_word_boundary_pairs_inserted_text_not_rescanned() {
        // "b"'s replacement contains needle "a" at a word boundary; a second
        // scan over inserted text would corrupt it.
        let pairs = [("a", "(X)"), ("b", "(a + 1)")];
        let result = replace_word_boundary_pairs("a + b", &pairs);
        assert_eq!(result, "(X) + (a + 1)");
    }

    #[test]
    fn replace_word_boundary_any_still_shares_semantics_with_pairs() {
        // _any delegates to _pairs; identical inputs must stay identical.
        let via_any = replace_word_boundary_any("x + y", &["x", "y"], "(z)");
        let pairs = [("x", "(z)"), ("y", "(z)")];
        let via_pairs = replace_word_boundary_pairs("x + y", &pairs);
        assert_eq!(via_any, via_pairs);
        assert_eq!(via_any, "(z) + (z)");
    }

    proptest! {
        // Any (haystack, needle, replacement) triple must not panic, and a
        // haystack containing no ASCII needle occurrence must round-trip
        // byte-identical (guards the char-boundary advance).
        #[test]
        fn replace_word_boundary_never_panics_on_unicode(
            haystack in "\\PC{0,40}",
            needle in "[a-z_]{1,8}",
            replacement in "\\PC{0,12}",
        ) {
            let out = replace_word_boundary(&haystack, &needle, &replacement);
            if !haystack.contains(&needle) {
                prop_assert_eq!(out, haystack);
            }
        }
    }

    #[test]
    fn replace_word_boundary_match_in_parens() {
        let result = replace_word_boundary("(net_price)", "net_price", "(a)");
        assert_eq!(result, "((a))");
    }

    #[test]
    fn replace_word_boundary_entire_string() {
        let result = replace_word_boundary("net_price", "net_price", "(a + b)");
        assert_eq!(result, "(a + b)");
    }

    #[test]
    fn replace_word_boundary_at_start() {
        let result = replace_word_boundary("net_price * 2", "net_price", "(x)");
        assert_eq!(result, "(x) * 2");
    }

    #[test]
    fn replace_word_boundary_at_end() {
        let result = replace_word_boundary("2 * net_price", "net_price", "(x)");
        assert_eq!(result, "2 * (x)");
    }

    #[test]
    fn replace_word_boundary_multiple_occurrences() {
        let result = replace_word_boundary("net_price + net_price", "net_price", "(x)");
        assert_eq!(result, "(x) + (x)");
    }

    #[test]
    fn replace_word_boundary_empty_needle() {
        let result = replace_word_boundary("abc", "", "x");
        assert_eq!(result, "abc");
    }

    // -------------------------------------------------------------------
    // replace_word_boundary_any tests
    // -------------------------------------------------------------------

    #[test]
    fn replace_any_qualified_wins_over_unqualified() {
        // Qualified needle is tried first; the unqualified `unit_price` inside the
        // emitted replacement must NOT be re-scanned (no double substitution).
        let result = replace_word_boundary_any(
            "s.unit_price",
            &["s.unit_price", "unit_price"],
            "(s.unit_price)",
        );
        assert_eq!(result, "(s.unit_price)");
    }

    #[test]
    fn replace_any_unqualified_fallback() {
        // When only the unqualified form appears, it still matches.
        let result =
            replace_word_boundary_any("SUM(unit_price)", &["s.unit_price", "unit_price"], "(x)");
        assert_eq!(result, "SUM((x))");
    }

    #[test]
    fn replace_any_inside_qualified_metric() {
        // Metric referencing an identity fact by its qualified column.
        let result = replace_word_boundary_any(
            "SUM(s.unit_price)",
            &["s.unit_price", "unit_price"],
            "(s.unit_price)",
        );
        assert_eq!(result, "SUM((s.unit_price))");
    }

    #[test]
    fn replace_any_no_substring_match() {
        let result =
            replace_word_boundary_any("unit_price_total", &["s.unit_price", "unit_price"], "(x)");
        assert_eq!(result, "unit_price_total");
    }

    #[test]
    fn replace_any_utf8_passthrough() {
        // Non-ASCII, non-matching content must not panic and must round-trip.
        let result = replace_word_boundary_any("héllo + unit_price", &["unit_price"], "(x)");
        assert_eq!(result, "héllo + (x)");
    }

    // -------------------------------------------------------------------
    // starts_with_keyword_ci tests
    // -------------------------------------------------------------------

    #[test]
    fn keyword_ci_matches_case_insensitively() {
        assert!(starts_with_keyword_ci("LIKE 'x'", "LIKE"));
        assert!(starts_with_keyword_ci("like 'x'", "LIKE"));
        assert!(starts_with_keyword_ci("LiKe", "LIKE"));
    }

    #[test]
    fn keyword_ci_rejects_shorter_input() {
        assert!(!starts_with_keyword_ci("LIK", "LIKE"));
        assert!(!starts_with_keyword_ci("", "LIKE"));
    }

    #[test]
    fn keyword_ci_no_panic_on_multibyte_straddle() {
        // "aΩΩ" is 5 bytes; byte 4 is mid-Ω. The old slice pattern panicked
        // here (PA-1); byte comparison just fails.
        assert!(!starts_with_keyword_ci("aΩΩ", "LIKE"));
        assert!(!starts_with_keyword_ci("Ωx", "IN"));
    }

    // -------------------------------------------------------------------
    // blank_sql_comments tests
    // -------------------------------------------------------------------

    #[test]
    fn blank_comments_no_comments_borrows() {
        let s = "SELECT 1";
        assert!(matches!(
            blank_sql_comments(s),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn blank_comments_line_comment() {
        let out = blank_sql_comments("DROP SEMANTIC VIEW a -- oops\n;");
        assert_eq!(out, "DROP SEMANTIC VIEW a        \n;");
        assert_eq!(out.len(), "DROP SEMANTIC VIEW a -- oops\n;".len());
    }

    #[test]
    fn blank_comments_block_comment_nested() {
        // Nested per SQL standard (PostgreSQL/DuckDB behaviour).
        let out = blank_sql_comments("a /* x /* y */ z */ b");
        assert_eq!(out, "a                   b");
    }

    #[test]
    fn blank_comments_unterminated_block_blanks_to_end() {
        let out = blank_sql_comments("a /* never closed");
        assert_eq!(out, "a                ");
    }

    #[test]
    fn blank_comments_markers_inside_string_inert() {
        let s = "COMMENT = 'a -- not a comment /* neither */'";
        assert_eq!(blank_sql_comments(s), s);
    }

    #[test]
    fn blank_comments_markers_inside_quoted_ident_inert() {
        let s = "\"weird--name\" AS x";
        assert_eq!(blank_sql_comments(s), s);
    }

    #[test]
    fn blank_comments_markers_inside_dollar_quotes_inert() {
        // YAML bodies ride in $$...$$ — '--' sequences inside must survive.
        let s = "FROM YAML $$name: v\n# yaml comment\nvalue: a--b$$";
        assert_eq!(blank_sql_comments(s), s);
        let s = "FROM YAML $tag$ -- inert $tag$";
        assert_eq!(blank_sql_comments(s), s);
    }

    #[test]
    fn blank_comments_dollar_parameter_not_a_tag() {
        // $1 is a parameter, not a dollar-quote opener; the comment after it
        // must still be blanked.
        let out = blank_sql_comments("WHERE x = $1 -- c");
        assert_eq!(out, "WHERE x = $1     ");
    }

    #[test]
    fn blank_comments_quote_inside_comment_inert() {
        // An apostrophe inside a comment must not open a string region.
        let out = blank_sql_comments("a -- don't\nb 'lit'");
        assert_eq!(out, "a         \nb 'lit'");
    }

    #[test]
    fn blank_comments_multibyte_inside_comment() {
        let input = "x -- café ☕\ny";
        let out = blank_sql_comments(input);
        assert_eq!(out.len(), input.len());
        assert!(out.starts_with("x  "));
        assert!(out.ends_with("\ny"));
        // Result must be valid UTF-8 by construction (checked by the type),
        // and the non-comment text intact.
        assert_eq!(&out[input.len() - 1..], "y");
    }

    #[test]
    fn blank_comments_escaped_quote_in_string() {
        let s = "'it''s -- fine' -- real";
        let out = blank_sql_comments(s);
        assert_eq!(out, "'it''s -- fine'        ");
    }

    // -------------------------------------------------------------------
    // extract_single_quoted_prefix tests
    // -------------------------------------------------------------------

    #[test]
    fn quoted_prefix_basic() {
        let (s, n) = extract_single_quoted_prefix("'abc' rest").unwrap();
        assert_eq!(s, "abc");
        assert_eq!(n, 5);
    }

    #[test]
    fn quoted_prefix_escaped_quote() {
        let (s, n) = extract_single_quoted_prefix("'a''b'").unwrap();
        assert_eq!(s, "a'b");
        assert_eq!(n, 6);
    }

    #[test]
    fn quoted_prefix_empty_literal() {
        let (s, n) = extract_single_quoted_prefix("''").unwrap();
        assert_eq!(s, "");
        assert_eq!(n, 2);
    }

    #[test]
    fn quoted_prefix_non_ascii_content_survives() {
        // PA-2 regression: the per-site copies Latin-1-ized this to "cafÃ©".
        let (s, n) = extract_single_quoted_prefix("'café et plus'").unwrap();
        assert_eq!(s, "café et plus");
        assert_eq!(n, "'café et plus'".len());

        let (s, _) = extract_single_quoted_prefix("'東京 ☕'").unwrap();
        assert_eq!(s, "東京 ☕");
    }

    #[test]
    fn quoted_prefix_errors() {
        assert_eq!(
            extract_single_quoted_prefix("abc"),
            Err(SingleQuoteError::NotQuoted)
        );
        assert_eq!(
            extract_single_quoted_prefix("'abc"),
            Err(SingleQuoteError::Unterminated)
        );
        assert_eq!(
            extract_single_quoted_prefix(""),
            Err(SingleQuoteError::NotQuoted)
        );
    }

    proptest! {
        // Round-trip: escaping then extracting returns the original content
        // and consumes exactly the literal, for arbitrary unicode content.
        #[test]
        fn quoted_prefix_roundtrips_arbitrary_content(
            content in "\\PC{0,40}",
            tail in "[ a-zA-Z]{0,10}",
        ) {
            let literal = format!("'{}'{}", content.replace('\'', "''"), tail);
            let (extracted, consumed) = extract_single_quoted_prefix(&literal).unwrap();
            prop_assert_eq!(&extracted, &content);
            prop_assert_eq!(&literal[consumed..], &tail);
        }
    }

    // -------------------------------------------------------------------
    // suggest_closest property tests
    // -------------------------------------------------------------------

    proptest! {
        /// Any suggestion returned by suggest_closest must be a member of the
        /// input `available` list. This prevents the function from inventing
        /// names that don't exist in the model.
        #[test]
        fn suggestion_is_always_valid_name(
            query in "[a-z_]{1,20}",
            names in prop::collection::vec("[a-z_]{1,20}", 1..20)
        ) {
            if let Some(suggestion) = suggest_closest(&query, &names) {
                prop_assert!(
                    names.contains(&suggestion),
                    "suggest_closest returned '{}' which is not in available names: {:?}",
                    suggestion,
                    names
                );
            }
        }

        /// An exact match (query == one of the available names) should always
        /// produce a suggestion, since edit distance is 0 which is within the
        /// threshold of 3.
        #[test]
        fn exact_match_always_suggests(
            name in "[a-z_]{1,20}",
            others in prop::collection::vec("[a-z_]{1,20}", 0..10)
        ) {
            let mut names = others;
            names.push(name.clone());
            let suggestion = suggest_closest(&name, &names);
            prop_assert!(
                suggestion.is_some(),
                "exact match '{}' should always produce a suggestion",
                name
            );
            prop_assert_eq!(
                suggestion.unwrap(),
                name,
                "exact match should suggest itself"
            );
        }

        /// When the available list is empty, suggest_closest must return None.
        #[test]
        fn empty_names_returns_none(
            query in "[a-z_]{1,20}"
        ) {
            let names: Vec<String> = vec![];
            prop_assert!(suggest_closest(&query, &names).is_none());
        }
    }
}
