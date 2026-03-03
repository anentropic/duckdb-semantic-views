# Quick Task 11: Remove time_dimensions and granularities

## Status: INITIALIZED — needs planning + execution

## What to do

Remove `time_dimensions` as a separate concept and the `granularities` query-time MAP override.
Time dimensions should just be regular dimensions — users write `date_trunc('month', created_at)`
as the dimension expr themselves, like Snowflake does.

## Why

The time_dimensions + granularities pattern was borrowed from Cube.dev/dbt MetricFlow, which are
middleware layers where users don't write SQL. In our case users write SQL directly, so they can
express granularity in the dimension expression. Snowflake semantic views don't have this concept
either. It's unnecessary complexity.

## GSD workflow state

- **Mode:** quick --full
- **Task number:** 11
- **Slug:** remove-time-dimensions-and-granularities
- **Directory:** .planning/quick/11-remove-time-dimensions-and-granularities
- **Init done:** yes
- **Directory created:** yes
- **Planning:** NOT STARTED
- **Execution:** NOT STARTED

## Scope of changes (gathered from prior context)

### DDL changes (create_semantic_view now takes 5 args, not 6)
- `src/ddl/define.rs` — remove time_dimensions_type from signatures, drop from named_parameters()
- `src/ddl/parse_args.rs` — remove time_dimensions parsing from parse_define_args_from_bind()

### Model changes
- `src/model.rs` — remove TimeDimension struct, remove time_dimensions field from SemanticViewDefinition, remove granularity/dim_type fields from Dimension

### Expand changes
- `src/expand.rs` — remove date_trunc codegen for time dimensions, remove granularity_overrides from QueryRequest, simplify dimension expansion

### Query changes
- `src/query/table_function.rs` — remove granularities named parameter, remove extract_map_strings, remove granularity validation in bind()

### Test changes
- `test/sql/phase2_ddl.test` — remove time_dimensions sections, update DDL calls to 5-arg
- `test/sql/phase4_query.test` — rewrite time dimension tests to use regular dimensions with date_trunc expr
- `test/sql/phase2_restart.test` — update DDL calls
- `test/integration/test_ducklake_ci.py` — update DDL + remove granularity override test
- `test/integration/test_ducklake.py` — update DDL
- Rust unit tests in model.rs, expand.rs — update/remove time dimension tests
- proptest — may need updates if time dimensions are generated

### Doc changes
- `README.md` — remove time dimensions section, simplify DDL signature to 5 args
- `MAINTAINER.md` — update if it references time dimensions

### Fuzz changes
- `fuzz/fuzz_targets/fuzz_sql_expand.rs` — SemanticViewDefinition derives Arbitrary, may need update
- `fuzz/fuzz_targets/fuzz_query_names.rs` — remove granularity_overrides from QueryRequest

## Resume command

```
/gsd:quick --full remove time_dimensions and granularities, merge into regular dimensions
```
