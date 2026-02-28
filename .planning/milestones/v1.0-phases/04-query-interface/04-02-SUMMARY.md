---
phase: 04-query-interface
plan: "02"
subsystem: query
tags: [duckdb, vtab, explain, table-function, debugging]

requires:
  - phase: 04-query-interface
    plan: "01"
    provides: "QueryState, execute_sql_raw, extract_list_strings, semantic_query registration pattern"
  - phase: 03-expansion-engine
    provides: "expand() function for SQL generation"
provides:
  - "explain_semantic_view table function with three-part formatted output"
  - "Metadata header (view name, dimensions, metrics)"
  - "Pretty-printed expanded SQL display"
  - "DuckDB EXPLAIN plan with graceful fallback on error"
affects: [04-03-integration-tests, 05-hardening]

tech-stack:
  added: []
  patterns: ["collect_explain_lines helper for FFI EXPLAIN extraction", "chunked line output via AtomicUsize row_index"]

key-files:
  created:
    - src/query/explain.rs
  modified:
    - src/query/mod.rs
    - src/query/table_function.rs
    - src/lib.rs

key-decisions:
  - "pub-crate-ffi-helpers: promoted execute_sql_raw and extract_list_strings from private to pub(crate) for reuse by explain module; avoids code duplication"
  - "collect-explain-lines-helper: extracted EXPLAIN plan collection into a separate unsafe fn to keep bind() under clippy's 100-line limit while maintaining the same FFI data extraction pattern"
  - "graceful-explain-fallback: if EXPLAIN execution fails (tables not yet created), output includes '-- (not available -- {error})' instead of propagating the error"
  - "chunked-line-emission: func() uses AtomicUsize row_index with 2048-line chunk size for correctness with large EXPLAIN outputs"

patterns-established:
  - "FFI helper reuse pattern: pub(crate) visibility for shared unsafe FFI utilities across query module"

requirements-completed: [QUERY-04]

duration: 4min
completed: 2026-02-25
---

# Plan 04-02: Explain Semantic View Summary

**explain_semantic_view table function returning metadata header, pretty-printed expanded SQL, and DuckDB EXPLAIN plan with graceful fallback**

## Performance

- **Duration:** ~4 min
- **Started:** 2026-02-25T19:57:55Z
- **Completed:** 2026-02-25T20:01:40Z
- **Tasks:** 1
- **Files modified:** 4

## Accomplishments
- `explain_semantic_view('view', dimensions := [...], metrics := [...])` returns formatted three-part output
- Metadata header with view name, requested dimensions, and requested metrics
- Pretty-printed expanded SQL (CTE structure from expand() emitted line-by-line)
- DuckDB EXPLAIN plan via independent connection, with graceful `-- (not available -- ...)` fallback
- Reusable FFI helpers (execute_sql_raw, extract_list_strings) promoted to pub(crate)

## Task Commits

1. **Task 1: Implement explain_semantic_view table function** - `103b35a` (feat)

## Files Created/Modified
- `src/query/explain.rs` - ExplainSemanticViewVTab with bind/init/func, collect_explain_lines helper
- `src/query/mod.rs` - Added `pub mod explain` declaration (extension-gated)
- `src/query/table_function.rs` - Promoted execute_sql_raw and extract_list_strings to pub(crate)
- `src/lib.rs` - Imported ExplainSemanticViewVTab and registered explain_semantic_view function

## Decisions Made
- Promoted execute_sql_raw and extract_list_strings to pub(crate) for reuse (avoids duplicating FFI code)
- Extracted collect_explain_lines helper to satisfy clippy pedantic line-count limit
- Graceful EXPLAIN fallback instead of hard error when referenced tables do not exist
- Chunked output via AtomicUsize for correct handling of large EXPLAIN plans

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- explain_semantic_view registered and callable for integration tests (Plan 04-03)
- All QUERY-* requirements (01-04) now implemented
- Ready for end-to-end integration testing

## Self-Check: PASSED

- FOUND: src/query/explain.rs
- FOUND: commit 103b35a
- FOUND: 04-02-SUMMARY.md

---
*Phase: 04-query-interface*
*Completed: 2026-02-25*
