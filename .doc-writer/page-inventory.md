# Page Inventory -- DDL-Time Type Inference for Dimensions and Metrics

**Generated:** 2026-04-26
**Based on:** 6 flagged stale pages related to commit `7c58f54` (feat: infer DATA_TYPE for dimensions and metrics at DDL time)
**Branch:** feat/dim-metric-type-inference
**Scan method:** Each flagged doc page read in full. Source changes reviewed via `git diff df769cd..7c58f54` covering `src/ddl/define.rs` (type inference at DDL time), `src/query/table_function.rs` (new `type_id_to_display_name` function), and `test/integration/test_type_inference.py` (14 integration tests confirming behavior). Cross-referenced with `src/ddl/show_dims.rs`, `src/ddl/show_metrics.rs`, `src/ddl/show_columns.rs`, `src/ddl/show_dims_for_metric.rs`, `src/ddl/describe.rs`, and `src/model.rs` (Dimension/Metric `output_type` field).

---

## Change Summary

Prior to commit `7c58f54`, the `output_type` field on `Dimension` and `Metric` model structs was only populated for facts (via `typeof()` at DDL time). Dimensions and metrics always had `output_type: None`, causing all SHOW/DESCRIBE commands to display an empty string in the `data_type` / `DATA_TYPE` column.

The new behavior: at DDL time (during `CREATE SEMANTIC VIEW`), the extension runs a `LIMIT 0` query via the persist connection (file-backed databases only) to infer column types. The inferred type IDs are mapped back to dimension and metric `output_type` fields using the new `type_id_to_display_name()` function. This populates `data_type` / `DATA_TYPE` in SHOW and DESCRIBE output with human-readable type names (e.g., `VARCHAR`, `BIGINT`, `DOUBLE`, `DATE`).

Key behavioral details:
- **File-backed databases:** Type inference runs at DDL time. Dimensions and metrics show inferred types.
- **In-memory databases:** No persist connection available, so `output_type` stays `None` and `data_type` / `DATA_TYPE` remains empty.
- **DECIMAL-typed expressions:** `type_id_to_display_name()` returns `None` for DECIMAL (lossy CAST avoidance), so `data_type` stays empty for `SUM(decimal_col)`.
- **Derived metrics:** Also get inferred types when the LIMIT 0 query resolves them.
- **Supported type mappings:** VARCHAR, BOOLEAN, TINYINT, SMALLINT, INTEGER, BIGINT, FLOAT, DOUBLE, DATE, TIME, TIMESTAMP, INTERVAL, UUID, BLOB, BIT, and timestamp/time variants. HUGEINT maps to BIGINT, UHUGEINT maps to UBIGINT, ENUM maps to VARCHAR, STRUCT/MAP map to VARCHAR.

---

## Content Refresh

Existing pages compared against current source code. Pages with real discrepancies list specific changes needed. Pages that are still accurate note "no changes needed."

