---
phase: 64
plan: 04
subsystem: tests-docs-traceability
tags: [sqllogictest, fuzz-seeds, regression-test, changelog, requirements, tech-debt, closeout]
requires:
  - file: src/ident.rs (from 64-01)
  - file: src/parse.rs (capture-site wiring from 64-02)
  - file: src/expand/resolution.rs (idempotent quote_table_ref from 64-03)
provides:
  - "End-to-end sqllogictest acceptance fixture covering QID-01..06"
  - "Workspace integration regression test asserting bare-name normalisation in validate_and_rewrite"
  - "Tracked fuzz seed bank for quoted-identifier inputs"
  - "CHANGELOG [0.9.0] ### Fixed bullets documenting the bug fix"
  - ".planning/REQUIREMENTS.md QID-01..QID-07 + 21/21 coverage"
  - ".planning/ROADMAP.md Phase 64 entry marked complete"
  - "TECH-DEBT.md entry 24 documenting deferred body-parser limitation"
affects:
  - test/sql/TEST_LIST
  - fuzz/fuzz_targets/fuzz_ddl_parse.rs
tech_stack:
  added: []
  patterns:
    - sqllogictest `statement error` with `----` separator + expected substring
    - workspace integration test driving public crate API (semantic_views::parse::validate_and_rewrite)
    - libfuzzer seed corpus bootstrap from tracked fuzz/seeds/<target>/ directory
key_files:
  created:
    - test/sql/phase64_quoted_idents.test
    - fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_bare.txt
    - fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_fqn.txt
    - fuzz/seeds/fuzz_ddl_parse/seed_phase64_mixed_quoting.txt
    - tests/quoted_idents_regression.rs
    - .planning/phases/64-fix-quoted-identifier-handling/64-04-SUMMARY.md
  modified:
    - test/sql/TEST_LIST
    - fuzz/fuzz_targets/fuzz_ddl_parse.rs
    - CHANGELOG.md
    - .planning/REQUIREMENTS.md
    - .planning/ROADMAP.md
    - TECH-DEBT.md
decisions:
  - "sqllogictest `statement error` block uses the project's `<sql>\\n----\\n<expected substring>` format (matches v080_transactional_ddl.test / error_caret_*.test convention). Inline `statement error <regex>` is NOT supported by the runner."
  - "explain_semantic_view's output column is `explain_output`, not `plan`. The QID-04 LIKE-filter queries were targeted accordingly."
  - "Regression test asserts BOTH the positive (`'orders_sv'` literal embedded in rewritten SQL) AND the negative (`\"memory\".\"main\".\"orders_sv\"` NOT embedded). The negative assertion is the load-bearing one — it pins the actual bug shape."
  - "Fuzz seeds went into the tracked `fuzz/seeds/fuzz_ddl_parse/` directory (NOT the gitignored `fuzz/corpus/`). Confirmed via `.gitignore:36` listing only `fuzz/corpus/`."
  - "TECH-DEBT entry 24 documents Pitfall 5 (body parser whitespace tokenizer inside quoted parts) as a deferred limitation — workaround is to alias inside the DuckDB host first."
requirements:
  - QID-01
  - QID-02
  - QID-03
  - QID-04
  - QID-05
  - QID-06
  - QID-07
metrics:
  duration_minutes: 6
  tasks: 3
  files_created: 6
  files_modified: 6
  tests_added: 4   # 3 regression tests + 1 sqllogictest fixture (counts as one suite)
  completed_at: "2026-05-17T16:00:00Z"
---

# Phase 64 Plan 04: Closeout — Acceptance Tests, Fuzz Seeds, Docs Summary

**One-liner:** End-to-end closeout for Phase 64: `test/sql/phase64_quoted_idents.test` covers QID-01..06 through the full extension load → parser_override → catalog → expand pipeline (47/47 sqllogictests green); three tracked fuzz seeds + a workspace integration test (3 passing tests) lock the quoted-identifier inputs in as permanent regression guards; CHANGELOG `### Fixed` bullets, REQUIREMENTS QID-01..07 with 14→21 coverage bump, ROADMAP plan-list tick, and TECH-DEBT entry 24 complete the docs/traceability surface.

