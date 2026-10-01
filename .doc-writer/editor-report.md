# Editor Report

**Generated:** 2026-10-01, working tree on `docs/v0.13.0-audit-fixes` (uncommitted Author revisions + Editor pass)
**Mode:** edit. All four passes were run on the 43 revised pages. The terminology pass also scanned `tutorials/index.rst` and `explanation/index.rst`. Example SQL, outputs, error texts and behavioral claims were **not** edited. Where one looks wrong, it is reported below. The one exception: an example table name was renamed consistently in both the DDL and its output (see `describe-semantic-view.rst`).
**Files reviewed:** 45 (`docs/index.rst`, 43 pages under `tutorials/`, `how-to/`, `explanation/`, `reference/` including the new `reference/functions.rst`, plus `CHANGELOG.md`)
**Changes made:** 34 fixed findings (45 findings in total; 11 left open for the Author)
  - BLOCKING: 7 (5 fixed, all added cross-reference links; 2 open)
  - SUGGESTION: 22 (15 fixed; 7 open)
  - NITPICK: 16 (14 fixed; 2 open)

**Build:** `sphinx-build -b html -W -E docs` passes after the edits (clean environment, output written to `$TMPDIR`, `docs/_build` untouched). No `:ref:` label was added, removed or renamed by the Editor. The Author-side label changes (for example, `howto-ds-ambiguous-paths` removed) leave no dangling references.

## Summary

The three Authors resolved almost every row of the 2026-09-30 audit. That includes all 36 correctness findings except the ones covered by the project author's standing decisions. Cross-page consistency is now good. The `FROM describe_semantic_view()` / `FROM list_semantic_views()` idiom is explained the same way on four pages, `revenue_by_region` is the materialization table everywhere, and all 13 how-tos share one layout. The Editor's work was mostly terminology normalization, two Troubleshooting sections brought into line, internal jargon ("the fence") removed, and five missing links added. Two remaining factual problems need an Author: `metadata-annotations.rst` says dimensions take no access modifier, and one error-catalogue template is incomplete.

---

## Status of the 2026-09-30 report

**Resolved (by the Authors unless marked):**

