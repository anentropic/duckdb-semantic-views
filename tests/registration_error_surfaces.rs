//! Phase 65.1 Plan 02b — WR-02 D-08/D-09 + CR-02 D-05.
//!
//! Verifies the error_buf ABI surfaces underlying registration failures.
//!
//! ## Verification strategy
//!
//! The plan's literal acceptance criterion asks for a Rust integration test
//! that invokes `sv_register_table_function` with `init_cb = None`, asserts
//! the returned `bool` is `false`, and asserts the supplied `error_buf`
//! contains the substring `init_cb is mandatory`. That symbol is, however,
//! only present in the binary when the `extension` feature is enabled
//! (build.rs only compiles `cpp/src/shim.cpp` under `--features extension`).
//! `cargo test --features extension` does not work either because the
//! `duckdb/loadable-extension` feature replaces all `libduckdb-sys` C API
//! calls with no-op stubs — `duckdb_open(":memory:")` returns false and the
//! `duckdb_database` handle we'd need to hand to `sv_register_table_function`
//! cannot be obtained. Dual-compiling `shim.cpp` under the default `bundled`
//! feature is structurally feasible but risks C++ symbol collisions with
//! `libduckdb-sys`'s bundled DuckDB amalgamation (which independently
//! compiles `duckdb.cpp` into a `duckdb` static lib) and would expand the
//! plan's scope to a build-system rework.
//!
//! Per Plan 02b's explicit fallback guidance ("adapt by gating with
//! `#[cfg(not(feature = "extension"))]` or by re-using a Rust-side helper
//! exposed in `src/lib.rs` for testing — discretion to whichever resolves
//! cleanly given the existing crate structure"), this test takes the
//! **structural** path established by `tests/no_long_lived_conn.rs`: it
//! parses `cpp/src/shim.cpp` and asserts both:
//!
//!   1. The body of `sv_register_table_function` contains the literal
//!      string `init_cb is mandatory` — D-05 refusal text reaches the
//!      `error_buf` channel.
//!   2. `sv_register_table_function`'s declaration carries the
//!      `(char *error_buf, size_t error_buf_len)` trailing pair — D-08/D-09
//!      ABI shape.
//!
//! Behavioural coverage of the same surface comes from `just test-sql`,
//! which builds the extension and exercises the `LOAD semantic_views`
//! registration sequence end-to-end through `init_extension` — any
//! registration failure surfaces through the new error_buf cascade that
//! Plan 02b landed in `src/lib.rs`.
//!
//! ## W-01 fix — typed stubs over the transmute footgun
//!
//! The original plan draft suggested using the `mem` module's `transmute`
//! intrinsic to coerce a no-arg `extern "C" fn` into the multi-arg
//! `bind_cb` / `exec_cb` signatures. That compiles but is a segfault hazard
//! if the D-05 null-init check is ever weakened. Because this test takes
//! the structural path, neither stubs nor that intrinsic are needed at
//! all — the W-01 footgun is eliminated by construction. The grep-based
//! invariant `grep -q "std" + "::mem::transmute" tests/registration_error_surfaces.rs`
//! (plan-checker spelling) returns no hits because the literal sequence
//! `std` `:` `:` `mem` `:` `:` `transmute` never appears as adjacent
//! tokens in this file — the words around them are split across this
//! comment and the runtime needle below is concatenated from parts.

use std::fs;
use std::path::Path;

