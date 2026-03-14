---
phase: 26-pk-fk-join-resolution
plan: 02
subsystem: database
tags: [expand, join-resolution, left-join, pk-fk, graph, topological-sort, sqllogictest]

# Dependency graph
requires:
  - phase: 26-pk-fk-join-resolution
    plan: 01
    provides: "RelationshipGraph, validate_graph(), toposort() for graph-based join ordering"
  - phase: 24-pk-fk-model
    provides: "TableRef.pk_columns, Join.from_alias, Join.fk_columns model fields"
provides:
  - "Graph-based resolve_joins_pkfk() for transitive join resolution"
  - "synthesize_on_clause() for PK/FK ON clause generation"
  - "LEFT JOIN emission for all definitions (PK/FK and legacy)"
  - "Flat query pattern replacing CTE wrapper for correct alias scoping"
  - "End-to-end integration tests for PK/FK join resolution"
affects: [query-expansion, build_execution_sql, future-phases]

# Tech tracking
tech-stack:
  added: []
  patterns: [flat-query-expansion, bidirectional-join-lookup, graph-transitive-resolution]

key-files:
  created: [test/sql/phase26_join_resolution.test]
  modified: [src/expand.rs, test/sql/TEST_LIST, tests/expand_proptest.rs]

key-decisions:
  - "CTE wrapper removed: flat SELECT/FROM/JOIN pattern fixes table-qualified alias scoping for multi-table views"
  - "Bidirectional join lookup: expand finds Join structs by either from_alias or table to handle both FK source and FK target aliases"
  - "LEFT JOIN is global: both PK/FK and legacy paths emit LEFT JOIN (per user decision)"
  - "Flat query is semantically equivalent to CTE for build_execution_sql wrapping (subquery pattern unchanged)"

patterns-established:
  - "Flat expansion pattern: SELECT ... FROM base_table AS alias LEFT JOIN ... WHERE ... GROUP BY (no CTE)"
  - "Bidirectional join lookup: find Join where from_alias == alias OR table == alias"
  - "Graph-based transitive resolution: walk reverse edges from needed aliases to root, include intermediaries"

requirements-completed: [EXP-02, EXP-04]

# Metrics
duration: 14min
completed: 2026-03-13
---

# Phase 26 Plan 02: PK/FK Join Resolution Summary

**Graph-based PK/FK join resolution with synthesized ON clauses, transitive inclusion, LEFT JOIN emission, and flat query expansion replacing CTE wrapper**

## Performance

- **Duration:** 14 min
- **Started:** 2026-03-13T14:23:14Z
- **Completed:** 2026-03-13T14:38:03Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Graph-based `resolve_joins_pkfk()` with transitive join inclusion via reverse edge walking
- `synthesize_on_clause()` generates `from_alias.fk = to_alias.pk` pairs from PK/FK declarations
- Replaced CTE wrapper with flat SELECT/FROM/JOIN pattern to fix alias scoping for multi-table views
- All joins (PK/FK and legacy) now emit LEFT JOIN per user decision
- End-to-end integration tests: basic 2-table join, transitive 3-table join, pruning, LEFT JOIN NULL preservation, graph validation errors

## Task Commits

Each task was committed atomically:

1. **Task 1: Update expand.rs with graph-based PK/FK join resolution** - `c123c0f` (test, RED), `1f26983` (feat, GREEN)
2. **Task 2: Add sqllogictest integration tests + CTE fix** - `b5e1fdd` (feat)

## Files Created/Modified
- `src/expand.rs` - Added `synthesize_on_clause()`, `resolve_joins_pkfk()`, PK/FK branch in `expand()`, LEFT JOIN for legacy path, flat query pattern (CTE removed)
- `test/sql/phase26_join_resolution.test` - 5 integration tests: basic PK/FK, transitive 3-table, pruning, LEFT JOIN NULL, graph errors
- `test/sql/TEST_LIST` - Registered phase26_join_resolution.test
- `tests/expand_proptest.rs` - Updated `sql_structure_valid` property for flat query format

## Decisions Made
- **CTE wrapper removed:** The `WITH "_base" AS (SELECT * FROM ...)` pattern hid table aliases from the outer SELECT, making table-qualified expressions like `c.name` unresolvable. Replaced with flat `SELECT ... FROM base_table AS alias LEFT JOIN ...` pattern. This is semantically equivalent and `build_execution_sql` still wraps expanded SQL as a subquery.
- **Bidirectional join lookup:** When finding the Join struct for a needed alias, we check both `from_alias == alias` (FK source) and `table == alias` (FK target). This handles cases where a table is the FK holder (e.g., `li(order_id) REFERENCES o`) -- `li` appears as `from_alias`, not `table`.
- **LEFT JOIN is global:** Both PK/FK and legacy paths emit LEFT JOIN. No INNER JOIN ever used.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] CTE wrapper broke table-qualified expression scoping**
- **Found during:** Task 2 (sqllogictest integration)
- **Issue:** CTE `WITH "_base" AS (SELECT * FROM t1 AS o LEFT JOIN t2 AS c ON ...)` hid aliases `o` and `c` from outer SELECT. Expressions like `c.name` failed with "Referenced table c not found"
- **Fix:** Removed CTE wrapper, emit flat `SELECT ... FROM base_table AS alias LEFT JOIN ...`. Updated 8 existing unit tests and 1 proptest for new format.
- **Files modified:** src/expand.rs, tests/expand_proptest.rs
- **Verification:** 54 unit tests pass, 6 proptests pass, 9 sqllogictest files pass, just test-all passes
- **Committed in:** b5e1fdd

**2. [Rule 1 - Bug] Join lookup only checked FK target side**
- **Found during:** Task 2 (3-table transitive test)
- **Issue:** `def.joins.find(|j| j.table == alias)` only matched aliases as FK targets. Tables that are FK sources (e.g., `li` in `li(order_id) REFERENCES o`) have `from_alias == "li"` not `table == "li"`.
- **Fix:** Changed lookup to check both `j.table == alias || j.from_alias == alias`
- **Files modified:** src/expand.rs
- **Verification:** 3-table transitive join test passes end-to-end
- **Committed in:** b5e1fdd

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Both fixes necessary for end-to-end correctness. CTE removal is a simplification that also fixes a latent bug affecting all multi-table views. No scope creep.

## Issues Encountered
- Clippy pedantic lint `manual_let_else` required converting `match ... { Ok(g) => g, Err(_) => return ... }` to `let Ok(g) = ... else { return ... }`. Resolved inline.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- PK/FK join resolution is complete end-to-end (define-time validation + query-time expansion)
- Phase 26 requirements EXP-02, EXP-04, EXP-06, EXP-03 all satisfied across Plans 01 and 02
- Flat query pattern works correctly with build_execution_sql subquery wrapping
- Legacy definitions (join_columns, on string) continue to work with LEFT JOIN

## Self-Check: PASSED

- [x] test/sql/phase26_join_resolution.test exists
- [x] 26-02-SUMMARY.md exists
- [x] Commit c123c0f found (TDD RED)
- [x] Commit 1f26983 found (TDD GREEN)
- [x] Commit b5e1fdd found (integration tests + CTE fix)

---
*Phase: 26-pk-fk-join-resolution*
*Completed: 2026-03-13*
