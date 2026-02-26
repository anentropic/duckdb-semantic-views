---
phase: 06-tech-debt-cleanup
plan: "01"
subsystem: infra
tags: [dead-code, feature-gates, test-portability, idempotent-tests]

# Dependency graph
requires:
  - phase: 04-query-interface
    provides: "table_function.rs with SemanticViewBindData and infer_schema_or_default"
  - phase: 02-storage-and-ddl
    provides: "catalog.rs sidecar tests and phase2_ddl.test"
provides:
  - "Clean table_function.rs with no dead code or #[allow(dead_code)] annotations"
  - "Consistent feature-gating for all extension-only modules (ddl and query)"
  - "Portable catalog sidecar tests using std::env::temp_dir()"
  - "Idempotent SQLLogicTest restart section"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "std::env::temp_dir() for all test temp files (not hardcoded /tmp/)"
    - "CASE-based conditional cleanup in SQLLogicTest for idempotent re-runs"

key-files:
  created: []
  modified:
    - "src/query/table_function.rs"
    - "src/lib.rs"
    - "src/catalog.rs"
    - "test/sql/phase2_ddl.test"

key-decisions:
  - "infer_schema_or_default returns Vec<String> only; types discarded at caller via _types (try_infer_schema unchanged)"
  - "sidecar_path_derivation test left unchanged (pure function test with hardcoded string inputs, not file I/O)"
  - "CASE-based conditional drop in SQLLogicTest section 10 (DuckDB evaluates CASE lazily)"

patterns-established:
  - "temp_dir pattern: all Rust tests creating temp files use std::env::temp_dir() for sandbox portability"

requirements-completed: []

# Metrics
duration: 3min
completed: 2026-02-26
---

# Phase 06 Plan 01: Tech Debt Cleanup Summary

**Removed dead code from table_function.rs, added feature-gate to query module, and made catalog tests portable with std::env::temp_dir()**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-26T12:49:51Z
- **Completed:** 2026-02-26T12:52:18Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Eliminated all `#[allow(dead_code)]` annotations from table_function.rs by removing the unused `column_type_ids` field and `logical_type_from_duckdb_type` function
- Feature-gated `pub mod query` in lib.rs consistent with `pub mod ddl`, ensuring clean compilation under default features
- Made 3 catalog sidecar tests portable by replacing hardcoded `/tmp/` paths with `std::env::temp_dir()`, fixing sandbox failures
- Added CASE-based conditional cleanup to phase2_ddl.test restart section for idempotent re-runs

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove dead code from table_function.rs** - `8c4a4f2` (refactor)
2. **Task 2: Fix feature-gate, portable test paths, idempotent restart** - `5286c43` (fix)

## Files Created/Modified
- `src/query/table_function.rs` - Removed dead `column_type_ids` field, `logical_type_from_duckdb_type` fn, refactored `infer_schema_or_default` return type
- `src/lib.rs` - Added `#[cfg(feature = "extension")]` gate to `pub mod query`
- `src/catalog.rs` - Replaced hardcoded `/tmp/` with `std::env::temp_dir()` in 3 test functions
- `test/sql/phase2_ddl.test` - Added CASE-based conditional drop before define in restart section

## Decisions Made
- `infer_schema_or_default` now returns `Vec<String>` only; type information from `try_infer_schema` is discarded at the caller with `_types` -- `try_infer_schema` itself is unchanged (types are cheap side-effect-free computation needed to extract column names)
- `sidecar_path_derivation` test intentionally left unchanged -- it tests a pure function with hardcoded string inputs, not file I/O
- Used CASE expression for conditional cleanup in SQLLogicTest (DuckDB evaluates CASE lazily, avoiding `drop_semantic_view` call when view does not exist)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- All tech debt items from the v1.0 milestone audit (plan 01 scope) are resolved
- Codebase is cleaner with consistent feature-gating and portable tests
- Ready for any remaining tech debt plans in phase 06

## Self-Check: PASSED

All files exist, all commits verified (8c4a4f2, 5286c43).

---
*Phase: 06-tech-debt-cleanup*
*Completed: 2026-02-26*
