---
phase: quick-14
plan: 01
subsystem: ffi
tags: [rust, ffi, c++, dead-code, cleanup]

# Dependency graph
requires:
  - phase: 21
    provides: sv_validate_ddl_rust and sv_rewrite_ddl_rust as canonical FFI entry points
provides:
  - Clean FFI surface with only 2 active entry points (sv_validate_ddl_rust, sv_rewrite_ddl_rust)
  - No orphaned backward-compat wrappers in parse.rs
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "No backward-compat wrappers -- tests call canonical function names directly"

key-files:
  created: []
  modified:
    - src/parse.rs
    - cpp/src/shim.cpp

key-decisions:
  - "Migrated legacy test cases to call canonical functions rather than deleting test coverage"

patterns-established:
  - "FFI surface documented in module doc comment -- update when adding/removing entry points"

requirements-completed: [CLEANUP-01]

# Metrics
duration: 15min
completed: 2026-03-09
---

# Quick Task 14: Remove 3 Orphaned Backward-Compat FFI Exports Summary

**Removed 2 dead FFI exports (sv_parse_rust, sv_execute_ddl_rust), 2 backward-compat wrappers, and updated module docs to reflect the 2-entrypoint FFI surface**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-09T17:53:50Z
- **Completed:** 2026-03-09T18:09:45Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Removed sv_parse_rust FFI export (replaced by sv_validate_ddl_rust in Phase 21)
- Removed sv_execute_ddl_rust FFI export (DDL execution moved to C++ side in Phase 20)
- Removed stale sv_parse_rust extern declaration from shim.cpp
- Removed detect_create_semantic_view and rewrite_ddl_to_function_call backward-compat wrappers
- Migrated all legacy test cases to call canonical function names (detect_semantic_view_ddl, rewrite_ddl)
- Updated module doc comment to accurately list only sv_validate_ddl_rust and sv_rewrite_ddl_rust

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove orphaned FFI exports and C++ declaration** - `cf7faf2` (fix)
2. **Task 2: Remove backward-compat wrappers, migrate tests, fix doc comment** - `338f998` (refactor)

## Files Created/Modified
- `src/parse.rs` - Removed sv_parse_rust FFI, sv_execute_ddl_rust FFI, detect_create_semantic_view wrapper, rewrite_ddl_to_function_call wrapper; migrated tests; updated module doc
- `cpp/src/shim.cpp` - Removed stale sv_parse_rust extern declaration

## Decisions Made
- Migrated legacy test cases to canonical function names rather than deleting them, preserving test coverage

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- FFI surface is clean with 2 entry points matching C++ declarations
- No follow-up work needed

## Self-Check: PASSED

- All modified files exist on disk
- Both task commits verified (cf7faf2, 338f998)
- Zero grep hits for removed symbols in src/ and cpp/
- `just test-all` passes (cargo test + sqllogictest + ducklake CI)

---
*Phase: quick-14*
*Completed: 2026-03-09*