## Objective Recap

Land the user-visible / integration-level proof that the Phase 64 helper (64-01), capture-site wiring (64-02), and expansion fix (64-03) actually solve the reported bugs end-to-end. Add permanent regression guards that survive future refactors and the gitignored fuzz corpus. Document the fix and ledger the requirements + deferred limitations.

## Test Counts

| Suite | Command | Result |
|-------|---------|--------|
| Workspace regression | `cargo test --test quoted_idents_regression` | **3 passing** |
| Rust lib + integration | included in `just test-all` | 838 lib + 3 regression + all other workspace tests pass |
| sqllogictest | `just test-sql` (called by `just test-all`) | **47/47** tests run, 0 failed (phase64_quoted_idents.test included) |
| Python integration | `just test-all` (test-vtab-crash + test-caret + test-adbc + test-large-view + test-multi-db + test-readonly + test-concurrent) | all green |
| Full quality gate | `just test-all` | **EXIT 0** |
| Pre-push gate | `just ci` (lint + test-all + check-fuzz + docs-check) | **EXIT 0** |

## Fixture Behaviour Map

Each scenario in `test/sql/phase64_quoted_idents.test` maps directly to one or more QID-* requirements:

| Block | QID coverage | Verifies |
|-------|-------------|----------|
| QID-01 | QID-01 | Fully-quoted FQN CREATE OR REPLACE; `semantic_view('orders_sv', ...)` returns 300.00 |
| QID-02 | QID-02 | `main."orders_sv"` and `"main".orders_sv` CREATE forms both resolve via bare-key lookup |
| QID-03 | QID-03, QID-06 | `DESCRIBE "orders_sv"` ok; `ALTER "orders_sv" RENAME TO "memory"."main"."orders_sv_v2"` (both slots quoted); old name produces unquoted error; runtime arg `semantic_view('"orders_sv_v2"', ...)` resolves; `DROP "main"."orders_sv_v2"` |
| QID-04 | QID-04 | `TABLES (o AS "memory"."main"."orders" ...)` source-table FQN; `explain_semantic_view(...)` plan contains no `"""` substring (count = 0) AND contains `"memory"."main"."orders"` (count = 1) |
| QID-05 | QID-05 | `GET_DDL('SEMANTIC_VIEW', 'orders_sv')` returns `CREATE OR REPLACE SEMANTIC VIEW orders_sv AS%`; re-CREATE from that shape round-trips |
| QID-06 | QID-06 | DESCRIBE on missing quoted FQN → "semantic view 'nonexistent_view' does not exist" (bare); DROP same; CREATE duplicate → "semantic view 'orders_sv' already exists" (bare) |

## Sqllogictest Format Notes (for future researchers)

The project's Python sqllogictest runner does **NOT** support the inline `statement error <regex>` form that some sqllogictest dialects use. Errors must be expressed as a block:

```
statement error
<SQL>
----
<expected error message substring>
```

Initial draft used inline `statement error semantic view '...' does not exist` and the runner reported `Parser Error: Failed to parse statement: statement error needs to have an expected error message`. The block-form was found by grepping `v080_transactional_ddl.test` and `error_caret_*.test` — these are the canonical style references.

The other gotcha encountered: `explain_semantic_view`'s output column is **`explain_output`**, not `plan`. Confirmed by grepping `test/sql/phase28_e2e.test` and `test/sql/phase57_introspection.test`. The QID-04 LIKE-filter queries use `WHERE explain_output LIKE '%...%'`.

## Fuzz Seed Placement

