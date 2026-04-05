---
phase: 38-module-directory-splits
plan: 02
subsystem: refactoring
tags: [rust, module-split, graph-validation, toposort]

# Dependency graph
requires:
  - phase: none
    provides: src/graph.rs (2,333-line monolithic file)
provides:
  - "src/graph/ module directory with 5 focused submodules + test_helpers"
  - "mod.rs re-exports preserving exact prior public API surface"
affects: [39-metadata-storage, 40-show-alignment, 41-describe-rewrite]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Split impl block pattern (toposort.rs extends RelationshipGraph)", "pub(super) test helpers shared across submodule test blocks"]

key-files:
  created:
    - src/graph/mod.rs
    - src/graph/relationship.rs
    - src/graph/toposort.rs
    - src/graph/facts.rs
    - src/graph/derived_metrics.rs
    - src/graph/using.rs
    - src/graph/test_helpers.rs
  modified: []

key-decisions:
  - "validate_fk_references stays private in relationship.rs (only called by validate_graph)"
  - "is_word_boundary_byte duplicated in facts.rs and derived_metrics.rs (private helper, avoids cross-module dependency for 2-line fn)"
  - "Phase 33 FK tests use super::super::validate_fk_references to access private fn from nested test module"

patterns-established:
  - "Module directory pattern: mod.rs with pub use re-exports, submodule #[cfg(test)] blocks, shared test_helpers.rs"
  - "Split impl block: toposort.rs defines impl RelationshipGraph in separate file from relationship.rs"

requirements-completed: [REF-02, REF-05]

# Metrics
duration: 25min
completed: 2026-04-01
---

# Phase 38 Plan 02: Graph Module Directory Split Summary

**Split src/graph.rs (2,333 lines) into 5 single-responsibility submodules with mod.rs re-exports, preserving exact public API and all 59 tests**

## Performance

- **Duration:** 25 min
- **Started:** 2026-04-01T21:18:33Z
- **Completed:** 2026-04-01T21:43:42Z
- **Tasks:** 2
- **Files modified:** 8 (1 deleted, 7 created)

## Accomplishments
- Decomposed graph.rs (2,333 lines) into 7 files totaling 2,403 lines (test code distributed)
- All 59 graph tests pass in their new submodule locations
- Full test suite (482 tests) passes with zero behavior changes
- No external consumer files modified (all crate::graph::* imports continue to work via mod.rs re-exports)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create graph/ production code submodules and mod.rs** - `c4a2e5b` (refactor)
2. **Task 2: Distribute graph tests and create test_helpers.rs** - `cf7834e` (test)

## Files Created/Modified
- `src/graph/mod.rs` - Module declarations and pub use re-exports (16 lines)
- `src/graph/relationship.rs` - RelationshipGraph struct, from_definition, check_no_diamonds, check_no_orphans, validate_fk_references, check_source_tables_reachable, validate_graph + 27 tests (923 lines)
- `src/graph/toposort.rs` - Split impl block with toposort() + find_cycle_path helper (112 lines)
- `src/graph/facts.rs` - find_fact_references, validate_facts, fact DAG helpers + 12 tests (408 lines)
- `src/graph/derived_metrics.rs` - contains_aggregate_function, validate_derived_metrics, extract_identifiers + 16 tests (600 lines)
- `src/graph/using.rs` - validate_using_relationships + 4 tests (155 lines)
- `src/graph/test_helpers.rs` - Shared make_def, make_def_with_facts, make_def_with_derived_metrics, make_def_with_named_joins (189 lines)
- `src/graph.rs` - Deleted (was 2,333 lines)

## Decisions Made
- `validate_fk_references` kept private in relationship.rs since it's only called by `validate_graph` in the same file
- `is_word_boundary_byte` duplicated in facts.rs and derived_metrics.rs rather than making it pub in a shared location -- it's a 2-line private helper, and deduplicating would require cross-submodule public visibility for no practical benefit
- Tests in the phase33_fk_reference_tests nested module access `validate_fk_references` via `super::super::` path since it's private to the parent module

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed import path for suggest_closest**
- **Found during:** Task 1
- **Issue:** Plan referenced `crate::util::suggest_closest` but the actual import path is `crate::expand::suggest_closest`
- **Fix:** Used the correct import path `crate::expand::suggest_closest` in all submodules
- **Files modified:** src/graph/relationship.rs, src/graph/facts.rs, src/graph/derived_metrics.rs
- **Committed in:** c4a2e5b (Task 1 commit)

**2. [Rule 1 - Bug] Removed unused import warning**
- **Found during:** Task 2
- **Issue:** `RelationshipGraph` imported but not used in relationship.rs tests (tests call `validate_graph` which returns the graph, but don't import the type directly)
- **Fix:** Removed unused `RelationshipGraph` from test imports
- **Files modified:** src/graph/relationship.rs
- **Committed in:** cf7834e (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Both fixes necessary for correctness. No scope creep.

## Issues Encountered
None

## Known Stubs
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- graph/ module directory complete with clean single-responsibility submodules
- All external import paths preserved via mod.rs re-exports
- Ready for Phase 39 (metadata storage) and beyond

## Self-Check: PASSED

All 7 created files verified present. Both commit hashes (c4a2e5b, cf7834e) verified in git log. src/graph.rs confirmed deleted.

---
*Phase: 38-module-directory-splits*
*Completed: 2026-04-01*
