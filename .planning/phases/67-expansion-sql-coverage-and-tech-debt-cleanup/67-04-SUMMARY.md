---
phase: 67-expansion-sql-coverage-and-tech-debt-cleanup
plan: 04
subsystem: integration-test-metric-naming-fix
tags: [test, naming, semi-additive, min-by, adbc]
requires:
  - Phase 67 CONTEXT.md D-15 (test-rename-first heuristic, escalation gate)
  - Phase 66 REVIEW-FIX WR-02 (which surfaced the latest_qty / MIN_BY ambiguity)
provides:
  - Scenario 4 metric name aligned with standard DuckDB MIN_BY semantics (`earliest_qty`)
  - Recoverable investigation log demonstrating Case A classification under D-15
affects:
  - test/integration/test_adbc_queries.py
tech_stack:
  added: []
  patterns:
    - Standard DuckDB / SQL aggregate semantics (`MIN_BY(arg, val)` returns arg at row of minimum val)
key_files:
  created: []
  modified:
    - test/integration/test_adbc_queries.py
decisions:
  - Classified as Case A per D-15 — standard DuckDB `MIN_BY` semantics confirmed (no project-side override exists; semi_additive.rs has no MIN_BY-specific handling and delegates to the native aggregate), so the cheapest fix is renaming the metric.
  - Rename applied to both occurrences in scenario 4 only (`grep -rn "latest_qty" test/ src/ docs/` returned only the two scenario-4 lines before the rename; zero hits after).
  - No code path or emission semantics changed — metric identifiers are arbitrary user labels.
metrics:
  duration_minutes: 18
  completed_at: 2026-05-27T00:13:00Z
  tasks_total: 2
  tasks_completed: 2
---

# Phase 67 Plan 04: `latest_qty` → `earliest_qty` Metric Rename Summary

Scenario 4 of `test/integration/test_adbc_queries.py` defined a semi-additive metric `i.latest_qty AS MIN_BY(i.qty, i.snapshot_date)` whose name was misleading: standard DuckDB `MIN_BY(arg, val)` returns `arg` at the row of **minimum** `val`, so the value computed is the qty at the **earliest** snapshot, not the latest. Per Phase 67 CONTEXT.md D-15, investigation confirmed standard semantics apply (Case A) and the metric was renamed to `earliest_qty` in both occurrences (DDL and assertion arg). `just test-adbc-queries` exits 0 with 7/7 PASS; `just test-all` exits 0.

## Commits

| # | Hash      | Type | Subject                                                                            |
| - | --------- | ---- | ---------------------------------------------------------------------------------- |
| 1 | `338cf7a` | fix  | `fix(67): rename latest_qty -> earliest_qty per MIN_BY semantics (C4 / D-15)`      |

## Case Classification (D-15)

**Case A — standard semantics confirmed; rename is the correct fix.**

### Investigation evidence

Investigation log: `/tmp/claude/c4_investigation_67_04.log` (kept on disk for traceability).

1. **Scenario 4 DDL** (`test/integration/test_adbc_queries.py:279`, pre-rename):
   ```
   METRICS (i.latest_qty AS MIN_BY(i.qty, i.snapshot_date))
   ```
   Test data: WH1 = (2026-01-01, qty=10), (2026-01-02, qty=15); WH2 = (2026-01-01, qty=20), (2026-01-02, qty=25). Assertion: `rows == 2` (row-count only, no value check).

2. **Standard / DuckDB `MIN_BY` semantics**: `min_by(arg, val)` returns `arg` at the row where `val` is the **minimum** in the group. For `MIN_BY(qty, snapshot_date)` per warehouse this returns the qty at the **earliest** snapshot_date — i.e. WH1 → 10, WH2 → 20.

3. **Project-side overrides ruled out**:
   - `grep -rn "MIN_BY|min_by" src/` → **0 hits**. The project does not register or rewrite the `MIN_BY` aggregate.
   - `grep -n "MIN_BY|min_by|earliest|latest" src/expand/semi_additive.rs` → **0 hits**. The semi-additive expansion path emits the aggregate function verbatim through to DuckDB.
   - Conclusion: `MIN_BY` resolution is delegated to DuckDB's native aggregate; standard semantics hold.

