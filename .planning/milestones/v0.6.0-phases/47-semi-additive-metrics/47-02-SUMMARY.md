---
phase: 47-semi-additive-metrics
plan: 02
subsystem: expand, query
tags: [semi-additive, row-number, cte, snapshot, fan-trap, sqllogictest]

# Dependency graph
requires:
  - phase: 47-semi-additive-metrics
    plan: 01
    provides: NonAdditiveDim/SortOrder/NullsOrder model types, body parser, render_ddl, DESCRIBE, test_helpers
  - phase: 42-module-refactoring
    provides: expand/ module directory structure with sql_gen.rs, fan_trap.rs, join_resolver.rs
provides:
  - CTE-based semi-additive expansion via expand_semi_additive()
  - ROW_NUMBER snapshot selection with PARTITION BY / ORDER BY
  - Effectively-regular classification (Snowflake semantics)
  - Mixed regular + semi-additive conditional aggregation
  - Fan trap exclusion for semi-additive metrics
  - End-to-end sqllogictest integration tests
affects: [expansion-pipeline, query-interface]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "CTE wrapping: WITH __sv_snapshot AS (...) SELECT ... FROM __sv_snapshot for pre-aggregation filtering"
    - "Conditional aggregation: SUM(CASE WHEN __sv_rn = 1 THEN val END) for snapshot-only metrics"
    - "Effectively-regular classification: skip CTE when all NA dims are in queried dims"
    - "extract_aggregate_inner/func helpers for parsing SUM(expr) -> (SUM, expr)"

key-files:
  created:
    - src/expand/semi_additive.rs
    - test/sql/phase47_semi_additive.test
  modified:
    - src/expand/mod.rs
    - src/expand/sql_gen.rs
    - src/expand/fan_trap.rs
    - test/sql/TEST_LIST

key-decisions:
  - "CTE approach with ROW_NUMBER() OVER for snapshot selection (not LAST_VALUE IGNORE NULLS due to DuckDB LTS crash bug)"
  - "Single CTE (__sv_snapshot) shared by all metrics with per-NA-group __sv_rn columns"
  - "Fan trap check skips semi-additive metrics entirely (ROW_NUMBER handles fan-out inherently)"
  - "Effectively-regular check uses queried_dim_names HashSet for O(1) membership test"

patterns-established:
  - "CTE wrapping pattern: semi_additive.rs generates WITH...SELECT independently of sql_gen.rs regular path"
  - "Dispatch pattern: has_active_semi_additive check before regular SELECT generation in expand()"
  - "Conditional aggregation: CASE WHEN __sv_rn = 1 for snapshot, plain agg for regular metrics in same query"

requirements-completed: [SEMI-01, SEMI-02, SEMI-03, SEMI-04, SEMI-05]

# Metrics
duration: 47min
completed: 2026-04-12
---

# Phase 47 Plan 02: Semi-Additive Expansion Pipeline Summary

**CTE-based ROW_NUMBER snapshot selection for semi-additive metrics with effectively-regular classification, mixed metric support, fan trap exclusion, and end-to-end integration tests**

## Performance

- **Duration:** 47 min
- **Started:** 2026-04-12T14:23:32Z
- **Completed:** 2026-04-12T15:10:19Z
- **Tasks:** 2
- **Files modified:** 6 (4 Rust source + 1 test + 1 test list)

## Accomplishments
- Created semi_additive.rs module with CTE-based expansion using ROW_NUMBER() OVER for snapshot row selection
- Implemented effectively-regular classification (Snowflake semantics: all NA dims in query -> standard aggregation)
- Mixed regular + semi-additive queries produce correct results via conditional CASE WHEN aggregation
- Fan trap detection skips semi-additive metrics (ROW_NUMBER inherently handles fan-out)
- 12 unit tests + 8 sqllogictest integration tests covering all expansion paths
- Full quality gate passes: 616 cargo tests + 29 sqllogictest files + DuckLake CI

