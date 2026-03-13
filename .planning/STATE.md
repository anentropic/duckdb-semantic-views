---
gsd_state_version: 1.0
milestone: v0.1
milestone_name: milestone
status: completed
stopped_at: Completed 28-01-PLAN.md
last_updated: "2026-03-13T18:23:53Z"
last_activity: 2026-03-13 -- Phase 28 Plan 01 complete (function DDL source code removal)
progress:
  total_phases: 6
  completed_phases: 4
  total_plans: 13
  completed_plans: 12
  percent: 80
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand -- the extension handles expansion, DuckDB handles execution.
**Current focus:** Phase 28 - Integration Testing & Documentation

## Current Position

Phase: 28 (5 of 5 in v0.5.2) -- In Progress
Plan: 1 of 3 in current phase (28-01 complete)
Status: Plan 28-01 complete (function DDL source code removal)
Last activity: 2026-03-13 -- Phase 28 Plan 01 complete (function DDL source code removal)

Progress: [████████░░] 80%

## Performance Metrics

**Velocity (v0.5.2, current):**
- Plans completed: 9 (25-01, 25-02, 25-03, 26-01, 26-02, 27-01, 27-02, 27-03, 28-01); 25-04 verified; Phases 25, 25.1, 26, 27 complete
- Timeline: 2026-03-11 to 2026-03-13 (ongoing)
- 28-01: 18 min / 2 tasks / 4 files
- 27-03: 6 min / 2 tasks / 4 files
- 27-02: 12 min / 2 tasks / 7 files
- 27-01: 12 min / 2 tasks / 4 files
- 26-02: 14 min / 2 tasks / 4 files
- 26-01: 12 min / 2 tasks / 3 files
- 25-01: 19 min / 3 tasks / 8 files
- 25-02: 8 min / 2 tasks / 1 file
- 25-03: 8 min / 2 tasks / 3 files
- 25-04: ~20 min / 2 tasks (auto) + 1 checkpoint / 5 files

**Velocity (v0.5.1):**
- Total plans completed: 9
- Phases: 5 (19-23)
- Timeline: 1 day (2026-03-09)

**Velocity (v0.5.0):**
- Total plans completed: 8
- Commits: 45
- Timeline: 2 days (2026-03-07 -> 2026-03-08)

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
All prior milestone decisions archived in milestones/ directories.

