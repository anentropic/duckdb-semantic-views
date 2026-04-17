---
phase: 46-wildcard-selection-queryable-facts
plan: 01
subsystem: query
tags: [wildcard, expansion, facts, vtab, access-modifier, deduplication]

# Dependency graph
requires:
  - phase: 43-metadata-annotations
    provides: PRIVATE/PUBLIC access modifiers on metrics and facts
provides:
  - expand_wildcards() function for table_alias.* pattern expansion
  - WildcardItemType enum for dimension/metric/fact dispatch
  - QueryRequest.facts field for fact query mode
  - FactsMetricsMutualExclusion, UnknownFact, DuplicateFact, FactPathViolation error variants
  - facts named parameter on semantic_view() and explain_semantic_view()
  - Wildcard expansion in both VTab bind() paths
affects: [46-02-PLAN, query-expansion, fact-query-mode]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Wildcard expansion as pre-processing step before expand() call"
    - "Shared wildcard module in expand/ accessible by extension-gated VTab code"

key-files:
  created:
    - src/expand/wildcard.rs
    - test/sql/phase46_wildcard.test
  modified:
    - src/expand/types.rs
    - src/expand/sql_gen.rs
    - src/expand/test_helpers.rs
    - src/expand/mod.rs
    - src/query/table_function.rs
    - src/query/explain.rs
    - src/query/error.rs
    - src/ddl/define.rs
    - tests/expand_proptest.rs
    - fuzz/fuzz_targets/fuzz_sql_expand.rs
    - fuzz/fuzz_targets/fuzz_query_names.rs
    - test/sql/TEST_LIST

key-decisions:
  - "Wildcard module placed in expand/ rather than query/ so unit tests run without extension feature"
  - "expand_wildcards takes &WildcardItemType reference to avoid moving enum value"

patterns-established:
  - "Wildcard expansion as pre-processing: expand_wildcards() runs before expand(), transforming user input into concrete names"

requirements-completed: [WILD-01, WILD-02, WILD-03, FACT-03]

# Metrics
duration: 69min
completed: 2026-04-12
---

# Phase 46 Plan 01: Wildcard Selection + Facts Interface Summary

**table_alias.* wildcard expansion for dims/metrics/facts with PRIVATE filtering, facts named parameter on both VTabs, and facts/metrics mutual exclusion enforcement**

## Performance

- **Duration:** 69 min
- **Started:** 2026-04-12T00:31:46Z
- **Completed:** 2026-04-12T01:40:46Z
- **Tasks:** 3
- **Files modified:** 14

## Accomplishments
- Wildcard expansion (table_alias.*) works in dimensions and metrics parameters, expanding to all matching names scoped to the alias while excluding PRIVATE items
- QueryRequest extended with facts field; four new ExpandError variants for fact-mode errors
- Facts named parameter registered on both semantic_view() and explain_semantic_view() VTabs
- Full integration test coverage via sqllogictest (8 test scenarios)
- All 573 Rust tests + 27 SQL logic tests + 13 DuckLake tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend QueryRequest, add error variants, update expand()** - `dfe5b1f` (feat)
2. **Task 2: Wildcard expansion function and VTab parameter registration** - `3ddbec2` (feat)
3. **Task 3: Wildcard integration tests via sqllogictest** - `f8feadc` (test)

