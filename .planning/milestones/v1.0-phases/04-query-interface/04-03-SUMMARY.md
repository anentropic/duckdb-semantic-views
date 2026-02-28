---
phase: 04-query-interface
plan: "03"
subsystem: testing
tags: [duckdb, integration-test, sqllogictest, ducklake, iceberg, ffi]

requires:
  - phase: 04-query-interface
    provides: "semantic_query table function and explain_semantic_view"
  - phase: 03-expansion-engine
    provides: "expand() for SQL generation with joins, filters, pruning"
provides:
  - "SQLLogicTest covering basic round-trip, WHERE composition, multi-join, EXPLAIN, dims-only, metrics-only, and errors"
  - "DuckLake/Iceberg integration test with jaffle-shop data via Python script"
  - "Just recipes for DuckLake setup and Iceberg testing"
  - "Fixed duckdb_string_t vector reading replacing broken duckdb_value_varchar"
affects: [05-hardening]

tech-stack:
  added: []
  patterns: ["VARCHAR-cast wrapper for uniform chunk reading", "duckdb_string_t direct decode from vector data"]

key-files:
  created:
    - test/sql/phase4_query.test
    - configure/setup_ducklake.py
    - test/integration/test_ducklake.py
  modified:
    - src/query/table_function.rs
    - src/query/explain.rs
    - Justfile
    - .gitignore

key-decisions:
  - "varchar-output-columns: all semantic_query output columns declared as VARCHAR to avoid type mismatch panics when writing string data to typed output vectors; DuckDB handles implicit casting in downstream operations"
  - "varchar-cast-wrapper: expanded SQL wrapped in SELECT CAST(...AS VARCHAR) subquery to ensure all result chunk vectors contain duckdb_string_t data; replaces broken duckdb_value_varchar deprecated API"
  - "direct-string-t-decode: read duckdb_string_t inline/pointer union directly from vector memory instead of calling duckdb_string_t_data/duckdb_string_t_length API functions; avoids potential unavailability in loadable-extension stubs"
  - "unqualified-join-expressions: dimension/metric expressions must use unqualified column names (e.g., 'tier' not 'test_customers.tier') because the CTE flattens all tables into _base namespace"
  - "python-ducklake-test: DuckLake integration test uses Python script instead of SQLLogicTest because the test runner cannot dynamically install DuckDB extensions (ducklake)"

patterns-established:
  - "VARCHAR-cast wrapper pattern: wrap expanded SQL in SELECT CAST(col AS VARCHAR) FROM (...) for uniform chunk reading"
  - "Vector string reading pattern: decode duckdb_string_t union directly from vector data pointer"

requirements-completed: [TEST-03, TEST-04]

duration: 29min
completed: 2026-02-25
---

# Plan 04-03: Integration Tests Summary

**SQLLogicTest with 8 test sections covering full query round-trip, plus DuckLake/Iceberg integration test with jaffle-shop data and fixed FFI value reading via direct duckdb_string_t decode**

## Performance

- **Duration:** ~29 min
- **Started:** 2026-02-25T19:58:19Z
- **Completed:** 2026-02-25T20:27:07Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Full query round-trip integration tests: basic aggregation, WHERE composition, multi-table joins, EXPLAIN equivalence, dimensions-only, metrics-only, and error cases
- Fixed critical bug in semantic_query and explain_semantic_view: deprecated duckdb_value_varchar API returned empty strings; replaced with direct duckdb_string_t vector decode
- DuckLake/Iceberg integration test infrastructure with jaffle-shop data download, catalog creation, and 4 test scenarios
- Just recipes for setup-ducklake and test-iceberg, updated test-all to include DuckLake tests

## Task Commits

1. **Task 1: Core query integration tests** - `9fe8e1f` (feat)
2. **Task 2: DuckLake/Iceberg integration test and setup** - `cda2737` (feat)

## Files Created/Modified
- `test/sql/phase4_query.test` - SQLLogicTest with 8 sections covering all query scenarios
- `configure/setup_ducklake.py` - Python script to download jaffle-shop seeds and create DuckLake catalog
- `test/integration/test_ducklake.py` - Python integration test for DuckLake/Iceberg queries
- `src/query/table_function.rs` - Fixed value reading: VARCHAR-cast wrapper, direct duckdb_string_t decode, VARCHAR output columns
- `src/query/explain.rs` - Updated to use read_varchar_from_vector helper for EXPLAIN plan extraction
- `Justfile` - Added setup-ducklake, test-iceberg recipes; updated test-all
- `.gitignore` - Added entries for test data, DuckLake files, configure artifacts

## Decisions Made
- All semantic_query output columns declared as VARCHAR (avoids type mismatch panics when writing string data to typed vectors)
- Expanded SQL wrapped in VARCHAR-cast subquery for uniform chunk reading (replaces broken duckdb_value_varchar)
- Direct duckdb_string_t union decode from vector data (avoids reliance on C API helper functions in loadable extension stubs)
- DuckLake test uses Python script instead of SQLLogicTest (runner cannot install DuckDB extensions dynamically)
- Dimension/metric expressions must use unqualified column names after CTE flattening

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed duckdb_value_varchar returning empty strings**
- **Found during:** Task 1 (Core query integration tests)
- **Issue:** The deprecated `duckdb_value_varchar` C API does not work with DuckDB 1.4.4's chunked result format. All values returned as empty strings, making semantic_query and explain_semantic_view non-functional.
- **Fix:** Replaced the deprecated API with three changes: (1) wrap expanded SQL in a VARCHAR-cast subquery, (2) read duckdb_string_t directly from chunk vector data, (3) declare all output columns as VARCHAR.
- **Files modified:** src/query/table_function.rs, src/query/explain.rs
- **Verification:** Python test confirms correct values; SQLLogicTest passes all assertions
- **Committed in:** 9fe8e1f (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Critical bug fix required for any integration tests to pass. The semantic_query table function was non-functional without this fix. No scope creep.

## Issues Encountered
- Pre-existing phase2_ddl.test restart section hangs due to stale sidecar file persistence -- logged as out-of-scope deferred item
- DuckLake setup script requires network access and writable extension directory -- sandbox restrictions prevent automated verification but script structure is correct

## User Setup Required

**DuckLake integration test requires manual setup.** Run:
```bash
just setup-ducklake  # Download jaffle-shop data and create DuckLake catalog
just test-iceberg    # Run DuckLake integration test
```

## Next Phase Readiness
- All Phase 4 integration tests in place; ready for Phase 5 hardening
- semantic_query value reading is now robust for all DuckDB types
- explain_semantic_view now outputs full DuckDB plans (not just metadata)

## Self-Check: PASSED

All claimed files verified to exist. All commit hashes verified in git log.

---
*Phase: 04-query-interface*
*Completed: 2026-02-25*