| Area | Rows | Status |
|------|------|--------|
| Cross-cutting | function-discovery gap; how-to structure (Rule 3); reference parameter layout; spelling; Rule 1 internals; 39 em dashes; "worth knowing" ×4 | **All resolved.** `reference/functions.rst` added, and `index.rst` points to it. All 13 how-tos now have Prerequisites, Troubleshooting and Related. The Editor converted the two Troubleshooting sections that still used plain paragraphs. The parameter layout is now a consistent split: statements use definition lists, functions use a Parameter / Type / Description list-table (recorded in `terminology.yaml`). There are 0 em dashes and 0 British spellings; the Editor fixed the last one, "labelled". The only British spelling left is the label `explanation-grain-modelling`, which was kept to avoid breaking references. `__sv_agg` now appears only in explain output, and the `SemanticViewDefinition` / serde / connection wording is gone. |
| `index.rst` | grammar; `Ducklake`; Databricks URL | Resolved. |
| Tutorials | `SHOW SEMANTIC VIEWS` columns / timestamp; missing `ORDER BY`; `month` TIMESTAMP; "core value" phrasing | Resolved. |
| `data-sources.rst` | S3 credential chain; Postgres attached tables; diamond section drift; heading | Resolved. The diamond rule is now a Troubleshooting entry linking `howto-rp-diamond`. |
| `derived-metrics.rst` | unknown-name troubleshooting | Resolved. |
| `facts.rst` | Pass 4 links; intro annotation list | Resolved. The aggregate-in-FACTS text is **kept by project decision (2)**. |
| `fan-traps.rst` | type blur in Multi-Grain section; wrong background link; truncated error; grammar / `...` fragment | Resolved. |
| `materializations.rst` | `FROM (DESCRIBE …)` parser error; missing `;`; `daily_revenue_by_region` name; unqualified `FROM` in sample | Resolved. |
| `metadata-annotations.rst` | private items in SHOW; `GET_DDL` link; private reference scope; filter scope wording; merged sample row | Resolved. A new, related issue is listed below as B-open-1. |
| `query-facts.rst`, `role-playing-dimensions.rst`, `filtering.rst` | em dash / none / none | Resolved or nothing to do. |
| `semi-additive-metrics.rst` | `explain_output` column; `GET_DDL` link; blur (moved to a dropdown); `important::`; heading case; bold label case | Resolved. |
| `window-metrics.rst` | pseudo-notation block; `LAG` naming; abbreviated errors | Resolved. |
| `wildcard-selection.rst` | derived metric in `o.*`; `status` dimension; hedging | Resolved. |
| `yaml-definitions.rst` | `SHOW SEMANTIC VIEWS` link; missing `;` | Resolved. |
| `semantic-views-vs-regular-views.rst`, `metric-grain.rst`, `databricks-comparison.rst` | all rows | Resolved. |
| `snowflake-comparison.rst` | PK/`REFERENCES` rule; invented error text; Snowflake DDL transactions; Pass 4 links; `access_modifier`; project name; implicit `Metric Grain`_ target | Resolved. |
| `transactional-ddl-and-limitations.rst` | read-only error text; Pass 4 links; roadmap promise; PEG restore wording | Resolved. The type-blur row is **still open** (S-open-4). |
| `create-semantic-view.rst` | diamond rule; Pass 4 links; grammar missing `COMMENT` / `PUBLIC`; "before that change"; unused `is_international` | Resolved. The aggregate-in-FACTS text is **kept by decision (2)**. |
| `alter-` / `drop-semantic-view.rst` | promised ambiguity error; wrong error text | Resolved (`search_path` rule, **decision (3)**). |
| `describe-semantic-view.rst` | `FROM (DESCRIBE …)` failures; table-comment row | Resolved via option (a): `describe_semantic_view()` is documented as a FROM source (**decision (1)**). |
| `show-semantic-views.rst` | `FROM (SHOW …)`; `created_on` format | Resolved. |
| `show-semantic-dimensions` / `-metrics` / `-facts.rst` | `synonyms` shows `[]`; `GET_DDL` links | Resolved. |
| `show-semantic-dimensions-for-metric.rst` | `GET_DDL` link; undefined `filter_sv`; window-metric scoping; truncated tip error | Resolved. |
| `show-columns-semantic-view.rst` | `GET_DDL` link | Resolved. |
| `get-ddl.rst` | `<name>` → `object_name`; Pass 4 links; unexplained `list_semantic_views()` | Resolved (**decision (4)**). |
| `read-yaml-from-semantic-view.rst` | field stripping; resolution rule; sample output / `output_type: null` | Resolved. |
| `yaml-format.rst` | complete example does not import; required keys; `is_filter`; Pass 4 links; Rule 1 intro | Resolved. |
| `semantic-view-function.rst`, `explain-semantic-view-function.rst` | undefined members in examples; raw-column rule; `SUM` casing | Resolved. |
| `error-messages.rst` | wrong message texts ×5; missing entries; Pass 4 links | Resolved. The aggregate-in-FACTS entry is **kept by decision (2)**. |
| `CHANGELOG.md` | 0.12.0 "which only a YAML definition can do" | Resolved. |

**Still open from the previous report:** `CHANGELOG.md` tracker IDs, the 0.10.0 `REFERENCES target(cols)` shorthand claim, subsection order, and the `[0.5.3]` link (S-open-6, S-open-7, N-open-2), plus the procedure blur in `transactional-ddl-and-limitations.rst` (S-open-4). The source-side observations (§ "Source-side observations" of the old report) are code issues and are outside this pass.

---

## Cross-page consistency check (requested focus)

