---
phase: 13-type-mapping-and-property-based-tests-for-typed-column-dispatch
plan: 02
subsystem: test
tags: [rust, proptest, duckdb, ffi, binary-read, regression-tests]

requires:
  - phase: 13-type-mapping-and-property-based-tests-for-typed-column-dispatch
    plan: 01
    provides: binary-read dispatch pipeline, extended TypedValue enum, RawDb test helper
provides:
  - 36 property-based tests covering the full binary-read pipeline
  - TIMESTAMP-NULL and BOOLEAN-UB regression tests
  - DECIMAL and LIST(BIGINT) integration tests
  - NULL propagation tests for all scalar types
---

## Summary

Created `tests/output_proptest.rs` with 36 property-based tests across two layers validating the binary-read pipeline introduced in Plan 13-01. Fixed a critical test infrastructure bug where `raw_connection()` used incorrect pointer arithmetic on `duckdb::Connection` (which contains `RefCell<InnerConnection>`, not a raw pointer at offset 0), causing SIGSEGV. Replaced with `RawDb` — a RAII wrapper that opens a DuckDB database directly via the C API.

## What Was Built

**`RawDb` helper in `src/lib.rs`** (under `#[cfg(not(feature = "extension"))]`):
- Opens an in-memory DuckDB database + connection via `ffi::duckdb_open` + `ffi::duckdb_connect`
- Provides `exec(&self, sql)` for DDL/DML setup
- Automatically disconnects and closes on `Drop`
- Avoids all pointer arithmetic on `duckdb::Connection`

**`tests/output_proptest.rs`** (36 tests, 666 lines):

Layer 1 — unit tests (deterministic boundary values):
- `timestamp_returns_non_null` — TIMESTAMP NULL bug regression (key test)
- `boolean_reads_correctly` — BOOLEAN UB bug regression
- `integer_boundary_values`, `bigint_boundary_values` — boundary values for signed integers
- `date_binary_read`, `date_epoch_zero`, `date_before_epoch` — DATE direct binary read
- `timestamp_known_value` — specific epoch microsecond value
- `float_reads_correctly`, `double_reads_correctly` — float boundary values
- `null_propagation_integer/bigint/boolean/timestamp` — NULL returns `TestValue::Null`
- `tinyint_boundary_values`, `utinyint_boundary_values`, `ubigint_boundary_values`
- `decimal_backing_integer`, `decimal_negative_value` — DECIMAL internal integer representation
- `list_bigint_reads_correctly`, `list_empty` — LIST offset/length tracking
- `varchar_reads_correctly`, `varchar_long_string` — inline vs pointer VARCHAR layout

Layer 1 — Proptest unit PBTs (50 cases each):
- `bigint_binary_read`, `integer_binary_read`, `boolean_binary_read` — arbitrary value roundtrip
- `double_binary_read` — uses `{v:.17e}` format and `to_bits()` comparison for exact roundtrip

Layer 2 — Integration PBTs (20 cases each, full column roundtrip):
- `bigint_column_roundtrip`, `integer_column_roundtrip`, `boolean_column_roundtrip`
- `double_column_roundtrip` — bit-exact comparison via `to_bits()`
- `timestamp_column_roundtrip` — TIMESTAMP NULL fix regression
- `date_column_roundtrip` — DATE direct binary read regression

Deterministic integration tests:
- `decimal_roundtrip` — 3 DECIMAL(10,2) rows, backing integer verification
- `list_bigint_roundtrip` — 3 LIST(BIGINT) rows with offset/length tracking
- `null_propagation` — mixed NULL/non-NULL rows

## Key Bugs Encountered and Fixed

| Bug | Root Cause | Fix |
|-----|-----------|-----|
| SIGSEGV in `raw_connection()` | `duckdb::Connection` layout is `RefCell<InnerConnection>`, not `*duckdb_connection` at offset 0 | Replaced with `RawDb` that opens database via C API directly |
| `bigint_boundary_values` / `integer_boundary_values` fail | DuckDB parses `-9223372036854775808` as `INT128(9223372036854775808)` — overflows BIGINT | Used `(-9223372036854775807::BIGINT - 1::BIGINT)` pattern |
| `tinyint_boundary_values` fails | Same issue: DuckDB parses `-128` as `INT32(128)` before casting to TINYINT | Used `(-127::TINYINT - 1::TINYINT)` |
| `timestamp_column_roundtrip` fails | `epoch_us(BIGINT)` doesn't exist; INTERVAL multiplier is INT32-limited | Used `INTEGER * INTERVAL '1 microsecond'` with restricted range `0..=2_000_000_000` |
| `double_binary_read` / `double_column_roundtrip` fail | Large f64 formatting (`1e288`) loses precision through SQL round-trip | Used `{v:.17e}` format + `to_bits()` bit-exact comparison |

## Self-Check

- [x] `tests/output_proptest.rs` exists — 666 lines (well above 150 minimum)
- [x] `cargo test --test output_proptest -- --test-threads=1` — 36 passed, 0 failed
- [x] `cargo test` — 93 lib + 6 expand_proptest + 36 output_proptest + 1 doc test = 136 total, 0 failures
- [x] `cargo clippy -- -D warnings` — 0 warnings
- [x] TIMESTAMP NULL regression test: `unit_tests::timestamp_returns_non_null` — PASS
- [x] BOOLEAN UB regression test: `unit_tests::boolean_reads_correctly` — PASS
- [x] DECIMAL integration test: `decimal_roundtrip` — PASS
- [x] LIST(BIGINT) integration test: `list_bigint_roundtrip` — PASS
- [x] NULL propagation test: `null_propagation` — PASS

## Key Files

```
key-files:
  modified:
    - path: src/lib.rs
      changes: "Added RawDb struct to test_helpers module (replaces raw_connection); added Safety doc to exec()"
  created:
    - path: tests/output_proptest.rs
      changes: "36 property-based tests across unit + integration layers"
```

## Commits

- `943d868` feat(13-02): add output_proptest.rs — binary-read pipeline property-based tests
