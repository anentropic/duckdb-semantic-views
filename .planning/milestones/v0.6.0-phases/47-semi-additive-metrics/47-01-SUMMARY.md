---
phase: 47-semi-additive-metrics
plan: 01
subsystem: model, parser, ddl
tags: [semi-additive, non-additive-by, serde, body-parser, render-ddl, describe]

# Dependency graph
requires:
  - phase: 43-metadata-annotations
    provides: AccessModifier enum, COMMENT/SYNONYMS annotation parsing
  - phase: 45-alter-comment-get-ddl
    provides: render_ddl module for DDL reconstruction
provides:
  - NonAdditiveDim, SortOrder, NullsOrder model types
  - NON ADDITIVE BY parser in body_parser.rs
  - GET_DDL emission for NON ADDITIVE BY
  - DESCRIBE NON_ADDITIVE_BY property row
  - Define-time dimension reference validation
  - with_non_additive_by test helper builder
affects: [47-02-semi-additive-expansion, expand-pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "3-word keyword detection via find_keyword_ci + strip_prefix chain"
    - "DESC defaults to NULLS FIRST (matches DuckDB/Snowflake convention)"
    - "Always emit explicit NULLS in GET_DDL to avoid version divergence"

key-files:
  created: []
  modified:
    - src/model.rs
    - src/body_parser.rs
    - src/render_ddl.rs
    - src/ddl/describe.rs
    - src/expand/test_helpers.rs
    - src/expand/sql_gen.rs
    - src/graph/test_helpers.rs
    - tests/expand_proptest.rs

key-decisions:
  - "NON ADDITIVE BY extracted before USING in parse order (reverse of emission order) to handle both together"
  - "DESC defaults to NULLS FIRST when user does not specify NULLS (matches DuckDB/Snowflake)"
  - "GET_DDL always emits explicit NULLS LAST/FIRST even for defaults, avoiding version divergence"
  - "Define-time validation rejects NON ADDITIVE BY dim refs not in the view's dimension list"

patterns-established:
  - "3-word keyword detection: strip_prefix chain after find_keyword_ci for multi-word SQL keywords"
  - "Reverse extraction order: parse later keywords first to handle nesting (NON ADDITIVE BY before USING)"

requirements-completed: [SEMI-01]

# Metrics
duration: 31min
completed: 2026-04-12
---

# Phase 47 Plan 01: Semi-Additive Metrics Model and Parser Summary

**NON ADDITIVE BY model types (SortOrder/NullsOrder/NonAdditiveDim), DDL parsing with ASC/DESC/NULLS modifiers, GET_DDL round-trip, DESCRIBE surfacing, and define-time dimension validation**

## Performance

- **Duration:** 31 min
- **Started:** 2026-04-12T13:46:42Z
- **Completed:** 2026-04-12T14:18:00Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Added SortOrder, NullsOrder enums and NonAdditiveDim struct with serde backward compatibility
- Extended body parser to handle NON ADDITIVE BY (dim [ASC|DESC] [NULLS FIRST|LAST]) syntax with define-time validation
- GET_DDL correctly round-trips NON ADDITIVE BY clauses with explicit NULLS placement
- DESCRIBE surfaces NON_ADDITIVE_BY property row for metrics that have it
- 22 new tests covering model types, parsing, render_ddl, and validation

## Task Commits

Each task was committed atomically:

1. **Task 1: Model types and MetricEntry extension** - `f1a0527` (feat)
2. **Task 2: GET_DDL reconstruction, DESCRIBE surfacing, and define-time validation** - `d02c87e` (feat)

## Files Created/Modified
- `src/model.rs` - Added SortOrder, NullsOrder, NonAdditiveDim types; non_additive_by field on Metric
- `src/body_parser.rs` - Extended MetricEntry to 8-tuple; NON ADDITIVE BY parsing; define-time dimension validation
- `src/render_ddl.rs` - NON ADDITIVE BY emission in GET_DDL between USING and AS
- `src/ddl/describe.rs` - NON_ADDITIVE_BY property row in DESCRIBE output
- `src/expand/test_helpers.rs` - with_non_additive_by builder method; non_additive_by in all Metric literals
- `src/expand/sql_gen.rs` - non_additive_by: vec![] in all Metric struct literals
- `src/graph/test_helpers.rs` - non_additive_by: vec![] in all Metric struct literals
- `tests/expand_proptest.rs` - non_additive_by: vec![] in all Metric struct literals

## Decisions Made
- NON ADDITIVE BY extracted before USING in parse order to handle cases where both appear together
- DESC sort order defaults to NULLS FIRST when user does not explicitly specify NULLS placement
- GET_DDL always emits explicit NULLS LAST or NULLS FIRST even for defaults, to avoid DuckDB version divergence
- Define-time validation checks NON ADDITIVE BY dimension names case-insensitively against declared dimensions

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed NON ADDITIVE BY + USING parse order**
- **Found during:** Task 1 (GREEN phase)
- **Issue:** Initial implementation searched for NON ADDITIVE BY in `name_portion` (after USING extraction), but when USING appears before NON ADDITIVE BY in the text, the NAB clause was in the discarded portion
- **Fix:** Reversed extraction order: extract NON ADDITIVE BY from full `before_as` first, then extract USING from the remainder
- **Files modified:** src/body_parser.rs
- **Verification:** parse_metrics_non_additive_by_with_using test passes
- **Committed in:** f1a0527 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential for correctness when USING and NON ADDITIVE BY appear together. No scope creep.

## Issues Encountered
- Clippy pedantic caught 9 issues (manual_strip, format_push_string, uninlined_format_args, too_many_lines, doc_markdown) -- all resolved before commit
- Adding non_additive_by field to Metric struct required updating ~60 Metric struct literals across 5 files

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Model types and parser complete -- Plan 02 can implement the expansion pipeline against well-defined types
- NonAdditiveDim struct is ready for use in ROW_NUMBER CTE generation
- with_non_additive_by test helper available for expansion tests

## Self-Check: PASSED

All files found, all commits found, all content assertions verified.

---
*Phase: 47-semi-additive-metrics*
*Completed: 2026-04-12*
