---
phase: 54
slug: materialization-model-ddl
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-19
---

# Phase 54 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust (cargo test) + sqllogictest |
| **Config file** | `Cargo.toml` / `test/sqllogictest/*.slt` |
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
| 54-01-01 | 01 | 1 | MAT-01 | — | N/A | unit | `cargo test materialization` | ❌ W0 | ⬜ pending |
| 54-01-02 | 01 | 1 | MAT-06 | — | N/A | unit | `cargo test materialization` | ❌ W0 | ⬜ pending |
| 54-01-03 | 01 | 1 | MAT-07 | — | N/A | unit | `cargo test materialization` | ❌ W0 | ⬜ pending |
| 54-01-04 | 01 | 1 | MAT-01 | — | N/A | sqllogictest | `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Unit tests for `Materialization` struct parsing and serde
- [ ] Unit tests for MATERIALIZATIONS clause parsing
- [ ] sqllogictest for full DDL → query round-trip with materializations
- [ ] Tests for backward compatibility (pre-v0.7.0 JSON without materializations field)

*Existing test infrastructure (cargo test + sqllogictest) covers all framework needs.*

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
