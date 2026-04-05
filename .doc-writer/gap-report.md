# Gap Report

**Generated:** 2026-04-02
**Source root:** src/
**Language:** rust
**Total undocumented symbols:** 0 (user-facing)

## Coverage Assessment

All user-facing SQL DDL verbs and table functions are documented:

| Feature | Doc Page |
|---|---|
| CREATE SEMANTIC VIEW | `docs/reference/create-semantic-view.rst` |
| ALTER SEMANTIC VIEW ... RENAME TO | `docs/reference/alter-semantic-view.rst` |
| DROP SEMANTIC VIEW | `docs/reference/drop-semantic-view.rst` |
| DESCRIBE SEMANTIC VIEW | `docs/reference/describe-semantic-view.rst` |
| SHOW SEMANTIC VIEWS [LIKE/STARTS WITH/LIMIT] | `docs/reference/show-semantic-views.rst` |
| SHOW SEMANTIC DIMENSIONS [LIKE/IN/STARTS WITH/LIMIT] | `docs/reference/show-semantic-dimensions.rst` |
| SHOW SEMANTIC METRICS [LIKE/IN/STARTS WITH/LIMIT] | `docs/reference/show-semantic-metrics.rst` |
| SHOW SEMANTIC FACTS [LIKE/IN/STARTS WITH/LIMIT] | `docs/reference/show-semantic-facts.rst` |
| SHOW SEMANTIC DIMENSIONS ... FOR METRIC | `docs/reference/show-semantic-dimensions-for-metric.rst` |
| semantic_view() table function | `docs/reference/semantic-view-function.rst` |
| explain_semantic_view() | `docs/reference/explain-semantic-view-function.rst` |
| Error messages reference | `docs/reference/error-messages.rst` |

## Outdated Documentation (v0.5.5 Breaking Changes)

6 existing reference pages have **outdated column schemas** from the v0.5.5 SHOW/DESCRIBE
alignment work. These pages describe the OLD output format:

### docs/reference/describe-semantic-view.rst
- **Was:** single-row JSON blob with 6 columns (name, base_table, dimensions, metrics, joins, facts)
- **Now:** property-per-row format with 5 columns (object_kind, object_name, parent_entity, property, property_value)

### docs/reference/show-semantic-views.rst
- **Was:** 2 columns (name, base_table)
- **Now:** 5 columns (created_on, name, kind, database_name, schema_name)

### docs/reference/show-semantic-dimensions.rst
- **Was:** 5 columns (semantic_view_name, name, expr, source_table, data_type)
- **Now:** 6 columns (database_name, schema_name, semantic_view_name, table_name, name, data_type)

### docs/reference/show-semantic-metrics.rst
- Same column changes as SHOW SEMANTIC DIMENSIONS

### docs/reference/show-semantic-facts.rst
- Same column changes as SHOW SEMANTIC DIMENSIONS

### docs/reference/show-semantic-dimensions-for-metric.rst
- **Was:** 5 columns (semantic_view_name, name, expr, source_table, data_type)
- **Now:** 4 columns (table_name, name, data_type, required BOOLEAN)

## Note

The scanner output includes internal Rust structs (`ShowFactsBindData`, `AlterRenameVTab`,
etc.). These are implementation details, not user-facing API surface. Since
`api_reference: "manual"` is set, these are excluded from gap tracking.
