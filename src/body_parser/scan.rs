//! Low-level byte/keyword scanning helpers shared by the clause parsers.

use crate::errors::ParseError;

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
///
/// Dollar-quoted strings (`$tag$ ... $tag$`, PARSE-1 / code-review 2026-07-18)
/// are tracked too: a `,` / `)` / keyword inside one is inert, matching the
/// comment-blanker and the CREATE-body extractor which already share the tag
/// grammar via [`crate::util::read_dollar_tag_len`]. Without this a comma inside
/// a dimension/metric expression's `$$...$$` split one entry into two garbage
/// entries (the P-1/P-2 silent-mis-parse class).
#[derive(Default, Clone, Copy)]
pub(super) struct QuoteState {
    pub(super) in_string: bool,
    pub(super) in_ident: bool,
    /// When inside a `$tag$ ... $tag$` region, the byte span `[start, end)` of
    /// the OPENING tag within the buffer being scanned; `None` otherwise. Stored
    /// as offsets (not the tag bytes) so `QuoteState` stays `Copy`; the offsets
    /// index the same `bytes` slice passed to every `step` call, which every
    /// scan-in-a-loop caller preserves.
    dollar_open: Option<(usize, usize)>,
}

impl QuoteState {
    /// True while inside an unterminated (still-open) dollar-quoted region.
    pub(super) fn in_dollar(&self) -> bool {
        self.dollar_open.is_some()
    }

