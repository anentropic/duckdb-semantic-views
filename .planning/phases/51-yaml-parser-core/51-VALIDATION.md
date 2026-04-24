---
phase: 51
slug: yaml-parser-core
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-18
---

# Phase 51 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust unit/proptest/doc) + sqllogictest + just test-ducklake-ci |
| **Config file** | Cargo.toml (test config), test/sql/ (sqllogictest files) |
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
| 51-01-01 | 01 | 1 | YAML-03 | — | N/A | unit | `cargo test yaml` | ❌ W0 | ⬜ pending |
| 51-01-02 | 01 | 1 | YAML-05 | — | N/A | unit+proptest | `cargo test yaml` | ❌ W0 | ⬜ pending |
| 51-01-03 | 01 | 1 | YAML-09 | — | N/A | unit | `cargo test yaml` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/yaml.rs` — YAML parsing module with from_yaml function
- [ ] `src/yaml.rs` tests or `tests/yaml_tests.rs` — unit tests for YAML parsing

*Existing test infrastructure (cargo test, sqllogictest, just test-all) covers all framework needs.*

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
