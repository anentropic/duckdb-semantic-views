---
phase: 20-extended-ddl-statements
plan: 01
subsystem: database
tags: [duckdb, parser-hook, ddl, rust, ffi, sqllogictest]

# Dependency graph
requires:
  - phase: 19-parser-hook-validation-spike
    provides: Empirical confirmation that all 7 DDL prefixes trigger the parser fallback hook
provides:
  - DdlKind enum with 7 variants for DDL form dispatch
  - Multi-prefix detection (detect_semantic_view_ddl, detect_ddl_kind) with longest-first ordering
  - Multi-form rewrite (rewrite_ddl) mapping all 7 DDL forms to function calls
  - Name extraction (extract_ddl_name) for CREATE/DROP/DESCRIBE with None for SHOW
  - End-to-end native DDL for DROP, DROP IF EXISTS, CREATE OR REPLACE, CREATE IF NOT EXISTS
  - Generic C++ error message ("Semantic view DDL failed")
affects: [20-02-PLAN, describe-semantic-view, show-semantic-views]

# Tech tracking
tech-stack:
  added: []
  patterns: [enum-based DDL dispatch, longest-prefix-first detection, 3-category rewrite (CREATE-with-body, name-only, no-args)]

key-files:
  created:
    - test/sql/phase20_extended_ddl.test
  modified:
    - src/parse.rs
    - cpp/src/shim.cpp
    - test/sql/phase19_parser_hook_validation.test
    - test/sql/TEST_LIST

key-decisions:
  - "Combined TDD RED+GREEN for parse.rs since implementation is a direct extension of proven v0.5.0 pattern"
  - "Kept backward-compatible wrappers (detect_create_semantic_view, rewrite_ddl_to_function_call) for existing callers"
  - "SHOW returns 'ok' as name_out placeholder since sv_ddl_internal expects a single-column result"

patterns-established:
  - "DdlKind enum dispatch: detect_ddl_kind -> function_name + prefix_len -> category-based rewrite"
  - "Longest-prefix-first ordering: check 'create or replace' and 'create...if not exists' before 'create semantic view'"

requirements-completed: [DDL-03, DDL-04, DDL-05, DDL-06]

# Metrics
duration: 8min
completed: 2026-03-09
---

# Phase 20 Plan 01: Extended DDL Statements Summary

**DdlKind enum with 7-variant dispatch, multi-prefix detection/rewrite, and 4 new end-to-end native DDL statements (DROP, DROP IF EXISTS, CREATE OR REPLACE, CREATE IF NOT EXISTS)**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-09T11:34:33Z
- **Completed:** 2026-03-09T11:42:59Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Refactored parse.rs from single-prefix CREATE to 7-prefix DdlKind enum dispatch with longest-first ordering
- All 7 DDL prefixes detected and rewritten to function calls (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW)
- 4 mutating DDL statements work end-to-end via native syntax (DDL-03, DDL-04, DDL-05, DDL-06)
- 36 new unit tests + 1 new sqllogictest file covering all DDL forms
- Full test suite green: 185 Rust tests + 6 sqllogictest files + DuckLake CI

## Task Commits

Each task was committed atomically:

1. **Task 1: Refactor parse.rs to DdlKind enum with multi-prefix detection and rewriting** - `d3ece7b` (feat)
2. **Task 2: Update C++ error message and add sqllogictest for mutating DDL** - `f086fd9` (feat)

## Files Created/Modified
- `src/parse.rs` - DdlKind enum, detect_semantic_view_ddl, detect_ddl_kind, rewrite_ddl, extract_ddl_name, updated FFI entry points
- `cpp/src/shim.cpp` - Generic error message "Semantic view DDL failed"
- `test/sql/phase20_extended_ddl.test` - Integration tests for DDL-03, DDL-04, DDL-05, DDL-06
- `test/sql/phase19_parser_hook_validation.test` - Updated from spike expectations to reflect working DDL
- `test/sql/TEST_LIST` - Added phase20_extended_ddl.test

## Decisions Made
- Combined TDD RED+GREEN for parse.rs since the implementation directly extends the proven v0.5.0 pattern with no design uncertainty
- Kept backward-compatible wrappers for detect_create_semantic_view and rewrite_ddl_to_function_call
- SHOW SEMANTIC VIEWS writes "ok" to name_out as a placeholder (Plan 02 will add proper result forwarding)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated phase19_parser_hook_validation.test**
- **Found during:** Task 2
- **Issue:** Phase 19 spike test expected `statement error` for DDL forms (DROP, CREATE OR REPLACE, etc.) that now work after Phase 20 implementation
- **Fix:** Updated test expectations from `statement error` to `statement ok` for all 6 newly-working prefixes; removed assertion about spurious "IF" view
- **Files modified:** test/sql/phase19_parser_hook_validation.test
- **Verification:** `just test-sql` passes all 6 test files
- **Committed in:** f086fd9 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary update -- the spike test documented pre-implementation behavior that is now superseded. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- DESCRIBE SEMANTIC VIEW and SHOW SEMANTIC VIEWS are detected and rewritten but return only a single-column result (view name / "ok")
- Plan 02 needs C++ result forwarding refactor to pass full multi-column results through sv_ddl_internal
- The P3 blocker (three-connection lock conflict during DROP) was not observed -- DROP works cleanly via sequential connection pattern

---
*Phase: 20-extended-ddl-statements*
*Completed: 2026-03-09*
