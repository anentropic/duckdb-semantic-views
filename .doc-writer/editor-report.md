# Editor Report

**Generated:** 2026-08-09
**Files reviewed:** 22 revised pages, plus a terminology sweep of all 39 `.rst` files under `docs/`
**Changes made:** 31
  - BLOCKING: 8
  - SUGGESTION: 12
  - NITPICK: 11

## Summary

The five parallel authors produced unusually clean prose — the humanizer pass
found exactly one hit across 22 files — but they drifted on facts, as expected.
The most serious drift is a shared `data_type` one-liner that six pages inherited
from a stale CHANGELOG sentence and that directly contradicts `yaml-format.rst`
and `src/model.rs`. The per-grain story now agrees across `metric-grain.rst`,
`fan-traps.rst` and `snowflake-comparison.rst`; both new pages' hand-computed
result tables reconcile against their own `INSERT` rows; and no surviving
after-`AS` `NON ADDITIVE BY` example exists anywhere in `docs/`.

### Priority checks, resolved

| # | Check | Result |
|---|-------|--------|
| 1 | `data_type` wording identical across five pages | **Was not.** One-liner was factually wrong on six pages; prose block existed in three different wordings and was missing entirely from a fourth. All normalized. No `LIMIT 0` CREATE-time inference reintroduced; `TECH-DEBT #51` appears nowhere in `docs/` (nor does any other tracker ID). |
| 2 | `NON ADDITIVE BY` clause order | **Clean.** Grepped all 39 files. The only after-`AS` occurrences are the two deliberate counter-examples (`semi-additive-metrics.rst:251` troubleshooting, `create-semantic-view.rst:346` rule statement). Both are correctly framed as errors. Verified against `src/body_parser/mod.rs` (`non_additive_by_after_as_is_rejected`, `using_after_as_is_rejected`). |
| 3 | Per-grain behaviour consistent | **Two gaps, both fixed.** `snowflake-comparison.rst` and `metric-grain.rst` both omitted the role-played-dimension + active-semi-additive exclusion that `fan-traps.rst` documents. Added to both, matching `role_played_dimension_with_a_semi_additive_metric_stays_ineligible` (`tests_per_grain.rs:1545`). |
| 4 | New pages: Diataxis, terminology, arithmetic | **Types are clean** (see Pass 2). All four result tables in `filtering.rst` and the one in `metric-grain.rst` reconcile against their own `INSERT` rows — arithmetic verified by hand, nothing executed. |
| 5 | RST internal-notes blocks | **Removed** from both new pages. |
| 6 | Cross-references and toctrees | **All 317 `:ref:` targets resolve**, no duplicate labels, both new pages present in their toctree and in `conf.py` `nav_links`. Two links added; nothing introduced that would break `-W`. |

---

## docs/reference/show-semantic-dimensions.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Output Columns → `data_type` | The shared one-liner claimed the type is "Empty string unless the definition declares one, which only a YAML definition can do". A YAML `output_type` is now **rejected at import** — `reject_output_type` in `src/model.rs:839` raises `"declares output_type '<T>', which no DDL clause can express"`. This directly contradicts `yaml-format.rst:476-482` ("**No longer accepted.**") on the same site. | Replaced with: "The **declared** output type. Empty for every view created since v0.10.0 -- no surface can declare a type and nothing infers one. Populated only for views stored before that release." |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Examples | The explanatory `data_type` paragraph ended "populated only for views stored before that change" with no antecedent for *that change* — two changes are described in the sentence (the v0.10.0 inference removal and the YAML withdrawal). | Named the release explicitly and folded in the missing inference-removal clause. Now byte-identical to the sibling paragraph on `show-semantic-facts.rst` and `show-semantic-metrics.rst`. |

---

## docs/reference/show-semantic-facts.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Output Columns → `data_type` | Same stale one-liner. | Same replacement. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Examples | Third distinct wording of the same paragraph, and the only one that named the removed pass as "the CREATE-time `typeof` pass" — vocabulary no other page uses. | Normalized to the canonical paragraph; the removed pass is called "the define-time inference pass" here and on the `snowflake-comparison.rst` anchor. |

---

