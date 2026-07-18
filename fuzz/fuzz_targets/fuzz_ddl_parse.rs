#![no_main]
// Phase 64 seed inputs covering quoted-identifier forms live in:
//   fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_bare.txt
//   fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_fqn.txt
//   fuzz/seeds/fuzz_ddl_parse/seed_phase64_mixed_quoting.txt
// The same inputs are asserted as permanent regression tests in
// tests/quoted_idents_regression.rs (workspace-level cargo test).
use libfuzzer_sys::fuzz_target;
use semantic_views::parse::{
    detect_semantic_view_ddl, plan_rewrite, RewriteAction, PARSE_DETECTED,
};
use semantic_views::render_ddl::render_create_ddl;

fuzz_target!(|data: &[u8]| {
    // Reject invalid UTF-8 — not a crash, just out of scope for this target.
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };

    // detect_semantic_view_ddl must never panic regardless of input.
    let detected = detect_semantic_view_ddl(query);

    // If detected as our DDL, plan_rewrite must also never panic. Its result
    // (a structured RewriteAction, None, or ParseError) is all acceptable —
    // only a panic would be a fuzz failure.
    if detected == PARSE_DETECTED {
        if let Ok(Some(RewriteAction::Create { name, def, .. })) = plan_rewrite(query) {
            // Render oracle: a definition the CREATE front door accepted must
            // reconstruct to DDL. `render_create_ddl`'s ONLY error is the
            // documented legacy case of an empty `tables` list (reachable via
            // `FROM YAML $$...$$`), so any OTHER failure is a real render bug.
            match render_create_ddl(&name, &def) {
                Ok(_) => {}
                Err(_) => assert!(
                    def.tables.is_empty(),
                    "render_create_ddl failed on a non-legacy definition plan_rewrite accepted"
                ),
            }
        }
    }
});
