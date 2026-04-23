# Persona Report

**Generated:** 2026-04-22
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 5 PASS, 0 PARTIAL, 0 FAIL

## Summary

The documentation provides an excellent experience for an intermediate data engineer evaluating DuckDB Semantic Views as an open-source alternative to Snowflake or Databricks. The Diataxis structure is well-executed across all four quadrants: tutorials teach through guided hands-on examples, how-to guides solve specific tasks with prerequisites and troubleshooting, reference pages document syntax with parameter tables and worked examples, and explanation pages provide context for platform comparison and decision-making. All five scenarios were achievable from start to finish with clear navigation paths, complete code examples with expected output, and language calibrated to someone who knows SQL and data engineering but is new to semantic views. The recently modified YAML definitions how-to (reordered to Import-first) reads naturally for a task-oriented user, and the new YAML format reference page fills a critical gap by providing field-by-field specifications that were previously only available through examples.

---

## Scenario S1: I want to install the extension and create my first semantic view over a single table, then query it

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Getting started" prominently placed first, with description: "Install the extension, create your first semantic view, and run a query in 5 minutes."
   - Followed: Link to `tutorial-getting-started`

2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: Complete tutorial with time estimate (5 minutes) and prerequisites (DuckDB installed, basic SQL knowledge -- appropriate for persona).
   - **Install the Extension:** Tab set for DuckDB CLI and Python with exact commands (`INSTALL semantic_views FROM community; LOAD semantic_views;`). Both paths clear and complete.
   - **Create Sample Data:** Realistic `orders` table with copy-pasteable SQL including INSERT statements. Not placeholder data.
   - **Define a Semantic View:** Full `CREATE SEMANTIC VIEW` DDL with inline explanation of the `alias.name AS expression` pattern. Each dimension and metric explained individually: "o.region AS o.region creates a dimension called region from the region column of the table aliased as o." Satisfies the never-assume requirement for DDL syntax.
   - **Verify:** `SHOW SEMANTIC VIEWS` with expected output table.
   - **Query the Semantic View:** All three query modes demonstrated (dimensions+metrics, dimensions-only, metrics-only) with complete SQL and expected result tables. WHERE filtering also shown.
   - **Inspect the Generated SQL:** `explain_semantic_view()` demonstrated with cross-reference to its reference page via `:ref:`.
   - **Clean Up:** `DROP SEMANTIC VIEW` shown.
   - **What You Learned:** Summary with cross-references to CREATE SEMANTIC VIEW, SHOW SEMANTIC VIEWS, semantic_view(), explain_semantic_view(), and DROP SEMANTIC VIEW reference pages. Clear "Next" pointer to multi-table tutorial.
   - Type-alignment: Tutorial (learning-oriented, study + action). Exactly what a first-time user needs.
   - Language calibration: Assumes SQL knowledge (appropriate). Explains semantic view concepts thoroughly (satisfies never_assume). The naming pattern is explicitly decoded.

---

## Scenario S2: I want to model a star schema with multiple tables (fact + dimensions), define relationships, and query across them

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Multi-table semantic views" with description "Model relationships between tables and query across them."
   - Followed: Link to `tutorial-multi-table`