| # | Type | Title | Key Sections | File Path |
|---|------|-------|--------------|-----------|
| 1 | (refresh) | SHOW SEMANTIC DIMENSIONS | **Stale -- description and examples.** (a) Line 109: `data_type` column description says "Reserved for future use. Currently always an empty string for dimensions." This is now incorrect -- type inference populates `data_type` at DDL time (file-backed DBs). Must update description to match the pattern already used by SHOW SEMANTIC FACTS: "The inferred data type. Empty string if not resolved." (b) Example output at lines 133-139 shows empty `data_type` for all dimensions (`customer_name`, `order_date`, `region`). With type inference, these should show inferred types (e.g., `VARCHAR`, `DATE`, `VARCHAR`). Must update example output. | docs/reference/show-semantic-dimensions.rst |
| 2 | (refresh) | SHOW SEMANTIC METRICS | **Stale -- description and examples.** (a) Line 109: `data_type` column description says "Reserved for future use. Currently always an empty string for metrics." Must update to: "The inferred data type. Empty string if not resolved." (b) Example output at lines 133-138 shows empty `data_type` for `order_count` and `total_amount`. With inference, `order_count` (COUNT(*)) should show `BIGINT`, `total_amount` (SUM of amount, likely non-DECIMAL) should show an inferred type. Must update example output. (c) Derived metric example output at lines 174-181 shows empty `data_type` for all metrics (`cost`, `margin`, `profit`, `revenue`). Derived metrics also get inferred types when resolvable. Must update example output. | docs/reference/show-semantic-metrics.rst |
| 3 | (refresh) | DESCRIBE SEMANTIC VIEW | **Stale -- examples only (descriptions already correct).** The property description tables for DIMENSION DATA_TYPE (line 158) and METRIC DATA_TYPE (line 177) already say "The inferred data type. Empty string if not resolved." -- these are accurate. However: (a) Simple single-table example output at lines 266 and 269 shows empty DATA_TYPE for dimension `region` and metric `total`. With inference on a file-backed DB, these should show `VARCHAR` and an inferred metric type respectively. (b) Metadata annotations example output at lines 305 and 310 shows empty DATA_TYPE for dimension `region` and metric `revenue`. Must update both example outputs. | docs/reference/describe-semantic-view.rst |
| 4 | (refresh) | SHOW COLUMNS IN SEMANTIC VIEW | **Stale -- examples only (description already correct).** The `data_type` column description at line 60 already says "The inferred data type. Empty string if not resolved." -- accurate. However: (a) Example output at lines 132-135 shows empty `data_type` for all four column kinds (`avg_order` DERIVED_METRIC, `region` DIMENSION, `raw_amount` FACT, `revenue` METRIC). With inference, dimension, metric, and derived metric types would be populated (FACT types were already inferred pre-commit). Must update example output. | docs/reference/show-columns-semantic-view.rst |
| 5 | (refresh) | SHOW SEMANTIC DIMENSIONS FOR METRIC | **Stale -- examples only (description already correct).** The `data_type` column description at line 93 already says "The inferred data type of the dimension. Empty string if not resolved." -- accurate. However: (a) All example outputs show empty `data_type` for every dimension across 8 examples (lines 139-144, 182-188, 197-203, 231-237, 255-259, 269-273, 285-289, 316-319). With inference, dimensions like `product` (VARCHAR), `region` (VARCHAR), `customer_name` (VARCHAR), `order_date` (DATE), `item_qty` (INTEGER) would show their inferred types. Must update all example outputs. | docs/reference/show-semantic-dimensions-for-metric.rst |
| 6 | (refresh) -- no changes needed | semantic_view() | Line 135 states "Column types are inferred at define time from the underlying table columns. If type inference is not available, columns default to VARCHAR." This is accurate and already covers the new behavior. The `semantic_view()` function's output column type inference was implemented before this commit (using `column_type_names` + `column_types_inferred`). The new commit extends inference to also populate `output_type` on dimension/metric model structs (for SHOW/DESCRIBE output), but the query output behavior described on this page is unchanged. No updates needed. | docs/reference/semantic-view-function.rst |

---

## API Reference Status

**Manual reference.** `api_reference: "manual"` is set in config.yaml. This project exposes a SQL interface, so reference pages are hand-authored SQL syntax reference pages. Inline code mentions in prose use plain `code` formatting without cross-reference links.

## Audience Targeting

All pages target the single configured persona: **Data engineers exploring semantic views** (intermediate skill level). SQL fluency and DuckDB basics are assumed. Semantic view concepts, DDL syntax, and modeling patterns are always explained.

## Coverage Gaps

1. **In-memory vs file-backed distinction**: The type inference behavior differs between file-backed and in-memory databases. The refreshed description text ("Empty string if not resolved") covers this implicitly, but none of the existing pages explicitly explain why `data_type` might be empty. Consider adding a brief note or tip admonition on one of the SHOW pages explaining that type inference requires a file-backed database (i.e., not `:memory:`) and that the types are inferred at `CREATE SEMANTIC VIEW` time. This is a documentation enhancement, not a staleness fix.
2. **DECIMAL avoidance**: `SUM(decimal_column)` intentionally produces an empty `data_type` to avoid lossy CAST. This behavioral nuance is not documented anywhere. Consider mentioning in a tip or note that certain parameterized types (DECIMAL, LIST, ARRAY) may show empty `data_type` even with inference enabled.
3. **versionadded annotation**: The type inference for dimensions and metrics is new behavior. Consider adding `.. versionadded::` annotations on the refreshed pages to indicate when this behavior was introduced.

## Summary of Work

- **0 new pages** to write
- **5 pages** requiring content refresh (entries #1-5, real discrepancies with current source)
- **1 page** confirmed as still accurate (entry #6, no changes needed)
