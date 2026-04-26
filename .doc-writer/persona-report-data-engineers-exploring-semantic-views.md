# Persona Report

**Generated:** 2026-04-26
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 5 PASS, 0 PARTIAL, 0 FAIL

## Summary

The documentation provides an excellent experience for an intermediate data engineer evaluating DuckDB Semantic Views as an open-source alternative to Snowflake or Databricks. The Diataxis structure is well-executed across all four quadrants: tutorials teach through guided hands-on examples, how-to guides solve specific tasks with prerequisites and troubleshooting, reference pages document syntax with parameter tables and worked examples, and explanation pages provide context for platform comparison and decision-making. All five scenarios were achievable from start to finish with clear navigation paths, complete code examples with expected output, and language calibrated to someone who knows SQL and data engineering but is new to semantic views.

---

## Scenario S1: I want to install the extension and create my first semantic view over a single table, then query it

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Homepage with six navigation cards organized in two grids. "Getting started" card is first and prominently placed.
   - Followed: "Getting started" card link to `tutorial-getting-started`
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: Complete tutorial with time estimate (5 minutes) and appropriate prerequisites (DuckDB installed, basic SQL).
   - Install section uses tab-set for CLI and Python -- both paths are clear and complete.
   - Sample data uses realistic `orders` table with INSERT statements, not placeholder data.
   - `CREATE SEMANTIC VIEW` DDL includes line-by-line explanation of the `alias.name AS expression` pattern, satisfying the never-assume requirement for DDL syntax.
   - All three query modes demonstrated (dimensions+metrics, dimensions-only, metrics-only) with complete SQL and expected result tables.
   - `explain_semantic_view()` shown for SQL inspection with cross-reference to its reference page.
   - "What You Learned" summary has cross-references to all six relevant reference pages and a "Next" pointer to the multi-table tutorial.
   - Type-alignment: Tutorial (learning-oriented, study + action). Exactly what a first-time user needs.

---

