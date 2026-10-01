# Persona Report

**Generated:** 2026-09-30
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 6 (S1-S5 reused from 2026-08-09; S6 added for v0.13.0)
**Results:** 4 PASS, 1 PARTIAL, 1 FAIL

## Summary

Since the previous run the site has closed most of its gaps. Grain now has its own
explanation page (`explanation/metric-grain.rst`), linked from the multi-table
tutorial, the fan-trap how-to and the explanation index. Pre-aggregation filtering
has a how-to (`how-to/filtering.rst`) with a clear "which filter" table and a worked
counter-example, and the getting-started tutorial now warns that the outer `WHERE`
runs after aggregation. `explain_semantic_view()` documents `where_clause`. The
`NON ADDITIVE BY` clause position is consistent across all pages, and the stale
define-time type-inference blocks are gone from the CREATE reference. As a result,
the star-schema, Snowflake/Databricks comparison and advanced-metrics journeys now
pass.

The one outright failure is the new v0.13.0 feature. Apart from a single bullet in the
release notes, the docs never mention `duckdb_functions()`. No page shows how to query
it, how to scope it to this extension, or what it returns. No page lists the full
set of functions the extension registers either. `list_semantic_views()` is used in
the GET_DDL reference but has no entry of its own. The table functions behind
SHOW/DESCRIBE, which the changelog says "remain available", are not documented
anywhere. The application-embedding journey (S4) is still PARTIAL, for two reasons.
The DuckDB + Iceberg + app stack is still a single tip with no links onward. And no
page says how to pass a user-chosen value into `where_clause` safely. The filtering
guide's tip actually encourages building the predicate string from request
parameters.

### Convergence vs previous run (2026-08-09)

| Scenario | Previous | Now | Change |
|----------|----------|-----|--------|
| S1 | PASS | PASS | unchanged |
| S2 | PARTIAL | PASS | improved (metric-grain explanation page added and cross-linked) |
| S3 | PASS | PASS | unchanged |
| S4 | PARTIAL | PARTIAL | partly improved (filtering how-to added; explain now documents `where_clause`); embedding story and safe parameterisation still missing |
| S5 | PARTIAL | PASS | improved (clause position unified; type-inference contradiction removed) |
| S6 | n/a | FAIL | new scenario |

---

## Scenario S1: Install the extension, define a first single-table semantic view, and query it three ways

**Verdict:** PASS. The tutorial covers install, DDL, all three query modes with expected output, and `explain_semantic_view()`, with no dead ends.

**Pages used:** `docs/index.rst`, `docs/tutorials/getting-started.rst`, `docs/reference/semantic-view-function.rst`, `docs/how-to/filtering.rst`

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: a clear semantic-layer framing that names Snowflake Semantic Views and Databricks Metric Views, and a "Getting started" card.
   - Followed: the "Getting started" card.
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: install steps in CLI and Python tabs, a realistic five-row `orders` table, the DDL, and a gloss of the `alias.name AS expression` pattern. That pattern is a `never_assume` item for this persona.
   - Found: all three modes (dimensions + metrics, dimensions only, metrics only), each with an expected result table, plus `explain_semantic_view()` and cleanup.
   - New since last run: a warning after the outer-`WHERE` example. It says the filter runs after aggregation and links to `howto-filtering` for `where_clause`. This closes the wrong-number trap flagged last time.
3. Followed: the `semantic_view()` link to the reference, to confirm the parameters. The four modes (including facts) are documented, with the generated SQL shape for each.

### Gap Analysis

None blocking. One minor item carried over: the tutorial still does not link to `explanation-sv-vs-views` ("how is this different from `CREATE VIEW`"). The homepage and the Explanation tab cover this well enough.

---

## Scenario S2: Model a star schema, declare relationships, query across tables, and understand grain safety

**Verdict:** PASS. Grain is now taught as a modelling concept on its own page, with a worked two-grain example and a list of refused shapes. One inconsistency in the "check before you query" tool is noted below; it does not block the goal.

