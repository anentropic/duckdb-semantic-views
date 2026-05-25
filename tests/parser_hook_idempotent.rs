//! Phase 65.1 Plan 12 Task 3 (WR-09 D-21 + B-07 plan-checker fix) — STRUCTURAL
//! verification of parser-hook idempotence.
//!
//! ## Why structural and not behavioural
//!
//! The plan's literal acceptance criterion asks for a Rust integration test
//! that calls `sv_register_parser_hooks` twice against the same
//! `duckdb_database` handle and then asserts `sv_count_parser_extensions`
//! returns 1. That symbol path is, however, only present in the binary when
//! the `extension` feature is enabled (build.rs only compiles
//! `cpp/src/shim.cpp` under `--features extension`). `cargo test --features
//! extension` does not help either — the `duckdb/loadable-extension`
//! feature replaces all `libduckdb-sys` C API calls with no-op stubs, so
//! `duckdb_open(":memory:")` returns false and the `duckdb_database`
//! handle we'd need to hand to `sv_register_parser_hooks` cannot be
//! obtained. This is the same constraint Plan 02b documented in
//! `tests/registration_error_surfaces.rs`.
//!
//! Per the precedent set by Plan 02b — and per the B-07 plan-checker
//! decision in 65.1-12-PLAN.md to expose the helper publicly (option (a))
//! rather than gate behind cfg(test) plumbing — this test takes the
//! **structural** path:
//!
//!   1. `cpp/src/shim.hpp` declares `int32_t sv_count_parser_extensions(...)`
//!      with the WR-02-style `(error_buf, error_buf_len)` trailing pair.
//!   2. `cpp/src/shim.cpp` implements the helper: iterates
//!      `DBConfig::GetCallbackManager().ParserExtensions()` and counts
//!      entries whose `parser_override == sv_parser_override`.
//!   3. The helper's docstring names the option-(a) decision rationale
//!      per the B-07 plan-checker fix.
//!   4. `cpp/src/shim.cpp::sv_register_parser_hooks` contains the
//!      idempotence check (`already_registered` flag + `for (auto &existing :
//!      cbmgr.ParserExtensions())` loop + function-pointer comparison).
//!   5. The dedup check guards the entire `ParserExtension` build + Register
//!      block AND the `SemanticViewsParserInfo` allocation (so the skip path
//!      does not leak a fresh parser_info shared_ptr).
//!
//! ## Behavioural coverage
//!
//! End-to-end behavioural coverage of the same surface comes from the Python
//! integration test `test/integration/test_load_extension_twice_idempotent.py`:
//! it actually loads the extension binary, calls `LOAD semantic_views` twice,
//! and exercises sentinel DDL through the parser_override hook after the
//! second LOAD. The two tests together provide both compile-time structural
//! evidence (this file) and runtime behavioural evidence (the Python test).
//!
//! Discrimination: on the pre-Task-1 binary (the dedup check absent), this
//! file's assertions 4 and 5 would fail because neither the `already_registered`
//! flag nor the `cbmgr.ParserExtensions()` iteration would appear inside
//! `sv_register_parser_hooks`. On the pre-Task-3 binary (the helper missing),
//! assertions 1, 2, and 3 would fail. Post-Task-1 + post-Task-3 (this PR),
//! every assertion passes — pinning the WR-09 fix structurally.

use std::fs;
use std::path::Path;

