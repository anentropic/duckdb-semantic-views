---
phase: 55
slug: materialization-routing-engine
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-19
---

# Phase 55 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust (cargo test) + sqllogictest |
| **Config file** | Cargo.toml, test/sql/*.slt |
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
| 55-01-01 | 01 | 1 | MAT-02 | — | N/A | unit + integration | `cargo test materialization_routing` | ❌ W0 | ⬜ pending |
| 55-01-02 | 01 | 1 | MAT-03 | — | N/A | unit | `cargo test materialization_fallback` | ❌ W0 | ⬜ pending |
| 55-01-03 | 01 | 1 | MAT-04 | — | N/A | unit | `cargo test semi_additive_skip` | ❌ W0 | ⬜ pending |
| 55-01-04 | 01 | 1 | MAT-05 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |

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
