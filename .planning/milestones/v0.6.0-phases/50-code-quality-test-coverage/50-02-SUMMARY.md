---
phase: 50-code-quality-test-coverage
plan: 02
subsystem: refactoring
tags: [generics, newtypes, code-deduplication, dead-code-removal, type-safety, expand-module]

# Dependency graph
requires:
  - phase: 50-code-quality-test-coverage
    plan: 01
    provides: "38 regression tests as safety net for expand module refactoring"
provides:
  - "resolve_names generic helper deduplicating 4 resolution loops in sql_gen.rs"
  - "DimensionName and MetricName newtypes with case-insensitive Eq/Hash"
  - "QueryRequest using Vec<DimensionName> and Vec<MetricName> instead of Vec<String>"
  - "NaGroup named struct replacing tuple type in semi_additive.rs"
  - "Dead parse_constraint_columns removed from model.rs"
affects: [expand-module, type-safety, query-interface]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Generic resolution helper with closures for error construction"
    - "Newtype pattern with case-insensitive Eq/Hash for domain names"
    - "AsRef<str> generic bound for accepting both newtypes and String in resolve_names"

key-files:
  created: []
  modified:
    - "src/expand/sql_gen.rs"
    - "src/expand/types.rs"
    - "src/expand/semi_additive.rs"
    - "src/expand/mod.rs"
    - "src/expand/window.rs"
    - "src/model.rs"
    - "src/query/table_function.rs"
    - "src/query/explain.rs"
    - "src/ddl/define.rs"
    - "tests/expand_proptest.rs"

key-decisions:
  - "resolve_names uses 9 closure parameters instead of a trait object -- avoids dynamic dispatch and keeps error construction at call sites"
  - "DimensionName/MetricName implement AsRef<str> + Deref<Target=str> for ergonomic interop with existing string-based code"
  - "NaGroup uses named struct fields but keeps internal grouping key as tuple for simplicity"
  - "Retained clippy::too_many_lines annotations after rustfmt expanded resolve_names call sites"

patterns-established:
  - "Newtype pattern: wrap String with case-insensitive Eq/Hash for domain names"
  - "Generic resolution helper: resolve_names<T, N: AsRef<str>> deduplicates name lookup patterns"

requirements-completed: [QUAL-02, QUAL-03, QUAL-04, QUAL-05]

# Metrics
duration: 20min
completed: 2026-04-14
---

# Phase 50 Plan 02: Expand Module Refactoring Summary

**Generic resolve_names helper deduplicating 4 resolution loops, DimensionName/MetricName newtypes with case-insensitive semantics, NaGroup named struct, and dead code removal**

## Performance

- **Duration:** 20 min
- **Started:** 2026-04-14T11:40:14Z
- **Completed:** 2026-04-14T12:00:14Z
- **Tasks:** 3
- **Files modified:** 10

## Accomplishments
- Extracted `resolve_names<T, N: AsRef<str>>` generic helper that replaces 4 duplicated resolution loops (dims+metrics in expand(), facts+dims in expand_facts()), eliminating ~60 lines of duplicated logic
- Introduced DimensionName and MetricName newtypes with case-insensitive PartialEq/Eq/Hash, centralizing the 189+ ad-hoc `eq_ignore_ascii_case`/`to_ascii_lowercase` patterns at the type level
- Updated QueryRequest to use Vec<DimensionName> and Vec<MetricName>, providing compile-time type safety for dimension vs metric name handling
- Replaced tuple type `(Vec<NonAdditiveDim>, Vec<usize>)` with NaGroup named struct in semi_additive.rs for readability
- Removed dead parse_constraint_columns function and its 5 tests from model.rs
- Added 7 newtype unit tests verifying case-insensitive equality, hash set membership, display, deref, and From conversions

## Task Commits

Each task was committed atomically:

1. **Task 1: Extract resolve_names generic helper** - `ccee84e` (refactor)
2. **Task 2: Introduce DimensionName/MetricName newtypes** - `1643bbb` (feat)
3. **Task 3: NaGroup named struct and dead code removal** - `bf19156` (refactor)
4. **Task 2 fix: Update expand_proptest.rs for newtypes** - `e9b8fe0` (fix)

## Files Created/Modified
- `src/expand/sql_gen.rs` - Added resolve_names generic helper, replaced 4 resolution loops with calls to it, updated all test QueryRequest constructions
- `src/expand/types.rs` - Added DimensionName and MetricName newtypes with case-insensitive Eq/Hash, updated QueryRequest fields, added 7 newtype unit tests
- `src/expand/mod.rs` - Added DimensionName and MetricName to public exports
- `src/expand/semi_additive.rs` - Added NaGroup struct, updated collect_na_groups return type and consumers, updated test QueryRequest constructions
- `src/expand/window.rs` - Updated test QueryRequest constructions for newtypes
- `src/model.rs` - Removed dead parse_constraint_columns function and constraint_column_parsing_tests module
- `src/query/table_function.rs` - Updated QueryRequest construction to use DimensionName::new/MetricName::new
- `src/query/explain.rs` - Updated QueryRequest construction to use DimensionName::new/MetricName::new
- `src/ddl/define.rs` - Updated QueryRequest construction to use DimensionName::new/MetricName::new
- `tests/expand_proptest.rs` - Updated QueryRequest construction and imports for newtypes

## Decisions Made
- Used 9 closure parameters for resolve_names rather than a trait object -- keeps error construction at call sites and avoids dynamic dispatch overhead
- Implemented both Deref<Target=str> and AsRef<str> on newtypes for maximum compatibility with existing string-consuming code
- Kept internal grouping tuple in collect_na_groups as-is (it's a local implementation detail); only the return type becomes NaGroup
- Retained clippy::too_many_lines annotations because rustfmt expanded the multi-line closure arguments past the 100-line threshold

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed expand_proptest.rs compilation after newtype change**
- **Found during:** Task 2 verification (just test-all)
- **Issue:** tests/expand_proptest.rs constructs QueryRequest with Vec<String> but QueryRequest now expects Vec<DimensionName>/Vec<MetricName>
- **Fix:** Updated imports and 3 QueryRequest construction sites in expand_proptest.rs
- **Files modified:** tests/expand_proptest.rs
- **Verification:** just test-all passes (all 704+ tests)
- **Committed in:** e9b8fe0

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** File was outside the plan's scope (tests/ not src/) but required updating for the newtype change to pass the full test suite. No scope creep.

## Issues Encountered
- Clippy pedantic catches `must_use_candidate` and `too_many_arguments` on the new resolve_names function -- resolved by adding appropriate allow annotations
- rustfmt reformats multi-line closure arguments to be more verbose, pushing function line counts above clippy threshold -- retained too_many_lines annotations

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Expand module internals are cleaner with ~60 fewer lines of duplicated resolution logic
- DimensionName/MetricName newtypes provide compile-time safety for dimension vs metric name handling
- All 616 Rust unit tests + 88 proptest cases + 30 sqllogictest files + 6 DuckLake CI tests pass
- Phase 50 complete -- both plans (test coverage + refactoring) delivered

## Self-Check: PASSED

All 10 modified files exist. All 4 task commits verified in git log.

---
*Phase: 50-code-quality-test-coverage*
*Completed: 2026-04-14*