| Check | Result |
|-------|--------|
| FROM-source idiom | `functions.rst`, `describe-semantic-view.rst` and `show-semantic-views.rst` all say: the statement is primary, "DuckDB cannot use a statement as a subquery, so `FROM (…)` is a parser error", and the backing table function returns the same rows. `materializations.rst` said "can't take a statement … as a subquery" and "syntax error". **Editor rewrote it** to the shared wording and linked `describe_semantic_view()`. `get-ddl.rst` states the reason briefly and links `list_semantic_views()` (fine). |
| Materialization example table | `revenue_by_region` on 7 pages. `describe-semantic-view.rst` still used `daily_revenue_agg`. **Editor renamed it** in both the DDL and the output row. The name has the same length, so the box table stays aligned. |
| How-to layout | All 13 have Title → (page-level `versionadded` on `materializations` and `yaml-definitions` only, placed directly under the title) → intro → **Prerequisites:** → tasks → Troubleshooting → Related. **Editor fixed** two Troubleshooting sections (definition-list form) and the `filtering.rst` Related entries (capitalization and punctuation). |
| Parameter tables | Statement pages (`alter`, `drop`, `describe`, `show-*`) use definition lists. Function pages (`semantic-view-function`, `explain`, `get-ddl`, `read-yaml`) use a Parameter / Type / Description list-table. This split is consistent. The type spellings differ, though (S-open-2). |
| Terms | "fan trap error" vs "fan-trap error" were mixed (10 vs 7). **Normalized** to the open form. "the fence" (internal jargon) appeared on two pages and is **removed**. "define-time" / "query-time" are consistent (open adverbial, hyphenated attributive). |
| Link titles | `show-semantic-dimensions-for-metric.rst` linked `howto-fan-traps` with the stale explicit title "How to Understand and Avoid Fan Traps". **Editor fixed it** to use the page's own title. |
| Project decisions (1)–(4) | All four are respected on every page. No edit touched them. |

---

## docs/how-to/metadata-annotations.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Set Access Modifiers, warning, L144 | **Open (behavioral claim, not edited).** "Dimensions do not support access modifiers." But `PUBLIC` is accepted on a dimension as a no-op (`src/body_parser/entries.rs:68`, test at `src/body_parser/mod.rs:4486`). `create-semantic-view.rst` (grammar L46, parameters L318) and `snowflake-comparison.rst` L65/L117 say the same. | "Dimensions accept only `PUBLIC`, as a no-op; `PRIVATE` on a dimension is rejected." |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Mark a Named Filter, L172 | The `where_clause` link pointed at the top of `ref-semantic-view-function`. The other pages link the parameter's own section. | **Fixed:** retargeted to `ref-sv-pre-agg-filtering`. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Headings | 4 sentence-case subheadings ("View-level comment", "Table-level comment", "Comments on dimensions, metrics, and facts", "Via SHOW commands") in a title-case corpus. | **Fixed:** title case (no labels affected). |

---

## docs/reference/error-messages.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Unknown metric in a derived metric, L250-253 | **Open (error text, not edited).** The template omits the optional `; did you mean '<suggestion>'?` that `src/graph/derived_metrics.rs:184-194` inserts before `. Available metrics:`. `derived-metrics.rst` L166 shows the full form, so the two pages disagree. | Template: `unknown metric '<name>' referenced in derived metric '<derived>'[; did you mean '<suggestion>'?]. Available metrics: [<list>]` |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Near-Miss DDL Detection, L982 | "provides helpful suggestions" (filler). | **Fixed:** "suggests the statement you meant". |
| L990 | "semantic view DDL prefix" vs "semantic-view DDL" (9 other uses). | **Fixed.** |

---

