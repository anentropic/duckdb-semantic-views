# Persona Report

**Generated:** 2026-04-13
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 5 PASS, 0 PARTIAL, 0 FAIL

## Summary

The documentation provides a complete and well-structured experience for data engineers evaluating DuckDB Semantic Views as an open-source alternative to Snowflake. Every major user journey -- from first install through complex multi-table modeling, Snowflake feature comparison, catalog management, and fan trap awareness -- is fully navigable from the homepage through clear cross-references. The Diataxis structure is well-applied: tutorials teach by doing with realistic analytics data, how-to guides solve specific problems, explanations provide context for decision-making, and reference pages document exact syntax with output columns and worked examples. Language calibration is strong -- SQL and data engineering terminology is used naturally while semantic-view-specific concepts (TABLES clause, RELATIONSHIPS, FACTS, derived metrics, fan traps, USING RELATIONSHIPS) are always explained from scratch. New pages added since the last evaluation (semi-additive metrics, window metrics, wildcard selection, query facts, metadata annotations, GET_DDL, SHOW COLUMNS) are well-integrated into the navigation via the how-to index, reference index, and Snowflake comparison concept mapping table.

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
   - Sample data: Realistic `orders` table with INSERT VALUES.
   - DDL: Full `CREATE SEMANTIC VIEW` with inline explanation of the `alias.name AS expression` pattern. The TABLES, DIMENSIONS, and METRICS clauses are each explained.
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
   - Found: Complete tutorial with realistic e-commerce schema (customers, products, orders) including dates.
   - TABLES clause: Three tables with aliases and PRIMARY KEY declarations. Tip clarifies PRIMARY KEY is metadata only, not a DuckDB constraint.
   - RELATIONSHIPS clause: Two FK references with emphasize-lines. Clear prose: "o(customer_id) REFERENCES c means the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)." Satisfies never-assume for relationship modeling.
   - Selective join: Query for customer dimensions only, with `explain_semantic_view()` to verify products table is not joined. Directly satisfies "see generated SQL to verify join correctness."
   - Cross-table: Both dimension tables joined when both are requested, with expected output.
   - Computed dimension: `date_trunc('month', o.ordered_at)` demonstrates expression-based dimensions.
   - DESCRIBE: Full view inspection.
   - CREATE OR REPLACE: View update with new dimension and metric. Tip cross-references ALTER SEMANTIC VIEW RENAME TO.
   - Next steps: Links to howto-facts, howto-derived-metrics, howto-role-playing.

Type-alignment: Tutorial (progressive hands-on learning). Correct. All success criteria met.

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "Snowflake comparison" with description "Feature-by-feature comparison with Snowflake's CREATE SEMANTIC VIEW."
   - Followed: Link to `explanation-snowflake`
2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Comprehensive comparison page.
   - YAML spec disclaimer: Note clarifying SQL DDL interface comparison only.
   - Concept mapping table: 16+ rows covering CREATE/ALTER/DROP/DESCRIBE/SHOW/GET_DDL, TABLES, RELATIONSHIPS, DIMENSIONS, METRICS, FACTS, derived metrics, semi-additive metrics, window metrics, metadata annotations, access modifiers, wildcard selection, query interface. Cross-references to relevant how-to and reference pages throughout.
   - Syntax alignment: Side-by-side tab set showing actual DDL from both platforms. PRIMARY KEY difference is immediately visible.
   - Key Differences sections:
     - PRIMARY KEY declarations: Three-case table (native DuckDB with PK, without PK, external sources) plus Iceberg-specific tip. Thorough.
     - Query interface: Warning admonition about table function vs direct SQL. Side-by-side examples including Snowflake's AGG syntax (noted as not supported).
     - Cardinality inference: Explains PK/UNIQUE-based inference.
     - USING RELATIONSHIPS: Identical syntax confirmed.
     - Facts query mode: v0.6.0 addition with `facts := [...]` parameter.
     - Semi-additive and window metrics: v0.6.0 addition with behavioral differences listed.
   - Features not yet supported: Honest table with 3 items (direct SQL query interface, column-level security, ASOF/temporal relationships) with status and rationale.
   - YAML spec section: Lists YAML-only concepts (time_dimensions, custom_instructions, access_modifier, sample_values) and explains they serve Cortex Analyst.

Type-alignment: Explanation (understanding-oriented, study + cognition). Correct for platform evaluation. All success criteria met.

---

## Scenario S4: I want to inspect and manage the semantic views I have defined -- rename a view, list all views with filtering, and explore what dimensions/metrics/facts a view has

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "DDL reference" linking to CREATE SEMANTIC VIEW. The hidden toctree includes `reference/index`.
   - Followed: Navigation to reference section.