**Pages used:** `docs/tutorials/multi-table.rst`, `docs/tutorials/building-a-model.rst`, `docs/explanation/metric-grain.rst`, `docs/how-to/fan-traps.rst`, `docs/reference/show-semantic-dimensions-for-metric.rst`, `docs/reference/create-semantic-view.rst`

### Navigation Path

1. Started at: `docs/index.rst`, then the "Multi-table semantic views" card.
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: a three-table schema with data, the DDL with `RELATIONSHIPS` highlighted, a plain-English gloss of `REFERENCES`, and a tip that `PRIMARY KEY` is semantic metadata rather than a constraint.
   - Found: a selective-join demo, with `explain_semantic_view()` used to prove that `products` is not joined.
   - New: the closing paragraph explains that every query in the tutorial was single-grain and links to `explanation-metric-grain`. This is exactly the signpost that was missing.
3. Followed: that link to `docs/explanation/metric-grain.rst`
   - Found: what grain is, and why a metric's grain belongs to its table. It gives the modelling rule "declare each metric on the table whose rows it aggregates".
   - Found: a worked `accounts` example with data and output, showing the West row with a `NULL` order count and explaining what `NULL` means ("no rows at this grain", not zero). It also shows the wrong number the old base-anchored path would have produced (1300.00 vs 800.00).
   - Found: the recombination shape (NULL-safe `FULL OUTER JOIN`, or `CROSS JOIN` when there are no dimensions), five refused shapes with reasons, and three modelling habits, the third of which is "check before you query" with `SHOW SEMANTIC DIMENSIONS ... FOR METRIC`.
   - Type alignment: this is a real explanation page, and it is what I needed while designing rather than debugging.
4. Followed: `howto-fan-traps` (from "Further Reading").
   - Found: the diagnostic view, with a worked blocked query, the error text, three fixes, per-grain shapes and "Other Shapes the Fence Rejects". It now points back to the explanation page for modelling.
5. Followed: the tip link to `docs/reference/show-semantic-dimensions-for-metric.rst`
   - Found: output columns, rules and worked examples for single-table, star, window and derived metrics.
   - Friction: the reference's "Fan Trap Filtering" rules seem out of step with the v0.12.0 grain rules documented on the metric-grain page, in two places:
     - **Derived metrics:** "A dimension is included if it is reachable from at least one of those source tables without fan-out." The metric-grain page says a dimension below *a* metric's grain is refused. For a derived metric that combines two grains (for example `avg_items AS item_count / order_count`), a line-item dimension is reachable from `line_items` but is below the `orders` grain. Under the SHOW rule it would be listed; under the grain rule the query would be refused.
     - **Window metrics:** "Fan trap checking is skipped for window function metrics. All dimensions reachable in the relationship graph are returned." But the fan-trap how-to and metric-grain page say that a window metric's inner aggregate is computed at its own grain and that below-grain dimensions are still refused.
   - The metric-grain page promises this command "applies the same grain rules at inspection time". Because of the two points above, I cannot fully trust it for derived and window metrics. For ordinary base metrics it is consistent.

### Gap Analysis (non-blocking)

