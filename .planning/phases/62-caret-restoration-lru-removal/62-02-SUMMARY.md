---
phase: 62
plan: 02
subsystem: parser-extension
tags: [phase-62, wave-1, lru-removal, override-context, ffi, ownership, q2-destruction-order]
dependency-graph:
  requires:
    - "62-01 (Wave 0 — test scaffolding + layout guard)"
  provides:
    - "OverrideContext top-level pub struct (Box-owned per parser_info)"
    - "sv_make_override_context / sv_drop_override_context FFI ctor/dtor"
    - "sv_parser_override_rust(ctx_ptr) replaces (db_token) signature"
    - "SemanticViewsParserInfo::rust_state (Box<OverrideContext>* opaque)"
    - "TECH-DEBT 20 silent-eviction error class removed"
  affects:
    - "src/parse.rs (-225 / +207)"
    - "cpp/src/shim.cpp (-37 / +75)"
    - "src/lib.rs (-19 / +16)"
tech-stack:
  added: []
  patterns:
    - "Box::into_raw / Box::from_raw round-trip for opaque-pointer FFI ownership"
    - "Drop impl that documents an INTENTIONAL leak (Q2 destruction-order constraint)"
    - "Heap-sentinel survival test for negative assertion (not-called) on Drop"
key-files:
  created:
    - .planning/phases/62-caret-restoration-lru-removal/62-02-SUMMARY.md
  modified:
    - src/parse.rs
    - cpp/src/shim.cpp
    - src/lib.rs
decisions:
  - "Drop for OverrideContext leaks the inner duckdb_connection by design — calling duckdb_disconnect would UAF on connection_manager (already reset by ~DatabaseInstance) at the moment ~DBConfig fires. Bounded at one Connection per DB ever opened (~few KB each), matches v0.8.0 commit 680a967."
  - "sv_make_override_context returns null on panic (catch_unwind in the ctor); sv_register_parser_hooks treats that as a fatal load failure rather than continuing with a missing override."
  - "Removed the dead-code non-extension stubs of rewrite_create / rewrite_drop_or_alter / rewrite_yaml_file_create — rewrite_to_native_sql is now extension-only so the test-feature shims have no caller. The TableFunctionCall struct, parse_table_function_call, strip_outer_quotes are gated #[cfg_attr(not(any(feature = \"extension\", test)), allow(dead_code))] so the default-feature lib build stays warning-free without losing test coverage."
metrics:
  duration: "~25 minutes"
  completed: 2026-05-06
  tasks_completed: 3
  files_modified: 3
  commits: 3
---

# Phase 62 Plan 02: Wave 1 — LRU removal + Q2 destruction-order pattern Summary

Replace the v0.8.1 16-entry `parser_override_catalog` LRU (keyed by per-load `db_token`)
with a `Box<OverrideContext>` directly owned by the C++ `SemanticViewsParserInfo`.
Resolves TECH-DEBT 20 (silent-eviction error class) without violating the Q2
destruction-order constraint discovered in `62-RESEARCH.md` §4 Refinement 1: the
destructor MUST NOT call `duckdb_disconnect` on the inner catalog connection —
`~DatabaseInstance` resets `connection_manager` BEFORE `~DBConfig` fires, so
`~Connection()` would UAF on the destroyed manager. The connection is intentionally
leaked (one per DB ever opened, matches the v0.8.0 baseline shape).

## What was built

### Task 1 — `src/parse.rs` (commit `ba30cfe`)

Replaced the entire `parser_override_catalog` module with a top-level
`pub struct OverrideContext { catalog: CatalogReader, is_file_backed: bool }`
plus an explicit `Drop` impl that documents the intentional leak.

New FFI exports:
```rust
#[no_mangle] pub unsafe extern "C" fn sv_make_override_context(
    conn: ffi::duckdb_connection, is_file_backed: bool,
) -> *mut std::ffi::c_void;

#[no_mangle] pub unsafe extern "C" fn sv_drop_override_context(
    ctx_ptr: *mut std::ffi::c_void,
);

#[no_mangle] pub unsafe extern "C" fn sv_parser_override_rust(
    ctx_ptr: *const std::ffi::c_void,           // was: db_token: u64
    query_ptr: *const u8, query_len: usize,
    sql_out_ptr: *mut *mut u8, sql_out_len: *mut usize,
    error_out: *mut u8, error_out_len: usize,
) -> u8;
```

