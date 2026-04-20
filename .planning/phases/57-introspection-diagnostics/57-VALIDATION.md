---
phase: 57
slug: introspection-diagnostics
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-20
---

# Phase 57 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest + cargo test |
| **Config file** | `test/sql/TEST_LIST` |
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
| 57-01-01 | 01 | 1 | INTR-01 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 57-01-02 | 01 | 1 | INTR-01 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 57-01-03 | 01 | 1 | INTR-02 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 57-01-04 | 01 | 1 | INTR-02 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 57-01-05 | 01 | 1 | INTR-03 | T-57-01 | SQL injection prevented by existing safe_name escaping | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 57-01-06 | 01 | 1 | INTR-03 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 57-01-07 | 01 | 1 | INTR-03 | — | N/A | unit | `cargo test` | ❌ W0 | ⬜ pending |
| 57-01-08 | 01 | 1 | INTR-03 | — | N/A | unit | `cargo test` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase57_introspection.test` — stubs for INTR-01, INTR-02, INTR-03
- [ ] `test/sql/TEST_LIST` — add phase57 entry
- [ ] Parse detection unit tests in `parse.rs` `#[cfg(test)]` — INTR-03

*Existing infrastructure covers framework needs.*

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
