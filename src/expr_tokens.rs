//! One quote-aware, case-insensitive tokenizer for *identifier references in
//! expression text*, shared by the graph validators and the expression
//! inliners.
//!
//! ## Why this exists (E-2 / E-3 / E-5)
//!
//! Fact and derived-metric expansion works by **textual substitution**: a
//! metric expression like `profit AS revenue - cost` is produced by replacing
//! the identifiers `revenue` / `cost` with their resolved SQL. Before this
//! module that replacement was a family of hand-rolled word-boundary scanners
//! ([`crate::util::replace_word_boundary_pairs`] and friends) that were **not**
//! quote-aware and disagreed with the validators on two things:
//!
//! - **E-2** — validation resolved references case-insensitively but the
//!   substitution matched case-sensitively / quote-blindly, so a quoted or
//!   mixed-case reference passed CREATE validation yet was silently not inlined.
//! - **E-3** — `.` was treated as a plain word boundary, so the bare needle
//!   `revenue` matched the column part of an *unrelated* qualified reference
//!   `x.revenue`, and a metric name inside a `'...'` string literal was
//!   substituted into — both producing invalid SQL from a valid definition.
//!
//! The fix is one tokenizer that understands SQL lexical structure:
//! [`scan_references`] walks an expression and yields the identifier
//! **reference chains** in it (`a`, `a.b`, `a.b."C d"`), skipping single-quoted
//! string literals and `$tag$…$tag$` dollar-quoted strings entirely. Two thin
//! consumers sit on top:
//!
//! - [`referenced_names_qualified`] / [`references_ref`] — the FIND primitive
//!   (which of these known names does this expression reference?), for
//!   dependency discovery and validation. A fact/metric matches bare or by its
//!   own `source_table.name`, never by a foreign relation's alias.
//! - [`inline_references`] — the INLINE primitive (splice a replacement over
//!   each reference whose normalized key is in the map).
//!
//! By construction this kills the class:
//! - string-literal / dollar-quote contents are never emitted as references, so
//!   they are never matched or substituted;
//! - `x.revenue` is a two-part chain with key `x.revenue`, so a bare needle
//!   `revenue` can never match it — no `.`-boundary special-case needed;
//! - matching goes through [`crate::ident::normalize_ident_part`], so it is
//!   case-insensitive **and** quote-insensitive (`"Revenue"` matches the
//!   declaration `revenue` — TECH-DEBT #28) with no third case/quote rule.
//!
//! Expressions reaching this layer are already comment-blanked at the parse
//! entry points (PA-7, [`crate::util::blank_sql_comments`]), so the tokenizer
//! does not handle SQL comments — they are spaces by the time we see them.
//!
//! The **alias-qualifier** rewriters (`expand::window` / `expand::sql_gen` /
//! `expand::semi_additive`, which rewrite the `a` in `a.city` to a scoped
//! alias `a__dep`) are a *different* operation — they intentionally match the
//! part *before* a dot — and stay on [`crate::util::replace_word_boundary`].

use std::collections::HashMap;

/// One identifier reference found in expression text: a maximal run of
/// dot-joined identifier parts (`a`, `a.b`, `a.b."C d"`), with its byte span in
/// the original expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdentRef<'a> {
    /// Byte offset of the first byte of the chain in the source expression.
    pub(crate) start: usize,
    /// Byte offset one past the last byte of the chain.
    pub(crate) end: usize,
    /// The raw chain text, `&expr[start..end]` (quotes and all).
    pub(crate) raw: &'a str,
}

impl IdentRef<'_> {
    /// The case-folded, quote-stripped match key for this chain, e.g. `x.revenue`
    /// or `revenue`. Delegates to [`crate::ident::normalize_ident_part`], the
    /// single source of the identifier match rule, so FIND and INLINE agree with
    /// name resolution everywhere else.
    pub(crate) fn key(&self) -> String {
        crate::ident::normalize_ident_part(self.raw)
    }
}

