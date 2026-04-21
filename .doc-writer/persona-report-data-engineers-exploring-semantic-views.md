# Persona Report

**Generated:** 2026-04-21
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 5 PASS, 0 PARTIAL, 0 FAIL

## Summary

The documentation provides an excellent experience for an intermediate data engineer evaluating DuckDB Semantic Views as an open-source alternative to Snowflake or Databricks. The Diataxis structure is well-executed: tutorials teach through guided hands-on examples with realistic analytics data, how-to guides solve specific tasks with prerequisites and troubleshooting, reference pages document syntax with parameter tables and worked examples, and explanation pages provide context for platform comparison and decision-making. All five scenarios were achievable from start to finish with clear navigation paths, complete code examples with expected output, and language calibrated to someone who knows SQL and data engineering but is new to semantic views. New features added since the last evaluation (materializations, YAML definitions, Databricks comparison) are fully documented and well-integrated into the navigation structure.

---

## Scenario S1: I want to install the extension and create my first semantic view over a single table, then query it

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Getting started" with description "Install the extension, create your first semantic view, and run a query in 5 minutes." Prominently first card.
   - Followed: Link to `tutorial-getting-started`
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: Complete tutorial with time estimate (5 minutes) and prerequisites (DuckDB installed, basic SQL knowledge).
   - Install: Tab set for DuckDB CLI and Python, showing `INSTALL semantic_views FROM community; LOAD semantic_views;`
   - Sample data: Realistic `orders` table with INSERT VALUES (regions, categories, amounts).
   - DDL: Full `CREATE SEMANTIC VIEW` with inline explanation of the `alias.name AS expression` pattern. TABLES, DIMENSIONS, and METRICS clauses each explained.
   - Verification: `SHOW SEMANTIC VIEWS` with expected output table.
   - Query: All three query modes demonstrated (dimensions+metrics, dimensions-only, metrics-only) with complete SQL and expected result tables. WHERE filtering also shown.
   - Inspection: `explain_semantic_view()` for generated SQL, cross-referenced to its reference page.
   - Cleanup: `DROP SEMANTIC VIEW`.
   - Summary: "What You Learned" section with cross-references to CREATE SEMANTIC VIEW, SHOW SEMANTIC VIEWS, semantic_view(), explain_semantic_view(), and DROP SEMANTIC VIEW reference pages.
   - Next: Clear pointer to multi-table tutorial.

Type-alignment: Tutorial (learning-oriented, study + action). Exactly what a first-time user needs. No friction. All success criteria met.

---

## Scenario S2: I want to model a star schema with multiple tables (fact + dimensions), define relationships, and query across them

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Multi-table semantic views" with description "Model relationships between tables and query across them."
   - Followed: Link to `tutorial-multi-table`
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: Complete tutorial with realistic e-commerce star schema (customers, products, orders) including dates.
   - TABLES clause: Three tables with aliases and PRIMARY KEY declarations. Tip clarifies PRIMARY KEY is metadata only, not a DuckDB constraint.
   - RELATIONSHIPS clause: Two FK references with emphasized lines. Clear prose: "o(customer_id) REFERENCES c means the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)." Satisfies never-assume for relationship modeling.
   - Selective join: Query for customer dimensions only, with `explain_semantic_view()` to verify products table is not joined. Directly satisfies "see generated SQL to verify join correctness."
   - Cross-table: Both dimension tables joined when both requested, with expected output.
   - Computed dimension: `date_trunc('month', o.ordered_at)` demonstrates expression-based dimensions.
   - DESCRIBE: Full view inspection.
   - CREATE OR REPLACE: View update with new dimension and metric. Tip cross-references ALTER SEMANTIC VIEW RENAME TO.
   - Next steps: Links to howto-facts, howto-derived-metrics, howto-role-playing.

Type-alignment: Tutorial (progressive hands-on learning). Correct. All success criteria met.

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake and Databricks Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Snowflake comparison" with description "Feature-by-feature comparison with Snowflake's CREATE SEMANTIC VIEW."
   - Followed: Link to `explanation-snowflake`
2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Comprehensive comparison page.
   - YAML spec disclaimer: Note clarifying SQL DDL interface comparison only.
   - Concept mapping table: 19 rows covering all DDL statements (CREATE/ALTER/DROP/DESCRIBE/SHOW/GET_DDL), core model concepts (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS, FACTS), advanced features (semi-additive, window, materializations, wildcards, metadata annotations, access modifiers), and query interface. Cross-references to relevant how-to and reference pages throughout.
   - Syntax alignment: Side-by-side tab set showing DDL from both platforms. PRIMARY KEY difference immediately visible.
   - Key Differences sections: Primary Key declarations with three-case table and Iceberg tip, Query Interface with warning about table function vs direct SQL, Cardinality Inference, USING RELATIONSHIPS, Facts Query Mode, Semi-Additive and Window Metrics, Materializations (unique to DuckDB).
   - Features not yet supported: Honest three-item table with status and rationale.
   - YAML spec section: Explains Snowflake YAML-only concepts and clarifies DuckDB YAML format serves version control, not AI prompt tuning.
3. Navigated to: `docs/explanation/index.rst` (via sidebar)
   - Found: Link to Databricks Comparison
   - Followed: Link to `explanation-databricks`
