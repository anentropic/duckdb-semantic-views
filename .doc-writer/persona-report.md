# Persona Report

**Generated:** 2026-08-09
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5
**Results:** 2 PASS, 3 PARTIAL, 0 FAIL

## Summary

This documentation set is unusually strong for a pre-1.0 extension. The Diataxis
split is real rather than cosmetic: the three tutorials form a genuine learning
ladder (single table → star schema → facts and derived metrics), the how-to
guides are goal-shaped with prerequisites and troubleshooting, and the Snowflake
and Databricks comparison pages are the best material on the site for this
persona -- they answer the "should I adopt this" question directly, with citations
back to Snowflake's own rules. Language is calibrated correctly: it uses grain,
fan-out and cardinality without apology, and it defines the extension-specific
terms (FACTS, derived metric, role-playing, USING) before using them.

The gaps are of three kinds. First, **internal contradictions in the reference
section**: `create-semantic-view.rst` still documents define-time type inference
that three other pages say was removed, it shows `NON ADDITIVE BY` in two
different clause positions within a single page, and two pages carry derived-metric
examples that the site's own validation rules say would be rejected. A reference
page that disagrees with itself is the one page a working engineer cannot route
around. Second, **grain is under-signposted**: per-grain multi-grain aggregation
is a headline v0.12.0 behaviour that changes returned numbers, yet it lives only
inside a page titled "How to Understand and Avoid Fan Traps" and in a
Snowflake-comparison subsection -- no tutorial or how-to introduces grain as a
modelling concept. Third, **the application-embedding story is assembled rather
than told**: pre-aggregation filtering (`where_clause`) exists only in the
`semantic_view()` reference page, and the "DuckDB + Iceberg + app" use case that
the homepage advertises gets one tip paragraph and no worked example.

No scenario failed outright. Every goal is reachable by a determined reader.

---

## Scenario S1: Install the extension, define a first single-table semantic view, and query it three ways

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: a clear framing of what a semantic layer is, name-checking Snowflake
     Semantic Views and Databricks Metric Views -- exactly the vocabulary I arrived with.
   - Found: a six-card grid; the first card is "Getting started -- Install the
     extension, create your first semantic view, and run a query in 5 minutes."
   - Followed: that card.
2. Navigated to: `docs/tutorials/getting-started.rst`
   - Found: time estimate, prerequisites, install via tab-set for both the DuckDB
     CLI (`INSTALL semantic_views FROM community; LOAD semantic_views;`) and Python.
   - Found: a five-row `orders` table with realistic values (regions, categories,
     amounts) -- no `foo`/`bar` placeholders.
   - Found: the DDL, followed immediately by an explanation of the
     `alias.name AS expression` pattern. This is the single most important thing to
     spell out for someone arriving from Snowflake, and it is spelled out.
   - Found: all three query modes, each with the exact expected result table. I could
     verify my own run against them without guessing.
   - Found: outer `WHERE` filtering, `explain_semantic_view()`, cleanup, and a
     "What You Learned" list where every bullet links to its reference page.
   - Followed: the closing link to the multi-table tutorial.

### Gap Analysis

None blocking. One observation carried forward rather than logged as a gap: the
tutorial never links to `explanation-sv-vs-views`, which is the page that answers
"how is this different from `CREATE VIEW`" -- a `never_assume` item for this
persona. The homepage covers the concept adequately and the Explanation tab is one
click away, so the goal is not hindered.

---

