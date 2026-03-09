---
phase: 21-error-location-reporting
plan: 03
subsystem: parsing
tags: [duckdb, parser, validation, scan_clause_keywords, tdd, error-reporting]

# Dependency graph
requires:
  - phase: 21-01
    provides: "scan_clause_keywords, validate_clauses, validate_and_rewrite functions"
  - phase: 21-02
    provides: "Integration test file phase21_error_reporting.test with error path coverage"
provides:
  - "scan_clause_keywords recognizes both := and ( as clause-keyword delimiters"
  - "20 unit tests covering all validation paths with ( syntax"
  - "Integration tests for unknown-far keyword, case insensitivity, CREATE OR REPLACE, metrics-only"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Dual-delimiter recognition in clause keyword scanner"
    - "Error-path integration tests use ( syntax; success-path tests retain := for DuckDB execution"

key-files:
  created: []
  modified:
    - "src/parse.rs"
    - "test/sql/phase21_error_reporting.test"

key-decisions:
  - "Integration error tests migrated to ( syntax; success tests retain := because rewrite_ddl passes body verbatim to DuckDB function calls requiring := named parameter syntax"
  - "Section 2 structural error tests adapted for ( syntax compatibility (missing-paren test simplified, missing-close-paren test uses no-paren body)"

patterns-established:
  - "Dual delimiter gate: scan_clause_keywords accepts := OR ( after clause keyword"

requirements-completed: [ERR-01, ERR-02, ERR-03]

# Metrics
duration: 12min
completed: 2026-03-09
---

# Phase 21 Plan 03: Gap Closure Summary

**Fixed scan_clause_keywords ( delimiter gate, migrated 12 unit tests from := to ( syntax, added 8 coverage gap tests, migrated integration error tests to ( syntax**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-09T18:41:09Z
- **Completed:** 2026-03-09T18:53:41Z
- **Tasks:** 2 (Task 1 TDD with RED+GREEN commits)
- **Files modified:** 2

## Accomplishments
- Fixed the root cause of UAT tests 2 and 6: scan_clause_keywords now recognizes `(` as a clause delimiter alongside `:=`
- Rewrote all 12 existing validation unit tests from := to ( syntax
- Added 8 new unit tests closing coverage gaps: CREATE OR REPLACE, CREATE IF NOT EXISTS, relationships clause, unknown-far keyword, case insensitivity, tables+metrics only, DESCRIBE, DROP IF EXISTS
- Migrated all integration error tests to ( syntax and added new integration tests for unknown-far keyword, case insensitivity, CREATE OR REPLACE, and metrics-only

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix delimiter gate and rewrite all unit tests to ( syntax (TDD)**
   - RED: `065c49f` (test) - 12 rewritten + 8 new tests, all failing
   - GREEN: `631c867` (feat) - delimiter gate fix, all 170 unit tests pass
2. **Task 2: Migrate integration tests to ( syntax and add coverage** - `8c4c123` (feat)

## Files Created/Modified
- `src/parse.rs` - Fixed scan_clause_keywords delimiter gate (line 420), updated doc comment, rewrote 12 validation unit tests to ( syntax, added 8 new coverage gap tests
- `test/sql/phase21_error_reporting.test` - Migrated error-path tests to ( syntax, added tests for unknown-far keyword, case insensitivity, CREATE OR REPLACE, metrics-only; adapted structural error tests

## Decisions Made
- Integration error tests (statement error) migrated to ( syntax to verify scan_clause_keywords accepts ( delimiter
- Integration success tests (statement ok) retain := syntax because rewrite_ddl passes body verbatim to DuckDB function calls which require := named parameter syntax
- Section 2 structural error tests adapted: missing-paren test simplified to `CREATE SEMANTIC VIEW err_test;`, missing-close-paren test uses `tables, dimensions` (no parens) to avoid false `)` matches from `rfind`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Integration success tests cannot use ( syntax**
- **Found during:** Task 2 (integration test migration)
- **Issue:** Plan specified migrating ALL integration tests to ( syntax, but statement ok tests fail when body uses ( syntax because rewrite_ddl passes body verbatim to DuckDB function calls. DuckDB interprets `TABLES ([...])` as a function call, producing "Scalar Function with name tables does not exist!"
- **Fix:** Kept := syntax for statement ok tests; only migrated statement error tests to ( syntax (validation errors fire before rewrite)
- **Files modified:** test/sql/phase21_error_reporting.test
- **Verification:** just test-all passes with all 7 sqllogictest files
- **Committed in:** 8c4c123

**2. [Rule 3 - Blocking] Section 2 structural error tests incompatible with ( body syntax**
- **Found during:** Task 2 (integration test migration)
- **Issue:** Missing-opening-paren test `CREATE SEMANTIC VIEW err_test tables ([])` would have DuckDB find the ( inside the body instead of failing with Expected '('. Missing-closing-paren test with ( in body has rfind(')') matching inner parens.
- **Fix:** Simplified missing-paren test to `CREATE SEMANTIC VIEW err_test;`, missing-close-paren test uses `tables, dimensions` (no parens) to ensure no false `)` match
- **Files modified:** test/sql/phase21_error_reporting.test
- **Verification:** just test-all passes
- **Committed in:** 8c4c123

---

**Total deviations:** 2 auto-fixed (2 blocking issues)
**Impact on plan:** Both auto-fixes were necessary for correctness. Error-path coverage with ( syntax is achieved. Success-path tests retain := which is the only syntax DuckDB can execute. No scope creep.

## Issues Encountered
None beyond the documented deviations.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 21 error location reporting is complete
- All validation paths tested with ( syntax (unit tests) and integration tests
- scan_clause_keywords accepts both := and ( delimiters
- Full quality gate passes: 170 unit tests, 6 proptests, 36 output proptests, 33 parse proptests, 7 sqllogictest files, DuckLake CI

## Self-Check: PASSED

- FOUND: src/parse.rs
- FOUND: test/sql/phase21_error_reporting.test
- FOUND: 21-03-SUMMARY.md
- FOUND: 065c49f (RED commit)
- FOUND: 631c867 (GREEN commit)
- FOUND: 8c4c123 (Task 2 commit)

---
*Phase: 21-error-location-reporting*
*Completed: 2026-03-09*
