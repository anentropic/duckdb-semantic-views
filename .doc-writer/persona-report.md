# Persona Report

**Generated:** 2026-03-21
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 4 PASS, 1 PARTIAL, 0 FAIL

## Summary

The documentation provides an excellent experience for a data engineer evaluating DuckDB Semantic Views. The Diataxis-organized structure (Tutorials, How-To Guides, Explanation, Reference) maps well to the persona's journey from discovery to production modeling. Tutorials are well-paced for intermediate users, how-to guides are goal-oriented with realistic examples, the Snowflake comparison directly addresses the discovery story, and the reference pages are thorough. The one area of friction is the Iceberg/data-sources guide, which covers the topic but lacks the depth needed for a data engineer building a production DuckDB + Iceberg stack.

---

## Scenario S1: I want to install the extension and create my first semantic view over a single table, then query it

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Clear homepage with grid cards linking to all major sections. The "Getting Started" card is the first and most prominent, with description: "Install the extension, create your first semantic view, and run your first query in 5 minutes."
   - Followed: "Getting Started" card link to `tutorials/getting-started`
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: Complete tutorial with clear prerequisites (DuckDB installed, basic SQL knowledge -- appropriate for persona), time estimate (5 minutes), and installation instructions for both CLI and Python via tab set.
   - Found: A note about pre-release status with link to source repo for build-from-source -- addresses the reality that the extension may not be on the registry yet.
   - Found: Sample data creation with realistic `orders` table and INSERT values -- not placeholder data.
   - Found: Full `CREATE SEMANTIC VIEW` with clear explanation of the `alias.name AS expression` pattern for both dimensions and metrics.
   - Found: Verification step (`SHOW SEMANTIC VIEWS`) with expected output.
   - Found: All three query modes demonstrated with expected output tables: dimensions + metrics, dimensions only, metrics only, plus filtering with WHERE.
   - Found: `explain_semantic_view()` section showing how to inspect generated SQL.
   - Found: Clean up section (`DROP SEMANTIC VIEW`) and summary of what was learned.
   - Found: Clear next step link to multi-table tutorial via `:ref:` cross-reference.
   - Type-alignment: Tutorial (learning by doing) -- matches exactly what a first-time user needs. The guided lesson format with sample data, step-by-step instructions, and expected output is well-calibrated.
   - Language calibration: Appropriate for intermediate users. Does not over-explain SQL basics, but fully explains semantic view concepts (what dimensions and metrics are, the naming pattern, what each query mode does). Matches the persona's "never assume" list.

---

## Scenario S2: I want to model a star schema with multiple tables (fact + dimensions), define relationships, and query across them

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Multi-Table Semantic Views" card: "Learn to model relationships between tables and query across them."
   - Followed: Card link to `tutorials/multi-table`
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: Clear prerequisite linking back to getting-started tutorial via `:ref:`. Assumes familiarity with star schema concepts -- appropriate for the persona's assumed knowledge of data engineering concepts.
   - Found: Realistic e-commerce schema (orders, customers, products) with sample data including dates for time-dimension demonstration.
   - Found: Full DDL with TABLES, RELATIONSHIPS, DIMENSIONS, METRICS. The RELATIONSHIPS clause is explained clearly: "the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)."
   - Found: Helpful tip that PRIMARY KEY is metadata for the semantic view, not a DuckDB constraint -- important clarification.
   - Found: Query demonstrating automatic join pruning (only customers joined, products not needed) with suggestion to verify via `explain_semantic_view()`.
   - Found: Query across both dimension tables.
   - Found: Computed dimension example (`date_trunc('month', o.ordered_at)`) demonstrating that dimensions are not limited to column references.
   - Found: `DESCRIBE SEMANTIC VIEW` usage.
   - Found: `CREATE OR REPLACE` for updating views.
   - Found: Clean up and summary with cross-references to how-to guides (FACTS, derived metrics, role-playing dimensions).
   - Type-alignment: Tutorial -- correct. The user is learning multi-table modeling by building a complete example.
   - Language calibration: Uses data engineering terminology naturally (fact table, dimension tables, star schema, foreign keys) as expected for the persona, while fully explaining the semantic-view-specific concepts (TABLES clause, RELATIONSHIPS clause, join pruning behavior).

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Concepts" card: "Understand how semantic views differ from regular views and how they compare to Snowflake."
   - Followed: Card link to `explanation/index`
2. Navigated to: `docs/explanation/index.rst`
   - Found: Toctree listing two pages: "semantic-views-vs-regular-views" and "snowflake-comparison".
   - Followed: `snowflake-comparison` (the directly relevant page for this scenario)
3. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Clear opening note distinguishing Snowflake's two interfaces (SQL DDL vs YAML spec) and stating all comparisons target the SQL DDL only -- exactly what the persona needs, following the guidance in context.md.
   - Found: Concept mapping table covering all major concepts (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS, FACTS, derived metrics, query interface, inspection, listing) with side-by-side columns for Snowflake and DuckDB.
   - Found: Syntax alignment section with side-by-side code examples in a tab set (Snowflake vs DuckDB) showing the same semantic view definition. The key difference (PRIMARY KEY required in DuckDB) is visually apparent.
   - Found: Key Differences section with warning admonitions for: PRIMARY KEY declarations (Snowflake resolves from catalog, DuckDB requires explicit), query interface (table function vs direct SQL), cardinality inference, and USING RELATIONSHIPS.
   - Found: "Features Not Yet Supported" table listing: semi-additive metrics (deferred), window function metrics (not planned), direct SQL query interface (not planned), column-level security (out of scope), ASOF/temporal relationships (not planned). Each has a clear status and rationale.
   - Found: Final section on Snowflake's YAML spec, explaining why YAML-spec-only concepts (time_dimensions, custom_instructions, access_modifier, sample_values) are not applicable.
   - Type-alignment: Explanation -- correct. The user wants to understand differences and make a decision, not follow steps. The page provides conceptual understanding and comparison context.
   - Language calibration: Assumes familiarity with Snowflake (appropriate), explains all DuckDB-specific behaviors and differences (as required by "never assume" list for Snowflake/Databricks behavioral differences).

---

## Scenario S4: I want to connect DuckDB to my Iceberg tables and define a semantic view over them

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-To Guides" card: "Task-oriented guides for FACTS, derived metrics, role-playing dimensions, fan traps, and data sources."
   - Note: Iceberg is not mentioned by name on the homepage. The "data sources" phrase is the closest match, but a data engineer searching specifically for Iceberg might not immediately identify this as the right link.
   - Followed: Card link to `how-to/index`
2. Navigated to: `docs/how-to/index.rst`
   - Found: Toctree listing five pages: facts, derived-metrics, role-playing-dimensions, fan-traps, data-sources.
   - Followed: `data-sources` (the relevant page for Iceberg)
