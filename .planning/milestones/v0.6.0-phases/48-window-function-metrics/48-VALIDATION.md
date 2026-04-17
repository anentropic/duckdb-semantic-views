---
phase: 48
slug: window-function-metrics
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-12
---

# Phase 48 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust unit + proptest + doc tests), sqllogictest, just test-ducklake-ci |
| **Config file** | Cargo.toml, tests/ directory |
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
| 48-01-01 | 01 | 1 | WIN-01 | — | N/A | unit | `cargo test window_spec` | ❌ W0 | ⬜ pending |
| 48-01-02 | 01 | 1 | WIN-01 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 48-02-01 | 02 | 2 | WIN-02 | — | N/A | unit | `cargo test window_metric` | ❌ W0 | ⬜ pending |
| 48-02-02 | 02 | 2 | WIN-03 | — | N/A | unit | `cargo test window_aggregate_mixing` | ❌ W0 | ⬜ pending |
| 48-02-03 | 02 | 2 | WIN-04 | — | N/A | unit | `cargo test fan_trap` | ✅ | ⬜ pending |
| 48-02-04 | 02 | 2 | WIN-05 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Window spec model + parser tests — stubs for WIN-01
- [ ] Window metric expansion tests — stubs for WIN-02
- [ ] Window/aggregate mixing error tests — stubs for WIN-03
- [ ] Fan trap interaction tests — extend existing fan trap tests for WIN-04
- [ ] SHOW DIMS FOR METRIC required=TRUE tests — stubs for WIN-05

*Existing test infrastructure (cargo test, sqllogictest, DuckLake CI) covers all framework needs.*

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