## Scenario S2: Model a star schema, declare relationships, query across tables, and understand grain safety

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst` → card "Multi-table semantic views".
2. Navigated to: `docs/tutorials/multi-table.rst`
   - Found: three-table e-commerce schema with data, then the full DDL with
     `RELATIONSHIPS` lines emphasised.
   - Found: a plain-English gloss of `order_customer AS o(customer_id) REFERENCES c`.
     This is precisely the "relationship modelling" item on my never-assume list, and it
     is handled well.
   - Found: a tip clarifying that `PRIMARY KEY` here is semantic metadata, not a DuckDB
     constraint. Important, and easy to get wrong coming from Snowflake.
   - Found: the selective-join demonstration (query one dimension table, then two) plus
     `explain_semantic_view()` to verify which tables were joined.
   - Friction: the tutorial never uses the word *grain*, and never shows a metric on a
     table other than the fact table. Everything here is single-grain by construction.
3. Followed: "Next" links to `docs/tutorials/building-a-model.rst`
   - Found: an excellent refactoring narrative -- start with duplicated row-level
     arithmetic in metrics, extract to `FACTS`, compose `profit` and `margin` as derived
     metrics, then inspect the expansion. This is the modelling-best-practices content the
     persona needs and it is genuinely good.
   - Friction: still single-grain. All metrics sit on `line_items`.
4. Followed: `how-to/index.rst` → `howto-fan-traps` (the only entry that sounds like
   it addresses "will my numbers be right").
5. Navigated to: `docs/how-to/fan-traps.rst`
   - Found: a clear definition, the cardinality inference rules, a worked blocked
     query with the verbatim error text, three fixes, and a tip pointing at
     `SHOW SEMANTIC DIMENSIONS ... FOR METRIC` for checking before writing a query.
     That tip is the best single piece of practical guidance on the site.
   - Found: a section `howto-fan-per-grain` describing the v0.12.0 per-grain
     computation -- metrics on a parent table, metrics at two grains queried together,
     chasm traps, derived metrics fusing grains, and the NULL-safe `FULL OUTER JOIN`
     semantics for dimension groups present at one grain but not another.
   - Friction (type-alignment): I needed *explanation* (how does this system think about
     grain, and what will it do with my model) and *how-to* (how do I model an
     orders/line-items view so both metrics are queryable). I got troubleshooting content
     on a page framed entirely around an error I have not hit yet. I only found it because
     I went looking for correctness guarantees; a reader modelling a schema would not
     click "How to Understand and Avoid Fan Traps" until something broke.
6. Cross-checked: `docs/explanation/snowflake-comparison.rst` § "Metric Grain"
   - Found: the same behaviour explained again, better, with the Snowflake rule quoted.
     But it is inside the Snowflake comparison page, reachable only by someone doing a
     platform comparison.

**Gap Analysis**

**Where:** `docs/tutorials/multi-table.rst`, `docs/tutorials/building-a-model.rst`,
`docs/how-to/index.rst`, `docs/explanation/index.rst` (no page exists for grain)
**What:** Grain -- the concept that each metric is computed at the grain of its own
logical table, that multi-grain queries are answered per-grain and joined, and that a
dimension below a metric's grain is rejected -- is never introduced as a modelling
concept. It is documented only as (a) a subsection of a fan-trap troubleshooting
how-to and (b) a subsection of the Snowflake comparison. The how-to index bullet for
fan traps ("Understand, detect, and resolve fan traps that inflate aggregation
results") gives no hint that multi-grain querying is covered there.
**Impact:** Per-grain aggregation changes what numbers come back -- a dimension group
present at one grain and absent at another now yields a row with a `NULL` metric
rather than being dropped. A data engineer modelling orders + line_items + shipments
will hit this behaviour without having read anything that prepared them for it, and
has no page to consult while designing the model rather than while debugging it.
**Suggested Fix:** In `docs/explanation/`, add a short "Metric grain and how queries
are assembled" page (or promote the existing `howto-fan-per-grain` and Snowflake
§ "Metric Grain" text into one), covering: each metric aggregates over its own table;
single-grain queries emit one base-anchored SELECT; multi-grain queries emit one CTE
per grain joined on the shared dimensions with `FULL OUTER JOIN`; a dimension below a
metric's grain is rejected. Link it from `docs/explanation/index.rst`, from the
"What You Learned" section of `tutorials/multi-table.rst`, and from the top of
`how-to/fan-traps.rst`. Also extend the `how-to/index.rst` bullet for fan traps to
mention multi-grain queries so it is findable before an error is hit.

---

## Scenario S3: Compare feature-by-feature with Snowflake and Databricks to decide on adoption

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: the "Snowflake comparison" card in the second grid; also the Explanation tab.
   - Followed: the card.
2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Found: an upfront note that Snowflake has two interfaces and that this page targets
     the SQL DDL only, not the Cortex Analyst YAML spec. This is exactly the disambiguation
     I needed and I would not have thought to ask for it.
   - Found: a 20-row concept-mapping table, a Snowflake-vs-DuckDB syntax tab-set, a
     "Syntax conveniences for porting" note listing every Snowflake spelling now accepted
     (optional table alias, trailing `COMMENT =`, `PUBLIC` on dimensions, `WITH SYNONYMS`
     without `=`, `DESC SEMANTIC VIEW`).
   - Found: the differences that actually matter for a port -- explicit `PRIMARY KEY`
     required (with the v0.10.0 breaking change and its migration instruction), table
     function instead of direct SQL, expression scoping rules citing Snowflake's own
     validation-rules page, grain behaviour, schema/search_path resolution, and
     case-insensitivity following DuckDB rather than Snowflake.
   - Found: a "Feature Parity Notes" table that states plainly what is supported,
     out of scope, and not planned, with reasons. `where_clause` and named filters are
     both marked supported here.
3. Followed: `explanation/index.rst` → `explanation-databricks`.
4. Navigated to: `docs/explanation/databricks-comparison.rst`
   - Found: concept mapping, syntax tab-set, the `MEASURES` vs `METRICS` note, a
     "features here not there" table and a "features there not here" table (Unity Catalog,
     row-level security, AI/BI, Delta materialized views), and a "Choosing Between Them"
     section that is honest rather than promotional.
   - Found: a dated-scope caveat ("as of early 2026, may vary by runtime"), which I trust
     more than an undated claim.

### Gap Analysis

None blocking. Minor: there is no single three-way table and no step-by-step port
checklist; I had to synthesise the porting steps from the "Key Differences" prose and
the conveniences note. This did not prevent the goal.

---

## Scenario S4: Define views over Iceberg / Parquet / Postgres and serve filtered results from an application

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst`
   - Found: the animated subtitle cycles "Iceberg tables / CSV files / Ducklake /
     dataframes", so I expect first-class source coverage.
   - Followed: "How-to guides" card → `how-to/index.rst` → `howto-data-sources`.