4. **Repo-wide reference scan** (defense-in-depth before renaming):
   - `grep -rn "latest_qty" test/ src/ docs/` → only **two** hits, both in `test/integration/test_adbc_queries.py` (lines 279 and 287). No other scenario, fixture, source file, or doc references this name.

Together these establish Case A: the metric label is misleading, no project-side semantics override makes it correct, and the rename has no blast radius outside the two scenario-4 lines.

### Why not Case B

Case B would require evidence that the project's `MIN_BY` returns the latest-snapshot value despite the standard name. No such evidence exists — neither in code (no MIN_BY handling at all in src/), nor in test expectations (the assertion is row-count-only and unchanged by the value chosen).

## Task 1 — Investigation

**Files modified:** (none)

Read scenario 4 (`test/integration/test_adbc_queries.py:242-291`) in full, confirmed metric DDL and test data, reasoned about standard `MIN_BY` semantics, verified by grep that the project has no override path for the aggregate, and confirmed `latest_qty` appears nowhere else in the repo. Classified as Case A per D-15. Logged the conclusion to `/tmp/claude/c4_investigation_67_04.log`.

## Task 2 — Rename + verify

**Files modified:** `test/integration/test_adbc_queries.py`

### Diff (full)

```diff
@@ -276,7 +276,7 @@
             CREATE SEMANTIC VIEW inv_view AS
               TABLES (i AS staging.inventory PRIMARY KEY (id))
               DIMENSIONS (i.warehouse AS i.warehouse)
-              METRICS (i.latest_qty AS MIN_BY(i.qty, i.snapshot_date))
+              METRICS (i.earliest_qty AS MIN_BY(i.qty, i.snapshot_date))
             """,
         )
         conn.commit()
@@ -284,7 +284,7 @@
         rows = _scalar(
             conn,
             "SELECT COUNT(*) FROM semantic_view('inv_view', "
-            "dimensions := ['warehouse'], metrics := ['latest_qty'])",
+            "dimensions := ['warehouse'], metrics := ['earliest_qty'])",
         )
         assert rows == 2, f"expected 2 rows, got {rows}"
```

### Verification

| Check | Command | Result |
|-------|---------|--------|
| No `latest_qty` remains | `grep -c "latest_qty" test/integration/test_adbc_queries.py` | `0` |
| `earliest_qty` present in both sites | `grep -c "earliest_qty" test/integration/test_adbc_queries.py` | `2` |
| Other scenarios untouched | `grep -rn "latest_qty" test/ src/ docs/` | `0 hits` |
| ADBC scenario suite | `uv run test/integration/test_adbc_queries.py` | `7 passed, 0 failed` (scenario 4 included) |
| Full quality gate | `just test-all` | exit code `0` |

The `just test-all` run includes Rust unit tests, proptest, sqllogictest, and DuckLake CI tests — the final lines of the log show `SUMMARY: 12/12 tests passed` for the read-only / lifetime test suite and `PASS: exactly one concurrent CREATE committed` from the concurrent-DDL integration suite, with the runner exiting 0 overall.

## Deviations from Plan

None — plan executed exactly as written (Case A path).

The only non-plan setup work was infrastructure restoration unrelated to the rename itself: the fresh worktree was missing `cpp/include/duckdb.cpp` and `cpp/include/duckdb.hpp` (both gitignored, vendored DuckDB amalgamation), the `extension-ci-tools` submodule was uninitialised, and `make configure` had not yet run. These were one-shot setup steps to enable the build/test pipeline; the rename itself is the only working-tree change in the commit.

## Threat Flags

None. The rename touches only a user-supplied identifier in a test fixture; no SQL emission semantics, no FFI surface, no new trust boundary.

## Self-Check: PASSED

- File `test/integration/test_adbc_queries.py` exists and contains `earliest_qty` at lines 279, 287; zero remaining `latest_qty` occurrences.
- Commit `338cf7a` exists on HEAD (`git log -1 --oneline` confirms).
- SUMMARY.md created at `.planning/phases/67-expansion-sql-coverage-and-tech-debt-cleanup/67-04-SUMMARY.md`.
- No STATE.md or ROADMAP.md modifications (orchestrator owns those writes).