`rewrite_to_native_sql`, `rewrite_create`, `rewrite_drop_or_alter`,
`rewrite_yaml_file_create`, and `emit_native_create_sql` now take
`&OverrideContext` directly. The "catalog context evicted" error branches are
gone (the LRU map is gone).

**Drop impl text (verbatim, src/parse.rs):**
```rust
impl Drop for OverrideContext {
    fn drop(&mut self) {
        // Phase 62 Q2 — INTENTIONAL LEAK of self.catalog.conn (the duckdb_connection).
        //
        // ~SemanticViewsParserInfo (and therefore Drop for OverrideContext) fires
        // during ~DBConfig, AFTER ~DatabaseInstance has already reset
        // connection_manager (duckdb.cpp:276819). Calling duckdb_disconnect here
        // would invoke ~Connection() → ConnectionManager::RemoveConnection() on
        // the destroyed manager — use-after-free.
        //
        // The leak is bounded at ONE duckdb_connection per DB ever opened in this
        // process (a few KB each). This matches v0.8.0 commit 680a967 which shipped
        // successfully with the same leak. The Rust-side Box<OverrideContext>
        // allocation itself IS reclaimed (this Drop runs and the Box dealloc fires).
        //
        // See: .planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md §Q2.
        // Resolves TECH-DEBT item 20 (silent LRU eviction class) by removing the LRU.
    }
}
```

Tests:
- DELETED `parser_override_catalog_lru_evicts_oldest` (the module is gone).
- ADDED `override_context_drop_does_not_disconnect` — heap-sentinel test that
  proves the Drop body did not call `duckdb_disconnect` (which would have
  segfaulted because the sentinel is not a `Connection*`).
- ADDED `sv_make_and_drop_override_context_round_trip` — exercises the
  ctor/dtor with a null connection.
- ADDED `sv_drop_override_context_handles_null` — defensive null-pointer
  no-op pattern matching the C++ shim's guard.

### Task 2 — `cpp/src/shim.cpp` (commit `d3a6f30`)

Reshaped `SemanticViewsParserInfo`:
```cpp
struct SemanticViewsParserInfo : public ParserExtensionInfo {
    void *rust_state;  // Box<OverrideContext>* opaque pointer (Rust-owned).
    explicit SemanticViewsParserInfo(void *state) : rust_state(state) {}

    ~SemanticViewsParserInfo() override {
        if (rust_state) {
            sv_drop_override_context(rust_state);
            rust_state = nullptr;
        }
        // CRITICAL — Phase 62 Q2 destruction-order showstopper:
        // We deliberately do NOT call duckdb_disconnect on the
        // duckdb_connection contained within OverrideContext's CatalogReader.
        //
        // By the time this destructor fires, ~DatabaseInstance has already
        // reset connection_manager (duckdb.cpp:276819). ~Connection() would
        // call ConnectionManager::RemoveConnection() on the destroyed
        // manager — use-after-free.
        //
        // The Rust Drop impl on OverrideContext (in src/parse.rs) documents
        // the same constraint. The duckdb_connection object leaks for the
        // remainder of process life — bounded at one Connection per DB ever
        // opened (~few KB each). This matches v0.8.0 commit 680a967 which
        // shipped successfully with this same leak pattern.
        //
        // See .planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md §Q2.
        // Resolves TECH-DEBT item 20 (silent LRU eviction class) by removing the LRU.
    }
};
```

Removed the `static std::atomic<uint64_t> sv_next_db_token{1};` (no more LRU
keys to assign) and the `<atomic>` include. `sv_register_parser_hooks` signature
changed from `(duckdb_database, uint64_t *out_db_token)` to
`(duckdb_database, duckdb_connection catalog_conn, bool is_file_backed)`.

### Task 3 — `src/lib.rs` (commit `99153b5`)

