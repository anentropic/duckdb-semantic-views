# Plan 44-02 Summary

**Status:** Complete
**Phase:** 44-show-describe-metadata-surface-enhancements
**Plan:** 02

## What was built

Added three new DDL introspection forms: SHOW TERSE, IN SCHEMA/DATABASE scope filtering, and SHOW COLUMNS IN SEMANTIC VIEW.

### Task 1: SHOW TERSE and IN SCHEMA/DATABASE scope filtering
- New `DdlKind::ShowTerse` variant for `SHOW TERSE SEMANTIC VIEWS`
- 5-column terse output (created_on, name, kind, database_name, schema_name) via `ListTerseSemanticViewsVTab`
- Extended `ShowClauses` with `in_schema`/`in_database` fields
- `IN SCHEMA schema_name` / `IN DATABASE db_name` scope filtering for both SHOW and SHOW TERSE
- `build_filter_suffix` extended for scope filtering SQL generation
- TERSE supports all existing clauses: LIKE, STARTS WITH, LIMIT, IN SCHEMA, IN DATABASE
- Updated existing `phase34_1_1_show_filtering.test` error test for new IN SCHEMA/DATABASE syntax
- Created `phase44_show_terse_scope.test` with 158 lines of integration tests

### Task 2: SHOW COLUMNS IN SEMANTIC VIEW
- New `DdlKind::ShowColumns` variant
- New `src/ddl/show_columns.rs` module with `ShowColumnsInSemanticViewVTab`
- 8-column output: database_name, schema_name, semantic_view_name, column_name, data_type, kind, expression, comment
- `kind` column distinguishes DIMENSION, FACT, METRIC, DERIVED_METRIC
- PRIVATE facts and metrics excluded from output
- Results sorted by kind then column_name
- Created `phase44_show_columns.test` with 107 lines of integration tests

## Key files

### Created
- `src/ddl/show_columns.rs` — ShowColumnsInSemanticViewVTab (203 lines)
- `test/sql/phase44_show_terse_scope.test` — TERSE and scope filtering tests
- `test/sql/phase44_show_columns.test` — SHOW COLUMNS tests

### Modified
- `src/parse.rs` — DdlKind::ShowTerse, ShowColumns variants, IN SCHEMA/DATABASE parsing (+244/-50)
- `src/ddl/list.rs` — ListTerseSemanticViewsVTab (+123)
- `src/ddl/mod.rs` — show_columns module declaration
- `src/lib.rs` — registration of new VTab functions
- `test/sql/phase34_1_1_show_filtering.test` — updated error test for IN syntax
- `tests/parse_proptest.rs` — updated for new DdlKind variants

## Deviations

None. All requirements delivered as planned.

## Self-Check: PASSED
- `cargo test` — 527 tests pass
- `just build && just test-sql` — 24 sqllogictest files pass
