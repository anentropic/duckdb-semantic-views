---
phase: 21-error-location-reporting
plan: 01
subsystem: api
tags: [parser, error-reporting, ffi, strsim, levenshtein, duckdb-parser-extension]

# Dependency graph
requires:
  - phase: 19-parser-hook-all-prefixes
    provides: "7 DDL prefix detection via detect_ddl_kind()"
  - phase: 20-extended-ddl
    provides: "DdlKind enum, rewrite_ddl(), sv_rewrite_ddl_rust FFI"
provides:
  - "ParseError struct with message + optional byte position"
  - "validate_and_rewrite() wrapping rewrite_ddl() with structural validation"
  - "validate_clauses() for CREATE body clause validation with suggestions"
  - "detect_near_miss() for fuzzy DDL prefix matching"
  - "sv_validate_ddl_rust FFI entry point (tri-state 0/1/2)"
  - "C++ sv_parse_stub with DISPLAY_EXTENSION_ERROR and error_location"
affects: [22-error-location-reporting-integration-tests]

# Tech tracking
tech-stack:
  added: []
  patterns: [tri-state-ffi, parse-phase-validation, positioned-error-reporting]

key-files:
  created: []
  modified:
    - src/parse.rs
    - cpp/src/shim.cpp

key-decisions:
  - "Word-count-based slicing for near-miss prefix comparison avoids false positives on long queries"
  - "validate_clauses extracted into helper functions (validate_brackets, scan_clause_keywords) to satisfy clippy too_many_lines"
  - "Position sentinel u32::MAX for no-position in FFI (matches DuckDB optional_idx pattern)"
  - "sv_parse_rust kept for backward compat -- sv_validate_ddl_rust is the new primary parse path"

patterns-established:
  - "Tri-state FFI pattern: 0=success, 1=error, 2=not-ours with output buffers"
  - "ParseError struct for validation errors with byte-offset positions"
  - "validate_and_rewrite wraps rewrite_ddl with pre-validation"

requirements-completed: [ERR-01, ERR-02, ERR-03]

# Metrics
duration: 14min
completed: 2026-03-09
---

# Phase 21 Plan 01: Error Location Reporting Summary

**ParseError struct with byte-offset positions, clause validation with suggestions, near-miss DDL prefix detection, and tri-state FFI routing errors through DuckDB's DISPLAY_EXTENSION_ERROR caret rendering**

## Performance

- **Duration:** 14 min
- **Started:** 2026-03-09T13:13:02Z
- **Completed:** 2026-03-09T13:27:21Z
- **Tasks:** 2 (TDD task 1: 3 commits; auto task 2: 1 commit)
- **Files modified:** 2

## Accomplishments
- ParseError struct with message + optional byte position for DuckDB caret rendering
- validate_and_rewrite() validates CREATE body structure (empty body, missing clauses, clause typos with suggestions, unbalanced brackets) before delegating to rewrite_ddl()
- detect_near_miss() catches DDL prefix typos like "CREAT SEMANTIC VIEW" using word-count-based Levenshtein comparison
- sv_validate_ddl_rust FFI returns tri-state (0/1/2) with error message + position through output buffers
- C++ sv_parse_stub returns DISPLAY_EXTENSION_ERROR with error_location for validation failures
- 24 new unit tests covering all validation behaviors, all 55 existing tests preserved

## Task Commits

Each task was committed atomically:

1. **Task 1: ParseError struct, validation functions, and near-miss detection** - `12b121f` (test: RED), `3c373e3` (feat: GREEN)
2. **Task 2: sv_validate_ddl_rust FFI and C++ tri-state sv_parse_stub** - `fc84ed2` (feat)

_Note: Task 1 followed TDD with RED/GREEN commits._

## Files Created/Modified
- `src/parse.rs` - Added ParseError struct, validate_and_rewrite(), validate_clauses(), validate_brackets(), scan_clause_keywords(), detect_near_miss(), suggest_clause_keyword(), sv_validate_ddl_rust FFI, write_position helper, and 24 unit tests
- `cpp/src/shim.cpp` - Updated sv_parse_stub to call sv_validate_ddl_rust with tri-state handling and error_location support; added sv_validate_ddl_rust FFI declaration

## Decisions Made
- Word-count-based slicing for near-miss detection: extract the first N words of the query (where N matches the prefix word count) for Levenshtein comparison, avoiding false positives on long queries
- Refactored validate_clauses into three helper functions (validate_brackets, scan_clause_keywords, check_close_bracket) to satisfy clippy pedantic too_many_lines lint
- Used u32::MAX as sentinel for "no position" in FFI, matching DuckDB's optional_idx pattern
- Kept sv_parse_rust exported for backward compatibility -- sv_validate_ddl_rust is the new primary path called from sv_parse_stub

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- C++ most vexing parse: `ParserExtensionParseResult err_result(string(error_buf))` was parsed as function declaration. Fixed by constructing `string err_msg(error_buf)` first, then passing it to the constructor.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Validation layer complete and routing through DuckDB's error rendering pipeline
- Ready for plan 02: integration tests verifying caret rendering through full extension load
- sv_parse_rust still exported but no longer called from sv_parse_stub

## Self-Check: PASSED

- [x] src/parse.rs exists
- [x] cpp/src/shim.cpp exists
- [x] 21-01-SUMMARY.md exists
- [x] Commit 12b121f (test RED) exists
- [x] Commit 3c373e3 (feat GREEN) exists
- [x] Commit fc84ed2 (feat Task 2) exists

---
*Phase: 21-error-location-reporting*
*Completed: 2026-03-09*
