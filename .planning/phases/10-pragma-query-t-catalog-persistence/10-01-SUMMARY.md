---
phase: 10-pragma-query-t-catalog-persistence
plan: 01
subsystem: database
tags: [duckdb, cpp, rust, ffi, pragma, extension-loader]

requires:
  - phase: 08-cpp-shim-infrastructure
    provides: shim.cpp skeleton, C++/Rust build boundary, ExtensionLoader vendored headers

provides:
  - pragma_query_t callbacks for define_semantic_view_internal and drop_semantic_view_internal
  - semantic_views_pragma_define / _drop C functions for invoke path (separate connection, no deadlock)
  - Rust FFI declarations for both C functions in shim/mod.rs

affects: [10-02, 10-03, 11-create-semantic-view-ddl]

tech-stack:
  added: []
  patterns: [pragma_query_t callback pattern, separate-connection persist pattern, cfg-feature-gated FFI declarations]

key-files:
  created: []
  modified:
    - src/shim/shim.cpp
    - src/shim/shim.h
    - src/shim/mod.rs

key-decisions:
  - "shim.h includes duckdb.h at the top (before __cplusplus guard) so duckdb_connection type is available from plain C"
  - "mod.rs gates FFI declarations under #[cfg(feature = 'extension')] — cargo test (bundled) compiles cleanly without C++ shim"
  - "Kept duckdb/main/config.hpp and duckdb/parser/parser_extension.hpp includes from Phase 8 — Phase 11 will need them"

patterns-established:
  - "pragma_query_t: callback returns SQL string; DuckDB executes it in the caller's transaction (PERSIST-02 mechanism)"
  - "Separate-connection write: semantic_views_pragma_define/drop use a second duckdb_connection to avoid deadlock in invoke"
  - "cfg-gated FFI: #[cfg(feature = 'extension')] mod ffi ensures unit tests compile without shim.cpp object"

requirements-completed: [PERSIST-01, PERSIST-02]

duration: 15min
completed: 2026-03-01
---

# Plan 10-01: C++ pragma registration + Rust FFI declarations

**pragma_query_t callbacks registered via ExtensionLoader for transactional catalog persistence, plus separate-connection C functions for invoke path**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-03-01T00:00:00Z
- **Completed:** 2026-03-01T00:15:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Replaced Phase 8 no-op skeleton with real ExtensionLoader pragma registration
- Registered two PRAGMA callbacks (define/drop) using pragma_query_t — returned SQL executes in caller's transaction (PERSIST-02)
- Implemented two C functions (semantic_views_pragma_define/drop) for Rust invoke path using separate connection
- Added Rust FFI declarations in mod.rs gated on `#[cfg(feature = "extension")]`

## Task Commits

1. **Task 1: shim.cpp + shim.h** - `5adb790` (feat)
2. **Task 2: mod.rs FFI declarations** - `8bbed0b` (feat)

## Files Created/Modified
- `src/shim/shim.cpp` - Real pragma registration replacing Phase 8 skeleton + two C invoke functions
- `src/shim/shim.h` - Updated to declare semantic_views_pragma_define + semantic_views_pragma_drop
- `src/shim/mod.rs` - Rust FFI declarations for both C functions, gated on extension feature

## Decisions Made
- `shim.h` includes `duckdb.h` before the `__cplusplus` guard so `duckdb_connection` typedef is available from plain C
- FFI declarations in `mod.rs` are gated on `#[cfg(feature = "extension")]` matching the build.rs gate for shim.cpp compilation
- Phase 8 includes (`config.hpp`, `parser_extension.hpp`) retained — Phase 11 will need them

## Deviations from Plan
None - plan executed exactly as written.

## Issues Encountered
- Pre-existing `expand_proptest.rs` test failures from Phase 9 struct additions (`dim_type`/`granularity` fields) — these are unrelated to Phase 10 and `cargo test --lib` (64 tests) passes cleanly

## Next Phase Readiness
- Plan 10-02 can proceed: FFI declarations are in place for define.rs and drop.rs to call
- Wave 2: Replace sidecar writes in define/drop invoke with pragma FFI calls

---
*Phase: 10-pragma-query-t-catalog-persistence*
*Completed: 2026-03-01*
