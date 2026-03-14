---
phase: 27-alias-based-query-expansion
plan: 02
subsystem: parser
tags: [ddl, parse, cleanup, dead-code-removal]

# Dependency graph
requires:
  - phase: 25-sql-body-parser
    provides: AS-body keyword parser as replacement for paren-body path
provides:
  - Clean parse.rs with no paren-body DDL code path
  - All sqllogictest files using AS-body or function-based syntax only
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [validate_and_rewrite as sole CREATE DDL entry point]

key-files:
  created: []
  modified:
    - src/parse.rs
    - tests/parse_proptest.rs
    - test/sql/phase20_extended_ddl.test
    - test/sql/phase21_error_reporting.test
    - test/sql/TEST_LIST

key-decisions:
  - "rewrite_ddl made private and rejects CREATE forms -- validate_and_rewrite is sole entry point"
  - "CLAUSE_KEYWORDS and suggest_clause_keyword removed from parse.rs (body_parser.rs has its own copies)"
  - "validate_create_body returns clear error for non-AS-body syntax with position pointing after view name"

patterns-established:
  - "CREATE DDL always routes through validate_and_rewrite -> rewrite_ddl_keyword_body (AS-body path only)"

requirements-completed: [CLN-01]

# Metrics
duration: 12min
completed: 2026-03-13
---

# Phase 27 Plan 02: Remove Paren-Body DDL Syntax Summary

**Deleted 6 paren-body functions and 843 lines from parse.rs; rewrote 4 sqllogictest files to AS-body syntax**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-13T16:01:34Z
- **Completed:** 2026-03-13T16:13:54Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Removed all paren-body DDL parsing code from parse.rs (parse_create_body, parse_ddl_text, validate_brackets, scan_clause_keywords, validate_clauses, check_close_bracket, matching_close, suggest_clause_keyword, CLAUSE_KEYWORDS)
- validate_create_body now rejects non-AS-body syntax with clear "no longer supported" error message
- Deleted phase16_parser.test and phase19_parser_hook_validation.test (pure paren-body tests)
- Rewrote phase20_extended_ddl.test and phase21_error_reporting.test to use AS-body keyword syntax
- Updated parse_proptest.rs: removed validate_clauses/bracket validation proptests, updated CREATE form tests to use AS-body

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove paren-body code path from parse.rs** - `55b0eff` (feat)
2. **Task 2: Delete or rewrite old sqllogictest files** - `1bc8d46` (feat)

## Files Created/Modified
- `src/parse.rs` - Removed 6 functions, 2 constants; modified validate_create_body, rewrite_ddl, extract_ddl_name; deleted 40+ paren-body unit tests
- `tests/parse_proptest.rs` - Removed bracket validation proptests (TEST-05), updated CREATE/position tests to AS-body syntax
- `test/sql/phase16_parser.test` - Deleted (paren-body parser hook tests)
- `test/sql/phase19_parser_hook_validation.test` - Deleted (paren-body 7-form validation tests)
- `test/sql/phase20_extended_ddl.test` - Rewritten to AS-body syntax (all 7 DDL verbs)
- `test/sql/phase21_error_reporting.test` - Rewritten: paren-body error tests removed, "no longer supported" error added, AS-body clause errors added
- `test/sql/TEST_LIST` - Removed phase16 and phase19 entries

## Decisions Made
- rewrite_ddl made private and rejects CREATE forms: prevents direct use of the old paren-body rewrite path
- CLAUSE_KEYWORDS/suggest_clause_keyword removed from parse.rs: body_parser.rs has its own copies, eliminating duplication
- validate_create_body error message includes example of correct AS-body syntax for user guidance

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed error substring match case in phase21 test**
- **Found during:** Task 2 (sqllogictest rewrite)
- **Issue:** Test expected "Did you mean" (capital D) but body_parser error message uses lowercase "did you mean"
- **Fix:** Changed test expectation to lowercase "did you mean"
- **Files modified:** test/sql/phase21_error_reporting.test
- **Verification:** just test-sql passes
- **Committed in:** 1bc8d46 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Trivial case-sensitivity fix in test expectation. No scope creep.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 27 (alias-based-query-expansion) is now complete (2 of 2 plans)
- parse.rs is clean: no paren-body code remains
- All test suites pass: cargo test, sqllogictest, DuckLake CI

---
*Phase: 27-alias-based-query-expansion*
*Completed: 2026-03-13*
