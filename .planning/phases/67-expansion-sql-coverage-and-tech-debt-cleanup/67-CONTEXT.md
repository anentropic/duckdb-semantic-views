# Phase 67: Expansion-SQL Coverage and Tech-Debt Cleanup - Context

**Gathered:** 2026-05-26
**Status:** Ready for planning
**Source:** `/gsd-discuss-phase 67 --assumptions` (assumptions confirmed wholesale by user)

<domain>
## Phase Boundary

Tighten the sqllogictest regression net around *rewritten expansion SQL* — the layer Phase 66 retrofitted but only smoke-tested at the ADBC integration boundary — and clear the small batch of tech-debt and Phase 66 review-pass cleanup items that are mechanical fixes blocking real edge cases or hygiene.

Primary deliverable: shape assertions over `explain_semantic_view(...)` output covering all five expand paths under non-default-schema and multi-DB-ATTACH conditions. Secondary: body-parser quoted-identifier fix (TECH-DEBT #24) and four micro-cleanup items from the Phase 66 review.

This is the **final technical phase** of v0.10.0. Release prep (CHANGELOG, version bump, example file, tag, milestone close) is explicitly **out of scope** per `feedback_defer_release_tasks` — release rituals happen at milestone close, not folded into the last technical phase.

**In scope:**
- A1–A4: sqllogictest shape coverage for the five expand paths (new `phase67_qualified_emission.test` + shape assertions added to phase46/47/48)
- B5: body parser quoted-identifier fix + fixture (TECH-DEBT #24)
- C1–C3: Phase 66 review-pass micro-cleanup (WR-02 reclassification, WR-01 sentinel decision, IN-02 shlex.quote)
- C4: `latest_qty` / `MIN_BY` semantics resolution in `test_adbc_queries.py` scenario 4

**Out of scope (explicit):**
- TECH-DEBT #12, #19, #21, #23 — architecturally blocked on DuckDB-side changes
- FFI fuzz, Iceberg Python test — accepted long-term constraints
- All v0.10.0 release-prep tasks (CHANGELOG, Cargo.toml/description.yml bump, example file, tagging) — milestone close, not this phase

</domain>

<decisions>
## Implementation Decisions

### Architectural premise

- **D-01:** [informational] Phase 66 migrated five expand-call-sites to `qualify_and_quote_table_ref` (fact-query, semi-additive, window, materialization, main-expand), and verified the migration at the ADBC integration layer. What it did **not** add is a sqllogictest-layer regression guard against silent regression to unqualified `quote_table_ref` — the root cause dissolved with Phase 65's read-elimination, so the failure mode no longer reproduces in-process, but the test gap is genuine.
- **D-02:** [informational] The project's `EXPLAIN`-equivalent is `SELECT * FROM explain_semantic_view('view', ...)` returning expanded SQL as rows. Established idiom: `WHERE explain_output LIKE '%pattern%'` (see `test/sql/phase4_query.test:191-197`, `test/sql/phase28_e2e.test:139-156`). All shape assertions in this phase use this idiom — no new test infrastructure.

### A. Expansion-SQL coverage via sqllogictest

- **D-03:** A1 new fixture lands at `test/sql/phase67_qualified_emission.test` and is registered in `test/sql/TEST_LIST` (per `CLAUDE.md` rule — new sqllogictest files must be added to TEST_LIST or the runner will skip them).
- **D-04:** A1 exercises five expand paths × two conditions (non-default-schema via `CREATE SCHEMA staging` and attached-DB via `ATTACH ':memory:' AS db2`):
  - Main expand (`src/expand/sql_gen.rs` main path) — `FROM "memory"."staging"."t"` and `FROM "db2"."main"."t"`
  - FACTS path — qualified shape in the inner subquery
  - Semi-additive — qualified shape in the snapshot CTE
  - Window — qualified shape in the inner subquery feeding the OVER clause
  - Materialization — qualified shape for both the materialization-target and the routing-fallback path
- **D-05:** A1 fixture also asserts that the query actually executes via `semantic_view(...)` (not just `explain_semantic_view`). This is a fail-closed smoke check that the rewritten SQL resolves — NOT a value-correctness test. The check is structural ("query succeeds and returns at least one row"), not behavioral.
- **D-06:** A2/A3/A4 shape assertions are **co-located** with existing behavioral tests in `phase47_semi_additive.test`, `phase48_window_metrics.test`, `phase46_fact_query.test` — not centralized in a separate `phase67_expansion_shape.test`. Rationale: shape and behavior should fail together so a regression points to one file, not two. (D-02 candidate from SCOPE.md resolved this way.)
- **D-07:** Shape assertions pin **emission fragments** (`OVER (PARTITION BY`, `NULLS LAST`, qualified `FROM "x"."y"."z"`), not full-line equality. `LIKE '%fragment%'` is the established pattern; sqllogictest doesn't do inline regex. Trades exactness for resilience against unrelated whitespace/parenthesization drift.
- **D-08:** **No negative assertions.** Asserting that unqualified `FROM "<table>"` does NOT appear in expansion output is rejected — too brittle against unrelated emission tweaks, and the qualified-shape positive assertion is sufficient. (D-01 candidate from SCOPE.md resolved this way.)

### B. Body parser quoted-identifier fix

- **D-09:** B5 fixes TECH-DEBT #24: `src/body_parser.rs::parse_single_table_entry` tokenizes the `TABLES (...)` clause on whitespace, breaking entries like `TABLES (o AS "my db"."schema"."t" PRIMARY KEY (id))`. Fix reuses `src/ident.rs::find_identifier_end` (shipped in Phase 64 for the same identifier-aware tokenization problem). No new identifier parser.
- **D-10:** B5 includes an audit pass: grep `src/body_parser.rs` for other `split_whitespace` / `split(' ')` call sites that could harbor the same whitespace-on-quoted-name bug. If the audit surfaces additional sites, planner decides scope-creep response (fix in-phase if mechanical, surface as follow-up if structural). Default expectation: the one site SCOPE.md identifies is the only one.
- **D-11:** New sqllogictest fixture (likely `phase67_quoted_source_tables.test` or folded into `phase67_qualified_emission.test`) exercises at least one source-table name with internal whitespace and verifies the full CREATE → query → expansion roundtrip. Planner picks the file location.

### C. Phase 66 review-pass cleanup

- **D-12:** C1 — Reclassify WR-02 in `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md` from `skipped: needs-design-decision` to `not-a-defect`, citing CONTEXT.md D-08/D-09 (ADBC tests are catalog-resolution regression guards, not value-correctness tests). Single-edit commit.
- **D-13:** C2 — WR-01 sentinel decision: keep the sentinel-value fix as-is. The original `COUNT(*) == 2` already discriminated routing-vs-fallback (raw expansion produces 3 rows from source data, not 2), but sentinels make routing failures fail faster and more diagnostically. The "value test" objection is technically present but minor — sentinels are smoke values, not domain assertions. No code change; document the rationale in the WR-01 entry of `66-REVIEW-FIX.md` so the design call is recoverable.
- **D-14:** C3 — Apply IN-02 from `66-REVIEW.md`: wrap the `FORCE INSTALL '{extension_path}'` interpolation in `test/integration/test_adbc_queries.py` with `shlex.quote` (or equivalent escaping). Low practical risk but worth the one-line hardening before it becomes a real defect.
- **D-15:** C4 — `latest_qty` / `MIN_BY` semantics resolution: **try test rename first**. If `MIN_BY(qty, snapshot_date)` returns the value at the *earliest* `snapshot_date` (as the SQL semantically implies), the metric name `latest_qty` is misleading and the cheapest fix is renaming the metric to `earliest_qty` in `test_adbc_queries.py` scenario 4. Only escalate to a code-level audit of project `MIN_BY` semantics if the rename is contradicted by Phase 47's RESEARCH.md or by the actual emitted SQL.

### Sequencing

- **D-16:** Implementation order:
  1. **A2/A3/A4 first** (existing-fixture shape assertions). Cheapest, lowest-risk, validates the LIKE-pattern shape-assertion strategy before committing to the A1 fixture design.
  2. **A1 next** (new `phase67_qualified_emission.test`). Builds on what A2-A4 proved.
  3. **B5** (body-parser fix). Independent surgery; sequencing flexible.
  4. **C1/C2/C3 cleanup batch**. Bookkeeping; any time.
  5. **C4 last** (or fold into A1 if it resolves to a test rename).
- **D-17:** Likely plan split: ~4 plans (A coverage / B body-parser / C cleanup / C4 metric semantics), with C4 absorbed into A if it's a test rename. Planner has discretion to merge or split further during plan-phase.
- **D-18:** **No research agent.** SCOPE.md already cites exact files, lines, and patterns. Phase goes straight from discuss → plan.

</decisions>

<canonical_refs>
## Canonical References

### Source files touched
- `test/sql/phase67_qualified_emission.test` (NEW — A1)
- `test/sql/phase46_fact_query.test` (A4 — tighten FACTS shape assertions)
- `test/sql/phase47_semi_additive.test` (A2 — semi-additive snapshot CTE shape)
- `test/sql/phase48_window_metrics.test` (A3 — window OVER frame shape)
- `test/sql/TEST_LIST` (register new fixture(s))
- `src/body_parser.rs::parse_single_table_entry` (B5 surgery site)
- `src/ident.rs::find_identifier_end` (B5 reuse — DO NOT modify)
- `test/integration/test_adbc_queries.py` (C3 shlex.quote, C4 metric rename)
- `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md` (C1 reclassify WR-02, C2 document WR-01 rationale)

### Reference docs
- `.planning/phases/66-expansion-qualification-adbc-tests/66-CONTEXT.md` — D-08/D-09 framing the ADBC tests as catalog-resolution regression guards (citation for C1)
- `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW.md` — WR-01..WR-06, IN-01..IN-05
- `TECH-DEBT.md` — entry #24 (body parser whitespace tokenizer) is the B5 target
- `CLAUDE.md` — TEST_LIST registration requirement, `statement error` block-form rule

### Established patterns to mirror
- `test/sql/phase4_query.test:191-197` and `test/sql/phase28_e2e.test:139-156` — canonical `explain_semantic_view(...) WHERE explain_output LIKE '%pattern%'` shape-assertion idiom

</canonical_refs>

<code_context>
## Existing Code Insights

- `qualify_and_quote_table_ref` (Phase 64) is the unifying function under test in A1; all five migrated sites pass `(name, def)` and emit `"database"."schema"."name"`.
- Phase 66 verified the migration at the ADBC integration layer (`test_adbc_queries.py` scenarios 1–6, all passing). The sqllogictest layer has no equivalent guard — that's the A1 gap.
- `src/ident.rs::find_identifier_end` (Phase 64) is the canonical identifier-aware tokenizer; B5 reuses it rather than duplicating the parser.
- `ATTACH ':memory:' AS db2` works in sqllogictest. Multi-DB ATTACH inside a single test file may require `DETACH` between scenarios to avoid state bleed — planner should verify during A1 fixture authoring.
- Existing FACTS path shape (per `phase46_fact_query.test`) uses `count(*) > 0` existence checks (A4's target for tightening). FACTS emits a distinct inner-vs-outer aggregation structure that should be pinnable.

</code_context>

<risk_areas>
## Risks and Mitigations

- **Test brittleness.** Shape assertions risk phantom failures on unrelated emission tweaks. Mitigation: pin fragments via `LIKE '%...%'`, not full-line equality (D-07).
- **B5 scope creep.** `parse_single_table_entry` may not be the only quoted-identifier whitespace bug in `body_parser.rs`. Mitigation: D-10 mandates an audit grep; planner decides scope-creep response.
- **C4 escalation.** If `MIN_BY` semantics in the project differ from standard SQL, the resolution could ripple beyond a test rename. Mitigation: D-15 caps initial effort at "try rename first"; escalation requires explicit re-scoping.
- **A1 multi-DB-ATTACH state bleed.** ATTACH inside a sqllogictest file may interact with subsequent statements. Mitigation: explicit `DETACH` between scenarios if state bleed shows up during authoring.

</risk_areas>

<deferred>
## Deferred Ideas (NOT in scope)

- Architectural #1 / Test coverage #1, #2 in TECH-DEBT.md (FFI fuzz / Iceberg Python test) — accepted long-term constraints
- TECH-DEBT #12 (DDL pipeline all-VARCHAR result forwarding) — performance/cosmetic; defer until DDL schemas stabilize
- TECH-DEBT #19 (DESCRIBE/SHOW read committed state) — blocked on DuckDB C-API
- TECH-DEBT #21 (`disable_peg_parser` resets parser_override) — blocked on DuckDB-side fix
- TECH-DEBT #23 (CREATE IF NOT EXISTS race PK violation) — blocked on DuckDB hook
- Negative assertions on absence of unqualified `FROM "<table>"` — rejected per D-08 as brittle
- Centralized `phase67_expansion_shape.test` catch-all fixture — rejected per D-06 in favor of co-location
- v0.10.0 release prep (CHANGELOG, version bump, example, tag, milestone close) — handled at milestone close per `feedback_defer_release_tasks`

</deferred>

## Next Steps

1. `/gsd-plan-phase 67` — produce concrete plan files implementing D-01..D-18
