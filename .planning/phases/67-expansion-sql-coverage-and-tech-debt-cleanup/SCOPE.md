---
phase: 67
title: Expansion-SQL Coverage and Tech-Debt Cleanup
captured_from: post-Phase-66 conversation, 2026-05-26
captured_by: Claude (Opus 4.7) — orchestrator session that completed Phase 66 execution + code review + verification
status: draft-scope-for-discuss
---

# Phase 67 Scope Draft

Captured immediately after Phase 66 verification (passed) so the analysis context isn't lost on `/clear`. The discuss-phase agent should treat this as the starting point for D-01..D-NN decisions, not as final scope.

## Origin

Phase 66 verified `passed` (EXPAND-CTX-01/02/03 satisfied). The code-review pass + post-review analysis surfaced two related findings:

1. The sqllogictest suite has uneven coverage of *rewritten expansion SQL* — well-covered for the main path and materialization routing, missing entirely for semi-additive and window paths, and stub-only (`count(*) > 0`) for the FACTS path. More importantly, **no sqllogictest fixture exercises the non-default-schema or multi-DB-ATTACH conditions** that EXPAND-CTX-01 was specifically supposed to guard against. The recent migration retained value as defense-in-depth (the dissolved-by-Phase-65 root cause meant the failure mode no longer reproduced), but the sqllogictest-layer regression guard is genuinely missing.
2. `TECH-DEBT.md` carries open items (`❓`) that are worth a deliberate triage — most are architecturally blocked on DuckDB-side changes and should stay deferred, but at least one (#24, body-parser quoted-name handling) is a doable mechanical fix that unblocks a real edge case.

User direction during scope capture (2026-05-26):
- *"primarily test for expected re-written SQL, although some actual value tests are fine where appropriate"*
- *"I need those tests to be comprehensive"* (re: expansion-SQL coverage)
- *"add a phase to address these gaps and also review other tech debt"*

## Proposed In-Scope Work

### A. Expansion-SQL coverage via sqllogictest (`explain_semantic_view`)

The project's `EXPLAIN`-equivalent is `SELECT * FROM explain_semantic_view('view', ...)` which returns the expanded SQL as rows. Existing usage is in 6 files; pattern is `... WHERE explain_output LIKE '%pattern%'`. The user's preferred testing strategy ("primarily test for expected re-written SQL") aligns with widening this assertion shape.

**A1 (M) — New `phase67_qualified_emission.test` fixture.** The single most important addition. Creates `CREATE SCHEMA staging`, `ATTACH ':memory:' AS db2`, builds one semantic view per migrated expand-path against non-default-schema and attached-DB base tables, asserts the qualified `FROM "db"."schema"."table"` shape in `explain_semantic_view` output. Scope per migrated path:
- Main expand (`sql_gen.rs` main path) — `FROM "memory"."staging"."t"` and `FROM "db2"."main"."t"`
- FACTS (`sql_gen.rs:181/224/244` equivalent) — qualified shape in the inner subquery
- Semi-additive (`semi_additive.rs:195/220/238` equivalent) — qualified shape in the snapshot CTE
- Window (`window.rs:156/181/199` equivalent) — qualified shape in the inner subquery feeding the OVER clause
- Materialization (`materialization.rs:157` equivalent) — qualified shape for both the materialization-target and the routing-fallback path
Also asserts that running the query through `semantic_view(...)` (not just EXPLAIN) succeeds, so the test fails-closed if the rewritten SQL doesn't resolve.

**A2 (S) — Shape assertions added to `phase47_semi_additive.test`.** Three or four `LIKE '%pattern%'` assertions on `explain_semantic_view` output that pin the snapshot-CTE shape: `ROW_NUMBER() OVER (PARTITION BY ... ORDER BY report_date DESC) AS sv_rn` (or whatever the actual emitted name is — researcher should grep `src/expand/semi_additive.rs` to find the canonical pattern), the `WHERE sv_rn = 1` predicate, and the qualified `FROM` reference. Keep the existing behavioral tests; just add shape assertions alongside.

**A3 (S) — Shape assertions added to `phase48_window_metrics.test`.** Same pattern as A2 but for `OVER (PARTITION BY ... ORDER BY ... NULLS LAST)` emission. Pin the exact frame clause shape so a window-frame regression would be caught at the SQL layer, not just at the value layer.

**A4 (S) — Tighten `phase46_fact_query.test` shape assertions.** Replace the existing `count(*) > 0` existence checks with shape assertions on the FACTS-specific aggregation pattern (FACTS emits a distinct inner-vs-outer structure that should be pinnable).

### B. Tech-debt items worth pulling in

Reviewed all open (`❓`) items in `TECH-DEBT.md`. Most are architecturally blocked. One is doable.

**B5 (S–M) — `TECH-DEBT.md` #24: body parser splits on whitespace inside quoted source-table names.** `src/body_parser.rs::parse_single_table_entry` tokenizes the `TABLES (...)` clause on whitespace, which breaks for `TABLES (o AS "my db"."schema"."t" PRIMARY KEY (id))` style entries. Fix: port `src/ident.rs::find_identifier_end` into the body parser and replace the whitespace-tokenizer at the source-table-name capture site. Add a sqllogictest fixture exercising at least one quoted source-table name with internal whitespace. Edge case but unblocks a real user-facing parse failure.

**Deferred (NOT in scope — surfaced for the record):**
- `TECH-DEBT.md` #12 (DDL pipeline all-VARCHAR result forwarding) — performance/cosmetic; defer until DDL schemas stabilize.
- `TECH-DEBT.md` #19 (DESCRIBE/SHOW read committed state) — blocked on DuckDB C-API exposing the bind connection.
- `TECH-DEBT.md` #21 (`disable_peg_parser` resets parser_override setting) — blocked on DuckDB-side fix.
- `TECH-DEBT.md` #23 (`CREATE IF NOT EXISTS` race PK violation) — blocked on DuckDB hook for retry-on-conflict in `parser_override`.
- Architectural #1 / Test coverage #1, #2 (FFI fuzz / Iceberg Python test) — accepted long-term constraints with current mitigations.

### C. Phase 66 review-pass cleanup

These are tiny items that surfaced during the Phase 66 code-review-fix pass. Worth folding in since they touch files Phase 66 already changed and are individual-commit-sized.

**C1 (XS) — Reclassify WR-02 in `66-REVIEW-FIX.md`.** Currently flagged `skipped: needs-design-decision`. Per the post-review analysis (CONTEXT.md D-08/D-09 explicitly framed the ADBC tests as catalog-resolution regression guards, NOT value-correctness tests), the right disposition is `not-a-defect`. Update the entry with the framing rationale referencing CONTEXT.md D-08/D-09.

**C2 (XS) — Revisit WR-01's sentinel-row fix.** The original `COUNT(*) == 2` assertion in scenario 6 (`materialization_routing_non_default_schema_target`) would already have caught silent fall-back to raw expansion (raw expansion produces `COUNT == 3` from the source data, not 2). The sentinel-value fix made the proof slightly tighter but shifted the assertion subtly toward the "value test" pattern the project explicitly doesn't want at the ADBC layer. Decide: keep sentinels (tighter), revert to `COUNT(*)` (cleaner), or split the difference (keep one sentinel as a smoke for routing-vs-fallback discrimination). Discuss decision.

**C3 (XS) — Apply IN-02 from `66-REVIEW.md`.** `FORCE INSTALL '{extension_path}'` in `test/integration/test_adbc_queries.py` interpolates the path without escaping. Low practical risk (project-internal helper) but worth a one-line `shlex.quote` or equivalent before this becomes a real defect.

**C4 (S) — Resolve `latest_qty` / `MIN_BY` ambiguity in `test_adbc_queries.py` scenario 4.** The metric is named `latest_qty` but `MIN_BY(qty, snapshot_date)` returns the value at the *earliest* snapshot_date, not the latest. Either (a) the metric name is misleading and should be `earliest_qty`, (b) the metric definition should be `MAX_BY(qty, snapshot_date)`, or (c) the project's `MIN_BY` semantics differ from standard SQL `MIN_BY` and the test is actually correct. Discuss-phase should pull in whoever can confirm the intended behaviour. Once resolved, the WR-02 "value assertions for scenario 4" question dissolves naturally.

## Open Questions for Discuss-Phase

- D-01 candidate: Should A1's new fixture also assert that the *unqualified* `FROM "<table>"` shape does NOT appear in the expansion output? (i.e., negative assertion as a stronger regression guard.) Trade-off: tighter guard vs. fixture brittleness against unrelated expansion-shape evolution.
- D-02 candidate: For A2/A3/A4, should shape assertions go alongside existing behavioral tests in the same file, or in a new shared `phase67_expansion_shape.test` fixture? Co-location keeps related tests together; separation centralizes the "rewritten SQL" assertion style and makes the testing strategy explicit.
- D-03 candidate: For B5, should the fix include a deprecation/rename of `src/body_parser.rs::parse_single_table_entry` toward a name that reflects identifier-aware tokenization (e.g., `parse_table_entry_with_quoted_idents`)? Or leave the name and just swap the implementation?
- D-04 candidate: For C4, who is the canonical authority on the intended `MIN_BY` semantics? The Snowflake semantic-view docs the project mirrors, or the project's own ADR / RESEARCH.md from the original Phase 47 (semi-additive metric implementation)?

## Estimate

- A1: M (~1 day) — new fixture, ~150–250 lines of sqllogictest, exercises 5 paths × 2 conditions
- A2, A3, A4: S each (~2 hours each) — incremental additions to existing fixtures
- B5: S–M (~half-day to full day) — body parser surgery + new test fixture for quoted source-table names
- C1, C2, C3: XS each (~30 minutes each) — single-edit commits
- C4: S — depends on whether the design call is fast or needs research

Roughly a 4-plan phase: (1) A1 + A2 + A3 + A4 (sqllogictest shape coverage), (2) B5 (body parser fix), (3) C1 + C2 + C3 (review-pass cleanup batch), (4) C4 (metric semantics resolution) — or fold (4) into (1) if it turns out to be a name change rather than a code change.

## References

- `.planning/phases/66-expansion-qualification-adbc-tests/66-CONTEXT.md` — D-08/D-09 framing the ADBC tests as catalog-resolution regression guards (the key citation for C1)
- `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW.md` — WR-01..WR-06, IN-01..IN-05 findings
- `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md` — WR-02 currently flagged `needs-design-decision`, the entry C1 reclassifies
- `TECH-DEBT.md` — entry #24 (body parser whitespace tokenizer) is B5
- `test/integration/test_adbc_queries.py` — scenarios 4 and 6 are the targets for C2 and C4
- `test/sql/phase47_semi_additive.test`, `test/sql/phase48_window_metrics.test`, `test/sql/phase46_fact_query.test`, `test/sql/phase57_introspection.test`, `test/sql/phase64_quoted_idents.test` — the existing sqllogictest landscape A1..A4 sits within
- `src/body_parser.rs::parse_single_table_entry`, `src/ident.rs::find_identifier_end` — the B5 surgery sites

## Next Steps

1. `/clear` — drop the bloated post-Phase-66 context
2. `/gsd-discuss-phase 67` — convert this draft into formal D-01..D-NN decisions, surface any missing context, resolve open questions above
3. `/gsd-plan-phase 67` — produce concrete plan files

This SCOPE.md is a starting point, not a finished spec. Discuss-phase should challenge any item that doesn't hold up under scrutiny and add items I missed.
