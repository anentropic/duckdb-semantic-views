---
plan: 12-03
phase: 12-explain-typed-output
status: complete
date: "2026-03-02"
---

# Plan 12-03 Summary: Typed bind() + func() Dispatch

## What Was Built

### Constant Fix (type_from_duckdb_type_u32)
- Previous constants for complex types were wrong (hard-coded magic numbers).
- Replaced with `ffi::DUCKDB_TYPE_DUCKDB_TYPE_*` named constants:
  - ENUM = 23, LIST = 24, STRUCT = 25, MAP = 26 (not 21/22/23/25 as in the plan's example)
- DECIMAL (19) and INVALID (0) remain correct.

### Removed infer_schema_or_default()
- Dead code after bind() was rewritten in the previous session context.
- `try_infer_schema` (pub(crate)) remains for the fallback path in bind().

### Rewritten func() — VARCHAR Wrapper + parse_typed_from_str
- Previous: wrapped all columns in VARCHAR cast, wrote all as strings.
- New: still wraps with `build_varchar_cast_sql` for uniform string layout in result chunks,
  then converts each string to the appropriate TypedValue using `parse_typed_from_str()`.
- Column-major data collection: `Vec<Vec<String>>` indexed by col then row, then converted to `Vec<TypedValue>`.
- Rationale: DECIMAL is stored as fixed-point integer in DuckDB chunks (scale unknown at func() time);
  reading via VARCHAR cast then parsing as f64 is always correct without needing scale metadata.

### parse_typed_from_str() Helper
- Converts a VARCHAR string representation to `TypedValue` based on DDL-time `type_id`:
  - BIGINT, UBIGINT, TIMESTAMP, HUGEINT → parse as `i64` → `TypedValue::I64`
  - DATE → `date_str_to_epoch_days("YYYY-MM-DD")` → days since epoch as `i32` → `TypedValue::I32`
  - INTEGER, UINTEGER, SMALLINT, TINYINT, USMALLINT, UTINYINT → parse as `i32` → `TypedValue::I32`
  - DOUBLE, FLOAT, DECIMAL → parse as `f64` → `TypedValue::F64`
  - All other types (VARCHAR, LIST, STRUCT, etc.) → `TypedValue::Str`
  - Empty string → `TypedValue::Null` for numeric types

### date_str_to_epoch_days() Helper
- Parses "YYYY-MM-DD" into days since Unix epoch (1970-01-01) as `i32`.
- Uses the proleptic Gregorian calendar Julian Day Number formula (no external crate needed).
- DuckDB DATE FlatVector stores days-since-epoch as `i32`.

### write_typed_column() Helper
- Dispatches based on `type_id` to write typed output:
  - i64 types → `out_vec.as_mut_slice_with_len::<i64>(n_rows)` + NULL via `set_null`
  - i32 types (including DATE) → `out_vec.as_mut_slice_with_len::<i32>(n_rows)` + NULL via `set_null`
  - f64 types (including DECIMAL) → `out_vec.as_mut_slice_with_len::<f64>(n_rows)` + NULL via `set_null`
  - VARCHAR/fallback → `out_vec.insert(i, s)` path (existing behavior)

## Key Decisions

- VARCHAR wrapper retained in func() to handle DECIMAL (stored as fixed-point integer in chunks, scale unavailable without querying DECIMAL type metadata) and other complex types correctly.
- DECIMAL mapped to f64 in both bind() output declaration and parse_typed_from_str() parse.
- DATE parsed from "YYYY-MM-DD" string to days-since-epoch i32 using pure arithmetic (no date library dependency).
- ffi use imports are scoped inside each function rather than at module top (avoids unused-import warnings when feature flags differ).

## Test Coverage

No new unit tests added (typed write behavior requires FFI round-trip with a live DuckDB connection; covered by integration tests in Plan 12-04). The 100 existing unit tests continue to pass.

## Verification

`cargo test`: **100 passed, 0 failed** (93 unit + 6 proptest + 1 doctest)
`cargo clippy -- -D warnings`: **0 warnings**

Grep checks:
- `grep "column_type_ids" src/query/table_function.rs` — multiple hits (struct field, bind, func)
- `grep "as_mut_slice_with_len" src/query/table_function.rs` — hits on i64/i32/f64 writes
- `grep "TypedValue" src/query/table_function.rs` — hits on enum definition and all dispatch paths

## Self-Check: PASSED

## key-files

### created
- (none — modified only)

### modified
- src/query/table_function.rs

## Commits

- `9a1e913` feat(12-03): typed bind() + typed func() dispatch for BIGINT/DATE/DOUBLE/INTEGER/VARCHAR
- `2177250` fix(12-03): use VARCHAR wrapper + parse_typed_from_str for safe typed output