## Task Commits

Each task was committed atomically:

1. **Task 1: Semi-additive expansion module and expand() dispatch** - `ea487d4` (feat)
2. **Task 2: End-to-end sqllogictest integration tests** - `0b5e0e8` (test)

## Files Created/Modified
- `src/expand/semi_additive.rs` - New module: CTE generation with ROW_NUMBER, collect_na_groups, extract_aggregate_inner/func helpers, 12 unit tests
- `src/expand/mod.rs` - Added `mod semi_additive;` declaration
- `src/expand/sql_gen.rs` - Added has_active_semi_additive dispatch before regular SELECT generation
- `src/expand/fan_trap.rs` - Added non_additive_by skip in check_fan_traps loop
- `test/sql/phase47_semi_additive.test` - 8 end-to-end test cases: DDL, snapshot query, effectively-regular, validation errors, mixed metrics, GET_DDL, DESCRIBE, global aggregate
- `test/sql/TEST_LIST` - Added phase47_semi_additive.test entry

## Decisions Made
- Used CTE approach with ROW_NUMBER() OVER (not LAST_VALUE IGNORE NULLS) per STATE.md decision about DuckDB LTS crash bug
- Single CTE (__sv_snapshot) shared by all metrics; each distinct NA group gets its own __sv_rn column
- Fan trap check skips semi-additive metrics entirely rather than applying modified logic
- expand() returns early to semi_additive path; regular path remains completely untouched for zero-regression risk

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed sqllogictest DATE column type specifier**
- **Found during:** Task 2 (sqllogictest execution)
- **Issue:** Plan used 'D' for DATE columns in query type declaration (TDR), but DuckDB sqllogictest only recognizes T, I, R
- **Fix:** Changed TDR to TTR (DATE renders as text in sqllogictest output)
- **Files modified:** test/sql/phase47_semi_additive.test
- **Committed in:** 0b5e0e8 (Task 2 commit)

**2. [Rule 1 - Bug] Fixed DESCRIBE function call syntax**
- **Found during:** Task 2 (sqllogictest execution)
- **Issue:** Plan used TABLE(DESCRIBE_SEMANTIC_VIEW('name')) syntax, but actual function is describe_semantic_view('name') without TABLE wrapper
- **Fix:** Changed to SELECT * FROM describe_semantic_view('name') matching existing test patterns
- **Files modified:** test/sql/phase47_semi_additive.test
- **Committed in:** 0b5e0e8 (Task 2 commit)

**3. [Rule 1 - Bug] Fixed DESCRIBE NON_ADDITIVE_BY expected value**
- **Found during:** Task 2 (test data analysis)
- **Issue:** Plan expected "report_date DESC" but the stored model has NullsOrder::First (DESC defaults to NULLS FIRST per Wave 1), so DESCRIBE emits "report_date DESC NULLS FIRST"
- **Fix:** Updated expected output to "report_date DESC NULLS FIRST"
- **Files modified:** test/sql/phase47_semi_additive.test
- **Committed in:** 0b5e0e8 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (3 bugs in plan test expectations)
**Impact on plan:** All fixes corrected plan test expectations to match actual system behavior. No scope creep.

## Issues Encountered
- Clippy pedantic caught 10 issues (doc_markdown, unnecessary_wraps, uninlined_format_args, map_unwrap_or, explicit_iter_loop) -- all resolved before commit
- Pre-commit rustfmt hook reformatted code on first commit attempt -- resolved by running cargo fmt before staging

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Semi-additive metrics are fully functional from DDL to query
- Phase 47 is complete: model types (Plan 01) + expansion pipeline (Plan 02)
- Ready for milestone-level verification and completion

## Self-Check: PASSED

All files found, all commits found, all content assertions verified.

---
*Phase: 47-semi-additive-metrics*
*Completed: 2026-04-12*