/// Scan `expr` and return every identifier reference chain in it, in source
/// order, skipping the contents of single-quoted string literals (`'…'` with
/// `''` escape) and `$tag$…$tag$` dollar-quoted strings.
///
/// A chain is one or more identifier parts joined by `.` with no intervening
/// whitespace; a part is either a bare run of identifier bytes
/// ([`crate::util::is_ident_byte`] — ASCII alphanumerics, `_`, and every
/// non-ASCII byte, so Unicode identifiers scan intact) or a `"…"` quoted part
/// (with `""` escape). The returned spans are non-overlapping and tile the
/// non-literal identifier text.
pub(crate) fn scan_references(expr: &str) -> Vec<IdentRef<'_>> {
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut refs = Vec::new();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        match b {
            b'\'' => i = skip_single_quoted(bytes, i),
            b'$' => {
                if let Some(next) = try_skip_dollar_quoted(bytes, i) {
                    i = next;
                } else {
                    i += 1;
                }
            }
            b'"' => {
                let end = scan_chain(bytes, i);
                refs.push(IdentRef {
                    start: i,
                    end,
                    raw: &expr[i..end],
                });
                i = end;
            }
            _ if crate::util::is_ident_byte(b) => {
                let end = scan_chain(bytes, i);
                refs.push(IdentRef {
                    start: i,
                    end,
                    raw: &expr[i..end],
                });
                i = end;
            }
            _ => i += 1,
        }
    }
    refs
}

/// Byte offset just past the closing `'` of a single-quoted string starting at
/// `bytes[i] == '\''`, honouring the `''` escape. Saturates at `len` for an
/// unterminated literal.
fn skip_single_quoted(bytes: &[u8], i: usize) -> usize {
    let len = bytes.len();
    let mut j = i + 1;
    while j < len {
        if bytes[j] == b'\'' {
            if j + 1 < len && bytes[j + 1] == b'\'' {
                j += 2; // '' escape — stay inside the literal
                continue;
            }
            return j + 1; // closing quote consumed
        }
        j += 1;
    }
    len
}

/// If `bytes[i]` opens a valid `$tag$` dollar quote, return the byte offset just
/// past its matching close (saturating at `len` if unterminated); otherwise
/// `None` (so a lone `$` is treated as an ordinary non-identifier byte).
fn try_skip_dollar_quoted(bytes: &[u8], i: usize) -> Option<usize> {
    let tag_len = crate::util::read_dollar_tag_len(bytes, i)?;
    let len = bytes.len();
    let tag = &bytes[i..i + tag_len];
    let mut j = i + tag_len;
    while j + tag_len <= len {
        if &bytes[j..j + tag_len] == tag {
            return Some(j + tag_len);
        }
        j += 1;
    }
    Some(len) // unterminated dollar quote — swallow to end
}

/// Scan one identifier chain starting at `bytes[start]` (a `"` or an identifier
/// byte). Returns the byte offset one past the chain.
fn scan_chain(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    let mut i = start;
    loop {
        if i < len && bytes[i] == b'"' {
            i = skip_quoted_part(bytes, i);
        } else {
            let part_start = i;
            while i < len && crate::util::is_ident_byte(bytes[i]) {
                i += 1;
            }
            if i == part_start {
                // No progress (e.g. a stray `.` with no following part) — stop.
                break;
            }
        }
        // Continue the chain only across a `.` that is *immediately* followed by
        // another part (`a.b`, `a."b"`), never across whitespace (`a . b` is two
        // chains) — mirroring `ident::parse_qualified_identifier`.
        if i + 1 < len
            && bytes[i] == b'.'
            && (bytes[i + 1] == b'"' || crate::util::is_ident_byte(bytes[i + 1]))
        {
            i += 1; // consume the dot; loop for the next part
        } else {
            break;
        }
    }
    i
}

