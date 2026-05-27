---
phase: 66-expansion-qualification-adbc-tests
plan: 01
subsystem: test-scaffolding
tags: [adbc, integration-test, regression-guard, expand-ctx-02]
requires:
  - milestone/v0.10.0 HEAD (Phase 65 closeout — per-call Connection(*context.db) model)
  - test_ducklake_helpers (get_ext_dir / get_extension_path)
  - adbc-driver-manager>=1.10 (bundled adbc_driver_duckdb inside duckdb==1.5.2 wheel)
provides:
  - test/integration/test_adbc_queries.py — 7-scenario ADBC end-to-end harness for EXPAND-CTX-02
  - just test-adbc-queries recipe — added to test-all aggregate
  - D-09 baseline-gate documentation embedded inline in test module docstring
affects:
  - Justfile (test-adbc-queries recipe + test-all dependency list)
tech-stack:
  added: []
  patterns:
    - "Skip-by-flag pattern: SKIP_UNTIL_PLAN_02 constant + MIGRATION_LANDED boolean lets Plan 02 un-skip 5 scenarios via a single one-line edit"
    - "Homegrown _SCENARIOS list-of-tuples runner (no pytest dependency) mirrors test_adbc_transactions.py's run_tests() shape"
key-files:
  created:
    - test/integration/test_adbc_queries.py
  modified:
    - Justfile
decisions:
  - "Scenario 7 (multi-DB ATTACH + FACTS) gated by SKIP_UNTIL_PLAN_02 in addition to scenarios 3-6 because it exercises the FACTS expansion path on the attached DB's table — same unmigrated quote_table_ref at sql_gen.rs:181,224,244. Final accounting: 2 PASS / 5 SKIP / 0 FAIL on milestone/v0.10.0 HEAD (matches plan acceptance criteria)."
  - "Used a list-of-tuples runner (_SCENARIOS) rather than introducing a pytest dependency. Each scenario is a stand-alone function that receives (extension_path, ext_dir, tmp_path) — matches the test_adbc_transactions.py style and keeps the file pytest-free."
  - "Scenario 6 (materialization routing) pre-populates the agg.daily_revenue target with the expected aggregate rows so that exact-match routing returns rows once the materialization.rs:157 emission is qualified. Without the seed rows the routed query would PASS on pre-migration (zero rows from unqualified table fails to bind) AND PASS on post-migration (zero rows from empty agg table) — false negative. Seeding makes the migration's effect observable."
metrics:
  duration_minutes: ~20
  tasks_completed: 2
  files_changed: 2
  lines_added: 567
  completed_date: 2026-05-26
---

# Phase 66 Plan 01: ADBC End-to-End Query Test Scaffolding Summary

ADBC end-to-end query regression harness lands ahead of Plan 02's
expansion-path migration; suite reports 2 PASS / 5 SKIP / 0 FAIL on
milestone/v0.10.0 HEAD and is wired into `just test-all`.

## Tasks Completed

| Task | Name                                                        | Commit  | Files                                          |
| ---- | ----------------------------------------------------------- | ------- | ---------------------------------------------- |
| 1    | Create test_adbc_queries.py with 7 scenario functions       | 1e7066f | test/integration/test_adbc_queries.py (+556)   |
| 2    | Add test-adbc-queries justfile recipe + wire into test-all  | 7cf627d | Justfile (+11/-1)                              |

Task 3 in the plan is a `checkpoint:human-verify` gate (no files
modified). Its automated portion — `just build`, `just test-adbc-queries`,
`just test-all` — was run by the executor; results recorded under
"Verification Results" below. The user's resume-signal is still
required to advance to Plan 02, per the plan's gate semantics.

## What Was Built