Tracked seeds go into `fuzz/seeds/fuzz_ddl_parse/` (the directory is checked into git; new files appear as untracked-then-staged in `git status`). `.gitignore:36` lists only `fuzz/corpus/` (the libfuzzer runtime cache that's bootstrapped from `fuzz/seeds/` on each run). Confirmed via `grep -n 'fuzz/corpus\|fuzz/seeds' .gitignore` returning exactly one match for `fuzz/corpus/` and zero for `fuzz/seeds/`.

The three new seed files (`seed_phase64_quoted_bare.txt`, `seed_phase64_quoted_fqn.txt`, `seed_phase64_mixed_quoting.txt`) are one-line payloads, sitting alongside the existing `seed_basic.txt`, `seed_facts.txt`, `seed_homoglyph.txt`, etc. `fuzz/fuzz_targets/fuzz_ddl_parse.rs` gained a doc-comment pointer to the new seed files + the workspace regression test.

## Regression Integration Test — Assertion Shape

`tests/quoted_idents_regression.rs` drives the three quoted-identifier inputs through `semantic_views::parse::validate_and_rewrite` and asserts:

1. **Positive:** `sql.contains("'orders_sv'")` (or `'v'` for the short-part variants) — the bare name appears as a single-quoted SQL string literal inside the rewritten `SELECT * FROM create_semantic_view_from_json('<name>', '<json>')` call.
2. **Negative:** `!sql.contains("\"memory\".\"main\".\"orders_sv\"")` (or the corresponding quoted FQN for each input) — the quoted form must NOT appear anywhere in the rewritten SQL. **This is the load-bearing assertion** — it pins the exact bug shape that Phase 64 fixes.
3. **Sanity:** `sql.starts_with("SELECT * FROM ")` — the parser_override path produces the expected prefix.

The rewrite path used by these tests does NOT require the `extension` feature — `validate_and_rewrite` for CREATE forms routes to `validate_create_body` (always compiled), then `rewrite_ddl_keyword_body` (always compiled), which emits the `SELECT * FROM create_semantic_view_from_json('<safe_name>', '<safe_json>')` shape. The `emit_native_create_sql` path is feature-gated but irrelevant here; the tests inspect the rewrite that Phase 64-02's `validate_create_body` capture-site fix produces.

## Documentation Surface

| File | Change |
|------|--------|
| `CHANGELOG.md` | `### Fixed` subsection added between existing `### Added` and `### Known limitations` under `[0.9.0]`. Two bullets: primary quoted-identifier-handling fix + triple-quoting expansion fix. No new version section — Phase 64 ships under the existing `[0.9.0]` per CLAUDE.md milestone-completion rules. |
| `.planning/REQUIREMENTS.md` | New `### Quoted Identifier Handling` subsection after `### Release` with QID-01..QID-07. Seven `[x]` traceability rows mapping each to Phase 64 (marked complete via `gsd-tools requirements mark-complete`). Coverage counter bumped `14 total` → `21 total`, `Mapped to phases: 21`. |
| `.planning/ROADMAP.md` | Phase 64 entry's plan list ticks `[x] 64-04-PLAN.md` (the other three plans were already ticked by earlier executors). `Requirements: QID-01..QID-07` was pre-populated by the planner — verified intact. |
| `TECH-DEBT.md` | New `## v0.9.0 additions` section with entry 24 documenting Pitfall 5 (body-parser TABLES-clause-with-whitespace-in-quoted-parts limitation). Includes origin, decision rationale, deferral rationale, and user-side workaround. |

## Commits

| Hash      | Type  | Subject                                                                          |
| --------- | ----- | -------------------------------------------------------------------------------- |
| `066f71f` | test  | test(64-04): add phase64_quoted_idents.test sqllogictest fixture                  |
| `bbbfe8a` | test  | test(64-04): add quoted-ident regression test + tracked fuzz seeds                |
| `30454cf` | docs  | docs(64-04): CHANGELOG Fixed bullets + QID-01..07 traceability + tech-debt 24    |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] sqllogictest `statement error` format mismatch**

- **Found during:** Task 1 first `just test-sql` run
- **Issue:** Initial draft used `statement error semantic view 'orders_sv' does not exist` (inline error-message regex form). The project's Python sqllogictest runner reported `Parser Error: test/sql/phase64_quoted_idents.test:94: Failed to parse statement: statement error needs to have an expected error message`.
- **Fix:** Rewrote every `statement error <inline>` to the block form: `statement error\n<sql>\n----\n<substring>`. Matches the convention in `v080_transactional_ddl.test` and `error_caret_*.test`.
- **Files modified:** `test/sql/phase64_quoted_idents.test` (folded into commit `066f71f` before initial commit)

