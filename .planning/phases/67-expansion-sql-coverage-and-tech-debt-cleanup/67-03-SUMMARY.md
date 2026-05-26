---
phase: 67-expansion-sql-coverage-and-tech-debt-cleanup
plan: 03
subsystem: review-cleanup-and-integration-test-hardening
tags: [docs, security-hardening, review-cleanup, adbc]
requires:
  - Phase 66 REVIEW-FIX disposition framework
  - Phase 66 CONTEXT.md D-08 / D-09 (ADBC tests as catalog-resolution guards)
  - Phase 67 CONTEXT.md D-12 / D-13 / D-14
provides:
  - Final disposition for all Phase 66 Warning-tier findings (5 fixed, 1 reclassified, 0 skipped)
  - SQL-literal escape hardening at the FORCE INSTALL interpolation site (IN-02 closed)
  - Documented sentinel-keep design call recoverable by future maintainers
affects:
  - .planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md
  - test/integration/test_adbc_queries.py
tech_stack:
  added: []
  patterns:
    - SQL-string-literal escape via doubling single quotes (DuckDB-recognised pattern)
key_files:
  created: []
  modified:
    - .planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md
    - test/integration/test_adbc_queries.py
decisions:
  - C1 implemented as Option A from PLAN: WR-02 moved out of `## Skipped Issues` into a new `## Reclassified Post-Phase-66 (not a defect)` section; Skipped Issues left with an explicit empty marker.
  - C3 implemented via Approach B (SQL-literal escape) — chosen because the interpolation target is a SQL string literal, not a shell argument. Both `shlex.quote` and `replace("'", "''")` satisfy CONTEXT.md D-14; SQL-escape is the equivalent escaping for the actual target grammar.
metrics:
  duration_minutes: 12
  completed_at: 2026-05-26T22:23:23Z
  tasks_total: 3
  tasks_completed: 3
---

# Phase 67 Plan 03: Phase-66 Review-Cleanup Bookkeeping Summary

Three small bookkeeping items from the Phase 66 review-fix pass landed as three atomic commits: WR-02 disposition reclassified to `not-a-defect` per Phase 66 CONTEXT.md D-08/D-09, the sentinel-keep design call for WR-01 recorded inline so it is recoverable, and the `FORCE INSTALL '{extension_path}'` interpolation in `test_adbc_queries.py` hardened against single-quote injection with a SQL-literal escape (IN-02 closed). No production code touched.

## Commits

| # | Hash      | Type | Subject                                                                            |
| - | --------- | ---- | ---------------------------------------------------------------------------------- |
| 1 | `1aeb5fe` | docs | `docs(67): reclassify WR-02 as not-a-defect per Phase 66 D-08/D-09`                |
| 2 | `75f076e` | docs | `docs(67): document WR-01 sentinel-keep design call per D-13`                      |
| 3 | `586f2b3` | fix  | `fix(67): IN-02 escape extension_path for SQL string literal in test_adbc_queries` |

## Task 1 (C1): WR-02 reclassification

**Files changed:** `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md`

### Frontmatter changes

- `skipped: 1` → `skipped: 0`
- `status: partial` → `status: complete`
- New field added: `reclassified_post_phase66: 1`
- Summary block extended to spell out the reclassification: original 5 fixed + 1 skipped, post-reclassification 5 fixed + 1 reclassified + 0 skipped.

### Verbatim disposition + rationale block (added under WR-02)

```
**Disposition (post-Phase-66 reclassification):** `not-a-defect`.

**Rationale:** Per Phase 66 CONTEXT.md D-08 and D-09, the ADBC integration tests in `test_adbc_queries.py` are catalog-resolution regression guards, not value-correctness tests. Their purpose is to catch the failure class `Catalog Error: Table with name X does not exist!` arising from unqualified `quote_table_ref` emission on a per-call Connection whose default catalog/schema search path diverges from `memory.main`. Count-only assertions are sufficient for that purpose: if a regression breaks qualified emission, the count-only assertion fails with a Catalog Error before the count is even computed; if a regression returns wrong rows with right cardinality (the case WR-02 worries about), that is a value-correctness regression and belongs to the SQL-layer fixture set, not the ADBC catalog-resolution guard.

The value-correctness regression surface that WR-02 names is covered (or will be) by `test/sql/phase67_qualified_emission.test` (Phase 67 Plan 01 A1) and the existing fixture-level behavioural tests in `phase47_semi_additive.test` and `phase48_window_metrics.test`. Per the project's stated testing strategy ("primarily test for expected re-written SQL", see SCOPE.md), value-correctness lives at the sqllogictest layer not the ADBC layer.

Reclassified in Phase 67 Plan 03 Task 1.
```

