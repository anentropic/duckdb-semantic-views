---
phase: 27-alias-based-query-expansion
plan: 01
subsystem: query-expansion
tags: [qualified-refs, join-resolution, pkfk, cleanup]

# Dependency graph
requires:
  - phase: 26-pk-fk-join-resolution
    provides: "PK/FK graph-based join resolution (resolve_joins_pkfk)"
provides:
  - "EXP-05: Verified qualified column refs (alias.column) emit verbatim in SQL"
  - "CLN-03: Legacy resolve_joins() and append_join_on_clause() deleted"
  - "resolve_joins_pkfk() is the sole join resolution path"
affects: [27-02-plan, future multi-table query expansion]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Single join resolution path: resolve_joins_pkfk for all definitions"

key-files:
  created:
    - "test/sql/phase27_qualified_refs.test"
  modified:
    - "src/expand.rs"
    - "test/sql/phase4_query.test"
    - "test/sql/TEST_LIST"

key-decisions:
  - "Deleted 11 legacy join unit tests -- all tested empty fk_columns definitions incompatible with sole PK/FK path"
  - "Updated phase4_query.test joined_orders from create_semantic_view() to native CREATE SEMANTIC VIEW with PK/FK syntax"
  - "No backward compat for legacy join_columns definitions per STATE.md no-backward-compat policy"

patterns-established:
  - "All multi-table semantic views must use PK/FK DDL syntax (TABLES + RELATIONSHIPS with PRIMARY KEY and REFERENCES)"

requirements-completed: [EXP-01, EXP-05, CLN-02, CLN-03]

# Metrics
duration: 12min
completed: 2026-03-13
---

# Phase 27 Plan 01: Qualified Column Refs + Legacy Join Cleanup Summary

**Verified alias.column expressions emit verbatim in SQL and removed legacy substring-matching join resolution, leaving PK/FK graph as sole join path**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-13T15:46:33Z
- **Completed:** 2026-03-13T15:59:13Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- EXP-05 verified: Rust unit tests confirm expand() emits `c.name` and `sum(o.amount)` verbatim in SQL
- EXP-05 end-to-end: sqllogictest confirms qualified column refs resolve correctly through full extension pipeline
- CLN-03 complete: Deleted resolve_joins() (substring-matching heuristic) and append_join_on_clause() (legacy ON builder)
- Removed has_pkfk conditional -- resolve_joins_pkfk() is now unconditional
- Net code reduction: -694 lines, +61 lines (633 net lines removed)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add EXP-05 verification tests for qualified column references** - `42a3c9c` (test)
2. **Task 2: Remove legacy join resolution code (CLN-03)** - `f1f1520` (feat)

## Files Created/Modified
- `test/sql/phase27_qualified_refs.test` - New sqllogictest: qualified column refs end-to-end (2 test scenarios)
- `src/expand.rs` - Removed resolve_joins(), append_join_on_clause(), has_pkfk conditional; added Phase 27 unit tests
- `test/sql/phase4_query.test` - Updated joined_orders to native PK/FK DDL syntax
- `test/sql/TEST_LIST` - Added phase27_qualified_refs.test

## Decisions Made
- Deleted 11 legacy join unit tests that constructed definitions with empty fk_columns -- all tested the now-deleted legacy join resolution path
- Updated phase4_query.test joined_orders from function-based create_semantic_view() with join_columns to native CREATE SEMANTIC VIEW with PK/FK syntax, because create_semantic_view() does not populate fk_columns/from_alias fields needed by resolve_joins_pkfk()
- No backward compat maintained for legacy join_columns-only definitions per no-backward-compat policy (pre-release v0.5.x)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated phase4_query.test joined_orders to PK/FK DDL syntax**
- **Found during:** Task 2 (Remove legacy join resolution code)
- **Issue:** phase4_query.test used create_semantic_view() function-based DDL with join_columns, which never populates fk_columns/from_alias. After removing the legacy join path, resolve_joins_pkfk() returns empty for these definitions, dropping all joins.
- **Fix:** Rewrote joined_orders to use native CREATE SEMANTIC VIEW with TABLES/RELATIONSHIPS/PK/FK syntax. Updated cleanup to use DROP SEMANTIC VIEW.
- **Files modified:** test/sql/phase4_query.test
- **Verification:** just test-sql passes all 10 sqllogictest files
- **Committed in:** f1f1520 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to maintain test coverage after removing legacy join path. No scope creep.

## Issues Encountered
None -- plan executed with one blocking deviation handled via Rule 3.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- resolve_joins_pkfk() is the sole join resolution path -- clean foundation for Plan 02 (build_execution_sql type-cast verification)
- All tests pass via just test-all (cargo test + sqllogictest + DuckLake CI + fuzz + caret tests)

---
*Phase: 27-alias-based-query-expansion*
*Completed: 2026-03-13*
