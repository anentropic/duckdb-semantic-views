---
phase: 31
slug: fan-trap-detection
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-14
---

# Phase 31 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust test + proptest 1.9 + sqllogictest |
| **Config file** | `Cargo.toml` (dev-dependencies), `test/sql/TEST_LIST` |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 31-01-01 | 01 | 1 | FAN-01 | unit | `cargo test -- cardinality` | No -- Wave 0 | ⬜ pending |
| 31-01-02 | 01 | 1 | FAN-01 | unit | `cargo test -- cardinality` | No -- Wave 0 | ⬜ pending |
| 31-01-03 | 01 | 1 | FAN-01 | unit | `cargo test -- cardinality` | No -- Wave 0 | ⬜ pending |
| 31-01-04 | 01 | 1 | FAN-01 | proptest | `cargo test -- cardinality` | No -- Wave 0 | ⬜ pending |
| 31-02-01 | 02 | 1 | FAN-02 | unit | `cargo test -- fan_trap` | No -- Wave 0 | ⬜ pending |
| 31-02-02 | 02 | 1 | FAN-02 | unit | `cargo test -- fan_trap` | No -- Wave 0 | ⬜ pending |
| 31-02-03 | 02 | 1 | FAN-02 | unit | `cargo test -- fan_trap` | No -- Wave 0 | ⬜ pending |
| 31-02-04 | 02 | 1 | FAN-02 | unit | `cargo test -- fan_trap` | No -- Wave 0 | ⬜ pending |
| 31-02-05 | 02 | 1 | FAN-02 | unit | `cargo test -- fan_trap` | No -- Wave 0 | ⬜ pending |
| 31-03-01 | 03 | 2 | FAN-03 | integration (slt) | `just test-sql` | No -- Wave 0 | ⬜ pending |
| 31-03-02 | 03 | 2 | FAN-03 | integration (slt) | `just test-sql` | No -- Wave 0 | ⬜ pending |
| 31-03-03 | 03 | 2 | FAN-03 | integration (slt) | `just test-sql` | No -- Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Unit tests in `model.rs` for `Cardinality` enum serde (round-trip, default)
- [ ] Unit tests in `body_parser.rs` for cardinality keyword parsing
- [ ] Unit tests in `expand.rs` for `check_fan_traps` function
- [ ] Proptest in `tests/parse_proptest.rs` for cardinality clause variations
- [ ] sqllogictest `test/sql/phase31_fan_trap.test` for end-to-end scenarios
- [ ] Add `test/sql/phase31_fan_trap.test` to `test/sql/TEST_LIST`

*Existing infrastructure covers framework installation — only test file stubs needed.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