**Where:** `docs/reference/show-semantic-dimensions-for-metric.rst`, section "Fan Trap Filtering" (the Derived metrics and Window metrics bullets)
**What:** The derived-metric rule ("reachable from at least one source table") and the window-metric rule ("fan trap checking is skipped") appear to contradict the per-grain refusal rules in `explanation/metric-grain.rst`, `how-to/fan-traps.rst` and the v0.12.0 release notes.
**Impact:** The page recommended as the pre-query safety check may list dimensions that the query then rejects, for exactly the metric shapes where grain matters most.
**Suggested Fix:** In `reference/show-semantic-dimensions-for-metric.rst`, section "Fan Trap Filtering": state the actual v0.12.0 rule for derived metrics (included only if at or above the grain of *every* component) and for window metrics (checked against the inner aggregate's grain). If the command really does behave as currently written, add a `.. warning::` saying it is more permissive than the query for these two shapes, and link `explanation-grain-refused`.

---

## Scenario S3: Compare feature-by-feature with Snowflake and Databricks to decide on adoption

**Verdict:** PASS. Both comparison pages give concept maps, side-by-side syntax, behavioural differences and explicit unsupported-feature tables with reasons.

**Pages used:** `docs/index.rst`, `docs/explanation/snowflake-comparison.rst`, `docs/explanation/databricks-comparison.rst`, `docs/reference/create-semantic-view.rst`

### Navigation Path

1. Started at: `docs/index.rst`, then the "Snowflake comparison" card.
2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: an upfront note that the comparison targets the Snowflake SQL DDL, not the Cortex Analyst YAML spec.
   - Found: a concept-mapping table, a syntax tab-set, a "syntax conveniences for porting" note, and key differences: explicit `PRIMARY KEY`, table function vs `SEMANTIC_VIEW()`/`AGG()`, expression scope, reported data types, metric grain, `USING`, facts mode, semi-additive direction and NULLs, materializations, transactional DDL, and `search_path` resolution with DuckDB case rules.
   - Found: a Feature Parity Notes table covering `where_clause` (supported), query-time scalar expressions (not yet, with a workaround), named filters (supported), and the out-of-scope and not-planned items with reasons.
3. Followed: the Explanation index to `docs/explanation/databricks-comparison.rst`
   - Found: a dated scope note ("as of August 2026"), concept mapping including `rely.at_most_one_match` vs inferred cardinality, a syntax comparison, two-way "features not in the other" tables, and an honest "Choosing Between Them" section.

### Gap Analysis

None blocking. Two minor items carried over:
- There is still no step-by-step checklist for porting a Snowflake view. The port steps have to be assembled from "Key Differences".
- `explanation/semantic-views-vs-regular-views.rst` § Trade-Offs still lists "window functions" as something only regular views can express. That is now misleading, because window metrics exist (`howto-window-metrics`).

---

## Scenario S4: Define views over Iceberg / Parquet / Postgres and serve filtered results from an application

**Verdict:** PARTIAL. Source connectivity and pre-aggregation filtering are now well covered. The app-server story is still spread across pages with no links between them, and nothing says how to pass a user-chosen value into `where_clause` safely.

**Pages used:** `docs/index.rst`, `docs/how-to/index.rst`, `docs/how-to/data-sources.rst`, `docs/how-to/filtering.rst`, `docs/reference/semantic-view-function.rst`, `docs/reference/explain-semantic-view-function.rst`, `docs/explanation/transactional-ddl-and-limitations.rst`

### Navigation Path

1. Started at: `docs/index.rst`, then the "How-to guides" card, then `how-to/index.rst`.
   - Found: "Data & Queries" now lists both `howto-data-sources` and `howto-filtering`.
2. Navigated to: `docs/how-to/data-sources.rst`
   - Found: Parquet, CSV and Iceberg (`iceberg_scan`, S3 credentials, metadata paths, VIEW vs TABLE freshness, schema evolution), Postgres via `ATTACH ... (TYPE POSTGRES)`, a mixed-source example, catalog-qualified names, and the diamond-path rule.
   - Friction: the Postgres section ends with "Alternatively, use the attached tables directly if DuckDB can resolve them". That is vague about whether `TABLES (o AS pg.public.orders ...)` works. The answer is in `transactional-ddl-and-limitations.rst` § Attached Databases, which says qualified references to attached databases are fine, but there is no link to it.
   - Friction: the "DuckDB + Iceberg + analytics application stack" tip is still three lines, with no links to the read-only bootstrap or single-catalog sections. Iceberg catalogs are covered only through `iceberg_scan` metadata paths.
3. Navigated to: `docs/how-to/filtering.rst`
   - Found: a "Which Filter to Use" table, a runnable example view, a `where_clause` date-range example with output, and a worked demonstration of why an outer `WHERE` gives the wrong answer. It also covers named filters, combining both filters, verifying with `explain_semantic_view()`, and troubleshooting. This is the how-to I needed last time, and it is good.
   - Friction (the app side): my application takes the date range from an HTTP request. The only guidance is a tip: "Application code that assembles a predicate from optional request parameters can therefore pass an empty string for 'no filter'." Nothing tells me whether `where_clause`, or the `dimensions`/`metrics` lists, can be supplied as a prepared-statement parameter (`?`/`$1`). Nothing tells me what happens if I pass unvalidated input, or how to quote literals safely beyond doubling single quotes. For a data engineer shipping an API, this is the key question, and the docs point towards string building.
4. Checked: `docs/reference/explain-semantic-view-function.rst`. `where_clause` is now in the syntax and parameter table, so I can inspect the exact filtered query my app will issue. (Fixed since last run.)
5. Navigated to: `docs/explanation/transactional-ddl-and-limitations.rst`. I had to find this from the Explanation index, because no link from data-sources leads here.
   - Found: where the definitions live (`semantic_layer._definitions` in the primary database), the bootstrap-then-reopen read-only Python workflow, the single-catalog `ATTACH`/`USE` rule, and handling for concurrent start-up races. This is the lifecycle content I need, but the page title ("Known Limitations") does not suggest it.

### Gap Analysis

**Where:** `docs/how-to/filtering.rst`, section "Verify Which Rows Were Aggregated" (the closing tip), and `docs/reference/semantic-view-function.rst`, section "Pre-aggregation -- `where_clause`"
**What:** Nothing says how to pass request-scoped values into `where_clause` safely: whether prepared-statement parameters can bind to it (or to the list parameters), and what the security implications of a string-built predicate are. The existing tip implicitly recommends building the predicate from request parameters.
**Impact:** The flagship use case (per-request filtering in an app) leads the reader towards string-built SQL, with no word on validation or binding. Users either guess or ship an injection-prone path.
**Suggested Fix:** In `how-to/filtering.rst`, add a short section, "Pass a request-scoped value from application code". Give a Python example and state plainly whether `where_clause := ?` binds. If it binds, show it with the date as a bound parameter. If it does not, show validating or whitelisting the value and using named filters (`LABELS = (FILTER)`) so user input never reaches the predicate text, and add a `.. warning::` about unvalidated input. Mirror one sentence in the `semantic_view()` reference.

**Where:** `docs/how-to/data-sources.rst`, section "Iceberg Tables" (closing tip) and section "Postgres via postgres_scanner"
**What:** The app-stack tip and the Postgres "use the attached tables directly" sentence do not link to the pages that answer the follow-up questions: `explanation-txn-ddl-readonly` (ship a database and reopen it read-only), `explanation-txn-ddl-attach` (qualified references to attached databases are fine; semantic-view DDL must run from the primary database), and `howto-filtering`.
**Impact:** The persona's headline stack needs three pages stitched together, and the most important one sits under a "Known Limitations" title that gives no hint it has deployment guidance.
**Suggested Fix:** In `how-to/data-sources.rst`, expand the Iceberg tip to cross-link `explanation-txn-ddl-readonly`, `explanation-txn-ddl-attach` and `howto-filtering`. Replace the Postgres "if DuckDB can resolve them" sentence with a concrete `TABLES (o AS pg.public.orders PRIMARY KEY (id))` example and a link to `explanation-txn-ddl-attach`. Longer term, add a short "How to embed semantic views in an application" guide.

---

## Scenario S5: Define semi-additive and window metrics for snapshot and time-series analysis

**Verdict:** PASS. Clause position is now unambiguous everywhere, and both how-tos give syntax, activation rules, restrictions, verification and troubleshooting. Minor inconsistencies are listed below.

**Pages used:** `docs/how-to/index.rst`, `docs/how-to/semi-additive-metrics.rst`, `docs/how-to/window-metrics.rst`, `docs/reference/create-semantic-view.rst`, `docs/reference/explain-semantic-view-function.rst`, `docs/reference/show-semantic-dimensions-for-metric.rst`, `docs/reference/error-messages.rst`

### Navigation Path

1. Started at: `docs/index.rst`, then "How-to guides", then the "Advanced Metrics" group.
2. Navigated to: `docs/how-to/semi-additive-metrics.rst`
   - Found: the snapshot problem with a worked double-counting example.
   - Found: a new "Clause Order: NON ADDITIVE BY Comes Before AS" section with the full metric grammar, the exact define-time error text, and a 0.12.0 note explaining that the after-`AS` form used to be silently accepted. This resolves last run's main S5 gap.
   - Found: sort and NULLS semantics, the 0.11.0 polarity-reversal warning with migration steps, active vs inactive behaviour, mixing restrictions, a sample `__sv_snapshot` CTE with an explanation of the reversed `ORDER BY`, and troubleshooting.
3. Cross-checked: `docs/reference/create-semantic-view.rst` § METRICS. The grammar, the prose "Clause order" paragraph and all examples agree (before `AS`). The two type-inference blocks are gone; each section now points to `explanation-sf-data-types`.
4. Navigated to: `docs/how-to/window-metrics.rst`
   - Found: `PARTITION BY` vs `PARTITION BY EXCLUDING` with worked partition-set examples, ORDER BY and NULLS, frames, `LAG`/`LEAD` extra arguments, required-dimension errors, the mixing restriction, and troubleshooting.

### Gap Analysis (non-blocking)

- `how-to/semi-additive-metrics.rst` § Verify the Generated SQL says "The `sql` column shows the generated query". The `explain_semantic_view()` reference says the output is one VARCHAR column named `explain_output`, organised as header, expanded SQL and plan sections.
- `how-to/window-metrics.rst` § Verify the Generated SQL describes the `__sv_agg` CTE in two bullets but shows no sample output, unlike the semi-additive page.
- `how-to/semi-additive-metrics.rst` § "Mixed regular and semi-additive metrics" says a co-queried `COUNT(*)` produces an error. The v0.12.0 material (`explanation/metric-grain.rst`, `how-to/fan-traps.rst`) says a latest-balance metric *can* be queried alongside an order count at another grain. The how-to does not say that its restriction applies only when both metrics are at the same grain.
- The SHOW ... FOR METRIC window-metric rule (see S2) also affects this scenario's "required dimensions" check.

---

## Scenario S6: Discover, over SQL, what functions the extension provides, their parameters, and how to call them (v0.13.0)

**Verdict:** FAIL. The `duckdb_functions()` metadata that v0.13.0 adds is mentioned only in one release-notes bullet. No page shows how to query it or what it returns, and no page lists the full set of functions the extension registers.

**Pages used:** `docs/index.rst`, `docs/reference/index.rst`, `docs/how-to/index.rst`, `docs/explanation/index.rst`, `docs/changelog.md` (Release notes, which renders the project changelog), `docs/reference/semantic-view-function.rst`, `docs/reference/explain-semantic-view-function.rst`, `docs/reference/get-ddl.rst`, `docs/reference/read-yaml-from-semantic-view.rst`, `docs/reference/show-semantic-views.rst`, `docs/reference/describe-semantic-view.rst`, `docs/how-to/metadata-annotations.rst`

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: no card or sentence about programmatic discovery, tool or agent integration, or function metadata.
   - Followed: the Reference tab.
2. Navigated to: `docs/reference/index.rst`
   - Found: DDL statements, two query functions (`semantic_view()`, `explain_semantic_view()`), GET_DDL and READ_YAML. There is no "Functions" or "Function catalog" entry and no mention of `duckdb_functions()`.
3. Navigated to: `docs/how-to/index.rst` and `docs/explanation/index.rst`
   - Found: nothing on introspecting the extension's functions. `howto-metadata-annotations` sounded relevant ("for discoverability"), but it covers COMMENT and SYNONYMS on view *members*, not the extension's own functions.
4. Navigated to: Release notes (`docs/changelog.md`, which renders the project changelog)
   - Found: under 0.13.0 "Added", the only mention on the site: "Every function the extension registers is documented in `duckdb_functions()`. Each now carries a description, real parameter names (`semantic_view(view_name, dimensions, …)` rather than `col0`), and a runnable example ... The table functions behind `SHOW` / `DESCRIBE SEMANTIC …` say so and point to the statement to use instead, and every table function notes that its `search_path` parameter is reserved."
   - Found: under 0.13.0 "Changed": `list_semantic_views()` and `describe_semantic_view()` "remain available". Neither has a reference page.
   - Type-alignment mismatch: this is a release note, not documentation of how to use the feature. It confirms the feature exists but does not show how to use it.
5. Tried to build the inventory from the reference pages instead.
   - `semantic-view-function.rst` and `explain-semantic-view-function.rst` give parameter tables. They correctly flag `search_path` as supplied automatically and not for hand use. Good.
   - `get-ddl.rst` and `read-yaml-from-semantic-view.rst` give scalar signatures.
   - `get-ddl.rst` § Examples uses `FROM list_semantic_views()` as a source, but that function has no reference entry, no column list and no link.
   - `show-semantic-views.rst` § Examples mentions "the underlying table function" without naming it.
   - Dead end: from the docs I cannot tell how many functions the extension registers, what they are called, which are for direct use, and which are the internal backings of SHOW/DESCRIBE statements.

### What I could not find

- The query to run. For example, `SELECT function_name, function_type, description, parameters, parameter_types, examples FROM duckdb_functions() WHERE ...`, with the `WHERE` condition that selects only this extension's functions. The docs do not say whether function names share a prefix, whether there is a column identifying the extension, or what to filter on.
- Which `duckdb_functions()` columns carry the description, parameter names and example, and what a row looks like for `semantic_view` (sample output).
- A complete function inventory: user-facing vs internal, table vs scalar, and for internal ones which statement to use instead. The release notes say this is in the metadata, but the docs do not reproduce it.
- Any guidance aimed at tool and agent builders, for example "use `SHOW`/`DESCRIBE` statements rather than the backing table functions; never pass `search_path`; use `SHOW COLUMNS IN SEMANTIC VIEW` or `DESCRIBE` to discover members, then call `semantic_view()`".

### Gap Analysis

**Where:** no page exists. The only mention is `docs/changelog.md` (Release notes, 0.13.0 § Added). Related dead end: `docs/reference/get-ddl.rst`, section "Dump and restore across schemas" (`list_semantic_views()` is used without a reference).
**What:** The v0.13.0 function-metadata feature is undocumented in the docs proper. There is no reference page listing the extension's functions, no example query against `duckdb_functions()`, no description of what the metadata contains, and no guidance on which functions are internal. `list_semantic_views()` and `describe_semantic_view()` are named ("remain available") but not documented.
**Impact:** A tool or agent builder cannot find out from the docs how to introspect the extension over SQL, which is the stated purpose of the release. A determined reader who knows DuckDB might guess at `duckdb_functions()` after reading the changelog, but would still not know how to filter to this extension or which functions not to call. Most of the done-when criteria are unmet.
**Suggested Fix:**
1. Add `docs/reference/functions.rst` ("Functions registered by the extension"), listed under a new "Functions and discovery" group in `reference/index.rst`. Include:
   - a table of every registered function: name, kind (table or scalar), purpose, user-facing or internal, and the statement to use instead for internal ones, linking the existing reference page where one exists;
   - a runnable `SELECT ... FROM duckdb_functions() WHERE ...` query with the correct filter for this extension, and sample output for the `semantic_view` row showing description, parameter names/types and example;
   - a note that `search_path` is reserved on every table function, linking `ref-sv-params`.
2. Give `list_semantic_views()` a short entry (columns, and "use as a `FROM` source; prefer `SHOW SEMANTIC VIEWS` interactively"), and link it from `get-ddl.rst` and `show-semantic-views.rst` § Examples, which currently says "the underlying table function" without naming it.
3. Optionally add a how-to, "How to let a tool or agent discover and query semantic views". Its steps would be: `duckdb_functions()` for the function surface, then `SHOW SEMANTIC VIEWS`, then `SHOW COLUMNS IN SEMANTIC VIEW` / `DESCRIBE`, then `SHOW SEMANTIC DIMENSIONS ... FOR METRIC`, then `semantic_view()`. Link it from the homepage's how-to card text.

---

## Additional Observations (not scenario-blocking)

- **Rule 2 (working examples):**
  - `reference/semantic-view-function.rst` § Examples still queries a view named `shop` with `where_clause := 'ordered_at >= ...'`, `facts := ['net_price']` and `dimensions := ['region']`. The tutorial's `shop` declares none of these members (carried over).
  - `reference/describe-semantic-view.rst`, second example: the output shows a `TABLE ... COMMENT = 'Order data'` row, but the DDL above it declares no table comment.
  - `tutorials/multi-table.rst` shows `date_trunc('month', o.ordered_at)` returning `2024-01-01`, while `tutorials/building-a-model.rst` shows the same expression over the same `DATE` column returning `2024-01-01 00:00:00`. At least one expected output is wrong.
  - `reference/read-yaml-from-semantic-view.rst` § Field Stripping still describes `column_type_names` / `column_types_inferred` as coming from "DDL-time type inference. Regenerated at import time". Other pages say v0.10.0 removed this.
  - The same page's `<view_name>` says the function "resolves the bare view name from the last component". That reads as ignoring the schema qualifier, which contradicts `get-ddl.rst` § "How an unqualified name resolves" ("READ_YAML_FROM_SEMANTIC_VIEW ... follows the same rule").
- **Accuracy:** `how-to/facts.rst` § Annotate Facts with Metadata says the four annotations, including `PRIVATE`/`PUBLIC`, "may appear in any order after the expression". `PRIVATE` goes before the alias, as the example and `metadata-annotations.rst` show.
- **Rule 4 (cross-references):**
  - `SHOW SEMANTIC FACTS` and `DESCRIBE SEMANTIC VIEW` in `how-to/facts.rst` § Annotate Facts are still plain inline code (carried over).
  - `list_semantic_views()` in `get-ddl.rst` has no link target (see S6).
  - `READ_YAML_FROM_SEMANTIC_VIEW` in `create-semantic-view.rst`'s schema note is unlinked, while GET_DDL next to it is linked.
- **Rule 1 (no internal details):** the "TECH-DEBT #51" references flagged last time are gone. Resolved.
- **Rule 5 (persona calibration):** strong throughout. The new metric-grain and filtering pages use grain, fan-out and cardinality naturally and define the extension-specific terms. The remaining `never_assume` shortfall is "modelling best practices": `metric-grain.rst` § "What This Means for a Model" now gives three habits, which helps.

---

## Revision Recommendations

### FAIL Issues (trigger revision)

| Scenario | Page | Gap | Suggested Fix |
|----------|------|-----|---------------|
| S6 | No page exists (only a mention in `changelog.md` 0.13.0 § Added); `reference/index.rst` | The v0.13.0 `duckdb_functions()` metadata is undocumented. There is no function inventory, no discovery query, no description of the metadata columns, and no user-facing vs internal guidance. `list_semantic_views()` / `describe_semantic_view()` are named but have no reference. | Add `reference/functions.rst` covering the full function inventory (kind, purpose, user-facing or internal, statement to use instead), a runnable filtered `duckdb_functions()` query with sample output for `semantic_view`, and the reserved `search_path` note. List it in `reference/index.rst`. Add a `list_semantic_views()` entry and link it from `get-ddl.rst` and `show-semantic-views.rst`. Optionally add an agent/tool discovery how-to. |

### PARTIAL Issues (for project author approval)

| Scenario | Page | Gap | Suggested Fix |
|----------|------|-----|---------------|
| S4 | `how-to/filtering.rst` (§ Verify Which Rows Were Aggregated, tip); `reference/semantic-view-function.rst` (§ Pre-aggregation) | Nothing says whether `where_clause` or the list parameters accept prepared-statement parameters, and there is no warning about building the predicate from request input. The tip implicitly encourages string building. | Add a "Pass a request-scoped value from application code" section: a Python example with a bound parameter if supported; otherwise validation plus named filters and a `.. warning::`. Mirror one sentence in the reference. |
| S4 | `how-to/data-sources.rst` (§ Iceberg Tables tip, § Postgres) | The app-stack tip and the Postgres "use attached tables directly if DuckDB can resolve them" do not link to the read-only bootstrap, single-catalog/ATTACH or filtering pages. | Cross-link `explanation-txn-ddl-readonly`, `explanation-txn-ddl-attach` and `howto-filtering`. Replace the vague Postgres sentence with a concrete `TABLES (o AS pg.public.orders ...)` example. Consider an embedding how-to. |
| S2 / S5 (non-blocking) | `reference/show-semantic-dimensions-for-metric.rst` (§ Fan Trap Filtering) | The derived-metric rule ("reachable from at least one source table") and the window-metric rule ("fan trap checking is skipped") contradict the v0.12.0 grain refusals, yet `metric-grain.rst` recommends this command as the pre-query check. | State the actual rule for derived and window metrics, or add a warning that the command is more permissive for those shapes, and link `explanation-grain-refused`. |
| S5 (non-blocking) | `how-to/semi-additive-metrics.rst` (§ Verify the Generated SQL, § Snapshot Behavior) | Refers to a `sql` column (the reference names it `explain_output`). The mixing restriction does not say it applies only at the same grain. | Say "the Expanded SQL section of `explain_output`". Add one sentence that metrics at a different grain are computed per grain (link `explanation-grain-multi`). |
| S5 (non-blocking) | `how-to/window-metrics.rst` (§ Verify the Generated SQL) | No sample expanded SQL, unlike the semi-additive page. | Add a short sample showing `__sv_agg` and the outer window `SELECT`. |
| S3 (non-blocking) | `explanation/semantic-views-vs-regular-views.rst` (§ Trade-Offs) | Lists "window functions" as regular-view-only, but window metrics exist. | Reword to "arbitrary window logic beyond metric-over-dimensions", and link `howto-window-metrics`. |
| (none) | `reference/semantic-view-function.rst` (§ Examples) | The `shop` examples name `ordered_at`, `net_price` and `region`, which the tutorial's `shop` does not declare (carried over). | Rename the example view or declare the members used. |
| (none) | `reference/read-yaml-from-semantic-view.rst` (§ Parameters, § Field Stripping) | Stale "DDL-time type inference" wording; the "resolves the bare view name from the last component" wording contradicts `get-ddl.rst`'s schema-aware resolution. | Update the stripping rows to reflect that no inference exists, and align the name-resolution text with `ref-get-ddl-resolution`. |
| (none) | `reference/describe-semantic-view.rst` (second example); `tutorials/multi-table.rst` vs `tutorials/building-a-model.rst` | The example output includes a table comment the DDL does not declare; the two tutorials show different output types for the same `date_trunc('month', DATE)` expression. | Add `COMMENT = 'Order data'` to the example DDL (or drop the row), and correct whichever tutorial output is wrong. |
| (none) | `how-to/facts.rst` (§ Annotate Facts with Metadata) | Says `PRIVATE`/`PUBLIC` may appear "in any order after the expression"; the two SHOW/DESCRIBE mentions are unlinked. | Say that `PRIVATE` precedes the alias, and link `ref-show-semantic-facts` and `ref-describe-semantic-view`. |
