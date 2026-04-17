---
phase: 48-window-function-metrics
plan: 02
subsystem: database
tags: [rust, duckdb, semantic-views, window-functions, expansion, cte]

requires:
  - phase: 48-window-function-metrics/01
    provides: WindowSpec model, OVER clause parser, MetricEntry 9-tuple, is_window() helper, with_window_spec builder
provides:
  - CTE-based window metric expansion (expand_window_metrics) in src/expand/window.rs
  - Window/aggregate mixing error (WindowAggregateMixing) blocking mixed queries
  - Required dimension validation (WindowMetricRequiredDimension) for EXCLUDING/ORDER BY dims
  - Fan trap skip for window metrics in check_fan_traps
  - SHOW DIMS FOR METRIC required=TRUE for window metric EXCLUDING/ORDER BY dimensions
  - End-to-end sqllogictest coverage for all window metric paths
affects: []

tech-stack:
  added: []
  patterns: [cte-window-expansion, partition-by-excluding-set-difference, window-aggregate-mixing-guard]

key-files:
  created:
    - src/expand/window.rs
    - test/sql/phase48_window_metrics.test
  modified:
    - src/expand/mod.rs
    - src/expand/sql_gen.rs
    - src/expand/types.rs
    - src/expand/fan_trap.rs
    - src/ddl/show_dims_for_metric.rs
    - test/sql/TEST_LIST

key-decisions:
  - "CTE named __sv_agg aggregates inner metrics by ALL queried dims; outer SELECT applies window functions with computed PARTITION BY"
  - "PARTITION BY EXCLUDING resolved at expansion time via set difference of queried dims minus excluded dims"
  - "Window metrics and aggregate metrics are mutually exclusive in the same query (WindowAggregateMixing error)"
  - "EXCLUDING and ORDER BY dimensions must be present in the query (WindowMetricRequiredDimension error)"
  - "SHOW DIMS FOR METRIC required field driven by window_spec EXCLUDING/ORDER BY dimension sets"

patterns-established:
  - "Window expansion module pattern: CTE aggregation + outer window SELECT (parallels semi_additive CTE pattern)"
  - "Required dimension validation: window metrics enforce dim presence at query time, not just define time"

requirements-completed: [WIN-02, WIN-03, WIN-04, WIN-05]

duration: 32min
completed: 2026-04-12
---

# Phase 48 Plan 02: Window Metric Expansion Pipeline + Integration Tests

**CTE-based window metric expansion with PARTITION BY EXCLUDING set difference, window/aggregate mixing guard, fan trap skip, SHOW DIMS required=TRUE, and end-to-end sqllogictest coverage**

## Performance

- **Duration:** 32 min
- **Started:** 2026-04-12T19:12:09Z
- **Completed:** 2026-04-12T19:44:09Z
- **Tasks:** 2 completed
- **Files modified:** 8

## Accomplishments
- Window metric expansion pipeline generates CTE with GROUP BY all dims + outer SELECT with window functions and computed PARTITION BY
- WindowAggregateMixing error blocks queries combining window and aggregate metrics with both metric lists
- WindowMetricRequiredDimension error validates EXCLUDING/ORDER BY dims are present in the query
- Fan trap check skips window metrics (pre-aggregated CTE handles fan-out)
- SHOW DIMS FOR METRIC returns required=TRUE for dimensions in window metric EXCLUDING/ORDER BY
- End-to-end sqllogictest covers DDL, AVG query, LAG query, mixing error, required dim error, SHOW DIMS, GET_DDL, DESCRIBE, fan trap skip

## Task Commits

1. **Task 1: Window metric expansion module, expand() dispatch, fan trap skip, and mixing error** - `827aae1` (feat)
2. **Task 2: SHOW DIMS required=TRUE and end-to-end integration tests** - `3e8ab28` (feat)

## Files Created/Modified
- `src/expand/window.rs` - CTE-based expansion for window function metrics (expand_window_metrics)
- `src/expand/mod.rs` - Added mod window declaration
- `src/expand/sql_gen.rs` - Window metric dispatch after semi-additive, WindowAggregateMixing guard
- `src/expand/types.rs` - WindowAggregateMixing and WindowMetricRequiredDimension error variants with Display
- `src/expand/fan_trap.rs` - is_window() skip in check_fan_traps
- `src/ddl/show_dims_for_metric.rs` - required field from window_spec EXCLUDING/ORDER BY dims
- `test/sql/phase48_window_metrics.test` - 9 end-to-end test cases (DDL, queries, errors, introspection)
- `test/sql/TEST_LIST` - Added phase48_window_metrics entry

## Decisions Made
- CTE named `__sv_agg` (distinct from semi-additive's `__sv_snapshot`) to avoid confusion when both patterns coexist
- PARTITION BY EXCLUDING resolved via HashSet difference at expansion time rather than storing pre-computed partition dims
- Window metrics validated at both define-time (Plan 01: EXCLUDING dims exist in view) and query-time (Plan 02: EXCLUDING dims in query request)
- DESCRIBE WINDOW_SPEC omits default ASC and NULLS LAST for compact display; GET_DDL always emits explicit NULLS for portability

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] EXCLUDING dims use bare names not qualified names in OVER clause**
- **Found during:** Task 2 (sqllogictest integration)
- **Issue:** Test DDL used qualified names like `d.date` in OVER EXCLUDING, but parser stores bare names and define-time validation matches against dimension names (bare)
- **Fix:** Used bare dimension names in OVER clauses in test DDL
- **Files modified:** test/sql/phase48_window_metrics.test
- **Committed in:** 3e8ab28 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Test data correction only, no code change needed. The parser/validation behavior is correct.

## Issues Encountered
- Pre-commit hook reformats code on first commit attempt, requiring re-stage and re-commit (standard workflow)
- Clippy pedantic flagged doc comment missing backticks around `AVG(total_qty)` -- fixed inline

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Window function metrics feature complete: DDL -> storage -> query -> results pipeline works end-to-end
- All WIN-01 through WIN-05 requirements satisfied across Plans 01 and 02
- Phase 48 ready for verification

---
*Phase: 48-window-function-metrics*
*Completed: 2026-04-12*