2. Navigated to: `docs/how-to/data-sources.rst`
   - Found: Parquet (both `CREATE TABLE AS` and `CREATE VIEW` forms), CSV, Iceberg with
     `iceberg_scan`, S3 credential settings, a note on catalog-managed metadata paths, a
     `VIEW`-vs-`TABLE` trade-off for snapshot freshness, schema-evolution guidance,
     Postgres via `ATTACH ... (TYPE POSTGRES)`, a mixed-source example, and
     catalog-qualified table names.
   - Found: a v0.11.0 note that diamond join paths are now rejected at CREATE time,
     with role-playing explicitly carved out. Useful and well placed.
   - Friction: the "DuckDB + Iceberg + analytics application stack" -- the persona's
     stated end goal and the homepage's headline framing -- is a single `.. tip::`
     paragraph. No worked example of the application side.
3. Looked for per-request filtering. My app needs "revenue by region for a user-chosen
   date range", which must filter *before* aggregation.
   - `tutorials/getting-started.rst` § Filtering: shows only outer `WHERE` on the result.
     No mention of pre-aggregation filtering, no link onward.
   - `how-to/index.rst`: no entry for filtering. Twelve guides, none about `WHERE`.
   - Followed the inline `semantic_view()` link that appears on nearly every page.
4. Navigated to: `docs/reference/semantic-view-function.rst`
   - Found: `where_clause := '<predicate>'`, fully and clearly documented -- what it may
     name, why it is not spelled `where`, that predicate members go through the same
     reachability and fan-out checks, and exactly where it is injected on each emission
     path. This is excellent reference writing; I simply had no route to it except luck.
   - Friction (type-alignment): I was in work mode looking for a how-to ("how do I filter
     a semantic view query"). The only answer lives in a reference page, reachable only via
     an inline symbol link.
5. Checked whether I can debug a filtered query: `docs/reference/explain-semantic-view-function.rst`
   - Friction: the syntax block lists only `dimensions` and `metrics`. `where_clause` is
     absent, and the page does not say whether it is unsupported. So I cannot tell whether
     I am able to inspect the SQL for the exact query my application will run.
6. Followed the README/`explanation` route for lifecycle: `explanation-transactional-ddl`
   - Found: the read-only bootstrap-then-reopen Python workflow, the single-catalog
     `ATTACH`/`USE` rule, where definitions live (`semantic_layer._definitions` in the
     primary database), and a Python snippet for handling concurrent bootstrap in
     multi-worker container start-up. This is genuinely the app-server content I needed,
     but it is on a page called "Transactional DDL and Known Limitations" -- I would not have
     opened it if the README had not pointed at it.

### Gap Analysis

**Where:** `docs/how-to/index.rst` (no page exists), `docs/tutorials/getting-started.rst`
§ "Query the Semantic View" (the filtering paragraph)
**What:** Pre-aggregation filtering via `where_clause` is documented only in the
`semantic_view()` reference. There is no how-to guide for filtering, no entry in the
how-to index, and the tutorial's filtering example shows the outer `WHERE` without
noting that it cannot filter the rows a metric aggregates over.
**Impact:** An engineer building request-scoped queries will reach for the outer `WHERE`,
which silently gives a different (post-aggregation) answer for anything date- or
segment-scoped. The failure mode is a wrong number, not an error.
**Suggested Fix:** In `docs/how-to/`, add a "How to filter semantic view queries" guide
covering the two filters and when each applies, with a worked date-range example; list it
under "Data & Queries" in `how-to/index.rst`. In `tutorials/getting-started.rst`, after the
outer-`WHERE` example, add one sentence plus a `:ref:` to
`ref-sv-pre-agg-filtering` noting that filtering the rows *behind* a metric requires
`where_clause`.

**Where:** `docs/reference/explain-semantic-view-function.rst` § "Syntax" / "Parameters"
**What:** `where_clause` is not listed and its absence is not explained, unlike the
`facts` parameter which has an explicit note saying it is unsupported.
**Impact:** I cannot determine whether the filtered query my application issues can be
inspected before it ships. Verifying generated SQL is the main reason I use this function.
**Suggested Fix:** Add `where_clause` to the syntax block and parameter table if it is
accepted; if it is not, add a note in the same style as the existing `facts` note stating
so and pointing at `ref-sv-pre-agg-filtering`.

**Where:** `docs/how-to/data-sources.rst` § "Iceberg Tables" (the closing tip)
**What:** The DuckDB + Iceberg + application-server stack that the homepage headlines gets
a three-line tip. There is no end-to-end example showing an application opening the
database, loading the extension, and fetching `semantic_view()` results; the pieces
(read-only bootstrap, single-catalog rule, view-vs-table freshness trade-off) are spread
across `data-sources` and `transactional-ddl-and-limitations`.
**Impact:** The persona's flagship use case requires stitching three pages together and
knowing to open a page titled "Known Limitations" to find the deployment pattern.
**Suggested Fix:** Either expand the tip into a short "How to embed semantic views in an
application" guide (define views once at build time, ship the database, reopen read-only,
query from Python, refresh Iceberg snapshots), or at minimum add cross-links from the
Iceberg tip to `explanation-txn-ddl-readonly` and `explanation-txn-ddl-attach`.