Recent decisions affecting current work:
- [v0.5.2 init]: NO backward compatibility needed -- pre-release, old syntax removed entirely
- [v0.5.2 init]: Snowflake semantic view syntax is the DDL grammar model
- [v0.5.2 init]: Zero new Cargo dependencies -- hand-written parser and graph traversal
- [25-01]: 16 KB validation path / 64 KB execution path buffer sizes for C++ DDL shim
- [25-01]: Phase 24 model fields (pk_columns, from_alias, fk_columns, name) added in 25-01 as Rule 3 auto-fix
- [25-01]: skip_serializing_if on all new model fields for backward-compatible JSON
- [Phase 25]: Single commit for Tasks 1+2: both operate on same file as coherent implementation unit
- [Phase 25]: allow(too_many_lines) on find_clause_bounds: state machine intentionally kept in one place for readability
- [Phase 25-sql-body-parser]: kind param added to validate_create_body for AS-body dispatch without global state
- [Phase 25-sql-body-parser]: DefineFromJsonVTab reuses DefineBindData/DefineInitData/DefineState; no new types needed
- [Phase 25-sql-body-parser]: JSON-bridge pattern: AS-body parsed in Rust, serialized to JSON, embedded in SELECT * FROM fn_from_json(name, json)
- [25-04]: sv_rewrite_ddl_rust must use validate_and_rewrite (not rewrite_ddl) to route both paren-body and AS-body DDL correctly
- [25-04]: sqllogictest tables use phase-prefixed names (p25_) to avoid cross-test catalog pollution
- [Phase 25-04]: sv_rewrite_ddl_rust must call validate_and_rewrite (not rewrite_ddl) to handle both paren-body and AS-body DDL through the same dispatch path
- [Phase 25.1-01]: Corpus seeds placed in fuzz/seeds/fuzz_ddl_parse/ (git-tracked) not fuzz/corpus/ (gitignored by .gitignore convention)
- [Phase 25.1-01]: TEST-08 adversarial tests use catch_unwind in standalone #[test] functions (not proptest blocks) for absolute invariant verification
- [Phase 25.1-01]: Fuzz target rejects invalid UTF-8 early with early return (not a panic/crash)
- [Phase 25.1-parser-robustness-security-hardening]: validate_and_rewrite calls detect_ddl_prefix on trimmed_no_semi so plen is relative to trimmed start; trim_offset + plen invariant preserved
- [Phase 25.1-parser-robustness-security-hardening]: prefix_len removed entirely (not kept as dead code); detect_ddl_prefix returns (kind, bytes) eliminating two-step detect + measure pattern
- [Phase 26-01]: Kahn's algorithm for toposort -- naturally detects cycles via leftover nodes, simpler than DFS for this use case
- [Phase 26-01]: Adjacency list + reverse edges for O(1) diamond detection (parent count in reverse map)
- [Phase 26-01]: Graph validation runs after parse, before type inference, before persist in both DDL paths
- [Phase 26-01]: Legacy definitions (empty fk_columns or empty tables) skip graph validation entirely
- [Phase 26-02]: CTE wrapper removed -- flat SELECT/FROM/JOIN pattern fixes table-qualified alias scoping for multi-table views
- [Phase 26-02]: Bidirectional join lookup: expand finds Join structs by either from_alias or table to handle FK source and FK target aliases
- [Phase 26-02]: LEFT JOIN is global for all definitions (PK/FK and legacy) per user decision
- [Phase 27-01]: resolve_joins() and append_join_on_clause() deleted -- resolve_joins_pkfk() is sole join path
- [Phase 27-01]: 11 legacy join unit tests deleted; phase4_query.test joined_orders updated to PK/FK DDL syntax
- [Phase 27-01]: create_semantic_view() function-based DDL does not populate fk_columns/from_alias -- join resolution only works with native DDL
- [Phase 27-02]: rewrite_ddl made private, rejects CREATE forms -- validate_and_rewrite is sole DDL entry point
- [Phase 27-02]: CLAUSE_KEYWORDS/suggest_clause_keyword removed from parse.rs (body_parser.rs has own copies)
- [Phase 27-02]: validate_create_body returns clear error for non-AS-body syntax with position
- [Phase 27]: Error message says 'Expected AS keyword' without referencing old syntax that was never released
- [Phase 28-01]: function_name() CREATE arms left as-is -- called before match rejects CREATE, so unreachable!() would panic
- [Phase 28-01]: DefineSemanticViewVTab and parse_args.rs removed; only DefineFromJsonVTab path remains for CREATE operations

### Pending Todos

None.

### Blockers/Concerns

- Research flag: verify `build_execution_sql` type-cast wrapper works with direct FROM+JOIN SQL (spike before Phase 27)
- C++ shim 4096-byte DDL buffer: RESOLVED in Phase 25 Plan 01 (upgraded to 64 KB heap allocation)

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 14 | Remove 3 orphaned backward-compat FFI exports | 2026-03-09 | ea84c9b | [14-remove-3-orphaned-backward-compat-ffi-ex](./quick/14-remove-3-orphaned-backward-compat-ffi-ex/) |
| 15 | Fix CI amalgamation auto-download | 2026-03-09 | 3859d68 | [15-check-gh-run-list-and-fix-the-failing-jo](./quick/15-check-gh-run-list-and-fix-the-failing-jo/) |
| Phase 25-sql-body-parser P03 | 8 | 2 tasks | 3 files |
| Phase 25-sql-body-parser P04 | 25 | 3 tasks | 5 files |
| Phase 25.1 P01 | 6 | 2 tasks | 8 files |
| Phase 25.1-parser-robustness-security-hardening P02 | 15 | 2 tasks | 2 files |
| Phase 27 P03 | 6 | 2 tasks | 4 files |

## Session Continuity

Last session: 2026-03-13T18:23:53Z
Stopped at: Completed 28-01-PLAN.md
Resume file: None