3. Navigated to: `docs/how-to/data-sources.rst`
   - Found: Opening states semantic views work over "any table that DuckDB can see" including Iceberg -- addresses the core question.
   - Found: Iceberg section with `INSTALL iceberg; LOAD iceberg;` and `iceberg_scan()` usage with S3 path, followed by a single-table semantic view definition.
   - Found: Tip about DuckDB + Iceberg + analytics application stack: "semantic views provide a stable query interface over Iceberg tables."
   - Found: Mixed sources section showing Iceberg + Postgres + Parquet all feeding into one multi-table semantic view -- directly relevant to the "build a DuckDB + Iceberg + analytics app stack" user task.
   - Found: Catalog-qualified table names section, useful for more complex setups.
   - Friction: The Iceberg section is quite brief. For a data engineer building a production DuckDB + Iceberg stack, several practical questions are left unanswered:
     - No mention of S3 credentials configuration (AWS_ACCESS_KEY_ID, etc.) or authentication setup.
     - No mention of Iceberg catalog types (REST catalog, Glue, Hive metastore) and how to specify them.
     - No mention of schema evolution considerations -- what happens if the Iceberg table schema changes after the semantic view is defined?
     - No mention of DuckLake (which appears to be part of the project's test suite) or other Iceberg-related DuckDB features.
     - The example uses `CREATE TABLE ... AS SELECT * FROM iceberg_scan(...)` which copies data into DuckDB memory. A production setup would more likely use a view or direct reference to avoid data duplication. This is mentioned in the Parquet section ("Alternatively, create a view over the Parquet file") but not in the Iceberg section.
   - Type-alignment: How-to guide -- correct type for "I want to connect to Iceberg." However, the depth is insufficient for the stated user task of "Build a DuckDB + Iceberg + analytics app stack." The page covers the happy path but omits the practical details a production setup requires.

### Gap Analysis

**Where:** `docs/how-to/data-sources.rst` > Iceberg Tables section
**What:** The Iceberg section covers the minimum path (install, scan, define view) but omits practical production details: S3 credential configuration, catalog type specification, schema evolution handling, and the option to use a view instead of copying data into memory. The "alternatively, create a view" pattern shown in the Parquet section is not repeated for Iceberg.
**Impact:** A data engineer following only this guide would get a working proof-of-concept but would hit unanswered questions when moving to a production Iceberg setup. They would need to leave the docs and consult DuckDB's Iceberg extension documentation separately.
**Suggested Fix:** In `docs/how-to/data-sources.rst`, section "Iceberg Tables": (1) Add a note or link to DuckDB's Iceberg extension documentation for credential and catalog configuration. (2) Show the `CREATE VIEW` alternative for Iceberg as is done for Parquet, so users understand they do not have to copy all data into DuckDB memory. (3) Optionally add a brief mention of schema evolution considerations -- if the Iceberg table schema changes, the semantic view definition may need updating via `CREATE OR REPLACE`.

---

## Scenario S5: I want to understand how to avoid fan traps and use FACTS and derived metrics to build a robust semantic model

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-To Guides" card mentioning FACTS, derived metrics, and fan traps.
   - Followed: Card link to `how-to/index`
2. Navigated to: `docs/how-to/index.rst`
   - Found: Three relevant pages: facts, derived-metrics, fan-traps.
   - Followed: `fan-traps` first (understanding the problem before learning the tools)
3. Navigated to: `docs/how-to/fan-traps.rst`
   - Found: Clear explanation of what a fan trap is using a concrete orders/line_items example. The explanation progresses from concept (one-to-many duplication) to mechanism (how the extension infers cardinality from PK/UNIQUE) to concrete example with full DDL, sample data, safe query, and blocked query with exact error message.
   - Found: Three approaches to fix fan traps: remove problematic dimension, use metric from same table, restructure the view.
   - Found: One-to-one relationship section explaining the safe traversal case.
   - Type-alignment: This page is a hybrid -- it explains concepts (what is a fan trap, how detection works) and provides how-to steps (how to fix them). For this scenario, the blend works well because the user needs both understanding and actionable fixes.
   - Followed: Back to `how-to/index.rst`, then to `facts`
4. Navigated to: `docs/how-to/facts.rst`
   - Found: Clear explanation that facts are row-level (not aggregate) expressions scoped to a table alias. Basic fact definition with `net_price` example.
   - Found: Fact chaining section showing `tax_amount` referencing `net_price` with step-by-step resolution chain.
   - Found: Multi-table facts example showing facts scoped to one table used with dimensions from other tables.
   - Found: Verification via `explain_semantic_view()`.
   - Found: Troubleshooting section covering circular references, aggregate functions in facts, and name-not-found errors.
   - Cross-references: Prerequisites link to multi-table tutorial via `:ref:`.
   - Followed: Back to `how-to/index.rst`, then to `derived-metrics`
5. Navigated to: `docs/how-to/derived-metrics.rst`
   - Found: Clear distinction between base metrics (table alias prefix, aggregate functions) and derived metrics (no prefix, reference metric names).
   - Found: Complete example with revenue, cost, profit, margin showing the composition pattern.
   - Found: Query example with expected output.
   - Found: Stacking section showing how derived metrics can reference other derived metrics with full expansion chain.
   - Found: Combined facts + derived metrics example showing the full resolution pipeline: fact -> base metric -> derived metric.
   - Found: Troubleshooting for aggregate-in-derived, circular references, and unknown references.
   - Cross-references: Prerequisites link to multi-table tutorial and facts how-to via `:ref:`.
   - Type-alignment: How-to guides -- correct. The user has a specific task (build robust models with these features) and the pages provide step-by-step guidance.
   - Language calibration: Uses data engineering terms naturally (row-level expression, aggregation, dependency chain) while fully explaining semantic-view-specific concepts (FACTS clause, derived metrics, fact chaining, topological resolution).

---

## Revision Recommendations

### FAIL Issues (trigger revision)

No FAIL issues identified.

### PARTIAL Issues (for project author approval)

| Scenario | Page | Gap | Suggested Fix |
|----------|------|-----|---------------|
| S4 | `docs/how-to/data-sources.rst` > Iceberg Tables | Iceberg section is minimal -- missing S3 credential guidance, catalog configuration, CREATE VIEW alternative (shown for Parquet but not Iceberg), and schema evolution considerations | In `docs/how-to/data-sources.rst`, section "Iceberg Tables": (1) Add a cross-reference or link to DuckDB Iceberg extension docs for credential and catalog setup. (2) Show the `CREATE VIEW` alternative as done in the Parquet section to avoid copying data into memory. (3) Add a brief note on schema evolution: if the underlying table schema changes, update the semantic view with `CREATE OR REPLACE`. |
