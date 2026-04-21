# Page Inventory -- v0.7.0 Features and Content Refresh

**Generated:** 2026-04-21
**Based on:** gap-report.md (4 undocumented symbol groups from v0.7.0 phases 54-57), 29 flagged stale pages
**Branch:** gsd/v0.7.0-yaml-definitions-materialization-routing
**Scan method:** Each undocumented feature traced through source (model.rs, body_parser.rs, expand/materialization.rs, expand/sql_gen.rs, ddl/show_materializations.rs, ddl/read_yaml.rs, render_yaml.rs, render_ddl.rs, ddl/describe.rs, query/explain.rs, parse.rs, lib.rs). Each flagged doc page read in full and compared against current source.

---

## New Pages

New documentation pages for v0.7.0 features with no existing coverage.

| # | Type | Title | Key Sections | File Path |
|---|------|-------|--------------|-----------|
| 1 | how-to | How to Use Materializations | Prerequisites, declare a materialization in DDL, how routing works (exact match), multiple materializations (first match wins), routing exclusions (semi-additive and window metrics), verify routing with explain_semantic_view(), inspect with SHOW/DESCRIBE, troubleshooting | docs/how-to/materializations.rst |
| 2 | how-to | How to Export and Import YAML Definitions | Prerequisites, export with READ_YAML_FROM_SEMANTIC_VIEW, import with FROM YAML (inline dollar-quoted), import from file with FROM YAML FILE, round-trip workflow, stripped fields, troubleshooting | docs/how-to/yaml-definitions.rst |
| 3 | reference | SHOW SEMANTIC MATERIALIZATIONS | Syntax (single-view IN form and cross-view all form), parameters, output columns (7 columns: database_name, schema_name, semantic_view_name, name, table, dimensions, metrics), sorting behavior, examples, error cases | docs/reference/show-semantic-materializations.rst |
| 4 | reference | READ_YAML_FROM_SEMANTIC_VIEW | Syntax, parameters (view name, supports schema/catalog-qualified), output (VARCHAR YAML string), field stripping behavior (column_type_names, column_types_inferred, created_on, database_name, schema_name), examples, error cases | docs/reference/read-yaml-from-semantic-view.rst |

**Notes on new pages:**

- Page 1 (materializations how-to) covers the MATERIALIZATIONS clause in CREATE SEMANTIC VIEW DDL, automatic query routing to pre-aggregated tables, and the interaction with explain_semantic_view(). It links to the CREATE reference page for full clause syntax. The how-to explains the user workflow: create a pre-aggregated table, declare a materialization mapping dimensions/metrics to it, then queries matching that exact coverage route to the pre-aggregated table instead of expanding raw sources.
- Page 2 (YAML definitions how-to) covers three related features that form a natural workflow: `READ_YAML_FROM_SEMANTIC_VIEW()` (export to YAML), `CREATE SEMANTIC VIEW ... FROM YAML $$ ... $$` (inline import), and `CREATE SEMANTIC VIEW ... FROM YAML FILE '/path'` (file import). These are v0.7.0 features from phases 52-53 (YAML DDL) and phase 56 (YAML export). The gap report lists `READ_YAML_FROM_SEMANTIC_VIEW` as undocumented; `FROM YAML` and `FROM YAML FILE` are also undocumented but are part of the same feature family.
- Pages 3-4 are reference pages for the new DDL/function surface. SHOW SEMANTIC MATERIALIZATIONS has two forms: per-view (`IN view_name`) and cross-view (no parameters).

---

## Content Refresh

Existing pages compared against current source code. Pages with real discrepancies list specific changes needed. Pages that are still accurate note "no changes needed."

