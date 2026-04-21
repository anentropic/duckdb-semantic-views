# Gap Report

**Generated:** 2026-04-21
**Source root:** src/
**Language:** rust
**Total undocumented symbols:** 4 (user-facing features from v0.7.0 phases 54-57)
**Potentially stale pages:** 29

## Undocumented Symbols

New in v0.7.0 (phases 54-57) — no dedicated documentation pages exist for these features:

### MATERIALIZATIONS clause (src/body_parser.rs, src/model.rs)
- `Materialization` struct — declares a pre-aggregated table mapped to a subset of dimensions and metrics
- MATERIALIZATIONS keyword body section in CREATE SEMANTIC VIEW DDL

### Materialization routing (src/expand/sql_gen.rs)
- Automatic query routing to pre-aggregated tables when queried dimensions/metrics are a subset of a materialization's declared coverage

### YAML export (src/ddl/read_yaml.rs, src/render_yaml.rs)
- `READ_YAML_FROM_SEMANTIC_VIEW('name')` scalar function — exports semantic view definition as YAML
- `render_yaml_export` — internal render logic with field stripping (strips column_type_names, created_on, database/schema context)

### Materialization introspection (src/ddl/show_materializations.rs)
- `SHOW SEMANTIC MATERIALIZATIONS IN SEMANTIC VIEW 'name'` — per-view materialization listing
- `SHOW SEMANTIC MATERIALIZATIONS` — cross-view materialization listing
- Materialization details in DESCRIBE and EXPLAIN output

## Potentially Stale Pages

29 doc pages are older than recent source changes:

- `docs/explanation/semantic-views-vs-regular-views.rst` (doc: 2026-03-27, source: 2026-04-21)
- `docs/how-to/data-sources.rst` (doc: 2026-03-27, source: 2026-04-21)
- `docs/how-to/derived-metrics.rst` (doc: 2026-03-27, source: 2026-04-21)
- `docs/how-to/fan-traps.rst` (doc: 2026-03-27, source: 2026-04-21)
- `docs/how-to/role-playing-dimensions.rst` (doc: 2026-03-27, source: 2026-04-21)
- `docs/reference/drop-semantic-view.rst` (doc: 2026-03-27, source: 2026-04-21)
- `docs/index.rst` (doc: 2026-04-05, source: 2026-04-21)
- `docs/tutorials/getting-started.rst` (doc: 2026-04-05, source: 2026-04-21)
- `docs/tutorials/multi-table.rst` (doc: 2026-04-05, source: 2026-04-21)
- `docs/explanation/snowflake-comparison.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/how-to/facts.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/how-to/metadata-annotations.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/how-to/query-facts.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/how-to/semi-additive-metrics.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/how-to/wildcard-selection.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/how-to/window-metrics.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/alter-semantic-view.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/create-semantic-view.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/describe-semantic-view.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/error-messages.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/explain-semantic-view-function.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/get-ddl.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/semantic-view-function.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/show-columns-semantic-view.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/show-semantic-dimensions-for-metric.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/show-semantic-dimensions.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/show-semantic-facts.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/show-semantic-metrics.rst` (doc: 2026-04-17, source: 2026-04-21)
- `docs/reference/show-semantic-views.rst` (doc: 2026-04-17, source: 2026-04-21)

## Note

Compared to previous gap report (2026-04-13): all 4 undocumented symbols are newly discovered — they were added in v0.7.0 phases 54-57. The previous report had 7 stale-content issues (e.g., PARTITION BY variant gaps); those may or may not be resolved.

The scanner output includes internal Rust structs (VTab bindings, graph validators, etc.).
These are implementation details. Since `api_reference: "manual"` is set, they are excluded
from gap tracking.
