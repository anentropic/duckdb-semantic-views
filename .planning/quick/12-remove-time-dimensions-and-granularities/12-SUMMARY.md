---
phase: quick-12
plan: 01
subsystem: model, expand, ddl, query
tags: [simplification, time-dimensions, removal]
dependency_graph:
  requires: []
  provides: [simplified-dimension-model, simplified-query-request]
  affects: [model, expand, ddl, query, tests, fuzz, docs]
tech_stack:
  added: []
  patterns: [date-trunc-in-dimension-expr]
key_files:
  created: []
  modified:
    - src/model.rs
    - src/expand.rs
    - src/ddl/define.rs
    - src/ddl/parse_args.rs
    - src/query/table_function.rs
    - src/query/explain.rs
    - tests/expand_proptest.rs
    - fuzz/fuzz_targets/fuzz_sql_expand.rs
    - fuzz/fuzz_targets/fuzz_query_names.rs
    - test/sql/phase2_ddl.test
    - test/sql/phase4_query.test
    - test/integration/test_ducklake.py
    - test/integration/test_ducklake_ci.py
    - README.md
decisions:
  - "Users express date truncation via dimension expr (e.g. date_trunc('month', col))"
  - "Removed dim_type/granularity from Dimension struct -- 4 fields: name, expr, source_table, output_type"
  - "Removed granularity_overrides from QueryRequest -- 2 fields: dimensions, metrics"
  - "DDL functions take 4 named params (tables, relationships, dimensions, metrics)"
  - "Query function takes 2 named params (dimensions, metrics)"
metrics:
  duration: "~10 min"
  completed: "2026-03-03"
---

# Quick Task 12: Remove Time Dimensions and Granularities Summary

Eliminated time_dimensions as a separate DDL parameter and granularities as a query-time override, simplifying the model to match Snowflake semantics where users write SQL expressions directly.

## Commits

| Task | Name | Commit | Key Changes |
|------|------|--------|-------------|
| 1 | Remove time dimensions from core layers | c393859 | Dimension struct: 4 fields; QueryRequest: 2 fields; DDL: 4 params; query: 2 params |
| 2 | Update tests, fuzz, and docs | 7044649 | Proptest, fuzz targets, SQL tests, DuckLake CI, README all updated |

## What Changed

### Model Layer (src/model.rs)
- Removed `dim_type` and `granularity` fields from `Dimension` struct
- Simplified `from_json()` to pure serde deserialize (no time dimension validation)
- Removed `time_dimension_tests` module (6 tests)

### Expand Layer (src/expand.rs)
- Removed `granularity_overrides` field from `QueryRequest`
- Removed the `if dim.dim_type == "time"` codegen branch -- dimension expr is always used directly
- Removed `time_dimension_expand_tests` module (4 tests)
- Removed `HashMap` import (no longer needed)

### DDL Layer (src/ddl/define.rs, src/ddl/parse_args.rs)
- Removed `time_dimensions` named parameter from DDL functions (4 params, not 5)
- Removed `VALID_GRANULARITIES`, `validate_granularity()`, and time_dimensions parsing block
- Removed `granularity_validation_tests` module (4 tests)

### Query Layer (src/query/table_function.rs, src/query/explain.rs)
- Removed `granularities` named parameter from `semantic_view` query function
- Removed `extract_map_strings()` function (was only used for granularities)
- Removed granularity override validation block
- Removed `HashMap` import from explain.rs

### Tests and Fuzz Targets
- Updated all `Dimension` constructions to remove `dim_type: None, granularity: None`
- Updated all `QueryRequest` constructions to remove `granularity_overrides: HashMap::new()`
- Updated SQL logic tests to use `date_trunc()` in dimension expr instead of `time_dimensions`
- Updated DuckLake integration tests to use `date_trunc('day', ordered_at)` as regular dimension

### Documentation (README.md)
- Removed "Time dimensions" from feature list
- Updated DDL signature to 5-arg (was 6-arg)
- Replaced time_dimensions example with date_trunc dimension expr
- Updated query examples to show date_trunc as regular dimension

## Deviations from Plan

None -- plan executed exactly as written.

## Verification

```
just test-all   # cargo test (89 pass) + SQL logic tests (3/3 pass) + DuckLake CI (6/6 pass)
```

Zero grep hits for: `dim_type`, `granularity_overrides`, `time_dimensions_type`, `extract_map_strings`, `validate_granularity` in src/, tests/, fuzz/, test/.

## Self-Check: PASSED