## docs/how-to/fan-traps.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Heading L226, versionchanged L232 | "Other Shapes the Fence Rejects" / "slipped past the fence": "fence" is maintainer jargon (Rule 1 / Rule 5). | **Fixed:** heading is now "Other Shapes That Raise the Fan Trap Error" (label `howto-fan-other-shapes` unchanged, unreferenced), and the body says "fan trap check". |
| L264 | "fan-trap check" | **Fixed:** "fan trap check" (term map). |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| L219-221 | "why the last three shapes are refused". The list it refers to has only three items. | **Fixed:** "these three shapes". |
| Note, L273 | "As of v0.11.0" matches the knowledge-cutoff pattern. | **Fixed:** "Since v0.11.0". |

---

## docs/how-to/filtering.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Troubleshooting, L466 | "the fan-out fence" (jargon). | **Fixed:** "the fan trap check". |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Related | Entries were lowercase with trailing periods. The other 12 how-tos capitalize and use no period. | **Fixed** (4 entries). |

---

## docs/how-to/materializations.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Inspect with SHOW and DESCRIBE, L248 | `describe_semantic_view()` named in prose with no link to its reference entry. | **Fixed:** linked `ref-functions-describe`. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Note, L267-274 | FROM-source explanation worded differently from the three other pages ("can't take a statement … as a subquery", "syntax error"). | **Fixed:** rewritten to the shared idiom ("cannot use a statement as a subquery … parser error … returns the same rows"). |
| Troubleshooting | Bold heading followed by an unindented paragraph. The other 11 how-tos use definition lists. | **Fixed:** 5 entries converted (content unchanged). |

---

## docs/how-to/yaml-definitions.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Troubleshooting | Same layout mismatch as `materializations.rst`. | **Fixed:** 7 entries converted (content unchanged). |

---

## docs/how-to/semi-additive-metrics.rst

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Mixed metrics, L221 | Contraction "don't" (the corpus avoids contractions). | **Fixed:** "do not". |

---

## docs/how-to/data-sources.rst

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Heading L174 | "Postgres via the postgres Extension" (the extension name read as a lowercase word). | **Fixed:** "Postgres via the ``postgres`` Extension" (underline extended; label unchanged). |

---

## docs/tutorials/multi-table.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Update the View, L205 | `CREATE OR REPLACE` introduced with no link; the page never links the CREATE reference. | **Fixed:** linked `ref-create-variants`. |

---

## docs/explanation/snowflake-comparison.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Metric Grain, L374 | "our `fan trap detected` error" (first person, against the tone rules). | **Fixed:** "the extension's". |
| Metric Grain, L398 | "The rescue covers a queried dimension's own table" (unclear jargon). | **Fixed:** "This covers only a queried dimension's own table". |
| L305, L362, L397 | "fan-trap error" ×3 | **Fixed:** "fan trap error". |

---

## docs/explanation/databricks-comparison.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Concept table, L34 | DuckDB column shows `CREATE SEMANTIC VIEW` unlinked, while neighboring rows link their references. | **Fixed:** linked `ref-create-semantic-view`. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| L190, L250 | "fan-trap error" ×2 | **Fixed.** |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Heading L220 | "Naming: measures vs metrics" | **Fixed:** title case. |

---

## docs/explanation/metric-grain.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| L108, L172 | "fan-trap error" ×2 | **Fixed.** |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| L279 | "labelled" (the last British spelling in prose). | **Fixed:** "labeled". |

---

## docs/explanation/transactional-ddl-and-limitations.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Type blur (S-open-4) | **Open, carried over.** The Python catch-and-treat-as-success pattern (L94-101) and the bootstrap-then-reopen workflow (L175-197) are how-to procedures inside an explanation. Not structural enough to block, and each is short. | Move them into a how-to ("Ship a read-only database with semantic views") and link to it from here. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| L224, L265 | Filler: "the rule is simply", "genuinely rolls back". | **Fixed.** |
| Heading L172 | "Bootstrap-then-reopen workflow" | **Fixed:** title case. |

---

