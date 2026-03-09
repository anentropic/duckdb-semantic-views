---
phase: 20-extended-ddl-statements
plan: 02
subsystem: database
tags: [duckdb, parser-hook, ddl, c++-ffi, result-forwarding, sqllogictest]

# Dependency graph
requires:
  - phase: 20-extended-ddl-statements
    plan: 01
    provides: DdlKind enum with 7 variants, multi-prefix detection/rewrite, sv_execute_ddl_rust FFI
provides:
  - sv_rewrite_ddl_rust FFI (rewrite-only, no execution)
  - C++ result-forwarding pipeline (sv_ddl_bind executes rewritten SQL and captures full result schema + data)
  - Dynamic column schema per DDL form (1 for CREATE/DROP, 6 for DESCRIBE, 2 for SHOW)
  - Working DESCRIBE SEMANTIC VIEW returning full view definition
  - Working SHOW SEMANTIC VIEWS listing all views
  - sqllogictest runner patch for StatementType.EXTENSION handling
affects: [community-extension-registry, semantic-view-documentation]

# Tech tracking
tech-stack:
  added: []
  patterns: [C++-executes-Rust-rewrites, dynamic-result-schema-forwarding, statement-cache-disable-for-variable-schema, sqllogictest-runner-patching]

key-files:
  created: []
  modified:
    - src/parse.rs
    - cpp/src/shim.cpp
    - test/sql/phase20_extended_ddl.test
    - scripts/patch_sqllogictest.py

key-decisions:
  - "All columns forwarded as VARCHAR for simplicity (all underlying functions already return VARCHAR)"
  - "SupportStatementCache disabled because return schema varies per DDL form (1/2/6 columns)"
  - "sqllogictest runner patched to treat StatementType.EXTENSION with QUERY_RESULT as query result"
  - "sv_execute_ddl_rust kept for backward compatibility but no longer called from C++"

patterns-established:
  - "sv_rewrite_ddl_rust FFI: Rust rewrites SQL, C++ executes via duckdb_query on sv_ddl_conn"
  - "Result forwarding: duckdb_column_count + duckdb_value_varchar + duckdb_free for full result capture"
  - "Variable-schema table function: disable statement caching when bind returns different column counts"

requirements-completed: [DDL-07, DDL-08]

# Metrics
duration: 23min
completed: 2026-03-09
---

# Phase 20 Plan 02: C++ Result Forwarding and DESCRIBE/SHOW Integration Summary

**C++ result-forwarding pipeline enabling DESCRIBE SEMANTIC VIEW (6 columns) and SHOW SEMANTIC VIEWS (2 columns) through dynamic schema capture from rewritten SQL execution**

## Performance

- **Duration:** 23 min
- **Started:** 2026-03-09T11:46:15Z
- **Completed:** 2026-03-09T12:10:09Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Refactored C++ sv_ddl_bind to call sv_rewrite_ddl_rust (rewrite-only FFI) then execute via duckdb_query, dynamically forwarding the result schema and data
- DESCRIBE SEMANTIC VIEW returns full 6-column definition (name, base_table, dimensions, metrics, filters, joins)
- SHOW SEMANTIC VIEWS returns 2-column listing of all views (name, base_table)
- All 7 DDL forms (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW) work end-to-end via native SQL syntax
- 185 Rust tests + 6 sqllogictest files + DuckLake CI all pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Add sv_rewrite_ddl_rust FFI and refactor C++ to forward result sets** - `873d950` (feat)
2. **Task 2: Add DESCRIBE and SHOW integration tests and verify full suite** - `61fb66a` (feat)

## Files Created/Modified
- `src/parse.rs` - Added sv_rewrite_ddl_rust FFI (rewrite-only, no execution); kept sv_execute_ddl_rust for backward compatibility
- `cpp/src/shim.cpp` - Refactored SvDdlBindData to store full result sets; sv_ddl_bind now uses sv_rewrite_ddl_rust + duckdb_query; sv_ddl_execute emits multi-row results with offset tracking; disabled statement caching
- `test/sql/phase20_extended_ddl.test` - Added DDL-07 (DESCRIBE) and DDL-08 (SHOW) integration tests with case insensitivity, error handling, and full lifecycle test
- `scripts/patch_sqllogictest.py` - Added patch for StatementType.EXTENSION handling in Python sqllogictest runner

## Decisions Made
- All columns forwarded as VARCHAR: all underlying semantic view functions (describe, list, create, drop) already return VARCHAR columns, so VARCHAR forwarding is lossless
- Disabled SupportStatementCache: the sv_ddl_internal table function returns different column counts per DDL form (1 for CREATE/DROP, 6 for DESCRIBE, 2 for SHOW), so DuckDB must re-bind on every invocation
- Patched sqllogictest runner: DuckDB reports parser extension statements as StatementType.EXTENSION with all three expected result types (QUERY_RESULT, CHANGED_ROWS, NOTHING), causing the Python test runner to misclassify them as CHANGED_ROWS (1 BIGINT column); the patch treats EXTENSION type with QUERY_RESULT as a query result
- Kept sv_execute_ddl_rust for backward compatibility even though C++ no longer calls it

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Disabled statement caching for variable-schema table function**
- **Found during:** Task 2
- **Issue:** DuckDB cached the plan from the first sv_ddl_internal invocation (CREATE, 1 column) and reused it for subsequent invocations (DESCRIBE, 6 columns), causing column count mismatches
- **Fix:** Added `SupportStatementCache() -> false` override on SvDdlBindData so DuckDB always re-binds
- **Files modified:** cpp/src/shim.cpp
- **Verification:** DESCRIBE and SHOW now correctly report their actual column counts
- **Committed in:** 61fb66a (Task 2 commit)

**2. [Rule 3 - Blocking] Patched sqllogictest runner for EXTENSION statement type**
- **Found during:** Task 2
- **Issue:** Python sqllogictest runner's is_query_result() returned False for StatementType.EXTENSION (len(expected_result_type) == 3, not 1), causing multi-column parser extension results to be treated as single-column CHANGED_ROWS
- **Fix:** Extended patch_sqllogictest.py to add StatementType.EXTENSION handling before the len==1 check
- **Files modified:** scripts/patch_sqllogictest.py
- **Verification:** `just test-sql` passes all 6 test files including phase20_extended_ddl.test
- **Committed in:** 61fb66a (Task 2 commit)

**3. [Rule 1 - Bug] Corrected JSON format in test expectations**
- **Found during:** Task 2
- **Issue:** Test expected JSON without output_type field, but actual serialization includes `"output_type":null` and alphabetically orders keys
- **Fix:** Updated test expectations to match actual serde_json serialization format
- **Files modified:** test/sql/phase20_extended_ddl.test
- **Verification:** All test assertions match actual output
- **Committed in:** 61fb66a (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** Statement caching and sqllogictest runner issues were fundamental blockers preventing multi-column parser extension results from working in tests. JSON format was a cosmetic mismatch. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 7 DDL forms work end-to-end via native SQL syntax (DDL-03 through DDL-08 plus DDL-01/DDL-02 from v0.5.0)
- Phase 20 complete -- ready for milestone tagging or next feature phase
- The sqllogictest runner patch should be upstreamed to extension-ci-tools or addressed when the pinned duckdb-sqllogictest-python version is updated

---
*Phase: 20-extended-ddl-statements*
*Completed: 2026-03-09*
