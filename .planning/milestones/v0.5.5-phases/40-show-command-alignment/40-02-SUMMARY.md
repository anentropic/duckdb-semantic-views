---
phase: 40-show-command-alignment
plan: 02
subsystem: ddl
tags: [vtab, show-commands, snowflake-alignment, boolean, sqllogictest, duckdb]

# Dependency graph
requires:
  - phase: 40-show-command-alignment
    provides: "5-column SHOW VIEWS, 6-column DIMS/METRICS/FACTS VTab schemas from Plan 01"
provides:
  - "4-column SHOW DIMS FOR METRIC with BOOLEAN required column"
  - "All sqllogictest files aligned to new SHOW command schemas"
  - "No VTab file exposes expr or source_table columns"
affects: [41-describe-rewrite]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "LogicalTypeId::Boolean with as_mut_slice::<bool>() for constant BOOLEAN column emission"
    - "DuckDB sqllogictest renders BOOLEAN as 'false'/'true' text (use T type, not I)"
    - "SHOW SEMANTIC VIEWS tests use SELECT FROM list_semantic_views() to skip non-deterministic created_on"

key-files:
  created: []
  modified:
    - src/ddl/show_dims_for_metric.rs
    - test/sql/phase34_1_show_dims_for_metric.test
    - test/sql/phase34_1_1_show_filtering.test
    - test/sql/phase34_1_alter_rename.test
    - test/sql/phase20_extended_ddl.test
    - test/sql/phase25_keyword_body.test

key-decisions:
  - "BOOLEAN column renders as 'false' text in sqllogictest (not '0' integer); use T type letter not I"
  - "DuckDB cannot parse SHOW inside subquery; use list_semantic_views() table function directly in tests"

patterns-established:
  - "SHOW DIMS FOR METRIC 4-column schema: table_name, name, data_type, required (BOOLEAN constant FALSE)"
  - "BOOLEAN VTab column: LogicalTypeId::Boolean + as_mut_slice::<bool>() pattern"
  - "SHOW VIEWS tests: SELECT cols FROM list_semantic_views() WHERE ... to avoid non-deterministic created_on"

requirements-completed: [SHOW-05, SHOW-06, SHOW-07, SHOW-08]

# Metrics
duration: 12min
completed: 2026-04-02
---

# Phase 40 Plan 02: SHOW DIMS FOR METRIC Schema + Remaining sqllogictest Updates Summary

**4-column SHOW DIMS FOR METRIC with BOOLEAN required column, plus all 5 sqllogictest files updated for new SHOW command schemas across the full test suite**

## Performance

- **Duration:** 12 min
- **Started:** 2026-04-02T11:32:34Z
- **Completed:** 2026-04-02T11:44:42Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- show_dims_for_metric.rs updated from 5-column schema (semantic_view_name, name, expr, source_table, data_type) to 4-column Snowflake-aligned schema (table_name, name, data_type, required BOOLEAN)
- All 5 remaining sqllogictest files updated to match new SHOW command column schemas
- No VTab file in the codebase exposes expr or source_table columns (SHOW-06, SHOW-07 fully complete)
- Full quality gate passes: 18 sqllogictest files green, all Rust tests pass, DuckLake CI passes

## Task Commits

Each task was committed atomically:

1. **Task 1: Update show_dims_for_metric.rs to 4-column schema with BOOLEAN** - `5cadb62` (feat)
2. **Task 2: Update all remaining sqllogictest files for new schemas** - `e7489de` (feat)

## Files Created/Modified
- `src/ddl/show_dims_for_metric.rs` - 5-col to 4-col with BOOLEAN required column (constant FALSE)
- `test/sql/phase34_1_show_dims_for_metric.test` - query TTTTT -> query TTTT with 4-col expected output
- `test/sql/phase34_1_1_show_filtering.test` - DIMS/METRICS/FACTS to TTTTTT, FOR METRIC to TTTT, SHOW VIEWS via list_semantic_views()
- `test/sql/phase34_1_alter_rename.test` - SHOW VIEWS assertions updated to use list_semantic_views()
- `test/sql/phase20_extended_ddl.test` - SHOW VIEWS assertions updated for new 5-col schema
- `test/sql/phase25_keyword_body.test` - SHOW VIEWS assertion updated for new 5-col schema

## Decisions Made
- BOOLEAN columns render as `false`/`true` text in DuckDB's sqllogictest runner, not `0`/`1` integers. Use `T` type letter (text) for BOOLEAN columns in query directives.
- DuckDB parser cannot parse `SELECT ... FROM (SHOW SEMANTIC VIEWS ...)` because the SHOW command is intercepted before SQL parsing. Tests use `SELECT ... FROM list_semantic_views() WHERE ...` to skip non-deterministic `created_on` while still testing the VTab output.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] BOOLEAN renders as 'false' text, not '0' integer**
- **Found during:** Task 2 (sqllogictest update)
- **Issue:** Plan suggested `I` type letter with `0` for BOOLEAN. DuckDB sqllogictest renders BOOLEAN as text `false`/`true`.
- **Fix:** Changed query type from `TTTI` to `TTTT` and expected value from `0` to `false`
- **Files modified:** phase34_1_show_dims_for_metric.test, phase34_1_1_show_filtering.test
- **Verification:** `just test-sql` passes
- **Committed in:** e7489de (Task 2 commit)

**2. [Rule 3 - Blocking] SELECT FROM (SHOW ...) syntax not supported**
- **Found during:** Task 2 (SHOW VIEWS test update)
- **Issue:** Plan suggested wrapping SHOW in subquery but DuckDB cannot parse SHOW inside subquery
- **Fix:** Changed SHOW VIEWS tests to use `SELECT ... FROM list_semantic_views() WHERE ...` directly
- **Files modified:** phase34_1_1_show_filtering.test
- **Verification:** `just test-sql` passes
- **Committed in:** e7489de (Task 2 commit)

**3. [Rule 3 - Blocking] Pre-existing SHOW VIEWS test failures from Plan 01**
- **Found during:** Task 2 (verification)
- **Issue:** Plan 01 changed list.rs from 2-col to 5-col schema but only updated phase34_1_show_commands.test. Three other test files (phase20_extended_ddl.test, phase25_keyword_body.test, phase34_1_alter_rename.test) still expected old 2-col output.
- **Fix:** Updated all three files to use `SELECT name, kind FROM list_semantic_views()` pattern
- **Files modified:** phase20_extended_ddl.test, phase25_keyword_body.test, phase34_1_alter_rename.test
- **Verification:** `just test-all` passes with all 18 sqllogictest files green
- **Committed in:** e7489de (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 bug, 2 blocking)
**Impact on plan:** All auto-fixes necessary for test correctness and quality gate. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 40 (SHOW Command Alignment) is complete: all SHOW-01 through SHOW-08 requirements met
- Phase 41 (DESCRIBE rewrite) can proceed -- all SHOW VTab schemas are stable and tested
- Key pattern for future: BOOLEAN columns use `LogicalTypeId::Boolean` + `as_mut_slice::<bool>()`

---
## Self-Check: PASSED

All 6 modified files confirmed present. Both task commits (5cadb62, e7489de) verified in git log.

---
*Phase: 40-show-command-alignment*
*Completed: 2026-04-02*
