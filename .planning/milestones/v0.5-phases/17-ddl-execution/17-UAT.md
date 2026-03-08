---
status: complete
phase: 17-ddl-execution
source: [17-01-SUMMARY.md]
started: 2026-03-07T23:50:00Z
updated: 2026-03-08T15:45:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Native DDL Creates a Semantic View
expected: `CREATE SEMANTIC VIEW myview (tables := [...], dimensions := [...], metrics := [...])` completes without error via DuckDB CLI sqllogictest runner.
result: pass
note: Verified via `just test-sql` — phase16_parser.test creates a view via native DDL with `statement ok`.

### 2. Native DDL View is Queryable
expected: After creating a view via native DDL, `SELECT * FROM semantic_view('myview', dimensions := [...], metrics := [...])` returns expected aggregated results.
result: pass
note: Verified via `just test-sql` — phase16_parser.test queries native DDL view and asserts East=300, West=150.

### 3. Function-Based DDL Still Works
expected: `create_semantic_view(...)` still works alongside native DDL. Both views are independently queryable.
result: pass
note: Verified via `just test-sql` — phase16_parser.test creates a second view via function-based DDL and queries both.

## Summary

total: 3
passed: 3
issues: 0
pending: 0
skipped: 0

## Known Issue (out of scope)

**Python vtab bind panic:** Calling `create_semantic_view()` from Python DuckDB (via `conn.execute()`) crashes with `panic in a function that cannot unwind` at `duckdb::vtab::bind`. This is a **pre-existing issue** in the duckdb Rust crate's vtab FFI boundary — `unsafe extern "C" fn bind<T>` does not wrap `T::bind()` in `catch_unwind`. Not a Phase 17 regression. DuckLake CI tests pass under specific conditions (uv-managed environment with DuckLake attached catalog). Affects all vtab-based table functions, not just native DDL.

## Gaps

[none]
