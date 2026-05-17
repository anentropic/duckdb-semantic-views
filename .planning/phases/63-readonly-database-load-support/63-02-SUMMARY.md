---
phase: 63
plan: 02
subsystem: test infrastructure (Python integration + sqllogictest fixture + justfile wiring)
tags: [readonly, load, tests, sqllogictest, justfile, integration]
dependency-graph:
  requires:
    - "63-01 (read-only LOAD core: src/lib.rs + src/catalog.rs)"
    - "build/debug/semantic_views.duckdb_extension (built by Plan 01)"
  provides:
    - "test/integration/test_readonly_load.py — three-function Python integration test covering RO-01..RO-05"
    - "test/sql/readonly_load.test — writable bootstrap smoke fixture (full RO scenarios delegated)"
    - "justfile::test-readonly recipe + test-all dependency"
  affects:
    - "test/sql/TEST_LIST (added readonly_load.test entry — required for fixture to actually run)"
    - "Justfile (new recipe + comment update)"
tech-stack:
  added: []
  patterns: ["PEP-723 inline metadata for uv-runnable scripts", "subprocess-bootstrap pattern for in-process RW→RO reopen (workaround Phase 62 OverrideContext leak — see deferred-items.md)", "case-insensitive substring assertion on DuckDB error text for version-resilience"]
key-files:
  created:
    - "test/integration/test_readonly_load.py (338 lines)"
    - "test/sql/readonly_load.test (59 lines)"
    - ".planning/phases/63-readonly-database-load-support/63-02-SUMMARY.md"
  modified:
    - "Justfile (+13 lines: new test-readonly recipe + test-all chain entry + comment)"
    - "test/sql/TEST_LIST (+1 line: readonly_load.test entry — Rule 3 deviation)"
decisions:
  - "Add readonly_load.test entry to test/sql/TEST_LIST (Rule 3 — blocking). The plan's Task 2 acceptance criterion 'just test-sql output includes readonly_load.test in the executed set' could not be met without this; the runner gates on TEST_LIST membership."
  - "Used `Justfile` (capital J) as tracked git path because macOS case-insensitive FS exposes the file under both names but git tracks the capitalised form."
  - "Skipped `just ci` per plan §Task 4 escape hatch (docs-check expected to fail until Plan 03 lands the new explanation/reference cross-references). Plan 04 (release) is the gate that re-runs `just ci`."
metrics:
  duration: "~10 min (resumed after kill mid-Task 2; Task 1 already in HEAD as 080e5ea)"
  completed: 2026-05-15
---

# Phase 63 Plan 02: Read-Only LOAD Test Infrastructure Summary

End-to-end test infrastructure for Plan 01's read-only LOAD support: a three-function Python integration test (`test_fresh_readonly_empty_list`, `test_bootstrapped_readonly_query_works`, `test_readonly_ddl_fails`) covering RO-01..RO-05; a sqllogictest smoke fixture pinning the writable bootstrap path (full RO scenario coverage delegated to the Python test per Wave 0 spike outcome — `python_runner/` lacks `load <path> readonly`); and a new `just test-readonly` recipe wired into the standard `test-all` chain.

## What Shipped

**Files created (2):**

