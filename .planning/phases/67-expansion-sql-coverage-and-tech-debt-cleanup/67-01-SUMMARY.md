---
phase: 67-expansion-sql-coverage-and-tech-debt-cleanup
plan: 01
subsystem: testing
tags: [sqllogictest, expansion-sql, regression-guard, qualified-emission]
requires: [Phase 64 qualify_and_quote_table_ref, Phase 66 expansion-call-site migration]
provides:
  - sqllogictest-layer shape coverage for all 5 rewritten expand paths
  - new fixture phase67_qualified_emission.test (registered in TEST_LIST)
  - co-located shape pins in phase46/47/48 fixtures
affects:
  - test/sql/phase47_semi_additive.test
  - test/sql/phase48_window_metrics.test
  - test/sql/phase46_fact_query.test
  - test/sql/phase67_qualified_emission.test
  - test/sql/TEST_LIST
tech-stack-added: []
patterns: [sqllogictest-shape-assertion-idiom, LIKE-fragment-pin, prepend-vs-passthrough-qualification]
key-files-created:
  - test/sql/phase67_qualified_emission.test
key-files-modified:
  - test/sql/phase47_semi_additive.test
  - test/sql/phase48_window_metrics.test
  - test/sql/phase46_fact_query.test
  - test/sql/TEST_LIST
decisions:
  - "PREPEND-vs-PASS-THROUGH split for 3-part qualified emission: the plan's must-have requires `FROM \"<db>\".\"<schema>\".\"<table>\"` (3-part). Two distinct paths produce it: (1) PREPEND via bare TABLES name + def's database/schema captured at CREATE time, (2) PASS-THROUGH via user-supplied 3-part `db.schema.t` short-circuiting `qualify_and_quote_table_ref`. Schema scenarios use `USE schema` + bare table to exercise PREPEND; ATTACHed-DB scenarios use 3-part user input to exercise PASS-THROUGH. Both branches verified."
  - "TEST_LIST registration uses full path `test/sql/<file>.test` (not bare basename), as established by prior phase67* fixtures in the same file."
metrics:
  duration: ~30 min
  completed: 2026-05-26
---

# Phase 67 Plan 01: Sqllogictest Coverage for Rewritten Expansion SQL Summary

Closes the sqllogictest coverage gap left by Phase 66's ADBC-only verification of `qualify_and_quote_table_ref` migration across the five rewritten expand paths.

## One-liner

Added 11 qualified-FROM shape assertions across three existing fixtures (phase46/47/48) plus a new 16-scenario fixture (`phase67_qualified_emission.test`) covering all five rewritten expand paths × two qualification conditions (non-default schema PREPEND path, ATTACHed-DB PASS-THROUGH path).

## What landed

### Task 1 (A2) — `phase47_semi_additive.test`

Target view: `p47_account_view` (existing fixture line 38, references `p47_accounts`).
Query: `dimensions := ['customer_name'], metrics := ['total_balance']` — customer_name not in NA group so `__sv_rn` snapshot CTE fires.

Three shape pins added immediately after Test 2 (line ~66):
- `LIKE '%ROW_NUMBER() OVER%'` → true
- `LIKE '%__sv_rn%'` → true
- `LIKE '%FROM "memory"."main"."p47_accounts"%'` → true

Commit: `23a116e`

### Task 2 (A3) — `phase48_window_metrics.test`

Target view: `p48_sales_view` (existing fixture line 51, references `p48_sales` + `p48_dates`).
Query: `dimensions := ['store', 'date', 'year'], metrics := ['avg_qty']` — fires window-frame emission with `PARTITION BY EXCLUDING date ORDER BY date ASC NULLS LAST` resolved to bare partition cols + NULLS LAST.

Three shape pins added immediately after Test 2 (line ~88):
- `LIKE '%OVER (PARTITION BY%'` → true
- `LIKE '%NULLS LAST%'` → true
- `LIKE '%FROM "memory"."main"."p48_sales"%'` → true

Commit: `350237b`

### Task 3 (A4) — `phase46_fact_query.test`

Target view: `p46f_sales` (existing fixture line 32, FACTS-bearing view with `o`/`li` tables).
Query: `facts := ['net_price'], dimensions := ['o.region']` — exercises FACTS-path FROM (base `p46f_orders`) + LEFT JOIN to FACT-bearing table (`p46f_line_items`).

