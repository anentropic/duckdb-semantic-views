---
phase: 34
slug: duckdb-1-5-upgrade-lts-branch
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-16
---

# Phase 34 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust) + sqllogictest (Python runner) + uv-run integration tests |
| **Config file** | Cargo.toml, test/sql/TEST_LIST, justfile |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~120 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green on BOTH branches
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 34-01-xx | 01 | 1 | DKDB-01 | integration | `just test-all` (on main after upgrade) | ✅ existing | ⬜ pending |
| 34-01-xx | 01 | 1 | DKDB-05 | unit | `cat .duckdb-version` | ❌ W0 | ⬜ pending |
| 34-02-xx | 02 | 2 | DKDB-02 | integration | `just test-all` (on LTS branch) | ✅ existing | ⬜ pending |
| 34-02-xx | 02 | 2 | DKDB-03 | smoke | `git branch -r \| grep duckdb/1.4.x` | ❌ W0 | ⬜ pending |
| 34-02-xx | 02 | 2 | DKDB-04 | smoke | CI pipeline verification | ❌ W0 | ⬜ pending |
| 34-02-xx | 02 | 2 | DKDB-06 | smoke | `workflow_dispatch` trigger | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `.duckdb-version` file — version marker for branch identification
- [ ] `test/sql/peg_compat.test` — PEG parser compatibility smoke test (optional)

*Existing infrastructure covers most phase requirements — `just test-all` validates build + test correctness on each branch.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CI matrix runs both versions | DKDB-04 | Requires push to trigger CI | Push to both branches, verify GitHub Actions runs |
| Version monitor workflow | DKDB-06 | Requires `workflow_dispatch` | Trigger DuckDBVersionMonitor.yml manually |
| LTS branch exists on remote | DKDB-03 | Branch creation is manual | `git push origin duckdb/1.4.x` then verify |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