The original `Reason: needs-design-decision` body and the four numbered `Why skipped` points are preserved verbatim under a `**Historical context (preserved from original skip):**` heading. The original `Recommended follow-up` paragraph is annotated as `superseded by reclassification above`.

### Verification

- `grep -c "not-a-defect" 66-REVIEW-FIX.md` = 3 ≥ 1 PASS.
- `grep -c "D-08" 66-REVIEW-FIX.md` = 3 ≥ 1 PASS.
- `grep -c "D-09" 66-REVIEW-FIX.md` = 4 ≥ 1 PASS.
- `grep -c "skipped: 0" 66-REVIEW-FIX.md` = 1 PASS.
- `grep -c "status: complete" 66-REVIEW-FIX.md` = 1 PASS.
- `grep -c "needs-design-decision" 66-REVIEW-FIX.md` = 4 (history preserved) PASS.

## Task 2 (C2): WR-01 sentinel-keep rationale

**Files changed:** `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md`

### Verbatim rationale block (appended to WR-01 entry)

```
**Sentinel-keep design call (Phase 67 Plan 03 Task 2):**

The sentinel-value fix landed in commit `7669878` is retained. The trade-off considered:

- The original `COUNT(*) == 2` assertion already discriminated routing-vs-fallback in scenario 6: raw expansion over the source `sales` table produces 3 rows (`US`, `EU`, `APAC` — see scenario setup), so a silent fall-back to raw expansion would have failed the count check.
- The sentinel values `('US', -1.00), ('EU', -2.00)` (replacing the original `('US', 100.00), ('EU', 200.00)` that coincide with `SUM(s.amount) GROUP BY region`) make a routing failure fail FASTER (on the scalar value check) and more DIAGNOSTICALLY (the test name + assertion message immediately surface the value mismatch rather than the count discrepancy).
- The "value test" objection is technically present (the assertion now compares a specific scalar, not just a row count), but minor in context: sentinels are smoke values chosen to be impossible-from-raw-expansion, not domain assertions about correct materialization arithmetic. They function as canaries, not as value-correctness tests.
- The cleaner alternative (revert to `COUNT(*) == 2` with the 3-row source) loses the failure-mode-precision benefit and gains nothing. The split-the-difference alternative (one sentinel for smoke, then count) over-engineers a single-scenario test.

Design call recorded so the call is recoverable. No code change in this task; the sentinel fix from commit `7669878` stands.
```

### Verification

- `grep -c "Sentinel-keep design call" 66-REVIEW-FIX.md` = 1 ≥ 1 PASS.
- `grep -c "7669878" 66-REVIEW-FIX.md` = 3 (existing reference + 2 new) PASS.
- Original `Applied fix: requires human verification` body and reviewer-suggested-fix narrative preserved PASS.
- `git diff --name-only HEAD~1` on the C2 commit shows only the markdown file PASS.

## Task 3 (C3): IN-02 SQL-literal escape

**Files changed:** `test/integration/test_adbc_queries.py`

### Diff (before → after)

```diff
 def _bootstrap_extension(conn, extension_path: Path) -> None:
     """Install + load the extension on a fresh ADBC connection, then commit."""
-    _execute(conn, f"FORCE INSTALL '{extension_path}'")
+    # IN-02: escape single-quotes for SQL string literal (path may contain '
+    # though project-internal paths today don't). DuckDB-recognised escape
+    # inside a single-quoted string literal is doubling the single quote.
+    extension_path_sql = str(extension_path).replace("'", "''")
+    _execute(conn, f"FORCE INSTALL '{extension_path_sql}'")
     _execute(conn, "LOAD semantic_views")
     conn.commit()
```

Note: the sibling `_execute(conn, "LOAD semantic_views")` already loads by registered extension name, not by path — no second interpolation to harden in this file.

### Approach choice — D-14 satisfaction

CONTEXT.md D-14 reads "`shlex.quote` OR equivalent escaping". Two candidates were considered:

| Approach                     | Target grammar    | Notes                                                                                                                                                                          |
| ---------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `shlex.quote`                | POSIX shell       | Idiomatic Python, but applies shell-quoting semantics to a string that will land inside a SQL string literal, not a shell argument.                                            |
| `replace("'", "''")` (chosen) | SQL string literal | DuckDB-recognised escape: doubling a single quote inside a single-quoted string is the standard SQL way to embed a literal quote. Matches the actual interpolation target.     |

Both close the injection surface; SQL-escape is the equivalent escaping for the actual target grammar (a SQL string literal), so the chosen approach is the more correct of the two.

### Verification

