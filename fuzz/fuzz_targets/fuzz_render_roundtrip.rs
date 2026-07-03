#![no_main]
// RT-3 (code-review 2026-07-02): machine-check for parse ↔ render drift.
//
// For an ARBITRARY `SemanticViewDefinition` the strict round-trip
// `parse(render(def)) == def` cannot hold (arbitrary field content need not
// be in the parser's canonical form — unbalanced quotes in expressions,
// annotation keywords at depth 0, etc.). The robust invariant is the
// FIXPOINT: whenever `render(def)` re-parses at all, rendering the re-parsed
// definition must reproduce the same DDL byte-for-byte:
//
//   render(parse(render(def))) == render(def)
//
// Any grammar drift between `render_ddl.rs` and the body parser shows up as
// either a re-parse failure on freshly rendered DDL (tolerated here — the
// exact-equality property for canonical definitions lives in
// tests/roundtrip_proptest.rs) or a fixpoint violation (asserted).
use libfuzzer_sys::fuzz_target;
use semantic_views::body_parser::parse_keyword_body;
use semantic_views::model::SemanticViewDefinition;
use semantic_views::render_ddl::render_create_ddl;

/// Strip the rendered header (`CREATE OR REPLACE SEMANTIC VIEW <name>
/// [COMMENT = '...'] `) and return the ` AS\n...` body suffix. Rendered
/// output always uses the exact ` AS\n` separator, and the quoted name /
/// comment ahead of it are located with the same quote-aware helpers the
/// parser uses, so a name containing " AS\n" cannot fool the split.
fn body_of(ddl: &str) -> Option<&str> {
    let rest = ddl.strip_prefix("CREATE OR REPLACE SEMANTIC VIEW ")?;
    let name_end = semantic_views::ident::find_identifier_end(rest, true);
    let mut after = &rest[name_end..];
    let trimmed = after.trim_start();
    if trimmed.len() >= 7 && trimmed.as_bytes()[..7].eq_ignore_ascii_case(b"COMMENT") {
        let after_kw = trimmed[7..].trim_start();
        let after_eq = after_kw.strip_prefix('=')?.trim_start();
        let (_, consumed) =
            semantic_views::util::extract_single_quoted_prefix(after_eq).ok()?;
        after = &after_eq[consumed..];
    }
    let trimmed = after.trim_start();
    if trimmed.len() >= 2 && trimmed.as_bytes()[..2].eq_ignore_ascii_case(b"AS") {
        Some(trimmed)
    } else {
        None
    }
}

fuzz_target!(|def: SemanticViewDefinition| {
    let Ok(rendered) = render_create_ddl("fuzz_view", &def) else {
        return; // legacy-format defs (empty tables) don't render
    };
    let Some(body) = body_of(&rendered) else {
        return;
    };
    let Ok(reparsed) = parse_keyword_body(body, 0) else {
        return; // non-canonical arbitrary content — exact property covered by proptest
    };
    let reparsed_def = SemanticViewDefinition {
        tables: reparsed.tables,
        joins: reparsed.relationships,
        facts: reparsed.facts,
        dimensions: reparsed.dimensions,
        metrics: reparsed.metrics,
        materializations: reparsed.materializations,
        ..Default::default()
    };
    let rerendered =
        render_create_ddl("fuzz_view", &reparsed_def).expect("re-parsed definition must render");
    let Some(body2) = body_of(&rerendered) else {
        panic!("re-rendered DDL lost its AS body: {rerendered}");
    };
    assert_eq!(
        body, body2,
        "render(parse(render(def))) != render(def) — grammar drift between \
         render_ddl and the body parser.\nfirst:\n{rendered}\nsecond:\n{rerendered}"
    );
});
