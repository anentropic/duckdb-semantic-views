# Persona Report

**Generated:** 2026-03-25
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 3 PASS, 2 PARTIAL, 0 FAIL

## Summary

The core documentation experience -- installation, single-table and multi-table tutorials, and the Snowflake comparison -- remains thorough and well-calibrated for an intermediate data engineer. Five new reference pages added since the last evaluation (ALTER SEMANTIC VIEW, SHOW SEMANTIC DIMENSIONS, SHOW SEMANTIC METRICS, SHOW SEMANTIC FACTS, SHOW SEMANTIC DIMENSIONS FOR METRIC) are individually complete and well-written. The gap is navigational: these new pages are not cross-referenced from the tutorials or how-to guides that would naturally lead a user to them. Specifically, the getting-started tutorial uses `SHOW SEMANTIC VIEWS` without linking to its reference page (Rule 4), and the fan-traps how-to guide has no mention of `SHOW SEMANTIC DIMENSIONS FOR METRIC` -- the command specifically designed to answer the question "which dimensions can I safely combine with this metric?"

---

## Scenario S1: I want to install the extension and create my first semantic view over a single table, then query it

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: Six grid cards. The "Getting Started" card is prominently first: "Install the extension, create your first semantic view, and run your first query in 5 minutes."
   - Followed: "Getting Started" card to `docs/tutorials/getting-started.rst`
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: Complete end-to-end tutorial. Installation with CLI and Python tab-set. Pre-release note with link to project repo for build-from-source.
   - DDL section: TABLES, DIMENSIONS, and METRICS each introduced before use. The `alias.name AS expression` pattern is explicitly explained: "o.region AS o.region creates a dimension called region from the region column of the table aliased as o." This satisfies the never-assume requirement for DDL syntax and dimension definitions.
   - All three query modes shown with expected output: dimensions + metrics (grouped aggregation), dimensions only (distinct values), metrics only (grand total).
   - Filtering with WHERE on the outer query is demonstrated.
   - `SHOW SEMANTIC VIEWS` used to verify the view was created, with expected output shown.
   - `explain_semantic_view()` introduced with a working example.
   - Cleanup with DROP SEMANTIC VIEW.
   - "What You Learned" summary with cross-references to CREATE SEMANTIC VIEW, semantic_view(), explain_semantic_view(), DROP SEMANTIC VIEW reference pages.
   - Ends with cross-reference to multi-table tutorial.
   - Type-alignment: Tutorial (learning-oriented, study mode, action-based). Correct for first-time use.

No gaps identified. All success criteria met.

---

## Scenario S2: I want to model a star schema with multiple tables (fact + dimensions), define relationships, and query across them

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Multi-Table Semantic Views" card: "Learn to model relationships between tables and query across them."
   - Followed: Card to `docs/tutorials/multi-table.rst`
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: Complete 3-table e-commerce tutorial (orders, customers, products) with realistic sample data including dates.
   - TABLES clause: Three tables with aliases and PRIMARY KEY. Tip explains that PRIMARY KEY is metadata for the extension, not a DuckDB constraint.
   - RELATIONSHIPS clause: Both relationships annotated with emphasize-lines. The explanation is clear: "o(customer_id) REFERENCES c means the customer_id column on orders (alias o) is a foreign key to the primary key of customers (alias c)." Satisfies the never-assume requirement for relationship modeling.
   - Selective join verification: query requesting only customer dimensions, with instruction to verify via `explain_semantic_view()` that products is excluded. This directly satisfies the "see generated SQL to verify join correctness" success criterion.
   - Cross-table join: both dimension tables joined when dimensions from both are requested.
   - Computed dimension: `date_trunc('month', o.ordered_at)` demonstrates SQL expression dimensions.
   - DESCRIBE SEMANTIC VIEW and CREATE OR REPLACE SEMANTIC VIEW both covered.
   - Next steps: cross-references to howto-facts, howto-derived-metrics, howto-role-playing.
   - Type-alignment: Tutorial (progressive learning). Correct.

No gaps identified. All success criteria met.

---

## Scenario S3: I want to compare this extension's capabilities and syntax with Snowflake Semantic Views to decide if it fits my use case

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "Concepts" card: "Understand how semantic views differ from regular views and how they compare to Snowflake."
   - Followed: Card to `docs/explanation/index.rst`