## Files Created/Modified
- `src/expand/wildcard.rs` - New module: expand_wildcards() and WildcardItemType enum with 7 unit tests
- `src/expand/types.rs` - Added facts field to QueryRequest, 4 new ExpandError variants with Display impls
- `src/expand/sql_gen.rs` - Mutual exclusion check, updated EmptyRequest guard, 5 new unit tests
- `src/expand/test_helpers.rs` - Added with_private_metric and with_private_fact builders
- `src/expand/mod.rs` - Registered wildcard submodule
- `src/query/table_function.rs` - facts parameter extraction, wildcard expansion, fact type inference
- `src/query/explain.rs` - facts parameter, wildcard expansion, Facts header in explain output
- `src/query/error.rs` - Updated EmptyRequest Display to mention facts
- `src/ddl/define.rs` - Updated QueryRequest construction with facts field
- `tests/expand_proptest.rs` - Updated QueryRequest construction with facts field
- `fuzz/fuzz_targets/fuzz_sql_expand.rs` - Updated QueryRequest construction with facts field
- `fuzz/fuzz_targets/fuzz_query_names.rs` - Updated QueryRequest construction with facts field
- `test/sql/phase46_wildcard.test` - 8 integration test scenarios for wildcard and facts features
- `test/sql/TEST_LIST` - Added phase46_wildcard.test

## Decisions Made
- **Wildcard module in expand/ not query/**: expand_wildcards() is a pure function with no FFI dependency. Placing it in expand/ allows unit tests to run with `cargo test` (no extension feature needed), while the extension-gated VTab code imports it via `crate::expand::wildcard`.
- **Reference semantics for WildcardItemType**: `expand_wildcards` takes `&WildcardItemType` rather than consuming the enum, allowing callers to reuse the variant.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated QueryRequest in fuzz targets and proptest**
- **Found during:** Task 1
- **Issue:** Fuzz targets and proptest construct QueryRequest directly; adding the facts field caused compilation failures
- **Fix:** Added `facts: vec![]` to all QueryRequest constructions in fuzz/fuzz_targets/ and tests/expand_proptest.rs
- **Files modified:** fuzz/fuzz_targets/fuzz_sql_expand.rs, fuzz/fuzz_targets/fuzz_query_names.rs, tests/expand_proptest.rs
- **Committed in:** dfe5b1f (Task 1 commit)

**2. [Rule 3 - Blocking] Moved expand_wildcards to expand/ module**
- **Found during:** Task 2
- **Issue:** Plan placed expand_wildcards in query/table_function.rs, but that module is gated behind the extension feature. Unit tests in `cargo test` (default features) could not see the function.
- **Fix:** Created src/expand/wildcard.rs as a shared module accessible to both non-extension tests and extension-gated VTab code
- **Files modified:** src/expand/wildcard.rs (created), src/expand/mod.rs, src/query/table_function.rs, src/query/explain.rs
- **Committed in:** 3ddbec2 (Task 2 commit)

**3. [Rule 1 - Bug] Fixed DDL syntax in sqllogictest**
- **Found during:** Task 3
- **Issue:** Plan's suggested DDL omitted parentheses around TABLES/RELATIONSHIPS/DIMENSIONS/METRICS clauses and used wrong RELATIONSHIPS format
- **Fix:** Added parentheses around clause bodies and used correct `rel_name AS from_alias(fk_cols) REFERENCES to_alias` syntax
- **Files modified:** test/sql/phase46_wildcard.test
- **Committed in:** f8feadc (Task 3 commit)

---

**Total deviations:** 3 auto-fixed (2 blocking, 1 bug)
**Impact on plan:** All auto-fixes necessary for correctness. No scope creep.

## Issues Encountered
- clippy::too_many_lines on ExpandError Display impl after adding 4 new variants -- resolved with #[allow] annotation
- dead_code warning on wildcard module in non-extension builds -- resolved with #[allow(dead_code)] on module declaration

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- QueryRequest.facts field is ready for Plan 02's fact query expansion
- UnknownFact, DuplicateFact, FactPathViolation error variants are ready for Plan 02
- Wildcard expansion infrastructure works for facts via WildcardItemType::Fact
- Both VTabs extract and expand the facts parameter; expand() receives it in QueryRequest

## Self-Check: PASSED

- All 9 key files verified to exist on disk
- All 3 task commit hashes verified in git log
- Full quality gate (just test-all) passed: 573 Rust + 27 SQL + 13 DuckLake + 3 caret

---
*Phase: 46-wildcard-selection-queryable-facts*
*Completed: 2026-04-12*
