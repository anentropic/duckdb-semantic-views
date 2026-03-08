---
phase: 17
slug: ddl-execution
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 17 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest (Python runner) + cargo test (Rust) + DuckLake CI (Python) |
| **Config file** | `test/sql/TEST_LIST` (sqllogictest), `Cargo.toml` (Rust tests) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 17-01-01 | 01 | 1 | DDL-01 | unit | `cargo test parse_ddl` | ❌ W0 | ⬜ pending |
| 17-01-02 | 01 | 1 | DDL-01 | unit | `cargo test rewrite_ddl` | ❌ W0 | ⬜ pending |
| 17-01-03 | 01 | 1 | DDL-01 | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 17-01-04 | 01 | 1 | DDL-02 | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 17-01-05 | 01 | 1 | DDL-03 | integration | `just test-sql` | ✅ existing | ⬜ pending |
| 17-01-06 | 01 | 1 | BUILD-03 | integration | `just test-sql && just test-ducklake-ci` | ✅ existing | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Add Rust unit tests for `parse_ddl_text()` in `src/parse.rs`
- [ ] Add Rust unit tests for `rewrite_ddl_to_function_call()` in `src/parse.rs`
- [ ] Update `test/sql/phase16_parser.test` — replace stub assertions with real DDL execution tests (create view via native DDL, query it, verify results)
- [ ] Add DDL-02 test case: view created via native DDL is queryable via `semantic_view()`

*Existing infrastructure covers DDL-03 (phase2_ddl.test, phase4_query.test) and BUILD-03 (semantic_views.test + DuckLake CI).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Python client LOAD | BUILD-03 | DuckLake CI covers this but may need manual verification if CI environment differs | `python -c "import duckdb; c=duckdb.connect(); c.load_extension('path/to/ext')"` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