2. Navigated to: `docs/explanation/index.rst`
   - Found: Two pages: semantic-views-vs-regular-views and snowflake-comparison.
   - Followed: `snowflake-comparison`
3. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: Comprehensive comparison covering all elements needed for a platform evaluation decision.
   - YAML spec disclaimer at top: note clarifies comparisons target the SQL DDL interface only. This correctly implements the never-assume guidance and prevents confusion from YAML-spec-only concepts.
   - Concept mapping: 10-row table mapping Snowflake SQL DDL concepts to DuckDB equivalents (CREATE SEMANTIC VIEW, TABLES, RELATIONSHIPS, DIMENSIONS, METRICS, FACTS, derived metrics, query interface, DESCRIBE, SHOW).
   - Syntax alignment: Side-by-side tab-set comparing actual DDL. The PRIMARY KEY difference is visible in the code.
   - Key differences with `.. warning::` admonitions: (1) PK declarations required in DuckDB, (2) table function query interface vs direct SQL, (3) cardinality inference from PK/UNIQUE, (4) USING RELATIONSHIPS identical.
   - Unsupported features: honest table listing semi-additive metrics, window functions, direct SQL, column-level security, ASOF relationships with status for each.
   - YAML spec section: lists YAML-only features and explains they are not applicable.
   - Type-alignment: Explanation (understanding-oriented, cognitive). Correct for an evaluation decision.

No gaps identified. Feature-by-feature comparison with syntax examples and honest gap disclosure fully satisfies the evaluation criteria.

---

## Scenario S4: I want to inspect and manage the semantic views I have defined -- rename a view, list all views with filtering, and explore what dimensions/metrics/facts a view has

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "DDL Reference" card: "Full syntax reference for CREATE SEMANTIC VIEW and all DDL statements."
   - Followed: Card to `docs/reference/create-semantic-view.rst`
2. Navigated to: `docs/reference/create-semantic-view.rst`
   - Found: Full CREATE syntax reference. The left sidebar (via the reference/index toctree) shows all reference pages: create-semantic-view, alter-semantic-view, drop-semantic-view, describe-semantic-view, show-semantic-views, show-semantic-dimensions, show-semantic-metrics, show-semantic-facts, show-semantic-dimensions-for-metric, semantic-view-function, explain-semantic-view-function, error-messages.
   - From the sidebar, navigated to `alter-semantic-view.rst`, `show-semantic-views.rst`, `show-semantic-dimensions.rst`, `show-semantic-metrics.rst`, `show-semantic-facts.rst`.
3. Navigated to: `docs/reference/alter-semantic-view.rst`
   - Found: Complete reference for ALTER SEMANTIC VIEW RENAME TO. Both variants (with and without IF EXISTS) documented. Output columns described. Examples cover rename, safe no-op, name-already-exists error. Snowflake warning notes that ALTER in Snowflake supports ADD/DROP/ALTER on individual entities; this extension currently supports only RENAME TO.
   - Content is complete and well-calibrated for an intermediate user.
4. Navigated to: `docs/reference/show-semantic-views.rst`
   - Found: Complete reference with LIKE/STARTS WITH/LIMIT filtering. Clause order warning present. Examples cover all filtering combinations. Case-sensitivity behavior documented (LIKE is case-insensitive, STARTS WITH is case-sensitive). Cross-references from "Unsupported Clauses" section link to SHOW SEMANTIC DIMENSIONS, SHOW SEMANTIC METRICS, SHOW SEMANTIC FACTS.
5. Navigated to: `docs/reference/show-semantic-dimensions.rst`, `show-semantic-metrics.rst`, `show-semantic-facts.rst`
   - Found: All three pages are complete. Syntax, variants (single-view vs all-views), filtering clauses, output columns, and examples are all documented. SHOW SEMANTIC METRICS explicitly shows derived metrics with empty source_table. SHOW SEMANTIC FACTS notes that data_type is absent (unlike dimensions and metrics) because facts are inlined at expansion time.
   - Each page cross-references the others where relevant.

**Friction encountered:**

