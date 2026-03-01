---
phase: 09-time-dimensions
plan: 02
subsystem: expand + table_function
tags: [rust, date_trunc, map-parameter, time-dimensions, ffi]

requires:
  - phase: 09-01
    provides: Dimension struct with dim_type + granularity fields

provides:
  - date_trunc('gran', expr)::DATE codegen in expand() for time dimensions
  - granularity_overrides in QueryRequest for query-time override
  - extract_map_strings() FFI helper for DuckDB MAP(VARCHAR, VARCHAR)
  - granularities named parameter on semantic_query (MAP(VARCHAR, VARCHAR))
  - bind-time validation: non-time dimension override → error; unsupported value → error

affects: [expand.rs, table_function.rs, explain.rs]

tech-stack:
  added: []
  patterns:
    - "date_trunc('gran', expr)::DATE — ::DATE cast required (TIME-04) because date_trunc returns TIMESTAMP by default"
    - "HashMap keyed by lowercased dimension name for case-insensitive override lookup"
    - "LogicalTypeHandle::map() available in duckdb-rs 1.4.4 — no C API fallback needed"
    - "extract_map_strings() mirrors extract_list_strings() pattern: duckdb_get_map_size/key/value + duckdb_get_varchar + destroy"

key-files:
  created: []
  modified:
    - src/expand.rs
    - src/query/table_function.rs
    - src/query/explain.rs

key-decisions:
  - "LogicalTypeHandle::map() exists in duckdb-rs 1.4.4 — confirmed during Phase 9 research; used directly without C API fallback"
  - "Override keys lowercased at extraction time in extract_map_strings() — consistent with suggest_closest() and dimension resolution patterns"
  - "explain.rs gets granularity_overrides: HashMap::new() only — EXPLAIN does not expose override parameter (future work)"
  - "Validation skips dimensions not found in def.dimensions — expand() will catch as UnknownDimension error"

patterns-established:
  - "Pattern: extract_map_strings() — for future MAP(VARCHAR, VARCHAR) named parameters"
  - "Pattern: VALID_GRANULARITIES const in bind() mirrors from_json() validation — single source of truth deferred to future refactor"

requirements-completed: [TIME-02, TIME-03, TIME-04]

duration: 35min
completed: 2026-03-01
---

# Phase 9 Plan 02 Summary

**Wired `date_trunc` SQL codegen and query-time granularity override into `expand()` and `bind()`.**

## Performance

- **Duration:** ~35 min (including context restoration from compaction)
- **Started:** 2026-03-01T00:00:00Z
- **Completed:** 2026-03-01T00:00:00Z
- **Tasks:** 2 (Task 1: expand.rs codegen; Task 2: table_function.rs MAP parameter)
- **Files modified:** 3

## Accomplishments

- `QueryRequest` extended with `granularity_overrides: HashMap<String, String>`
- `expand()` SELECT loop generates `date_trunc('gran', expr)::DATE` for time dimensions (TIME-02, TIME-04)
  — override map takes precedence over declared granularity (TIME-03)
- `extract_map_strings()` FFI helper added alongside `extract_list_strings()` — same pattern, MAP C API
- `bind()` extracts `granularities` MAP parameter and validates overrides at bind time
- `named_parameters()` registers `granularities` as `MAP(VARCHAR, VARCHAR)` using `LogicalTypeHandle::map()`
- `explain.rs` updated with `granularity_overrides: HashMap::new()` (compile fix)
- 64 total lib tests pass — no regressions
- Extension build (`--features extension`) clean

## Task Commits

1. **Task 1+2: date_trunc codegen + granularities MAP parameter** - `b94cc10` (feat)
   (Tasks 1 and 2 committed together because Task 1 changes were unstaged from previous session)

## Files Created/Modified

- `src/expand.rs` — QueryRequest + granularity_overrides field; date_trunc codegen in SELECT loop; 4 new tests
- `src/query/table_function.rs` — extract_map_strings(); granularity extraction + validation in bind(); granularities in named_parameters()
- `src/query/explain.rs` — granularity_overrides: HashMap::new() added to QueryRequest literal

## Self-Check: PASSED

- [x] `cargo test --lib` passes: 64/64 tests
- [x] `cargo build --features extension` clean
- [x] `granularity_overrides` in QueryRequest struct (expand.rs line 44)
- [x] `date_trunc('gran', expr)::DATE` codegen in expand() SELECT loop
- [x] Override takes precedence over declared granularity (test_time_dimension_with_granularity_override)
- [x] `::DATE` cast present (TIME-04, test_date_trunc_includes_date_cast)
- [x] `extract_map_strings()` function in table_function.rs
- [x] `granularities` extracted in bind() after metrics
- [x] Validation: non-time dimension override → "is not a time dimension" error
- [x] Validation: unsupported granularity → "valid values: day, week, month, year" error
- [x] `granularities` registered in named_parameters() as MAP(VARCHAR, VARCHAR)
- [x] `LogicalTypeHandle::map()` used directly (available in duckdb-rs 1.4.4)