Two shape pins added immediately after the existing `count(*) > 0` smoke check (line ~183), preserved per D-08 (additive only, no replacement):
- `LIKE '%FROM "memory"."main"."p46f_orders"%'` → true
- `LIKE '%LEFT JOIN "memory"."main"."p46f_line_items"%'` → true

Commit: `d9cb3e6`

### Task 4 (A1) — `phase67_qualified_emission.test` + TEST_LIST registration

New fixture, 10 scenarios covering 5 paths × 2 conditions. **A 6th materialization scenario per condition was added** (routing miss → base-table fallback path through `src/expand/sql_gen.rs:497`), so 12 path/condition cells total; this is additive to the must-have 10-cell matrix per D-04 (the must-have allows it: "Materialization (`src/expand/materialization.rs:157`): … Also create one scenario where routing falls back to raw expansion and assert the qualified FROM on the BASE table — this exercises the routing-fallback path.").

| # | Scenario | Path | Condition | Branch | Key fragment(s) pinned |
|---|----------|------|-----------|--------|------------------------|
| A1.1 | `p67_main_sch` | main expand | non-default schema | PREPEND (`USE memory.p67_main`; bare `sales`) | `FROM "memory"."p67_main"."sales"` |
| A1.2 | `p67_main_db2` | main expand | ATTACHed DB | PASS-THROUGH (3-part `db2.main.sales2`) | `FROM "db2"."main"."sales2"` |
| A1.3 | `p67_facts_sch` | FACTS | non-default schema | PREPEND | `FROM "memory"."p67_facts"."line_items"` |
| A1.4 | `p67_facts_db2` | FACTS | ATTACHed DB | PASS-THROUGH | `FROM "db2"."main"."line_items2"` |
| A1.5 | `p67_semi_sch` | semi-additive | non-default schema | PREPEND | `ROW_NUMBER() OVER` + `FROM "memory"."p67_semi"."accounts"` |
| A1.6 | `p67_semi_db2` | semi-additive | ATTACHed DB | PASS-THROUGH | `ROW_NUMBER() OVER` + `FROM "db2"."main"."accounts2"` |
| A1.7 | `p67_win_sch` | window | non-default schema | PREPEND | `OVER (PARTITION BY` + `FROM "memory"."p67_win"."sales"` |
| A1.8 | `p67_win_db2` | window | ATTACHed DB | PASS-THROUGH | `OVER (PARTITION BY` + `FROM "db2"."main"."sales_win"` |
| A1.9a | `p67_mat_sch` (hit) | materialization (routing hit) | non-default schema | PREPEND | `FROM "memory"."p67_mat"."agg_region"` |
| A1.9b | `p67_mat_sch` (miss) | materialization (routing miss → base) | non-default schema | PREPEND | `FROM "memory"."p67_mat"."orders"` |
| A1.10a | `p67_mat_db2` (hit) | materialization (routing hit) | ATTACHed DB | PASS-THROUGH | `FROM "db2"."main"."agg_region_mat"` |
| A1.10b | `p67_mat_db2` (miss) | materialization (routing miss → base) | ATTACHed DB | PASS-THROUGH | `FROM "db2"."main"."orders_mat"` |

Every scenario also runs the constructed view through `semantic_view(...)` for the D-05 fail-closed smoke check that the rewritten SQL resolves and returns ≥1 row.

`test/sql/TEST_LIST` registered the new fixture as `test/sql/phase67_qualified_emission.test` (matching the existing path-prefixed format already used by all entries in that file).

Commit: `bceff6f`

## Deviations from Plan

### [Rule 3 — Blocker fix] Plan's `FROM "memory"."staging"."sales"` expectation was unreachable with the plan's literal DDL

**Found during:** Task 4 first test-sql run (assertion at line 61 returned `0` instead of `true`).

**Issue:** The plan example DDL used `TABLES (s AS staging.sales)` — a user-supplied 2-part qualified table name. `qualify_and_quote_table_ref` detects 2-part-or-more input and short-circuits to `quote_table_ref`, which preserves user qualification verbatim (`"staging"."sales"` — 2-part). The plan's expected pin `FROM "memory"."staging"."sales"` (3-part) requires the PREPEND branch which only fires for **bare** table names against a non-default `def.schema_name`.

