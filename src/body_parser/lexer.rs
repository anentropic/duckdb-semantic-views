//! Token stream over SQL clause text (§6.1 incremental lexer + cursor
//! migration, code-review 2026-07-11).
//!
//! This is the shared tokenizer the clause parsers are being migrated onto,
//! one clause per phase (TABLES first). It replaces the family of ad-hoc
//! quote-state loops and "find keyword anywhere and slice" scanners that let
//! the P-1/P-2/P-3 silent-discard bug class re-emerge in every new grammar
//! slot: once identifiers, strings, and punctuation are *tokens*, a keyword
//! search can only ever match a real keyword token — never a substring inside
//! a quoted identifier or a string literal — and a clause parser consumes its
//! tokens in order, so "text between the name and the constraint" becomes a
//! visible unexpected token rather than a region silently skipped over.
//!
//! ## Scope
//!
//! The token kinds are exactly what the migrated clauses need today:
//! double-quoted / bare identifiers, single-quoted string literals, and
//! single-byte symbols. Comment handling deliberately stays upstream in
//! [`crate::util::blank_sql_comments`] — §6.1 phase 8 (2026-07-15) evaluated
//! folding it into this lexer and declined. Blanking is a whole-query,
//! length-preserving pre-pass shared by statement detection
//! (`parse::detect`) and the CREATE front door (`parse::rewrite`), neither of
//! which tokenizes through this lexer, so a fold would not remove the
//! pre-pass; and this lexer only ever receives already-blanked text, so
//! comment handling here would be dead code on its own path. Length
//! preservation is also load-bearing: stored expressions are re-sliced from
//! raw source and error carets are computed on the blanked text, so offsets
//! must map 1:1 onto the original bytes (pinned by
//! `caret_after_in_body_comment_is_honest`) — a lexer that merely skipped
//! comment tokens would let raw slices re-absorb comment bytes. Revisit only
//! as part of a universal-front-door refactor in which detect/rewrite also
//! tokenize through this lexer and every raw-slice consumer reads via a
//! comment-aware token layer. Numeric and dollar-quoted literals get their
//! own kinds when a consumer first needs them distinguished (numbers
//! currently tokenize as bare identifiers, which is harmless for the
//! identifier-only clauses migrated so far).
//!
//! ## UTF-8 safety
//!
//! Only ASCII bytes (`"`, `'`, whitespace, punctuation) are ever compared, and
//! [`crate::util::is_ident_byte`] classifies every byte `>= 0x80` as an
//! identifier byte, so a multi-byte UTF-8 codepoint is consumed whole into a
//! bare-identifier token. Token spans therefore always land on char
//! boundaries — no byte-slice ever cuts a codepoint (the PA-1/PA-2 class).

use crate::util::is_ident_byte;

/// What a [`Token`] is. `Symbol` carries the raw punctuation byte so a cursor
/// can match `(`, `)`, `,`, `.`, `=` etc. directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenKind {
    /// A bare (`orders`) or double-quoted (`"my table"`) identifier. `quoted`
    /// distinguishes the two so keyword matching only ever fires on bare
    /// idents — a quoted `"PRIMARY"` is data, never the keyword.
    Ident { quoted: bool },
    /// A single-quoted string literal `'...'`, `''` treated as an escape.
    String,
    /// A single punctuation byte outside any quoted region.
    Symbol(u8),
    /// An unterminated `"..."` (`ident = true`) or `'...'` (`ident = false`)
    /// region. Spans from the opening quote to end-of-input; the clause parser
    /// turns this into a context-specific "Unterminated quoted identifier" /
    /// "Unterminated string literal" error rather than the lexer guessing the
    /// message.
    Unterminated { ident: bool },
}

