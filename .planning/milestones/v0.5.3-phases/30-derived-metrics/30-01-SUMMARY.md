---
phase: 30-derived-metrics
plan: 01
subsystem: parser, validation
tags: [derived-metrics, body-parser, graph-validation, cycle-detection, aggregate-detection]

requires:
  - phase: 29-facts-clause-hierarchies
    provides: "fact parsing pattern (parse_qualified_entries), fact validation (cycle detection, word-boundary matching)"
provides:
  - "parse_metrics_clause: mixed qualified/unqualified metric parsing in body_parser.rs"
  - "contains_aggregate_function: aggregate function detection in expressions"
  - "validate_derived_metrics: CREATE-time validation (cycles, unknown refs, aggregates, duplicates)"
  - "Derived metrics stored with source_table: None in Metric struct"
affects: [30-02-derived-metric-expansion, 31-fan-trap-detection]

tech-stack:
  added: []
  patterns:
    - "Separate parse function per clause type (parse_metrics_clause vs parse_qualified_entries)"
    - "Identifier extraction with string-literal-aware scanning (extract_identifiers)"
    - "SQL keyword skip list for unknown-reference detection"

key-files:
  created: []
  modified:
    - src/body_parser.rs
    - src/graph.rs
    - src/ddl/define.rs

key-decisions:
  - "Separate parse_metrics_clause function instead of modifying parse_qualified_entries -- FACTS/DIMENSIONS still require qualified entries"
  - "Unknown reference detection via identifier extraction + SQL keyword skip list (not just word-boundary matching)"
  - "validate_derived_metrics refactored into 4 helper functions to satisfy clippy too_many_lines"

patterns-established:
  - "parse_metrics_clause: dot-presence determines qualified vs unqualified metric entry"
  - "contains_aggregate_function: word-boundary + paren-follows check with string-literal skipping"
  - "Derived metric DAG: only derived-to-derived edges for cycle detection; base metrics are external"

requirements-completed: [DRV-01, DRV-04, DRV-05]

duration: 13min
completed: 2026-03-14
---

# Phase 30 Plan 01: Derived Metric Parsing and Validation Summary

**Mixed qualified/unqualified METRICS clause parsing with CREATE-time validation for cycles, unknown references, aggregates, and duplicate names**

## Performance

- **Duration:** 13 min
- **Started:** 2026-03-14T13:37:15Z
- **Completed:** 2026-03-14T13:50:08Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- METRICS clause now accepts both `alias.name AS expr` (base) and `name AS expr` (derived) entries
- Derived metrics stored with `source_table: None` in Metric struct, enabling downstream expansion differentiation
- CREATE-time validation catches 4 error classes: duplicate names, aggregate functions, unknown references (with "did you mean?"), and cycles
- FACTS and DIMENSIONS clauses unchanged -- unqualified entries still rejected with clear errors
- 26 new tests added (10 parser + 16 validation), all 378 tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Parse mixed qualified/unqualified metric entries** - `a039d79` (feat)
2. **Task 2: Validate derived metrics at CREATE time** - `19d56c2` (feat)

## Files Created/Modified
- `src/body_parser.rs` - Added `parse_metrics_clause()` and `parse_single_metric_entry()` for mixed qualified/unqualified parsing; updated `parse_keyword_body()` to use new function for METRICS clause
- `src/graph.rs` - Added `contains_aggregate_function()`, `validate_derived_metrics()` with 4 helpers (`check_metric_name_uniqueness`, `check_no_aggregates_in_derived`, `check_derived_metric_references`, `check_derived_metric_cycles`), `extract_identifiers()`, `is_sql_keyword_or_builtin()`
- `src/ddl/define.rs` - Wired `validate_derived_metrics` into `bind()` after `validate_hierarchies`

## Decisions Made
- Created separate `parse_metrics_clause` function instead of modifying `parse_qualified_entries` -- FACTS and DIMENSIONS still require qualified (alias.name) format, and a typo (missing dot) must still produce an error
- Used identifier extraction + SQL keyword skip list for unknown reference detection, rather than only `find_fact_references` -- this catches references to names that don't exist at all (not just known names)
- Refactored `validate_derived_metrics` into 4 helper functions to satisfy clippy's `too_many_lines` lint

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Pre-commit rustfmt hook repeatedly reformatted code requiring re-staging -- resolved by running `cargo fmt` before staging
- Clippy pedantic lint (`too_many_lines`, `redundant_closure_for_method_calls`, `unnecessary_map_or`, `format_push_string`) required refactoring `validate_derived_metrics` into helper functions

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Derived metric parsing and validation complete -- ready for Plan 02 (expansion-time inlining)
- `source_table: None` on derived metrics enables Plan 02 to differentiate and inline at query time
- Validated DAG ordering enables safe topological-sort-based expansion in Plan 02

---
*Phase: 30-derived-metrics*
*Completed: 2026-03-14*
