---
status: complete
phase: 20-extended-ddl-statements
source: 20-01-SUMMARY.md, 20-02-SUMMARY.md
started: 2026-03-09T15:00:00Z
updated: 2026-03-09T15:00:00Z
---

## Current Test

[testing complete]

## Tests

### 1. CREATE OR REPLACE SEMANTIC VIEW
expected: Create a view, then replace it with a different metric. Second query reflects the replaced definition.
result: pass

### 2. CREATE SEMANTIC VIEW IF NOT EXISTS
expected: Creating a view that already exists with IF NOT EXISTS succeeds silently (no error).
result: pass

### 3. DROP SEMANTIC VIEW
expected: Drop removes the view. Querying after drop produces an error.
result: pass

### 4. DROP SEMANTIC VIEW IF EXISTS
expected: Dropping a non-existent view with IF EXISTS succeeds silently (no error).
result: pass

### 5. DESCRIBE SEMANTIC VIEW
expected: Returns 6 columns showing the view's definition (name, base_table, dimensions, metrics, filters, joins).
result: pass

### 6. SHOW SEMANTIC VIEWS
expected: Returns 2-column listing (name, base_table) of all defined semantic views.
result: pass

## Summary

total: 6
passed: 6
issues: 0
pending: 0
skipped: 0

## Gaps

[none yet]
