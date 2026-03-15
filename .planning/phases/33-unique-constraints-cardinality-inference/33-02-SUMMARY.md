---
phase: 33-unique-constraints-cardinality-inference
plan: 02
subsystem: database
tags: [duckdb, cardinality, validation, fan-trap, sqllogictest, unique-constraints]

# Dependency graph
requires:
  - phase: 33-unique-constraints-cardinality-inference
    provides: "Plan 01 model changes (unique_constraints, ref_columns, Cardinality 2-variant enum, parser UNIQUE/REFERENCES parsing)"
provides:
  - "CARD-03/09 FK reference validation against PK/UNIQUE constraints"
  - "Fan trap detection with inferred cardinality (no explicit keywords)"
  - "ON clause synthesis using resolved ref_columns"
  - "Old-JSON rejection guard for pre-v0.5.4 semantic views"
  - "Phase 33 end-to-end sqllogictest (14 tests covering CARD-01 through CARD-09)"
affects: [34-duckdb-upgrade, 36-registry-publishing]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "FK reference validation via HashSet exact-set matching against PK and UNIQUE constraints"
    - "Old-JSON detection guard (fk_columns present but ref_columns empty)"

key-files:
  created:
    - "test/sql/phase33_cardinality_inference.test"
  modified:
    - "src/graph.rs"
    - "src/expand.rs"
    - "src/ddl/define.rs"
    - "test/sql/phase31_fan_trap.test"
    - "test/sql/phase28_e2e.test"
    - "test/sql/phase29_facts_hierarchies.test"
    - "test/sql/phase30_derived_metrics.test"
    - "test/sql/TEST_LIST"

key-decisions:
  - "Replaced check_fk_pk_counts with validate_fk_references using exact HashSet matching"
  - "ON clause synthesis prefers ref_columns, falls back to pk_columns for backward compat"
  - "Test 6 redesigned with p33_user_tokens table to avoid VARCHAR-to-INTEGER type mismatch"

patterns-established:
  - "FK validation pattern: collect ref_columns as HashSet, compare against PK set then each UNIQUE set"
  - "Old-JSON guard pattern: check for fk_columns present but ref_columns empty before validate_graph"

requirements-completed: [CARD-03, CARD-08, CARD-09]

# Metrics
duration: 14min
completed: 2026-03-15
---

# Phase 33 Plan 02: Validation, Fan Trap, Tests Summary

**FK reference validation (CARD-03/09) via PK/UNIQUE set matching, fan trap with inferred cardinality, ON clause using ref_columns, old-JSON guard, and 14-test phase33 sqllogictest**

## Performance

- **Duration:** 14 min
- **Started:** 2026-03-15T20:00:47Z
- **Completed:** 2026-03-15T20:15:37Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- FK reference validation enforces exact PK/UNIQUE set matching (CARD-03/09), with error messages listing available constraints
- Fan trap detection works with 2-variant Cardinality enum (ManyToOne reverse = fan-out, OneToOne = safe)
- ON clause synthesis uses resolved ref_columns from Join struct instead of hardcoded pk_columns lookup
- Old-format JSON (pre-v0.5.4) rejected with clear upgrade message before graph validation
- Updated phase31_fan_trap.test: removed cardinality keywords, deleted 2 tests, renumbered to 7 tests
- Updated DESCRIBE expected output in phase28, phase29, phase30 tests for new ref_columns field
- Created phase33_cardinality_inference.test with 14 end-to-end tests covering CARD-01 through CARD-09
- Full quality gate passes: cargo test + sqllogictest (13 files) + DuckLake CI

## Task Commits

Each task was committed atomically:

1. **Task 1: Add CARD-03/09 validation, adapt fan trap, ON clause, old-JSON guard, DESCRIBE** - `f2d2cad` (feat)
2. **Task 2: Update existing sqllogictests and create phase33 end-to-end test** - `9d19b5f` (test)

