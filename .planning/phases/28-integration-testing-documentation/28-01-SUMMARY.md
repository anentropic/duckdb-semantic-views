---
phase: 28-integration-testing-documentation
plan: 01
subsystem: ddl
tags: [vtab, function-ddl, code-removal, parse-args]

# Dependency graph
requires:
  - phase: 25-sql-body-parser
    provides: "DefineFromJsonVTab as backend for native DDL (AS-body)"
  - phase: 27-alias-based-query-expansion
    provides: "Paren-body DDL path already removed from parse.rs"
provides:
  - "Function DDL interface (create_semantic_view etc.) removed from source"
  - "DefineSemanticViewVTab struct and VTab impl removed"
  - "parse_args.rs deleted (function DDL argument parser)"
  - "Only native DDL path remains for CREATE operations"
affects: [28-02, 28-03]

# Tech tracking
tech-stack:
  added: []
  patterns: ["single-DDL-path: all CREATE operations route through native DDL -> DefineFromJsonVTab"]

key-files:
  created: []
  modified:
    - src/lib.rs
    - src/ddl/define.rs
    - src/ddl/mod.rs
  deleted:
    - src/ddl/parse_args.rs

key-decisions:
  - "Left function_name() CREATE arms as-is in parse.rs -- they are technically reachable (called before match rejects CREATE) so unreachable!() would panic"
  - "Cleaned up stale DefineSemanticViewVTab doc comment references in DefineFromJsonVTab"

patterns-established:
  - "Single DDL path: all CREATE semantic view operations go through native DDL -> parse.rs -> DefineFromJsonVTab"

requirements-completed: []

# Metrics
duration: 18min
completed: 2026-03-13
---

# Phase 28 Plan 01: Remove Function DDL Source Code Summary

**Removed DefineSemanticViewVTab, parse_args.rs, and 3 function DDL registrations -- only native DDL path remains for CREATE operations**

## Performance

- **Duration:** 18 min
- **Started:** 2026-03-13T18:05:32Z
- **Completed:** 2026-03-13T18:23:53Z
- **Tasks:** 2
- **Files modified:** 3 (+ 1 deleted)

## Accomplishments
- Removed the entire function-based CREATE DDL interface (create_semantic_view, create_or_replace_semantic_view, create_semantic_view_if_not_exists)
- Deleted parse_args.rs (235 lines of function DDL argument parsing code)
- Removed DefineSemanticViewVTab struct + 155-line VTab impl from define.rs
- All 282 tests pass (199 unit + 6 expand proptest + 36 output proptest + 35 parse proptest + 5 vector ref + 1 doctest)

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove function DDL registrations and imports from lib.rs** - `fcf7901` (feat)
2. **Task 2: Remove DefineSemanticViewVTab from define.rs, delete parse_args.rs, clean up mod.rs** - `cb1290a` (feat)

## Files Created/Modified
- `src/lib.rs` - Removed DefineSemanticViewVTab import; removed 3 function DDL registration blocks; restructured DefineState variables inline with _from_json registrations
- `src/ddl/define.rs` - Removed DefineSemanticViewVTab struct + VTab impl (155 lines); removed parse_args and expand imports; cleaned up stale doc comments
- `src/ddl/mod.rs` - Removed parse_args module declaration
- `src/ddl/parse_args.rs` - DELETED (235 lines, no remaining callers)

## Decisions Made
- Left function_name() CREATE arms as-is in parse.rs rather than replacing with unreachable!() -- the function IS called with CREATE variants (line 207, before the match on line 209 rejects them), so unreachable!() would cause a runtime panic. The match arms return unused strings, which is harmless.
- Cleaned up 3 stale doc comment references to DefineSemanticViewVTab in DefineFromJsonVTab's comments.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Source code cleanup complete; extension compiles with only the native DDL path for CREATE operations
- Plan 28-02 (test file rewrite/deletion) is unblocked -- SQL test files and Python integration tests still reference create_semantic_view() and will need updating
- Plan 28-03 (E2E integration test + README rewrite) is unblocked after 28-02

---
*Phase: 28-integration-testing-documentation*
*Completed: 2026-03-13*
