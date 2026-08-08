use semantic_views::body_parser::parse_keyword_body;
use semantic_views::model::{Metric, SemanticViewDefinition, TableRef};
use semantic_views::render_ddl::render_create_ddl;

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

/// The exact input CI's `fuzz_render_roundtrip` crashed on, replayed through
/// the same stages the target runs.
#[test]
fn ci_crash_input_no_longer_breaks_the_fixpoint() {
    let def = SemanticViewDefinition {
        tables: vec![TableRef {
            alias: "emanti\0\0\0\0".into(),
            table: "ew ".into(),
            pk_columns: vec!["end trur".into()],
            unique_constraints: vec![vec![]],
            comment: Some(String::new()),
            synonyms: vec![],
        }],
        metrics: vec![
            Metric {
                name: "PRIVATE".into(),
                expr: " Regit0$ lRRRRRRRRRRR".into(),
                output_type: Some(String::new()),
                using_relationships: vec![String::new()],
                comment: Some("gio".into()),
                ..Default::default()
            },
            Metric {
                name: "m ?a Regit0,(lR]RRRRRRRRRR".into(),
                expr: String::new(),
                output_type: Some(String::new()),
                using_relationships: vec![String::new()],
                comment: Some("gio".into()),
                ..Default::default()
            },
        ],
        database_name: Some(" ".into()),
        schema_name: Some("a Regit0, lRRRRRRRR".into()),
        ..Default::default()
    };

    let rendered0 = render_create_ddl("fuzz_view", &def).expect("render0");
    let body0 = body_of(&rendered0).expect("body0");

    // TC-14 (code-review 2026-08-08): this used to be
    // `let Ok(kb1) = ... else { return; }` — a silent escape. The input is
    // FIXED, so "the parser happened to reject it" is not a case to skip past:
    // the assertion below evaporates with no signal, the RT-5 fuzz-oracle
    // shape. Making it explicit immediately showed the escape was live rather
    // than defensive — the body does NOT parse today.
    //
    // What matters here is the invariant the CI crash was about: a
    // round-tripped definition must never yield a metric with no name. Both
    // outcomes satisfy it, and both are now pinned, so neither can change
    // unobserved:
    //
    //   Ok  — every parsed metric carries a name (the original assertion).
    //   Err — the parser refuses LOUDLY, and for the one reason we know of.
    //         The metric here is named `PRIVATE`; `render_create_ddl`
    //         quote-protects names on lexing grounds only, so it emits it bare
    //         and the parser peels entry-initial `PRIVATE` as the access
    //         modifier. That is review finding RT-9 (2026-08-08), still open:
    //         the arm below is pinning a *known-degraded* state, not a
    //         contract. When RT-9 is fixed this test flips to the Ok arm on
    //         its own and this arm becomes dead — delete it then.
    match parse_keyword_body(body0, 0) {
        Ok(kb1) => {
            for m in &kb1.metrics {
                assert!(
                    !m.name.trim().is_empty(),
                    "parser produced a nameless metric: {kb1:#?}"
                );
            }
        }
        Err(e) => {
            assert!(
                e.message.contains("Missing metric name"),
                "the only rejection we accept for this fixed body is RT-9's \
                 bare-`PRIVATE`-metric-name one; any other parse failure means \
                 this replay stopped exercising what it was written for.\n\
                 got: {e:?}\nrendered body: {body0}"
            );
        }
    }
}
