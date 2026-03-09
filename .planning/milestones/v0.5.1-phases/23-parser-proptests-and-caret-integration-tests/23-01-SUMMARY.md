---
phase: 23-parser-proptests-and-caret-integration-tests
plan: 01
subsystem: testing
tags: [proptest, property-based-testing, parser, case-insensitive, near-miss, bracket-validation]

requires:
  - phase: 21-error-location-reporting
    provides: ParseError struct, validate_and_rewrite, validate_clauses, detect_near_miss
provides:
  - 33 property-based tests for all 7 parser public functions
  - Case-insensitive detection coverage for all DDL forms
  - Position invariant verification across whitespace variations
  - Near-miss false-positive safety net
  - Bracket validation edge case coverage
affects: [future parser changes, DDL syntax additions]

tech-stack:
  added: []
  patterns: [arb_case_variant strategy for case-insensitive proptest, parameterized DDL form testing via index strategy]

key-files:
  created:
    - tests/parse_proptest.rs
  modified: []

key-decisions:
  - "Separate proptest blocks for detection (TEST-01), rewrite (TEST-02), position (TEST-03), near-miss (TEST-04), and brackets (TEST-05)"
  - "arb_case_variant strategy generates random upper/lower per character via vec(bool) -- effective for case-insensitive testing"
  - "Parameterized DDL form testing uses index strategy (0..7usize) into const arrays to avoid proptest macro limitations"
  - "Documented bracket validator behavior: unmatched close brackets with empty stack are silently tolerated"

patterns-established:
  - "arb_case_variant(prefix): proptest strategy for random case variation of ASCII string"
  - "DDL_FORMS/CREATE_FORMS/NAME_ONLY_FORMS const arrays for parameterized property testing"

requirements-completed: [TEST-01, TEST-02, TEST-03, TEST-04, TEST-05]

duration: 8min
completed: 2026-03-09
---

# Phase 23 Plan 01: Parser Property-Based Tests Summary

**33 proptest properties covering case-insensitive detection, rewrite verification, position invariants, near-miss safety, and bracket validation for all 7 parser public functions**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-09T14:34:21Z
- **Completed:** 2026-03-09T14:43:04Z
- **Tasks:** 2
- **Files created:** 1

## Accomplishments

- 33 property-based tests exercising all 7 public functions in `src/parse.rs`
- Case-insensitive detection verified for each DDL form individually with random case variation
- Position invariants verified: error positions account for leading whitespace across 4 error scenarios
- Near-miss detection confirmed safe: no false positives on 8 common SQL statement patterns
- Bracket validation edge cases covered: balanced, unbalanced, nested, string-embedded, mismatched
- Execution time: 0.14 seconds for all 33 properties (well under 10s threshold)

## Task Commits

Each task was committed atomically:

1. **Task 1: Detection and rewrite properties** - `ec9e8b7` (test)
2. **Task 2: Validation, position, near-miss, and bracket properties** - `07bb7ec` (test)

## Files Created/Modified

- `tests/parse_proptest.rs` (655 lines) - Property-based tests for all parser public functions

## Decisions Made

- **arb_case_variant strategy**: Generates random upper/lower per character using `proptest::collection::vec(bool, len)`. Each bool independently flips one character's case, giving 2^N combinations per prefix.
- **Parameterized DDL form testing**: Used const arrays `DDL_FORMS`, `CREATE_FORMS`, `NAME_ONLY_FORMS` with index strategy `0..7usize` to test all forms. This avoids proptest macro limitations with method calls in strategy positions.
- **Documented bracket tolerance**: The bracket validator silently ignores unmatched close brackets when the paren_stack is empty (check_close_bracket returns Ok when stack is empty). Test `brackets_extra_close_bracket_is_tolerated` documents this behavior.
- **No proptest config overrides**: Default 256 cases per property is sufficient. Parser functions are pure and fast (sub-microsecond per call), so all 33 properties complete in ~0.14s.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] proptest macro syntax incompatibility with method calls in strategy position**
- **Found during:** Task 1 (initial file creation)
- **Issue:** `0..DDL_FORMS.len()` and similar method-call strategies caused the `proptest!` macro to fail parsing -- the macro's `$strategy:expr` fragment cannot distinguish method calls from subsequent macro arms.
- **Fix:** Replaced all `.len()` calls with literal values: `0..7usize`, `0..3usize`.
- **Files modified:** tests/parse_proptest.rs
- **Verification:** Compilation succeeds, all tests pass.
- **Committed in:** ec9e8b7 (Task 1 commit)

**2. [Rule 3 - Blocking] Comments before first `#[test]` in proptest! block cause parse failure**
- **Found during:** Task 1 (initial file creation)
- **Issue:** Line comments (`// ---`) before the first `#[test]` inside the `proptest!` macro caused the macro to try parsing them as a config expression.
- **Fix:** Moved comments outside the macro or used doc comments (`///`) which are valid `#[$meta]` attributes.
- **Files modified:** tests/parse_proptest.rs
- **Verification:** Compilation succeeds.
- **Committed in:** ec9e8b7 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking: proptest macro syntax issues)
**Impact on plan:** Both fixes required for compilation. No scope change.

## Issues Encountered

- proptest `FileFailurePersistence::SourceParallel set, but failed to find lib.rs or main.rs` warning appears for all test functions. This is a cosmetic warning from proptest not finding a `lib.rs` in the integration test file directory. Does not affect test execution or results.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All 5 proptest requirement IDs (TEST-01 through TEST-05) satisfied by this plan
- TEST-06 (caret position Python integration tests) remains for Plan 02
- Parser module now has 33 property-based tests + 79 existing unit tests = comprehensive coverage

## Self-Check: PASSED

- FOUND: tests/parse_proptest.rs
- FOUND: ec9e8b7 (Task 1 commit)
- FOUND: 07bb7ec (Task 2 commit)
- FOUND: 23-01-SUMMARY.md

---
*Phase: 23-parser-proptests-and-caret-integration-tests*
*Completed: 2026-03-09*
