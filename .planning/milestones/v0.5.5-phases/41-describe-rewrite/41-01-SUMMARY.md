---
phase: 41-describe-rewrite
plan: 01
subsystem: ddl
tags: [vtab, describe, snowflake-alignment, property-per-row]

requires:
  - phase: 39-metadata-storage
    provides: created_on, database_name, schema_name, fact output_type on SemanticViewDefinition
  - phase: 40-show-alignment
    provides: Snowflake-aligned SHOW commands pattern (row struct, bind-time collection, func emit)
provides:
  - Snowflake-aligned DESCRIBE SEMANTIC VIEW with 5-column property-per-row output
  - DescribeRow struct and collect functions for all 6 object kinds
  - format_json_array helper for PRIMARY_KEY/FOREIGN_KEY/REF_KEY JSON format
  - Comprehensive phase41_describe.test covering DESC-01 through DESC-07
affects: [41-02-PLAN (existing test updates already done here)]

tech-stack:
  added: []
  patterns:
    - Property-per-row VTab pattern for DESCRIBE (DescribeRow struct with bind-time collection)
    - format_json_array for Snowflake-compatible JSON array column values

key-files:
  created:
    - test/sql/phase41_describe.test
  modified:
    - src/ddl/describe.rs
    - test/sql/TEST_LIST
    - test/sql/phase20_extended_ddl.test
    - test/sql/phase21_error_reporting.test
    - test/sql/phase25_keyword_body.test
    - test/sql/phase28_e2e.test
    - test/sql/phase29_facts.test
    - test/sql/phase30_derived_metrics.test
    - test/sql/phase33_cardinality_inference.test

key-decisions:
  - "Empty string for parent_entity where Snowflake uses NULL (VARCHAR column, consistent with SHOW VTab pattern)"
  - "Empty string for DATA_TYPE when output_type is None (matches Phase 40 SHOW behavior)"
  - "Skip unnamed/legacy joins for RELATIONSHIP rows (pre-Phase-24 backward compat)"
  - "Row ordering: tables, relationships, facts, dimensions, metrics (definition order)"
  - "Updated all 7 existing test files atomically with describe.rs rewrite (Rule 3: blocking)"

patterns-established:
  - "Property-per-row DESCRIBE: DescribeRow struct -> collect_*_rows() helpers -> bind-time Vec -> func emit"
  - "format_json_array: no-spaces-after-commas JSON array for Snowflake compatibility"

requirements-completed: [DESC-01, DESC-02, DESC-03, DESC-04, DESC-05, DESC-06, DESC-07]

duration: 15min
completed: 2026-04-02
---

# Phase 41 Plan 01: DESCRIBE Rewrite Summary

**Snowflake-aligned property-per-row DESCRIBE SEMANTIC VIEW with 5 VARCHAR columns, 6 object kinds, and comprehensive sqllogictest coverage**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-02T12:37:39Z
- **Completed:** 2026-04-02T12:52:53Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- Complete rewrite of describe.rs from single-row JSON-blob (6 columns) to Snowflake-aligned property-per-row format (5 columns: object_kind, object_name, parent_entity, property, property_value)
- All 6 object kinds implemented: TABLE, RELATIONSHIP, DIMENSION, FACT, METRIC, DERIVED_METRIC
- Created comprehensive phase41_describe.test with 8 test groups covering all DESC requirements
- Updated 7 existing test files to match new DESCRIBE format, ensuring zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Rewrite describe.rs to property-per-row format** - `83bfed0` (feat)
2. **Task 2: Create comprehensive phase41_describe.test** - `201cd79` (test)

## Files Created/Modified
- `src/ddl/describe.rs` - Complete rewrite: DescribeRow struct, format_json_array, 5 collect_*_rows helpers, 5-column VTab
- `test/sql/phase41_describe.test` - New: 8 test groups covering TABLE/RELATIONSHIP/DIMENSION/FACT/METRIC/DERIVED_METRIC, no-PK, error, case insensitivity, COUNT(*)
- `test/sql/TEST_LIST` - Added phase41_describe.test entry
- `test/sql/phase20_extended_ddl.test` - Updated 3 DESCRIBE assertions from 6-column to 5-column format
- `test/sql/phase21_error_reporting.test` - Updated 1 DESCRIBE assertion
- `test/sql/phase25_keyword_body.test` - Updated 1 DESCRIBE assertion
- `test/sql/phase28_e2e.test` - Updated 1 DESCRIBE assertion (3-table view, 38 rows)
- `test/sql/phase29_facts.test` - Updated 1 DESCRIBE assertion (facts with relationships, 44 rows)
- `test/sql/phase30_derived_metrics.test` - Updated 1 DESCRIBE assertion (derived metrics, 26 rows)
- `test/sql/phase33_cardinality_inference.test` - Updated 2 COUNT(*) assertions (18 rows each)

## Decisions Made
- **Empty string for parent_entity** where Snowflake uses NULL: consistent with existing SHOW VTab pattern using VARCHAR columns
- **Row ordering follows definition order** (tables, relationships, facts, dimensions, metrics): matches natural reading order of DDL
- **Skip unnamed/legacy joins**: pre-Phase-24 joins without names or from_alias are omitted from RELATIONSHIP output
- **PRIMARY_KEY row conditionally emitted**: only when pk_columns is non-empty (Snowflake always requires PK, we made it optional)
- **Updated existing tests in Task 2**: Plan had "see Plan 02 for test updates" but CLAUDE.md quality gate requires `just test-all` to pass, so updates were done atomically

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated 7 existing test files with new DESCRIBE format**
- **Found during:** Task 2 (phase41_describe.test creation)
- **Issue:** The describe.rs rewrite changed output from 6 columns to 5 columns with multi-row output. 7 existing test files had DESCRIBE assertions that expected the old format, causing `just test-all` to fail.
- **Fix:** Updated all DESCRIBE assertions across phase20, phase21, phase25, phase28, phase29, phase30, phase33 test files to use new 5-column property-per-row format
- **Files modified:** test/sql/phase20_extended_ddl.test, test/sql/phase21_error_reporting.test, test/sql/phase25_keyword_body.test, test/sql/phase28_e2e.test, test/sql/phase29_facts.test, test/sql/phase30_derived_metrics.test, test/sql/phase33_cardinality_inference.test
- **Verification:** `just test-all` passes (19 sqllogictests + 42 Rust tests + 6 DuckLake CI)
- **Committed in:** 201cd79 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** CLAUDE.md quality gate requires `just test-all` to pass. The existing test updates were necessary and planned for Plan 02 but executed here to maintain green tests. Plan 02 may have reduced scope.

## Issues Encountered
- PKOpt test (phase33) initially miscounted rows: tables without explicit PK in DDL still had pk_columns populated via catalog PK detection, requiring 18 rows instead of expected 16

## Known Stubs
None - all data paths are fully wired.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- DESCRIBE rewrite complete with full test coverage
- Plan 02 (existing test updates) may have reduced scope since all test updates were done here
- All 7 DESC requirements have dedicated test coverage

## Self-Check: PASSED

- FOUND: src/ddl/describe.rs
- FOUND: test/sql/phase41_describe.test
- FOUND: .planning/phases/41-describe-rewrite/41-01-SUMMARY.md
- FOUND: commit 83bfed0 (Task 1)
- FOUND: commit 201cd79 (Task 2)

---
*Phase: 41-describe-rewrite*
*Completed: 2026-04-02*
