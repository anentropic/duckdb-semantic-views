---
phase: quick-12
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
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
autonomous: true
requirements: [QUICK-12]

must_haves:
  truths:
    - "time_dimensions named parameter no longer exists in DDL functions"
    - "granularities named parameter no longer exists in semantic_view query function"
    - "Dimension struct has no dim_type or granularity fields"
    - "QueryRequest has no granularity_overrides field"
    - "Users express date truncation via the dimension expr directly (e.g. date_trunc('month', col))"
    - "All existing tests pass with the simplified model"
  artifacts:
    - path: "src/model.rs"
      provides: "Simplified Dimension struct without dim_type/granularity"
      contains: "pub struct Dimension"
    - path: "src/expand.rs"
      provides: "Simplified expand without time dimension codegen"
      contains: "pub struct QueryRequest"
    - path: "src/ddl/define.rs"
      provides: "DDL function with 4 named params (no time_dimensions)"
    - path: "src/ddl/parse_args.rs"
      provides: "Argument parsing without time_dimensions"
    - path: "src/query/table_function.rs"
      provides: "Query function without granularities parameter"
  key_links:
    - from: "src/ddl/parse_args.rs"
      to: "src/model.rs"
      via: "Dimension struct construction"
      pattern: "Dimension \\{"
    - from: "src/expand.rs"
      to: "src/model.rs"
      via: "Dimension field access in expand()"
      pattern: "dim\\.expr"
    - from: "src/query/table_function.rs"
      to: "src/expand.rs"
      via: "QueryRequest construction"
      pattern: "QueryRequest \\{"
---

<objective>
Remove time_dimensions as a separate DDL parameter and granularities as a query-time override.
Time dimensions become regular dimensions where users write date_trunc() in the expr themselves.

Purpose: Eliminate unnecessary complexity. Snowflake semantic views don't have this concept.
Users write SQL directly and can express granularity in dimension expressions.

Output: Simplified model, DDL, expand, and query code. All tests updated and passing.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/model.rs
@src/expand.rs
@src/ddl/define.rs
@src/ddl/parse_args.rs
@src/query/table_function.rs
@src/query/explain.rs
@tests/expand_proptest.rs
@fuzz/fuzz_targets/fuzz_sql_expand.rs
@fuzz/fuzz_targets/fuzz_query_names.rs
@test/sql/phase2_ddl.test
@test/sql/phase4_query.test
@test/integration/test_ducklake.py
@test/integration/test_ducklake_ci.py
@README.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: Remove time dimensions from model, expand, DDL, and query layers</name>
  <files>
    src/model.rs,
    src/expand.rs,
    src/ddl/define.rs,
    src/ddl/parse_args.rs,
    src/query/table_function.rs,
    src/query/explain.rs
  </files>
  <action>
**src/model.rs:**
1. Remove `dim_type` and `granularity` fields from the `Dimension` struct. Keep `name`, `expr`, `source_table`, `output_type`.
2. Remove all serde attributes for those fields.
3. In `from_json()`: remove the entire `VALID_GRANULARITIES` const, the `for dim in &def.dimensions` validation loop that checks `dim_type` and `granularity`. The method becomes a simple serde deserialize + return.
4. Remove the entire `mod time_dimension_tests` block (lines ~242-322). It tests time-specific validation that no longer exists.
5. In `mod phase12_model_tests`: update the `output_type_on_dimension_roundtrips` test -- remove `dim_type: None, granularity: None` from the Dimension construction (those fields no longer exist).
6. Update any remaining test that constructs a Dimension with `dim_type`/`granularity` fields.

**src/expand.rs:**
1. Remove `granularity_overrides` field from `QueryRequest` struct. It becomes just `{ dimensions, metrics }`.
2. In `expand()` function (lines ~463-476): remove the entire `if dim.dim_type.as_deref() == Some("time")` branch. The dimension expression is always used directly: `dim.expr.clone()`. The output_type CAST wrapping still applies.
3. Remove the entire `mod time_dimension_expand_tests` block (lines ~1544-1673). These test the time codegen path being removed.
4. Update ALL test helper functions and tests that construct `QueryRequest` -- remove the `granularity_overrides: HashMap::new()` field. This affects every test in expand_tests, phase11_1_expand_tests, phase12_cast_tests.
5. Remove `use std::collections::HashMap` if no longer needed (it was only for granularity_overrides).

