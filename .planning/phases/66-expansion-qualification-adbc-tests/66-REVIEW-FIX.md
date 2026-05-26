---
phase: 66-expansion-qualification-adbc-tests
fixed_at: 2026-05-26T00:00:00Z
review_path: .planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW.md
iteration: 1
findings_in_scope: 6
fixed: 5
skipped: 0
reclassified_post_phase66: 1
status: complete
---

# Phase 66: Code Review Fix Report

**Fixed at:** 2026-05-26
**Source review:** `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW.md`
**Iteration:** 1

**Summary:**
- Findings in scope: 6 (all Warning-tier — Info findings deferred per `fix_scope=critical_warning`).
- Fixed: 5 (WR-01, WR-03, WR-04, WR-05, WR-06).
- Skipped at original fix time: 1 (WR-02 — needs-design-decision).
- Reclassified in Phase 67 Plan 03 Task 1 to `not-a-defect` per Phase 66 CONTEXT.md D-08/D-09 (see new section below). Skipped count after reclassification: 0.

Each fix was committed atomically on a dedicated worktree branch
(`gsd-reviewfix/66-63879`) and verified via `just test-adbc-queries`
(7 passed, 0 failed both before and after each behavioural change). The
final cleanup tail fast-forwards `milestone/v0.10.0` to capture all five
commits.

## Fixed Issues

### WR-06: `f"  PASS"` is an f-string with no interpolation (lint surface)

**Files modified:** `test/integration/test_adbc_queries.py`
**Commit:** `2d59d4f`
**Applied fix:** Changed `print(f"  PASS")` to `print("  PASS")` in
`run_tests()`. Trivial textual fix; eliminates the F541 lint surface
flagged by the reviewer. No behavioural change.

### WR-05: Test runner swallows all exceptions with no traceback

**Files modified:** `test/integration/test_adbc_queries.py`
**Commit:** `bd1569f`
**Applied fix:** Added `import traceback` at module scope and inserted
`traceback.print_exc()` after the `print(f"  FAIL: …")` line in the
runner's `except Exception` handler. Also annotated the broad catch with
`# noqa: BLE001 (intentional broad catch in test runner)` to make the
deliberate-catch intent explicit. On CI failure the maintainer now sees
the file/line of the failing `_execute` / `_scalar` call directly. No
behavioural change to passing scenarios.

### WR-04: Scenario docstrings claim `SKIP_UNTIL_PLAN_02` gating that no longer exists

**Files modified:** `test/integration/test_adbc_queries.py`
**Commit:** `781ad3f`
**Applied fix:** Replaced the stale `"… SKIP_UNTIL_PLAN_02."` opening
line in scenarios 3, 4, 5, 6 with `"… ACTIVE."` Rewrote the body
narrative from "pre-migration this emits `FROM "table"`…" framing
(commit archeology) to "post-migration these sites use
`qualify_and_quote_table_ref`; a regression would fail…" framing (active
regression-guard intent). Scenario 7's docstring already framed the
post-migration semantics correctly — only the heading suffix was changed
there.

### WR-03: Dead `MIGRATION_LANDED` / `SKIP_UNTIL_PLAN_02` machinery

**Files modified:** `test/integration/test_adbc_queries.py`
**Commit:** `3324875`
**Applied fix:** Per Option A of the reviewer's suggested fix:
- Removed the `SKIP_UNTIL_PLAN_02` multi-line constant
- Removed the `MIGRATION_LANDED = True` flag
- Removed the "Skip gating" comment block introducing them
- Collapsed `_SCENARIOS` from a list of `(fn, skip_until_plan_02)`
  tuples to a plain list of scenario functions
- Removed the `skipped` counter, the `if skip_until_plan_02 and not
  MIGRATION_LANDED:` branch, and the `SKIP:` print path
- Updated the module-level docstring to drop the obsolete "Plan 01
  scaffolding ahead of Plan 02" narrative and the D-09 manual baseline
  gate section (both transitional state that no longer applies)
- Adjusted the exit-code documentation to reflect the simplified runner
  (no longer mentions skipped scenarios)

22 lines added, 67 lines removed. Verified `just test-adbc-queries`
still reports `7 passed, 0 failed` post-cleanup.

### WR-01: Scenario 6 assertion does not prove materialization routing actually occurred

**Files modified:** `test/integration/test_adbc_queries.py`
**Commit:** `7669878`
**Applied fix: requires human verification**

Implemented the reviewer's suggested fix:

1. Changed the `agg.daily_revenue` seed values from
   `('US', 100.00), ('EU', 200.00)` — which happen to coincide with what
   `SUM(s.amount) GROUP BY region` would yield from raw expansion — to
   sentinels `('US', -1.00), ('EU', -2.00)` that could never arise from
   raw expansion.
