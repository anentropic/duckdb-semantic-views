---
phase: 52
slug: yaml-ddl-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-18
---

# Phase 52 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) + sqllogictest runner |
| **Config file** | Cargo.toml + test/sql/*.test |
| **Quick run command** | `cargo test parse::tests::yaml` |
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
| 52-01-01 | 01 | 1 | YAML-01 | — | N/A | unit | `cargo test parse::tests::yaml` | ❌ W0 | ⬜ pending |
| 52-01-02 | 01 | 1 | YAML-01 | — | N/A | unit | `cargo test parse::tests::yaml` | ❌ W0 | ⬜ pending |
| 52-01-03 | 01 | 1 | YAML-01 | — | N/A | unit | `cargo test parse::tests::yaml` | ❌ W0 | ⬜ pending |
| 52-01-04 | 01 | 1 | YAML-06 | — | N/A | unit + sqllogictest | `cargo test parse::tests::yaml` + `just test-sql` | ❌ W0 | ⬜ pending |
| 52-01-05 | 01 | 1 | YAML-06 | — | N/A | unit + sqllogictest | `cargo test parse::tests::yaml` + `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase52_yaml_ddl.test` — sqllogictest for FROM YAML DDL integration
- [ ] Unit tests in `src/parse.rs` — `parse::tests::yaml*` test functions

*Existing infrastructure covers framework needs — no new test dependencies required.*

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
