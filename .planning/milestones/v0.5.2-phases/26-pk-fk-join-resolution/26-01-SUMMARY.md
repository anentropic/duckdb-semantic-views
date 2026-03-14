---
phase: 26-pk-fk-join-resolution
plan: 01
subsystem: database
tags: [graph, validation, topological-sort, kahn-algorithm, pk-fk, define-time]

# Dependency graph
requires:
  - phase: 24-pk-fk-model
    provides: "TableRef.pk_columns, Join.from_alias, Join.fk_columns model fields"
  - phase: 25-sql-body-parser
    provides: "Parser producing Join structs with from_alias, fk_columns, table"
provides:
  - "RelationshipGraph struct with adjacency list and reverse edges"
  - "validate_graph() function for define-time graph validation"
  - "toposort() via Kahn's algorithm for deterministic join ordering"
  - "Six validation checks: self-ref, cycles, diamonds, orphans, FK/PK count, source reachability"
  - "Graph validation wired into both DefineSemanticViewVTab and DefineFromJsonVTab bind paths"
affects: [26-02, expand, query-expansion, join-resolution]

# Tech tracking
tech-stack:
  added: []
  patterns: [adjacency-list-graph, kahns-algorithm, define-time-validation]

key-files:
  created: [src/graph.rs]
  modified: [src/lib.rs, src/ddl/define.rs]

key-decisions:
  - "Kahn's algorithm for topological sort -- naturally detects cycles via leftover nodes"
  - "Adjacency list with reverse edges for O(1) parent-count diamond detection"
  - "All alias comparisons normalized to lowercase via to_ascii_lowercase()"
  - "Legacy definitions (empty fk_columns or empty tables) skip graph validation entirely"
  - "Graph validation runs before type inference and before persisting in both DDL paths"
  - "write! macro for string append to satisfy clippy::format_push_string pedantic lint"

patterns-established:
  - "Graph module pattern: standalone src/graph.rs with RelationshipGraph struct and validate_graph() entry point"
  - "Define-time validation pattern: validate after parse, before type inference, before persist"
  - "Legacy skip pattern: check has_pkfk_joins before running any graph validation"

requirements-completed: [EXP-06, EXP-03]

# Metrics
duration: 12min
completed: 2026-03-13
---

# Phase 26 Plan 01: Relationship Graph Validation Summary

**Define-time relationship graph module with Kahn's toposort, 6 validation checks (self-ref, cycles, diamonds, orphans, FK/PK count, source reachability), wired into both DDL paths**

## Performance

- **Duration:** 12 min (effective; wall clock included 5-min extension build)
- **Started:** 2026-03-13T13:11:49Z
- **Completed:** 2026-03-13T13:23:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Created `src/graph.rs` with `RelationshipGraph`, `validate_graph()`, `toposort()`, and all 6 validation functions
- 14 unit tests covering all error cases (self-ref, cycle, diamond, orphan, FK/PK mismatch, unreachable source table) and happy paths (valid tree, star schema, deterministic ordering, legacy skip, case insensitivity)
- Wired `validate_graph()` into both `DefineSemanticViewVTab::bind()` and `DefineFromJsonVTab::bind()` -- validation runs before type inference and before persisting
- Legacy definitions with empty `fk_columns` skip validation automatically -- all 324 existing tests pass unchanged

## Task Commits

Each task was committed atomically:

1. **Task 1: Create src/graph.rs with RelationshipGraph, validation, and topological sort** - `9e4c52f` (feat)
2. **Task 2: Wire validate_graph into define.rs for both DDL paths** - `2df3048` (feat)

## Files Created/Modified
- `src/graph.rs` - New module: RelationshipGraph struct, validate_graph(), toposort(), check_no_diamonds(), check_no_orphans(), check_fk_pk_counts(), check_source_tables_reachable(), find_cycle_path(), 14 unit tests
- `src/lib.rs` - Added `pub mod graph;` declaration
- `src/ddl/define.rs` - Added validate_graph() calls in both DefineSemanticViewVTab::bind() and DefineFromJsonVTab::bind()

## Decisions Made
- **Kahn's algorithm chosen** over DFS-based toposort: naturally detects cycles (leftover nodes after queue drains), simpler for this use case
- **Deterministic ordering** achieved by: (1) always seeding queue with root first, (2) sorting other zero-in-degree nodes, (3) sorting neighbors before processing
- **Adjacency list + reverse edges** enables O(1) diamond detection (check parent count in reverse map)
- **All comparisons lowercased** via `to_ascii_lowercase()` consistent with existing codebase patterns
- **Validation placement**: after parse/deserialize, before type inference, before persist -- catches invalid graphs before any expensive operations

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Clippy pedantic lints required backtick-escaping identifiers in doc comments, using `sort_unstable()` for primitive types, flipping `if_not_else` conditionals, and using `write!` instead of `push_str(&format!(...))`. All resolved inline.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- `RelationshipGraph` and `validate_graph()` are ready for Plan 02 to use in query expansion
- Plan 02 can call `graph.toposort()` for join ordering and use `graph.reverse` for transitive join resolution
- All validation is define-time; query-time code can assume the graph is valid

## Self-Check: PASSED

- [x] src/graph.rs exists
- [x] 26-01-SUMMARY.md exists
- [x] Commit 9e4c52f found
- [x] Commit 2df3048 found

---
*Phase: 26-pk-fk-join-resolution*
*Completed: 2026-03-13*