## docs/reference/show-semantic-metrics.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Output Columns → `data_type` | Same stale one-liner. | Same replacement. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Examples | Rule 3 (Consistent Structure): this page was the only one of the four `SHOW` siblings with **no** explanatory paragraph after its first example — the explanation was deferred to a caption three examples later. A reader comparing the four pages sees a different layout. | Added the canonical paragraph after the first example; shortened the later caption to "for the reason given above". |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Filtering Clauses | Literal em dash where the rest of the corpus uses `--`. Under Sphinx smartquotes these render differently (— vs –). | Replaced 1 instance. |

---

## docs/reference/show-semantic-dimensions-for-metric.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Output Columns → `data_type` | Same stale one-liner. | Same replacement. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Examples | Fourth wording variant, shorter than the others and missing the inference-removal clause. | Normalized. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Parameters | Literal em dash. | Replaced 1 instance. |

---

## docs/reference/show-columns-semantic-view.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Output Columns → `data_type` | Same stale one-liner. | Same replacement. |
| Examples → Error | Documented error text was `Error: Semantic view 'nonexistent' not found`. `src/ddl/show_columns.rs:71` calls `crate::catalog::view_not_found_msg`, which produces `semantic view '<name>' does not exist` (`src/catalog/mod.rs:105`) — the same wording every other reference page shows. Both the capital *S* and the "not found" phrasing were wrong. | Corrected to `Error: semantic view 'nonexistent' does not exist`. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Examples | The one-line caption gave a different reason ("the view was created through SQL DDL") from the four `SHOW` pages, implying a YAML-defined view might differ. | Replaced with the canonical paragraph. |

---

## docs/explanation/snowflake-comparison.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Metric Grain | The bullet list omitted the one multi-grain pairing that still declines: a role-played dimension queried together with an active semi-additive metric. Read alongside the two bullets that *do* say role-playing-with-`USING` and semi-additive both work, the page implied the combination works. Source: `tests_per_grain.rs:1545` `role_played_dimension_with_a_semi_additive_metric_stays_ineligible`, which asserts the query must decline because the snapshot grain would bind the declaration-order relationship while the sibling grain binds the `USING`-named one. | Added a closing sentence to the role-playing bullet naming the exclusion and its reason. |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Metric Grain | "Two boundaries are worth knowing:" introduced a list of **four** bullets. | Changed to "Four boundaries". |
| Reported Data Types | Called the removed pass "the `typeof` pass" and said the column is empty "for every newly created view … before that change", while `create-semantic-view.rst` said "since v0.10.0 … before that change". | Aligned to "the define-time inference pass" and "since v0.10.0 … before that release", matching the reference pages that link here. |

---

## docs/explanation/metric-grain.rst (NEW)

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| What Is Still Refused, and Why | Same omission as `snowflake-comparison.rst`: the page said semi-additive metrics "are computed at their own grain and can appear alongside metrics at other grains" with no exception, while `fan-traps.rst` (corrected by the maintainer against the source tests) documents the role-played-dimension pairing as still ineligible. As written the two pages contradicted each other. | Added a fifth refused-shape entry, "A role-played dimension queried together with an active semi-additive metric", and qualified the closing semi-additive paragraph with "otherwise". Count updated from "Four shapes" to "Five shapes". |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Author's accuracy note (resolved) | The Author flagged that the two CHANGELOG semi-additive bullets disagree and that this page followed the later one. Confirmed correct against `tests_per_grain.rst:531` `multi_grain_with_active_semi_additive_metric_is_computed`, whose doc comment states it *supersedes* `..._still_errors`. No prose change needed. | None — claim verified. |
| Result table | The worked "accounts" table was hand-computed. Reconciled against the page's own `INSERT` rows: East = customers 1+2 = 500+300 = 800.00 and 3 orders; West = customer 3 = 900.00 with no order rows, hence `NULL`. Arithmetic is internally consistent; the `NULL`-vs-`0` behaviour matches the NULL-safe `FULL OUTER JOIN` the same page documents. | No change. Still unverified by execution, as instructed. |
| Cross-references | Missing entry in the refused list for `using_naming_a_non_role_played_relationship_still_declines` (`tests_per_grain.rs:1292`) — a `USING` that names a relationship which is not the role-played one also declines. Minor, and arguably covered by the "reached without `USING`" bullet. | Flagged only; not added, to avoid over-specifying an explanation page. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| End of file | 31-line `.. Internal notes for the Editor agent` RST comment block. No other page in `docs/` carries one. | Removed. |

