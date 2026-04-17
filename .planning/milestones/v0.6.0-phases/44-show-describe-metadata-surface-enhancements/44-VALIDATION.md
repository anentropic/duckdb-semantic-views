---
phase: 44
slug: show-describe-metadata-surface-enhancements
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-10
---

# Phase 44 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test + sqllogictest + just test-ducklake-ci |
| **Config file** | Cargo.toml, test/sql/*.test |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 44-01-01 | 01 | 1 | SHOW-01 | — | N/A | unit+sql | `cargo test` + `just test-sql` | ✅ | ⬜ pending |
| 44-01-02 | 01 | 1 | SHOW-06 | — | N/A | unit+sql | `cargo test` + `just test-sql` | ✅ | ⬜ pending |
| 44-02-01 | 02 | 2 | SHOW-02 | — | N/A | unit+sql | `cargo test` + `just test-sql` | ✅ | ⬜ pending |
| 44-02-02 | 02 | 2 | SHOW-03 | — | N/A | unit+sql | `cargo test` + `just test-sql` | ✅ | ⬜ pending |
| 44-02-03 | 02 | 2 | SHOW-04 | — | N/A | unit+sql | `cargo test` + `just test-sql` | ✅ | ⬜ pending |
| 44-02-04 | 02 | 2 | SHOW-05 | — | N/A | unit+sql | `cargo test` + `just test-sql` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

*Existing infrastructure covers all phase requirements.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