**2. [Rule 3 — Blocking] `explain_semantic_view` column name**

- **Found during:** Task 1 fixture authoring (read_first step)
- **Issue:** The plan's `<behavior>` block referenced `plan` as the column name in `SELECT ... FROM explain_semantic_view(...) WHERE plan LIKE ...`. The actual column is `explain_output` (confirmed via grep of `test/sql/phase28_e2e.test` and `test/sql/phase57_introspection.test`).
- **Fix:** Used `explain_output` in the QID-04 LIKE-filter queries.
- **Files modified:** `test/sql/phase64_quoted_idents.test` (caught before commit)

**3. [Sandbox-related — non-deviation] `just test-sql` first invocation blocked by sandbox**

- **Found during:** Task 1 verify step
- **Issue:** First `just test-sql` invocation failed at `mktemp: mkstemp failed on /var/folders/.../tmp.B6vISzQMDt: Operation not permitted` — the sandbox blocks mktemp into the system temp dir that the sqllogictest runner uses.
- **Fix:** Retried with `dangerouslyDisableSandbox: true` per the harness's documented protocol. Not a code deviation.

No other deviations. Plan executed as written.

## Authentication Gates

None.

## Known Stubs

None. Every assertion in the new fixture and regression test points at concrete behaviour delivered by Plans 64-01..03; no placeholders.

## Threat Flags

None. The Phase 64 fix is a parser-normalisation correctness change; no new external input surface, no auth changes, no PII handling, no FFI changes.

## Final Acceptance — at-a-glance

- [x] All tasks in 64-04-PLAN.md executed
- [x] Each task committed individually (`066f71f`, `bbbfe8a`, `30454cf`)
- [x] `test/sql/phase64_quoted_idents.test` exists; registered in `test/sql/TEST_LIST`
- [x] `tests/quoted_idents_regression.rs` exists with 3 `#[test]` cases (all passing)
- [x] `fuzz/seeds/fuzz_ddl_parse/seed_phase64_*.txt` (3 tracked seed files) exist
- [x] CHANGELOG `[0.9.0]` section has `### Fixed` bullets describing the quoted-identifier fix
- [x] `grep -cE "QID-0[1-7]" .planning/REQUIREMENTS.md` = 14 (7 requirements + 7 traceability rows)
- [x] `grep -n "v1 requirements: 21" .planning/REQUIREMENTS.md` returns 1; `"v1 requirements: 14"` returns 0
- [x] `.planning/ROADMAP.md` Phase 64 entry shows `Requirements: QID-01..QID-07`
- [x] `just test-all` exits 0
- [x] `just ci` exits 0
- [x] 64-04-SUMMARY.md created (this file)

## Maintainer Handoff

**v0.9.0 is ready to squash-merge to `main` and tag.**

Phase 63 Plan 04 already bumped `Cargo.toml` + `description.yml` to `0.9.0`; Phase 64 ships under the same version (parser-fix bullets land in the existing `[0.9.0]` CHANGELOG section, no new tag separate from `v0.9.0`). Squash-merge of `milestone/v0.9.0` → `main` and `git tag v0.9.0` are out of scope for `execute-plan` per Phase 63 Plan 04 SUMMARY's same handoff note.

## Self-Check: PASSED

- `test/sql/phase64_quoted_idents.test` exists — FOUND.
- `fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_bare.txt` — FOUND.
- `fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_fqn.txt` — FOUND.
- `fuzz/seeds/fuzz_ddl_parse/seed_phase64_mixed_quoting.txt` — FOUND.
- `tests/quoted_idents_regression.rs` — FOUND.
- Commit `066f71f` — FOUND in `git log`.
- Commit `bbbfe8a` — FOUND in `git log`.
- Commit `30454cf` — FOUND in `git log`.
- `phase64_quoted_idents.test` in TEST_LIST — FOUND (1 match).
- `cargo test --test quoted_idents_regression` — 3 passing tests.
- `just test-sql` — 47/47 SUCCESS.
- `just test-all` — EXIT 0.
- `just ci` — EXIT 0.