/// A lexed token: its [`TokenKind`] and its half-open byte span `[start, end)`
/// in the lexed source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// Tokenize `src`. Infallible: unterminated quotes/strings become
/// [`TokenKind::Unterminated`] tokens (so the parser owns the error message)
/// and whitespace is skipped. Spans are byte offsets into `src`.
pub(super) fn lex(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                let start = i;
                i += 1;
                let mut terminated = false;
                while i < bytes.len() {
                    if bytes[i] == b'"' {
                        // `""` is an escape — stay inside the quoted region.
                        if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        terminated = true;
                        break;
                    }
                    i += 1;
                }
                toks.push(Token {
                    kind: if terminated {
                        TokenKind::Ident { quoted: true }
                    } else {
                        TokenKind::Unterminated { ident: true }
                    },
                    start,
                    end: i,
                });
            }
            b'\'' => {
                let start = i;
                i += 1;
                let mut terminated = false;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        // `''` is an escape — stay inside the string.
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        terminated = true;
                        break;
                    }
                    i += 1;
                }
                toks.push(Token {
                    kind: if terminated {
                        TokenKind::String
                    } else {
                        TokenKind::Unterminated { ident: false }
                    },
                    start,
                    end: i,
                });
            }
            _ if is_ident_byte(b) => {
                let start = i;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
                toks.push(Token {
                    kind: TokenKind::Ident { quoted: false },
                    start,
                    end: i,
                });
            }
            _ => {
                toks.push(Token {
                    kind: TokenKind::Symbol(b),
                    start: i,
                    end: i + 1,
                });
                i += 1;
            }
        }
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).into_iter().map(|t| t.kind).collect()
    }

    /// Every token span lands on a char boundary and the spans tile the
    /// non-whitespace bytes left to right without overlap — i.e. the only bytes
    /// NOT covered by a token are whitespace. Asserting the gaps are whitespace
    /// (not just that tokens don't overlap) is what makes this catch a `lex()`
    /// regression that silently drops a non-whitespace byte.
    fn assert_well_formed(src: &str) {
        let toks = lex(src);
        let mut prev_end = 0;
        for t in &toks {
            assert!(
                src.is_char_boundary(t.start),
                "start not on boundary: {src:?}"
            );
            assert!(src.is_char_boundary(t.end), "end not on boundary: {src:?}");
            assert!(t.start >= prev_end, "overlap in {src:?}");
            assert!(t.end > t.start, "empty token in {src:?}");
            assert!(
                src[prev_end..t.start]
                    .bytes()
                    .all(|b| b.is_ascii_whitespace()),
                "non-whitespace bytes dropped in gap {prev_end}..{} of {src:?}",
                t.start
            );
            prev_end = t.end;
        }
        assert!(
            src[prev_end..].bytes().all(|b| b.is_ascii_whitespace()),
            "non-whitespace trailing bytes dropped after {prev_end} in {src:?}"
        );
    }

    #[test]
    fn bare_idents_and_symbols() {
        assert_eq!(
            kinds("o AS orders"),
            vec![
                TokenKind::Ident { quoted: false },
                TokenKind::Ident { quoted: false },
                TokenKind::Ident { quoted: false },
            ]
        );
    }

    #[test]
    fn dotted_name_is_ident_dot_ident() {
        let toks = lex("schema.orders");
        assert_eq!(
            toks.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::Ident { quoted: false },
                TokenKind::Symbol(b'.'),
                TokenKind::Ident { quoted: false },
            ]
        );
        // Contiguity: no whitespace gaps between the three tokens.
        assert_eq!(toks[0].end, toks[1].start);
        assert_eq!(toks[1].end, toks[2].start);
    }

    #[test]
    fn quoted_ident_keeps_quotes_and_inner_bytes() {
        let toks = lex("\"my table\"");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Ident { quoted: true });
        assert_eq!(&"\"my table\""[toks[0].start..toks[0].end], "\"my table\"");
    }

    #[test]
    fn quoted_ident_doubled_quote_escape() {
        // `"a""b"` is ONE token — the `""` does not close it.
        let toks = lex("\"a\"\"b\"");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Ident { quoted: true });
        assert_eq!(toks[0].end, "\"a\"\"b\"".len());
    }

    #[test]
    fn keyword_inside_quoted_ident_is_one_token() {
        // The whole point: `"PRIMARY KEY (id)"` never surfaces PRIMARY/KEY as
        // matchable keyword tokens.
        let toks = lex("\"PRIMARY KEY (id)\"");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Ident { quoted: true });
    }

    #[test]
    fn string_literal_with_escape_and_keywords() {
        let toks = lex("'a UNIQUE ''x'' mention'");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::String);
        assert_eq!(toks[0].end, "'a UNIQUE ''x'' mention'".len());
    }

    #[test]
    fn unterminated_quote_and_string() {
        assert_eq!(
            lex("\"unclosed").first().map(|t| t.kind),
            Some(TokenKind::Unterminated { ident: true })
        );
        assert_eq!(
            lex("'unclosed").first().map(|t| t.kind),
            Some(TokenKind::Unterminated { ident: false })
        );
        // Doubled-quote then no close is still unbalanced → unterminated.
        assert_eq!(
            lex("\"a\"\"b").first().map(|t| t.kind),
            Some(TokenKind::Unterminated { ident: true })
        );
    }

    #[test]
    fn non_ascii_is_one_bare_ident_no_midcodepoint_span() {
        let toks = lex("café東京");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Ident { quoted: false });
        assert_eq!(&"café東京"[toks[0].start..toks[0].end], "café東京");
    }

    #[test]
    fn paren_group_tokens() {
        assert_eq!(
            kinds("(a, b)"),
            vec![
                TokenKind::Symbol(b'('),
                TokenKind::Ident { quoted: false },
                TokenKind::Symbol(b','),
                TokenKind::Ident { quoted: false },
                TokenKind::Symbol(b')'),
            ]
        );
    }

    #[test]
    fn well_formed_over_hostile_inputs() {
        for s in [
            "",
            "   ",
            "o AS \"weird PRIMARY KEY name\" PRIMARY KEY (id)",
            "\"a\"\"b\".\"c d\" UNIQUE (\"x)y\")",
            "COMMENT = 'the PRIMARY KEY (id) lives here'",
            "café AS \"東京 table\"",
            "'unterminated and PRIMARY KEY (id)",
            "\"unterminated ident",
        ] {
            assert_well_formed(s);
        }
    }

    // The tiling invariant is the whole point of the lexer — it must hold for
    // ARBITRARY input, not just the curated cases above (PR #100 review). These
    // proptests make the "no byte-slice ever cuts a codepoint" / "no
    // non-whitespace byte is dropped" guarantees generative rather than sampled.
    mod proptests {
        use super::assert_well_formed;
        use proptest::prelude::*;

        proptest! {
            /// A dense hostile alphabet: `"`/`'` (forming `""`/`''` escapes and
            /// unterminated regions), `(` `)` `;` `.` `=` `-`, unicode, and
            /// whitespace — the bytes whose handling the lexer is responsible for.
            #[test]
            fn tiling_holds_over_hostile_alphabet(s in r#"[-a-zé_ "'(),.;=]{0,48}"#) {
                assert_well_formed(&s);
            }

            /// Fully arbitrary Unicode (control bytes, astral-plane codepoints)
            /// the curated alphabet can't reach — the generative form of the
            /// char-boundary guarantee.
            #[test]
            fn tiling_holds_over_arbitrary_unicode(s in r"[\s\S]{0,32}") {
                assert_well_formed(&s);
            }
        }
    }
}
