---
phase: 67-expansion-sql-coverage-and-tech-debt-cleanup
verified: 2026-05-27T09:00:00Z
status: passed
score: 7/7 must-haves verified
overrides_applied: 0
human_verification:
  - test: "Run `just test-all` (full quality gate) from a host where `uv` is not sandbox-blocked"
    expected: "All Python integration test suites pass — test-ducklake-ci, test-adbc-queries (7/7), test-adbc, test-multi-db, test-readonly, test-concurrent all exit 0"
    why_human: "Plans 02 and 03 ran in worktree-agent sandboxes where `uv` panics at startup (macOS SCDynamicStore NULL object). Plan 04 ran in a non-sandboxed context and reports `just test-all` exit 0 with `SUMMARY: 12/12 tests passed` and concurrent-DDL PASS — but Plan 04's surgery was a 2-line test-file rename with zero impact on the body_parser.rs code from Plan 02. The critical Plan 02 body_parser.rs surgery has not had `uv`-gated integration test coverage confirmed in the executor logs. The verifier cannot run `uv` commands in this sandbox environment."
---

# Phase 67: Expansion-SQL Coverage and Tech-Debt Cleanup Verification Report

**Phase Goal:** Tighten the sqllogictest regression net around rewritten expansion SQL (the layer Phase 66 retrofitted but only smoke-tested at the ADBC boundary) and clear the small batch of tech-debt and Phase 66 review-pass cleanup items that are mechanical fixes blocking real edge cases or hygiene. Final technical phase of v0.10.0; release prep handled at milestone close, NOT in this phase.
**Verified:** 2026-05-27
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `phase67_qualified_emission.test` exists, covers 5 expand paths × 2 conditions (12 cells), and is registered in `test/sql/TEST_LIST` | VERIFIED | File is 566 lines. `grep -c "explain_semantic_view(" = 16`, `grep -c "semantic_view(" = 28`, `grep -c 'FROM "db2"."main"' = 6`, `grep -c 'FROM "memory"."p67_' = 6`. `test/sql/TEST_LIST:57` = `test/sql/phase67_qualified_emission.test`. |
| 2 | Shape assertions added to `phase47_semi_additive.test` pinning `ROW_NUMBER() OVER`, `__sv_rn`, and qualified `FROM "memory"."main"."p47_accounts"` | VERIFIED | `grep -c "explain_semantic_view"` = 4 (baseline was 0+1 existing, now 4). `grep -c "ROW_NUMBER() OVER"` = 2. `grep -c "__sv_rn"` = 2. |
| 3 | Shape assertions added to `phase48_window_metrics.test` pinning `OVER (PARTITION BY`, `NULLS LAST`, and qualified FROM | VERIFIED | `grep -c "OVER (PARTITION BY"` = 8. `grep -c "NULLS LAST"` = 9. `grep -c "explain_semantic_view"` = 3. |
| 4 | Shape assertions added to `phase46_fact_query.test` pinning qualified `FROM` and `LEFT JOIN` for the FACTS path | VERIFIED | `grep -c "explain_semantic_view"` = 4. Lines 189-197 show `LEFT JOIN "memory"."main"."p46f_line_items"` assertion. Original `count(*) > 0` at line ~183 preserved. |
| 5 | `src/body_parser.rs::parse_single_table_entry` uses `find_identifier_end` from `src/ident.rs` for source-table-name tokenisation; 5 new unit tests cover quoted-whitespace, PRIMARY-KEY-in-name, 3-part FQN, regression baseline, UNIQUE-no-PK cases | VERIFIED | Import at `src/body_parser.rs:7` (`use crate::ident::find_identifier_end`). `find_identifier_end` called at line 694. `grep -c "test_parse_single_table_entry_"` = 5. `test/sql/phase67_quoted_source_tables.test` (157 lines, 4 scenarios) contains `"my orders"` × 8 hits and `"weird PRIMARY KEY name"` × 4 hits. Registered in `test/sql/TEST_LIST:58`. |
| 6 | TECH-DEBT.md entry #24 marked ✅ RESOLVED with Phase 67 reference; #25 added for sibling audit-grep finding | VERIFIED | `TECH-DEBT.md:212` = `### 24. ✅ Body parser's TABLES clause … RESOLVED in Phase 67`. Resolution annotation at line 218 cites commits `256ae65` and `5fb2ed4`. TECH-DEBT #25 added at line 224 for `NON ADDITIVE BY` / OVER `ORDER BY` sibling slots. |
| 7 | `66-REVIEW-FIX.md` updated: WR-02 reclassified to `not-a-defect` with D-08/D-09 citation; WR-01 sentinel-keep rationale documented; `test/integration/test_adbc_queries.py` scenario 4 uses `earliest_qty`; FORCE INSTALL SQL-literal escape applied (IN-02) | VERIFIED | `66-REVIEW-FIX.md` frontmatter: `skipped: 0`, `status: complete`, `reclassified_post_phase66: 1`. `grep -c "not-a-defect"` = 3. `grep -c "D-08\|D-09"` = 7. `grep -c "Sentinel-keep design call"` = 1. `grep -c "7669878"` = 3. `test_adbc_queries.py:100` = `extension_path_sql = str(extension_path).replace("'", "''")`. `grep -c "IN-02"` = 1. `grep -c "earliest_qty"` = 2. `grep -c "latest_qty"` = 0. |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `test/sql/phase67_qualified_emission.test` | 5 expand paths × 2 conditions, registered in TEST_LIST | VERIFIED | 566 lines; 16 shape assertions, 12 path/condition cells (10 required + 2 materialization routing-miss bonus cells); TEST_LIST:57 |
| `test/sql/phase67_quoted_source_tables.test` | 4 scenarios for TECH-DEBT #24 cases, registered in TEST_LIST | VERIFIED | 157 lines; `"my orders"` × 8, `"weird PRIMARY KEY name"` × 4; TEST_LIST:58 |
| `test/sql/phase47_semi_additive.test` | Shape assertions for semi-additive CTE | VERIFIED | 4 `explain_semantic_view` calls, 2 `ROW_NUMBER() OVER` pins, 2 `__sv_rn` pins |
| `test/sql/phase48_window_metrics.test` | Shape assertions for window OVER frame | VERIFIED | 8 `OVER (PARTITION BY` pins, 9 `NULLS LAST` pins |
| `test/sql/phase46_fact_query.test` | Tightened FACTS-path shape assertions | VERIFIED | 4 `explain_semantic_view` calls; qualified FROM + LEFT JOIN fragment pins |
| `test/sql/TEST_LIST` | Registration of both new fixtures | VERIFIED | Lines 57-58 contain `test/sql/phase67_qualified_emission.test` and `test/sql/phase67_quoted_source_tables.test` |
| `src/body_parser.rs` | `find_identifier_end` import + call in `parse_single_table_entry` + 5 new unit tests | VERIFIED | Import at line 7, call at line 694, 5 test functions at lines 2572–2625 |
| `TECH-DEBT.md` | Entry #24 marked RESOLVED with Phase 67 reference; #25 added | VERIFIED | Lines 212–232; ✅ status, commit citations, #25 structural-rewrite rationale |
| `.planning/phases/66-expansion-qualification-adbc-tests/66-REVIEW-FIX.md` | WR-02 reclassified; WR-01 rationale added; frontmatter updated | VERIFIED | `skipped: 0`, `status: complete`, `reclassified_post_phase66: 1`, D-08/D-09 cited, sentinel-keep documented |
| `test/integration/test_adbc_queries.py` | IN-02 SQL-literal escape; `earliest_qty` rename | VERIFIED | `replace("'", "''")` at line 100; `earliest_qty` at lines 279, 287; zero `latest_qty` remaining |
| `.planning/phases/67-expansion-sql-coverage-and-tech-debt-cleanup/67-REVIEW.md` | Code review report; 0 critical, 3 warnings, 4 info | VERIFIED | Frontmatter: `critical: 0`, `warning: 3`, `info: 4`, `status: issues_found`. WR-01 (error path regression for `o AS PRIMARY KEY (id)` pattern) is Warning-tier, not critical. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `test/sql/phase67_qualified_emission.test` | `test/sql/TEST_LIST` | Line 57 registration entry | WIRED | `grep "phase67_qualified_emission" TEST_LIST` = exact match at line 57 |
| `test/sql/phase67_quoted_source_tables.test` | `test/sql/TEST_LIST` | Line 58 registration entry | WIRED | `grep "phase67_quoted_source_tables" TEST_LIST` = exact match at line 58 |
| `src/body_parser.rs::parse_single_table_entry` | `src/ident.rs::find_identifier_end` | `use crate::ident::find_identifier_end` at line 7; call at line 694 | WIRED | `grep -n "find_identifier_end" src/body_parser.rs` shows import + 3 call sites |
| `TECH-DEBT.md #24` | Phase 67 Plan 02 commits | Resolution annotation with commit SHAs | WIRED | Cites `256ae65` (surgery) and `5fb2ed4` (fixture) |
| `66-REVIEW-FIX.md WR-02` | `66-CONTEXT.md D-08/D-09` | Inline citation in reclassification rationale | WIRED | `grep -c "D-08\|D-09" 66-REVIEW-FIX.md` = 7 |
| `test_adbc_queries.py scenario 4` | `earliest_qty` rename | DDL + assertion arg both updated | WIRED | Lines 279 and 287 both use `earliest_qty`; `latest_qty` count = 0 |