4. Navigated to: `docs/explanation/databricks-comparison.rst`
   - Found: Parallel structure to Snowflake comparison.
   - Concept mapping table covering all key concepts with clear terminology mapping (MEASURES vs METRICS, FROM clause vs TABLES+RELATIONSHIPS).
   - Side-by-side syntax tab set.
   - Key Differences: Multi-table handling (explicit JOIN in FROM vs declarative RELATIONSHIPS with join synthesis), Query Interface, MEASURES vs METRICS keyword, Dimension Expressions.
   - Features unique to DuckDB: 11-row table including FACTS, NON ADDITIVE BY, window metrics, MATERIALIZATIONS, RELATIONSHIPS, fan trap detection, role-playing dimensions, YAML import/export.
   - Features unique to Databricks: 5-row table including direct SQL, Unity Catalog, row-level security, AI/BI integration, Delta Lake materialized views.
   - "Choosing Between Them" section positioning DuckDB as lightweight, local-first alternative.

Type-alignment: Explanation (study + cognition). Correct for platform evaluation and decision-making. All success criteria met.

---

## Scenario S4: I want to define a semantic view with pre-aggregated materializations and export it as YAML for version control

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "How-to guides" mentioning materializations and YAML definitions in its description.
   - Followed: Link to `how-to-guides`
2. Navigated to: `docs/how-to/index.rst`
   - Found: Bulleted list with links to howto-materializations ("Declare materializations that route matching queries to pre-aggregated tables") and howto-yaml-definitions ("Export and import semantic view definitions as YAML for version control and migration").
   - Followed: Link to howto-materializations
3. Navigated to: `docs/how-to/materializations.rst`
   - Found: Complete how-to guide with versionadded 0.7.0 marker.
   - Concept: Clear explanation of what materializations do (map dims+metrics to pre-aggregated table).
   - Declare: Step-by-step with pre-aggregated table creation, then MATERIALIZATIONS clause with emphasized lines. Tip about column naming requirement.
   - Routing: Exact-match logic explained with two examples (matching vs non-matching). Warning about superset matching not being supported.
   - Multiple: Multiple materializations with definition-order precedence.
   - Exclusions: Semi-additive and window metrics always excluded with code example.
   - Verify: explain_semantic_view() with Materialization header line in output.
   - Inspect: SHOW SEMANTIC MATERIALIZATIONS and DESCRIBE with filtering.
   - Troubleshooting: 5 specific error scenarios with causes and fixes.
   - Related: Cross-references to CREATE reference, SHOW reference, explain reference, and related how-to guides.
   - Followed: Link back to how-to index for YAML guide
4. Navigated to: `docs/how-to/yaml-definitions.rst`
   - Found: Complete how-to guide with versionadded 0.7.0 marker.
   - Export: READ_YAML_FROM_SEMANTIC_VIEW() with COPY-to-file pattern and schema-qualified names.
   - Stripped fields: Explains which internal fields are omitted and why.
   - Import inline: FROM YAML with dollar-quoting, tagged dollar-quoting, CREATE OR REPLACE and IF NOT EXISTS variants.
   - Import file: FROM YAML FILE with single-quoted path.
   - Round-trip: Three-step workflow (export, import, verify with GET_DDL) with version control tip.
   - Troubleshooting: 7 specific error scenarios with causes and fixes.
   - Related: Cross-references to READ_YAML reference, CREATE reference, GET_DDL reference.

Type-alignment: How-to guides (work + action). Correct for task-oriented needs. All success criteria met.

---

## Scenario S5: I want to define semi-additive metrics and window function metrics for snapshot data and time-series analysis

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "How-to guides"
   - Followed: Link to `how-to-guides`
2. Navigated to: `docs/how-to/index.rst`
   - Found: Links to howto-semi-additive ("Define metrics with NON ADDITIVE BY for snapshot data like account balances and inventory levels") and howto-window-metrics ("Define window function metrics for rolling averages, lag comparisons, and rankings using OVER clauses").
   - Followed: Link to howto-semi-additive
3. Navigated to: `docs/how-to/semi-additive-metrics.rst`
   - Found: Complete how-to guide.
   - Snapshot Data: Clear explanation of the double-counting problem using account balances -- a realistic example table shows why SUM(balance) across dates gives wrong results.
   - Define: NON ADDITIVE BY syntax with sort order (DESC NULLS FIRST) and emphasized line.
   - Sort Order: ASC/DESC and NULLS FIRST/LAST options with examples.
   - Multiple: Multiple non-additive dimensions supported.
   - Behavior: Key distinction between active (NA dim not in query, CTE generated) and inactive (NA dim in query, standard aggregation). Snowflake alignment noted.
   - Verify: explain_semantic_view() showing the ROW_NUMBER CTE and CASE WHEN __sv_rn = 1 pattern. Generated SQL is fully shown.
   - Restrictions: Warning that NON ADDITIVE BY and OVER cannot be combined.
   - Troubleshooting: 3 specific issues with fixes.
4. Navigated to: `docs/how-to/index.rst` (back), then to howto-window-metrics
5. Navigated to: `docs/how-to/window-metrics.rst`
   - Found: Complete how-to guide.
   - Define: Window metric wrapping another metric with OVER clause.
   - PARTITION BY: Clear explanation of fixed PARTITION BY vs dynamic PARTITION BY EXCLUDING, with tip on when to use each.
   - ORDER BY: Sort direction and NULLS placement.
   - Frame Clauses: RANGE and ROWS support with 7-day rolling average example.
   - Extra Args: LAG/LEAD with offset argument.
   - Required Dimensions: Error messages shown for missing required dimensions. Tip to use SHOW SEMANTIC DIMENSIONS FOR METRIC.
   - Mixing Restriction: Warning that window and aggregate metrics cannot be mixed, with error message shown and workaround (two separate queries).
   - Verify: explain_semantic_view() showing CTE expansion.
   - Troubleshooting: 7 specific error scenarios with descriptions.

Type-alignment: How-to guides (work + action). Correct. All success criteria met.

---

## Revision Recommendations

No revision needed. All scenarios passed.
