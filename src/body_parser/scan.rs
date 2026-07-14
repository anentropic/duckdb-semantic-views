//! Low-level byte/keyword scanning helpers shared by the clause parsers.

/// Byte-scan state for SQL text: tracks single-quoted string literals and
/// double-quoted identifiers, honouring the SQL escape doubling (`''` inside
/// a string, `""` inside a quoted identifier).
///
/// This is the ONE quote-tracking implementation for every depth-0 scanner
/// in this module (PA-6, code-review 2026-07-02): scanners that tracked only
/// single quotes — or nothing — mis-split on quoted identifiers containing
/// commas / parens / dots (`o."a,b"`, `o AS "tbl)x"`, `"a.b"`) and matched
/// keywords inside string literals (PA-3: a `COMMENT = 'the PRIMARY KEY (id)
/// lives here'` fabricated a primary key from comment text).
///
/// Multi-byte UTF-8 is safe by construction: only ASCII bytes are compared,
/// and continuation bytes (>= 0x80) never equal an ASCII quote.
#[derive(Default, Clone, Copy)]
pub(super) struct QuoteState {
    pub(super) in_string: bool,
    pub(super) in_ident: bool,
}

impl QuoteState {
    /// Consume the byte at `i`, updating quote state. Returns
    /// `(next_index, is_live_code)` where `is_live_code` is true only when
    /// byte `i` is outside every quoted region and is not itself a quote
    /// delimiter. Escape pairs are consumed whole (`next_index == i + 2`).
    pub(super) fn step(&mut self, bytes: &[u8], i: usize) -> (usize, bool) {
        let b = bytes[i];
        if self.in_string {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    return (i + 2, false); // '' escape — stay in string
                }
                self.in_string = false;
            }
            (i + 1, false)
        } else if self.in_ident {
            if b == b'"' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    return (i + 2, false); // "" escape — stay in ident
                }
                self.in_ident = false;
            }
            (i + 1, false)
        } else {
            match b {
                b'\'' => {
                    self.in_string = true;
                    (i + 1, false)
                }
                b'"' => {
                    self.in_ident = true;
                    (i + 1, false)
                }
                _ => (i + 1, true),
            }
        }
    }
}

/// Scan `s` to completion and return the final quote state. Used by entry
/// parsers to reject unterminated `"..."` / `'...'` regions up front with a
/// precise error — the quote-aware scanners otherwise swallow everything
/// after the orphan quote and surface a misleading structural error
/// ("Expected 'AS' keyword") instead.
fn final_quote_state(s: &str) -> QuoteState {
    let bytes = s.as_bytes();
    let mut st = QuoteState::default();
    let mut i = 0;
    while i < bytes.len() {
        let (next, _) = st.step(bytes, i);
        i = next;
    }
    st
}

/// Reject an entry whose quoting never closes. Returns the error message
/// noun for the open region, if any.
pub(super) fn unterminated_quote_error(s: &str) -> Option<&'static str> {
    let st = final_quote_state(s);
    if st.in_ident {
        Some("Unterminated quoted identifier")
    } else if st.in_string {
        Some("Unterminated string literal")
    } else {
        None
    }
}

// `find_live_byte` (the quote-aware "first byte outside quotes" scan for
// alias/name dot-splits) was retired in the §6.1 migration: the qualifier `.`
// is now the first `.` SYMBOL token, which is inherently quote-aware since a
// dot inside a string/quoted-ident is part of that one token (code-review
// 2026-07-11).

/// Split `body` at depth-0 commas, respecting nested parens, single-quoted
/// strings, and double-quoted identifiers.
///
/// Returns `Vec<(offset_in_body, trimmed_slice)>` where the offset is that of
/// the **trimmed** slice's first byte — not the position right after the comma.
/// Leading whitespace between the comma and the entry is excluded, so an error
/// caret computed as `base + offset` lands on the entry itself rather than
/// drifting left into the gap (P-4, code-review 2026-07-11). Trailing empty
/// entries are discarded.
pub(crate) fn split_at_depth0_commas(body: &str) -> Vec<(usize, &str)> {
    let mut entries = Vec::new();
    let mut depth: i32 = 0;
    let mut st = QuoteState::default();
    let mut start = 0;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let (next, live) = st.step(bytes, i);
        if live {
            match bytes[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    let entry = body[start..i].trim();
                    if !entry.is_empty() {
                        entries.push((crate::util::byte_offset_within(body, entry), entry));
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        i = next;
    }
    let tail = body[start..].trim();
    if !tail.is_empty() {
        entries.push((crate::util::byte_offset_within(body, tail), tail));
    }
    entries
}

/// Split `s` at the first ASCII whitespace, returning `(first_token, rest)`.
/// If no whitespace found, returns `(s, "")`.
pub(super) fn split_first_token(s: &str) -> (&str, &str) {
    if let Some(pos) = s.find(|c: char| c.is_ascii_whitespace()) {
        (&s[..pos], &s[pos..])
    } else {
        (s, "")
    }
}

/// Phase 68 B1 (D-08): split a qualified identifier at the FIRST dot that
/// falls OUTSIDE a double-quoted region. Returns
/// `Some((before_first_dot, after_first_dot))` if a split-eligible dot
/// exists, else `None`. The `after_first_dot` slice may itself contain
/// further dots (and/or quoted regions); this helper does NOT recursively
/// split — callers that need 3+ segment handling must re-invoke or do their
/// own scanning. Doubled-quote `""` inside `"..."` is treated as an escape
/// (mirrors `is_quoting_balanced` / `find_identifier_end`).
///
/// Returns `None` if either side of the split would be empty (WR-01).
///
/// Examples:
/// - `"o.x"` → `Some(("o", "x"))`
/// - `"o.\"order date\""` → `Some(("o", "\"order date\""))`
/// - `"\"a.b\""` → `None` (the dot is inside the quoted region)
/// - `"bare"` → `None` (no dot)
/// - `".foo"` / `"foo."` → `None` (empty side, WR-01)
/// - `"db.sch.\"tbl\""` → `Some(("db", "sch.\"tbl\""))` (WR-02: caller
///   must handle further splitting if 3+ segments are expected)
pub(super) fn split_qualified_identifier(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_quote = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            if in_quote && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                // Doubled-quote escape — stay inside the quoted region.
                i += 2;
                continue;
            }
            in_quote = !in_quote;
            i += 1;
            continue;
        }
        if !in_quote && b == b'.' {
            let alias = &s[..i];
            let name = &s[i + 1..];
            // WR-01 (Phase 68 review): reject malformed inputs where either side
            // of the split is empty (e.g. leading `.foo` or trailing `foo.`).
            // Today's callers tolerate `Some(("", "foo"))` because every parsed
            // dimension carries a non-empty `source_table`, but the helper is a
            // leaf utility and a future caller deserves a clean None.
            if alias.is_empty() || name.is_empty() {
                return None;
            }
            return Some((alias, name));
        }
        i += 1;
    }
    None
}

