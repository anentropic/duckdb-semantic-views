---
phase: 66-expansion-qualification-adbc-tests
reviewed: 2026-05-26T00:00:00Z
depth: standard
files_reviewed: 7
files_reviewed_list:
  - src/expand/sql_gen.rs
  - src/expand/semi_additive.rs
  - src/expand/window.rs
  - src/expand/materialization.rs
  - test/sql/phase57_introspection.test
  - test/integration/test_adbc_queries.py
  - _notes/error_with_adbc.md
findings:
  critical: 0
  warning: 6
  info: 5
  total: 11
status: issues_found
---

# Phase 66: Code Review Report

**Reviewed:** 2026-05-26
**Depth:** standard
**Files Reviewed:** 7
**Status:** issues_found

## Summary

The migration of expand-path call sites from `quote_table_ref` to `qualify_and_quote_table_ref` is mechanically complete and consistent across all four target Rust modules: 9 of 9 expected sites are migrated (sql_gen.rs lines 179, 222, 242, 497, 528, 548; semi_additive.rs lines 195, 220, 238; window.rs lines 156, 181, 199; materialization.rs line 163). The Rust changes themselves are low-risk pass-through refactors threading `def: &MgrDef` through, with new unit tests in `sql_gen.rs` (`test_base_table_qualified_with_catalog_schema`, `test_join_table_qualified_with_catalog_schema`, `test_base_table_qualified_schema_only`, `test_base_table_unqualified_when_no_catalog_schema`, `test_already_qualified_table_not_double_qualified`) directly covering the qualification contract. The phase57 sqllogictest fixture is updated to expect the new fully-qualified `FROM "memory"."main"."p57_agg_region"` output.

The ADBC integration test harness, however, has multiple quality defects: the post-migration cleanup of the `MIGRATION_LANDED` / `SKIP_UNTIL_PLAN_02` skip-gating machinery was never performed (dead control flow / stale docstrings); the scenarios assert only row counts rather than row content, which materially weakens their value as regression guards (scenario 6 in particular could pass even if materialization routing silently fell back to raw expansion); error reporting in the runner drops tracebacks; and the runner masks all `Exception` subclasses with a single bare except that returns a printed string only.

No correctness bugs in the migration itself. No security vulnerabilities. All findings are quality-tier and concentrated in the integration test scaffolding.

## Warnings

### WR-01: Scenario 6 assertion does not prove materialization routing actually occurred

**File:** `test/integration/test_adbc_queries.py:441-446`
**Issue:** The materialization-routing scenario seeds `agg.daily_revenue` with `(US, 100.00), (EU, 200.00)` — the exact same totals that `SUM(s.amount) GROUP BY region` over the raw `sales` table would produce. The test then asserts only `COUNT(*) == 2`. Whether the query (a) successfully routes to `agg.daily_revenue`, (b) silently falls back to expanding the raw `sales` table, or (c) returns the materialization's seeded rows verbatim — all three return 2 rows. The intended regression guard (catching unqualified emission from `materialization.rs:163`) is preserved (pre-migration would fail catalog resolution), but the scenario does not prove the post-migration code path is the materialization path rather than raw expansion. A future regression that disables materialization routing entirely would pass this test.
**Fix:** Seed the materialization table with a sentinel value that would never arise from raw expansion (e.g. `('US', -1.00), ('EU', -2.00)`) and assert the sentinel is returned:
```python
_execute(conn, "INSERT INTO agg.daily_revenue VALUES ('US', -1.00), ('EU', -2.00)")
# ...
with conn.cursor() as cur:
    cur.execute("SELECT total FROM semantic_view('rev_view', dimensions := ['region'], metrics := ['total']) WHERE region = 'US'")
    val = cur.fetchone()[0]
assert val == Decimal("-1.00"), f"expected sentinel -1.00 from materialization, got {val} (likely raw expansion)"
```

### WR-02: All scenarios assert row count only, not row content

