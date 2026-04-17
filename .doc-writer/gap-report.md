# Gap Report

**Generated:** 2026-04-13
**Source root:** src/
**Language:** rust
**Total undocumented symbols:** 0
**Potentially stale pages:** 7

## Undocumented Symbols

All v0.6.0 features (phases 43-48) now have dedicated doc pages:

- Metadata annotations -> `docs/how-to/metadata-annotations.rst` (new)
- SHOW COLUMNS -> `docs/reference/show-columns-semantic-view.rst` (new)
- GET_DDL -> `docs/reference/get-ddl.rst` (new)
- Wildcard selection -> `docs/how-to/wildcard-selection.rst` (new)
- Queryable facts -> `docs/how-to/query-facts.rst` (new)
- Semi-additive metrics -> `docs/how-to/semi-additive-metrics.rst` (new)
- Window function metrics -> `docs/how-to/window-metrics.rst` (new)

Reference pages (create-semantic-view, alter-semantic-view, semantic-view-function, show-semantic-views, show-semantic-dimensions, show-semantic-metrics, show-semantic-facts, show-semantic-dimensions-for-metric, describe-semantic-view, error-messages, snowflake-comparison) have been updated in working tree.

No exported user-facing features remain undocumented at the page level.

## Potentially Stale Pages

7 doc pages have known content gaps where the page text does not match current source code behavior:

- `docs/how-to/window-metrics.rst` (new, untracked) — only documents `PARTITION BY EXCLUDING`; plain `PARTITION BY <dim>` is fully implemented (body_parser.rs:1571, expand/window.rs:247) but not mentioned anywhere in the page. The "PARTITION BY EXCLUDING" section heading and all examples exclusively use EXCLUDING. Semantic difference: EXCLUDING is dynamic (all queried dims minus excluded), PARTITION BY is explicit (exact dims listed).
- `docs/reference/create-semantic-view.rst` (modified) — syntax diagram line 59 shows only `PARTITION BY EXCLUDING`; missing `PARTITION BY <dim_name> [, ...]` variant. The description at line 293 only explains EXCLUDING semantics.
- `docs/reference/describe-semantic-view.rst` (modified) — verify window_spec display includes PARTITION BY (non-excluding) format in describe output
- `docs/reference/show-semantic-dimensions-for-metric.rst` (modified) — verify "required" column description covers PARTITION BY dims (not just EXCLUDING and ORDER BY)
- `docs/reference/error-messages.rst` (modified) — verify window metric required dimension error covers PARTITION BY reason string (code uses "PARTITION BY" as reason at expand/window.rs:75)
- `docs/explanation/snowflake-comparison.rst` (modified) — verify window metrics comparison mentions both partitioning modes
- `docs/how-to/facts.rst` (modified, 2026-03-27) — source last modified 2026-04-12; coarse staleness signal, verify content matches current fact query behavior

## Note

The scanner output includes internal Rust structs (VTab bindings, graph validators, etc.).
These are implementation details. Since `api_reference: "manual"` is set, they are excluded
from gap tracking. The substantive gap is content accuracy within existing pages, not missing pages.
