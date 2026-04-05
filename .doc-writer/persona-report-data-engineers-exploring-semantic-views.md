# Persona Report

**Generated:** 2026-04-02
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 5 PASS, 0 PARTIAL, 0 FAIL

## Summary

The documentation provides an excellent experience for an intermediate data engineer evaluating this extension as an open-source alternative to Snowflake Semantic Views. Every major user journey -- from first install through complex multi-table modeling, Snowflake comparison, catalog management, and fan trap awareness -- is fully navigable from the homepage through clear cross-references. The Diataxis structure is well-applied: tutorials teach by doing, how-to guides solve specific problems, explanations provide context, and reference pages document syntax precisely. The three gaps identified in the previous evaluation (missing cross-references for SHOW SEMANTIC VIEWS, ALTER SEMANTIC VIEW, and SHOW SEMANTIC DIMENSIONS FOR METRIC) have all been resolved. Language calibration is strong throughout -- SQL and data engineering terminology is used naturally while semantic-view-specific concepts are always explained.

---

## Scenario S1: I want to install the extension and create my first semantic view over a single table, then query it

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Homepage with six grid cards. The "Getting Started" card is prominently first: "Install the extension, create your first semantic view, and run your first query in 5 minutes."
   - Followed: "Getting Started" card to `docs/tutorials/getting-started.rst`.
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: Complete end-to-end tutorial covering installation (CLI and Python tab set with sync groups), sample data creation with realistic orders data, full `CREATE SEMANTIC VIEW` DDL with inline explanation of the `alias.name AS expression` pattern, `SHOW SEMANTIC VIEWS` for verification with expected output, all three query modes (dimensions+metrics, dimensions-only, metrics-only) with expected output, WHERE filtering on the outer query, `explain_semantic_view()` for SQL inspection, cleanup with `DROP SEMANTIC VIEW`, and a "What You Learned" summary with cross-references to all relevant reference pages.
   - The pre-release note handles the not-yet-on-registry situation gracefully with a link to the project repo.
   - The "What You Learned" summary cross-references `CREATE SEMANTIC VIEW`, `SHOW SEMANTIC VIEWS`, `semantic_view()`, `explain_semantic_view()`, and `DROP SEMANTIC VIEW` -- all with `:ref:` links.
   - Clear "Next" pointer to the multi-table tutorial.
   - Type-alignment: Tutorial (learning-oriented, study mode, action-based). Correct for first-time use.

No gaps identified. All success criteria met.

---

## Scenario S2: I want to model a star schema with multiple tables (fact + dimensions), define relationships, and query across them

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Multi-Table Semantic Views" card: "Learn to model relationships between tables and query across them."
   - Followed: Card to `docs/tutorials/multi-table.rst`.
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: Complete three-table e-commerce tutorial (orders, customers, products) with realistic sample data including dates.
   - TABLES clause: Three tables with aliases and PRIMARY KEY. Tip explains that PRIMARY KEY is metadata for the extension, not a DuckDB constraint.
   - RELATIONSHIPS clause: Both relationships with emphasized lines. Clear explanation: "o(customer_id) REFERENCES c means the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)." Satisfies the never-assume requirement for relationship modeling.
   - Selective join verification: Query requesting only customer dimensions, with instruction to verify via `explain_semantic_view()` that products is not joined. Directly satisfies the "see generated SQL to verify join correctness" success criterion.
   - Cross-table join: Both dimension tables joined when both are requested.
   - Computed dimension: `date_trunc('month', o.ordered_at)` demonstrates SQL expression dimensions.
   - DESCRIBE SEMANTIC VIEW and CREATE OR REPLACE SEMANTIC VIEW both covered.
   - UPDATE section includes a tip cross-referencing `ALTER SEMANTIC VIEW RENAME TO` with a code example, providing the navigation path to the rename command.
   - Next steps: Cross-references to howto-facts, howto-derived-metrics, howto-role-playing.
   - Type-alignment: Tutorial (progressive learning). Correct.

No gaps identified. All success criteria met.

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Concepts" card: "Understand how semantic views differ from regular views and how they compare to Snowflake."
   - Followed: Card to `docs/explanation/index.rst`.