The path to these pages relies on the reference section sidebar. The getting-started tutorial uses `SHOW SEMANTIC VIEWS` in a code block ("Verify the view was created: SHOW SEMANTIC VIEWS;") but provides no `:ref:` link to its reference page -- an inline code mention with no cross-reference (Rule 4). A user who wants to understand the filtering options would not know to look in the reference section unless they were already exploring the sidebar.

Similarly, `ALTER SEMANTIC VIEW` is not cross-referenced from the multi-table tutorial, which teaches `CREATE OR REPLACE SEMANTIC VIEW` as the way to update a view. A user who wants to rename a view (rather than redefine it) would not encounter `ALTER SEMANTIC VIEW` through the tutorial path -- they would need to browse the reference sidebar independently.

The reference pages themselves are correct and complete. The gap is the absence of in-prose cross-references from the tutorial and how-to content that first introduces these commands.

### Gap Analysis

**Where:** `docs/tutorials/getting-started.rst` > "Define a Semantic View" section (code block using `SHOW SEMANTIC VIEWS`)
**What:** `SHOW SEMANTIC VIEWS` appears as a code example with no `:ref:` link to `ref-show-semantic-views`. This violates Rule 4 (Cross-Reference Code Mentions). The "What You Learned" summary also omits `SHOW SEMANTIC VIEWS` from the list of covered concepts despite using it in the tutorial body.
**Impact:** A user who wants to understand filtering options for `SHOW SEMANTIC VIEWS` (LIKE, STARTS WITH, LIMIT) has no navigational path from the tutorial to the reference page. They must independently discover the reference section.
**Suggested Fix:** In `docs/tutorials/getting-started.rst`, section "Define a Semantic View": add inline text before the SHOW code block that references the statement by name with a `:ref:` link, e.g. "Verify the view was created with :ref:`SHOW SEMANTIC VIEWS <ref-show-semantic-views>`:" and add it to the "What You Learned" summary list with a link.

---

**Where:** `docs/tutorials/multi-table.rst` > "Update the View" section
**What:** The section shows `CREATE OR REPLACE SEMANTIC VIEW` for updating a view definition. There is no mention of `ALTER SEMANTIC VIEW RENAME TO` as the way to rename an existing view without redefining it. A user managing a catalog of evolving semantic views would miss this command entirely through the tutorial path.
**Impact:** A user who needs to rename a view (a common catalog management task) has no in-content path to `alter-semantic-view.rst` from any tutorial or how-to guide.
**Suggested Fix:** In `docs/tutorials/multi-table.rst`, section "Update the View": after the CREATE OR REPLACE example, add a sentence: "To rename an existing view without changing its definition, use :ref:`ALTER SEMANTIC VIEW RENAME TO <ref-alter-semantic-view>`."

---

## Scenario S5: I want to find out which dimensions are safe to query alongside a specific metric in a multi-table view, without triggering a fan trap

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: "How-To Guides" card. Followed to `docs/how-to/index.rst`.
2. Navigated to: `docs/how-to/index.rst`
   - Found: fan-traps listed among five how-to guides. Followed to `docs/how-to/fan-traps.rst`.
3. Navigated to: `docs/how-to/fan-traps.rst`
   - Found: Complete explanation of fan traps with a worked example (orders + line_items), cardinality inference rules, error message text, and three approaches to fixing fan trap errors.
   - The page explains the concept well and would satisfy a user who wants to understand fan traps and prevent them at view-definition time.
   - However: the page does NOT mention `SHOW SEMANTIC DIMENSIONS FOR METRIC` at all. There is no tip, cross-reference, or even a prose mention that a command exists to programmatically query which dimensions are safe for a given metric.
   - Dead end: A user whose goal is to discover safe dimension/metric combinations programmatically (e.g., for BI tool integration or dynamic UI building) reaches a dead end on this page. They would need to independently navigate to the reference section.
4. Alternative path: `docs/reference/show-semantic-dimensions.rst`
   - Found via the reference sidebar. The page ends with a tip: "To see only the dimensions that are safe to use with a specific metric (avoiding fan traps in multi-table views), use :ref:`SHOW SEMANTIC DIMENSIONS ... FOR METRIC <ref-show-dims-for-metric>` instead."
   - This tip provides the cross-reference to `show-semantic-dimensions-for-metric.rst`.