**src/ddl/define.rs:**
1. In `DefineSemanticViewVTab::named_parameters()`: remove the `time_dimensions_type` variable and the `("time_dimensions".to_string(), time_dimensions_type)` entry from the returned Vec. The function returns 4 named params: tables, relationships, dimensions, metrics.
2. Update the doc comment on `DefineSemanticViewVTab` to say 5-arg (not 6-arg), and remove references to time_dimensions from the SQL examples.

**src/ddl/parse_args.rs:**
1. Remove the entire "Param 4: time_dimensions" section (lines ~209-232). This block parsed the time_dimensions LIST(STRUCT) and appended Dimension entries with `dim_type: Some("time")` and `granularity: Some(gran)`.
2. Remove the `VALID_GRANULARITIES` const and `validate_granularity()` function.
3. Remove the entire `mod granularity_validation_tests` test block.
4. Update the module-level doc comment to reflect 5 args (not 6), remove time_dimensions from the argument mapping table.
5. In the dimensions parsing section: Dimension construction no longer sets `dim_type: None, granularity: None` -- those fields don't exist.

**src/query/table_function.rs:**
1. In `SemanticViewVTab::bind()`: remove the `granularity_overrides` extraction block (lines ~390-394, the `extract_map_strings` call). Remove the granularity override validation block (lines ~428-453, the `for (override_dim_lower, override_gran)` loop and `VALID_GRANULARITIES` const).
2. Update `QueryRequest` construction to not include `granularity_overrides`.
3. In `SemanticViewVTab::named_parameters()`: remove the `"granularities"` entry (the MAP(VARCHAR, VARCHAR) param). Only `dimensions` and `metrics` remain.
4. Remove `extract_map_strings()` function entirely (lines ~182-202). It was only used for granularities.
5. Remove `use std::collections::HashMap` from the import if no longer needed (check if HashMap is still used for type_map -- yes it is, keep the import).

**src/query/explain.rs:**
1. In `ExplainSemanticViewVTab::bind()`: the `QueryRequest` construction (line ~159-163) must drop `granularity_overrides`. The HashMap import can be removed if not needed elsewhere in the file.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo test 2>&1 | tail -20</automated>
  </verify>
  <done>
    - Dimension struct has only: name, expr, source_table, output_type
    - QueryRequest has only: dimensions, metrics
    - DDL named_parameters returns 4 params (no time_dimensions)
    - Query named_parameters returns 2 params (no granularities)
    - All Rust unit and proptest tests pass
  </done>
</task>

<task type="auto">
  <name>Task 2: Update tests, fuzz targets, and documentation</name>
  <files>
    tests/expand_proptest.rs,
    fuzz/fuzz_targets/fuzz_sql_expand.rs,
    fuzz/fuzz_targets/fuzz_query_names.rs,
    test/sql/phase2_ddl.test,
    test/sql/phase4_query.test,
    test/integration/test_ducklake.py,
    test/integration/test_ducklake_ci.py,
    README.md
  </files>
  <action>
**tests/expand_proptest.rs:**
1. Remove `granularity_overrides: std::collections::HashMap::new()` from all `QueryRequest` constructions in `arb_query_request()` and `global_aggregate_no_group_by`.
2. Remove `dim_type: None, granularity: None` from all Dimension structions in `simple_definition()` and `joined_definition()`.
3. Remove the HashMap import if no longer needed.

**fuzz/fuzz_targets/fuzz_sql_expand.rs:**
1. Remove `granularity_overrides: HashMap::new()` from QueryRequest in fuzz_target.
2. Remove `use std::collections::HashMap`.

**fuzz/fuzz_targets/fuzz_query_names.rs:**
1. Same as above: remove `granularity_overrides: HashMap::new()` from QueryRequest.
2. Remove `use std::collections::HashMap`.

**test/sql/phase2_ddl.test:**
1. Section 13 ("View with time dimensions"): rewrite the `create_semantic_view` call for `time_orders` to use a regular dimension with `date_trunc` in the expr. Change from:
   ```
   time_dimensions := [{'name': 'order_date', 'expr': 'created_at', 'granularity': 'month'}],
   ```
   to adding in the dimensions list:
   ```
   dimensions := [...existing..., {'name': 'order_date', 'expr': "date_trunc('month', created_at)", 'source_table': 'o'}],
   ```
   Remove the `time_dimensions :=` line entirely.
2. Section 16 (kwarg_test): remove `time_dimensions := [],` line.
3. Section 17 comment: update "Omitting optional params (relationships, time_dimensions)" to "Omitting optional params (relationships)".

