---
phase: 37-extract-shared-utilities
plan: 01
subsystem: refactoring
tags: [module-extraction, circular-dependency, rust-modules]

# Dependency graph
requires: []
provides:
  - "src/util.rs leaf module with suggest_closest, replace_word_boundary, is_word_boundary_char"
  - "src/errors.rs leaf module with ParseError struct"
  - "expand <-> graph circular dependency broken"
  - "parse <-> body_parser circular dependency broken"
affects: [38-expand-module-split, 39-metadata-storage, 40-show-alignment, 41-describe-rewrite]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Leaf module extraction pattern: shared types/functions moved to standalone modules with no intra-crate dependencies"]

key-files:
  created: [src/util.rs, src/errors.rs]
  modified: [src/lib.rs, src/expand.rs, src/graph.rs, src/parse.rs, src/body_parser.rs, src/query/table_function.rs, src/query/explain.rs, src/ddl/show_dims_for_metric.rs]

key-decisions:
  - "util.rs and errors.rs are true leaf modules with zero intra-crate imports (only strsim external dep in util.rs)"

patterns-established:
  - "Leaf module extraction: move shared functions/types to standalone modules that depend only on external crates"

requirements-completed: [REF-03, REF-04]

# Metrics
duration: 16min
completed: 2026-04-01
---

# Phase 37 Plan 01: Extract Shared Utilities Summary

**Two leaf modules (util.rs, errors.rs) extracted to break expand<->graph and parse<->body_parser circular dependencies with zero behavior changes across 482 tests**

## Performance

- **Duration:** 16 min
- **Started:** 2026-04-01T19:49:28Z
- **Completed:** 2026-04-01T20:05:56Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- Extracted suggest_closest, replace_word_boundary, is_word_boundary_char from expand.rs to new src/util.rs leaf module with 11 unit tests
- Extracted ParseError struct from parse.rs to new src/errors.rs leaf module
- Broke expand <-> graph circular dependency (graph.rs no longer imports from crate::expand)
- Broke parse <-> body_parser circular dependency (body_parser.rs no longer imports from crate::parse)
- All 482 Rust tests pass with zero behavior changes

## Task Commits

Each task was committed atomically:

1. **Task 1: Extract util.rs leaf module (REF-03)** - `6022f14` (refactor)
2. **Task 2: Extract errors.rs leaf module (REF-04)** - `f192be4` (refactor)

## Files Created/Modified
- `src/util.rs` - New leaf module: suggest_closest, replace_word_boundary, is_word_boundary_char + 11 tests
- `src/errors.rs` - New leaf module: ParseError struct
- `src/lib.rs` - Added pub mod util and pub mod errors declarations
- `src/expand.rs` - Removed 3 functions and 11 tests, added use crate::util import
- `src/graph.rs` - Changed import from crate::expand to crate::util
- `src/parse.rs` - Removed ParseError struct, added use crate::errors import
- `src/body_parser.rs` - Changed import from crate::parse to crate::errors
- `src/query/table_function.rs` - Split import: expand for expand/QueryRequest, util for suggest_closest
- `src/query/explain.rs` - Split import: expand for expand/QueryRequest, util for suggest_closest
- `src/ddl/show_dims_for_metric.rs` - Split import: expand for ancestors/collect, util for suggest_closest

## Decisions Made
- util.rs and errors.rs designed as true leaf modules with zero intra-crate dependencies (only strsim external dep in util.rs), making them safe foundation for Phase 38 module directory splits

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Worktree lacks build toolchain (extension-ci-tools submodule, Python venv) for `just test-all`. `cargo test` (482 tests) and `cargo check` verify correctness for this behavior-preserving refactoring. Sqllogictest and DuckLake CI exercise extension loading pipeline which is unaffected by internal import reorganization.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Both circular dependencies broken, enabling Phase 38 expand/ and graph/ module directory splits
- util.rs and errors.rs are stable leaf modules that Phase 38 modules can import without creating new cycles

## Self-Check: PASSED

- All created files exist (src/util.rs, src/errors.rs, 37-01-SUMMARY.md)
- All commits found (6022f14, f192be4)
- cargo check passes, cargo test passes (482 tests)

---
*Phase: 37-extract-shared-utilities*
*Completed: 2026-04-01*
