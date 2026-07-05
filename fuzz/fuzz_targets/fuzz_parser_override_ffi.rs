#![no_main]
//! D7: exercise the parser_override Rust entry path with arbitrary
//! byte slices, including invalid UTF-8 and embedded NULs. The goal is to
//! prove no panic and no undefined behaviour on any input.
//!
//! `sv_parser_override_rust` (the actual FFI symbol) is gated behind the
//! `extension` feature so it can't be called directly from this target.
//! `rewrite_to_native_sql` is also `extension`-gated and its catalog-aware
//! emission cannot be exercised meaningfully without a live DuckDB.
//!
//! What we DO fuzz here is the syntax-level Rust validation pipeline,
//! which is what executes inside `parser_override` BEFORE any catalog
//! access. The work performed is:
//!   1. UTF-8 validate the input bytes (was `from_utf8_unchecked` pre-v0.8.0;
//!      hardened to checked `from_utf8` for B2).
//!   2. Dispatch to `plan_rewrite` — the same entry point that
//!      `rewrite_to_native_sql` invokes for syntactic validation before
//!      consulting the catalog.
//!   3. Either return rewritten SQL (DDL accepted) or a `ParseError`
//!      (well-formed message + optional position).
//!
//! Step 1 is reproduced here verbatim. Step 2 is invoked when bytes decode
//! as UTF-8. We also test panic-resistance against pathological inputs
//! like embedded NUL bytes (which the C++ side passes through with their
//! length, not a NUL terminator) and leading-whitespace / comment edge
//! cases. Phase 62's caret rendering is not exercised here (it lives on
//! the C++ side, after parse_function publishes a position).

use libfuzzer_sys::fuzz_target;
use semantic_views::parse::plan_rewrite;

fuzz_target!(|data: &[u8]| {
    // Step 1: matches the from_utf8 call inside sv_parser_override_rust.
    // Invalid UTF-8 is the "defer" path (rc=2 on the FFI side); from this
    // target's perspective it's a non-event — we just must not panic.
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };

    // Step 2: syntax-only validation pipeline. Must never panic for any
    // input, including queries with embedded NUL bytes, leading
    // whitespace, partial DDL prefixes, or arbitrary garbage.
    let _ = plan_rewrite(query);
});