/// Locate the body of a top-level function or extern "C" function in a
/// pre-loaded source string. Walks parentheses to find the matching `)`
/// then advances to the next `{` and returns the half-open `(start, end)`
/// byte range of the body block (exclusive of the outer braces).
fn locate_fn_body<'a>(src: &'a str, header_needle: &str) -> &'a str {
    let fn_start = src
        .find(header_needle)
        .unwrap_or_else(|| panic!("could not locate `{header_needle}` in source"));
    let after_header = &src[fn_start..];

    // Find the matching `)` of the parameter list.
    let paren_open = after_header
        .find('(')
        .expect("function header must contain `(`");
    let mut depth: i32 = 0;
    let mut paren_close_rel: Option<usize> = None;
    for (i, ch) in after_header[paren_open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    paren_close_rel = Some(paren_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let paren_close = paren_close_rel.expect("matching `)` for header");

    // Find the next `{` after the closing paren and walk to its matching `}`.
    let body_brace_rel = after_header[paren_close..]
        .find('{')
        .expect("opening `{` of function body");
    let body_open = paren_close + body_brace_rel;

    let mut brace_depth: i32 = 0;
    let mut body_close_rel: Option<usize> = None;
    for (i, ch) in after_header[body_open..].char_indices() {
        match ch {
            '{' => brace_depth += 1,
            '}' => {
                brace_depth -= 1;
                if brace_depth == 0 {
                    body_close_rel = Some(body_open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let body_close = body_close_rel.expect("matching `}` for body");
    &after_header[body_open..=body_close]
}

#[test]
fn parser_hook_register_is_idempotent() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set under cargo");
    let shim_cpp_path = Path::new(&manifest_dir).join("cpp/src/shim.cpp");
    let shim_hpp_path = Path::new(&manifest_dir).join("cpp/src/shim.hpp");
    assert!(
        shim_cpp_path.exists(),
        "cpp/src/shim.cpp not found at {shim_cpp_path:?}"
    );
    assert!(
        shim_hpp_path.exists(),
        "cpp/src/shim.hpp not found at {shim_hpp_path:?}"
    );
    let shim_cpp = fs::read_to_string(&shim_cpp_path).expect("read cpp/src/shim.cpp");
    let shim_hpp = fs::read_to_string(&shim_hpp_path).expect("read cpp/src/shim.hpp");

    // -----------------------------------------------------------------------
    // Assertion 1: cpp/src/shim.hpp declares sv_count_parser_extensions
    //              with the WR-02-style (error_buf, error_buf_len) pair.
    // -----------------------------------------------------------------------
    assert!(
        shim_hpp.contains("sv_count_parser_extensions"),
        "Plan 12 Task 3 (B-07 fix) — cpp/src/shim.hpp must declare \
         `sv_count_parser_extensions` so the structural verification \
         path can name the helper. Option-(a) decision per the \
         plan-checker fix: the helper is exposed publicly rather than \
         gated behind cfg(test) plumbing."
    );
    // Locate the declaration and verify the ABI pair is present.
    let decl_idx = shim_hpp
        .find("sv_count_parser_extensions")
        .expect("already asserted present");
    // Pull a window around the declaration to inspect the signature.
    let decl_window_start = decl_idx.saturating_sub(200);
    let decl_window_end = (decl_idx + 400).min(shim_hpp.len());
    let decl_window = &shim_hpp[decl_window_start..decl_window_end];
    assert!(
        decl_window.contains("char *error_buf") && decl_window.contains("size_t error_buf_len"),
        "Plan 12 Task 3 — sv_count_parser_extensions declaration must \
         carry the WR-02-style `(char *error_buf, size_t error_buf_len)` \
         trailing pair so failures surface via the ABI-stable channel. \
         Declaration window: {decl_window}"
    );

    // -----------------------------------------------------------------------
    // Assertion 2: cpp/src/shim.cpp implements sv_count_parser_extensions
    //              iterating ParserExtensions() and counting matches.
    // -----------------------------------------------------------------------
    // Locate the implementation header (return type `int32_t`, function
    // name `sv_count_parser_extensions`).
    let impl_header = "int32_t sv_count_parser_extensions(";
    let impl_body = locate_fn_body(&shim_cpp, impl_header);
    assert!(
        impl_body.contains("ParserExtensions()"),
        "Plan 12 Task 3 — sv_count_parser_extensions body must iterate \
         `cbmgr.ParserExtensions()` to count entries. Body excerpt \
         (first 400 chars): {}",
        &impl_body[..impl_body.len().min(400)]
    );
    assert!(
        impl_body.contains("sv_parser_override"),
        "Plan 12 Task 3 — sv_count_parser_extensions body must compare \
         each entry's `parser_override` against the file-static \
         `sv_parser_override` symbol. Body excerpt (first 400 chars): {}",
        &impl_body[..impl_body.len().min(400)]
    );

    // -----------------------------------------------------------------------
    // Assertion 3: helper docstring names the option-(a) decision rationale
    //              per the B-07 plan-checker fix.
    // -----------------------------------------------------------------------
    // The docstring is in the .hpp (declaration site, the canonical
    // documentation location).
    let mention_option_a = shim_hpp.contains("option (a)")
        || shim_hpp.contains("option a)")
        || shim_hpp.contains("option(a)");
    let mention_b07 = shim_hpp.contains("B-07");
    assert!(
        mention_option_a && mention_b07,
        "Plan 12 Task 3 — `sv_count_parser_extensions`'s declaration in \
         cpp/src/shim.hpp must document the option-(a) choice (publicly \
         exposed helper rather than cfg(test)-gated) and cite the B-07 \
         plan-checker fix. mention_option_a={mention_option_a}, \
         mention_b07={mention_b07}"
    );

    // -----------------------------------------------------------------------
    // Assertion 4: sv_register_parser_hooks contains the dedup check.
    // -----------------------------------------------------------------------
    let register_body = locate_fn_body(&shim_cpp, "bool sv_register_parser_hooks(");
    assert!(
        register_body.contains("already_registered"),
        "Plan 12 Task 1 — sv_register_parser_hooks body must contain the \
         `already_registered` flag that controls the dedup-and-skip path \
         introduced by the WR-09 D-21 idempotence guard."
    );
    assert!(
        register_body.contains("cbmgr.ParserExtensions()")
            || register_body.contains("ParserExtensions()"),
        "Plan 12 Task 1 — sv_register_parser_hooks body must iterate \
         `cbmgr.ParserExtensions()` to detect an existing registration \
         of the sv_parser_override function pointer."
    );
    assert!(
        register_body.contains("existing.parser_override == sv_parser_override"),
        "Plan 12 Task 1 — sv_register_parser_hooks body must compare \
         each existing parser-extension entry's `parser_override` \
         function pointer against the file-static `sv_parser_override` \
         symbol to detect a duplicate registration."
    );

    // -----------------------------------------------------------------------
    // Assertion 5: dedup check guards both the ParserExtension build AND
    //              the SemanticViewsParserInfo allocation, so the skip path
    //              does not leak a fresh parser_info shared_ptr.
    //
    // We verify this structurally by checking that the
    // `SemanticViewsParserInfo` allocation appears AFTER the
    // `if (!already_registered)` guard within the function body.
    // -----------------------------------------------------------------------
    let guard_idx = register_body.find("if (!already_registered)").expect(
        "Plan 12 Task 1 — sv_register_parser_hooks must contain \
                 `if (!already_registered)` to gate the Register block",
    );
    let info_idx = register_body.find("new SemanticViewsParserInfo(").expect(
        "Plan 12 Task 1 — sv_register_parser_hooks must still \
                 allocate `SemanticViewsParserInfo` on the registration \
                 path; if you removed that allocation entirely the \
                 parser_override hook can no longer dispatch",
    );
    assert!(
        info_idx > guard_idx,
        "Plan 12 Task 1 — `new SemanticViewsParserInfo(...)` allocation \
         must appear AFTER the `if (!already_registered)` guard so the \
         skip path does not leak a fresh parser_info shared_ptr. \
         guard_idx={guard_idx}, info_idx={info_idx}"
    );

    // -----------------------------------------------------------------------
    // Assertion 6: cite of Phase 65.1 D-21 / WR-09 above the dedup loop.
    // -----------------------------------------------------------------------
    assert!(
        register_body.contains("D-21") || register_body.contains("WR-09"),
        "Plan 12 Task 1 — sv_register_parser_hooks should cite Phase 65.1 \
         D-21 and/or WR-09 in a comment block above the dedup check so a \
         future contributor reading the function can trace the decision."
    );
}
