//! FFI entry points for the semantic-view `parser_override` path (AR-1).
//!
//! These are the C-ABI symbols the C++ shim (`cpp/src/shim.cpp`) links
//! against: the parser-override hook (`sv_parser_override_rust`), the
//! parse-function validation hook (`sv_parse_function_rust`), and the
//! output-buffer reclaimer (`sv_free_buffer`). The
//! `run_validation_for_parse_function` bridge and the `publish_owned_sql`
//! helper live here too.
//!
//! AR-7: the empty `OverrideContext` lifecycle
//! (`sv_make_override_context` / `sv_drop_override_context` + the opaque
//! `ctx_ptr` threaded through the two hooks) was retired. It became a pure
//! no-op after Phase 65 Plan 06 moved the catalog pre-checks into the
//! emitted SQL, so the struct, its allocator/deallocator, and the
//! `SemanticViewsParserInfo::rust_state` round-trip carried no state.
//!
//! All of this is `unsafe` C-boundary plumbing, isolated from the pure parse
//! logic in the sibling modules. The hooks delegate to `plan_rewrite`
//! (syntax) and `rewrite_to_native_sql` (catalog-aware emission); buffer
//! handling goes through `crate::ffi_util`. `sv_free_buffer` is re-exported
//! from the parent module so `crate::parse::*` paths (and cross-module tests)
//! resolve unchanged.

#[cfg(feature = "extension")]
use super::rewrite_to_native_sql;
#[cfg(any(feature = "extension", test))]
use super::{detect_ddl_kind, detect_near_miss};
// `plan_rewrite` only backs the syntax-only `run_validation_for_parse_function`
// used in non-extension unit tests; the extension build routes through
// `rewrite_to_native_sql` instead, so importing it there is unused.
#[cfg(all(not(feature = "extension"), test))]
use super::plan_rewrite;
#[cfg(any(feature = "extension", test))]
use crate::errors::ParseError;

// ---------------------------------------------------------------------------
// FFI entry points (extension feature-gated)
// ---------------------------------------------------------------------------
//
// AR-7 (was Phase 65 Plan 06): the `OverrideContext` struct and its
// `sv_make_override_context` / `sv_drop_override_context` allocator pair are
// gone. Pre-Plan-06 the context carried a `CatalogReader` over a long-lived
// `duckdb_connection` used for `catalog.exists()` pre-checks; Plan 06 replaced
// those with pure-SQL guards (`SELECT CASE WHEN [NOT] EXISTS THEN error(...)
// ELSE TRUE END; <DML>`) that run on the caller's connection in the same
// transaction. The struct then held no state, so the hooks no longer take an
// opaque `ctx_ptr` and the C++ `SemanticViewsParserInfo` no longer round-trips
// a `rust_state` pointer.

// The error writer, buffer leak/reclaim, and publish helpers all live in
// `crate::ffi_util` (ST-4 consolidation) — this module used to carry its
// own copies, which was how the FF-5 truncation divergence happened.
// Convention: `write_error_to_buffer` only for short, bounded strings
// (error messages); unboundedly large outputs (rewritten SQL) go through
// `publish_owned_string` + `sv_free_buffer` — silently truncating SQL
// produced confusing downstream parser errors (v0.8.0 buffer-truncation
// fix).
#[cfg(any(feature = "extension", test))]
use crate::ffi_util::write_error_to_buffer;

#[cfg(feature = "extension")]
use crate::ffi_util::reclaim_c_buffer;

/// FFI export: free a heap buffer produced by an earlier
/// `sv_parser_override_rust` success return.
///
/// Safe to call with a null pointer (no-op).
///
/// # Safety
///
/// `ptr`/`len` must be the exact pair the Rust side returned via its
/// `sql_out_ptr` / `sql_out_len` out-parameters. Calling with any other
/// pair (or twice on the same pair) is undefined behaviour.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_free_buffer(ptr: *mut u8, len: usize) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reclaim_c_buffer(ptr, len);
    }));
}

