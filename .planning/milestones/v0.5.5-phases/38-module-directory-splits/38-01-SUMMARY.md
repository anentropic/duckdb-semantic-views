---
phase: 38-module-directory-splits
plan: 01
subsystem: refactoring
tags: [rust, module-split, expand, code-organization]

# Dependency graph
requires:
  - phase: 37-extract-shared-utilities
    provides: util.rs and errors.rs extracted from expand.rs (reduced from 4556 to 4299 lines)
provides:
  - src/expand/ module directory with 7 single-responsibility submodules
  - mod.rs re-exports preserving exact prior public API surface
  - 86 tests distributed to their correct submodule files
affects: [38-02-parse-module-split, expand-module-consumers]

# Tech tracking
tech-stack:
  added: []
  patterns: [module-directory-with-reexports, pub-super-visibility-for-cross-submodule-access]

key-files:
  created:
    - src/expand/mod.rs
    - src/expand/types.rs
    - src/expand/resolution.rs
    - src/expand/facts.rs
    - src/expand/fan_trap.rs
    - src/expand/role_playing.rs
    - src/expand/join_resolver.rs
    - src/expand/sql_gen.rs
    - src/expand/test_helpers.rs
  modified: []

key-decisions:
  - "pub(super) for cross-submodule function access within expand module"
  - "No make_def shared test helper created -- each test module has its own local builders"
  - "Tests in phase29/phase30 use crate::expand::facts:: imports for pub(super) functions"

patterns-established:
  - "Module directory pattern: mod.rs re-exports public API, submodules use pub(super) for internal sharing"
  - "Test distribution pattern: unit tests go with their function's submodule, end-to-end tests go with the entry point (sql_gen.rs)"

requirements-completed: [REF-01, REF-05]

# Metrics
duration: 32min
completed: 2026-04-01
---

# Phase 38 Plan 01: Split expand.rs into Module Directory Summary

**Split expand.rs (4,299 lines) into 7 single-responsibility submodules with 86 tests distributed to correct locations, zero behavior changes**

## Performance

- **Duration:** 32 min
- **Started:** 2026-04-01T21:17:22Z
- **Completed:** 2026-04-01T21:49:22Z
- **Tasks:** 2
- **Files modified:** 10 (1 deleted, 9 created)

## Accomplishments
- Decomposed monolithic expand.rs into 7 focused submodules: types, resolution, facts, fan_trap, role_playing, join_resolver, sql_gen
- mod.rs re-exports preserve exact prior public API -- no external consumer changes needed
- All 86 expand tests pass in their new submodule locations
- Full test suite (479 tests across all targets) passes with zero behavior changes

## Task Commits

Each task was committed atomically:

1. **Task 1: Create expand/ production code submodules and mod.rs** - `0729cd8` (refactor)
2. **Task 2: Distribute expand tests and create test_helpers.rs** - `bdfd727` (test)

## Files Created/Modified
- `src/expand/mod.rs` - Module declarations and public API re-exports
- `src/expand/types.rs` - QueryRequest struct, ExpandError enum, Display impl
- `src/expand/resolution.rs` - quote_ident, quote_table_ref, find_dimension, find_metric + quote tests
- `src/expand/facts.rs` - Fact inlining, toposort, derived metric collection
- `src/expand/fan_trap.rs` - Fan trap detection, ancestors_to_root
- `src/expand/role_playing.rs` - Role-playing dimension resolution, USING context
- `src/expand/join_resolver.rs` - PK/FK join synthesis and resolution
- `src/expand/sql_gen.rs` - Main expand() entry point + all end-to-end tests
- `src/expand/test_helpers.rs` - Placeholder for future shared test utilities
- `src/expand.rs` - DELETED (replaced by module directory)

## Decisions Made
- Used `pub(super)` visibility for functions that were private in the monolith but need cross-submodule access (e.g., find_dimension, toposort_facts, check_fan_traps)
- No shared `make_def()` test helper was created -- the original code had no such shared helper; each test module has its own local definition builders
- Phase 29/30 tests that directly call `toposort_facts`, `inline_facts`, `inline_derived_metrics` access them via `crate::expand::facts::` path since those are `pub(super)` (visible within the expand module tree)
- `test_helpers.rs` created as placeholder with cfg(test) gate in mod.rs

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Plan described non-existent make_def() helper**
- **Found during:** Task 2
- **Issue:** Plan specified extracting a `make_def()` shared helper function that does not exist in the original expand.rs code
- **Fix:** Created test_helpers.rs as a minimal placeholder instead of fabricating a function
- **Files modified:** src/expand/test_helpers.rs
- **Verification:** All 86 tests pass using their original local helper functions
- **Committed in:** bdfd727

---

**Total deviations:** 1 auto-fixed (1 plan inaccuracy)
**Impact on plan:** Minimal -- plan described a function that didn't exist; tests work correctly with their existing local helpers.

## Issues Encountered
- Two cargo warnings about "unused imports" for `pub(crate) use facts::collect_derived_metric_source_tables` and `pub(crate) use fan_trap::ancestors_to_root` in mod.rs -- these are expected because the consumers (ddl/show_dims_for_metric.rs) are behind `#[cfg(feature = "extension")]` which is not active during `cargo check`. The re-exports are correct and needed when building the extension.

## Known Stubs
None -- all code is production-ready, no placeholders in production paths.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- expand/ module directory is complete, ready for phase 38-02 (parse module split)
- Pattern established: module directory with mod.rs re-exports and pub(super) for internal access
- No blockers

## Self-Check: PASSED

- All 9 created files exist
- Both task commits found (0729cd8, bdfd727)
- src/expand.rs confirmed deleted
- 86 expand tests pass
- Full test suite passes (479 tests)

---
*Phase: 38-module-directory-splits*
*Completed: 2026-04-01*
