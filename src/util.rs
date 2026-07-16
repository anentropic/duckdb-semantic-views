//! Shared string utilities for fuzzy matching and identifier-boundary
//! classification.
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

/// Is `b` an identifier-continuation byte?
///
/// **This is the single source of truth for "what byte continues a SQL
/// identifier"** across the whole crate — the DDL keyword scanners
/// (`body_parser::scan::is_ident_continuation` delegates here), the
/// prefix matcher (`parse::match_keyword_prefix`), and the reference
/// tokenizer that drives fact/derived-metric inlining (`expr_tokens`) all
/// resolve through it.
/// Keeping one definition is what prevents the recurring boundary-drift
/// bug class (PR #50 review): a keyword must not match immediately before
/// an identifier byte, or `AS`/`BY`/`id` matches inside `ASx`/`BYé`/`idΩ`.
///
/// Continuation = ASCII alphanumerics, `_`, AND every non-ASCII byte
/// (>= 0x80): `DuckDB` identifiers may contain any non-ASCII character, so
/// UTF-8 lead/continuation bytes are identifier bytes, never boundaries.
#[must_use]
pub fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Byte offset of the subslice `inner` within `outer`.
///
/// `inner` MUST be a subslice of `outer` (borrowed from the same allocation,
/// as produced by `&outer[a..b]` / `.trim()` / `.split` etc.). Used to recover
/// an absolute error position from a re-sliced token without threading manual
/// byte counters through every clause scanner (R-2): the parser slices its way
/// down to the offending token, and this maps that token back to an offset in
/// the original query for the caret. `debug_assert`s the subslice relationship;
/// a non-subslice argument is a caller bug, not a runtime condition.
#[must_use]
pub fn byte_offset_within(outer: &str, inner: &str) -> usize {
    let outer_start = outer.as_ptr() as usize;
    let inner_start = inner.as_ptr() as usize;
    debug_assert!(
        inner_start >= outer_start && inner_start + inner.len() <= outer_start + outer.len(),
        "byte_offset_within: `inner` is not a subslice of `outer`"
    );
    inner_start - outer_start
}