2. Navigated to: `docs/reference/index.rst`
   - Found: Complete index listing all 11 DDL statements and 2 query functions with one-line descriptions. Clear links to ALTER, SHOW SEMANTIC VIEWS, SHOW SEMANTIC DIMENSIONS, SHOW SEMANTIC METRICS, SHOW SEMANTIC FACTS, plus newer entries (SHOW COLUMNS, GET_DDL).
3. Navigated to: `docs/reference/alter-semantic-view.rst`
   - Found: Complete reference for RENAME TO, SET COMMENT, UNSET COMMENT. Syntax grammar, all 6 variants (with/without IF EXISTS for each operation), parameters, output columns tables for each variant, realistic examples including rename, safe no-op, name collision error, comment set/unset with output, and case-insensitive syntax.
4. Navigated to: `docs/reference/show-semantic-views.rst`
   - Found: Complete reference with TERSE variant, LIKE (case-insensitive via ILIKE), IN SCHEMA/IN DATABASE, STARTS WITH (case-sensitive), LIMIT. Clause order warning. Output columns tables (6 for full, 5 for TERSE). Examples covering all filtering combinations, combined clauses, column selection technique, and empty results.
5. Navigated to: `docs/reference/show-semantic-dimensions.rst`
   - Found: Complete reference with IN <name> variant, 8 output columns including synonyms and comment. Clause order warning. Examples with expected output. Cross-reference tip to FOR METRIC variant.
6. Navigated to: `docs/reference/show-semantic-metrics.rst`
   - Found: Same consistent structure. 8 output columns. Derived metrics explanation with empty table_name. Complete examples.
7. Navigated to: `docs/reference/show-semantic-facts.rst`
   - Found: Same consistent structure. 8 output columns with data_type inference. Chained facts example explaining why data_type is empty for fact-referencing-fact. Error cases.

Structural consistency (Rule 3): All SHOW reference pages follow the same template: Syntax, Statement Variants, Parameters, Optional Filtering Clauses, Output Columns, Examples. All ALTER/SHOW/DESCRIBE pages use the same section ordering. Strong consistency.

Type-alignment: Reference (information-oriented, work + cognition). Correct for looking up exact syntax and output schemas. All success criteria met.

---

## Scenario S5: I want to find out which dimensions are safe to query alongside a specific metric in a multi-table view, without triggering a fan trap

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Grid card "How-to guides" mentioning "fan traps."
   - Followed: Link to `how-to-guides`
2. Navigated to: `docs/how-to/index.rst`
   - Found: "howto-fan-traps" listed with description: "Understand, detect, and resolve fan traps that inflate aggregation results in multi-table views."
   - Also found: "SHOW SEMANTIC DIMENSIONS FOR METRIC" referenced in the reference index (accessible via the "DDL reference" homepage card).
   - Followed: Link to reference index first for the specific command.
3. Navigated to: `docs/reference/index.rst`
   - Found: "SHOW SEMANTIC DIMENSIONS ... FOR METRIC -- List dimensions safe to use with a specific metric (fan trap aware)."
   - Followed: Link to `ref-show-dims-for-metric`
4. Navigated to: `docs/reference/show-semantic-dimensions-for-metric.rst`
   - Found: Complete reference page with:
     - Opening explanation with cross-reference back to `howto-fan-traps` for background (bidirectional link).
     - Syntax: IN and FOR METRIC as required clauses, LIKE/STARTS WITH/LIMIT as optional.
     - Parameters with fuzzy matching tip for error messages.
     - Fan trap filtering logic: 6 rules (same table, many-to-one safe, one-to-many excluded, one-to-one both safe, derived metrics trace to union of source tables, window metrics skip fan trap checking with required column).
     - Output columns: 4 columns including `required` boolean for window metrics.
     - Examples: Single-table (all dimensions safe), multi-table chain with clear explanation of why item_qty is excluded for order_total but included for line_item_sum, window metric with required dimensions, LIKE/STARTS WITH/LIMIT filtering, derived metrics inheritance, error cases.
   - Alternative path: `docs/reference/show-semantic-dimensions.rst` ends with a tip cross-referencing this page. `docs/how-to/fan-traps.rst` also cross-references this command.

Type-alignment: Reference (information-oriented) with strong worked examples that also serve a how-to function. Correct for the persona's need to programmatically discover safe dimension combinations. All success criteria met.

---

## Revision Recommendations

No revision needed. All scenarios passed.
