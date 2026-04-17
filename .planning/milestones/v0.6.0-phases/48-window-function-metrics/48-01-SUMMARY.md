---
phase: 48-window-function-metrics
plan: 01
subsystem: database
tags: [rust, duckdb, semantic-views, window-functions, parser, serde]

requires:
  - phase: 47-semi-additive-metrics
    provides: NonAdditiveDim/SortOrder/NullsOrder model patterns, MetricEntry 8-tuple, fan trap skip pattern
provides:
  - WindowSpec and WindowOrderBy model types with serde backward compat
  - OVER clause parsing in body_parser (PARTITION BY EXCLUDING, ORDER BY, frame clause)
  - MetricEntry extended to 9-tuple with Option<WindowSpec>
  - GET_DDL reconstructs OVER clause from parsed WindowSpec
  - DESCRIBE surfaces WINDOW_SPEC property
  - Define-time validation of EXCLUDING dims and inner metric refs
  - is_window() helper on Metric
  - with_window_spec builder in test_helpers
affects: [48-02-window-expansion]

tech-stack:
  added: []
  patterns: [window-spec-model, over-clause-parsing, expression-rewriting]

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
  - "WindowSpec stored alongside raw expr for expansion-time OVER clause rewriting"
  - "OVER clause reconstructed from parsed spec in GET_DDL for normalized formatting"
  - "Window metrics and NON ADDITIVE BY are mutually exclusive (enforced at parse time)"
  - "Define-time validation checks EXCLUDING dims and inner metric references exist"

patterns-established:
  - "Window metric identification: metric.is_window() / window_spec.is_some()"
  - "MetricEntry 9-tuple pattern with Option<WindowSpec> as 9th element"

requirements-completed: [WIN-01]

duration: 35min
completed: 2026-04-12
---

# Phase 48 Plan 01: WindowSpec Model + OVER Clause Parser

**WindowSpec model types, OVER (PARTITION BY EXCLUDING) body parser, GET_DDL round-trip, DESCRIBE surfacing, and define-time validation for window function metrics**

## Performance

- **Duration:** 35 min
- **Tasks:** 2 completed
- **Files modified:** 8

## Accomplishments
- WindowSpec and WindowOrderBy structs with full serde backward compat (skip_serializing_if, default)
- OVER clause parsing: extracts window_function, inner_metric, extra_args, excluding_dims, order_by, frame_clause
- Mutual exclusion enforced: OVER + NON ADDITIVE BY produces parse error
- OVER on derived (unqualified) metrics produces clear error
- GET_DDL reconstructs OVER clause from parsed WindowSpec with explicit NULLS placement
- DESCRIBE emits WINDOW_SPEC property row
- Define-time validation of EXCLUDING dimension and inner metric references

## Task Commits

1. **Task 1: WindowSpec model types and MetricEntry extension** - `9bb8471` (feat)
2. **Task 2: GET_DDL emission, DESCRIBE surfacing, validation** - `46f4c8a` (feat)

## Files Created/Modified
- `src/model.rs` - WindowSpec, WindowOrderBy structs; window_spec field on Metric; is_window() helper
- `src/body_parser.rs` - OVER clause parsing, MetricEntry 9-tuple, mutual exclusion, define-time validation
- `src/render_ddl.rs` - OVER clause emission from parsed WindowSpec in GET_DDL
- `src/ddl/describe.rs` - WINDOW_SPEC property row
- `src/expand/test_helpers.rs` - with_window_spec builder
- `src/expand/sql_gen.rs` - window_spec: None in test Metric literals
- `src/graph/test_helpers.rs` - window_spec: None in test Metric literals
- `tests/expand_proptest.rs` - window_spec: None in Arbitrary Metric

## Decisions Made
- Reconstructed OVER clause from parsed WindowSpec in GET_DDL rather than using raw expression, for consistent formatting and explicit NULLS placement
- Stored extra_args as Vec<String> for LAG/LEAD offset arguments

## Deviations from Plan
None - plan executed as written

## Issues Encountered
- Rate limit hit after Task 1 commit; Task 2 was already implemented but uncommitted. Resumed and committed.

## Next Phase Readiness
- WindowSpec model and parsing foundation ready for Plan 02 expansion pipeline
- is_window() helper ready for fan trap skip and expansion dispatch

---
*Phase: 48-window-function-metrics*
*Completed: 2026-04-12*