### Behavioral Spot-Checks

Cargo test and sqllogictest layers are runnable in the current sandbox. `uv`-driven integration tests are not (see Human Verification section).

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase 67 new fixtures registered in TEST_LIST | `grep "phase67" test/sql/TEST_LIST` | 2 entries found at lines 57-58 | PASS |
| `find_identifier_end` called in parse_single_table_entry | `grep -n "find_identifier_end" src/body_parser.rs` | 4 matches: import + 3 call sites including line 694 inside the function | PASS |
| `earliest_qty` present, `latest_qty` absent | `grep -c "earliest_qty\|latest_qty" test/integration/test_adbc_queries.py` | earliest=2, latest=0 | PASS |
| TECH-DEBT #24 resolution annotation present | `grep -n "Phase 67" TECH-DEBT.md` | Lines 212, 218 confirm resolution with SHA references | PASS |
| WR-02 reclassification in 66-REVIEW-FIX.md | `grep -c "not-a-defect\|skipped: 0\|status: complete"` | 3 + 1 + 1 matches | PASS |
| Qualified FROM shape pins in new fixture | `grep -c 'FROM "db2"."main"'` in phase67_qualified_emission.test | 6 matches | PASS |

### Probe Execution

No probes declared in PLAN files for this phase. Behavioral spot-checks serve as the automated verification layer.