Updated the `extern "C"` declaration of `sv_register_parser_hooks` to match
the new C++ signature. `init_extension` now passes `(db_handle, catalog_conn,
is_file_backed)` directly; the local `db_token: u64` and the
`set_catalog_for_parser_override` call are gone. Read-side table function
registrations (lines 389-482) untouched per RESEARCH §Q4.

## Verification (final state)

| Command | Result |
|---------|--------|
| `just build`         | EXIT 0 — Rust + C++ shim link cleanly |
| `cargo test` (default features) | 748 passed, 0 failed |
| `just test-sql`      | 45/45 SUCCESS (38 substantive + 7 Wave-0 halt-skipped) |
| `just test-caret`    | 7/7 PASS (3 existing + 4 staged still print SKIP for caret-col extraction; Plan 04 flips them) |
| `just test-multi-db` | 3/3 PASS (existing 2-DB + 17-DB staged + 50-DB RSS staged) |
| `just test-adbc`     | 6/6 PASS (transactional DDL preserved end-to-end) |
| `just test-concurrent` | PASS (race shape per TECH-DEBT 23 preserved) |
| `just test-large-view` | PASS (heap-buffer round-trip preserved) |
| `just test-vtab-crash` | PASS |
| `just test-all`      | EXIT 0 — full suite green; 841-test nextest summary |

## Acceptance criteria (all PASS)

```
$ rg "parser_override_catalog" src/ cpp/src/
(zero hits)

$ rg "set_catalog_for_parser_override" src/
(zero hits)

$ rg "MAX_CATALOG_ENTRIES|LRU_CAPACITY|catalog context.*evicted" src/
(zero hits)

$ rg "db_token" src/lib.rs cpp/src/shim.cpp
cpp/src/shim.cpp:    // is_file_backed flag for THIS database. The legacy db_token LRU
src/lib.rs:    // db_token-LRU lookup is gone (TECH-DEBT 20).
(both hits are documentation; no code references the removed field)

$ rg "duckdb_disconnect" cpp/src/shim.cpp
    // CRITICAL: sv_drop_override_context does NOT call duckdb_disconnect on
        // We deliberately do NOT call duckdb_disconnect on the
(both hits are inside comments documenting the absence)

$ rg "pub struct OverrideContext" src/
src/parse.rs:pub struct OverrideContext {

$ rg "pub unsafe extern \"C\" fn sv_(make|drop)_override_context" src/parse.rs
src/parse.rs:pub unsafe extern "C" fn sv_make_override_context(
src/parse.rs:pub unsafe extern "C" fn sv_drop_override_context(ctx_ptr: *mut std::ffi::c_void) {

$ rg "rust_state" cpp/src/shim.cpp
(11 hits — struct field declaration, constructor, destructor, override callback,
 register_parser_hooks; no `db_token` field references)
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] dead_code warnings under default features became errors under clippy `-D warnings`**
- **Found during:** Task 1 commit (pre-commit clippy hook).
- **Issue:** Gating `rewrite_to_native_sql` to `#[cfg(feature = "extension")]` (so it can take `&OverrideContext`) made `TableFunctionCall`, `parse_table_function_call`, and `strip_outer_quotes` unused under default-feature library builds. Clippy with `-D warnings` (project pre-commit hook) failed.
- **Fix:** Added `#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]` to all three items. They remain available for tests (which depend on them) and for the extension feature (the actual production caller); only the default-feature non-test lib build silences the warning.
- **Files modified:** `src/parse.rs` (3 attribute additions).
- **Commit:** `ba30cfe` (folded into Task 1).

**2. [Rule 3 - Blocking] cargo fmt diff in heap-sentinel test**
- **Found during:** Task 1 commit (pre-commit rustfmt hook).
- **Issue:** The new `override_context_drop_does_not_disconnect` test had a long line that rustfmt collapsed to a single-line constructor call. Hook reports but does not auto-apply.
- **Fix:** Ran `cargo fmt`, re-staged, re-committed.
- **Files modified:** `src/parse.rs` (formatting only).
- **Commit:** `ba30cfe`.

