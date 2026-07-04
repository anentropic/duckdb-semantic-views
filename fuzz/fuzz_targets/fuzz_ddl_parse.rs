#![no_main]
// Phase 64 seed inputs covering quoted-identifier forms live in:
//   fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_bare.txt
//   fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_fqn.txt
//   fuzz/seeds/fuzz_ddl_parse/seed_phase64_mixed_quoting.txt
// The same inputs are asserted as permanent regression tests in
// tests/quoted_idents_regression.rs (workspace-level cargo test).
use libfuzzer_sys::fuzz_target;
use semantic_views::parse::{detect_semantic_view_ddl, plan_rewrite, PARSE_DETECTED};

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
        let _ = plan_rewrite(query);
    }
});