- `test/integration/test_readonly_load.py` (338 lines) — PEP-723 script with three test functions modelled on `test/integration/test_multi_db_isolation.py`. Uses `duckdb.connect(path, read_only=True)`, the canonical Python-API kwarg, to reopen bootstrapped DBs read-only. The `bootstrap_in_subprocess()` helper (added by Plan 01's executor for the original Task 1 commit `080e5ea`) works around the Phase 62 `OverrideContext` in-process RW→RO hang documented in `deferred-items.md`.

- `test/sql/readonly_load.test` (59 lines) — Writable smoke fixture. Header documents the Wave 0 spike outcome (`grep -rn 'readonly' python_runner/` returned zero results; runner has no `load <path> readonly` directive) and points readers to the Python test for full RO coverage. The fixture exercises the writable bootstrap path that the read-only integration test depends on; if this fixture breaks, the integration test breaks too — earlier signal in CI.

**Files modified (2):**

- `Justfile` (+13 lines) — New `test-readonly` recipe between `test-multi-db` and `test-concurrent`. Updated `test-all` dependency list to include `test-readonly`. Updated the `test-all` comment.

- `test/sql/TEST_LIST` (+1 line) — Added `test/sql/readonly_load.test` so the runner picks it up. **Rule 3 deviation: not in plan**, but required for Task 2's acceptance criterion ("output includes `readonly_load.test` in the executed set with `OK`") to be met. Without this entry the fixture is silently ignored.

## Verification

| Step | Command | Result |
|------|---------|--------|
| 1 | `just test-readonly` | `SUMMARY: 3/3 tests passed` (PASS for `test_fresh_readonly_empty_list`, `test_bootstrapped_readonly_query_works`, `test_readonly_ddl_fails`) |
| 2 | `just test-sql` | `46 tests run, 0 failed`; `readonly_load.test` shown as `SUCCESS` in the run set |
| 3 | `just test-all` | exits 0; full chain green including new `test-readonly` step at the expected position; no regressions in `test-multi-db`, `test-concurrent`, `test-adbc`, `test-ducklake-ci`, etc. |
| 4 | `just ci` | **NOT RUN** — deferred to Plan 04 per plan §Task 4 (Plan 03 must land docs cross-references first or `docs-check` fails) |
| 5 | `git branch --show-current` | `milestone/v0.9.0` |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] Missing `test/sql/TEST_LIST` entry for `readonly_load.test`**

- **Found during:** Task 2 verification (`just test-sql` output did NOT include `readonly_load.test`)
- **Issue:** The Python sqllogictest runner gates execution on `test/sql/TEST_LIST` membership; files dropped into `test/sql/` are not auto-discovered. The plan's Task 2 specified only file creation, not the TEST_LIST update — but the acceptance criterion ("output includes `readonly_load.test` in the executed set with `OK` / passing status (not skipped)") cannot be met without it.
- **Fix:** Appended `test/sql/readonly_load.test` to `test/sql/TEST_LIST` in the same commit as the fixture itself (`dc4bb1c`).
- **Files modified:** `test/sql/TEST_LIST`
- **Commit:** `dc4bb1c`

### Documented Deferrals (per plan)

- **`just ci` not run.** Plan §Task 4 explicitly authorises this when only `docs-check` would fail because Plan 03 has not yet added the `_explanation-txn-ddl-readonly` label and per-reference notes. Plan 04 (release) is the gate that runs `just ci` after Plan 03 lands. `just test-all` (the unconditional CLAUDE.md quality gate) is green.

## Authentication Gates

None.

## Branch + Hand-off

- **Branch:** `milestone/v0.9.0` (verified before each commit)
- **Commits added by this plan:**
  - `080e5ea` — `test(63-02): add Python integration test for read-only LOAD` (committed by previous executor before kill; Task 1)
  - `dc4bb1c` — `test(63-02): add sqllogictest smoke fixture for read-only LOAD bootstrap` (Task 2 + TEST_LIST deviation)
  - `babdc52` — `chore(63-02): add test-readonly recipe and thread into test-all` (Task 3)
- **Hand-off:** Plan 03 (docs + example) can build against the working extension binary at `build/debug/semantic_views.duckdb_extension`. Plan 04 (release) re-runs `just test-all` and `just ci` post-Plan 03 as the milestone gate.

## Self-Check: PASSED

Verified files exist:
- FOUND: `test/integration/test_readonly_load.py` (338 lines)
- FOUND: `test/sql/readonly_load.test` (59 lines)
- FOUND: `Justfile` (modified, +13 lines)
- FOUND: `test/sql/TEST_LIST` (modified, +1 line)
- FOUND: `.planning/phases/63-readonly-database-load-support/63-02-SUMMARY.md` (this file)

Verified commits:
- FOUND: `080e5ea` — Task 1 Python integration test
- FOUND: `dc4bb1c` — Task 2 sqllogictest fixture + TEST_LIST entry
- FOUND: `babdc52` — Task 3 justfile recipe