- `grep -c "replace(\"'\", \"''\")" test/integration/test_adbc_queries.py` = 1 ≥ 1 PASS.
- `grep -c "IN-02" test/integration/test_adbc_queries.py` = 1 ≥ 1 PASS.
- `test/integration/test_adbc_transactions.py` NOT modified (out of scope per CONTEXT.md). See TECH-DEBT follow-up below.
- Hash `586f2b3` is an atomic commit touching only `test/integration/test_adbc_queries.py`.

### Quality-gate run

The Phase 67 Plan 03 quality gate had two halves:

1. **Native build + cargo test + sqllogictest (`just test-sql`)** — RAN GREEN.
   - `cargo test --lib --tests` → exit 0; all unit + proptest + doc tests pass.
   - `just test-sql` → 58/58 SUCCESS, 0 failed.

2. **`uv run`-driven integration tests (`just test-ducklake-ci`, `just test-adbc-queries`, `just test-large-view`, `just test-multi-db`, `just test-readonly`, `just test-concurrent`, `just test-adbc`)** — NOT RUN in this worktree-agent sandbox.

   `uv` itself panics in the sandboxed environment with:

   ```
   thread 'main2' panicked at system-configuration-0.6.1/src/dynamic_store.rs:154:1:
   Attempted to create a NULL object.
   thread 'main' panicked at uv-0.9.18/crates/uv/src/lib.rs:2540:10:
   Tokio executor failed, was there a panic?: Any { .. }
   ```

   This is `uv` itself failing on macOS `SCDynamicStoreCreate` before any test code runs (a known sandbox interaction with the `system-configuration` Rust crate). Per CLAUDE.md Rule 2 the listed `uv run test/integration/*.py` commands are pre-approved for `dangerouslyDisableSandbox: true`, but the bypass was denied by the harness in this worktree-agent context. The build itself (cargo + `make configure` + amalgamation compile) succeeded — only the Python test runner could not start.

   **C3 risk assessment under this verification gap:** the change is functionally a no-op for the project-internal `extension_path` value that `get_extension_path()` returns (no embedded single quotes today). `replace("'", "''")` on a quote-free string is the identity transform; the test code path is unchanged for current inputs. The hardening only activates if a maintainer ever introduces a path containing a single quote. The unrun ADBC tests were verified green at commit `7669878` (per the Phase 66 REVIEW-FIX entry) and the change between that commit and `586f2b3` is a 4-line additive edit inside a function body that compiles cleanly under Python lint. Treat as low residual risk; the orchestrator may re-run the full `uv`-gated suite when reassembling the wave outside this worktree-agent sandbox.

## TECH-DEBT follow-up surfaced

`test/integration/test_adbc_transactions.py:132` has an analogous `_execute(conn, f"FORCE INSTALL '{extension_path}'")` interpolation with the same un-escaped quote surface. Phase 67 Plan 03 explicitly leaves this site out of scope (Phase 66 REVIEW.md:116 marks it `out of scope to fix here` and the C3 task in `67-03-PLAN.md` instructs "Do NOT touch the `test_adbc_transactions.py` mirror"). Surfaced as TECH-DEBT-followup candidate: apply the same `replace("'", "''")` hardening in a future single-edit commit; the fix is identical mechanical change.

## Self-Check: PASSED

- File `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md` exists and contains:
  - `not-a-defect` (3 occurrences) PASS
  - `D-08` (3) and `D-09` (4) PASS
  - `Sentinel-keep design call` (1) PASS
  - `7669878` (3) PASS
  - frontmatter `skipped: 0`, `status: complete`, `reclassified_post_phase66: 1` PASS
- File `test/integration/test_adbc_queries.py` exists and contains:
  - `replace("'", "''")` (1) PASS
  - `IN-02` comment (1) PASS
- Commits exist in `git log`:
  - `1aeb5fe` PASS
  - `75f076e` PASS
  - `586f2b3` PASS
- `test/integration/test_adbc_transactions.py` NOT modified: `git diff --name-only HEAD~3..HEAD` shows no entry for it. PASS

## Deviations from Plan

None. The plan was executed as written:

- C1 implemented as Option A (preferred form per PLAN).
- C2 implemented as documentation-only append (no code touched).
- C3 implemented via Approach B (SQL-escape) as the plan explicitly directs.

The only execution-environment deviation was the `uv run`-gated portion of `just test-all`, which could not run under the worktree-agent sandbox. That is an environment constraint, not a plan deviation. Cargo test + sqllogictest (the layers that don't shell out to `uv`) ran fully green; the ADBC layer change is provably a no-op for current inputs.

## Threat Flags

None. The C3 change closes T-67-03-01 from the plan's threat register (Tampering at the `extension_path` → SQL string literal trust boundary). C1 + C2 are documentation-only and introduce no new trust boundaries.