**File:** `test/integration/test_adbc_queries.py:177-182, 220-225, 264-269, 315-320, 366-373, 441-446, 495-500`
**Issue:** Every scenario uses `_scalar(conn, "SELECT COUNT(*) FROM semantic_view(...)")` and asserts the count. A bug that returns the wrong rows but the right cardinality (e.g. wrong dimension grouping, wrong table joined, semi-additive snapshot logic broken, window-frame off-by-one) would not be detected. For scenarios 4 (semi-additive `MIN_BY`) and 5 (window `AVG(...) OVER`) this is especially weak because the whole point of those code paths is value correctness, not row count.
**Fix:** Assert at least one representative value per scenario — e.g. for scenario 4, assert that warehouse `WH1` has `latest_qty = 10` (the qty at the MIN snapshot_date), and for scenario 5, assert a specific user's running average matches the expected window result.

### WR-03: Dead `MIGRATION_LANDED` flag and `SKIP_UNTIL_PLAN_02` machinery never removed after migration landed

**File:** `test/integration/test_adbc_queries.py:84-99, 510-519, 534-539`
**Issue:** `MIGRATION_LANDED = True` was flipped in commit `9fe1ae5` ("test(66-02): flip MIGRATION_LANDED and fix 4 DDL bugs..."), making the `if skip_until_plan_02 and not MIGRATION_LANDED:` branch unreachable. The second tuple element in every `_SCENARIOS` entry is now dead — all five `True` flags and two `False` flags carry no behavior. The `SKIP_UNTIL_PLAN_02` constant (a multi-line string) is only referenced inside the dead branch. Leaving this control flow in place violates the documented Phase 66 spec ("flipping `MIGRATION_LANDED = True` in a single edit by Plan 02") — the spec implies the flag is transitional, not a permanent feature toggle. This is a readability and maintainability defect: a future reader will trace the gating logic, conclude it's a deliberate kill-switch, and try to preserve it.
**Fix:** Remove the gating after the migration is confirmed green. Either:
```python
# Option A: remove flag + machinery entirely
_SCENARIOS = [
    test_main_path_default_schema,
    test_main_path_non_default_schema,
    test_facts_non_default_schema,
    test_semi_additive_non_default_schema,
    test_window_non_default_schema,
    test_materialization_routing_non_default_schema_target,
    test_attach_facts_path,
]
# ... runner just iterates fns
```
Or leave a one-line comment noting the migration commit and delete `SKIP_UNTIL_PLAN_02`, `MIGRATION_LANDED`, and the second tuple element from `_SCENARIOS`.

### WR-04: Scenario docstrings are stale — claim to be `SKIP_UNTIL_PLAN_02`-gated when they now run

**File:** `test/integration/test_adbc_queries.py:230-237, 274-281, 325-330, 378-386, 451-460`
**Issue:** Scenarios 3 through 7 each have a docstring opening line like `"""Scenario 3 — FACTS path, non-default schema base table. SKIP_UNTIL_PLAN_02."""` (line 231). Because `MIGRATION_LANDED = True`, these scenarios actually run on every invocation — the docstrings now mislead. Same for scenarios 4 (line 275), 5 (line 326), 6 (line 381), 7 (line 452).
**Fix:** Replace `SKIP_UNTIL_PLAN_02.` with `ACTIVE.` in each docstring, or rewrite the docstrings to describe what the scenario verifies post-migration. Drop the historical "pre-migration this emits `FROM \"sales\"`..." narrative — that's commit-archeology, not test documentation.

### WR-05: Test runner swallows all exceptions with `except Exception` and prints only `f"{type(e).__name__}: {e}"`

**File:** `test/integration/test_adbc_queries.py:547-549`
**Issue:** A failing scenario produces output like `FAIL: AssertionError: expected 2 rows, got None`, with no traceback, no file/line, and no indication of which `_execute` or `_scalar` call raised. Debugging a CI failure forces the maintainer to re-run locally with manual instrumentation. The `except Exception` also catches `AssertionError`, `adbc_driver_manager.ProgrammingError`, and `OperationalError` uniformly — fine — but discarding the traceback is the actual defect.
**Fix:** Print the traceback:
```python
import traceback
# ...
except Exception as e:  # noqa: BLE001 (intentional broad catch in test runner)
    print(f"  FAIL: {type(e).__name__}: {e}")
    traceback.print_exc()
    failed += 1
```