2. Navigated to: `docs/explanation/index.rst`
   - Found: Two pages listed: semantic-views-vs-regular-views and snowflake-comparison.
   - Followed: Snowflake Comparison link.
3. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Comprehensive feature-by-feature comparison covering:
     - YAML spec disclaimer at top (note clarifying SQL DDL interface comparison only, listing YAML-specific concepts that are not applicable).
     - Concept mapping table: 10 rows mapping Snowflake SQL DDL concepts to DuckDB equivalents (CREATE SEMANTIC VIEW, TABLES, RELATIONSHIPS, DIMENSIONS, METRICS, FACTS, derived metrics, query interface, DESCRIBE, SHOW). The query interface difference (direct SQL vs table function) is visible in the table.
     - Syntax alignment: Side-by-side tab set showing actual DDL from both platforms. The PRIMARY KEY difference is immediately visible.
     - Key differences with detailed sections: (1) PK declarations -- when they are needed vs automatic resolution, with a three-row table covering native DuckDB tables, DuckDB tables without PKs, and external sources (Parquet, CSV, Iceberg, Postgres), plus an Iceberg-specific tip. (2) Query interface difference (table function vs direct SQL) with warning admonition. (3) Cardinality inference from PK/UNIQUE. (4) USING RELATIONSHIPS identical syntax.
     - Features not yet supported: Honest table listing semi-additive metrics, window functions, direct SQL, column-level security, ASOF/temporal relationships with status and rationale for each.
     - YAML spec section: Lists YAML-only features and explains they serve the Cortex Analyst AI layer.
   - Type-alignment: Explanation (understanding-oriented, cognitive). Correct for an evaluation decision.

No gaps identified. Feature-by-feature comparison with syntax examples and honest gap disclosure fully satisfies the evaluation criteria.

---

## Scenario S4: I want to inspect and manage the semantic views I have defined -- rename a view, list all views with filtering, and explore what dimensions/metrics/facts a view has

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "DDL Reference" card: "Full syntax reference for CREATE SEMANTIC VIEW and all DDL statements."
   - Followed: Card reaches the reference section. The reference index is accessible via the hidden toctree.
2. Navigated to: `docs/reference/index.rst`
   - Found: Complete listing of all DDL statements and query functions: CREATE, ALTER, DROP, DESCRIBE, SHOW SEMANTIC VIEWS, SHOW SEMANTIC DIMENSIONS, SHOW SEMANTIC METRICS, SHOW SEMANTIC FACTS, SHOW SEMANTIC DIMENSIONS FOR METRIC, plus semantic_view() and explain_semantic_view().
   - Followed: Links to ALTER, SHOW SEMANTIC VIEWS, SHOW SEMANTIC DIMENSIONS, SHOW SEMANTIC METRICS, SHOW SEMANTIC FACTS.
3. Navigated to: `docs/reference/alter-semantic-view.rst`
   - Found: Complete reference for ALTER SEMANTIC VIEW RENAME TO. Syntax grammar, both variants (with and without IF EXISTS), parameters, output columns table, examples covering rename, safe no-op with IF EXISTS, name collision error, and case-insensitive syntax. Note clarifies ALTER only supports RENAME TO; other changes use CREATE OR REPLACE.
4. Navigated to: `docs/reference/show-semantic-views.rst`
   - Found: Complete reference with LIKE/STARTS WITH/LIMIT filtering. Syntax grammar, clause order warning, case-sensitivity behavior documented (LIKE is case-insensitive via ILIKE mapping, STARTS WITH is case-sensitive). Output columns table with 5 columns (created_on, name, kind, database_name, schema_name). Version change note. Examples cover all filtering combinations, combined clauses, column selection to skip timestamp, and error-free empty results.
5. Navigated to: `docs/reference/show-semantic-dimensions.rst`
   - Found: Complete reference with IN <name> variant, same filtering clauses, output columns with 6 columns (database_name, schema_name, semantic_view_name, table_name, name, data_type). Examples cover single-view, all-views, LIKE, STARTS WITH, LIMIT, combined clauses, and error cases. Tip at bottom cross-references the FOR METRIC variant for fan-trap-aware inspection.
