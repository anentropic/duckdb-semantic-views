//! Phase 65.1 Wave 0 STUB — populated by Plan 02 (WR-02 D-08/D-09).
//!
//! WR-02 (registration error swallowing): the C++ helpers
//! `sv_register_table_function` and `sv_register_scalar_function`
//! currently log failures to stderr only — ADBC/JDBC/Python callers
//! that have redirected stderr never see the underlying DuckDB
//! exception message (`init_extension` returns a bare "false" with no
//! context). D-08 reworks the C ABI to accept the canonical
//! `(char *error_buf, size_t error_buf_len)` trailing pair already used
//! by `sv_parser_override_rust` and the 17 read-side dispatchers; D-09
//! keeps stderr as a debug-only side channel — the error buffer is the
//! single ABI-stable surface.
//!
//! Plan 02 will replace the body of
//! `init_extension_surfaces_registration_error_buf` with the real
//! force-fail trigger. The most reliable shape (per `65.1-PATTERNS.md`
//! "no analog found" guidance) is to call `sv_register_table_function`
//! directly with deliberately bad arguments — once D-05 lands (null-
//! init_cb refusal at registration time) passing `init_cb = std::ptr::null_mut()`
//! will trip the refusal branch and the error buffer should contain
//! the substring `"null required argument"`.
//!
//! Pattern matches `tests/no_long_lived_conn.rs`: top-level
//! `tests/*.rs` integration test with no `mod` wrapper; `#[test]`
//! items live at file scope. This stub must compile and pass as a
//! no-op so the file is picked up by `cargo test` discovery — Plan 02
//! adds `use libduckdb_sys as ffi;` and the actual force-fail call.

#[test]
fn init_extension_surfaces_registration_error_buf() {
    eprintln!("STUB: populated by Plan 02 (WR-02 D-08/D-09)");
    /* deliberately empty — pass */
}
