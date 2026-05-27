---
phase: 66
slug: expansion-qualification-adbc-tests
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-26
---

# Phase 66 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust (`cargo test` + sqllogictest harness) + Python integration tests via `uv run` |
| **Config file** | `Cargo.toml`; `test/sql/TEST_LIST`; per-file PEP 723 headers (Python) |
| **Quick run command** | `cargo test -p semantic-views --lib` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~1s (quick unit) · ~30s (`just test-sql`) · ~3-5min (`just test-all`) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p semantic-views --lib`
- **After every plan wave:** Run `just build && just test-sql` (and `just test-adbc-queries` once the new recipe exists)
- **Before `/gsd-verify-work`:** `just test-all` must be green
- **Max feedback latency:** ~30s (per-wave); ~5min (phase gate)

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 66-01-01 | 01 | 1 | EXPAND-CTX-02 | — | ADBC-driver `SELECT … FROM semantic_view(...)` returns rows across 7 scenarios | integration | `just test-adbc-queries` | ❌ W0 (new file) | ⬜ pending |
| 66-01-02 | 01 | 1 | EXPAND-CTX-02 | — | New recipe wired into `test-all` aggregate | integration | `just test-all` | ❌ W0 (recipe edit) | ⬜ pending |
| 66-01-03 | 01 | 1 | EXPAND-CTX-02 (D-09 baseline) | — | Scenarios 3-6 FAIL on pre-migration commit (proves test exercises failure mode) | manual | `git stash` migration → `just test-adbc-queries` → expect FAIL | (manual gate) | ⬜ pending |
| 66-02-01 | 02 | 2 | EXPAND-CTX-01 | — | All 7 sites emit `"db"."schema"."name"` via `qualify_and_quote_table_ref` | unit | `cargo test -p semantic-views --lib` | ✅ | ⬜ pending |
| 66-02-02 | 02 | 2 | EXPAND-CTX-01 | — | `materialization.rs::build_materialized_sql` signature accepts `def: &SemanticViewDefinition` | unit | `cargo test -p semantic-views --lib` | ✅ | ⬜ pending |
| 66-02-03 | 02 | 2 | EXPAND-CTX-01 | — | `test/sql/phase57_introspection.test:76` fixture updated to expect qualified shape | sqllogictest | `just build && just test-sql` | ✅ (fixture edit) | ⬜ pending |
| 66-02-04 | 02 | 2 | EXPAND-CTX-01 | — | No regression across sqllogictest aggregate | sqllogictest | `just build && just test-sql` | ✅ | ⬜ pending |
| 66-02-05 | 02 | 2 | EXPAND-CTX-02 | — | Scenarios 3-6 now PASS through `just test-adbc-queries` (migration unlock) | integration | `just test-adbc-queries` | ✅ (after Plan 01) | ⬜ pending |
| 66-03-01 | 03 | 3 | EXPAND-CTX-03 | — | `_notes/error_with_adbc.md` carries `## Resolution (v0.10.0)` header | manual | `git diff _notes/error_with_adbc.md` | ✅ | ⬜ pending |
| 66-03-02 | 03 | 3 | EXPAND-CTX-01 / -02 / -03 | — | Full quality gate green | integration | `just test-all` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

*Plan IDs/waves are indicative — final task ordering is set in PLAN.md frontmatter.*

---

## Wave 0 Requirements

- [ ] `test/integration/test_adbc_queries.py` — new file covering 7 scenarios for EXPAND-CTX-02 (Plan 01)
- [ ] `justfile` — `test-adbc-queries` recipe + `test-all` aggregate amendment (Plan 01)

*Framework already installed; `adbc_driver_duckdb` ships bundled with `duckdb==1.5.2` wheel (research §Standard Stack). No `cargo install` or `pip install` actions needed for Wave 0.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Pre-migration baseline FAILS scenarios 3-6 | EXPAND-CTX-02 (D-09) | Single one-time signal proving the test matrix exercises the failure mode; running automatically would require holding two branches simultaneously | Before applying Plan 02 migration: `git stash` Plan 02 diff (or check out pre-migration commit), run `just test-adbc-queries`, observe scenarios 3-6 raise `Catalog Error: Table with name X does not exist`. Record the failing output in the plan's verification field, then re-apply Plan 02. |
| `_notes/error_with_adbc.md` close-out reads coherently | EXPAND-CTX-03 | Documentation quality is reviewer judgment, not automatable | Read the appended `## Resolution (v0.10.0)` section; confirm 2-3 sentences point at Phase 66's fix and reference the relevant commits. |

---

## Validation Sign-Off

- [ ] All tasks have automated verify or are documented Manual-Only
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references (new test file + justfile recipe)
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s for per-wave sampling
- [ ] `nyquist_compliant: true` set in frontmatter (set when plans land + map verifies)

**Approval:** pending