### WR-06: `f"  PASS"` is an f-string with no interpolation (lint surface)

**File:** `test/integration/test_adbc_queries.py:545`
**Issue:** `print(f"  PASS")` and `print(f"  FAIL: {type(e).__name__}: {e}")` — the PASS line is a no-op f-string. Ruff / pyflakes will flag this as `F541 f-string without any placeholders`. The project's pre-commit hook does not appear to lint Python (Rust-only per CLAUDE.md), so this slips through, but it remains noise.
**Fix:** Use a plain string: `print("  PASS")`.

## Info

### IN-01: Unused dependency `pyarrow>=16` declared in PEP-723 inline script metadata

**File:** `test/integration/test_adbc_queries.py:3-9`
**Issue:** The `# /// script` header lists `pyarrow>=16` as a dependency but no `import pyarrow` (or any pyarrow symbol) appears in the file. ADBC drivers do transitively need pyarrow at runtime for Arrow result handling, so the dep is not strictly dead — but if it's there for runtime support of the driver rather than direct use by the test, a one-line comment clarifying intent would prevent future "trim unused" mistakes.
**Fix:** Either add a brief comment in the dependency block (`# pyarrow: required by adbc_driver_manager.dbapi for Arrow result fetch`) or remove if confirmed truly unused.

### IN-02: `FORCE INSTALL` SQL builds a path interpolation without escaping single quotes

**File:** `test/integration/test_adbc_queries.py:134`
**Issue:** `_execute(conn, f"FORCE INSTALL '{extension_path}'")` — a path containing a single quote would break the SQL. The path comes from `get_extension_path()` (project-internal helper) so the risk is effectively zero in practice, but the pattern mirrors a SQL-injection-style concern. The mirrored `test_adbc_transactions.py` is presumably the same — out of scope to fix here, but worth a one-time hardening pass.
**Fix:** Either accept the limitation (paths are project-controlled) or escape: `extension_path_sql = str(extension_path).replace("'", "''")`.

### IN-03: `AdbcDatabase` object created but never explicitly closed

**File:** `test/integration/test_adbc_queries.py:109-117`
**Issue:** `_connect_adbc` constructs `db = adbc_driver_manager.AdbcDatabase(...)`, then `conn = adbc_driver_manager.AdbcConnection(db)`, and returns the DBAPI wrapper `adbc_driver_manager.dbapi.Connection(db, conn, autocommit=False)`. The `finally: conn.close()` in each scenario closes the DBAPI wrapper, which should cascade — but if it doesn't fully release the underlying `AdbcDatabase`, file handles to the DuckDB DB file can linger past the `tempfile.TemporaryDirectory` cleanup, producing test pollution on Linux/macOS or test failures on Windows. The reference test `test_adbc_transactions.py` uses the same pattern, so this is consistent project style.
**Fix:** Either rely on the DBAPI wrapper to cascade (current behavior — document it in a comment) or explicitly track and close `db` in a nested `try/finally`.

### IN-04: Scenario 7 uses lazy import `import duckdb` at function scope

**File:** `test/integration/test_adbc_queries.py:465-466`
**Issue:** `import duckdb` appears inside `test_attach_facts_path` for the pre-creation step. This works but inverts the module-level import convention used elsewhere in the file (lines 77-79). Moving the import to the top would surface a missing-dependency error at module load rather than scenario run.
**Fix:** Move `import duckdb` to the top of the file alongside the other imports. The PEP-723 dependency block already pins `duckdb==1.5.2`.

### IN-05: `_notes/error_with_adbc.md` resolution header references commits that may need verification

**File:** `_notes/error_with_adbc.md:10`
**Issue:** "See commits: b55936f, b116553, 9fe1ae5" — the resolution header documents three commits. Two of these (`b55936f`, `b116553`) are the migration commits; `9fe1ae5` is the test-flip commit. A fourth commit (`ef81ea2` — sqllogictest fixture update) is missing from the list. Not load-bearing for users, but the curated record is incomplete.
**Fix:** Add `ef81ea2` to the commit list, or simplify to "See Phase 66 phase artifacts at `.planning/phases/66-expansion-qualification-adbc-tests/`".

---

_Reviewed: 2026-05-26_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
