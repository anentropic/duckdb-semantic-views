# Page Inventory

**Generated:** 2026-03-22
**Scope:** Undocumented symbols from gap-report.md only (5 symbols across 5 source files)

## Proposed Documentation

All five undocumented symbols are DDL statements that belong in the Reference section. They follow the existing reference page pattern (Syntax, Parameters/Variants, Output Columns, Examples) established by the current pages (CREATE, DROP, DESCRIBE, SHOW SEMANTIC VIEWS, semantic_view(), explain_semantic_view()).

| # | Type | Title | Key Sections | File Path |
|---|------|-------|--------------|-----------|
| 1 | reference | ALTER SEMANTIC VIEW | Syntax, Statement Variants, Parameters, Output Columns, Examples | docs/reference/alter-semantic-view.rst |
| 2 | reference | SHOW SEMANTIC DIMENSIONS | Syntax, Statement Variants, Parameters, Output Columns, Examples | docs/reference/show-semantic-dimensions.rst |
| 3 | reference | SHOW SEMANTIC METRICS | Syntax, Statement Variants, Parameters, Output Columns, Examples | docs/reference/show-semantic-metrics.rst |
| 4 | reference | SHOW SEMANTIC FACTS | Syntax, Statement Variants, Parameters, Output Columns, Examples | docs/reference/show-semantic-facts.rst |
| 5 | reference | SHOW SEMANTIC DIMENSIONS FOR METRIC | Syntax, Parameters, Output Columns, Fan Trap Filtering, Examples | docs/reference/show-semantic-dimensions-for-metric.rst |

## API Reference Status

**Status:** Manual reference pages (as configured in `config.yaml` with `api_reference: manual`). No auto-generated API reference -- this is a Rust+C++ extension with a SQL interface, so all reference pages are hand-written SQL syntax reference following the Snowflake SQL reference page pattern.

All five new pages will be added to `docs/reference/index.rst` toctree alongside the existing entries.

## Audience Targeting

All five pages target the single configured persona: **Data engineers exploring semantic views** (intermediate skill level). These are SQL DDL reference pages -- the audience knows SQL and DuckDB basics but needs the exact syntax, clause options, output schemas, and behavioral details (especially fan trap filtering for page 5) spelled out clearly.

## Coverage Gaps

- **reference/index.rst toctree update:** The five new pages must be added to the existing toctree in `docs/reference/index.rst`. This is a supporting edit, not a standalone page.
- **error-messages.rst additions:** The ALTER RENAME and SHOW ... FOR METRIC statements produce error messages (view does not exist, metric not found, "Did you mean?" suggestions, "Expected RENAME TO", "Missing new name") that are not yet listed in the error messages reference page. These are incremental additions to an existing page, not standalone pages.
- **Pages 2-4 structural similarity:** SHOW SEMANTIC DIMENSIONS, SHOW SEMANTIC METRICS, and SHOW SEMANTIC FACTS share the same two-variant pattern (cross-view without IN clause, single-view with IN clause) and nearly identical output column schemas (FACTS has 4 columns, DIMENSIONS and METRICS have 5). The pages will follow the same template but each documents its own statement.

## Notes

- The existing reference pages use `sqlgrammar` as the code-block language for syntax sections (via a custom Pygments lexer registered in conf.py). New pages will follow this convention.
- Pages use `:ref:` labels with `ref-` prefix for cross-referencing (e.g., `ref-alter-syntax`, `ref-show-dims-syntax`).
- The SHOW SEMANTIC DIMENSIONS FOR METRIC statement (page 5) is the most complex -- it filters dimensions based on fan-trap-aware graph traversal and supports derived metrics that transitively resolve source tables. The explanation of fan trap filtering behavior should reference the existing how-to page on fan traps (`:ref:` link to the how-to/fan-traps.rst page).