5. Navigated to: `docs/reference/show-semantic-dimensions-for-metric.rst`
   - Found: Complete and detailed reference page. The opening paragraph explains the statement's purpose -- filters out dimensions that would cause a fan trap -- and links to `howto-fan-traps` for background.
   - Fan trap filtering logic: documented with five rules (same table, many-to-one, one-to-many excluded, one-to-one, derived metrics). Clear.
   - Examples: single-table view (all dimensions returned), multi-table view with three-table schema showing which dimensions are excluded for `order_total` vs included for `line_item_sum`. The contrast between the two results demonstrates the filtering logic directly.
   - Filtering clauses: LIKE, STARTS WITH, LIMIT documented. Clause order enforced.
   - Type-alignment: Reference (information-oriented, work mode). Correct for a lookup use case.

**Friction:** The natural path from the fan-traps how-to to `SHOW SEMANTIC DIMENSIONS FOR METRIC` is broken. A user who reads `howto-fan-traps` will learn how to avoid fan traps structurally but will not discover the command to inspect safe combinations at runtime. The reference page (`show-semantic-dimensions-for-metric.rst`) is complete and links back to the how-to, but the forward direction -- from the how-to to the reference command -- is missing. A determined user who browses the reference sidebar would find the page, but there is no natural navigation path from the how-to guide.

### Gap Analysis

**Where:** `docs/how-to/fan-traps.rst` (no specific section -- the gap is the absence of a forward reference)
**What:** The fan-traps how-to explains cardinality-based detection and three fix strategies, but never mentions `SHOW SEMANTIC DIMENSIONS IN <view> FOR METRIC <metric>` -- the command that lets users programmatically inspect which dimensions are safe for a specific metric. This is the runtime complement to the structural prevention advice in the how-to. The `show-semantic-dimensions-for-metric.rst` reference page links back to this how-to, but the reverse direction is missing (Rule 4: code mention without cross-reference).
**Impact:** A data engineer building a BI integration who wants to dynamically enumerate safe dimension/metric combinations reaches a dead end on the fan-traps how-to. They would need to browse the reference section to discover `SHOW SEMANTIC DIMENSIONS FOR METRIC` exists.
**Suggested Fix:** In `docs/how-to/fan-traps.rst`, add a tip admonition after the "How to Fix Fan Trap Errors" section: "To inspect which dimensions are safe to combine with a specific metric without querying, use :ref:`SHOW SEMANTIC DIMENSIONS IN \<view\> FOR METRIC \<metric\> <ref-show-dims-for-metric>`. It applies the same cardinality analysis and returns only the dimensions that would not trigger a fan trap error."

---

## Revision Recommendations

### FAIL Issues (trigger revision)

None.

### PARTIAL Issues (for project author approval)

| Scenario | Page | Gap | Suggested Fix |
|----------|------|-----|---------------|
| S4 | `docs/tutorials/getting-started.rst` > "Define a Semantic View" section | `SHOW SEMANTIC VIEWS` used in code block with no `:ref:` link to its reference page; also omitted from "What You Learned" summary (Rule 4) | In the "Define a Semantic View" section, add a `:ref:` cross-reference on the prose introducing `SHOW SEMANTIC VIEWS`. Add `SHOW SEMANTIC VIEWS` to the "What You Learned" summary with a link to `ref-show-semantic-views`. |
| S4 | `docs/tutorials/multi-table.rst` > "Update the View" section | `ALTER SEMANTIC VIEW RENAME TO` not cross-referenced from any tutorial or how-to guide | After the CREATE OR REPLACE example, add a sentence: "To rename an existing view without redefining it, use :ref:`ALTER SEMANTIC VIEW RENAME TO <ref-alter-semantic-view>`." |
| S5 | `docs/how-to/fan-traps.rst` > (after "How to Fix Fan Trap Errors") | `SHOW SEMANTIC DIMENSIONS FOR METRIC` not mentioned in the fan-traps how-to, breaking the natural path from understanding fan traps to discovering the inspection command | Add a `.. tip::` admonition after the "How to Fix Fan Trap Errors" section referencing `SHOW SEMANTIC DIMENSIONS IN <view> FOR METRIC <metric>` with a `:ref:` link to `ref-show-dims-for-metric`. |