---

## Scenario S5: Define semi-additive and window metrics for snapshot and time-series analysis

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst` → "How-to guides" card → `how-to/index.rst`.
   - Found: an "Advanced Metrics" group with exactly the two guides I want.
2. Navigated to: `docs/how-to/semi-additive-metrics.rst`
   - Found: the problem stated with a concrete balance table and the double-counting
     arithmetic worked through (ACME 500 + 550 = 1050, wrong; 550, right). Ideal framing
     for this persona.
   - Found: the `NON ADDITIVE BY` definition, sort order and NULLS placement rules, the
     active/inactive behaviour split, the mixed-metric decomposition restrictions, the
     generated `__sv_snapshot` CTE with an explanation of why the emitted direction is the
     reverse of the declared one, and a troubleshooting section.
   - Found: a v0.11.0 `versionchanged` warning that the polarity was reversed, with
     explicit per-case migration instructions. Exactly what an upgrader needs.
   - **Friction:** every example here writes the clause *after* the expression --
     `a.total_balance AS SUM(a.balance) NON ADDITIVE BY (report_date)`.
3. Cross-checked against `docs/reference/create-semantic-view.rst`
   - The grammar block puts it **before** `AS`:
     `<alias>.<metric_name> [USING (...)] [NON ADDITIVE BY (...)] AS <aggregate_expression>`.
   - But the same page's METRICS section and Examples section put it **after**
     `AS SUM(a.balance)`.
   - `how-to/materializations.rst` and `explanation/snowflake-comparison.rst` both put it
     **before** `AS`.
   - Friction: I cannot tell from the documentation which position is correct, or whether
     both are accepted. I would write one, and if the parser rejected it, try the other.
4. Navigated to: `docs/how-to/window-metrics.rst`
   - Found: `PARTITION BY` vs `PARTITION BY EXCLUDING` contrasted with a worked example of
     how the partition set is computed at query time, ORDER BY / NULLS rules, frame clauses,
     extra arguments for `LAG`/`LEAD`, the required-dimension errors verbatim, the
     window-vs-aggregate mixing restriction, the `__sv_agg` CTE structure, and nine
     troubleshooting entries. No complaints -- this page is exemplary.
5. Went to inspect my model: `docs/reference/show-semantic-metrics.rst` /
   `docs/reference/show-semantic-dimensions.rst`
   - Found: `data_type` documented as "the **declared** output type. Empty string unless the
     definition declares one ... nothing infers a type", with the sample output showing an
     empty column and a pointer to the Snowflake comparison for the rationale.
   - Friction: this directly contradicts `create-semantic-view.rst`, which has two
     "Type inference" blocks stating that on a file-backed database the extension runs a
     `LIMIT 0` probe at define time to infer `DATA_TYPE` for dimensions and metrics, and that
     the result is visible in `SHOW SEMANTIC DIMENSIONS` / `SHOW SEMANTIC METRICS`.

### Gap Analysis

**Where:** `docs/reference/create-semantic-view.rst` § "Syntax" grammar block vs the same
page's § METRICS and § Examples; also `how-to/semi-additive-metrics.rst`,
`how-to/materializations.rst`, `explanation/snowflake-comparison.rst`
**What:** `NON ADDITIVE BY` appears in two different clause positions -- before `AS` in the
formal grammar, the materializations guide and the Snowflake comparison; after the
aggregate expression in the reference's own examples and throughout the semi-additive
how-to. Nothing states that both are accepted.
**Impact:** The formal grammar is the one artefact a reader consults when an example does
not parse. Here the grammar and the examples on the same page disagree, so neither can
resolve the other. Trial-and-error is the only route.
**Suggested Fix:** In `create-semantic-view.rst` § METRICS, state explicitly which
position(s) the parser accepts. If both are valid, say so in one sentence and make the
grammar block show the canonical one with the alternative noted; if only one is, correct
every example on the four affected pages to match.

**Where:** `docs/reference/create-semantic-view.rst` § DIMENSIONS "Type inference" and
§ FROM YAML "Type inference"
**What:** Both blocks describe define-time `LIMIT 0` type inference populating `DATA_TYPE`
on file-backed databases. Three other pages state the opposite:
`explanation/snowflake-comparison.rst` § "Reported Data Types" ("There is no type
inference: `CREATE` no longer probes the underlying tables (v0.10.0 removed the `typeof`
pass)"), `reference/show-semantic-dimensions.rst`, `reference/show-columns-semantic-view.rst`
("nothing infers a type"), and `reference/semantic-view-function.rst` ("there is no
`CREATE`-time type cache to fall back on").
**Impact:** A reader who builds tooling around `SHOW SEMANTIC METRICS`/`DIMENSIONS` on the
strength of the CREATE reference will get empty `data_type` columns and conclude the
extension is broken. This is stale content describing behaviour removed in v0.10.0.
**Suggested Fix:** Delete both "Type inference" blocks from `create-semantic-view.rst` and
replace with a one-line pointer to `explanation-sf-data-types`, matching the wording
already used on the SHOW pages.

**Where:** `docs/reference/show-columns-semantic-view.rst` § Examples;
`docs/how-to/facts.rst` § "Annotate Facts with Metadata"
**What:** Both pages define a derived metric containing an aggregate function --
`avg_order AS revenue / COUNT(*)` and `profit_margin AS total_net - SUM(li.raw_margin)`.
Neither name carries a table alias, so both are derived metrics, and
`create-semantic-view.rst` § METRICS validation rules plus `how-to/derived-metrics.rst`
§ Troubleshooting both state that derived metrics must not contain aggregate functions
and are rejected at define time.
**Impact:** Copy-pasting either example produces a define-time error, and the reader is
left unsure whether the rule or the example is wrong -- undermining trust in the examples
generally.
**Suggested Fix:** Rewrite both to conform to the rule, e.g. give the aggregate a base
metric of its own (`o.order_count AS COUNT(*)`, then `avg_order AS revenue / order_count`)
and in `facts.rst` make `profit_margin` reference a base metric over the private fact.

---

## Additional Observations (not scenario-blocking)

- **Rule 1 (no internal details):** `reference/show-semantic-dimensions.rst` and
  `explanation/snowflake-comparison.rst` both cite "TECH-DEBT #51" as the tracker for the
  unpopulated `data_type` column. A maintainer-side issue identifier is not actionable
  for an end user; the sentence works without it. (The `__sv_agg` / `__sv_snapshot` /
  `__sv_rn` names are fine by contrast -- users see those in `explain_semantic_view()`
  output.)
- **Rule 2 (working examples):** `reference/semantic-view-function.rst` § Examples uses a
  view named `shop` with `where_clause := 'ordered_at >= DATE ''2024-01-01'''`, but `shop`
  is the multi-table tutorial's view, which declares `month` (a `date_trunc` of
  `ordered_at`) and no `ordered_at` dimension. Per the same page's rule that the predicate
  may name only declared dimensions and facts, that example would not resolve against the
  tutorial's `shop`. The same Examples block also mixes in `net_price` and `region`, which
  belong to other pages' views. Worth either renaming the view or declaring the members
  used.
- **Rule 4 (cross-reference code mentions):** generally well observed -- `semantic_view()`,
  `explain_semantic_view()` and every DDL verb are consistently linked. A few inline
  mentions dead-end, e.g. `SHOW SEMANTIC FACTS` and `DESCRIBE SEMANTIC VIEW` in
  `how-to/facts.rst` § "Annotate Facts with Metadata" are plain inline code while the same
  statements are linked elsewhere.
- **Rule 5 (persona-calibrated language):** strong throughout. Every `never_assume` item is
  addressed somewhere: semantic-vs-regular views has its own explanation page, DDL syntax is
  introduced clause by clause, relationship modelling is glossed in plain English,
  Snowflake/Databricks differences have dedicated pages. The one under-served item is
  "modelling best practices (dos/don'ts)" -- best practice appears as scattered tips rather
  than as guidance a reader can seek out, and grain (see S2) is the biggest instance.

---

## Revision Recommendations

### FAIL Issues (trigger revision)

None. No scenario failed.

### PARTIAL Issues (for project author approval)

| Scenario | Page | Gap | Suggested Fix |
|----------|------|-----|---------------|
| S5 | `reference/create-semantic-view.rst` (§ DIMENSIONS, § FROM YAML) | Two "Type inference" blocks describe define-time `LIMIT 0` probing removed in v0.10.0; contradicted by three other pages | Delete both blocks; replace with a pointer to `explanation-sf-data-types`, matching the SHOW pages' wording |
| S5 | `reference/create-semantic-view.rst` (§ Syntax vs § METRICS/§ Examples) | `NON ADDITIVE BY` shown before `AS` in the grammar, after the expression in the examples; four pages disagree | State explicitly which position(s) parse; make grammar and all examples agree |
| S5 | `reference/show-columns-semantic-view.rst` (§ Examples), `how-to/facts.rst` (§ Annotate Facts) | Derived metrics `avg_order AS revenue / COUNT(*)` and `profit_margin AS total_net - SUM(li.raw_margin)` contain aggregates, which the site's own validation rules say are rejected | Rewrite both to reference a base metric instead of aggregating inline |
| S2 | No page exists (grain) | Per-grain multi-grain aggregation documented only inside `how-to/fan-traps.rst` and `explanation/snowflake-comparison.rst` § Metric Grain; no tutorial or explanation introduces grain as a modelling concept | Add an explanation page on metric grain and query assembly; link from `explanation/index.rst`, the multi-table tutorial's closing links, and the top of `fan-traps.rst`; widen the how-to index bullet to mention multi-grain queries |
| S4 | No page exists (filtering); `tutorials/getting-started.rst` § Query the Semantic View | `where_clause` (pre-aggregation filtering) lives only in the `semantic_view()` reference; no how-to, no index entry, tutorial shows only the outer `WHERE` | Add a "How to filter semantic view queries" guide under "Data & Queries"; add one sentence plus a `:ref:` to `ref-sv-pre-agg-filtering` after the tutorial's outer-`WHERE` example |
| S4 | `reference/explain-semantic-view-function.rst` (§ Syntax, § Parameters) | `where_clause` neither listed nor declared unsupported, unlike the `facts` parameter which has an explicit note | Add it to the syntax block and parameter table, or add a note in the same style as the `facts` note |
| S4 | `how-to/data-sources.rst` (§ Iceberg Tables, closing tip) | The DuckDB + Iceberg + application stack headlined on the homepage is one tip; deployment pattern is buried in `transactional-ddl-and-limitations.rst` | Expand into a short embedding guide, or at minimum cross-link the tip to `explanation-txn-ddl-readonly` and `explanation-txn-ddl-attach` |
| — | `reference/semantic-view-function.rst` (§ Examples) | Example view `shop` is the multi-table tutorial's view but the `where_clause` example names `ordered_at`, which that view does not declare | Rename the example view or declare the members the examples use |
| — | `reference/show-semantic-dimensions.rst`, `explanation/snowflake-comparison.rst` (§ Reported Data Types) | "TECH-DEBT #51" is a maintainer-side tracker reference in end-user documentation | Drop the identifier; the surrounding sentence stands without it |
