# Gap Report

**Generated:** 2026-03-25
**Source root:** src/
**Language:** rust
**Total undocumented symbols:** 0

## Coverage Assessment

All user-facing SQL DDL verbs and table functions are documented:

| Feature | Doc Page |
|---|---|
| CREATE SEMANTIC VIEW | `docs/reference/create-semantic-view.rst` |
| ALTER SEMANTIC VIEW … RENAME TO | `docs/reference/alter-semantic-view.rst` |
| DROP SEMANTIC VIEW | `docs/reference/drop-semantic-view.rst` |
| DESCRIBE SEMANTIC VIEW | `docs/reference/describe-semantic-view.rst` |
| SHOW SEMANTIC VIEWS [LIKE/STARTS WITH/LIMIT] | `docs/reference/show-semantic-views.rst` |
| SHOW SEMANTIC DIMENSIONS [LIKE/IN/STARTS WITH/LIMIT] | `docs/reference/show-semantic-dimensions.rst` |
| SHOW SEMANTIC METRICS [LIKE/IN/STARTS WITH/LIMIT] | `docs/reference/show-semantic-metrics.rst` |
| SHOW SEMANTIC FACTS [LIKE/IN/STARTS WITH/LIMIT] | `docs/reference/show-semantic-facts.rst` |
| SHOW SEMANTIC DIMENSIONS … FOR METRIC | `docs/reference/show-semantic-dimensions-for-metric.rst` |
| semantic_view() table function | `docs/reference/semantic-view-function.rst` |
| explain_semantic_view() | `docs/reference/explain-semantic-view-function.rst` |
| Error messages reference | `docs/reference/error-messages.rst` |

## Previously Identified Gaps (Now Closed)

The 2026-03-22 gap report identified 3 undocumented features:
- `LIKE` clause on all SHOW SEMANTIC commands → **documented**
- `STARTS WITH` clause on all SHOW SEMANTIC commands → **documented**
- `LIMIT` clause on all SHOW SEMANTIC commands → **documented**

All gaps from the previous run have been closed. The `ShowDimensions`, `ShowMetrics`,
`ShowFacts`, and `ShowDimensionsForMetric` Rust VTab types are internal implementation
details and do not require user-facing documentation (`api_reference: "manual"`).

## Note

The scanner output includes internal Rust structs (`ShowFactsBindData`, `AlterRenameVTab`,
etc.). These are implementation details, not user-facing API surface. Since
`api_reference: "manual"` is set, these are excluded from gap tracking.