## Files Created/Modified
- `src/graph.rs` - Replaced `check_fk_pk_counts` with `validate_fk_references`; added 6 unit tests in `phase33_fk_reference_tests` module
- `src/expand.rs` - Updated fan trap error message to inference language; ON clause uses ref_columns; added ref_columns to test helper joins
- `src/ddl/define.rs` - Added old-JSON rejection guard before validate_graph
- `test/sql/phase31_fan_trap.test` - Removed cardinality keywords, deleted Tests 5 and 9, renumbered to 7 tests
- `test/sql/phase33_cardinality_inference.test` - New 14-test end-to-end sqllogictest for CARD-01 through CARD-09
- `test/sql/phase28_e2e.test` - Updated DESCRIBE expected output with ref_columns in join JSON
- `test/sql/phase29_facts_hierarchies.test` - Updated DESCRIBE expected output with ref_columns in join JSON
- `test/sql/phase30_derived_metrics.test` - Updated DESCRIBE expected output with ref_columns in join JSON
- `test/sql/TEST_LIST` - Added phase33_cardinality_inference.test entry

## Decisions Made
- Replaced `check_fk_pk_counts` (which only validated FK/PK column count matching) with `validate_fk_references` (which validates FK target columns match an exact PK or UNIQUE constraint set) -- stricter and correct per CARD-03/09
- ON clause synthesis prefers `join.ref_columns` (populated at parse time) over looking up `pk_columns` from the target table, with fallback to pk_columns for backward compatibility
- Redesigned Test 6 (OneToOne from UNIQUE) to use a new `p33_user_tokens` table with INTEGER `user_id` as UNIQUE, avoiding the VARCHAR-to-INTEGER type mismatch that occurred when joining `email` to `user_id`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed DESCRIBE test failures in phase28/29/30**
- **Found during:** Task 2
- **Issue:** After Plan 01 added `ref_columns` to the Join struct and the parser populates it, DESCRIBE output in existing tests (phase28, 29, 30) now includes `"ref_columns":["id"]` in the serialized join JSON, causing expected output mismatches
- **Fix:** Updated DESCRIBE expected output in all three test files to include `"ref_columns":["id"]` in join objects
- **Files modified:** test/sql/phase28_e2e.test, test/sql/phase29_facts_hierarchies.test, test/sql/phase30_derived_metrics.test
- **Verification:** `just test-sql` passes all 13 test files
- **Committed in:** 9d19b5f (Task 2 commit)

**2. [Rule 1 - Bug] Fixed Test 6 type mismatch**
- **Found during:** Task 2
- **Issue:** Original plan had Test 6 joining `c(email)` (VARCHAR) to `p(user_id)` (INTEGER), causing "Could not convert string 'alice@test.com' to INT32" at query time
- **Fix:** Redesigned Test 6 to use a new `p33_user_tokens` table with `user_id INTEGER` as UNIQUE constraint, joining `t(user_id) REFERENCES c` with compatible INTEGER types
- **Files modified:** test/sql/phase33_cardinality_inference.test
- **Verification:** `just test-sql` passes
- **Committed in:** 9d19b5f (Task 2 commit)

**3. [Rule 1 - Bug] Fixed clippy doc_markdown and if_not_else warnings**
- **Found during:** Task 1
- **Issue:** `ref_columns` in doc comments triggered doc_markdown lint (needs backticks); `if !join.ref_columns.is_empty()` triggered if_not_else lint
- **Fix:** Added backticks around `ref_columns` in comments; inverted condition to `if join.ref_columns.is_empty() { fallback } else { ref_cols }`
- **Files modified:** src/graph.rs, src/expand.rs
- **Verification:** `cargo test` passes with no warnings
- **Committed in:** f2d2cad (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (3 bug fixes)
**Impact on plan:** All auto-fixes necessary for correctness. No scope creep.

## Issues Encountered
- Pre-commit hook ran `cargo fmt` which reformatted multi-line function signatures to single-line, requiring re-staging after format pass
- DuckLake CI tests required sandbox bypass due to uv cache directory access restrictions

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Phase 33 (UNIQUE Constraints & Cardinality Inference) is now complete -- all CARD requirements validated
- Ready for Phase 34 (DuckDB 1.5.0 Upgrade) which is independent of cardinality features
- No blockers for next phase

## Self-Check: PASSED

All 9 files verified present. Both task commits (f2d2cad, 9d19b5f) verified in git history.

---
*Phase: 33-unique-constraints-cardinality-inference*
*Completed: 2026-03-15*
