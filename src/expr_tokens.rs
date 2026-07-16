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
//! (`util::replace_word_boundary_pairs` and friends — since removed) that were **not**
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
//! alias `a__dep`) are a *different* operation — they rewrite the *head* part
//! of a chain rather than replacing the whole reference — but they go through
//! this same engine via [`rewrite_qualifier`], so they inherit its
//! literal-/function-/foreign-tail safety instead of the old quote-blind
//! `util::replace_word_boundary` (now retired).

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

    /// True when this chain is a single, *unqualified* part (`revenue`,
    /// `"Rev"`), false for a dotted chain (`a.b`, `o.amount`). Used by the
    /// derived-metric validator, which treats only bare identifiers as metric
    /// references — a qualified chain in a derived expression is a raw column,
    /// not a metric name.
    pub(crate) fn is_bare(&self) -> bool {
        first_part_len(self.raw) == self.raw.len()
    }

    /// The normalized (quote-stripped, lowercased) key of the chain's **last**
    /// part — the called name for a function head: `main.sum` → `sum`, but
    /// `"main.sum"` → `main.sum` because the dot lives *inside* a quoted part
    /// and so does not separate qualifiers. Splitting the joined [`key`] on `.`
    /// cannot make this distinction (a quoted dot and a qualifier dot look
    /// identical there), so this parses quote-aware via
    /// [`crate::ident::parse_qualified_identifier_with_quoting`].
    ///
    /// [`key`]: IdentRef::key
    pub(crate) fn last_part_key(&self) -> String {
        match crate::ident::parse_qualified_identifier_with_quoting(self.raw.trim()) {
            Ok(parts) => parts
                .last()
                .map(|(p, _)| p.to_ascii_lowercase())
                .unwrap_or_default(),
            Err(_) => crate::ident::normalize_ident_part(self.raw),
        }
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
///
/// A chain immediately followed by `(` (skipping whitespace) is a **function
/// call**, not a name reference (`SUM(x)`, `date_trunc (...)`), and is not
/// emitted — so a fact/metric named like a function (`sum`) is never confused
/// with the call, in either FIND or INLINE. The complementary chains (the call
/// heads themselves) are available via [`scan_function_heads`].
pub(crate) fn scan_references(expr: &str) -> Vec<IdentRef<'_>> {
    scan_chains(expr, ChainKind::Reference)
}

/// Which class of identifier chain a scan collects: [`scan_references`] keeps
/// the chains that are *not* function-call heads; [`scan_function_heads`] keeps
/// the ones that *are*. Every chain is exactly one of the two.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChainKind {
    /// A name reference — a chain not immediately followed by `(`.
    Reference,
    /// A function-call head — a chain immediately followed by `(`.
    FunctionHead,
}