/// Internal helper: publish an owned `String` to the FFI out-parameters.
/// Both-or-drop contract (see [`crate::ffi_util::publish_owned_bytes`]).
///
/// # Safety
///
/// Either both `sql_out_ptr` and `sql_out_len` must point to writable
/// `*mut u8` / `usize` slots, or both must be null. Mixed null is treated
/// as "drop and skip writing."
#[cfg(feature = "extension")]
unsafe fn publish_owned_sql(sql: String, sql_out_ptr: *mut *mut u8, sql_out_len: *mut usize) {
    crate::ffi_util::publish_owned_string(sql, sql_out_ptr, sql_out_len);
}

/// FFI entry point for `parser_override`. The sole DDL entry point for the
/// extension as of v0.8.0 — the legacy `parse_function/parse_stub` path was
/// retired. Rewrites recognized semantic-view DDL into native SQL suitable
/// for re-parsing through `DuckDB`'s own parser and execution on the caller's
/// connection.
///
/// Returns:
///   0 = success: heap-owned native SQL pointer + length written to
///       `*sql_out_ptr` / `*sql_out_len`. Caller takes ownership and must
///       release via `sv_free_buffer`. The buffer is **not** NUL-terminated;
///       read exactly `*sql_out_len` bytes.
///   1 = validation error / near-miss suggestion: error message written to
///       `error_out`. (Currently unused under `FALLBACK_OVERRIDE`; kept for
///       Phase 62 Plan 03 once `parse_function` returns to caret rendering.)
///   2 = not ours: defer to default parser. Used both for genuinely
///       non-semantic SQL and for the early-return on null/empty input or
///       invalid UTF-8.
///
/// # Safety
///
/// - `query_ptr` must point to bytes of length `query_len` (validated as
///   UTF-8 here; invalid UTF-8 returns 2 rather than triggering UB).
/// - `sql_out_ptr` must point to a writable `*mut u8` slot, or be null.
/// - `sql_out_len` must point to a writable `usize` slot, or be null.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_parser_override_rust(
    query_ptr: *const u8,
    query_len: usize,
    sql_out_ptr: *mut *mut u8,
    sql_out_len: *mut usize,
    error_out: *mut u8,
    error_out_len: usize,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if query_ptr.is_null() || query_len == 0 {
            return 2_u8; // not ours
        }
        // Reject invalid UTF-8 cleanly rather than relying on
        // from_utf8_unchecked (B2 hardening). DuckDB query strings are
        // UTF-8 by spec but a malformed input must not trigger UB.
        let bytes = std::slice::from_raw_parts(query_ptr, query_len);
        let Ok(query) = std::str::from_utf8(bytes) else {
            return 2; // not ours — defer
        };

        match rewrite_to_native_sql(query) {
            Ok(Some(sql)) => {
                publish_owned_sql(sql, sql_out_ptr, sql_out_len);
                0 // success — native SQL handed to caller
            }
            Ok(None) => {
                // Genuinely not ours — defer to the default parser. If the
                // input is a near-miss for one of our DDL prefixes (e.g.
                // `CRETAE SEMANTIC VIEW`), `parse_function` (registered
                // alongside `parser_override` from Phase 62 Plan 03 onward)
                // will pick this up after the default parser fails on the
                // unrecognised prefix and re-render the suggestion via
                // DISPLAY_EXTENSION_ERROR with caret position.
                let _ = (error_out, error_out_len); // unused under Phase 62
                2 // not ours, defer to default parser
            }
            Err(err) => {
                // Phase 62: defer to default parser → `sv_parse_stub`
                // (registered as `parse_function`) re-runs validation and
                // returns DISPLAY_EXTENSION_ERROR with caret position. The
                // synthesised `SELECT error('...')` workaround used in
                // v0.8.0 (sql_throwing) has been deleted now that DuckDB's
                // ParserException::SyntaxError caret rendering is reachable
                // again via the parse_function code path. Resolves
                // TECH-DEBT 22.
                //
                // Phase 65.1 WR-06: preserve the original Err message in
                // `error_out` even though the C++ side currently ignores
                // the rc=2 channel. `rewrite_to_native_sql` can fail in
                // non-deterministic ways (e.g. JSON serialisation, the
                // metadata-via-SQL now() rendering, the unreachable
                // 'internal error' dispatch paths). If the second
                // invocation under sv_parse_stub produces a different
                // error than this one — because catalog state changed
                // mid-call, or a transient panic-caught failure
                // reproduces differently — the user otherwise loses any
                // trace of the first failure. The buffer is heap-allocated
                // and the cost is sub-microsecond; keeping the channel
                // populated lets future tooling (debug-build flag,
                // tracing hook) surface both errors without another ABI
                // change. Safety: error_out_len is the caller's declared
                // capacity; write_error_to_buffer respects it and
                // truncates at a char boundary.
                if !error_out.is_null() && error_out_len > 0 {
                    write_error_to_buffer(error_out, error_out_len, &err.message);
                }
                2
            }
        }
    }));

    result.unwrap_or(2) // on panic: not ours
}