/// Is `b` a word-boundary byte — i.e. NOT an [`is_ident_byte`]? The
/// primitive used by the `expand::facts` COUNT / name matchers so they share
/// the parser's notion of an identifier boundary.
#[must_use]
pub fn is_word_boundary_char(b: u8) -> bool {
    !is_ident_byte(b)
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

/// Length in bytes of the dollar-quote opener `$tag$` at `bytes[start]`, or
/// `None` if there is no valid opener there.
///
/// Returns `None` when `start` is out of bounds or `bytes[start]` is not `$`,
/// so callers may probe any offset without a prior bounds/`$` check. When it
/// returns `Some(len)`, `bytes[start]` was `$` and `&bytes[start..start + len]`
/// is the opener (e.g. `$$`, `$yaml$`). The tag body is ASCII alphanumerics and
/// `_` and may **not** start with a digit — `$1` is a positional parameter, not
/// a dollar-quote tag — matching `PostgreSQL`/`DuckDB`. The empty tag `$$` is
/// valid; the returned length includes both `$` delimiters.
///
/// **Single source of truth for dollar-quote tags** (P-6, code-review
/// 2026-07-11), shared by [`blank_sql_comments`] and the CREATE-body
/// `extract_dollar_quoted` extractor so the two can never disagree about what
/// a valid tag is. Previously the extractor accepted any run between two `$`
/// (including `$1$` and `$ta g$`) while comment-blanking recognized only the
/// stricter form; a body opened with a tag the blanker rejected had its `--`
/// runs blanked as SQL before the extractor stored the (now corrupted) text.
#[must_use]
pub fn read_dollar_tag_len(bytes: &[u8], start: usize) -> Option<usize> {
    if start >= bytes.len() || bytes[start] != b'$' {
        return None;
    }
    let mut j = start + 1;
    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        // A tag may not START with a digit ($1 is a parameter, not a tag).
        if j == start + 1 && bytes[j].is_ascii_digit() {
            return None;
        }
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b'$' {
        Some(j - start + 1)
    } else {
        None
    }
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
///
/// This pre-pass is the settled design, not a stopgap: §6.1 phase 8
/// (2026-07-15) evaluated folding it into the body-parser lexer and declined
/// — see the decision record in `crate::body_parser::lexer`'s module docs.
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
                    if let Some(len) = read_dollar_tag_len(bytes, i) {
                        dollar_tag = Some(&bytes[i..i + len]);
                        i += len;
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // -------------------------------------------------------------------
    // byte_offset_within tests
    // -------------------------------------------------------------------

    #[test]
    fn byte_offset_within_returns_slice_offset() {
        let outer = "DROP SEMANTIC VIEW foo bar";
        let inner = &outer[19..22]; // "foo"
        assert_eq!(byte_offset_within(outer, inner), 19);
        assert_eq!(byte_offset_within(outer, outer), 0);
        // A trailing-trimmed token still maps back to its original offset.
        let tail = outer[18..].trim_start(); // "foo bar" at offset 19
        assert_eq!(byte_offset_within(outer, tail), 19);
    }

    #[test]
    fn byte_offset_within_handles_multibyte_token() {
        // 'Ω' is 2 bytes; the trailing token starts at byte 20.
        let outer = "SHOW SEMANTIC VIEWS Ωx";
        let inner = &outer[20..]; // "Ωx"
        assert_eq!(byte_offset_within(outer, inner), 20);
    }

    // -------------------------------------------------------------------
    // starts_with_keyword_ci tests
    // -------------------------------------------------------------------

    #[test]
    fn ident_byte_is_the_single_boundary_definition() {
        // is_word_boundary_char is exactly the inverse of is_ident_byte, and
        // the classification is: ASCII alnum / `_` / all non-ASCII bytes are
        // identifier bytes; ASCII punctuation and whitespace are boundaries.
        for b in 0u8..=255 {
            assert_eq!(is_word_boundary_char(b), !is_ident_byte(b), "byte {b}");
        }
        for &b in b"aZ0_" {
            assert!(is_ident_byte(b));
        }
        assert!(is_ident_byte(0xC3)); // UTF-8 lead byte (é etc.)
        assert!(is_ident_byte(0xA9)); // UTF-8 continuation byte
        for &b in b" \t.,()\";'" {
            assert!(!is_ident_byte(b), "byte {b} must be a boundary");
        }
    }

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
    // read_dollar_tag_len tests (P-6)
    // -------------------------------------------------------------------

    #[test]
    fn dollar_tag_valid_forms() {
        // Empty tag `$$` (len 2) and a named tag `$yaml$` (len 6).
        assert_eq!(read_dollar_tag_len(b"$$rest", 0), Some(2));
        assert_eq!(read_dollar_tag_len(b"$yaml$rest", 0), Some(6));
        assert_eq!(read_dollar_tag_len(b"$_t9$x", 0), Some(5));
    }

    #[test]
    fn dollar_tag_rejects_invalid_openers() {
        // P-6: these are the forms the extractor used to accept but the
        // comment-blanker rejected. Both must now agree they are NOT openers.
        assert_eq!(read_dollar_tag_len(b"$1$body$1$", 0), None); // digit-started tag
        assert_eq!(read_dollar_tag_len(b"$ta g$", 0), None); // interior whitespace
        assert_eq!(read_dollar_tag_len(b"$no_close", 0), None); // unterminated opener
        assert_eq!(read_dollar_tag_len(b"nope", 0), None); // no leading `$`
        assert_eq!(read_dollar_tag_len(b"", 0), None); // empty input
    }

    #[test]
    fn blank_comments_and_dollar_tag_agree_on_validity() {
        // A VALID tag makes the payload inert: `--` inside survives.
        let valid = "FROM YAML $y$a: 1 -- keep$y$";
        assert_eq!(blank_sql_comments(valid), valid);
        // An INVALID tag (`$1$`) is not a dollar-quote, so the payload is
        // scanned as SQL and its line comment IS blanked — matching the fact
        // that `extract_dollar_quoted` now rejects `$1$` outright rather than
        // storing this blanked text (P-6).
        let out = blank_sql_comments("$1$a -- x$1$");
        assert!(
            !out.contains("-- x"),
            "invalid tag payload must be treated as SQL: {out}"
        );
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
