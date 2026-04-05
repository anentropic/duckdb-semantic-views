---
phase: 40-show-command-alignment
plan: 01
subsystem: ddl
tags: [vtab, show-commands, snowflake-alignment, duckdb]

# Dependency graph
requires:
  - phase: 39-metadata-storage
    provides: "created_on, database_name, schema_name on SemanticViewDefinition; output_type on Fact"
provides:
  - "5-column SHOW SEMANTIC VIEWS (created_on, name, kind, database_name, schema_name)"
  - "6-column SHOW SEMANTIC DIMENSIONS (database_name, schema_name, semantic_view_name, table_name, name, data_type)"
  - "6-column SHOW SEMANTIC METRICS (same schema as dims)"
  - "6-column SHOW SEMANTIC FACTS (same schema, data_type from Fact.output_type)"
affects: [40-02-show-command-alignment, 41-describe-rewrite]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Snowflake-aligned SHOW column schema: database_name, schema_name, semantic_view_name, table_name, name, data_type"
    - "ListRow named struct replacing tuple in list.rs"

key-files:
  created: []
  modified:
    - src/ddl/list.rs
    - src/ddl/show_facts.rs
    - src/ddl/show_dims.rs
    - src/ddl/show_metrics.rs
    - test/sql/phase34_1_show_commands.test

key-decisions:
  - "data_type column is empty for dimensions/metrics (no type inference at define time) and for facts on empty tables"
  - "kind column uses SEMANTIC_VIEW (with underscore) per Snowflake docs"

patterns-established:
  - "6-column SHOW schema: database_name, schema_name, semantic_view_name, table_name, name, data_type"
  - "SHOW VIEWS 5-column schema: created_on, name, kind, database_name, schema_name"

requirements-completed: [SHOW-01, SHOW-02, SHOW-03, SHOW-04, SHOW-06, SHOW-07]

# Metrics
duration: 5min
completed: 2026-04-02
---

# Phase 40 Plan 01: SHOW Command Column Schema Alignment Summary

**Snowflake-aligned column schemas for all 4 SHOW SEMANTIC VTabs: list.rs (5-col), show_dims/metrics/facts.rs (6-col each), with expr and source_table columns removed**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-02T11:26:09Z
- **Completed:** 2026-04-02T11:30:41Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- SHOW SEMANTIC VIEWS expanded from 2 columns (name, base_table) to 5 Snowflake-aligned columns (created_on, name, kind, database_name, schema_name)
- SHOW SEMANTIC DIMENSIONS/METRICS updated from 5 columns to 6 columns (database_name, schema_name, semantic_view_name, table_name, name, data_type) -- expr column removed
- SHOW SEMANTIC FACTS updated from 4 columns to 6 columns (same schema) -- expr column removed, data_type added from Fact.output_type
- All sqllogictest expectations in phase34_1_show_commands.test updated to match new schemas

## Task Commits

Each task was committed atomically:

1. **Task 1: Update list.rs and show_facts.rs VTab schemas** - `41363b0` (feat)
2. **Task 2: Update show_dims.rs, show_metrics.rs, and phase34_1_show_commands.test** - `1aecde0` (feat)

## Files Created/Modified
- `src/ddl/list.rs` - 2-col to 5-col SHOW SEMANTIC VIEWS with metadata from SemanticViewDefinition
- `src/ddl/show_facts.rs` - 4-col to 6-col with data_type from Fact.output_type
- `src/ddl/show_dims.rs` - 5-col to 6-col, expr removed, database_name/schema_name added
- `src/ddl/show_metrics.rs` - 5-col to 6-col, same transformation as dims
- `test/sql/phase34_1_show_commands.test` - All SHOW expectations updated to 6-col format

## Decisions Made
- `data_type` column is empty string for dimensions and metrics (no define-time type inference exists for these yet) and for facts on empty tables (typeof() returns no rows)
- `kind` value is `SEMANTIC_VIEW` (with underscore) per Snowflake documentation
- list.rs now uses `SemanticViewDefinition::from_json()` instead of raw `serde_json::Value` parsing for metadata extraction

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Plan 40-02 updates `show_dims_for_metric.rs` and the filtering/SHOW VIEWS sqllogictest files
- `show_dims_for_metric.rs` still has old 5-column schema with `expr` -- will be aligned in Plan 02
- `phase34_1_1_show_filtering.test` and `phase34_1_show_dims_for_metric.test` need matching updates in Plan 02

---
## Self-Check: PASSED

All 5 modified files confirmed present. Both task commits (41363b0, 1aecde0) verified in git log.

---
*Phase: 40-show-command-alignment*
*Completed: 2026-04-02*
