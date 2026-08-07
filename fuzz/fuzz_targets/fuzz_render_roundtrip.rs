#![no_main]
// RT-3 (code-review 2026-07-02): machine-check for parse ↔ render drift.
//
// An ARBITRARY `SemanticViewDefinition` is NOT in the image of the parser: a
// metric `expr` may carry surrounding whitespace the parser trims (`" "` →
// `""`), an alias may be a NUL-bearing byte string no lexer emits, a stored
// column may hold a bare depth-0 comma. So the strong fixpoint
// `render(parse(render(def))) == render(def)` is UNSATISFIABLE on arbitrary
// defs — a free-form `expr` cannot be quote-protected, and the parser will
// always re-normalize it (fuzz_render_roundtrip, 2026-07-18).
//
// The satisfiable, meaningful invariant is IDEMPOTENCE OF `render` ON A
// PARSER-PRODUCED def. We normalize once — render the arbitrary def and parse
// it back to land on a def the parser could actually produce — then assert that
// re-rendering that def is a fixpoint:
//
//   d1 = parse(render(def))                 // parser-produced (canonical)
//   render(parse(render(d1))) == render(d1) // render is idempotent on d1
//
// Genuine grammar drift between `render_ddl.rs` and the body parser (a dropped
// field, a reordered clause, a mis-quoted special identifier) still breaks this.
// A re-parse failure at either stage is tolerated — the strict
// `parse(render(def)) == def` equality for CANONICAL defs lives in
// tests/roundtrip_proptest.rs, which backstops the "canonical def renders to
// parseable, identical DDL" property this target deliberately does not re-assert.
use libfuzzer_sys::fuzz_target;
use semantic_views::body_parser::{parse_keyword_body, KeywordBody};
use semantic_views::model::{validate_ddl_representable, SemanticViewDefinition};
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
        let (_, consumed) = semantic_views::util::extract_single_quoted_prefix(after_eq).ok()?;
        after = &after_eq[consumed..];
    }
    let trimmed = after.trim_start();
    if trimmed.len() >= 2 && trimmed.as_bytes()[..2].eq_ignore_ascii_case(b"AS") {
        Some(trimmed)
    } else {
        None
    }
}

/// Assemble the subset of a parsed [`KeywordBody`] that `render_create_ddl`
/// consumes into a `SemanticViewDefinition`.
fn kb_to_def(kb: KeywordBody) -> SemanticViewDefinition {
    SemanticViewDefinition {
        tables: kb.tables,
        joins: kb.relationships,
        facts: kb.facts,
        dimensions: kb.dimensions,
        metrics: kb.metrics,
        materializations: kb.materializations,
        ..Default::default()
    }
}

fuzz_target!(|def: SemanticViewDefinition| {
    // --- Precondition, applied UP FRONT ---
    //
    // RT-5 made this rule explicit but consulted it LAZILY — only in the
    // stage-1 parse-failure branch below. That was inconsistent, and the
    // inconsistency had teeth: when stage 1 happened to parse, the target went
    // on to assert against a chain rooted in a definition no entry point can
    // store, and reported the resulting stage-2 failure as render/parse drift.
    // (Observed 2026-08-07: an input whose `pk_columns` contained `""`, which
    // the precondition already rejects, reached the stage-2 panic.)
    //
    // A definition that fails `validate_ddl_representable` cannot be stored by
    // any real entry point — the DDL grammar rejects it, and YAML import now
    // rejects it too — so nothing downstream of it is a contract this project
    // owes anyone. Skipping it is a stated rule; skipping it CONSISTENTLY is
    // what makes the two assertions below mean what they say.
    if validate_ddl_representable(&def).is_err() {
        return;
    }

    // --- Normalize once into the parser's image ---
    let Ok(rendered0) = render_create_ddl("fuzz_view", &def) else {
        return; // legacy-format defs (empty tables) don't render
    };
    let Some(body0) = body_of(&rendered0) else {
        return;
    };
    // RT-5: this used to be an unconditional escape — "arbitrary content the
    // parser can't accept". That is what let the GET_DDL contract break exactly
    // where the oracle stopped looking: `Arbitrary` produced `name: None` /
    // `source_table: None`, render emitted DDL this parser rejects, and the
    // target classified its own finding as unreachable input and stayed green.
    // With the precondition above already applied, a parse failure here is a
    // genuine contract break, with no escape left.
    let kb1 = parse_keyword_body(body0, 0).unwrap_or_else(|e| {
        panic!(
            "render produced DDL the parser rejects, for a definition every entry \
             point would accept: {}\n{rendered0}",
            e.message
        )
    });
    let d1 = kb_to_def(kb1); // parser-produced (canonical)

    // --- Assert render is idempotent on the parser-produced def ---
    let rendered1 =
        render_create_ddl("fuzz_view", &d1).expect("parser-produced definition must render");
    let Some(body1) = body_of(&rendered1) else {
        panic!("rendered DDL lost its AS body: {rendered1}");
    };
    // RT-5: no escape here at all. `d1` came FROM the parser, so
    // `render(d1)` failing to re-parse is a contract break by definition. The
    // old comment deferred to `tests/roundtrip_proptest.rs` — whose generators
    // pin `name: Some(...)` and `source_table: Some(...)`, so it structurally
    // could not reach these shapes. Each half pointed at the other.
    let kb2 = parse_keyword_body(body1, 0).unwrap_or_else(|e| {
        panic!(
            "freshly-rendered canonical DDL no longer re-parses: {}\n{rendered1}",
            e.message
        )
    });
    let d2 = kb_to_def(kb2);
    let rendered2 = render_create_ddl("fuzz_view", &d2).expect("re-parsed definition must render");
    let Some(body2) = body_of(&rendered2) else {
        panic!("re-rendered DDL lost its AS body: {rendered2}");
    };
    assert_eq!(
        body1, body2,
        "render is not idempotent on a parser-produced definition — grammar drift \
         between render_ddl and the body parser.\nfirst:\n{rendered1}\nsecond:\n{rendered2}"
    );
});
