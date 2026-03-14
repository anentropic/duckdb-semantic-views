---
phase: 28
slug: integration-testing-documentation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 28 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest (Python runner) + cargo test (Rust) + uv (Python integration) |
| **Config file** | `test/sql/TEST_LIST` |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 28-01-01 | 01 | 1 | (implicit) | unit | `cargo test` | ✅ | ⬜ pending |
| 28-01-02 | 01 | 1 | (implicit) | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 28-02-01 | 02 | 1 | (implicit) | integration | `just test-all` | ✅ (rewritten) | ⬜ pending |
| 28-03-01 | 03 | 2 | DOC-01 | manual | N/A (documentation review) | ❌ (will create) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase28_e2e.test` — 3-table E2E integration test (NEW)
- [ ] Update `test/sql/TEST_LIST` — add `phase28_e2e.test`, remove `phase2_ddl.test` and `semantic_views.test`

*Existing infrastructure covers most phase requirements. Only the E2E test file and TEST_LIST update are needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| README content quality | DOC-01 | Documentation review requires human judgment | Review README for accuracy, completeness, correct syntax examples |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