**test/sql/phase4_query.test:**
1. The `typed_date_test` view (around line 282): change from `time_dimensions := [...]` to a regular dimension:
   ```
   dimensions := [{'name': 'event_date', 'expr': "date_trunc('month', event_date)", 'source_table': 'e'}],
   ```
   Remove the `time_dimensions :=` line.
2. The query assertion for `typed_date_test` (around line 291-295): the column type for event_date changes. Previously it was DATE (via `::DATE` cast in time codegen). Now it stays as whatever `date_trunc` returns, which is a TIMESTAMP in DuckDB. The test assertion format `query TI` may need adjustment -- `date_trunc('month', event_date)` on a DATE column returns DATE, so the values `2024-01-01` and `2024-02-01` should still match. Keep the `query TI` format and expected values; if the type changes to TIMESTAMP the output format might be `2024-01-01 00:00:00` instead of `2024-01-01`. Adjust accordingly: use `query TI` and check whether DuckDB's `date_trunc('month', DATE_COL)` returns DATE or TIMESTAMP. Since the source column is `DATE`, `date_trunc` preserves the DATE type, so the expected output should remain `2024-01-01` and `2024-02-01`. No change to expected values needed.

**test/integration/test_ducklake.py:**
1. Update the module docstring: remove "time_dimensions" from the 6-arg signature, making it 5-arg.
2. Remove test 6 description from the docstring.
3. In the `create_semantic_view` call: replace `time_dimensions := [{'name': 'ordered_at', 'expr': 'ordered_at', 'granularity': 'day'}]` with adding `{'name': 'ordered_at', 'expr': "date_trunc('day', ordered_at)", 'source_table': 'o'}` to the dimensions list.
4. Test 6 ("Time dimension with day granularity"): update the test description to "Date dimension with date_trunc" and adjust the query call -- remove the granularity override if present. The expected output should still be date values. Update the assertion to expect the same date values (date_trunc('day', timestamp) on DuckLake data).

**test/integration/test_ducklake_ci.py:**
1. Same changes as test_ducklake.py: update docstring, DDL call, and test 6.
2. Replace `time_dimensions := [...]` with a regular dimension using `date_trunc('day', ordered_at)`.
3. Update test 6 description and assertions.

**README.md:**
1. Remove the "Time dimensions" bullet point from the feature list.
2. Update the DDL signature: remove `time_dimensions` from the parameter list (5 params, not 6).
3. Remove the `time_dimensions := [...]` example from the DDL usage section.
4. Remove the "Time dimension (truncated to the defined granularity)" section from the query examples.
5. Show the date_trunc pattern as a regular dimension in the examples instead (e.g., `{'name': 'order_month', 'expr': "date_trunc('month', created_at)", ...}`).
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo test 2>&1 | tail -5 && just build 2>&1 | tail -3 && just test-sql 2>&1 | tail -5</automated>
  </verify>
  <done>
    - All proptest tests pass without granularity_overrides
    - Fuzz targets compile without HashMap/granularity_overrides
    - SQL integration tests pass with simplified DDL (no time_dimensions)
    - phase2_ddl.test creates views without time_dimensions param
    - phase4_query.test typed_date_test uses regular dimension with date_trunc
    - DuckLake integration tests updated (test 6 uses date_trunc dimension)
    - README reflects simplified 5-param DDL signature
    - `just test-all` passes
  </done>
</task>

</tasks>

<verification>
Run the full quality gate to confirm nothing is broken:

```bash
just test-all
```

This runs: cargo test (unit + proptest), just test-sql (sqllogictest), and just test-ducklake-ci.

Additionally verify no references remain:
```bash
grep -r "time_dimension\|granularity_overrides\|dim_type\|VALID_GRANULARITIES\|extract_map_strings\|validate_granularity" src/ tests/ fuzz/ test/ --include='*.rs' --include='*.test' --include='*.py'
```
(README.md may mention "granularity" in the context of date_trunc examples, which is fine.)
</verification>

<success_criteria>
1. `just test-all` passes (cargo test + sqllogictest + ducklake CI)
2. No `time_dimensions` named parameter in DDL functions
3. No `granularities` named parameter in query function
4. Dimension struct has 4 fields: name, expr, source_table, output_type
5. QueryRequest struct has 2 fields: dimensions, metrics
6. Zero grep hits for `dim_type`, `granularity_overrides`, `time_dimensions_type`, `extract_map_strings`, `validate_granularity` in src/
7. README shows simplified DDL with no time_dimensions parameter
</success_criteria>

<output>
After completion, create `.planning/quick/12-remove-time-dimensions-and-granularities/12-SUMMARY.md`
</output>