/// Walk `expr` and collect the identifier chains of the requested [`ChainKind`],
/// in source order, skipping single-quoted (`''`-escaped) and `$tag$…$tag$`
/// literal content. The one place that knows the tokenizer's lexical structure;
/// both public scanners are thin polarity choices over it.
fn scan_chains(expr: &str, kind: ChainKind) -> Vec<IdentRef<'_>> {
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
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
            _ if b == b'"' || crate::util::is_ident_byte(b) => {
                let end = scan_chain(bytes, i);
                let is_head = followed_by_open_paren(bytes, end);
                let want = if is_head {
                    ChainKind::FunctionHead
                } else {
                    ChainKind::Reference
                };
                if want == kind {
                    out.push(IdentRef {
                        start: i,
                        end,
                        raw: &expr[i..end],
                    });
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

/// Scan `expr` and return every identifier chain that is a **function-call
/// head** — a chain immediately followed by `(` (skipping whitespace) — in
/// source order, skipping single-quoted and `$tag$…$tag$` literal content
/// exactly as [`scan_references`] does.
///
/// This is the exact complement of [`scan_references`]: every identifier chain
/// in an expression is classified as *either* a name reference (returned there)
/// *or* a call head (returned here), never both and never neither. It is the
/// engine primitive behind aggregate-function detection — matching the last
/// part of each head against a known-aggregate set — so that detection shares
/// the tokenizer's literal handling (a `sum(` inside `'…'` is not a call) and
/// its single identifier-byte rule (E-5: `Ωsum(` is one chain `Ωsum`, not the
/// aggregate `sum`).
pub(crate) fn scan_function_heads(expr: &str) -> Vec<IdentRef<'_>> {
    scan_chains(expr, ChainKind::FunctionHead)
}

/// Is the next non-whitespace byte at or after `pos` an opening `(`? Used to
/// classify an identifier chain as a function call rather than a name reference.
fn followed_by_open_paren(bytes: &[u8], pos: usize) -> bool {
    let mut j = pos;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    j < bytes.len() && bytes[j] == b'('
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

/// Byte length of the **first part** of a reference chain (`a` in `a.city`, the
/// whole `"C d"` in `"C d".x`, or the entire chain when it is bare). `chain` is
/// assumed well-formed as produced by [`scan_chain`] — it starts with a `"` or
/// an identifier byte. Used to isolate the leading qualifier for
/// [`rewrite_qualifier`] and to decide [`IdentRef::is_bare`].
fn first_part_len(chain: &str) -> usize {
    let bytes = chain.as_bytes();
    if bytes.first() == Some(&b'"') {
        skip_quoted_part(bytes, 0)
    } else {
        let mut i = 0;
        while i < bytes.len() && crate::util::is_ident_byte(bytes[i]) {
            i += 1;
        }
        i
    }
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
/// that forced the former `util::replace_word_boundary_pairs`'s single-pass
/// design (SG-3) cannot occur here, and there is no needle-ordering concern: each chain
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

/// Rewrite the leading **qualifier** (first part) of every reference chain in
/// `expr` whose first part matches `alias`, replacing just that part with
/// `replacement` and leaving the rest of the chain intact (`a.city` →
/// `<replacement>.city`; a bare `a` → `<replacement>`).
///
/// This is the role-playing alias rewrite (`expand::window` / `sql_gen` /
/// `semi_additive` rewrite a dimension's `source_table` alias to a scoped
/// alias). It matches the first part through [`crate::ident::normalize_ident_part`],
/// so a quoted or mixed-case qualifier (`"A".city`) is rewritten too, and —
/// unlike the retired quote-blind `util::replace_word_boundary` — it operates
/// only on genuine reference chains: text inside a `'…'` / `$tag$…$` literal is
/// never touched, a function-call head (`date(x)` when `alias == "date"`) is
/// left alone, and a *foreign* qualified tail (`x.a` when `alias == "a"`) does
/// not match because only the chain's own first part is compared.
pub(crate) fn rewrite_qualifier(expr: &str, alias: &str, replacement: &str) -> String {
    let want = crate::ident::normalize_ident_part(alias);
    let mut out = String::with_capacity(expr.len());
    let mut copied = 0;
    for r in scan_references(expr) {
        let head_len = first_part_len(r.raw);
        if crate::ident::normalize_ident_part(&r.raw[..head_len]) == want {
            out.push_str(&expr[copied..r.start]);
            out.push_str(replacement);
            out.push_str(&r.raw[head_len..]);
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
        // `SUM` is a function call, not a name reference — only its argument is.
        assert_eq!(keys("SUM(o.amount)"), vec!["o.amount"]);
        assert_eq!(keys("a.b.c"), vec!["a.b.c"]);
    }

    #[test]
    fn function_call_heads_are_not_references() {
        // A chain immediately before `(` (any whitespace) is a call, not a ref,
        // so a fact/metric named like a function is never confused with it.
        assert_eq!(keys("sum(x)"), vec!["x"]);
        // The numeric literal `0` scans as a harmless chain (it can never be a
        // declared name, so it never matches); the function head is excluded.
        assert_eq!(keys("COALESCE ( revenue , 0 )"), vec!["revenue", "0"]);
        assert_eq!(keys("schema.date_trunc('day', o.ts)"), vec!["o.ts"]);
        // FIND/INLINE both ignore the function name even if a name collides.
        assert!(!references_ref("SUM(x)", "sum", None));
        let mut m: HashMap<String, &str> = HashMap::new();
        m.insert("sum".to_string(), "(BAD)");
        assert_eq!(inline_references("SUM(net_price)", &m), "SUM(net_price)");
    }

    #[test]
    fn quoted_parts_fold_and_join() {
        // A quoted part normalizes exactly like an unquoted one (DuckDB rule).
        assert_eq!(keys("\"Revenue\""), vec!["revenue"]);
        assert_eq!(keys("a.\"C d\""), vec!["a.c d"]);
        assert_eq!(keys("SUM(\"Rev\")"), vec!["rev"]);
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

    // ----- is_bare / scan_function_heads / rewrite_qualifier -----

    #[test]
    fn is_bare_distinguishes_qualified_chains() {
        let bare: Vec<bool> = scan_references("revenue + o.amount + \"Rev\" + a.\"C d\"")
            .iter()
            .map(IdentRef::is_bare)
            .collect();
        // revenue (bare), o.amount (qualified), "Rev" (bare), a."C d" (qualified)
        assert_eq!(bare, vec![true, false, true, false]);
    }

    #[test]
    fn function_heads_are_the_complement_of_references() {
        let expr = "SUM(o.amount) + main.avg(x) - revenue";
        let heads: Vec<String> = scan_function_heads(expr)
            .iter()
            .map(IdentRef::key)
            .collect();
        assert_eq!(heads, vec!["sum", "main.avg"]);
        // The same chains never appear as references…
        let refs: Vec<String> = keys(expr);
        assert_eq!(refs, vec!["o.amount", "x", "revenue"]);
        // …and reference vs head spans are disjoint.
        let head_spans: Vec<(usize, usize)> = scan_function_heads(expr)
            .iter()
            .map(|r| (r.start, r.end))
            .collect();
        for r in scan_references(expr) {
            assert!(!head_spans.contains(&(r.start, r.end)));
        }
    }

    #[test]
    fn function_heads_skip_literal_content() {
        // A `sum(` inside a string / dollar literal is not a call head.
        assert!(scan_function_heads("'sum(x)'").is_empty());
        assert!(scan_function_heads("$$avg(y)$$").is_empty());
    }

    #[test]
    fn rewrite_qualifier_rewrites_only_the_head_part() {
        // Bare-alias qualifier before a dot is rewritten; the column part stays.
        assert_eq!(rewrite_qualifier("a.city", "a", "a__dep"), "a__dep.city");
        // Every own-qualified chain in the expr is rewritten.
        assert_eq!(
            rewrite_qualifier("a.city || ' from ' || a.country", "a", "a__dep"),
            "a__dep.city || ' from ' || a__dep.country"
        );
        // A bare occurrence of the alias is rewritten wholesale.
        assert_eq!(rewrite_qualifier("a", "a", "a__dep"), "a__dep");
    }

    #[test]
    fn rewrite_qualifier_is_case_and_quote_insensitive() {
        assert_eq!(rewrite_qualifier("A.city", "a", "a__dep"), "a__dep.city");
        assert_eq!(
            rewrite_qualifier("\"A\".city", "a", "a__dep"),
            "a__dep.city"
        );
    }

    #[test]
    fn rewrite_qualifier_leaves_literals_functions_and_foreign_tails_intact() {
        // The alias `a` inside a string literal must NOT be rewritten (the E-3
        // hazard that quote-blind replace_word_boundary had).
        assert_eq!(
            rewrite_qualifier("a.city || ' a '", "a", "a__dep"),
            "a__dep.city || ' a '"
        );
        // A function head named like the alias is not a qualifier.
        assert_eq!(
            rewrite_qualifier("a(o.x) + a.city", "a", "a__dep"),
            "a(o.x) + a__dep.city"
        );
        // A FOREIGN qualified tail `x.a` is not the alias `a` as a qualifier.
        assert_eq!(
            rewrite_qualifier("x.a + a.city", "a", "a__dep"),
            "x.a + a__dep.city"
        );
        // No match → unchanged.
        assert_eq!(rewrite_qualifier("b.city", "a", "a__dep"), "b.city");
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

        /// References and function-call heads partition all identifier chains:
        /// their spans are disjoint, and no span appears in both scans.
        #[test]
        fn references_and_heads_are_disjoint(expr in arb_expr()) {
            let refs: std::collections::HashSet<(usize, usize)> =
                scan_references(&expr).iter().map(|r| (r.start, r.end)).collect();
            let heads: std::collections::HashSet<(usize, usize)> =
                scan_function_heads(&expr).iter().map(|r| (r.start, r.end)).collect();
            prop_assert!(refs.is_disjoint(&heads), "overlap in {expr:?}");
        }

        /// `scan_function_heads` is total and yields spans that tile a subset of
        /// the input on char boundaries (same invariant as `scan_references`).
        #[test]
        fn function_heads_are_ordered_and_in_bounds(expr in arb_expr()) {
            let mut last_end = 0;
            for r in scan_function_heads(&expr) {
                prop_assert!(r.start >= last_end);
                prop_assert!(r.end <= expr.len());
                prop_assert!(expr.is_char_boundary(r.start));
                prop_assert!(expr.is_char_boundary(r.end));
                prop_assert_eq!(&expr[r.start..r.end], r.raw);
                last_end = r.end;
            }
        }

        /// Rewriting a qualifier that matches no chain head leaves the
        /// expression untouched, and rewriting is always total (never panics).
        #[test]
        fn rewrite_qualifier_is_total_and_identity_on_no_match(
            expr in arb_expr(),
            alias in "[a-z]{1,5}",
        ) {
            let out = rewrite_qualifier(&expr, &alias, "<Q>");
            // If no chain's first part equals `alias`, the output is unchanged.
            let matches = scan_references(&expr)
                .iter()
                .any(|r| crate::ident::normalize_ident_part(&r.raw[..first_part_len(r.raw)])
                    == crate::ident::normalize_ident_part(&alias));
            if !matches {
                prop_assert_eq!(out, expr);
            }
        }

        /// For a contiguous `head.tail`, rewriting the qualifier `head` rewrites
        /// exactly the head part and preserves the tail (E-3: the foreign tail
        /// `tail` is never itself treated as the qualifier).
        #[test]
        fn rewrite_qualifier_rewrites_head_preserves_tail(
            head in "[a-z]{1,4}",
            tail in "[a-z]{1,5}",
        ) {
            let expr = format!("{head}.{tail}");
            prop_assert_eq!(
                rewrite_qualifier(&expr, &head, "<Q>"),
                format!("<Q>.{tail}")
            );
            // Rewriting by the tail name changes nothing (it is not a qualifier)
            // — as long as the tail differs from the head part.
            prop_assume!(head != tail);
            prop_assert_eq!(rewrite_qualifier(&expr, &tail, "<Q>"), expr);
        }
    }
}
