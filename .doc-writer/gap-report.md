# Gap Report

**Generated:** 2026-04-26
**Source root:** src/
**Language:** rust
**Total undocumented symbols:** 0
**Potentially stale pages:** 6

## Undocumented Symbols

No undocumented user-facing features. All SQL DDL statements, SHOW/DESCRIBE commands, query functions, and features have dedicated reference pages. The Rust `pub` items identified by the export scanner are internal implementation details (vtable structs, bind data types, parser functions) and are not part of the user-facing SQL interface.

Previous gap report (2026-04-23) also found 0 undocumented symbols — no new gaps.

## Potentially Stale Pages

6 doc pages have content that conflicts with the current source code on branch `feat/dim-metric-type-inference`. The branch adds DDL-time type inference for dimensions and metrics (previously only facts had type inference). Specific discrepancies are noted for each page:

- `docs/reference/show-semantic-dimensions.rst` (doc: 2026-04-17, source: 2026-04-26) — Line 109 says "Reserved for future use. Currently always an empty string for dimensions." This is now incorrect: type inference populates `data_type` for dimensions at DDL time. Examples show empty `data_type` columns.
- `docs/reference/show-semantic-metrics.rst` (doc: 2026-04-17, source: 2026-04-26) — Line 109 says "Reserved for future use. Currently always an empty string for metrics." This is now incorrect: type inference populates `data_type` for metrics at DDL time. Examples show empty `data_type` columns.
- `docs/reference/describe-semantic-view.rst` (doc: 2026-04-24, source: 2026-04-26) — Examples at lines 266, 269, 305, 310 show empty DATA_TYPE for dimensions and metrics. With DDL-time inference, these would show inferred types (e.g., VARCHAR, BIGINT, DOUBLE).
- `docs/reference/show-columns-semantic-view.rst` (doc: 2026-04-17, source: 2026-04-26) — Examples at lines 132-135 show empty `data_type` for all column kinds. With inference, dimensions and metrics would show inferred types.
- `docs/reference/show-semantic-dimensions-for-metric.rst` (doc: 2026-04-17, source: 2026-04-26) — Examples show empty `data_type` for dimensions. With inference, types would be populated.
- `docs/reference/semantic-view-function.rst` (doc: 2026-04-17, source: 2026-04-26) — Line 135 mentions type inference but may need update to clarify that dimension/metric types are now also inferred at define time (not just column output types).

## Notes

- The timestamp heuristic flags all 40 pages as stale (source modified today on feature branch). The 6 pages listed above are the ones with identified content discrepancies related to the dim/metric type inference feature.
- 34 remaining pages are timestamp-stale but their content appears current — the source change does not affect their described behavior.