    /// Consume the byte at `i`, updating quote state. Returns
    /// `(next_index, is_live_code)` where `is_live_code` is true only when
    /// byte `i` is outside every quoted region and is not itself a quote
    /// delimiter. Escape pairs / whole dollar tags are consumed at once.
    pub(super) fn step(&mut self, bytes: &[u8], i: usize) -> (usize, bool) {
        let b = bytes[i];
        if let Some((ts, te)) = self.dollar_open {
            // Inside `$tag$...$tag$`: only the IDENTICAL closing tag ends the
            // region — a different inner tag ($z$) or a lone `$` does not.
            if b == b'$' && bytes[i..].starts_with(&bytes[ts..te]) {
                self.dollar_open = None;
                return (i + (te - ts), false); // consume the whole closing tag
            }
            return (i + 1, false);
        }
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
                b'$' => {
                    // A valid `$tag$` opener starts a dollar-quoted region; a
                    // lone `$` or a `$1` positional parameter (rejected by
                    // read_dollar_tag_len) is ordinary live code.
                    if let Some(len) = crate::util::read_dollar_tag_len(bytes, i) {
                        self.dollar_open = Some((i, i + len));
                        (i + len, false) // consume the whole opening tag
                    } else {
                        (i + 1, true)
                    }
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
    } else if st.in_dollar() {
        Some("Unterminated dollar-quoted string")
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
/// drifting left into the gap (P-4, code-review 2026-07-11).
///
/// A single **trailing** comma is tolerated (`a, b,` → `[a, b]`). A **leading**
/// or **interior** empty entry — a stray comma with no content before it
/// (`,a`, `a,,b`) — is rejected rather than silently dropped (T-13, code-review
/// 2026-07-16; the P-2 no-silent-discard rule). The returned error has
/// `position: None` — the helper is base-offset-agnostic (the offsets it returns
/// are body-relative), so there is no absolute caret to attach — and callers
/// propagate its message as-is via `?`.
pub(crate) fn split_at_depth0_commas(body: &str) -> Result<Vec<(usize, &str)>, ParseError> {
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
                    if entry.is_empty() {
                        // A leading (`,a`) or interior (`a,,b`) empty entry is a
                        // stray comma. A trailing comma leaves its empty segment
                        // in the tail below (not between two commas), so `a,`
                        // stays allowed.
                        return Err(ParseError {
                            message: "Empty entry: a stray or leading ',' with no content before it. Remove the extra comma."
                                .to_string(),
                            position: None,
                        });
                    }
                    entries.push((crate::util::byte_offset_within(body, entry), entry));
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
    Ok(entries)
}

// `split_first_token` (first-whitespace split for the MATERIALIZATIONS name)
// was retired in the §6.1 MATERIALIZATIONS migration: the name is now the first
// TOKEN via `super::cursor::Cursor`, so a quoted name containing whitespace
// (`"my mat"`) stays one identifier instead of splitting mid-quote
// (code-review 2026-07-11).

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

/// Validate a captured identifier slot — a table alias, a source-table name, or
/// a dimension / metric / relationship name. A valid slot is a single,
/// well-formed (optionally dot-qualified, optionally `"quoted"`) SQL
/// identifier.
///
/// Returns `Some(reason)` when the slot is malformed, for the caller to splice
/// into a clause-specific error. Two porting-friction classes are caught
/// (code-review 2026-07-16):
///   * **F-9** — a whitespace-separated multi-token run (`o.d junk AS x` named
///     the dimension `"d junk"`): an unquoted space / `;` ends the identifier,
///     so anything after it is a stray token, not part of the name.
///   * **F-11** — an empty quoted identifier (`""`) and the other malformed
///     identifier shapes the shared grammar rejects (unterminated quote,
///     `foo"bar"` bare-abutting-quoted). `DuckDB` itself rejects a zero-length
///     quoted identifier, and view-name slots already do (`ident.rs`); body
///     slots now match.
///
/// A quoted identifier that itself contains whitespace (`"a b"`) is a single
/// token and is accepted. An empty (all-whitespace) slot returns `None` — the
/// call site reports emptiness with a "missing name/alias" message of its own.
pub(super) fn identifier_slot_error(slot: &str) -> Option<String> {
    let s = slot.trim();
    if s.is_empty() {
        return None;
    }
    // F-9: the identifier ends at the first unquoted whitespace / `;`. If that
    // is not end-of-slot, a second token follows — the slot is not one name.
    if crate::ident::find_identifier_end(s, /* allow_paren = */ false) != s.len() {
        return Some(format!("'{s}' is not a single identifier"));
    }
    // F-11 + malformed shapes: the shared identifier grammar rejects `""` and
    // friends. (Unterminated quotes are already caught upstream per entry, but
    // re-checking here is harmless and keeps this helper total.)
    crate::ident::parse_qualified_identifier(s).err()
}

// ---------------------------------------------------------------------------
// Render-side round-trip guards (RT-4, fuzz_render_roundtrip 2026-07-18)
//
// `render_ddl` must satisfy the fixpoint `render(parse(render(def))) ==
// render(def)`. A stored column / alias / table string that does NOT re-parse
// back to itself verbatim breaks that invariant when emitted raw (a depth-0
// comma splits one column into two, a blank alias re-parses as garbage, etc.).
// The renderer consults these predicates and wraps any non-round-tripping
// string with `quote_ident`; because they must agree exactly with how the
// clause parsers tokenize, they live here beside `QuoteState` / the lexer /
// `identifier_slot_error` and reuse the same primitives. Each predicate is
// conservative: canonical values (bare words, well-formed `"quoted"` idents,
// dotted table names) return `true` and are emitted UNCHANGED; anything whose
// verbatim round-trip is not guaranteed returns `false` and is quoted.
// ---------------------------------------------------------------------------

/// True when `s`, emitted verbatim as ONE element of a `(a, b, ...)` column
/// list (PRIMARY KEY / UNIQUE / FK / REFERENCES), re-parses back to exactly
/// `s` via `take_parens` + [`split_at_depth0_commas`].
///
/// Requirements, matching those two consumers: non-empty; no leading/trailing
/// whitespace (which the parser trims away); no depth-0 comma (which would
/// split it into multiple columns); and balanced quotes and brackets that
/// never dip negative (so it neither leaks an open quote/paren into the list
/// nor closes the enclosing `(...)` early). Both the `(`/`)`-only balance
/// `take_parens` tracks and the `()[]{}` balance `split_at_depth0_commas`
/// tracks are enforced. Idempotent on already-`quote_ident`ed input: a
/// well-formed `"..."` token has no depth-0 comma and balanced quotes, so it
/// returns `true` and is left as-is (never re-quoted).
pub(crate) fn column_roundtrips_verbatim(s: &str) -> bool {
    if s.is_empty() || s.trim() != s {
        return false;
    }
    let bytes = s.as_bytes();
    let mut st = QuoteState::default();
    let mut paren_depth: i32 = 0; // '(' / ')' only — matches take_parens
    let mut bracket_depth: i32 = 0; // ()[]{} — matches split_at_depth0_commas
    let mut i = 0;
    while i < bytes.len() {
        let (next, live) = st.step(bytes, i);
        if live {
            match bytes[i] {
                b'(' => {
                    paren_depth += 1;
                    bracket_depth += 1;
                }
                b')' => {
                    paren_depth -= 1;
                    bracket_depth -= 1;
                    if paren_depth < 0 || bracket_depth < 0 {
                        return false;
                    }
                }
                b'[' | b'{' => bracket_depth += 1,
                b']' | b'}' => {
                    bracket_depth -= 1;
                    if bracket_depth < 0 {
                        return false;
                    }
                }
                b',' if bracket_depth == 0 => return false,
                _ => {}
            }
        }
        i = next;
    }
    paren_depth == 0 && bracket_depth == 0 && !st.in_string && !st.in_ident && !st.in_dollar()
}

/// True when `s`, emitted verbatim in a single-identifier slot (a table alias,
/// a relationship `from_alias`, or a `REFERENCES` target alias), re-parses
/// back to exactly `s`.
///
/// Those slots are read as ONE value token by the cursor, so `s` must lex to
/// exactly one terminated identifier token (bare or `"quoted"`) spanning the
/// whole string, and pass [`identifier_slot_error`] (rejecting the empty
/// quoted `""`). A multi-token run (`a b`, `a.b`, `a(b`) or an empty string
/// therefore returns `false` and is quoted. Idempotent: a well-formed
/// `"..."` token lexes as one quoted identifier and passes the slot check.
pub(crate) fn identifier_slot_roundtrips_verbatim(s: &str) -> bool {
    use super::lexer::{lex, TokenKind};
    let toks = lex(s);
    let [tok] = toks.as_slice() else {
        return false;
    };
    matches!(tok.kind, TokenKind::Ident { .. })
        && tok.start == 0
        && tok.end == s.len()
        && identifier_slot_error(s).is_none()
}

/// True when `s`, emitted verbatim as a source-table name (the `AS <table>`
/// slot of a TABLES entry), re-parses back to exactly `s` via
/// `take_source_table_name`.
///
/// Unlike an alias slot, a source-table name is a maximal CONTIGUOUS run of
/// tokens, so a dot-qualified name (`schema.orders`, `"db"."t"`) is captured
/// whole and round-trips verbatim — this predicate must leave such canonical
/// names UNCHANGED (never always-quote). It returns `true` iff `s` lexes into
/// a gap-free run of identifier tokens joined only by `.` symbols, covering
/// the whole string, is not a bare reserved keyword the name slot rejects, and
/// is a well-formed dotted identifier. Any interior whitespace, `(`/`)`/`,`/`;`
/// symbol, string literal, unterminated quote, or empty input returns `false`
/// and is quoted (collapsing to a single quoted part — acceptable, since such
/// values are non-canonical). Idempotent on a `quote_ident`ed value: `"..."`
/// is one quoted token covering the whole string.
pub(crate) fn source_table_roundtrips_verbatim(s: &str) -> bool {
    use super::lexer::{lex, TokenKind};
    let toks = lex(s);
    let Some(first) = toks.first() else {
        return false; // empty
    };
    // No surrounding whitespace (lexer skips it, so a covered run starts at 0
    // and ends at len only when there is none).
    if first.start != 0 || toks.last().is_none_or(|t| t.end != s.len()) {
        return false;
    }
    let mut prev_end: Option<usize> = None;
    for t in &toks {
        if let Some(pe) = prev_end {
            if pe != t.start {
                return false; // interior whitespace gap ends the name early
            }
        }
        match t.kind {
            // An identifier part, or the `.` that separates a dotted FQN.
            TokenKind::Ident { .. } | TokenKind::Symbol(b'.') => {}
            _ => return false, // (, ), comma, ;, string literal, unterminated, ...
        }
        prev_end = Some(t.end);
    }
    // A bare reserved keyword in the name slot is rejected by the parser.
    if matches!(
        s.to_ascii_uppercase().as_str(),
        "PRIMARY" | "UNIQUE" | "FOREIGN" | "REFERENCES" | "NOT"
    ) {
        return false;
    }
    // Well-formed dotted identifier (rejects `""`, `a..b`, `foo"bar"`, ...).
    crate::ident::parse_qualified_identifier(s).is_ok()
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

// `extract_paren_content` (content between the first `(` and its matching `)`)
// was retired in the §6.1 MATERIALIZATIONS migration: the sub-body and each
// DIMENSIONS/METRICS list are now consumed with `super::cursor::Cursor`'s
// quote-aware `take_parens` (code-review 2026-07-11). `extract_paren_prefix`
// stays — the trailing-annotation parser still needs the consumed-byte count.

/// Returns the content inside the outermost `(...)` of `s` (which must start
/// with `(`) plus the number of bytes consumed through the matching closing `)`
/// (so a caller can advance past the whole `(...)` group and inspect what
/// follows). Quote-aware: brackets inside `'...'` string literals and `"..."`
/// quoted identifiers are inert (PA-6). Returns `None` if unbalanced.
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
