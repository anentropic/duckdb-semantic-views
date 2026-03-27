---
phase: quick
plan: 260318-fzu
subsystem: ddl
tags: [parser, model, describe, cleanup]

# Dependency graph
requires:
  - phase: 29
    provides: "HIERARCHIES clause implementation"
provides:
  - "Clean DDL surface without HIERARCHIES clause"
  - "7-column DESCRIBE output (was 8)"
affects: [registry-publishing, documentation]

# Tech tracking
tech-stack:
  added: []
  patterns: []

key-files:
  created: []
  modified:
    - src/model.rs
    - src/body_parser.rs
    - src/graph.rs
    - src/ddl/describe.rs
    - src/ddl/define.rs
    - src/parse.rs
    - src/expand.rs
    - tests/parse_proptest.rs
    - tests/expand_proptest.rs
    - test/sql/phase29_facts.test
    - test/sql/phase20_extended_ddl.test
    - test/sql/phase21_error_reporting.test
    - test/sql/phase25_keyword_body.test
    - test/sql/phase28_e2e.test
    - test/sql/phase30_derived_metrics.test
    - examples/advanced_features.py
    - README.md

key-decisions:
  - "HIERARCHIES removed entirely (struct, parser, validator, describe column) -- pure metadata with no query-time value"
  - "DESCRIBE output reduced from 8 to 7 columns -- hierarchies column removed"
  - "Old stored JSON with hierarchies field still deserializes (serde default) -- no migration needed"

patterns-established: []

requirements-completed: []

# Metrics
duration: 21min
completed: 2026-03-18
---

# Quick Task 260318-fzu: Remove HIERARCHIES Syntax Summary

**Complete removal of HIERARCHIES clause from DDL surface -- Hierarchy struct, parser, validator, describe column, and all tests deleted; DESCRIBE reduced to 7 columns**

## Performance

- **Duration:** 21 min
- **Started:** 2026-03-18T11:34:30Z
- **Completed:** 2026-03-18T11:56:16Z
- **Tasks:** 2
- **Files modified:** 21

## Accomplishments
- Deleted Hierarchy struct, parse_hierarchies_clause, validate_hierarchies, and all hierarchy fields
- Reduced DESCRIBE output from 8 columns to 7 columns (hierarchies column removed)
- Updated all 6 sqllogictest files with 7-column DESCRIBE expectations
- Removed HIERARCHIES from DDL clause keywords, ordering, and error messages
- Cleaned examples, README, fuzz seeds, and proptests of all hierarchy references
- Full quality gate passes: cargo test (all Rust), just test-sql (all 13 sqllogictests), just test-ducklake-ci

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove Hierarchy from Rust source code** - `8c8f68c` (feat)
2. **Task 2: Update sqllogictests, examples, docs, and fuzz seeds** - `3b8aa33` (feat)

## Files Created/Modified

### Rust source (Task 1)
- `src/model.rs` - Deleted Hierarchy struct and hierarchies field from SemanticViewDefinition
- `src/body_parser.rs` - Removed HIERARCHIES from clause keywords, parser functions, and KeywordBody struct
- `src/graph.rs` - Deleted validate_hierarchies function and all hierarchy tests
- `src/ddl/describe.rs` - Reduced from 8-column to 7-column output schema
- `src/ddl/define.rs` - Removed validate_hierarchies call
- `src/parse.rs` - Removed hierarchies field from definition construction
- `src/expand.rs` - Removed hierarchies: vec![] from 31 test fixtures
- `tests/parse_proptest.rs` - Deleted 3 hierarchy-related proptests, updated comments
- `tests/expand_proptest.rs` - Removed hierarchies: vec![] from 2 test fixtures

### Tests, docs, examples (Task 2)
- `test/sql/phase29_facts.test` - Renamed from phase29_facts_hierarchies.test; removed hierarchy tests
- `test/sql/phase20_extended_ddl.test` - Updated 4 DESCRIBE queries to 7 columns
- `test/sql/phase21_error_reporting.test` - Updated DESCRIBE to 7 columns
- `test/sql/phase25_keyword_body.test` - Updated DESCRIBE to 7 columns
- `test/sql/phase28_e2e.test` - Updated DESCRIBE to 7 columns
- `test/sql/phase30_derived_metrics.test` - Updated DESCRIBE to 7 columns
- `test/sql/TEST_LIST` - Updated filename reference
- `examples/advanced_features.py` - Removed HIERARCHIES clause and Section 3, renumbered
- `README.md` - Removed Hierarchies section and HIERARCHIES from DDL reference
- `fuzz/seeds/fuzz_ddl_parse/seed_hierarchies.txt` - Deleted
- `fuzz/seeds/fuzz_ddl_parse/seed_facts_and_hierarchies.txt` - Replaced with seed_facts_chain.txt

## Decisions Made
- HIERARCHIES removed entirely rather than deprecated -- it was pure metadata with no query-time impact, added complexity without value
- DESCRIBE output reduced to 7 columns -- breaking change acceptable before registry publishing
- Old stored JSON with hierarchies field still deserializes via serde `#[serde(default)]` on SemanticViewDefinition -- no migration needed for existing databases

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- DDL surface is clean for registry publishing (no unused metadata clauses)
- DESCRIBE is stable at 7 columns going forward

## Self-Check: PASSED

All 14 modified files verified present, all 3 deleted files verified absent, both commit hashes confirmed in git log.

---
*Quick task: 260318-fzu*
*Completed: 2026-03-18*
