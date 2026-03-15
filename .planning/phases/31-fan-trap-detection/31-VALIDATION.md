---
phase: 31
slug: fan-trap-detection
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-14
audited: 2026-03-15
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
| 31-01-01 | 01 | 1 | FAN-01 | unit | `cargo test cardinality_serde` | src/model.rs | ✅ green |
| 31-01-02 | 01 | 1 | FAN-01 | unit | `cargo test parse_relationship.*cardinality` | src/body_parser.rs | ✅ green |
| 31-01-03 | 01 | 1 | FAN-01 | unit | `cargo test old_json_without_cardinality` | src/model.rs | ✅ green |
| 31-01-04 | 01 | 1 | FAN-01 | proptest | `cargo test relationship_cardinality_keyword` | tests/parse_proptest.rs | ✅ green |
| 31-02-01 | 02 | 1 | FAN-02 | unit | `cargo test fan_trap_one_to_many_blocked` | src/expand.rs | ✅ green |
| 31-02-02 | 02 | 1 | FAN-02 | unit | `cargo test fan_trap_many_to_one_safe` | src/expand.rs | ✅ green |
| 31-02-03 | 02 | 1 | FAN-02 | unit | `cargo test fan_trap_one_to_one_safe` | src/expand.rs | ✅ green |
| 31-02-04 | 02 | 1 | FAN-02 | unit | `cargo test fan_trap_transitive_chain` | src/expand.rs | ✅ green |
| 31-02-05 | 02 | 1 | FAN-02 | unit | `cargo test fan_trap_derived_metric_blocked` | src/expand.rs | ✅ green |
| 31-03-01 | 03 | 2 | FAN-03 | sqllogictest | `just test-sql` | test/sql/phase31_fan_trap.test | ✅ green |
| 31-03-02 | 03 | 2 | FAN-03 | sqllogictest | `just test-sql` | test/sql/phase31_fan_trap.test | ✅ green |
| 31-03-03 | 03 | 2 | FAN-03 | sqllogictest | `just test-sql` | test/sql/phase31_fan_trap.test | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] Unit tests in `model.rs` for `Cardinality` enum serde (round-trip, default)
- [x] Unit tests in `body_parser.rs` for cardinality keyword parsing
- [x] Unit tests in `expand.rs` for `check_fan_traps` function
- [x] Proptest in `tests/parse_proptest.rs` for cardinality clause variations
- [x] sqllogictest `test/sql/phase31_fan_trap.test` for end-to-end scenarios
- [x] Add `test/sql/phase31_fan_trap.test` to `test/sql/TEST_LIST`

*All Wave 0 requirements satisfied during phase execution.*

---

## Test Coverage Summary

| Layer | Count | Files |
|-------|-------|-------|
| Unit (model.rs) | 4 cardinality serde/roundtrip tests | src/model.rs |
| Unit (body_parser.rs) | 7 cardinality parsing tests | src/body_parser.rs |
| Unit (expand.rs) | 8 fan trap detection tests | src/expand.rs |
| Proptest | 2 cardinality keyword generators | tests/parse_proptest.rs |
| sqllogictest | 9 end-to-end scenarios | test/sql/phase31_fan_trap.test |

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** complete

---

## Validation Audit 2026-03-15

| Metric | Count |
|--------|-------|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