## docs/reference/create-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| DIMENSIONS versionchanged, L342 | "define-time type inference pass" is a variant in the term map. | **Fixed:** "define-time inference pass". |
| `IF NOT EXISTS` note, L171 (S-open-1) | **Open (behavioral wording, not edited).** Says `IF NOT EXISTS` absorbs duplicates "within a single process" and fails in a race "between two separate processes". `transactional-ddl-and-limitations.rst` L76-90 and `snowflake-comparison.rst` L492 describe the race between **connections**, and note that only one process can open a file for writing. | Say "on a single connection" / "two connections". Also consider "a constraint or commit-conflict error" to match the explanation page. |
| Read-only notes, L175 (S-open-3) | **Open (error text).** Quotes `Cannot execute statement of type "..." which is attached in read-only mode!`. The real message (as quoted in `transactional-ddl-and-limitations.rst` L160) has `on database "<name>"` before `which`. The same quote appears in `alter-semantic-view.rst` L60 and `drop-semantic-view.rst` L44. | Quote the full form, or mark the gap with `…`. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| FROM YAML examples, L548, L556, L762, L770 (N-open-1) | **Open (example SQL).** The four `FROM YAML` / `FROM YAML FILE` statements have no terminating `;`. `yaml-definitions.rst` now has them. | Add `;`. |

---

## docs/reference/semantic-view-function.rst and explain-semantic-view-function.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Parameters tables (S-open-2) | **Open (type claim, not edited).** `dimensions`, `metrics`, `facts` and `search_path` are typed `LIST (named)`. `functions.rst` (L199-208, L255), which shows real `duckdb_functions()` output, gives `VARCHAR[]`. | Use `VARCHAR[] (named)` on both pages. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| `semantic-view-function.rst` headings L202, L219 | "Post-aggregation -- outer WHERE" / "Pre-aggregation -- where_clause" | **Fixed:** title case (label `ref-sv-pre-agg-filtering` unchanged). |

---

## docs/reference/describe-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| View with materializations, L351, L365 | Materialization table `daily_revenue_agg`. Every other page uses `revenue_by_region`. | **Fixed:** renamed in both the DDL and the output row. Same width, so the table stays aligned. No behavior changed. |
| Tip, L383 (S-open-5) | **Open (example SQL).** The relationships query reads `describe_semantic_view('multi_view')`, a view the page never defines (it has no relationships in `order_metrics`). | Point it at a view with relationships defined on the page, or mark it as illustrative. |

---

## docs/reference/get-ddl.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Dump and restore, L204 | "the table function behind `SHOW SEMANTIC VIEWS`": the statement is unlinked on this page. | **Fixed:** linked `ref-show-semantic-views`. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Heading L66 | "How an unqualified name resolves" | **Fixed:** title case. Label `ref-get-ddl-resolution` is unchanged; the three `:ref:` uses now render the new title. |

---

## docs/reference/read-yaml-from-semantic-view.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Intro, L10 | `CREATE SEMANTIC VIEW ... FROM YAML` unlinked. | **Fixed:** linked `ref-create-from-yaml`. |

---

## docs/reference/show-semantic-dimensions-for-metric.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Intro, L14 | The explicit link title "How to Understand and Avoid Fan Traps" is stale: the page is now "How to Diagnose and Fix Fan Traps". | **Fixed:** `:ref:\`howto-fan-traps\`` (renders the current title). |

---

## Pages with no findings this pass