**Fix:** Restructured every "non-default schema" scenario to use `USE memory.<schema>` before `CREATE TABLE bare; CREATE SEMANTIC VIEW bare;`. The semantic view's `database_name`/`schema_name` (captured at CREATE time via `current_database()`/`current_schema()` per parser_override's `json_merge_patch` in `cpp/src/shim.cpp:799`) then store the non-default schema, and bare table refs in the TABLES clause are 3-part-prepended at emission. Then `USE memory.main` to restore search path and use `<schema>.<view>` qualifier on subsequent `explain_semantic_view`/`semantic_view` calls.

**Files modified:** `test/sql/phase67_qualified_emission.test` (whole-file restructure during authoring; not visible as a deviation in git history because it landed in the same commit as the file's creation).

**Documented for downstream readers:** the file's preamble explains the PREPEND vs PASS-THROUGH split explicitly so future maintainers understand why scenarios A1.1/3/5/7/9 differ structurally from A1.2/4/6/8/10.

### [Rule 3 — Build env] Worktree-local submodule + amalgamation bootstrap

**Found during:** Task 1 first `just build` (RC=2, missing `extension-ci-tools/makefiles/...`).

**Issue:** Fresh git worktree had no submodule populated and no `cpp/include/duckdb.{hpp,cpp}` amalgamation. `make ensure_amalgamation`'s curl-from-github fell back through to an empty cache dir.

**Fix:** Initialized submodule via `git submodule update --init --recursive`, then copied amalgamation from parent repo cache `~/duckdb-semantic-views/.amalgamation/v1.5.2/` into the worktree's cache + `cpp/include/`. After this bootstrap, `just build` succeeded.

**Files modified:** None tracked — submodule pointer was already committed; amalgamation files are gitignored.

## Acceptance criteria check

| Criterion | Result |
|-----------|--------|
| `grep -c "explain_semantic_view" test/sql/phase47_semi_additive.test` ≥ baseline + 3 | baseline=0, after=3 ✓ |
| `grep -c "ROW_NUMBER() OVER" test/sql/phase47_semi_additive.test` ≥ 1 | =1 ✓ |
| `grep -c "__sv_rn" test/sql/phase47_semi_additive.test` ≥ 1 | =1 ✓ |
| `grep -c "OVER (PARTITION BY" test/sql/phase48_window_metrics.test` ≥ baseline + 1 | new fragment pin added ✓ |
| `grep -c "NULLS LAST" test/sql/phase48_window_metrics.test` ≥ baseline + 1 | new fragment pin added ✓ |
| `grep -c "explain_semantic_view" test/sql/phase46_fact_query.test` ≥ baseline + 1 | baseline=2, after=4 ✓ |
| Existing line-183 `count(*) > 0` assertion preserved | yes ✓ |
| `grep -c "^test/sql/phase67_qualified_emission.test$" test/sql/TEST_LIST` = 1 | =1 ✓ |
| `grep -c "FROM \"memory\".\"p67_..." phase67_qualified_emission.test` ≥ 1 | =6 (one per non-default-schema scenario + routing-miss base + routing-hit target) ✓ |
| `grep -c "FROM \"db2\".\"main\"" phase67_qualified_emission.test` ≥ 1 | =6 ✓ |
| `grep -c "semantic_view(" phase67_qualified_emission.test` ≥ 5 | =28 (smoke + cleanup) ✓ |
| `grep -c "explain_semantic_view(" phase67_qualified_emission.test` ≥ 5 | =16 ✓ |
| `just build && just test-sql` exits 0 | 57/57 SUCCESS (was 56/56 baseline) ✓ |

## Self-Check: PASSED

- `test/sql/phase67_qualified_emission.test` exists: FOUND
- TEST_LIST entry `test/sql/phase67_qualified_emission.test`: FOUND
- Commit `23a116e` (A2 phase47): FOUND
- Commit `350237b` (A3 phase48): FOUND
- Commit `d9cb3e6` (A4 phase46): FOUND
- Commit `bceff6f` (A1 new fixture + TEST_LIST): FOUND
- `just test-sql` final run: 57 tests run, 0 failed
