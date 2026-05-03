#![no_main]
//! D7 (v0.8.1): exercise the parser_override FFI entry path with arbitrary
//! byte slices, including invalid UTF-8 and embedded NULs. The goal is to
//! prove no panic and no undefined behaviour on any input.
//!
//! `sv_parser_override_rust` (the actual FFI symbol) is gated behind the
//! `extension` feature so it can't be called directly from this target,
//! but the work it performs on the Rust side is exactly:
//!   1. UTF-8 validate the input bytes (B2 hardening — was
//!      `from_utf8_unchecked` pre-v0.8.1).
//!   2. Dispatch to `rewrite_to_native_sql`.
//!   3. Either publish the rewritten SQL, defer (return code 2), or emit a
//!      `SELECT error('...')` synthesised statement (return code 0 with
//!      the embedded message — the FALLBACK workaround for DuckDB silently
//!      dropping `DISPLAY_EXTENSION_ERROR`).
//!
//! Step 1 is reproduced here verbatim. Step 2 is invoked when bytes decode
//! as UTF-8. Step 3 is exercised via `rewrite_to_native_sql` returning
//! `Err(...)`. We also test panic-resistance against pathological inputs
//! like embedded NUL bytes (which the C++ side passes through with their
//! length, not a NUL terminator).

use libfuzzer_sys::fuzz_target;
use semantic_views::parse::rewrite_to_native_sql;

fuzz_target!(|data: &[u8]| {
    // Step 1: matches the from_utf8 call inside sv_parser_override_rust.
    // Invalid UTF-8 is the "defer" path (rc=2 on the FFI side); from this
    // target's perspective it's a non-event — we just must not panic.
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };

    // Step 2: full rewrite pipeline. Must never panic for any input,
    // including queries with embedded NUL bytes, leading whitespace,
    // partial DDL prefixes, or arbitrary garbage.
    let _ = rewrite_to_native_sql(0, query);
});