2. Replaced the `COUNT(*) == 2` assertion with a scalar fetch of
   `total` WHERE `region = 'US'`, asserting `float(val) == -1.0`. The
   comparison is done via `float()` rather than `Decimal("-1.00")` to
   avoid coupling the test to the driver's exact Decimal precision
   choice (the metric is declared `DECIMAL(18,2)` but the cursor may
   return a Python float or a `decimal.Decimal` depending on ADBC
   driver version).

This is a behavioural change (the asserted value moves from "row count"
to "specific scalar"). It was verified post-fix via `just
test-adbc-queries` returning `7 passed, 0 failed`. The "requires human
verification" note exists because the logic change reframes scenario 6
from a count-only sanity check to a value-content regression guard — a
maintainer should confirm the sentinel approach matches their long-term
intent for this scenario.

## Reclassified Post-Phase-66 (not a defect)

### WR-02: All scenarios assert row count only, not row content

**File:** `test/integration/test_adbc_queries.py:177-182, 220-225, 264-269, 315-320, 366-373, 441-446, 495-500`
**Original disposition:** `skipped: needs-design-decision` (Phase 66 fix pass).
**Reclassified disposition:** `not-a-defect` (Phase 67 Plan 03 Task 1).

**Disposition (post-Phase-66 reclassification):** `not-a-defect`.

**Rationale:** Per Phase 66 CONTEXT.md D-08 and D-09, the ADBC integration tests in `test_adbc_queries.py` are catalog-resolution regression guards, not value-correctness tests. Their purpose is to catch the failure class `Catalog Error: Table with name X does not exist!` arising from unqualified `quote_table_ref` emission on a per-call Connection whose default catalog/schema search path diverges from `memory.main`. Count-only assertions are sufficient for that purpose: if a regression breaks qualified emission, the count-only assertion fails with a Catalog Error before the count is even computed; if a regression returns wrong rows with right cardinality (the case WR-02 worries about), that is a value-correctness regression and belongs to the SQL-layer fixture set, not the ADBC catalog-resolution guard.

The value-correctness regression surface that WR-02 names is covered (or will be) by `test/sql/phase67_qualified_emission.test` (Phase 67 Plan 01 A1) and the existing fixture-level behavioural tests in `phase47_semi_additive.test` and `phase48_window_metrics.test`. Per the project's stated testing strategy ("primarily test for expected re-written SQL", see SCOPE.md), value-correctness lives at the sqllogictest layer not the ADBC layer.

Reclassified in Phase 67 Plan 03 Task 1.

---

**Historical context (preserved from original skip):**

**Reason:** `needs-design-decision`
**Original issue:** Every scenario uses `COUNT(*)` only. Scenarios 4
(semi-additive `MIN_BY`) and 5 (window `AVG(...) OVER`) in particular
could pass with completely wrong row content but the right cardinality.

**Why skipped:**
1. The fix scope explicitly says: "If a tightening would risk breaking
   passing scenarios, document it in REVIEW-FIX.md as
   `skipped: needs-design-decision` rather than gambling."
2. Strengthening assertions for scenarios 4 and 5 requires deriving the
   expected value from the metric semantics. For scenario 4
   `MIN_BY(qty, snapshot_date)` would yield `latest_qty = 10` for
   `WH1` (earliest snapshot 2026-01-01 has qty=10) — but the metric is
   named `latest_qty` and is computed at the *minimum* snapshot_date,
   suggesting either the metric name is misleading or the
   reviewer's example value is mislabelled.  This wants a design
   review before a value assertion is hard-coded.
3. For scenario 5 the window frame
   `PARTITION BY EXCLUDING event_time ORDER BY event_time ASC NULLS LAST`
   over `total_amount = SUM(amount)` is non-trivial; the running-average
   value depends on whether `SUM` is computed per-row or per-group, which
   in turn depends on the v0.7.0 window-expansion semantics. The
   reviewer did not specify the expected value, only that one should
   exist.
4. WR-01 (already applied) covers the strongest case (scenario 6
   silent fall-back to raw expansion). The remaining count-only
   scenarios are weakened but not vacuous: a regression that returns
   the wrong column or wrong join would still error out on
   `Catalog Error`, which is the primary thing this test file is
   guarding against.

**Recommended follow-up (superseded by reclassification above):** the original recommendation was to open a tech-debt entry to land per-scenario value assertions in a follow-up commit. With the reclassification per D-08/D-09, value-correctness is owned by the sqllogictest layer (Phase 67 Plan 01 A1 + existing phase47/48 fixtures) and the ADBC layer correctly remains count-only.

## Skipped Issues

_(empty after Phase 67 Plan 03 reclassification of WR-02)_

---

_Fixed: 2026-05-26_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