- **`test/integration/test_adbc_queries.py`** (556 LOC)
  - PEP 723 inline script header pinning `duckdb==1.5.2`,
    `adbc-driver-manager>=1.10`, `pyarrow>=16`; mirrors
    `test_adbc_transactions.py:1-13` exactly.
  - Helpers `_connect_adbc`, `_execute`, `_scalar` mirrored from
    `test_adbc_transactions.py:60-93`; extension bootstrap via
    `FORCE INSTALL '<path>'` + `LOAD semantic_views` on the ADBC
    connection (matches the existing transactional-DDL test).
  - 7 scenario functions per CONTEXT.md D-08:
    1. `test_main_path_default_schema` — view in `memory.main`. **ACTIVE**.
    2. `test_main_path_non_default_schema` — `staging.t` base table.
       **ACTIVE** (main path was wired by Phase 64).
    3. `test_facts_non_default_schema` — FACTS on `staging.sales`.
       **SKIP** (guards `sql_gen.rs:181,224,244`).
    4. `test_semi_additive_non_default_schema` — `MIN_BY(qty, snapshot_date)`
       on `staging.inventory`. **SKIP** (guards `semi_additive.rs:195,220,238`).
    5. `test_window_non_default_schema` — `ROW_NUMBER() OVER (...)` on
       `staging.events`. **SKIP** (guards `window.rs:156,181,199`).
    6. `test_materialization_routing_non_default_schema_target` — routes
       to `agg.daily_revenue`. **SKIP** (guards `materialization.rs:157`).
    7. `test_attach_facts_path` — `ATTACH 'other.duckdb' AS db2`, creates
       `db2.main.attached_view` with FACTS metric. **SKIP** (cross-DB
       exercises the same FACTS path).
  - Skip gating via `SKIP_UNTIL_PLAN_02` constant + `MIGRATION_LANDED = False`
    boolean. Plan 02 flips a single line (`MIGRATION_LANDED = True`)
    at the same commit that lands the seven `qualify_and_quote_table_ref`
    migrations in `src/expand/`.
  - D-09 manual baseline gate documented in the module docstring:
    flipping `MIGRATION_LANDED = True` on pre-migration HEAD must
    reproduce `Catalog Error: Table with name X does not exist!` on
    scenarios 3-7; that failing output is the acceptance evidence
    record for Plan 02.
  - Tear-down via `tempfile.TemporaryDirectory(prefix="sv_adbc_q_")`
    per scenario; CLAUDE.md Rule 2 sandbox-bypass note inline.
  - Runner: `_SCENARIOS` list of `(fn, skip_until_plan_02)` tuples;
    `run_tests()` prints `RUN/PASS/FAIL/SKIP` and accounts skipped
    scenarios separately from passes and failures.

- **`Justfile`** (+11 / -1)
  - New `test-adbc-queries: build` recipe at Justfile:121 (immediately
    after `test-adbc: build` at Justfile:111), with a documenting
    comment block describing the regression scope and the
    `SKIP_UNTIL_PLAN_02` gate.
  - `test-all` aggregate at Justfile:159 (was :149 — line shift from
    the new recipe block) now includes `test-adbc-queries` immediately
    after `test-adbc`. Aggregate runs the new recipe as part of the
    standard quality gate.

## Verification Results

Plan acceptance criteria (Task 1):

- [x] File exists; `wc -l` reports 556 LOC (>= 250).
- [x] PEP 723 header matches `test_adbc_transactions.py` header exactly.
- [x] `grep -c "^def test_"` returns exactly 7.
- [x] `SKIP_UNTIL_PLAN_02` constant defined once at line 92; referenced
      from 5 SKIP scenario docstrings (scenarios 3-7) plus the runner.
- [x] Test exit code 0; stdout reports `Results: 2 passed, 0 failed, 5 skipped`.
- [x] D-09 manual-gate docstring names `MIGRATION_LANDED = True` as
      the un-skip flag and references the expected
      `Catalog Error: Table with name X does not exist!` failure mode
      (module docstring, "D-09 manual baseline gate" section).
- [x] No bare `tail` on the `uv run` invocation; verification redirects
      output to `/tmp/claude/adbc_queries_baseline.log` first per
      CLAUDE.md Rule 1.

Plan acceptance criteria (Task 2):

- [x] `grep -n "^test-adbc-queries:" Justfile` returns exactly one
      match (Justfile:121).
- [x] `grep -E "^test-all:" Justfile` shows `test-adbc test-adbc-queries`
      adjacent in dependency list.
