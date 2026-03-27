---
phase: quick-260322-1zx
plan: 01
subsystem: database
tags: [primary-key, catalog, duckdb-constraints, cardinality-inference]

# Dependency graph
requires:
  - phase: 33
    provides: "Cardinality inference (infer_cardinality) and PK/FK validation"
provides:
  - "Optional PRIMARY KEY in TABLES clause -- resolved from DuckDB catalog at bind time"
  - "catalog_conn on DefineState for catalog metadata queries"
  - "Tolerant infer_cardinality that defers to bind-time when target PK is empty"
affects: [define, parse, model]

# Tech tracking
tech-stack:
  added: []
  patterns: ["UNNEST for reading LIST columns via C API (duckdb_value_varchar returns NULL for LIST)"]

key-files:
  created: []
  modified:
    - src/parse.rs
    - src/ddl/define.rs
    - src/model.rs
    - src/lib.rs
    - test/sql/phase33_cardinality_inference.test

key-decisions:
  - "catalog_conn (not db_handle) for catalog queries -- duckdb_connect fails during bind in loadable extension context"
  - "UNNEST flattening for constraint_column_names -- duckdb_value_varchar returns NULL for LIST columns in DuckDB 1.5.0"
  - "parse_constraint_columns moved to model.rs for non-feature-gated unit testing"
  - "Error at CREATE time (not query time) when table has no PK in DDL or catalog"

patterns-established:
  - "Catalog metadata lookup pattern: UNNEST(constraint_column_names) via catalog_conn"
  - "Tolerant parse + bind-time resolution pattern: parse skips, bind resolves from catalog"

requirements-completed: [PK-OPTIONAL]

# Metrics
duration: 30min
completed: 2026-03-22
---

# Quick Task 260322-1zx: Make PRIMARY KEY Optional Summary

**Optional PRIMARY KEY in TABLES clause with automatic catalog resolution via duckdb_constraints() at bind time**

## Performance

- **Duration:** 30 min
- **Started:** 2026-03-22T01:37:15Z
- **Completed:** 2026-03-22T02:07:51Z
- **Tasks:** 3
- **Files modified:** 5

## Accomplishments
- Tables declared without PRIMARY KEY in the TABLES clause now resolve PKs from the DuckDB catalog at bind time
- Existing behavior preserved: explicit PKs in DDL, error when no PK exists anywhere
- End-to-end coverage: basic PK from catalog, composite PK, mixed mode, error case, DESCRIBE output

## Task Commits

Each task was committed atomically:

1. **Task 1: Make infer_cardinality tolerant and add catalog PK resolution** - `9905d44` (feat)
2. **Task 2: Add unit tests for catalog PK resolution** - `e941b53` (test)
3. **Task 3: Add sqllogictest for end-to-end PK-optional flow** - `da56196` (feat)

## Files Created/Modified
- `src/parse.rs` - Changed `infer_cardinality` to `pub(crate)`, tolerant skip when target has no PK
- `src/ddl/define.rs` - Added `resolve_pk_from_catalog`, `catalog_conn` to `DefineState`, reordered bind sequence
- `src/model.rs` - Added `parse_constraint_columns` helper with unit tests
- `src/lib.rs` - Create `catalog_conn` at init time, pass to all `DefineState` instances
- `test/sql/phase33_cardinality_inference.test` - 6 new PKOpt test cases appended

## Decisions Made
- Used `catalog_conn` (connection created at init time) instead of `db_handle` for catalog queries -- `duckdb_connect` returns error when called during bind in loadable extension context
- Used UNNEST to flatten `constraint_column_names` (VARCHAR[]) into individual VARCHAR rows -- `duckdb_value_varchar` returns NULL for LIST-typed columns in DuckDB 1.5.0 C API
- Moved `parse_constraint_columns` to `model.rs` so unit tests run under default `cargo test` (the `ddl` module is feature-gated with `#[cfg(feature = "extension")]`)
- Error fires at CREATE time (during bind) rather than query time when a table has no PK in either DDL or catalog -- this is the natural result of the bind-time guard check

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] duckdb_connect fails during bind in loadable extension**
- **Found during:** Task 3 (sqllogictest debugging)
- **Issue:** Plan specified using `db_handle` to create temporary connections at bind time; `duckdb_connect` returns error during bind
- **Fix:** Added `catalog_conn` field to `DefineState`, created at init time (not bind time)
- **Files modified:** `src/ddl/define.rs`, `src/lib.rs`
- **Verification:** All sqllogictests pass
- **Committed in:** da56196 (Task 3 commit)

**2. [Rule 1 - Bug] duckdb_value_varchar returns NULL for LIST columns**
- **Found during:** Task 3 (sqllogictest debugging)
- **Issue:** `constraint_column_names` is VARCHAR[] (LIST type); `duckdb_value_varchar` returns NULL for LIST columns in DuckDB 1.5.0
- **Fix:** Changed query to use `UNNEST(constraint_column_names)` to flatten list into individual VARCHAR rows
- **Files modified:** `src/ddl/define.rs`
- **Verification:** All sqllogictests pass, PK columns correctly resolved
- **Committed in:** da56196 (Task 3 commit)

**3. [Rule 1 - Bug] Error test expected CREATE to succeed then fail at query time**
- **Found during:** Task 3 (sqllogictest)
- **Issue:** PKOpt Test 4 used `statement ok` for CREATE + `statement error` for SELECT; but the error fires during CREATE (bind-time guard)
- **Fix:** Changed to `statement error` on the CREATE statement directly
- **Files modified:** `test/sql/phase33_cardinality_inference.test`
- **Verification:** Test passes with correct error matching
- **Committed in:** da56196 (Task 3 commit)

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** All fixes necessary for correctness. No scope creep. The `catalog_conn` and UNNEST approaches are superior to the plan's original `db_handle`/`parse_constraint_columns` design due to DuckDB 1.5.0 C API constraints.

## Issues Encountered
- Pre-existing proptest failure (`relationship_no_cardinality_defaults` with name "as" as relationship name): out of scope, not caused by these changes, logged for future fix

## Known Stubs
None -- all functionality is fully wired.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- PK-optional feature complete and tested
- Ready for milestone v0.5.4 continuation

## Self-Check: PASSED

All 5 files verified present. All 3 commit hashes verified in git log.

---
*Quick Task: 260322-1zx*
*Completed: 2026-03-22*
