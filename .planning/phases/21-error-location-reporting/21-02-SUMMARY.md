---
phase: 21-error-location-reporting
plan: 02
subsystem: testing
tags: [sqllogictest, error-reporting, integration-tests, parser-extension, duckdb-caret]

# Dependency graph
requires:
  - phase: 21-error-location-reporting
    plan: 01
    provides: "ParseError struct, validate_and_rewrite(), detect_near_miss(), sv_validate_ddl_rust FFI, C++ tri-state sv_parse_stub"
provides:
  - "Integration tests verifying all 3 ERR requirements through full extension pipeline"
  - "Confirms clause-level hints, positioned errors, and near-miss suggestions render through DuckDB's parser extension error path"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [sqllogictest-error-substring-matching]

key-files:
  created:
    - test/sql/phase21_error_reporting.test
  modified:
    - test/sql/TEST_LIST

key-decisions:
  - "Error substring matching in sqllogictest uses message text only (not caret lines) -- caret rendering verified by unit tests in Plan 01"
  - "Positioned errors tested via message content; sqllogictest does not expose caret line to assertions"

patterns-established:
  - "sqllogictest error tests match substring of DuckDB error message after ---- line"

requirements-completed: [ERR-01, ERR-02, ERR-03]

# Metrics
duration: 2min
completed: 2026-03-09
---

# Phase 21 Plan 02: Error Reporting Integration Tests Summary

**sqllogictest integration tests verifying clause-level hints, positioned errors, and near-miss DDL suggestions through the full extension load pipeline**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-09T13:30:48Z
- **Completed:** 2026-03-09T13:32:39Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- Created phase21_error_reporting.test with 10 test cases covering all 3 ERR requirements
- ERR-01: Missing tables clause, empty body, missing dimensions/metrics, clause keyword typo all produce clause-level error hints through extension pipeline
- ERR-02: Missing opening paren, missing view name, missing closing paren produce positioned errors (error_location set for DuckDB caret rendering)
- ERR-03: Near-miss DDL prefix typos ("CREAT SEMANTIC VIEW", "DROP SEMANTC VIEW") produce "Did you mean" suggestions through extension pipeline
- Non-interference confirmed: valid CREATE, DESCRIBE, normal SQL all unaffected
- Full test suite green: 55 Rust tests + 7 sqllogictest files + DuckLake CI

## Task Commits

Each task was committed atomically:

1. **Task 1: Integration test file for error reporting through extension load** - `51aa67c` (test)

## Files Created/Modified
- `test/sql/phase21_error_reporting.test` - 10 integration tests covering ERR-01 (clause hints), ERR-02 (positioned errors), ERR-03 (near-miss suggestions), and non-interference
- `test/sql/TEST_LIST` - Added phase21_error_reporting.test entry

## Decisions Made
- Used error message substring matching (sqllogictest `statement error` pattern) rather than attempting to verify caret position rendering directly -- caret rendering is a DuckDB framework feature verified by unit tests setting error_location correctly
- Tested missing closing paren as a positioned error case (ERR-02) in addition to the plan's suggested missing opening paren

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- All 3 ERR requirements verified through both unit tests (Plan 01) and integration tests (Plan 02)
- Phase 21 complete -- error location reporting fully implemented and tested
- Ready for Phase 22 or next milestone

## Self-Check: PASSED

- [x] test/sql/phase21_error_reporting.test exists
- [x] test/sql/TEST_LIST updated
- [x] Commit 51aa67c exists
- [x] 21-02-SUMMARY.md exists

---
*Phase: 21-error-location-reporting*
*Completed: 2026-03-09*