| # | Type | Title | Key Sections | File Path |
|---|------|-------|--------------|-----------|
| 5 | (refresh) | CREATE SEMANTIC VIEW | **Stale -- multiple gaps.** (a) Syntax grammar missing `MATERIALIZATIONS` clause entirely -- must add `[ MATERIALIZATIONS ( <name> AS ( TABLE <table_name>, [ DIMENSIONS (<dim>, ...) ], [ METRICS (<metric>, ...) ] ) [, ...] ) ]` after METRICS. (b) Clauses section says "Clauses must appear in the following order: TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS" -- must add MATERIALIZATIONS after METRICS. (c) No MATERIALIZATIONS sub-section documenting clause parameters, syntax, and validation rules. (d) Examples section has no materialization example. (e) Syntax grammar missing `FROM YAML $$ ... $$` and `FROM YAML FILE '/path'` DDL variants -- these are alternative body syntaxes alongside the AS keyword body. | docs/reference/create-semantic-view.rst |
| 6 | (refresh) | DESCRIBE SEMANTIC VIEW | **Stale -- missing object kind.** (a) object_kind list in "Object Kinds and Properties" does not include `MATERIALIZATION`. (b) Source at describe.rs:457-484 emits MATERIALIZATION rows with 3 properties: TABLE (physical table), DIMENSIONS (JSON array), METRICS (JSON array). Must add MATERIALIZATION object kind section after DERIVED_METRIC. (c) Row ordering description at line 68 does not mention MATERIALIZATION (appears after DERIVED_METRIC in definition order). | docs/reference/describe-semantic-view.rst |
| 7 | (refresh) | explain_semantic_view() | **Stale -- missing output line.** (a) Output section does not mention the new `-- Materialization: {name}` / `-- Materialization: none` header line. Source at query/explain.rs:227-229 adds this line after the Facts line (if present) or after the Metrics line. (b) Sample output example at line 96 missing the materialization line. Must update both the output description and the sample. | docs/reference/explain-semantic-view-function.rst |
| 8 | (refresh) | GET_DDL | **Stale -- incomplete clause list.** (a) Output description at line 48 says DDL includes "all clauses (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS)" -- must add MATERIALIZATIONS. Source at render_ddl.rs:327-328 emits MATERIALIZATIONS when non-empty. (b) Sample output example does not show a view with materializations -- consider adding one or noting that MATERIALIZATIONS appears after METRICS when declared. | docs/reference/get-ddl.rst |
| 9 | (refresh) | Error Messages | **Stale -- missing error categories.** (a) DDL Errors section missing materialization errors: duplicate materialization name (`Duplicate materialization name '<name>'`), materialization references unknown dimension (`Materialization '<name>' references unknown dimension '<dim>'`), materialization references unknown metric (`Materialization '<name>' references unknown metric '<met>'`), materialization must specify at least one of DIMENSIONS or METRICS. Source at body_parser.rs:561-615. (b) "Unknown clause keyword" section does not mention MATERIALIZATIONS in the known keyword list. (c) "Clause out of order" error text omits MATERIALIZATIONS from the ordering. (d) Missing YAML-related errors: invalid YAML content, unexpected trailing content after dollar-quote, empty file path, size cap exceeded. Source at parse.rs:1069-1326. | docs/reference/error-messages.rst |
| 10 | (refresh) | Semantic Views vs. Regular SQL Views | **Stale -- factually incorrect section.** "Semantic Views Are Not Materialized" section at line 143 states: "There is no caching, no pre-aggregation, and no materialized result set." Since v0.7.0, the MATERIALIZATIONS clause enables routing queries to pre-aggregated tables. Must update to explain that the default behavior is still on-demand SQL generation, but materializations optionally route queries to pre-aggregated tables when declared. The section title itself may need rewording (e.g., "Materialization Support"). | docs/explanation/semantic-views-vs-regular-views.rst |
| 11 | (refresh) | Snowflake Comparison | **Stale -- missing feature rows.** (a) Concept mapping table missing row for MATERIALIZATIONS / pre-aggregation routing. (b) "Features Not Yet Supported" section may need updating if Snowflake has comparable materialization support (verify against Snowflake docs). (c) YAML section note at line 378 says "DuckDB Semantic Views targets the SQL DDL interface only" but the extension now supports `FROM YAML` import and `READ_YAML_FROM_SEMANTIC_VIEW` export. Must add a note or row about YAML definition round-trip support (while clarifying this is distinct from Snowflake's Cortex Analyst YAML spec). | docs/explanation/snowflake-comparison.rst |
| 12 | (refresh) | Reference Index | **Stale -- missing entries.** DDL statements list does not include SHOW SEMANTIC MATERIALIZATIONS or READ_YAML_FROM_SEMANTIC_VIEW. Toctree missing new reference pages (show-semantic-materializations, read-yaml-from-semantic-view). Must add entries in the appropriate groups and update the toctree. | docs/reference/index.rst |
| 13 | (refresh) | How-To Index | **Stale -- missing entries.** Guide list does not include materializations or YAML definitions how-to pages. Toctree missing new how-to pages (materializations, yaml-definitions). Must add entries and update the toctree. | docs/how-to/index.rst |
| 14 | (refresh) | Home Page (index) | **Stale -- feature coverage.** Grid cards and description do not mention materializations or YAML import/export as features. Consider adding or updating existing cards. The how-to guides card description at line 42 lists "FACTS, derived metrics, role-playing dimensions, fan traps, data sources" but not materializations or YAML. | docs/index.rst |
| 15 | (refresh) -- no changes needed | How to Use Different Data Sources | Content accurately describes Parquet, CSV, Iceberg, Postgres, and mixed source connectivity. No v0.7.0 source changes affect data source behavior. | docs/how-to/data-sources.rst |
| 16 | (refresh) -- no changes needed | How to Compose Metrics with Derived Metrics | Derived metrics syntax, behavior, and error handling unchanged in v0.7.0. | docs/how-to/derived-metrics.rst |
| 17 | (refresh) -- no changes needed | How to Understand and Avoid Fan Traps | Fan trap detection logic unchanged. | docs/how-to/fan-traps.rst |
| 18 | (refresh) -- no changes needed | How to Model Role-Playing Dimensions | Role-playing dimension behavior unchanged. | docs/how-to/role-playing-dimensions.rst |
| 19 | (refresh) -- no changes needed | DROP SEMANTIC VIEW | DROP syntax and behavior unchanged. | docs/reference/drop-semantic-view.rst |
| 20 | (refresh) -- no changes needed | Getting Started | Installation, basic view creation, and query modes unchanged. | docs/tutorials/getting-started.rst |
| 21 | (refresh) -- no changes needed | Multi-Table Semantic Views | Multi-table tutorial content unchanged. | docs/tutorials/multi-table.rst |
| 22 | (refresh) -- no changes needed | semantic_view() | Query function syntax, parameters, modes, and filtering unchanged. Materialization routing is transparent to the user (same query interface, same parameters). | docs/reference/semantic-view-function.rst |
| 23 | (refresh) -- no changes needed | How to Use FACTS | FACTS clause syntax and behavior unchanged. | docs/how-to/facts.rst |
| 24 | (refresh) -- no changes needed | How to Use Metadata Annotations | COMMENT, SYNONYMS, PRIVATE/PUBLIC behavior unchanged. | docs/how-to/metadata-annotations.rst |
| 25 | (refresh) -- no changes needed | How to Query Facts Directly | Facts query mode behavior unchanged. | docs/how-to/query-facts.rst |
| 26 | (refresh) -- no changes needed | How to Use Semi-Additive Metrics | NON ADDITIVE BY behavior unchanged. Materialization exclusion for semi-additive metrics documented in materializations how-to. | docs/how-to/semi-additive-metrics.rst |
| 27 | (refresh) -- no changes needed | How to Use Wildcard Selection | Wildcard expansion behavior unchanged. | docs/how-to/wildcard-selection.rst |
| 28 | (refresh) -- no changes needed | How to Use Window Function Metrics | Window metrics behavior unchanged in v0.7.0 (PARTITION BY / PARTITION BY EXCLUDING both already documented from previous refresh). Materialization exclusion for window metrics documented in materializations how-to. | docs/how-to/window-metrics.rst |
| 29 | (refresh) -- no changes needed | ALTER SEMANTIC VIEW | RENAME TO, SET COMMENT, UNSET COMMENT behavior unchanged. | docs/reference/alter-semantic-view.rst |
| 30 | (refresh) -- no changes needed | SHOW SEMANTIC VIEWS | SHOW SEMANTIC VIEWS syntax and output unchanged. | docs/reference/show-semantic-views.rst |
| 31 | (refresh) -- no changes needed | SHOW COLUMNS IN SEMANTIC VIEW | Output columns and behavior unchanged. | docs/reference/show-columns-semantic-view.rst |
| 32 | (refresh) -- no changes needed | SHOW SEMANTIC DIMENSIONS FOR METRIC | Fan-trap-aware dimension filtering unchanged. | docs/reference/show-semantic-dimensions-for-metric.rst |
| 33 | (refresh) -- no changes needed | SHOW SEMANTIC DIMENSIONS | No discrepancies. | docs/reference/show-semantic-dimensions.rst |
| 34 | (refresh) -- no changes needed | SHOW SEMANTIC FACTS | No discrepancies. | docs/reference/show-semantic-facts.rst |
| 35 | (refresh) -- no changes needed | SHOW SEMANTIC METRICS | No discrepancies. | docs/reference/show-semantic-metrics.rst |

---

## API Reference Status

**Manual reference.** `api_reference: "manual"` is set in config.yaml. This project exposes a SQL interface (not a programmatic API), so reference pages are hand-authored SQL syntax reference pages. Inline code mentions in prose use plain `code` formatting without cross-reference links. Internal Rust types (VTab structs, graph validators, etc.) are excluded from documentation scope.

## Audience Targeting

All pages target the single configured persona: **Data engineers exploring semantic views** (intermediate skill level). SQL fluency and DuckDB basics are assumed. Semantic view concepts, DDL syntax, and modeling patterns are always explained.

## Coverage Gaps

1. **FROM YAML / FROM YAML FILE DDL variants** are covered in new page #2 (YAML definitions how-to) and refresh entry #5 (CREATE reference syntax grammar). These are v0.7.0 features from phases 52-53.
2. **Materialization routing transparency** -- the semantic_view() query interface is unchanged (same parameters, same behavior from the user's perspective). Routing happens automatically when materializations are declared and a query's dims/metrics exactly match a materialization's coverage. This is documented in the materializations how-to (page #1) rather than in the semantic_view() reference, since the query interface itself did not change.
3. **explain_semantic_view() materialization line** is covered by refresh entry #7.
4. **DESCRIBE SEMANTIC VIEW MATERIALIZATION object kind** is covered by refresh entry #6.
5. **GET_DDL MATERIALIZATIONS clause** is covered by refresh entry #8.
6. **New error messages** for materializations and YAML are covered by refresh entry #9.

## Summary of Work

- **4 new pages** to write (2 how-to, 2 reference)
- **10 pages** requiring content refresh (entries #5-14, real discrepancies with current source)
- **21 pages** confirmed as still accurate (entries #15-35, no changes needed)
