---
phase: 34
slug: duckdb-1-5-upgrade-lts-branch
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-16
nyquist_audited: 2026-03-27
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
| 34-01-xx | 01 | 1 | DKDB-01 | integration | `just test-all` (on main after upgrade) | ✅ existing | ✅ green |
| 34-01-xx | 01 | 1 | DKDB-05 | smoke | `bash test/infra/test_phase34_infra.sh` | ✅ created | ✅ green |
| 34-02-xx | 02 | 2 | DKDB-02 | manual | `git checkout duckdb/1.4.x && just test-all` | manual-only | manual |
| 34-02-xx | 02 | 2 | DKDB-03 | smoke | `bash test/infra/test_phase34_infra.sh` | ✅ created | ✅ green |
| 34-02-xx | 02 | 2 | DKDB-04 | smoke | `bash test/infra/test_phase34_infra.sh` | ✅ created | ✅ green |
| 34-02-xx | 02 | 2 | DKDB-06 | smoke | `bash test/infra/test_phase34_infra.sh` | ✅ created | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky · manual = cross-branch, inherently manual*

---

## Wave 0 Requirements

- [x] `.duckdb-version` file — version marker for branch identification (present, contains v1.5.0)
- [x] `test/sql/peg_compat.test` — PEG parser compatibility smoke test (100 lines, included in TEST_LIST)
- [x] `test/infra/test_phase34_infra.sh` — Infrastructure assertion script (22/22 passing)

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

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 120s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** nyquist-auditor 2026-03-27 — 22/22 infra assertions green; DKDB-02 classified manual-only (cross-branch)