### Requirements Coverage

No formal requirement IDs in REQUIREMENTS.md — scope defined by 67-CONTEXT.md D-01..D-18.

| Decision | Coverage | Status |
|----------|----------|--------|
| D-03: `phase67_qualified_emission.test` registered in TEST_LIST | File exists (566 lines), TEST_LIST:57 | SATISFIED |
| D-04: 5 expand paths × 2 conditions covered | 12 cells verified (10 required + 2 routing-miss bonus) | SATISFIED |
| D-05: smoke checks via `semantic_view(...)` in new fixture | `grep -c "semantic_view("` = 28 in the new fixture | SATISFIED |
| D-06: shape assertions co-located in phase46/47/48 | All three existing fixtures augmented in-place | SATISFIED |
| D-07: LIKE-fragment assertions, no full-line equality | All shape pins use `LIKE '%fragment%'` idiom | SATISFIED |
| D-08: no negative assertions | Confirmed: no `NOT LIKE` or absent-pattern assertions | SATISFIED |
| D-09: `src/ident.rs` NOT modified | Confirmed via git log — `src/ident.rs` not in any Phase 67 commit | SATISFIED |
| D-10: audit-grep ran; sibling sites classified | Two (c)-class structural sites surfaced as TECH-DEBT #25 | SATISFIED |
| D-11: separate `phase67_quoted_source_tables.test` fixture | Created at 157 lines, 4 scenarios | SATISFIED |
| D-12: WR-02 reclassified in 66-REVIEW-FIX.md | `not-a-defect` disposition with D-08/D-09 citation | SATISFIED |
| D-13: WR-01 sentinel-keep rationale documented | `Sentinel-keep design call` block added | SATISFIED |
| D-14: IN-02 SQL-literal escape applied | `replace("'", "''")` at line 100 with `IN-02` comment | SATISFIED |
| D-15: `latest_qty` renamed to `earliest_qty` after Case A investigation | 0 `latest_qty`, 2 `earliest_qty`; standard `MIN_BY` semantics confirmed | SATISFIED |
| D-17: 4 plans produced | 4/4 plans complete per ROADMAP.md | SATISFIED |
| D-18: no research agent spawned | Confirmed — plans went directly to execution | SATISFIED |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/body_parser.rs` | 692-709 | Dead-code dot-consumption loop arm (WR-03: `find_identifier_end` already walks dots; the `if after_as[name_end] == b'.'` arm is unreachable in practice) | Info | Not a correctness defect; misleading comment only. Identified in 67-REVIEW.md WR-03. |
| `test/integration/test_adbc_queries.py` | ~470 | Sibling `ATTACH '{other_db_path}'` interpolation left bare (WR-02 in 67-REVIEW.md — the FORCE INSTALL site was hardened in Plan 03 but `other_db_path` on line 470 was not) | Warning | Same defense-in-depth concern as IN-02; low practical risk for project-internal temp paths. Identified post-submission in 67-REVIEW.md. |

No `TBD`, `FIXME`, or `XXX` debt markers added by this phase.

### Human Verification Required

#### 1. Full `just test-all` with `uv`-gated integration tests

**Test:** On a host without the macOS `SCDynamicStore` sandbox restriction, run `just test-all` from the repository root after Phase 67's commits are merged.

**Expected:** Exit code 0. All Python integration test suites pass: `test-adbc-queries` (7/7 including scenario 4 with `earliest_qty`), `test-adbc` (6/6 transactional invariant), `test-readonly` (12/12), `test-multi-db` (3/3), `test-concurrent`, `test-ducklake-ci`, `test-vtab-crash`, `test-caret`, `test-large-view`.

**Why human:** Plans 02 and 03 executor sandboxes could not start `uv` (panics at `SCDynamicStore::new` before any test code runs). Plan 04 reports `just test-all` exit 0 with integration-test-level evidence, but Plan 04's surgery was a 2-line rename in `test_adbc_queries.py` with no impact on the body_parser.rs code from Plan 02. The most recent comprehensive `uv`-gated confirmation predates Plan 02's surgery. CLAUDE.md requires `just test-all` for any phase touching production code (`src/body_parser.rs`). The cargo-only and sqllogictest layers are fully green (955 + 60 tests), but the integration layer cannot be confirmed by the verifier in this environment.

---

## Gaps Summary

No gaps found. All 7 must-have truths are VERIFIED. The only open item is the human verification above: the `uv`-gated integration test layer was not confirmed for the Plan 02 body_parser.rs surgery specifically (Plan 04's test-all confirmation came after a different, simpler change). This is a verification coverage gap, not a code defect — the body-parser surgery is contained to a single function with no FFI, threading, transaction, or ADBC surface. The cargo unit tests (955 tests, 5 new ones directly exercising the surgery) and sqllogictest fixtures (60 tests, 4 new ones exercising CREATE → expand roundtrip) are comprehensive. The human gate is a low-risk precaution to satisfy CLAUDE.md's `just test-all` requirement.

The 67-REVIEW.md WR-01 finding (error path regression for `o AS PRIMARY KEY (id)` — bare keyword mis-tokenised as a table name) is a Warning-tier observation, not a BLOCKER. The correct error is still produced (DuckDB will report `table "PRIMARY" does not exist` rather than the project's structured "Missing physical table name" error), so users hitting this malformed DDL still get an error; the regression is in error message quality, not in correctness of the success path. This is post-submission review feedback, not a phase goal requirement, and is tracked as advisory follow-up.

---

_Verified: 2026-05-27_
_Verifier: Claude (gsd-verifier)_

---

## Post-Verification Note

User approved the verification on 2026-05-27 after confirming that the orchestrator
(execute-phase workflow) already ran `just test-all` with the project's pre-approved
sandbox bypass after the Wave 2 merge. The run exercised all 9 `uv`-gated integration
suites (test_adbc_queries, test_readonly_load, test_concurrent_ddl,
test_multi_db_isolation, test_ducklake_ci, test_adbc_transactions, test_vtab_crash,
test_caret_position, test_large_view_rewrite) with zero failures. Log retained at
`/tmp/claude/wave2-test-all.log`.

Status: human_needed → passed (orchestrator coverage retroactively satisfies the
verifier's caveat). The Plan 02 `body_parser.rs` surgery has full integration-test
coverage on the live host.