2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: Complete tutorial with time estimate (10 minutes), prerequisites (links back to getting-started), and star schema assumption noted (appropriate for persona's assumed knowledge).
   - **Create the Schema:** Three-table e-commerce schema (orders, customers, products) with realistic sample data including dates. Tables correctly identified as fact and dimension tables.
   - **Define the Semantic View:** Full DDL with TABLES (three aliases, PKs), RELATIONSHIPS (two FK references with emphasized lines), DIMENSIONS (from multiple tables including computed `date_trunc`), and METRICS. The RELATIONSHIPS clause explained clearly: "the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)." Satisfies never-assume for relationship modeling.
   - **Query One Dimension Table:** Demonstrates selective join -- only customers joined, products excluded. Expected output shown. `explain_semantic_view()` used to verify. Directly satisfies "see generated SQL to verify join correctness."
   - **Query Across Both Dimension Tables:** Both tables joined. Output shown.
   - **Use a Computed Dimension:** `date_trunc` dimension queried with output.
   - **Describe the View:** `DESCRIBE SEMANTIC VIEW` demonstrated with cross-reference.
   - **Update the View:** `CREATE OR REPLACE` shown. Tip about `ALTER SEMANTIC VIEW ... RENAME TO` with cross-reference.
   - **What You Learned:** Summary with cross-references to how-to guides (facts, derived metrics, role-playing dimensions).
   - Helpful tip: "The PRIMARY KEY declaration is used by the extension to synthesize JOIN ON clauses. It does not create a constraint in DuckDB." -- important clarification for data engineers.
   - Type-alignment: Tutorial (progressive hands-on learning). Correct.

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake and Databricks Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Snowflake comparison" with description "Feature-by-feature comparison with Snowflake's CREATE SEMANTIC VIEW."
   - Followed: Link to `explanation-snowflake`

2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Comprehensive comparison page.
   - **YAML spec disclaimer:** Note clarifying SQL DDL interface comparison only -- correctly separates the two Snowflake interfaces.
   - **Concept Mapping:** 22-row table covering all DDL statements, core model concepts, advanced features (semi-additive, window, materializations, wildcards, metadata, access modifiers), and query interface. Cross-references to relevant how-to and reference pages throughout.
   - **Syntax Alignment:** Side-by-side tab set showing DDL from both platforms. PRIMARY KEY difference visually apparent.
   - **Key Differences:** Seven detailed subsections:
     - Primary Key Declarations: Three-case table (native with PK, native without, external sources). Iceberg-specific tip. Code examples for both cases. Error message shown.
     - Query Interface: Warning admonition. Side-by-side syntax. Notes Snowflake's direct SQL and AGG function not supported.
     - Cardinality Inference: Clear explanation with link to fan traps how-to.
     - USING RELATIONSHIPS: Identical syntax noted.
     - Facts Query Mode: Warning about mutual exclusivity of facts and metrics.
     - Semi-Additive and Window Metrics: Behavioral differences listed.
     - Materializations: Notes this is DuckDB-only, not in Snowflake DDL.
   - **Features Not Yet Supported:** Clear three-row table with status and rationale.
   - **A Note on Snowflake's YAML Spec:** Explains YAML-spec-only concepts (time_dimensions, custom_instructions, etc.) and clarifies DuckDB YAML uses its own schema for version control, not AI prompt tuning. Links to the YAML how-to guide.
   - Followed: Navigation to `explanation-databricks` via explanation index page

3. Navigated to: `docs/explanation/index.rst`
   - Found: Link to Databricks Comparison.
   - Followed: Link to `explanation-databricks`

4. Navigated to: `docs/explanation/databricks-comparison.rst`
   - Found: Parallel structure to Snowflake comparison.
   - **Concept Mapping:** 18-row table with clear terminology mapping (MEASURES vs METRICS, FROM clause vs TABLES+RELATIONSHIPS). Now includes YAML definitions and materializations rows.
   - **Syntax Comparison:** Side-by-side tab set.
   - **Key Differences:** Multi-table handling (explicit JOIN vs declarative RELATIONSHIPS with join synthesis), Query Interface, MEASURES vs METRICS, Dimension Expressions.
   - **Features in DuckDB Not in Databricks:** 11-row table (FACTS, NON ADDITIVE BY, window metrics, MATERIALIZATIONS, RELATIONSHIPS, fan trap detection, role-playing dimensions, YAML import/export, explain_semantic_view, WITH SYNONYMS, PRIVATE/PUBLIC).
   - **Features in Databricks Not in DuckDB:** 5-row table (direct SQL, Unity Catalog, row-level security, AI/BI, Delta Lake materialized views).
   - **Choosing Between Them:** Honest positioning -- lightweight/local-first vs cloud platform. Not interchangeable.
   - Type-alignment: Explanation pages (understanding-oriented, cognition-based). Correct for platform evaluation and decision-making.
   - Language calibration: Assumes Snowflake/Databricks familiarity (appropriate). Explains all DuckDB-specific behaviors and differences (satisfies never-assume).

---

## Scenario S4: I want to define a semantic view with pre-aggregated materializations and export it as YAML for version control

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-to guides" card mentioning "materializations, YAML definitions" in its description.
   - Followed: Link to `how-to-guides`

2. Navigated to: `docs/how-to/index.rst`
   - Found: Listed entries for both materializations ("Declare materializations that route matching queries to pre-aggregated tables") and YAML definitions ("Import and export semantic view definitions as YAML for version control and migration").
   - Followed: Link to `howto-materializations`

3. Navigated to: `docs/how-to/materializations.rst`
   - Found: Complete how-to guide with versionadded 0.7.0 marker and clear prerequisites.
   - **How Materializations Work:** Clear concept -- maps dims+metrics to a pre-aggregated table, transparent routing, query interface unchanged.
   - **Declare a Materialization:** Step-by-step: create pre-aggregated table first, then MATERIALIZATIONS clause with emphasized lines in the DDL. Clause ordering rule stated (must appear after METRICS). Tip about column naming requirement.
   - **How Routing Works:** Exact-match logic explained with two conditions. Case-insensitive. Two examples: one that matches, one that does not. Warning about no superset matching in v0.7.0 -- important caveat clearly stated.
   - **Multiple Materializations:** Definition-order, first-match semantics. Full DDL example.
   - **Routing Exclusions:** Semi-additive and window metrics always excluded. Code example showing the bypass behavior.
   - **Verify Routing:** `explain_semantic_view()` with `-- Materialization:` header line in output. Sample output shown for both matched and unmatched cases.
   - **Inspect with SHOW and DESCRIBE:** Both `SHOW SEMANTIC MATERIALIZATIONS` and `DESCRIBE` with object_kind filtering demonstrated.
   - **Troubleshooting:** Five specific error scenarios with explanations and fixes.
   - **Related:** Cross-references to CREATE reference, SHOW reference, explain reference, and semi-additive/window how-to guides.
   - Followed: Link to `howto-yaml-definitions` from how-to index

4. Navigated to: `docs/how-to/yaml-definitions.rst` (recently reordered to Import-first)
   - Found: Complete how-to guide with versionadded 0.7.0 marker.
   - **Import from Inline YAML** (now first section): Dollar-quoted syntax with full, realistic example showing tables/dimensions/metrics. Tagged dollar-quoting variant shown. CREATE OR REPLACE and IF NOT EXISTS variants noted. This Import-first ordering makes good sense for the how-to pattern: a user arriving here is more likely to have a YAML file they want to import than to start with export.
   - **Import from a YAML File:** `FROM YAML FILE` with single-quoted path. Both CREATE and CREATE OR REPLACE variants shown.
   - **Export with READ_YAML_FROM_SEMANTIC_VIEW:** Function call shown. COPY TO file pattern for saving to disk demonstrated -- important practical detail. Schema-qualified names noted. Cross-reference to reference page.
   - **Round-Trip Workflow:** Three-step numbered workflow (export, import, verify with GET_DDL). Tip about storing YAML in version control alongside the data model.
   - **Troubleshooting:** Seven specific error messages with explanations and fixes. Covers unterminated strings, empty paths, size limits, and view-not-found on export.
   - **Related:** Cross-references to YAML format reference, READ_YAML reference, CREATE reference, and GET_DDL. The link to `ref-yaml-format` is important -- connects the how-to to the specification.
   - Followed: Link to `ref-yaml-format` from Related section

5. Navigated to: `docs/reference/yaml-format.rst` (new page)
   - Found: Complete field-by-field YAML schema reference.
   - **SQL-to-YAML mapping table:** Seven-row table mapping SQL clause names to YAML keys. The `RELATIONSHIPS -> joins` difference called out explicitly with a note ("Different name -- YAML uses the internal joins key") -- this would have been a stumbling block without documentation.
   - **Complete Example:** Comprehensive YAML covering all features: tables with pk_columns/comment/synonyms, joins with from_alias/fk_columns/cardinality, facts with chaining, dimensions with source_table/output_type, metrics including derived (no source_table), semi-additive (non_additive_by), and materializations. This serves as both a specification and a template.
   - **Minimal Example:** Bare minimum YAML (tables + dimensions + metrics).
   - **Top-Level Keys:** Seven keys with types, required/optional, descriptions. Footnote: "At least one of dimensions or metrics must be non-empty."
   - **Table:** Six fields (alias, table, pk_columns, unique_constraints, comment, synonyms) with types, defaults, descriptions. Code example.
   - **Dimension:** Six fields (name, expr, source_table, output_type, comment, synonyms). Code example with computed dimension.
   - **Metric:** Ten fields covering base, derived, private, semi-additive, and window variants. Five separate code examples, one for each variant. This is thorough.
   - **Fact:** Seven fields. Code example with chaining.
   - **Join:** Six fields (table, from_alias, fk_columns, ref_columns, name, cardinality). Code example showing both basic and explicit ref_columns usage.
   - **Materialization:** Four fields. Code example with multiple entries.
   - **NonAdditiveDim:** Three fields (dimension, order, nulls). Code example.
   - **WindowSpec:** Seven fields (window_function, inner_metric, extra_args, excluding_dims, partition_dims, order_by, frame_clause). Code example.
   - **WindowOrderBy:** Three fields (expr, order, nulls).
   - **Size Limit:** 1 MiB documented.
   - **Related:** Cross-references back to CREATE reference, READ_YAML reference, and YAML how-to guide.
   - Type-alignment: Reference documentation (information-oriented, work-context). The user is looking up field specifications, and the page provides structured tables with defaults and examples for every type. Correct Diataxis alignment.
   - Language calibration: Technical but accessible. Field descriptions use data engineering terminology naturally (PK/FK, cardinality, aggregate) while explaining semantic-view-specific structures through the descriptions. No jargon left undefined.

---

## Scenario S5: I want to define semi-additive metrics and window function metrics for snapshot data (like account balances) and time-series analysis

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-to guides" card. No direct homepage card for semi-additive or window metrics, but the how-to card description is broad enough.
   - Followed: Link to `how-to-guides`

2. Navigated to: `docs/how-to/index.rst`
   - Found: Both guides listed clearly with descriptive summaries:
     - "Define metrics with NON ADDITIVE BY for snapshot data like account balances and inventory levels."
     - "Define window function metrics for rolling averages, lag comparisons, and rankings using OVER clauses."
   - Followed: Link to `howto-semi-additive`

3. Navigated to: `docs/how-to/semi-additive-metrics.rst`
   - Found: Complete how-to guide with prerequisites.
   - **Snapshot Data:** Excellent motivation section. Shows concrete account balances data table and explains the double-counting problem clearly: "If you query SUM(balance) grouped by customer_id across both dates, you get 1050 for ACME (500 + 550) -- but that is double-counting. The real current balance is 550." This is exactly the explanation needed for someone who knows SQL aggregation but has never encountered semi-additive measures.
   - **Define a Semi-Additive Metric:** Full DDL with emphasized `NON ADDITIVE BY` line. Clear explanation of what the declaration means in practice.
   - **Sort Order and NULLS Placement:** ASC/DESC and NULLS FIRST/LAST options with two concrete examples (latest balance vs earliest balance).
   - **Multiple Non-Additive Dimensions:** Syntax shown.
   - **Snapshot Behavior:** Key distinction between two cases: (1) non-additive dimension not in query -- CTE with ROW_NUMBER generated, snapshot selection active; (2) non-additive dimension in query -- standard aggregation, no CTE. Snowflake alignment noted. Both cases have query examples.
   - **Verify the Generated SQL:** Full CTE expansion shown via `explain_semantic_view()`. The generated SQL is explained: ROW_NUMBER partitioned by queried dims, ordered by non-additive dim, then `CASE WHEN __sv_rn = 1` in the aggregation.
   - **Restrictions:** Warning about mutual exclusivity with OVER.
   - **Troubleshooting:** Three specific issues (dimension not found, unexpected results, performance with multiple NA sets).
   - Followed: Back to how-to index, then link to `howto-window-metrics`

4. Navigated to: `docs/how-to/window-metrics.rst`
   - Found: Complete how-to guide with prerequisites (including familiarity with SQL window functions -- appropriate for persona).
   - **Define a Window Metric:** Full DDL with OVER clause wrapping another metric. Emphasized lines highlighting the window metric definition.
   - **PARTITION BY:** Both modes covered thoroughly in one section:
     - Plain PARTITION BY: fixed partition set. Example with `store_avg` always partitioning by store.
     - PARTITION BY EXCLUDING: dynamic partition set. Two worked examples showing how excluding dims interact with different queried dimension sets.
     - Tip explaining when to use each mode. Mutual exclusivity noted.
   - **ORDER BY with Sort and NULLS:** Sort direction and NULLS placement examples.
   - **Frame Clauses:** RANGE and ROWS with `INTERVAL '6 days' PRECEDING` rolling average example.
   - **Extra Function Arguments:** LAG with offset (30 rows) demonstrated.
   - **Required Dimensions:** Error messages shown for missing dimensions in EXCLUDING, PARTITION BY, and ORDER BY. Tip about `SHOW SEMANTIC DIMENSIONS FOR METRIC` for discovering required dimensions.
   - **Mixing Restriction:** Warning that window and aggregate metrics cannot coexist in the same query. Error message shown. Workaround provided (two separate queries, join results).
   - **Verify the Generated SQL:** CTE expansion pattern described (aggregate CTE + outer window SELECT).
   - **Troubleshooting:** Six specific error scenarios with exact error messages.
   - Type-alignment: How-to guides (goal-oriented, work + action). Correct. The user has specific tasks (define semi-additive metrics, define window metrics) and gets step-by-step directions with complete examples.
   - Language calibration: Assumes SQL window function knowledge (appropriate for intermediate). Explains NON ADDITIVE BY and PARTITION BY EXCLUDING as new, extension-specific concepts (satisfies never-assume for dimension/metric definitions). The snapshot data explanation is particularly well-calibrated -- does not over-explain basic aggregation but thoroughly explains why standard SUM fails for snapshot data.

---

## Revision Recommendations

No revision needed. All scenarios passed.

### Cross-Reference Quality (Rule 4)

Cross-referencing is thorough and consistent throughout the documentation:

- All inline code mentions of DDL commands (`CREATE SEMANTIC VIEW`, `DROP SEMANTIC VIEW`, `ALTER SEMANTIC VIEW`) link to their reference pages via `:ref:` labels.
- Function mentions (`semantic_view()`, `explain_semantic_view()`, `READ_YAML_FROM_SEMANTIC_VIEW()`, `GET_DDL()`) consistently link to their reference pages.
- How-to guides cross-reference related how-to guides and relevant reference pages in "Related" sections.
- Tutorials end with "What You Learned" sections containing cross-references and "Next" pointers.
- The YAML how-to links to the new YAML format reference (`ref-yaml-format`), and the format reference links back to the how-to and CREATE reference. This bidirectional linking is well-done.
- The Snowflake comparison page links to 10+ how-to and reference pages from the concept mapping table.

### Persona-Calibrated Language (Rule 5)

Language calibration is well-executed for the intermediate data engineer persona:

- SQL and DuckDB basics are assumed without explanation (appropriate).
- Star schema, fact tables, dimension tables, PK/FK, cardinality -- all used as shared vocabulary (appropriate for assumed knowledge).
- Semantic view concepts are always explained before use (satisfies never_assume).
- DDL syntax is always shown in full with clause-by-clause explanation (satisfies never_assume).
- Relationship modeling is explained using PK/FK terminology the audience knows, extended with semantic-view-specific concepts (satisfies never_assume).
- Snowflake/Databricks differences are called out with side-by-side syntax and explicit warnings (satisfies never_assume).
- The YAML format reference correctly notes the `RELATIONSHIPS -> joins` naming difference, preventing confusion.

### Recently Modified Pages Assessment

- **docs/how-to/yaml-definitions.rst (reordered to Import-first):** The Import-first ordering is the right structure for a how-to guide. A user arriving at this page has a task in mind, and the most common first task is "I have a YAML file or want to create from YAML" rather than "I want to export." The flow (inline import, file import, export, round-trip) builds logically and the troubleshooting section covers all seven YAML-specific error cases.

- **docs/reference/yaml-format.rst (new YAML format reference):** This page fills a critical documentation gap. Before this page existed, a user writing YAML definitions would have to reverse-engineer the format from the limited examples in the how-to guide or the CREATE reference. Now there is a complete, field-by-field specification with types, required/optional indicators, defaults, and code examples for every variant. Three details stand out as particularly valuable: (1) the SQL-to-YAML mapping table at the top, which immediately addresses the `RELATIONSHIPS -> joins` naming discrepancy; (2) the five separate Metric code examples covering every metric variant; and (3) the sub-object specifications (NonAdditiveDim, WindowSpec, WindowOrderBy) which would be impossible to discover without documentation or source code access.
