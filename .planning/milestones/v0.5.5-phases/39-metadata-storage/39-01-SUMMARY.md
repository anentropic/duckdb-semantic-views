---
plan: 39-01
phase: 39-metadata-storage
status: complete
started: 2026-04-01
completed: 2026-04-01
---

# Plan 39-01: Metadata Storage — Summary

## What Was Built

Added 4 metadata fields to the semantic view model:

1. **`created_on: Option<String>`** on `SemanticViewDefinition` — ISO 8601 timestamp captured via `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')` at define time
2. **`database_name: Option<String>`** on `SemanticViewDefinition` — captured via `current_database()` at define time
3. **`schema_name: Option<String>`** on `SemanticViewDefinition` — captured via `current_schema()` at define time
4. **`output_type: Option<String>`** on `Fact` struct — inferred via `typeof(expr)` at define time (best-effort, falls back to None)

All fields use `#[serde(default)]` for backward compatibility with pre-v0.5.5 stored JSON.

## Key Files

### Created
- `test/sql/phase39_metadata_storage.test` — end-to-end sqllogictest for metadata round-trip

### Modified
- `src/model.rs` — 4 new fields + 4 serde roundtrip tests
- `src/ddl/define.rs` — metadata capture via `execute_sql_raw` on `catalog_conn` + fact type inference
- `src/body_parser.rs` — `output_type: None` in Fact construction
- `src/parse.rs` — 3 new fields in SemanticViewDefinition construction
- `src/expand/sql_gen.rs` — updated all Fact and SVD constructions in tests
- `src/graph/test_helpers.rs` — updated test helper SVD and Fact constructions
- `src/graph/relationship.rs` — updated test SVD constructions
- `tests/expand_proptest.rs` — updated proptest SVD constructions
- `test/sql/phase29_facts.test` — updated DESCRIBE expected output for Fact output_type field
- `test/sql/phase30_derived_metrics.test` — updated DESCRIBE expected output
- `test/sql/TEST_LIST` — added phase39 test
- `src/expand/mod.rs` — added missing pub(crate) re-exports from Phase 38

## Decisions

- Used `catalog_conn` (always available) for metadata capture, not `persist_conn` (file-backed only)
- Fact `output_type` inference uses `typeof(expr) FROM table LIMIT 1` — graceful degradation to None on empty tables or complex expressions
- Combined all 3 metadata queries into a single SQL statement for efficiency

## Self-Check: PASSED

- [x] `cargo test` — 483 tests pass (394 unit + 5 integration + 36 proptest + 42 model + 5 output + 1 doc)
- [x] `just test-all` — 18 sqllogictests pass including new phase39 test
- [x] Backward compat verified: old JSON without new fields deserializes to None
- [x] Zero behavior changes to existing queries
