---
phase: 46-wildcard-selection-queryable-facts
plan: 02
subsystem: query
tags: [facts, expansion, unaggregated, path-validation, fan-trap, inline-facts]

# Dependency graph
requires:
  - phase: 46-wildcard-selection-queryable-facts
    provides: QueryRequest.facts field, ExpandError variants (UnknownFact, DuplicateFact, FactPathViolation, PrivateFact), wildcard expansion for facts
provides:
  - expand_facts() function for unaggregated fact SQL generation
  - validate_fact_table_path() for linear path constraint enforcement
  - Fact query dispatch from expand() when req.facts is non-empty
  - LIMIT 0 type inference for fact queries in VTab bind()
  - End-to-end fact queries via semantic_view() and explain_semantic_view()
affects: [query-expansion, fact-query-mode, semantic-view-vtab]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Fact expansion as separate code path from metric expansion (no GROUP BY, no aggregation)"
    - "Table path validation reusing ancestors_to_root from fan trap infrastructure"
    - "LIMIT 0 type inference branch for fact queries (separate from DDL-time primary path)"

key-files:
  created:
    - test/sql/phase46_fact_query.test
  modified:
    - src/expand/sql_gen.rs
    - src/expand/fan_trap.rs
    - src/expand/test_helpers.rs
    - src/query/table_function.rs
    - test/sql/TEST_LIST

key-decisions:
  - "Fact queries use LIMIT 0 type inference rather than DDL-time type map (facts use per-fact output_type, not column_types_inferred)"
  - "expand_facts is a private function dispatched from expand(), not a separate public entry point"
  - "Table path validation is pairwise ancestor check using existing ancestors_to_root infrastructure"

patterns-established:
  - "Fact expansion separate from metric expansion: expand_facts() follows same SELECT/FROM/JOIN pattern but without GROUP BY or aggregation"
  - "Fact alias deduplication: fact_aliases Vec checked with contains() before pushing"

requirements-completed: [FACT-01, FACT-02, FACT-04]

# Metrics
duration: 53min
completed: 2026-04-12
---

# Phase 46 Plan 02: Queryable FACTS Expansion Summary

**Fact query expansion path producing unaggregated row-level SQL with DAG inlining, table path validation via ancestors_to_root, and LIMIT 0 type inference for fact columns**

## Performance

- **Duration:** 53 min
- **Started:** 2026-04-12T01:44:53Z
- **Completed:** 2026-04-12T02:38:01Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- expand_facts() generates unaggregated SQL for fact queries (SELECT without GROUP BY), with DAG-resolved fact expressions via inline_facts
- validate_fact_table_path() enforces linear path constraint for cross-table fact queries, reusing ancestors_to_root from fan trap infrastructure
- Fact queries work end-to-end through semantic_view() and explain_semantic_view() with correct LIMIT 0 type inference
- PRIVATE facts rejected, unknown facts error with did-you-mean suggestions, duplicate facts caught
- 12 integration test scenarios via sqllogictest covering all FACT requirements
- All 582 Rust + 28 SQL + 6 DuckLake + 3 caret tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Fact expansion path and table path validation** - `8ebdf1b` (test), `8251d3d` (feat)
2. **Task 2: Fact query type resolution and integration tests** - `1c62472` (feat)

_Task 1 used TDD: RED commit (failing tests) then GREEN commit (implementation)_

## Files Created/Modified
- `src/expand/sql_gen.rs` - Added expand_facts() function and dispatch from expand(); 9 unit tests in phase46_fact_query_tests module
- `src/expand/fan_trap.rs` - Added validate_fact_table_path() for linear path constraint enforcement
- `src/expand/test_helpers.rs` - Added with_pkfk_join() builder for PK/FK join test fixtures
- `src/query/table_function.rs` - Fact query LIMIT 0 type inference branch; removed stale fact loop from metrics-only fallback
- `test/sql/phase46_fact_query.test` - 12 integration test scenarios: basic, dims+facts, mutual exclusion, path validation, PRIVATE, unknown, DAG inlining, explain, wildcard
- `test/sql/TEST_LIST` - Added phase46_fact_query.test

## Decisions Made
- **LIMIT 0 type inference for facts**: Facts use per-fact `output_type` strings which are not in the `column_types_inferred` map built at DDL time. Rather than mapping output_type strings to DuckDB type IDs at bind time, the simpler correct approach is to run a LIMIT 0 query and let DuckDB infer the types from the actual expanded SQL expressions.
- **expand_facts as private function**: Rather than a separate public entry point, `expand_facts()` is called internally by `expand()` when `req.facts` is non-empty. This preserves the single public API surface (`expand()`) and keeps mutual exclusion/empty request checks in one place.
- **Pairwise ancestor check for path validation**: For n unique tables, checking all O(n^2) pairs with `ancestors_to_root` is simple and correct. Typical views have <10 tables so the quadratic cost is negligible.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed duplicate JOIN for multiple facts on same table**
- **Found during:** Task 2
- **Issue:** When two facts reference the same source table (e.g., `net_price` and `line_total` both on `li`), the `fact_aliases` loop pushed the alias twice, producing duplicate `LEFT JOIN` clauses that caused DuckDB's "Ambiguous reference" binder error
- **Fix:** Added `!fact_aliases.contains(&lower)` deduplication check before pushing
- **Files modified:** src/expand/sql_gen.rs
- **Committed in:** 1c62472 (Task 2 commit)

**2. [Rule 3 - Blocking] Added with_pkfk_join test helper**
- **Found during:** Task 1
- **Issue:** Tests needed PK/FK join fixtures with `from_alias`, `fk_columns`, and `ref_columns` fields, but no builder existed for this pattern
- **Fix:** Added `with_pkfk_join()` to `TestFixtureExt` trait and implementation
- **Files modified:** src/expand/test_helpers.rs
- **Committed in:** 8ebdf1b (Task 1 RED commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness. No scope creep.

## Issues Encountered
- clippy::doc_markdown on `source_table` in doc comment -- resolved with backtick quoting
- clippy::too_many_lines on expand_facts -- resolved with `#[allow]` annotation
- clippy::unnecessary_map_or on `map_or(true, ...)` -- resolved by switching to `is_none_or()`
- clippy::manual_contains on `iter().any(|a| *a == lower)` -- resolved by using `contains()`

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Fact query expansion is complete: users can query row-level facts alongside dimensions
- All FACT requirements (FACT-01, FACT-02, FACT-04) validated via integration tests
- FACT-03 (mutual exclusion) was implemented in Plan 01 and verified again in Plan 02's integration tests
- Phase 46 is complete: both wildcard selection and queryable FACTS are shipped

## Self-Check: PASSED

- All 6 key files verified to exist on disk
- All 3 task commit hashes verified in git log
- Full quality gate (just test-all) passed: 582 Rust + 28 SQL + 6 DuckLake + 3 caret

---
*Phase: 46-wildcard-selection-queryable-facts*
*Completed: 2026-04-12*
