---
phase: 27-alias-based-query-expansion
plan: 03
subsystem: parser
tags: [error-messages, caret-position, python-integration-tests, sqllogictest]

# Dependency graph
requires:
  - phase: 27-02
    provides: "Paren-body DDL removal (CLN-01), validate_create_body error path"
provides:
  - "Simplified error message without 'no longer supported' wording"
  - "Python caret tests exercising AS-body error paths end-to-end"
  - "All test suites green (quality gate)"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: []

key-files:
  created: []
  modified:
    - src/parse.rs
    - tests/parse_proptest.rs
    - test/sql/phase21_error_reporting.test
    - test/integration/test_caret_position.py

key-decisions:
  - "Error message says 'Expected AS keyword' without referencing old syntax that was never released"

patterns-established: []

requirements-completed: [EXP-01, EXP-05, CLN-01, CLN-02, CLN-03]

# Metrics
duration: 6min
completed: 2026-03-13
---

# Phase 27 Plan 03: Error Message + Caret Test Gap Closure Summary

**Simplified non-AS-body error message and rewrote Python caret tests for AS-body error scenarios, closing the 27-VERIFICATION.md gap**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-13T16:55:03Z
- **Completed:** 2026-03-13T17:01:26Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Removed "no longer supported" and "old paren-body" wording from error message (user directive: syntax was never released)
- Updated all 4 Rust tests (3 unit + 1 proptest) to assert on "Expected 'AS' keyword"
- Updated 2 sqllogictest assertions in phase21_error_reporting.test Section 1
- Rewrote 2 Python caret integration tests to exercise AS-body error paths (missing paren after clause keyword, misspelled clause keyword)
- Full quality gate (`just test-all`) passes: 281 Rust tests, sqllogictests, DuckLake CI, vtab crash, and 3 caret tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Simplify error message and update Rust + sqllogictest assertions** - `d7f1a8b` (fix)
2. **Task 2: Rewrite Python caret tests to AS-body error scenarios** - `7bfa13c` (fix)

## Files Created/Modified
- `src/parse.rs` - Simplified validate_create_body error message, updated 3 unit test assertions
- `tests/parse_proptest.rs` - Updated position_invariant_paren_body_rejected assertion
- `test/sql/phase21_error_reporting.test` - Updated Section 1 expected error substrings
- `test/integration/test_caret_position.py` - Rewrote test_caret_missing_paren (AS-body missing paren) and test_caret_clause_typo (AS-body clause typo)

## Decisions Made
- Error message says "Expected 'AS' keyword after view name" without referencing old syntax, per user directive that paren-body was never publicly released

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 27 verification gap fully closed
- All test suites green, quality gate met
- Ready for next phase in milestone

## Self-Check: PASSED

All files exist, all commits verified.

---
*Phase: 27-alias-based-query-expansion*
*Completed: 2026-03-13*