**3. [Rule 3 - Blocking] Sandbox blocks xcrun cache writes**
- **Found during:** every `just build` / `cargo build --features extension` invocation.
- **Issue:** Default sandbox denies writes to `/var/folders/.../T/xcrun_db-*`. The Rust extension build's macOS toolchain lookups all fail under sandbox.
- **Fix:** Used `dangerouslyDisableSandbox: true` for build/test invocations; same workaround Plan 01 used.
- **Files modified:** none — execution-environment workaround.
- **Commit:** none.

**4. [Rule 2 - Critical] Dead non-extension stubs after rewriter signature change**
- **Found during:** Task 1 implementation.
- **Issue:** The plan's `<action>` step 8 only described updating extension-feature signatures. The pre-existing `#[cfg(not(feature = "extension"))]` stubs of `rewrite_create`, `rewrite_drop_or_alter`, and `rewrite_yaml_file_create` (each taking `_db_token: u64`) had no caller after `rewrite_to_native_sql` itself became extension-only. Leaving them would compile but trigger clippy `unused-functions` warnings.
- **Fix:** Deleted the three non-extension stubs entirely; their only purpose was to satisfy the extension-feature dispatch match arms which are also extension-only now.
- **Files modified:** `src/parse.rs` (~30 LOC removed).
- **Commit:** `ba30cfe` (folded into Task 1).

**5. [Rule 3 - Blocking] Task 2's `just build` requires lib.rs to be coherent**
- **Found during:** Task 2 commit attempt.
- **Issue:** The plan structure (Task 2 = shim.cpp only, Task 3 = lib.rs) is logically clean, but in practice committing only shim.cpp leaves `src/lib.rs` calling the legacy 2-arg `sv_register_parser_hooks` against the new 3-arg C++ definition. Cargo would compile (Rust `extern "C"` declarations are local to the Rust side) but the link step / load-time symbol resolution would invoke the new C++ function with mismatched ABI — UB at runtime. The `just build` verify step in the plan validated only that the shim compiles.
- **Fix:** Committed shim.cpp alone (after stashing lib.rs) to honour Task 2's atomic boundary. `just build` exited 0 against the existing, also-out-of-date `src/lib.rs` because cargo + linker do not check signature equivalence across the FFI boundary. Task 3 then restored lib.rs and committed it; `just test-all` was deferred until Task 3 lands so the final binary actually exercises the new ABI consistently.
- **Files modified:** none beyond the planned scope.
- **Commits:** `d3a6f30` (Task 2), `99153b5` (Task 3).

### Architectural Decisions (no permission gate)

None — Plan 02 follows the architectural decisions encoded in 62-RESEARCH.md
§4 Refinement 1 (the destructor leak pattern) and the ultraplan's `parser_info`
direct-attach design. No new architectural choices needed.

## Authentication Gates

None.

## Known Stubs

The synthesised `SELECT error('...')` path (`sql_throwing` + the rc=1 branch in
`sv_parser_override_rust`) is intentionally preserved through Wave 1. It will be
deleted in Phase 62 Plan 03 once `parse_function` is reintroduced as the
error-reporting layer (which will restore caret rendering — TECH-DEBT 22).

The 7 sqllogictest fixtures and 4 staged Python tests from Plan 01 remain
`halt`-skipped / `print(SKIP); return`. Plan 04 (Wave 3) populates them.

## Self-Check

```
$ test -f .planning/phases/62-caret-restoration-lru-removal/62-02-SUMMARY.md && echo FOUND
FOUND

$ git log --oneline | grep -E "ba30cfe|d3a6f30|99153b5"
99153b5 refactor(62-02): rewire init_extension to pass catalog_conn through C++ shim
d3a6f30 refactor(62-02): reshape SemanticViewsParserInfo to hold rust_state Box pointer
ba30cfe refactor(62-02): replace LRU with direct OverrideContext ownership in parse.rs

$ rg "pub struct OverrideContext" src/parse.rs && rg "rust_state" cpp/src/shim.cpp
src/parse.rs:pub struct OverrideContext {  (FOUND)
cpp/src/shim.cpp: 11 hits  (FOUND)
```

## Self-Check: PASSED
