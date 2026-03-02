---
plan: 12-02
phase: 12-explain-typed-output
status: complete
date: "2026-03-02"
---

# Plan 12-02 Summary: DDL Rename + LIMIT 0 Inference + CAST Codegen

## What Was Built

### DDL Rename (lib.rs)
- `define_semantic_view` → `create_semantic_view`
- `define_or_replace_semantic_view` → `create_or_replace_semantic_view`
- New: `create_semantic_view_if_not_exists` registration

### DefineState Extension (define.rs)
- Added `if_not_exists: bool` field to `DefineState`
- When `if_not_exists = true` and catalog_insert returns "already exists" error: silently succeeds

### DDL-Time Type Inference (define.rs)
- `invoke()` now calls `try_infer_schema` via `persist_conn` after `expand()` generates SQL
- Runs `LIMIT 0` on expanded SQL to get column names and DuckDB type enums
- Stores both as parallel vecs in `parsed.def.column_type_names` and `column_types_inferred`
- Re-serializes `parsed.def` to JSON AFTER inference (so types are stored in catalog)
- In-memory databases: skips inference, both vecs stay empty (VARCHAR fallback in bind)

### CAST Codegen (expand.rs)
- When `dim.output_type` is `Some(type_str)`: emits `CAST(base_expr AS type_str)` in SELECT
- When `met.output_type` is `Some(type_str)`: emits `CAST(agg_expr AS type_str)` in SELECT
- `None` output_type: no CAST, existing behavior preserved

### Visibility Change (table_function.rs)
- `try_infer_schema` changed from private to `pub(crate)` so define.rs can call it

## Key Decisions

- `try_infer_schema` made `pub(crate)` rather than moving to a shared module (minimal change)
- DDL-time inference runs ONLY when `persist_conn` is `Some` (file-backed databases)
- JSON re-serialization moved to AFTER inference so stored catalog JSON includes type data
- `if_not_exists` and `or_replace` are distinct fields (not enum) to match DefineState's existing pattern

## Test Coverage

3 new unit tests in `mod phase12_cast_tests` in expand.rs:
- `output_type_on_metric_emits_cast` — BIGINT CAST on metric
- `output_type_on_dimension_emits_cast` — INTEGER CAST on dimension
- `no_output_type_no_cast` — None output_type preserves existing behavior

## Verification

`cargo test`: **100 passed, 0 failed** (93 unit + 6 proptest + 1 doctest)

Grep checks:
- `grep "create_semantic_view" src/lib.rs` — 4 hits (register, register, register, comment)
- `grep "if_not_exists" src/ddl/define.rs` — multiple hits (field + condition)
- `grep "column_types_inferred" src/ddl/define.rs` — hit on assignment
- `grep "pub(crate)" src/query/table_function.rs` — hit on try_infer_schema

## Self-Check: PASSED

## key-files

### created
- (none — modified only)

### modified
- src/lib.rs
- src/ddl/define.rs
- src/expand.rs
- src/query/table_function.rs

## Commits

- `413ce45` feat(12-02): DDL rename to create_semantic_view + LIMIT 0 inference + CAST codegen