---

## docs/how-to/filtering.rst (NEW)

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Author's accuracy notes (resolved) | Three open questions from the Author, all checked: (a) `explain_semantic_view()` does accept `where_clause` — `explain-semantic-view-function.rst:25,55-57` agrees, and the two functions share one registration; (b) `LABELS = (FILTER)` on a **dimension** is correct — `create-semantic-view.rst:47` grammar and `metadata-annotations.rst:152` both admit it on a fact *or* dimension; (c) `0.12.0` is the right release — `Cargo.toml` reads `version = "0.12.0"` against an open `## [Unreleased]` CHANGELOG section. | No prose change needed on any of the three. |
| Result tables | All four hand-computed tables reconcile against the five `INSERT` rows: unfiltered East 400.00/3 and West 550.00/2; 2024-filtered East 300.00/2 and West 150.00/1; the per-day breakout (250.00 / 50.00 / 150.00); and the combined filter keeping only East 300.00. Order 4 (2023-12-10) is correctly excluded throughout. | No change. Still unverified by execution, as instructed. |
| Diataxis | Type integrity is clean. The page stays imperative, every section is a task, and the one conceptual claim ("`where_clause` decides which rows the metrics aggregate over") is one sentence long and links out to `explanation-metric-grain` rather than expanding. No drift toward explanation. | No change. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| End of file | 27-line `.. Internal notes for the Editor agent` RST comment block. | Removed. |

---

## docs/how-to/fan-traps.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Multi-Grain Queries | Cross-type linking (Diataxis): the page is the *only* one of the three per-grain pages with no outbound link to `explanation-metric-grain`, even though `metric-grain.rst` opens by pointing here as its diagnostic counterpart. The pairing was one-directional. | Added a sentence after the single-grain note linking to `explanation-metric-grain` for the modelling view. |
| Prose | "computed per-grain" used adverbially (3 instances) where `metric-grain.rst` writes "computed per grain". Attributive uses ("the per-grain path", "per-grain assembly") are correct hyphenated and were left alone. | Normalized the 3 adverbial uses. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Throughout | Literal em dashes where the corpus uses `--`. | Replaced 10 instances. |

---

## docs/reference/create-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| FACTS | Rule 3: `DIMENSIONS` and `METRICS` each carry a **Reported data type** block; `FACTS` does not, although `SHOW SEMANTIC FACTS` reports the same empty column and `show-semantic-facts.rst` explains it at length. A reader working down the clause reference finds the note twice and then loses it. | **Not fixed** — adding a third block is new content, not an edit. Recommend the Author add a one-paragraph **Reported data type** block to the `FACTS` section mirroring the `METRICS` one. |
| Clause order / Reported data type blocks | Both `data_type` blocks and the `NON ADDITIVE BY` clause-order paragraph were checked against the source and against each other. They agree, and they agree with the `explanation-sf-data-types` anchor after this pass. | No change. |

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Throughout | Literal em dashes. | Replaced 19 instances. |

---

## docs/how-to/semi-additive-metrics.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Troubleshooting | Mid-sentence "Non-additive" / "Non-Additive" (3 instances) against 80+ lowercase uses elsewhere. Reads as a proper noun the project does not have. | Normalized to "non-additive". |
| Clause order | The `NON ADDITIVE BY`-before-`AS` rule, its grammar block, its error text and its `versionchanged:: 0.12.0` note were checked against `src/body_parser/mod.rs` and against `create-semantic-view.rst:346`. Consistent on both. | No change. |

---

## docs/how-to/facts.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Annotate Facts with Metadata | Two problems in one sentence. It claimed facts support "the same metadata annotations as dimensions and metrics" — but dimensions reject `PRIVATE` (`create-semantic-view.rst:311`) and metrics reject `LABELS` (`create-semantic-view.rst:109-112`), so the set is not shared with either. And it omitted `LABELS = (FILTER)`, which `create-semantic-view.rst`, `metadata-annotations.rst` and the new `filtering.rst` all document on facts. | Rewrote as a direct enumeration of the four annotations facts accept, and added a bullet for `LABELS = (FILTER)` linking to `howto-annotations-filters` and `howto-filtering`. |

---