/// FFI entry point for `parse_function` — Phase 62's error-reporting layer.
///
/// Called by `DuckDB`'s `Parser::ParseQuery` after the default parser fails on
/// an unrecognised prefix (e.g. `CREATE SEMANTIC VIEW …` or `CRETAE …`).
/// Re-runs validation against the user's input and returns the validation
/// error message + a byte-offset position so `DuckDB`'s
/// `ParserException::SyntaxError` can render `LINE 1: … ^` (caret) at the
/// offending token.
///
/// Return code (`u8`):
///   * `0` — success / unreachable. `parser_override` should have produced
///     rewritten SQL on the success path; if validation succeeds AND we
///     reach `parse_function`, the override didn't fire. We map this to
///     rc=3 in practice; rc=0 is the defensive "internal error" case.
///   * `1` — recognised prefix, but body is invalid OR a near-miss
///     (`CRETAE` etc.) suggestion was produced. `error_out` gets the
///     message; `position_out` gets the byte offset (or `u32::MAX` if no
///     position is available).
///   * `2` — not ours; defer (`DISPLAY_ORIGINAL_ERROR` on the C++ side).
///   * `3` — valid DDL but `parser_override` didn't fire (override setting
///     is `DEFAULT` or `STRICT`, e.g. after `CALL disable_peg_parser()`
///     reset the setting). `error_out` gets an actionable hint
///     (`SET allow_parser_override_extension='FALLBACK'`); `position_out=0`
///     so the caret lands on the `C` of `CREATE` / `D` of `DROP`.
///
/// The extension build re-runs the full rewrite (`rewrite_to_native_sql`); unit
/// tests (no `extension` feature) fall back to syntax-only validation via
/// `plan_rewrite`. Either way the purpose is to recover the `ParseError` message
/// + caret position for a *structurally invalid* DDL statement (e.g. a malformed
/// CREATE body), reproducing exactly what `parser_override` saw. Catalog-level
/// conditions — DROP-of-missing, rename-into-an-existing-name — are NOT
/// rewrite-time errors: `rewrite_drop` / `rewrite_alter_*` emit execution-time
/// SQL guards, so a well-formed-but-catalog-invalid statement rewrites
/// successfully (`Ok(Some)`) and its error surfaces when the emitted SQL runs,
/// not here (such a statement maps to rc=3 if `parser_override` is inactive).
///
/// # Safety
///
/// - `query_ptr` must point to bytes of length `query_len`. Invalid UTF-8
///   makes us return rc=2 (defer) rather than triggering UB.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes,
///   or be null. Null is treated as "do not write the message" (rc still
///   computed correctly).
/// - `position_out` must point to a writable `u32`, or be null. Null is
///   treated as "do not write the position".
#[cfg(any(feature = "extension", test))]
#[no_mangle]
pub unsafe extern "C" fn sv_parse_function_rust(
    query_ptr: *const u8,
    query_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
    position_out: *mut u32,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Initialise position_out to UINT32_MAX (no-position sentinel).
        if !position_out.is_null() {
            *position_out = u32::MAX;
        }

        // UTF-8 check; defer rather than synthesise an error on bad bytes.
        if query_ptr.is_null() || query_len == 0 {
            return 2_u8;
        }
        let bytes = std::slice::from_raw_parts(query_ptr, query_len);
        let Ok(query) = std::str::from_utf8(bytes) else {
            return 2_u8;
        };

        // Recognised DDL prefix?
        if detect_ddl_kind(query).is_none() {
            // Not a recognised prefix — try near-miss detection so the
            // user sees `Did you mean CREATE SEMANTIC VIEW?` instead of
            // a generic default-parser syntax error.
            if let Some(err) = detect_near_miss(query) {
                write_error_to_buffer(error_out, error_out_len, &err.message);
                if !position_out.is_null() {
                    *position_out = err
                        .position
                        .and_then(|p| u32::try_from(p).ok())
                        .unwrap_or(u32::MAX);
                }
                return 1_u8;
            }
            return 2_u8; // genuinely not ours
        }

        // Recognised prefix — re-run validation to recover the ParseError
        // wording + caret for a structurally-invalid statement, exactly as
        // parser_override saw it. Catalog-level conditions (DROP-of-missing,
        // rename collision) are execution-time SQL guards, not rewrite-time
        // errors, so they rewrite to Ok(Some) here and are not reproduced.
        // Unit tests fall back to syntax-only validation (see the two cfg'd
        // definitions below).
        let result = run_validation_for_parse_function(query);

        match result {
            Ok(Some(_rewritten)) => {
                // Valid DDL but we got here — `parser_override` must not have
                // fired. Most common cause: `disable_peg_parser` reset
                // `allow_parser_override_extension` to DEFAULT (TECH-DEBT 21).
                // Position 0 puts the caret on the `C` of CREATE / `D` of
                // DROP / etc.
                let msg = "semantic_views: parser_override is not active for \
                           this connection (allow_parser_override_extension is \
                           'DEFAULT' or 'STRICT'). Re-enable with: \
                           SET allow_parser_override_extension='FALLBACK';";
                write_error_to_buffer(error_out, error_out_len, msg);
                if !position_out.is_null() {
                    *position_out = 0;
                }
                3_u8
            }
            Ok(None) => {
                // detect_ddl_kind matched but validate returned None —
                // unreachable for a matched prefix. Defensive.
                write_error_to_buffer(
                    error_out,
                    error_out_len,
                    "semantic_views: internal error — recognised DDL prefix \
                     produced no rewrite (please report this bug)",
                );
                1_u8
            }
            Err(parse_err) => {
                write_error_to_buffer(error_out, error_out_len, &parse_err.message);
                if !position_out.is_null() {
                    *position_out = parse_err
                        .position
                        .and_then(|p| u32::try_from(p).ok())
                        .unwrap_or(u32::MAX);
                }
                1_u8
            }
        }
    }));

    result.unwrap_or(2) // on panic: not ours
}