`index.rst`, `tutorials/getting-started.rst`, `tutorials/building-a-model.rst`, `how-to/index.rst`, `how-to/facts.rst`, `how-to/derived-metrics.rst`, `how-to/role-playing-dimensions.rst`, `how-to/query-facts.rst`, `how-to/wildcard-selection.rst`, `how-to/window-metrics.rst`, `explanation/semantic-views-vs-regular-views.rst`, `reference/index.rst`, `reference/functions.rst` (new: Rule 2 examples present, links to every function page, consistent FROM-source wording), `reference/alter-semantic-view.rst` and `reference/drop-semantic-view.rst` (only the shared S-open-3 quote), `reference/show-semantic-views.rst`, `show-semantic-dimensions.rst`, `show-semantic-metrics.rst`, `show-semantic-facts.rst` (the three sibling pages diff cleanly against each other), `show-semantic-materializations.rst`, `show-columns-semantic-view.rst`, `yaml-format.rst`, `explain-semantic-view-function.rst` (only S-open-2).

---

## CHANGELOG.md (rendered as `docs/changelog.md`)

Only the 0.12.0 line was revised, and it now reads correctly. The Editor did not rewrite historical release entries, because `CHANGELOG.md` is governed by the CLAUDE.md milestone rules. The items below are still open.

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| 20 bullets (S-open-6) | `TECH-DEBT #NN` / "TECH-DEBT item NN" references on lines 76, 82, 84, 90, 114, 126, 142, 213, 217, 221, 223, 227, 235, 237, 245, 251, 353, 416, 417. They render on the Release Notes page. This goes against the "No internal tracker IDs in docs/" convention and the CLAUDE.md audience note. | Remove the IDs, rewording the "See TECH-DEBT item N" sentences at L416-417. |
| 0.10.0 → Changed, L345 (S-open-7) | "or use `REFERENCES target(cols)` shorthand on the foreign side" contradicts the now-corrected `snowflake-comparison.rst` and `error-messages.rst` ("An explicit column list … does not satisfy the requirement"). | Drop the clause, or add a correcting note. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Structure (N-open-2) | 0.11.0 has `Changed` before `Added`. 0.10.0 has `Removed` after `Security`. The `[0.5.3]` link compares `...tags/v0.5.3`, unlike every other entry. | Reorder; fix the link. |

---

## Terminology Changes

`.doc-writer/terminology.yaml` was updated (it validates as YAML). Normalizations applied:

| Term | Before | After | Authority |
|------|--------|-------|-----------|
| fan trap (attributive) | "fan-trap error" ×7, "fan-trap check" ×1 | "fan trap error", "fan trap check" | Majority form (10 open vs 7 hyphenated); term map canonical "fan trap" |
| define-time inference pass | "define-time type inference pass" (create L342) | "define-time inference pass" | Term map canonical |
| semantic-view DDL | "semantic view DDL prefix" (error-messages L990) | "semantic-view DDL prefix" | Majority attributive form (9 vs 1) |
| labeled | "labelled" (metric-grain L279) | "labeled" | American spelling, majority usage |
| Materialization example table | `daily_revenue_agg` (describe) | `revenue_by_region` | Cross-page example consistency |
| "fence" (jargon) | "the Fence", "the fence", "fan-out fence" | "fan trap check" / "Fan Trap Error" | Rule 1 / Rule 5 (maintainer term) |

Term-map updates with no doc change:

| Entry | Change |
|-------|--------|
| `ExpandError` source | `src/expand.rs` → `src/expand/types.rs` |
| `list_semantic_views()`, `describe_semantic_view()` | Annotated as FROM-source-only backing functions, with the shared wording and link targets |
| `duckdb_functions()`, `object_name`, `is_filter`, `DuckLake` | Added |
| View-not-found wording | Split into two canonical messages (catalog "does not exist" vs query-path "not found … Run SHOW SEMANTIC VIEWS"). They are no longer mapped as variants of each other, so a future pass cannot "normalize" correct text. |
| define time / query time | Variants removed. Note added: adverbial open, attributive hyphenated (the corpus is already consistent with this). |
| Conventions | Added: American spelling (British-spelled labels kept), title-case headings (error-catalogue entries excepted), statement vs function parameter layout, how-to section layout. `rules.md` still has no project rules; consider copying the spelling rule there. |
