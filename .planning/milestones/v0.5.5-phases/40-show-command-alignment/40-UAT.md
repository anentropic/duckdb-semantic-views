---
status: complete
phase: 40-show-command-alignment
source: [40-01-SUMMARY.md, 40-02-SUMMARY.md]
started: 2026-04-02T14:00:00Z
updated: 2026-04-02T14:30:00Z
---

## Current Test

[testing complete]

## Tests

### 1. SHOW SEMANTIC VIEWS — 5-column Snowflake schema
expected: SHOW SEMANTIC VIEWS returns 5 columns (created_on, name, kind, database_name, schema_name) with metadata populated
result: pass

### 2. SHOW SEMANTIC DIMENSIONS — 6-column schema, no expr
expected: SHOW SEMANTIC DIMENSIONS IN sales_analysis returns 6 columns (database_name, schema_name, semantic_view_name, table_name, name, data_type) — no expr column
result: issue
reported: "table_name shows alias 'o' instead of actual table name 'orders'. data_type is blank."
severity: major

### 3. SHOW SEMANTIC METRICS — 6-column schema, no expr
expected: SHOW SEMANTIC METRICS IN sales_analysis returns 6 columns (same schema as DIMENSIONS) — no expr column
result: issue
reported: "same issues as SHOW DIMENSIONS — alias in table_name, blank data_type"
severity: major

### 4. SHOW SEMANTIC FACTS — 6-column schema with data_type
expected: After adding FACTS, SHOW SEMANTIC FACTS returns 6 columns including data_type inferred from expression type
result: issue
reported: "same issues — alias in table_name, blank data_type even for facts where inference should work"
severity: major

### 5. SHOW DIMS FOR METRIC — 4-column with BOOLEAN required
expected: SHOW SEMANTIC DIMENSIONS IN sales_analysis FOR METRIC total_amount returns 4 columns (table_name, name, data_type, required) where required is false (BOOLEAN)
result: issue
reported: "same issues with table_name and data_type"
severity: major

### 6. SHOW filtering — LIKE, STARTS WITH, LIMIT
expected: SHOW SEMANTIC VIEWS LIKE 'sales%' filters correctly; SHOW SEMANTIC DIMENSIONS IN sales_analysis LIMIT 1 returns 1 row
result: issue
reported: "filtering works correctly but output rows still have alias in table_name and blank data_type"
severity: major

## Summary

total: 6
passed: 1
issues: 5
pending: 0
skipped: 0
blocked: 0

## Gaps

- truth: "table_name column in SHOW DIMS/METRICS/FACTS/DIMS-FOR-METRIC should show actual table name, not alias"
  status: failed
  reason: "User reported: table_name shows alias 'o' instead of actual table name 'orders'"
  severity: major
  test: 2
  root_cause: "source_table in model stores DDL alias; SHOW VTabs use it directly without resolving via def.tables alias→table mapping"
  artifacts:
    - path: "src/ddl/show_dims.rs"
      issue: "line 77: d.source_table used directly instead of resolved table name"
    - path: "src/ddl/show_metrics.rs"
      issue: "same pattern"
    - path: "src/ddl/show_facts.rs"
      issue: "same pattern"
    - path: "src/ddl/show_dims_for_metric.rs"
      issue: "same pattern"
  missing:
    - "Build alias→table_name HashMap from def.tables in each collect_* function"
    - "Resolve source_table alias to actual table name before emitting"
    - "Add sqllogictest coverage asserting actual table names in output"

- truth: "data_type column should show inferred types for facts (and ideally dims/metrics)"
  status: failed
  reason: "User reported: data_type blank for all object kinds including facts where typeof inference should work"
  severity: major
  test: 4
  root_cause: "Fact output_type inference at define time may not be populating correctly; dims/metrics have no inference (design gap)"
  artifacts:
    - path: "src/ddl/define.rs"
      issue: "typeof(expr) inference may be failing silently"
  missing:
    - "Investigate why fact output_type is None after define"
    - "Add test coverage for data_type column values"
