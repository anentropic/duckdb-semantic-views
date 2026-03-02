---
plan: 12-01
phase: 12-explain-typed-output
status: complete
date: "2026-03-02"
---

# Plan 12-01 Summary: Model Fields for Typed Output

## What Was Built

Added the typed output data model fields required by Plans 02 and 03:

- `output_type: Option<String>` added to `Dimension` struct (after `granularity`)
- `output_type: Option<String>` added to `Metric` struct (after `source_table`)
- `column_type_names: Vec<String>` added to `SemanticViewDefinition` (after `facts`)
- `column_types_inferred: Vec<u32>` added to `SemanticViewDefinition` (after `column_type_names`)

All new fields use `#[serde(default)]` for backward compatibility with existing stored catalog JSON.

`Default` derive was added to `Metric` and `SemanticViewDefinition` to allow struct literal initialization across the test suite.

## Key Decisions

- `column_type_names` and `column_types_inferred` are parallel vecs: names[i] identifies types[i]
- `u32` chosen for `column_types_inferred` to match `ffi::duckdb_type` (libduckdb-sys enum values)
- `#[serde(default)]` ensures old catalog JSON without these fields deserializes to empty vecs
- `Default` derive added to `Metric` and `SemanticViewDefinition` to simplify test fixture construction

## Test Coverage

Five new unit tests in `mod phase12_model_tests`:
- `output_type_on_dimension_roundtrips` — BIGINT round-trip
- `output_type_on_metric_roundtrips` — DOUBLE round-trip
- `column_types_inferred_roundtrips` — parallel vec round-trip
- `old_json_without_output_type_deserializes` — backward compat
- `old_json_without_column_types_inferred_deserializes` — backward compat

## Verification

`cargo test`: **97 passed, 0 failed** (90 unit + 6 proptest + 1 doctest)

## Self-Check: PASSED

## key-files

### created
- (none — modified only)

### modified
- src/model.rs
- src/expand.rs (struct literal updates)
- src/ddl/parse_args.rs (struct literal updates)
- tests/expand_proptest.rs (struct literal updates)

## Commits

- `cbe30a0` feat(12-01): add output_type + column_types_inferred fields to model.rs
