---
phase: 25-sql-body-parser
plan: 04
subsystem: database
tags: [rust, duckdb, parser, ddl, vtab, json, proptest, sqllogictest, semantic-views]

# Dependency graph
requires:
  - phase: 25-03
    provides: AS-body dispatch in validate_create_body, rewrite_ddl_keyword_body, DefineFromJsonVTab
provides:
  - Complete end-to-end integration: AS-body DDL creates queryable views via sqllogictest
  - Full proptest round-trip assertion for validate_and_rewrite -> create_semantic_view_from_json
  - AS-body position invariant proptest pointing at TABLSE typo
  - phase25_keyword_body.test registered in TEST_LIST, all statements passing
  - sv_rewrite_ddl_rust fixed to use validate_and_rewrite for correct AS-body dispatch
affects: [26-query-expansion, future-phases-using-AS-body-DDL]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "sv_rewrite_ddl_rust must call validate_and_rewrite (not rewrite_ddl) to handle both paren-body and AS-body DDL"
    - "sqllogictest statement error blocks require expected error message after ---- separator"

key-files:
  created:
    - .planning/phases/25-sql-body-parser/25-04-SUMMARY.md
  modified:
    - tests/parse_proptest.rs
    - test/sql/phase25_keyword_body.test
    - test/sql/TEST_LIST
    - src/parse.rs
    - .planning/phases/25-sql-body-parser/25-VALIDATION.md

key-decisions:
  - "sv_rewrite_ddl_rust changed from rewrite_ddl to validate_and_rewrite to unify both DDL body styles through the same dispatch path"
  - "phase25_keyword_body.test uses prefixed table names (p25_) to avoid collisions with other test files sharing the same in-memory catalog"
  - "DESCRIBE expected output includes full JSON for dimensions/metrics with output_type:null and source_table fields"

patterns-established:
  - "DDL rewrite FFI must use validate_and_rewrite, not rewrite_ddl, as validate_and_rewrite is the single source of truth for body-style dispatch"
  - "sqllogictest tables should use phase-specific prefixes to avoid cross-test pollution"

requirements-completed: [DDL-01, DDL-02, DDL-03, DDL-04, DDL-05, DDL-07]

# Metrics
duration: 20min
completed: 2026-03-11
---

# Phase 25 Plan 04: End-to-End Integration Verification Summary

**AS-body CREATE SEMANTIC VIEW pipeline fully verified end-to-end: proptest round-trip assertions upgraded, sqllogictest integration passing, and sv_rewrite_ddl_rust bug fixed to correctly dispatch AS-body DDL through validate_and_rewrite**

## Performance

- **Duration:** ~20 min
- **Started:** 2026-03-11T23:30:00Z
- **Completed:** 2026-03-11T23:40:37Z
- **Tasks:** 2 (Tasks 1-2 automated; Task 3 is checkpoint:human-verify)
- **Files modified:** 5

## Accomplishments
- `as_body_validate_and_rewrite_succeeds` proptest upgraded from weak detection-only check to full round-trip assertion: validates `validate_and_rewrite` returns `Ok(Some(sql))` with `create_semantic_view_from_json` route and view name embedded
- `as_body_position_invariant_clause_typo` proptest upgraded from detection check to actual error position assertion: verifies `err.position` points at byte offset of "TABLSE" in the query
- `phase25_keyword_body.test` fully implemented: `require semantic_views`, test data inserted, CREATE/OR-REPLACE/IF-NOT-EXISTS/RELATIONSHIPS/DESCRIBE/SHOW/DROP all verified with correct expected output
- `sv_rewrite_ddl_rust` bug fixed: changed from `rewrite_ddl` (paren-body only) to `validate_and_rewrite` (both paren-body and AS-body), enabling AS-body DDL to execute correctly through the C++ bind path
- `just test-all` green: 36 Rust unit/proptests, 8 sqllogictest files, 3 DuckLake CI tests

## Task Commits

Each task was committed atomically:

1. **Task 1+2: Strengthen integration tests and fix AS-body rewrite dispatch** - `0f86418` (feat)

## Files Created/Modified
- `tests/parse_proptest.rs` - Upgraded TEST-06 as_body_validate_and_rewrite_succeeds and as_body_position_invariant_clause_typo with full assertions
- `test/sql/phase25_keyword_body.test` - Complete integration test: require semantic_views, test data, all 7 DDL verbs with expected output, error cases
- `test/sql/TEST_LIST` - Added phase25_keyword_body.test to sqllogictest discovery
- `src/parse.rs` - Fixed sv_rewrite_ddl_rust to call validate_and_rewrite instead of rewrite_ddl
- `.planning/phases/25-sql-body-parser/25-VALIDATION.md` - Updated all task statuses to green, set nyquist_compliant: true

## Decisions Made
- `sv_rewrite_ddl_rust` must use `validate_and_rewrite` not `rewrite_ddl` because only `validate_and_rewrite` routes AS-body DDL through `rewrite_ddl_keyword_body` → JSON serialization path
- Phase-prefixed table names (p25_) in sqllogictest to avoid catalog collisions across test files
- DESCRIBE expected output explicitly encodes the full JSON including `output_type:null` and `source_table` fields to verify the full serialization path

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] sv_rewrite_ddl_rust called rewrite_ddl instead of validate_and_rewrite**
- **Found during:** Task 1 (running just test-sql after adding phase25_keyword_body.test)
- **Issue:** `sv_ddl_bind` in C++ calls `sv_rewrite_ddl_rust`, which called `rewrite_ddl`. `rewrite_ddl` uses `parse_create_body` which scans for `(` and fails on AS-body DDL (the `(` found is inside `TABLES (`, not the DDL body wrapper). The C++ then received malformed SQL and DuckDB reported "Parser Error: syntax error at or near 'AS'".
- **Fix:** Changed `sv_rewrite_ddl_rust` to call `validate_and_rewrite` (which already contains the AS-body dispatch to `rewrite_ddl_keyword_body`) and adapted the return type mapping
- **Files modified:** `src/parse.rs`
- **Verification:** `just test-sql` [8/8] SUCCESS after fix
- **Committed in:** `0f86418` (part of Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 — bug in FFI dispatch path)
**Impact on plan:** Fix was essential for the AS-body pipeline to work end-to-end. No scope creep.

## Issues Encountered
- `statement error` blocks in sqllogictest require an expected error substring after `----` — the initial test file had bare `statement error` blocks that caused a parse error from the sqllogictest runner

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Complete AS-body DDL pipeline is end-to-end verified: parse → rewrite → execute → catalog store → query
- Human checkpoint (Task 3) pending: visual verification of caret error position in DuckDB CLI
- Phase 26 (query expansion with JOIN graph) can proceed once Task 3 checkpoint is approved

---
*Phase: 25-sql-body-parser*
*Completed: 2026-03-11*
