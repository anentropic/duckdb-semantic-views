# Gap Detection Report

**Generated:** 2026-04-23
**Source root:** src/
**Language:** rust
**Total exported symbols:** N/A (SQL DDL interface, not Rust library)
**Undocumented symbols:** 0

## SQL Interface Coverage

All user-facing SQL statements and functions have dedicated reference pages:

| SQL Statement / Function | Reference Page | Status |
|--------------------------|----------------|--------|
| `CREATE SEMANTIC VIEW` | `reference/create-semantic-view.rst` | Documented |
| `ALTER SEMANTIC VIEW` | `reference/alter-semantic-view.rst` | Documented |
| `DROP SEMANTIC VIEW` | `reference/drop-semantic-view.rst` | Documented |
| `DESCRIBE SEMANTIC VIEW` | `reference/describe-semantic-view.rst` | Documented |
| `SHOW SEMANTIC VIEWS` | `reference/show-semantic-views.rst` | Documented |
| `SHOW SEMANTIC DIMENSIONS` | `reference/show-semantic-dimensions.rst` | Documented |
| `SHOW SEMANTIC METRICS` | `reference/show-semantic-metrics.rst` | Documented |
| `SHOW SEMANTIC FACTS` | `reference/show-semantic-facts.rst` | Documented |
| `SHOW SEMANTIC MATERIALIZATIONS` | `reference/show-semantic-materializations.rst` | Documented |
| `SHOW SEMANTIC DIMENSIONS FOR METRIC` | `reference/show-semantic-dimensions-for-metric.rst` | Documented |
| `SHOW COLUMNS IN SEMANTIC VIEW` | `reference/show-columns-semantic-view.rst` | Documented |
| `GET_DDL('SEMANTIC_VIEW', ...)` | `reference/get-ddl.rst` | Documented |
| `READ_YAML_FROM_SEMANTIC_VIEW()` | `reference/read-yaml-from-semantic-view.rst` | Documented |
| `semantic_view()` | `reference/semantic-view-function.rst` | Documented |
| `explain_semantic_view()` | `reference/explain-semantic-view-function.rst` | Documented |
| `FROM YAML` / `FROM YAML FILE` | `reference/create-semantic-view.rst` | Documented |
| YAML format specification | `reference/yaml-format.rst` | Documented (new) |

## How-To Coverage

| Feature | How-To Page | Status |
|---------|-------------|--------|
| FACTS | `how-to/facts.rst` | Documented |
| Derived metrics | `how-to/derived-metrics.rst` | Documented |
| Role-playing dimensions | `how-to/role-playing-dimensions.rst` | Documented |
| Fan traps | `how-to/fan-traps.rst` | Documented |
| Data sources | `how-to/data-sources.rst` | Documented |
| Metadata annotations | `how-to/metadata-annotations.rst` | Documented |
| Semi-additive metrics | `how-to/semi-additive-metrics.rst` | Documented |
| Window metrics | `how-to/window-metrics.rst` | Documented |
| Wildcard selection | `how-to/wildcard-selection.rst` | Documented |
| Query facts | `how-to/query-facts.rst` | Documented |
| Materializations | `how-to/materializations.rst` | Documented |
| YAML definitions | `how-to/yaml-definitions.rst` | Documented |

## Note

The standard Rust export scanner is not applicable for this project type. Coverage is assessed against the SQL DDL interface. All v0.7.0 features (materializations, YAML definitions, YAML format reference) now have dedicated documentation pages. Previous gap report (2026-04-21) flagged 4 undocumented symbols — all are now covered.