## docs/reference/yaml-format.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Complete example | "A **comprehensive** YAML definition covering all supported features" — AI-vocabulary hedge, and redundant beside "all supported features". | "A YAML definition covering all supported features". |

---

## Pages reviewed with no changes required

`docs/explanation/databricks-comparison.rst`, `docs/explanation/transactional-ddl-and-limitations.rst`,
`docs/explanation/index.rst`, `docs/how-to/index.rst`,
`docs/reference/explain-semantic-view-function.rst`, `docs/tutorials/getting-started.rst`,
`docs/tutorials/multi-table.rst`, `docs/tutorials/building-a-model.rst`, `docs/conf.py`.

All three tutorials' result tables were reconciled against their `INSERT` rows and are
arithmetically correct, including the DOUBLE margin values in `building-a-model.rst`
(141/236, 35/60, 94/176) and the fact-inlining totals (Alice 50.00 + 36.00 + 150.00 = 236.00).
`conf.py` `nav_links` carries both new pages with summaries in the right sections.

---

## Findings outside the revised set (reported, not edited)

| File | Severity | Finding |
|------|----------|---------|
| `docs/reference/describe-semantic-view.rst` | **BLOCKING** | Carries the same wrong `data_type` one-liner **four times** (lines 141-142, 165-166, 187-188, 211-212). It must get the corrected wording or `DESCRIBE` will contradict every `SHOW` page. This is the single most important follow-up. |
| `docs/reference/semantic-view-function.rst` | SUGGESTION | Line 142 describes the query-bind-time `LIMIT 0` probe. This is correct and is *not* the removed CREATE-time inference — the sentence already says so ("there is no `CREATE`-time type cache to fall back on"). Left alone deliberately; noting it so a future sweep does not mistake it for the removed pass. |
| `CHANGELOG.md` line 100 | SUGGESTION | Source of the drift. It says the column reports the type a definition declared "— which only a YAML definition can do", which line 106 of the same file later revokes. Worth reconciling before the changelog is rendered verbatim as the Release Notes page. |
| `docs/reference/get-ddl.rst` (10), `semantic-view-function.rst` (14), `error-messages.rst` (10), `show-semantic-views.rst` (3), `how-to/query-facts.rst` (2) | NITPICK | 39 literal em dashes outside the revised set. The corpus convention is `--`; under Sphinx smartquotes the two render differently. Worth one sweep. |
| `docs/reference/show-dims-for-metric` "Window metrics" rule | NITPICK | "Fan trap checking is skipped for window function metrics" is inspection-time behaviour and reads slightly against the query-time rule in `metric-grain.rst` ("two window metrics at different grains still error"). Not a contradiction — different surfaces — but a half-sentence of scoping would remove the friction. |

---

## Terminology Changes

| Term | Before | After | Authority |
|------|--------|-------|-----------|
| non-additive | `Non-additive`, `Non-Additive` (3×, `semi-additive-metrics.rst`) | `non-additive` | Most-frequent form (80+ lowercase uses corpus-wide) |
| computed per grain | `computed per-grain` (3×, `fan-traps.rst`) | `computed per grain` | `metric-grain.rst`; adverbial vs attributive grammar |
| define-time inference pass | `CREATE-time typeof pass` (`show-semantic-facts.rst`), `typeof pass` (`snowflake-comparison.rst`) | `define-time inference pass` | CHANGELOG v0.10.0 entry; consistency with `create-semantic-view.rst` |
| `semantic view '<name>' does not exist` | `Semantic view '<name>' not found` (`show-columns-semantic-view.rst`) | canonical form | `catalog::view_not_found_msg`, `src/catalog/mod.rs:105` |
| `--` (parenthetical dash) | literal em dash | `--` | Corpus convention (30 of 39 files); 31 instances replaced across 4 revised pages |

Terms deliberately **not** normalized: `fan trap` / `fan-trap`, `define time` / `define-time`,
`query time` / `query-time`, `base table` / `base-table`. Each pair is noun-versus-attributive
and the hyphenation is grammatically correct in the position it appears. Normalizing them would
be over-normalization, not consistency.

The term map at `.doc-writer/terminology.yaml` has been updated with these entries plus a new
`conventions` block recording the four cross-file rules this pass enforced (dash form, clause
order, `data_type` semantics, no internal tracker IDs).
