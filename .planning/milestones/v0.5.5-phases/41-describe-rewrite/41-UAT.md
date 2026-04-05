---
status: complete
phase: 41-describe-rewrite
source: [41-01-SUMMARY.md, 41-02-SUMMARY.md]
started: 2026-04-02T14:00:00Z
updated: 2026-04-02T14:30:00Z
---

## Current Test

[testing complete]

## Tests

### 1. DESCRIBE property-per-row format
expected: DESCRIBE SEMANTIC VIEW returns 5-column property-per-row output (object_kind, object_name, parent_entity, property, property_value) — not the old single-row JSON blob
result: issue
reported: "Format correct (5 columns, property-per-row), but TABLE object_name shows alias 'o' instead of 'orders', parent_entity shows alias, TABLE property shows alias, DATA_TYPE blank"
severity: major

### 2. DESCRIBE TABLE properties
expected: TABLE rows include BASE_TABLE_NAME, PRIMARY_KEY, BASE_TABLE_DATABASE_NAME, BASE_TABLE_SCHEMA_NAME properties. RELATIONSHIP rows show TABLE, REF_TABLE, FOREIGN_KEY, REF_KEY.
result: issue
reported: "Property structure correct but TABLE object_name shows alias, RELATIONSHIP TABLE/REF_TABLE show aliases, DATA_TYPE blank for facts"
severity: major

### 3. DESCRIBE DIMENSION and METRIC properties
expected: DIMENSION rows show TABLE, EXPRESSION, DATA_TYPE properties. METRIC rows show TABLE, EXPRESSION, DATA_TYPE properties.
result: issue
reported: "Same alias and blank DATA_TYPE issues"
severity: major

### 4. Error help message syntax
expected: Query a semantic view with no args — error message suggests using DESCRIBE SEMANTIC VIEW (not old function syntax)
result: pass

## Summary

total: 4
passed: 1
issues: 3
pending: 0
skipped: 0
blocked: 0

## Gaps

- truth: "DESCRIBE should resolve aliases to actual table names in object_name, parent_entity, and TABLE property"
  status: failed
  reason: "User reported: TABLE object_name shows 'o' not 'orders', parent_entity and TABLE property also show aliases"
  severity: major
  test: 1
  root_cause: "Same as Phase 40 — describe.rs uses alias from model without resolving via def.tables"
  artifacts:
    - path: "src/ddl/describe.rs"
      issue: "collect_table_rows uses t.alias for object_name; collect_dim/metric/fact_rows use source_table alias for parent_entity and TABLE property"
  missing:
    - "Resolve aliases to actual table names in all describe collect_* functions"
    - "Update sqllogictest expectations to assert actual table names"
