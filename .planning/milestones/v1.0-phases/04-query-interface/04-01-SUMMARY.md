---
phase: 04-query-interface
plan: "01"
subsystem: query
tags: [duckdb, vtab, ffi, table-function, expansion]

requires:
  - phase: 03-expansion-engine
    provides: "expand() function for query generation"
  - phase: 02-storage-and-ddl
    provides: "CatalogState for in-memory view definitions"
provides:
  - "semantic_query table function with named LIST(VARCHAR) parameters"
  - "FFI SQL execution via independent duckdb_connection"
  - "Schema inference via LIMIT 0 with VARCHAR/DOUBLE fallback"
  - "Dimensions-only (SELECT DISTINCT) and metrics-only query modes"
  - "QueryState and QueryError types for query module"
affects: [04-02-explain, 04-03-integration-tests, 05-hardening]

tech-stack:
  added: []
  patterns: ["manual FFI entrypoint replacing duckdb_entrypoint_c_api macro", "independent connection for lock isolation", "LIMIT 0 schema inference with fallback"]

key-files:
  created:
    - src/query/table_function.rs
    - src/query/error.rs
    - src/query/mod.rs
  modified:
    - src/expand.rs
    - src/lib.rs
    - tests/expand_proptest.rs

key-decisions:
  - "manual-ffi-entrypoint: replaced #[duckdb_entrypoint_c_api] macro with hand-written FFI entrypoint to capture raw duckdb_database handle before Connection wrapping; enables duckdb_connect for independent query connection"
  - "independent-query-connection: semantic_query uses a separate duckdb_connection created via duckdb_connect on the same database; avoids lock conflicts with host connection during expanded SQL execution"
  - "limit0-schema-inference: bind() executes expanded SQL with LIMIT 0 on the independent connection to discover column names and types; falls back to VARCHAR/DOUBLE if inference fails"
  - "varchar-string-materialization: func() reads all result values as VARCHAR strings via duckdb_value_varchar and writes to output via flat_vector insert; DuckDB handles implicit casting to declared output types"
  - "value-raw-ptr-transmute: extracts raw duckdb_value from duckdb::vtab::Value via pointer cast since the inner field is pub(crate); pinned to duckdb =1.4.4 layout"
  - "empty-request-replaces-empty-metrics: EmptyMetrics error variant replaced with EmptyRequest — triggered when both dimensions and metrics are empty; dimensions-only is now valid"

patterns-established:
  - "QueryState pattern: catalog + raw connection passed via extra_info to table functions"
  - "FFI SQL execution pattern: execute_sql_raw helper for CString/result/error lifecycle"
  - "Manual entrypoint pattern: captures raw handles before duckdb-rs wrapping"

requirements-completed: [QUERY-01, QUERY-02, QUERY-03]

duration: 20min
completed: 2026-02-25
---

# Plan 04-01: Core Query Table Function Summary

**semantic_query table function with FFI SQL execution via independent connection, LIMIT 0 schema inference, and dimensions-only/metrics-only query support**

## Performance

- **Duration:** ~20 min
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- `semantic_query('view', dimensions := [...], metrics := [...])` returns correct aggregated results
- Dimensions-only queries generate SELECT DISTINCT (no GROUP BY), metrics-only produce global aggregates
- Schema inference via LIMIT 0 on independent connection with VARCHAR/DOUBLE fallback
- Replaced #[duckdb_entrypoint_c_api] macro with manual FFI entrypoint to capture raw database handle
- Fuzzy view name matching with actionable error hints

## Task Commits

1. **Task 1: Modify expand() for dimensions-only and metrics-only queries** - `1a1bada` (feat)
2. **Task 2: Implement semantic_query table function with FFI SQL execution** - `c49bcc1` (feat)

## Files Created/Modified
- `src/query/table_function.rs` - SemanticViewVTab implementation with bind/init/func, FFI helpers, schema inference
- `src/query/error.rs` - QueryError enum with ViewNotFound, EmptyRequest, ExpandFailed, SqlExecution variants
- `src/query/mod.rs` - Module declarations (extension-gated)
- `src/expand.rs` - Dimensions-only SELECT DISTINCT, EmptyRequest error, pub suggest_closest
- `src/lib.rs` - Manual FFI entrypoint, independent query connection, semantic_query registration
- `tests/expand_proptest.rs` - Updated proptest strategies for dimensions-only requests

## Decisions Made
- Manual FFI entrypoint (instead of macro) to capture raw duckdb_database handle for duckdb_connect
- Independent connection via duckdb_connect for lock isolation during expanded SQL execution
- LIMIT 0 schema inference at bind time with VARCHAR/DOUBLE fallback
- All result values read as VARCHAR strings — DuckDB handles implicit casting
- Pointer cast to extract raw duckdb_value from Value struct (pinned to duckdb =1.4.4)

## Deviations from Plan

None - plan executed as specified.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- QueryState pattern ready for reuse by explain_semantic_view (Plan 04-02)
- semantic_query registered and callable for integration tests (Plan 04-03)

---
*Phase: 04-query-interface*
*Completed: 2026-02-25*