/// Byte offset just past a `"…"` quoted identifier part starting at
/// `bytes[i] == '"'`, honouring the `""` escape. Saturates at `len` for an
/// unterminated quote.
fn skip_quoted_part(bytes: &[u8], i: usize) -> usize {
    let len = bytes.len();
    let mut j = i + 1;
    while j < len {
        if bytes[j] == b'"' {
            if j + 1 < len && bytes[j + 1] == b'"' {
                j += 2; // "" escape
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    len
}

/// The set of normalized keys of every reference chain in `expr` (bare and
/// qualified), skipping string / dollar-quoted literal content. The FIND
/// building block: a name is referenced iff one of its acceptable keys is in
/// this set.
pub(crate) fn reference_keys(expr: &str) -> std::collections::HashSet<String> {
    scan_references(expr).into_iter().map(|r| r.key()).collect()
}

/// Of `candidates`, those referenced in `expr`, in slice order (each once).
///
/// Each candidate is `(name, source_table)`. It is referenced when a reference
/// chain in `expr` matches either its **bare** key (`name`) or, when it has a
/// source table, its **own-qualified** key (`source_table.name`). A *foreign*
/// qualified reference (`x.name` where `x` is not the candidate's source table)
/// does not match — this is the E-3 fix — and neither does text inside a
/// string / dollar-quoted literal. Matching is quote- and case-insensitive via
/// [`crate::ident::normalize_ident_part`], so `"Revenue"` matches `revenue`.
///
/// This is the shared FIND primitive for fact / metric dependency discovery: a
/// fact or base metric may be referenced bare (`revenue`) or qualified by its
/// own source table (`li.revenue`), but never by another relation's alias.
pub(crate) fn referenced_names_qualified<'a>(
    expr: &str,
    candidates: &[(&'a str, Option<&str>)],
) -> Vec<&'a str> {
    let keys = reference_keys(expr);
    candidates
        .iter()
        .filter(|(name, src)| {
            keys.contains(&crate::ident::normalize_ident_part(name))
                || src.is_some_and(|s| {
                    keys.contains(&crate::ident::normalize_ident_part(&format!("{s}.{name}")))
                })
        })
        .map(|(name, _)| *name)
        .collect()
}

/// Whether `expr` references the fact/metric `name` (bare or qualified by its
/// own `source_table`). The single-candidate FIND convenience.
pub(crate) fn references_ref(expr: &str, name: &str, source_table: Option<&str>) -> bool {
    !referenced_names_qualified(expr, &[(name, source_table)]).is_empty()
}

/// Replace each identifier reference in `expr` whose normalized key
/// ([`IdentRef::key`]) is present in `replacements` with the mapped text, in a
/// single left-to-right pass over the *original* expression.
///
/// Because the expression is tokenized once and replacements are spliced by
/// byte span, inserted text is never rescanned — the double-substitution hazard
/// that forced [`crate::util::replace_word_boundary_pairs`]'s single-pass design
/// (SG-3) cannot occur here, and there is no needle-ordering concern: each chain
/// resolves to exactly one key (`o.net_price` and `net_price` are distinct
/// keys). Literal text and unmatched references are copied verbatim, so a
/// metric name inside `'…'` or a foreign `x.net_price` is left intact.
///
/// Keys are normalized identifier keys (as produced by
/// [`crate::ident::normalize_ident_part`]): a bare `revenue` or a qualified
/// `o.net_price`.
pub(crate) fn inline_references(expr: &str, replacements: &HashMap<String, &str>) -> String {
    if replacements.is_empty() {
        return expr.to_string();
    }
    let mut out = String::with_capacity(expr.len());
    let mut copied = 0;
    for r in scan_references(expr) {
        if let Some(repl) = replacements.get(&r.key()) {
            out.push_str(&expr[copied..r.start]);
            out.push_str(repl);
            copied = r.end;
        }
    }
    out.push_str(&expr[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn keys(expr: &str) -> Vec<String> {
        scan_references(expr).iter().map(IdentRef::key).collect()
    }

    #[test]
    fn scans_bare_and_qualified_chains() {
        assert_eq!(keys("revenue - cost"), vec!["revenue", "cost"]);
        assert_eq!(keys("SUM(o.amount)"), vec!["sum", "o.amount"]);
        assert_eq!(keys("a.b.c"), vec!["a.b.c"]);
    }

    #[test]
    fn quoted_parts_fold_and_join() {
        // A quoted part normalizes exactly like an unquoted one (DuckDB rule).
        assert_eq!(keys("\"Revenue\""), vec!["revenue"]);
        assert_eq!(keys("a.\"C d\""), vec!["a.c d"]);
        assert_eq!(keys("SUM(\"Rev\")"), vec!["sum", "rev"]);
        // "" escape inside a quoted part stays one part.
        assert_eq!(keys("\"with\"\"q\""), vec!["with\"q"]);
    }

    #[test]
    fn string_literals_yield_no_references() {
        assert!(scan_references("'total revenue'").is_empty());
        assert!(scan_references("'it''s revenue'").is_empty());
        assert_eq!(keys("revenue + 'revenue'"), vec!["revenue"]);
    }

    #[test]
    fn dollar_quoted_strings_yield_no_references() {
        assert!(scan_references("$$revenue$$").is_empty());
        assert!(scan_references("$tag$a.b.c$tag$").is_empty());
        assert_eq!(keys("x $$revenue$$ y"), vec!["x", "y"]);
        // A lone `$` is not a dollar quote (or a positional `$1`): scanning
        // continues past it.
        assert_eq!(keys("revenue $ cost"), vec!["revenue", "cost"]);
    }

    #[test]
    fn dotted_chain_is_not_split_by_whitespace() {
        // `a . b` (spaced) is two chains, `a.b` (contiguous) is one.
        assert_eq!(keys("a . b"), vec!["a", "b"]);
        assert_eq!(keys("a.b"), vec!["a.b"]);
    }

    // ----- referenced_names_qualified / references_ref (FIND) -----

    /// Bare-only FIND helper (no source table) for the tests below.
    fn bare(expr: &str, names: &[&'static str]) -> Vec<&'static str> {
        let cands: Vec<(&str, Option<&str>)> = names.iter().map(|&n| (n, None)).collect();
        referenced_names_qualified(expr, &cands)
    }

    #[test]
    fn find_is_case_and_quote_insensitive() {
        // E-2: a mixed-case / quoted reference still resolves.
        assert_eq!(
            bare("REVENUE - Cost", &["revenue", "cost"]),
            vec!["revenue", "cost"]
        );
        assert_eq!(bare("\"Revenue\"", &["revenue"]), vec!["revenue"]);
    }

    #[test]
    fn find_skips_foreign_qualified_and_string_literals() {
        // E-3: a *foreign* `x.revenue` is NOT a `revenue` reference, and a
        // string literal is never a reference.
        assert!(bare("x.revenue", &["revenue"]).is_empty());
        assert!(bare("'revenue'", &["revenue"]).is_empty());
        assert!(!references_ref("x.revenue + 'revenue'", "revenue", None));
        assert!(references_ref("revenue + x.revenue", "revenue", None));
    }

    #[test]
    fn find_matches_own_source_qualified_but_not_foreign() {
        // A fact/metric `net_price` on table `o` is referenced by `o.net_price`
        // (own table) but NOT by `x.net_price` (foreign) — the distinction the
        // bare-only scan could not make.
        assert!(references_ref("o.net_price + 1", "net_price", Some("o")));
        assert!(!references_ref("x.net_price + 1", "net_price", Some("o")));
        // Bare still matches regardless of source table.
        assert!(references_ref("SUM(net_price)", "net_price", Some("o")));
    }

    #[test]
    fn find_preserves_names_order_and_dedupes() {
        assert_eq!(
            bare("cost + revenue + cost", &["revenue", "cost", "profit"]),
            vec!["revenue", "cost"]
        );
    }

    // ----- inline_references (INLINE) -----

    #[test]
    fn inline_bare_and_qualified_keys() {
        let mut m: HashMap<String, &str> = HashMap::new();
        m.insert("revenue".to_string(), "(SUM(o.amount))");
        assert_eq!(
            inline_references("revenue - cost", &m),
            "(SUM(o.amount)) - cost"
        );
        // A foreign qualified column is left intact (E-3).
        assert_eq!(inline_references("x.revenue", &m), "x.revenue");
        // Own qualified form matches its own key.
        m.insert("o.net_price".to_string(), "(price)");
        assert_eq!(inline_references("SUM(o.net_price)", &m), "SUM((price))");
    }

    #[test]
    fn inline_leaves_string_literals_intact() {
        let mut m: HashMap<String, &str> = HashMap::new();
        m.insert("revenue".to_string(), "(SUM(o.amount))");
        // The metric name inside the literal survives; the bare ref is replaced.
        assert_eq!(
            inline_references("revenue || ' revenue '", &m),
            "(SUM(o.amount)) || ' revenue '"
        );
    }

    #[test]
    fn inline_is_case_and_quote_insensitive() {
        let mut m: HashMap<String, &str> = HashMap::new();
        m.insert("revenue".to_string(), "(R)");
        m.insert("cost".to_string(), "(C)");
        // E-2 + #28: mixed-case and quoted references both inline.
        assert_eq!(inline_references("REVENUE - \"Cost\"", &m), "(R) - (C)");
    }

    #[test]
    fn inline_does_not_rescan_inserted_text() {
        // Replacement text that itself contains a key is not re-substituted.
        let mut m: HashMap<String, &str> = HashMap::new();
        m.insert("net_price".to_string(), "(o.net_price)");
        assert_eq!(
            inline_references("SUM(net_price)", &m),
            "SUM((o.net_price))"
        );
    }

    // ----- generative proptests -----

    /// A quoted-identifier part whose inner content is arbitrary (including
    /// characters that would otherwise be delimiters), `""`-escaped.
    fn arb_expr_atom() -> impl Strategy<Value = String> {
        prop_oneof![
            // bare identifiers (incl. non-ASCII)
            "[a-zA-Z_][a-zA-Z0-9_]{0,6}",
            "[a-zàéΩ東][a-zàéΩ東_]{0,4}",
            // qualified references
            "[a-z]{1,4}\\.[a-z]{1,4}",
            // quoted identifiers with hostile inner content
            r#""[a-zA-Z0-9 .,()]{0,6}""#,
            // single-quoted string literals with identifier-looking content
            "'[a-zA-Z0-9 .]{0,8}'",
            // dollar-quoted strings
            Just("$$a.b revenue$$".to_string()),
            // operators / punctuation / whitespace
            prop::sample::select(vec![
                " + ".to_string(),
                " - ".to_string(),
                " * ".to_string(),
                ", ".to_string(),
                "(".to_string(),
                ")".to_string(),
                " ".to_string(),
            ]),
        ]
    }

    fn arb_expr() -> impl Strategy<Value = String> {
        prop::collection::vec(arb_expr_atom(), 0..12).prop_map(|atoms| atoms.concat())
    }

    proptest! {
        /// The reference spans are non-overlapping, in order, in bounds, and on
        /// char boundaries — they tile a subset of the input.
        #[test]
        fn refs_are_ordered_non_overlapping_and_on_char_boundaries(expr in arb_expr()) {
            let mut last_end = 0;
            for r in scan_references(&expr) {
                prop_assert!(r.start >= last_end, "overlap/disorder in {expr:?}");
                prop_assert!(r.end <= expr.len());
                prop_assert!(expr.is_char_boundary(r.start));
                prop_assert!(expr.is_char_boundary(r.end));
                prop_assert_eq!(&expr[r.start..r.end], r.raw);
                last_end = r.end;
            }
        }

        /// Scanning any input is total: it never panics, and `key()` on every
        /// ref is likewise total. (Key *stability* is a property of
        /// `normalize_ident_part`, tested in `ident`, not here — a quoted
        /// whitespace-only identifier is a deliberate non-goal.)
        #[test]
        fn scanning_is_total(expr in arb_expr()) {
            for r in scan_references(&expr) {
                let _ = r.key();
            }
        }

        /// A key that appears ONLY inside a string/dollar literal is never a
        /// found reference and is never substituted.
        #[test]
        fn literal_content_is_never_a_reference(inner in "[a-z]{1,6}") {
            let lit = format!("'{inner}'");
            prop_assert!(!references_ref(&lit, &inner, None));
            let dollar = format!("$${inner}$$");
            prop_assert!(!references_ref(&dollar, &inner, None));
            let mut m: HashMap<String, &str> = HashMap::new();
            m.insert(inner.clone(), "REPL");
            prop_assert_eq!(inline_references(&lit, &m), lit.clone());
            prop_assert_eq!(inline_references(&dollar, &m), dollar);
        }

        /// FIND and INLINE agree: a name is reported as referenced iff inlining
        /// it changes the expression.
        #[test]
        fn find_and_inline_agree(expr in arb_expr(), name in "[a-z]{1,5}") {
            let found = references_ref(&expr, &name, None);
            let mut m: HashMap<String, &str> = HashMap::new();
            m.insert(name.to_ascii_lowercase(), "<R>");
            let changed = inline_references(&expr, &m) != expr;
            // A bare reference is found iff a (bare-key) inline changes the text.
            prop_assert_eq!(found, changed, "expr={:?} name={:?}", expr, name);
        }

        /// A bare needle never matches the tail of a contiguous qualified
        /// reference (E-3).
        #[test]
        fn bare_needle_never_matches_qualified_tail(
            head in "[a-z]{1,4}",
            tail in "[a-z]{1,5}",
        ) {
            let expr = format!("{head}.{tail}");
            // `tail` is not referenced bare (it is a foreign qualified tail),
            // and it is not referenced when qualified by a source table other
            // than `head`.
            prop_assert!(!references_ref(&expr, &tail, None));
            let mut m: HashMap<String, &str> = HashMap::new();
            m.insert(tail.clone(), "<R>");
            prop_assert_eq!(inline_references(&expr, &m), expr);
        }
    }
}