## Scenario S2: I want to model a star schema with multiple tables (fact + dimensions), define relationships, and query across them

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Multi-table semantic views" card on homepage
   - Followed: Card link to `tutorial-multi-table`
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: Complete tutorial with time estimate (10 minutes), prerequisites linking back to getting-started, and star schema familiarity noted (appropriate for persona's assumed knowledge).
   - Three-table e-commerce schema (orders, customers, products) with realistic sample data. Tables correctly labeled as fact and dimension tables.
   - RELATIONSHIPS clause explained clearly with emphasized lines: "the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)." Satisfies never-assume for relationship modeling.
   - Selective join behavior demonstrated -- query for customer_name + revenue joins only customers, not products. `explain_semantic_view()` used to verify.
   - Computed dimension (`date_trunc('month', o.ordered_at)`) demonstrated with output.
   - `CREATE OR REPLACE` workflow shown. Tip about `ALTER SEMANTIC VIEW ... RENAME TO` with cross-reference.
   - "What You Learned" links forward to the building-a-model tutorial and relevant how-to guides (facts, derived metrics, role-playing dimensions).
   - Tip: "The PRIMARY KEY declaration is used by the extension to synthesize JOIN ON clauses. It does not create a constraint in DuckDB." -- important clarification for data engineers working with external sources.
   - Type-alignment: Tutorial (progressive hands-on learning). Correct.

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake and Databricks Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Snowflake comparison" card on homepage (direct link)
   - Followed: Card link to `explanation-snowflake`
2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Comprehensive comparison with 22-row concept mapping table, side-by-side syntax tab-set, seven detailed "Key Differences" sections (PK declarations with three-case table by data source type, query interface, cardinality inference, USING RELATIONSHIPS, facts query mode, semi-additive/window metrics, materializations), "Features Not Yet Supported" table, and YAML spec disclaimer.
   - Cross-references to how-to guides and reference pages are embedded throughout the concept mapping table.
   - The three-row table for PK resolution by data source type (native with PK, native without, external) is especially relevant for the Iceberg use case from this persona's discovery story.
3. Navigated to: `docs/explanation/index.rst` (via navbar Explanation tab)
   - Found: Index listing all three explanation pages including Databricks comparison
   - Followed: Link to `explanation-databricks`
4. Navigated to: `docs/explanation/databricks-comparison.rst`
   - Found: Parallel structure to Snowflake comparison. 18-row concept mapping, side-by-side syntax, key differences (multi-table handling showing explicit JOIN vs declarative RELATIONSHIPS, query interface, MEASURES vs METRICS, dimension expressions), 11-row "Features in DuckDB Not in Databricks" table, 5-row "Features in Databricks Not in DuckDB" table, and honest "Choosing Between Them" positioning (lightweight/local-first vs cloud platform).
   - Type-alignment: Explanation pages (understanding-oriented, cognition-based). Correct for platform evaluation and decision-making.

---

## Scenario S4: I want to define a semantic view with pre-aggregated materializations and export it as YAML for version control

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-to guides" card mentioning "materializations, YAML definitions" in description
   - Followed: Card link to `how-to-guides`
2. Navigated to: `docs/how-to/index.rst`
   - Found: Both guides listed under "Operations" category with descriptive summaries
   - Followed: Link to `howto-materializations`
3. Navigated to: `docs/how-to/materializations.rst`
   - Found: Complete how-to with versionadded 0.7.0 marker. Covers concept (transparent routing, query interface unchanged), declaration (pre-aggregated table first, then MATERIALIZATIONS clause with emphasized lines, clause ordering rule), exact-match routing logic with match/no-match examples, warning about no superset matching in v0.7.0, multiple materializations with first-match semantics, routing exclusions (semi-additive/window always excluded), verification with `explain_semantic_view()` showing `-- Materialization:` header, SHOW/DESCRIBE introspection, five troubleshooting entries, and Related section with cross-references to CREATE reference, SHOW reference, explain reference, and semi-additive/window how-tos.
4. Followed: Navigation back to how-to index, then to `howto-yaml-definitions`
5. Navigated to: `docs/how-to/yaml-definitions.rst`
   - Found: Complete how-to with versionadded 0.7.0. Covers inline YAML import (dollar-quoted with realistic example), tagged dollar-quoting variant, CREATE OR REPLACE/IF NOT EXISTS variants, file-based YAML import, export with `READ_YAML_FROM_SEMANTIC_VIEW()`, COPY TO file pattern for saving to disk, schema-qualified names, round-trip workflow (export -> import -> verify with GET_DDL), tip about version control, seven troubleshooting entries, and Related section linking to YAML format reference, READ_YAML reference, CREATE reference, and GET_DDL reference.
   - Type-alignment: How-to guides (goal-oriented, work + action). Correct for both pages.

---

## Scenario S5: I want to define semi-additive metrics and window function metrics for snapshot data (like account balances) and time-series analysis

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-to guides" card on homepage
   - Followed: Card link to `how-to-guides`
2. Navigated to: `docs/how-to/index.rst`
   - Found: Both guides listed under "Advanced Metrics" with clear descriptions
   - Followed: Link to `howto-semi-additive`
3. Navigated to: `docs/how-to/semi-additive-metrics.rst`
   - Found: Complete how-to. The snapshot data motivation section is excellent -- shows concrete account balances data table and explains why naive SUM double-counts: "If you query SUM(balance) grouped by customer_id across both dates, you get 1050 for ACME (500 + 550) -- but that is double-counting. The real current balance is 550." This satisfies the never-assume requirement for dimension/metric definitions and NON ADDITIVE BY syntax.
   - Covers: `NON ADDITIVE BY` syntax with emphasized lines, sort order options (ASC/DESC/NULLS), multiple non-additive dimensions, behavioral distinction (active when NA dim absent from query, standard when present), generated SQL verification showing ROW_NUMBER CTE with `CASE WHEN __sv_rn = 1`, restriction warning (cannot combine with OVER), three troubleshooting entries.
4. Navigated to: `docs/how-to/window-metrics.rst`
   - Found: Complete how-to. Prerequisite notes familiarity with SQL window functions (appropriate for persona).
   - Covers: Window metric definition with emphasized OVER clause, thorough coverage of PARTITION BY vs PARTITION BY EXCLUDING (fixed vs dynamic partition sets, worked examples, mutual exclusivity, tip on when to use each), ORDER BY with sort/NULLS, frame clauses (RANGE/ROWS with interval example), extra function arguments (LAG with offset), required dimensions with error messages, mixing restriction (window + aggregate metrics cannot coexist) with error message and workaround, generated SQL verification (aggregate CTE + outer window SELECT), six troubleshooting entries.
   - Type-alignment: How-to guides (goal-oriented, work + action). Correct. Users have specific tasks and get step-by-step directions.
   - Language calibration: Assumes SQL window function knowledge (appropriate for intermediate). Explains extension-specific concepts (NON ADDITIVE BY, PARTITION BY EXCLUDING) thoroughly. Snapshot data explanation is well-calibrated -- does not over-explain basic aggregation but thoroughly explains why standard SUM fails.

---

## Revision Recommendations

No revision needed. All scenarios passed.

### Cross-Reference Quality (Rule 4)

Cross-referencing is thorough and consistent:

- All inline code mentions of DDL commands link to their reference pages via `:ref:` labels.
- Function mentions (`semantic_view()`, `explain_semantic_view()`, `READ_YAML_FROM_SEMANTIC_VIEW()`, `GET_DDL()`) consistently link to reference pages.
- How-to guides cross-reference related guides and reference pages in "Related" sections.
- Tutorials end with "What You Learned" sections containing cross-references and "Next" pointers.
- The Snowflake comparison links to 10+ how-to and reference pages from within the concept mapping table.
- Bidirectional linking between how-to guides and their corresponding reference pages is consistently maintained.

### Persona-Calibrated Language (Rule 5)

Language calibration is well-executed for the intermediate data engineer persona:

- SQL and DuckDB basics assumed without explanation (appropriate per assumed_knowledge).
- Star schema, fact tables, dimension tables, PK/FK, cardinality used as shared vocabulary (appropriate).
- Semantic view concepts always explained before use (satisfies never_assume).
- DDL syntax always shown in full with clause-by-clause explanation (satisfies never_assume).
- Relationship modeling explained using PK/FK terminology the audience knows, extended with semantic-view-specific concepts (satisfies never_assume).
- Snowflake/Databricks differences called out with side-by-side syntax and explicit warnings (satisfies never_assume).