#[test]
fn init_extension_surfaces_registration_error_buf() {
    // Locate cpp/src/shim.cpp relative to CARGO_MANIFEST_DIR so the test
    // runs from any working directory `cargo test` chooses.
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set under cargo");
    let shim_path = Path::new(&manifest_dir).join("cpp/src/shim.cpp");
    assert!(
        shim_path.exists(),
        "cpp/src/shim.cpp not found at {shim_path:?} — Plan 02b's structural assertion needs it",
    );
    let shim_src = fs::read_to_string(&shim_path).expect("read cpp/src/shim.cpp");

    // 1. D-05 refusal text: the literal `init_cb is mandatory` must appear
    //    inside the body of `sv_register_table_function`. Locate the
    //    function header, then scan its body for the substring.
    let fn_header = "bool sv_register_table_function(";
    let fn_start = shim_src.find(fn_header).unwrap_or_else(|| {
        panic!(
            "could not locate `{fn_header}` in cpp/src/shim.cpp — Plan 02a should have landed this signature",
        )
    });

    // Find the opening brace `{` that starts the body (after the
    // closing parenthesis of the parameter list). Walk forward from
    // fn_start, count parens to find the matching `)`, then the next `{`.
    let after_header = &shim_src[fn_start..];
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
    let paren_close = paren_close_rel.expect("matching `)` for sv_register_table_function header");
    let body_brace_rel = after_header[paren_close..]
        .find('{')
        .expect("opening `{` of sv_register_table_function body");
    let body_open = paren_close + body_brace_rel;

    // Find the matching `}` of the body.
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
    let body_close = body_close_rel.expect("matching `}` for sv_register_table_function body");
    let body = &after_header[body_open..=body_close];

    assert!(
        body.contains("init_cb is mandatory"),
        "Plan 02a D-05 refusal text missing — `sv_register_table_function` body must contain \
         the literal `init_cb is mandatory` so a null `init_cb` is rejected at registration time \
         and the message reaches the caller via `error_buf`. Body excerpt (first 400 chars): {}",
        body.get(..400).unwrap_or(body),
    );

    // 2. D-08/D-09 ABI shape: the header must include the trailing
    //    `(char *error_buf, size_t error_buf_len)` pair.
    let header_slice = &after_header[..=paren_close];
    assert!(
        header_slice.contains("char *error_buf") && header_slice.contains("size_t error_buf_len"),
        "Plan 02a D-08/D-09 ABI missing — `sv_register_table_function` signature must carry \
         trailing `(char *error_buf, size_t error_buf_len)` so failures surface via the \
         ABI-stable buffer. Header excerpt: {header_slice}",
    );

    // 3. W-01 invariant: this very file must not invoke the transmute
    //    intrinsic on the bind/exec callback function pointers. The
    //    structural test path naturally satisfies the invariant (it
    //    doesn't invoke any FFI at all).
    //
    //    WR-03 (Phase 68 review): the original needle only matched the
    //    single textual idiom `std::mem::transmute`; semantically
    //    equivalent forms (`core::mem::transmute`, unqualified
    //    `mem::transmute` after `use std::mem;`, the bare `transmute`
    //    intrinsic after `use std::mem::transmute;`) silently bypassed
    //    the guard. The needle set below covers all three qualifying
    //    paths. Needles are constructed at runtime via concatenation
    //    so this assertion source itself contains no adjacent token
    //    sequence that would re-trigger the guard, and the plan-checker
    //    `grep -q "std" + "::mem::transmute"` still returns zero hits.
    let self_src =
        fs::read_to_string(Path::new(&manifest_dir).join("tests/registration_error_surfaces.rs"))
            .expect("read tests/registration_error_surfaces.rs");
    // Needles cover the three qualifying paths to the FFI intrinsic:
    //   * `std::mem::transmute(...)` / `std::mem::transmute::<T,U>(...)`
    //   * `core::mem::transmute(...)` (Rust 2021+ re-exports `core::mem`)
    //   * unqualified `mem::transmute(...)` (after `use std::mem;`)
    // Each needle is built at runtime via array concat so this file's
    // source never contains an adjacent `std::mem::transmute` token
    // sequence in non-comment code — preserving the plan-checker
    // `grep -q "std" + "::mem::transmute"` zero-hits invariant.
    let needles: Vec<String> = vec![
        ["std::", "mem::", "transmute"].concat(),
        ["core::", "mem::", "transmute"].concat(),
        ["mem::", "transmute"].concat(),
    ];
    let offending: Vec<&str> = self_src
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| needles.iter().any(|n| line.contains(n.as_str())))
        .collect();
    assert!(
        offending.is_empty(),
        "Plan 02b W-01 / WR-03 invariant violated — this test must not invoke \
         the FFI intrinsic on callback function pointers (typed stubs only). \
         Matched any of: std::mem::*, core::mem::*, or bare mem::* qualified \
         path. Offending lines: {offending:?}",
    );
}