- [x] `just --list | grep test-adbc-queries` lists the recipe.
- [x] `just --dry-run test-adbc-queries` shows `make debug` (build
      dependency resolution) and `uv run test/integration/test_adbc_queries.py`.

Task 3 automated checkpoint gates:

- [x] `just build` exit 0 (`/tmp/claude/build.log`).
- [x] `just test-adbc-queries` exit 0; reports 2 PASS / 5 SKIP / 0 FAIL
      (`/tmp/claude/test_adbc_queries.log`).
- [x] `just test-all` exit 0; full aggregate (Rust unit + sqllogictest +
      DuckLake CI + vtab-crash + caret + ADBC transactions + ADBC queries
      + large-view + multi-DB + readonly + concurrent) stays green
      (`/tmp/claude/test_all.log`).

Human verification still pending (resume-signal for Plan 02 advance);
the user runs the same three commands locally and confirms the same
outcomes.

## Architecture / Decisions

- **Skip-by-flag pattern over per-test pytest.mark.skip**. The file is
  not pytest-driven (mirrors `test_adbc_transactions.py`'s homegrown
  runner), so the skip gate is a list-tuple flag rather than a pytest
  marker. Plan 02 un-skips all 5 gated scenarios in a single one-line
  diff (`MIGRATION_LANDED = True`).
- **Scenario 7 (multi-DB ATTACH) gated**. Originally Task 1 acceptance
  said "Final accounting: 2 PASS, 5 SKIP, 0 FAIL" — scenario 7 belongs
  in the SKIP set because it exercises the FACTS expansion path on
  `db2.main.sales`, same unmigrated `quote_table_ref` at
  `sql_gen.rs:181,224,244`. The plan's acceptance criteria explicitly
  states this re-evaluation.
- **Scenario 6 materialization seeding**. The agg target table is
  pre-populated with the expected aggregate rows so the migration's
  effect is observable. Without the seed rows the routed query would
  PASS pre-migration (zero rows from a fail-to-resolve unqualified
  reference) AND PASS post-migration (zero rows from an empty agg
  table) — a false negative. Seeding makes pre/post divergence visible.
- **In-process bootstrap** (no subprocess). Per RESEARCH.md §R-03 the
  Phase 65 closeout retired the OverrideContext-driven in-process
  reopen-hang, so the subprocess pattern Phase 63 used is no longer
  needed. The new file uses the same single-process model as
  `test_adbc_transactions.py`.

## Deviations from Plan

None — the plan was executed exactly as written.

The line-number reference in the plan's Task 2 ("Append a new recipe
immediately after the existing test-adbc recipe (currently lines
107-112). … Update the test-all aggregate recipe at justfile:149")
landed at `Justfile:121` (recipe) and `Justfile:159` (aggregate) due
to the inserted comment block; the structural placement (immediately
after `test-adbc`, immediately before `test-large-view`) matches the
plan's intent.

## Auth Gates / Checkpoints

- Task 3 is a `checkpoint:human-verify` gate. The executor ran the
  automated portion (build + test-adbc-queries + test-all) and
  recorded results above. The user's resume-signal ("approved" or
  issue description) is still required before advancing to Plan 02.

## Known Stubs

None. Scenarios 3-7 are gated by `SKIP_UNTIL_PLAN_02` — this is the
plan's intentional design (per CONTEXT.md D-09), not a stub. Plan 02
will flip the gate as part of its own migration commit. The skip
state is fully documented inline in the test file's module docstring.

## Self-Check: PASSED

Files verified present:

- [x] `test/integration/test_adbc_queries.py` (556 LOC)
- [x] `Justfile` (modified — recipe + test-all aggregate updated)

Commits verified in git log:

- [x] `1e7066f test(66-01): add ADBC end-to-end query test scaffolding`
- [x] `7cf627d chore(66-01): wire test-adbc-queries recipe into justfile + test-all aggregate`

Test outcomes verified:

- [x] `uv run test/integration/test_adbc_queries.py` → `2 passed, 0 failed, 5 skipped`
- [x] `just test-adbc-queries` exit 0
- [x] `just test-all` exit 0 (aggregate including the new recipe stays green)