/// Is `b` an identifier-continuation byte? Post-keyword boundary checks must
/// use this rather than a bare `is_ascii_alphanumeric()` test, or `BY_foo` /
/// `BYé` mis-tokenizes as the keyword `BY` ending early (PR #50 review).
///
/// Thin alias for [`crate::util::is_ident_byte`], the crate-wide single
/// source of truth for identifier bytes — kept under this name so the many
/// scanner call sites read naturally.
pub(super) fn is_ident_continuation(b: u8) -> bool {
    crate::util::is_ident_byte(b)
}

/// Phase 68 A4: returns `true` if `s` has balanced double-quote runs, treating
/// a doubled-quote `""` inside a quoted region as an escape (does NOT close).
/// Mirrors the escape rule used by `src/ident.rs::find_identifier_end` so the
/// two callers agree on what counts as "balanced". A naive
/// `s.matches('"').count() % 2 == 0` is incorrect because it double-counts
/// escaped quotes; this helper walks bytes explicitly.
pub(super) fn is_quoting_balanced(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut in_quote = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if in_quote && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                // Doubled-quote escape inside a quoted region — skip both
                // bytes without toggling the in_quote state.
                i += 2;
                continue;
            }
            in_quote = !in_quote;
        }
        i += 1;
    }
    !in_quote
}

/// Extract content inside the outermost `(...)` of `s` (which must start with `(`).
/// Returns the content between the first `(` and its matching `)`, or `None` if unbalanced.
/// Quote-aware: brackets inside `'...'` string literals and `"..."` quoted
/// identifiers are inert (PA-6).
pub(super) fn extract_paren_content(s: &str) -> Option<&str> {
    extract_paren_prefix(s).map(|(inner, _)| inner)
}

/// Like [`extract_paren_content`], but also returns the number of bytes consumed
/// through the matching closing `)` (so a caller can advance past the whole
/// `(...)` group and inspect what follows). `s` must start with `(`.
pub(super) fn extract_paren_prefix(s: &str) -> Option<(&str, usize)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[0] != b'(' {
        return None;
    }
    let mut depth = 0i32;
    let mut st = QuoteState::default();
    let mut start = None;
    let mut i = 0;
    while i < bytes.len() {
        let (next, live) = st.step(bytes, i);
        if live {
            match bytes[i] {
                b'(' => {
                    depth += 1;
                    if depth == 1 {
                        start = Some(i + 1);
                    }
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((&s[start.unwrap()..i], i + 1));
                    }
                }
                _ => {}
            }
        }
        i = next;
    }
    None
}

// `find_keyword_ci` (case-insensitive, quote-aware, word-boundaried keyword
// search) and `find_depth0_keyword` (its paren-depth-0 variant, used for OVER)
// were retired in the §6.1 migration: clause parsers now recognize keywords as
// bare-identifier TOKENS via `super::cursor::Cursor` (`find_kw` / `find_kw_seq`
// / `find_kw_depth0`), which are inherently quote-aware and UTF-8-safe
// (code-review 2026-07-11).

// `find_primary_key` / `find_unique` (the case-insensitive, quote-aware
// "find this constraint keyword anywhere in the post-name slice and slice from
// it" scanners) were retired in the §6.1 TABLES migration: the TABLES clause
// now consumes tokens in order via `super::cursor::Cursor`, so a constraint
// keyword is recognized as a bare-identifier token — inherently quote-aware and
// UTF-8-safe — rather than by a substring scan (code-review 2026-07-11).