6. Navigated to: `docs/reference/show-semantic-metrics.rst`
   - Found: Same consistent structure. Derived metrics explicitly shown with empty table_name and explained.
7. Navigated to: `docs/reference/show-semantic-facts.rst`
   - Found: Same consistent structure. Chained facts example and data type inference example included. Version change note documents the schema evolution.

**Previous gaps resolved:**
- S4 gap 1 (SHOW SEMANTIC VIEWS cross-reference): The "What You Learned" summary in `getting-started.rst` now includes `SHOW SEMANTIC VIEWS` with a `:ref:` link to `ref-show-semantic-views`.
- S4 gap 2 (ALTER SEMANTIC VIEW cross-reference): The "Update the View" section in `multi-table.rst` now includes a tip with `:ref:` link to `ref-alter-semantic-view` and a code example.

No remaining gaps. All success criteria met.

---

## Scenario S5: I want to find out which dimensions are safe to query alongside a specific metric in a multi-table view, without triggering a fan trap

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-To Guides" card linking to the how-to index.
   - Followed: Card to `docs/how-to/index.rst`.
2. Navigated to: `docs/how-to/index.rst`
   - Found: Fan traps guide listed among five how-to guides.
   - Followed: Link to `docs/how-to/fan-traps.rst`.
3. Navigated to: `docs/how-to/fan-traps.rst`
   - Found: Complete explanation of fan traps with worked example (orders + line_items), cardinality inference rules, error message text, three approaches to fixing fan trap errors, and one-to-one relationship exception.
   - At the end, a tip admonition now cross-references `SHOW SEMANTIC DIMENSIONS FOR METRIC`: "Before writing a query, you can ask the extension which dimensions are safe to combine with a specific metric." with a `:ref:` link to `ref-show-dims-for-metric` and a working code example.
   - Followed: The `:ref:` link to `ref-show-dims-for-metric`.
4. Navigated to: `docs/reference/show-semantic-dimensions-for-metric.rst`
   - Found: Complete reference page covering:
     - Opening paragraph explaining the purpose (filtering out dimensions that would cause fan traps) with a cross-reference back to `howto-fan-traps` for background.
     - Syntax with required IN and FOR METRIC clauses plus optional LIKE/STARTS WITH/LIMIT.
     - Parameters with fuzzy matching tip for error suggestions.
     - Fan trap filtering logic: five rules (same table, many-to-one forward safe, one-to-many reverse excluded, one-to-one both directions safe, derived metrics trace dependencies to union of source tables).
     - Output columns table with 4 columns (table_name, name, data_type, required). Version change note.
     - Examples: single-table view (all dimensions safe), multi-table three-table chain showing excluded vs included dimensions for two different metrics (order_total vs line_item_sum) with clear explanation, derived metrics inheritance example, LIKE/STARTS WITH/LIMIT applied after fan trap filtering, error cases.
   - Type-alignment: Reference (information-oriented, work mode). Correct for a lookup use case.

**Previous gap resolved:** The fan-traps how-to now includes the forward cross-reference to `SHOW SEMANTIC DIMENSIONS FOR METRIC`, completing the bidirectional navigation between the how-to guide and the reference page.

5. Alternative path verified: `docs/reference/show-semantic-dimensions.rst` also ends with a tip cross-referencing `ref-show-dims-for-metric` for fan-trap-aware inspection. Multiple discovery paths now exist.

No remaining gaps. All success criteria met.

---

## Revision Recommendations

No revision needed. All scenarios passed.

All three gaps identified in the previous evaluation (2026-03-25) have been resolved:
- `SHOW SEMANTIC VIEWS` now has a `:ref:` cross-reference in the getting-started tutorial summary.
- `ALTER SEMANTIC VIEW RENAME TO` is now cross-referenced from the multi-table tutorial's "Update the View" section.
- `SHOW SEMANTIC DIMENSIONS FOR METRIC` is now cross-referenced from the fan-traps how-to guide.