/// Re-run validation for the `parse_function` path. Mirrors what
/// `sv_parser_override_rust` did at parse time (`rewrite_to_native_sql`), so a
/// structurally-invalid DDL statement surfaces the same `ParseError` wording +
/// position that `parser_override` produced. Catalog-level errors (DROP-of-missing,
/// rename collision) are enforced by guards in the *emitted* SQL at execution
/// time, so they are not reproduced here — such statements return `Ok(Some(_))`.
///
/// Returning `Ok(Some(_))` means "validation succeeded" — at the
/// `parse_function` call site this can only happen when `parser_override`
/// itself didn't run, so the caller maps it to rc=3 (actionable hint).
#[cfg(feature = "extension")]
fn run_validation_for_parse_function(query: &str) -> Result<Option<String>, ParseError> {
    rewrite_to_native_sql(query)
}

/// Test-only sibling of `run_validation_for_parse_function` — pure syntax
/// validation. Under `cargo test` the `extension` feature is OFF (default
/// features = bundled), so `rewrite_to_native_sql` is unavailable; only the
/// `Ok(Some)/Ok(None)/Err` shape matters (the caller discards the string).
#[cfg(all(not(feature = "extension"), test))]
fn run_validation_for_parse_function(query: &str) -> Result<Option<String>, ParseError> {
    plan_rewrite(query).map(|opt| opt.map(|_| String::new()))
}
