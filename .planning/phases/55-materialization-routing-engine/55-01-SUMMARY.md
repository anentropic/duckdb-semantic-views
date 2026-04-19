---
phase: 55-materialization-routing-engine
plan: 01
subsystem: query-expansion
tags: [materialization, routing, hashset, sql-generation, pre-aggregation]

# Dependency graph
requires:
  - phase: 54-materialization-model-ddl
    provides: "Materialization struct, MATERIALIZATIONS clause parser, DDL persistence, YAML support"
provides:
  - "try_route_materialization() pure function for exact-match routing"
  - "build_materialized_sql() SQL generation for materialized queries"
  - "with_materialization() test fixture builder"
  - "Routing integration in expand() after step 3 (name resolution)"
  - "End-to-end sqllogictest coverage for routing, fallback, and exclusions"
affects: [phase-56-yaml-export, phase-57-introspection-extensions]

# Tech tracking
tech-stack:
  added: []
  patterns: [early-exit-routing, hashset-exact-match, pure-function-sql-generation]

key-files:
  created:
    - src/expand/materialization.rs
    - test/sql/phase55_materialization_routing.test
  modified:
    - src/expand/mod.rs
    - src/expand/sql_gen.rs
    - src/expand/test_helpers.rs
    - test/sql/TEST_LIST

key-decisions:
  - "Routing placed after step 3 (name resolution) and before step 4 (fact toposort) in expand() -- routing checks semi-additive/window exclusions internally"
  - "HashSet<String> with to_ascii_lowercase() for case-insensitive exact set matching -- consistent with codebase pattern"
  - "output_type casts applied in materialized SQL to maintain type consistency with raw expansion"
  - "QueryRequest DimensionName/MetricName newtypes used in end-to-end tests (discovered during TDD)"

patterns-established:
  - "Early-exit routing: try_route_materialization() returns Option<String> as early exit in expand()"
  - "Pure function routing: no side effects, no DB access -- takes definition + resolved names, returns SQL or None"

requirements-completed: [MAT-02, MAT-03, MAT-04, MAT-05]

# Metrics
duration: 18min
completed: 2026-04-19
---

# Phase 55 Plan 01: Materialization Routing Engine Summary

**Pure-function materialization routing engine with exact-match HashSet comparison, semi-additive/window exclusion, and 17 unit + 8 integration test sections**

## Performance

- **Duration:** 18 min
- **Started:** 2026-04-19T15:52:52Z
- **Completed:** 2026-04-19T16:11:16Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Implemented `try_route_materialization()` pure function that transparently routes queries to pre-aggregated tables when materializations exactly cover requested dimensions and metrics
- Integrated routing as early-exit path in `expand()` after name resolution -- zero behavior change for views without materializations
- Semi-additive and window metrics unconditionally excluded from routing (MAT-04)
- Full end-to-end verification via 8 sqllogictest sections proving routing, fallback, and exclusion through the DDL -> query pipeline

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement materialization routing module with unit tests** - `d8d5e9f` (feat)
2. **Task 2: Sqllogictest integration tests and full suite verification** - `f5be72a` (test)

## Files Created/Modified
- `src/expand/materialization.rs` - New module: `try_route_materialization()` routing function, `build_materialized_sql()` SQL generator, 17 unit tests
- `src/expand/mod.rs` - Added `mod materialization;` declaration
- `src/expand/sql_gen.rs` - Inserted routing call after step 3 (name resolution) as early-exit path
- `src/expand/test_helpers.rs` - Added `with_materialization()` builder method to `TestFixtureExt` trait, added `Materialization` import
- `test/sql/phase55_materialization_routing.test` - 8 integration test sections: exact-match routing, no-match fallback, dimensions-only no-route, no-materializations transparency, semi-additive exclusion, window exclusion, first-match-wins, case-insensitive matching
- `test/sql/TEST_LIST` - Added phase55 test entry

## Decisions Made
- Routing placement: after step 3 (metric resolution) and before step 4 (fact toposort) in `expand()`. The routing function internally checks for semi-additive/window exclusions, so the placement is safe regardless of downstream dispatch paths.
- Case-insensitive matching via `HashSet<String>` with `to_ascii_lowercase()` -- matches the codebase's existing pattern (e.g., `queried_dim_names` in sql_gen.rs).
- Output type casts applied in materialized SQL via `build_materialized_sql()` -- maintains type consistency with raw expansion path.
- Used `DimensionName`/`MetricName` newtypes in end-to-end tests (discovered during TDD that `QueryRequest` uses these newtypes, not plain strings).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed QueryRequest type mismatch in end-to-end tests**
- **Found during:** Task 1 (TDD RED phase)
- **Issue:** Plan specified `vec!["region".to_string()]` for QueryRequest fields, but QueryRequest uses `DimensionName`/`MetricName` newtypes, not `String`
- **Fix:** Changed to `vec![DimensionName::new("region")]` and `vec![MetricName::new("total_revenue")]`
- **Files modified:** `src/expand/materialization.rs`
- **Verification:** `cargo test materialization` passes
- **Committed in:** d8d5e9f (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor type correction in test code. No scope change.

## Issues Encountered
- Pre-commit hook applies rustfmt formatting that differs from hand-written style -- resolved by running `cargo fmt` before staging. Standard workflow.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Materialization routing engine is complete and tested
- All 834+ tests pass (Rust unit + proptest + 34 sqllogictest + 6 DuckLake CI)
- Ready for Phase 56 (YAML Export) which needs to include materializations in YAML output
- Ready for Phase 57 (Introspection Extensions) which may surface routing decisions in explain output

## Self-Check: PASSED

All files exist, all commits verified, all content markers present.

---
*Phase: 55-materialization-routing-engine*
*Completed: 2026-04-19*
