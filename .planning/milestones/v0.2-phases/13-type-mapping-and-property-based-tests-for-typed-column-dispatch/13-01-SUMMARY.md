---
phase: 13-type-mapping-and-property-based-tests-for-typed-column-dispatch
plan: 01
subsystem: query
tags: [rust, ffi, duckdb, binary-read, type-dispatch, bugfix]

requires:
  - phase: 12-explain-and-typed-output-columns
    provides: typed output column framework (write_typed_column, TypedValue enum, bind() type inference)
provides:
  - binary-read dispatch pipeline replacing VARCHAR cast intermediary
  - fixes for TIMESTAMP all-NULL, BOOLEAN UB, DECIMAL/LIST/ENUM output type bugs
  - read_typed_from_vector() for direct binary chunk reading
  - declare_output_type() for DECIMAL(width,scale) and LIST(child) output declarations
---

## Summary

Replaced the VARCHAR-cast intermediary pipeline in `semantic_view()` with direct binary chunk reads per type. Fixed three silent data-corruption bugs: TIMESTAMP all-NULL, BOOLEAN UB, and DECIMAL/LIST declared as VARCHAR output.

## What Was Built

**Binary-read dispatch (`read_typed_from_vector`):**
- Central dispatch function reading DuckDB result chunk vectors via raw pointer arithmetic
- Covers all scalar types: BOOLEAN, TINYINT through UBIGINT, FLOAT, DOUBLE, DATE, TIMESTAMP variants, TIME, HUGEINT/UHUGEINT (truncated), DECIMAL (internal type dispatch), UUID (formatted), ENUM (dictionary decode), LIST (parent + child vector read), VARCHAR

**Extended `TypedValue` enum:**
- Added: `Bool(bool)`, `I8(i8)`, `I16(i16)`, `U8(u8)`, `U16(u16)`, `U32(u32)`, `U64(u64)`, `F32(f32)`, `I128(i128)`, `List(Vec<TypedValue>)`
- Retained: `Null`, `I32(i32)`, `I64(i64)`, `F64(f64)`, `Str(String)`

**Dead code deleted:**
- `build_varchar_cast_sql()` — VARCHAR cast wrapper SQL builder
- `parse_typed_from_str()` — string parse dispatch (replaced by binary reads)
- `date_str_to_epoch_days()` — string-to-i32 date parse (replaced by direct i32 binary read)

**Extended `bind()` for DECIMAL/LIST/ENUM:**
- `declare_output_type()` reads DECIMAL width/scale via `duckdb_decimal_width/scale`
- LIST declares `LIST(child_type)` using `duckdb_list_type_child_type`
- ENUM declares VARCHAR output (user sees string values, not ordinals)
- `bind()` runs a LIMIT-0 query only when DECIMAL/LIST/ENUM columns are present (optimization)

**RAII management:**
- `LogicalTypeOwned` struct wrapping `duckdb_logical_type` with `Drop` implementation calling `duckdb_destroy_logical_type`

## Bug Fixes

| Bug | Cause | Fix |
|-----|-------|-----|
| TIMESTAMP all-NULL | VARCHAR cast → `parse::<i64>()` fails on "2024-01-15 10:30:00" | Direct i64 binary read from chunk vector |
| BOOLEAN UB | Type mismatch writing bool through VARCHAR string path | Read u8 (0/1) directly, write to BOOLEAN slot |
| DECIMAL as VARCHAR | No metadata available for DECIMAL width/scale in VARCHAR path | LIMIT-0 at bind time for DECIMAL/LIST/ENUM columns |

## Self-Check

- [x] `grep "build_varchar_cast_sql\|parse_typed_from_str\|date_str_to_epoch_days" src/query/table_function.rs` → 0 function matches (only comment remains)
- [x] `grep "read_typed_from_vector" src/query/table_function.rs` → 3 matches (comment + call + definition)
- [x] `grep "duckdb_decimal_width\|duckdb_decimal_scale" src/query/table_function.rs` → 2 matches in bind() path
- [x] TypedValue has Bool, I8, I16, U8, U16, U32, U64, F32, I128, List variants
- [x] `cargo test` — 93 lib tests + 6 proptest tests pass
- [x] `cargo clippy -- -D warnings` — 0 warnings

## Key Files

```
key-files:
  modified:
    - path: src/query/table_function.rs
      changes: "Binary-read dispatch pipeline, extended TypedValue enum, DECIMAL/LIST/ENUM bind(), deleted dead code"
```

## Commits

- `e8bb69c` feat(13-01): replace VARCHAR-cast pipeline with binary-read dispatch

## Notes

The `query/table_function.rs` module is gated by `#[cfg(feature = "extension")]` in `query/mod.rs`. This means inline unit tests cannot use `Connection::open_in_memory()` (which requires the bundled feature). Integration tests verifying correctness are handled by Plan 13-02 (`tests/output_proptest.rs`).
