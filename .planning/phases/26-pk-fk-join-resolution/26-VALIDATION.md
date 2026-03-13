---
phase: 26
slug: pk-fk-join-resolution
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 26 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test + proptest 1.9 + sqllogictest |
| **Config file** | Cargo.toml (dev-dependencies) |
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
| 26-01-01 | 01 | 1 | EXP-02 | unit | `cargo test --lib graph::tests::pkfk_on_clause` | ❌ W0 | ⬜ pending |
| 26-01-02 | 01 | 1 | EXP-02 | unit | `cargo test --lib expand::tests::left_join` | ❌ W0 | ⬜ pending |
| 26-01-03 | 01 | 1 | EXP-02 | unit | `cargo test --lib graph::tests::composite_pkfk` | ❌ W0 | ⬜ pending |
| 26-01-04 | 01 | 1 | EXP-03 | unit | `cargo test --lib graph::tests::toposort` | ❌ W0 | ⬜ pending |
| 26-01-05 | 01 | 1 | EXP-03 | unit | `cargo test --lib graph::tests::toposort_deterministic` | ❌ W0 | ⬜ pending |
| 26-01-06 | 01 | 1 | EXP-04 | unit | `cargo test --lib expand::tests::transitive_pkfk` | ❌ W0 | ⬜ pending |
| 26-01-07 | 01 | 1 | EXP-04 | unit | `cargo test --lib expand::tests::pruning_pkfk` | ❌ W0 | ⬜ pending |
| 26-01-08 | 01 | 1 | EXP-06 | unit | `cargo test --lib graph::tests::cycle_detected` | ❌ W0 | ⬜ pending |
| 26-01-09 | 01 | 1 | EXP-06 | unit | `cargo test --lib graph::tests::diamond_detected` | ❌ W0 | ⬜ pending |
| 26-01-10 | 01 | 1 | EXP-06 | unit | `cargo test --lib graph::tests::self_ref` | ❌ W0 | ⬜ pending |
| 26-01-11 | 01 | 1 | EXP-06 | unit | `cargo test --lib graph::tests::orphan_table` | ❌ W0 | ⬜ pending |
| 26-01-12 | 01 | 1 | EXP-06 | unit | `cargo test --lib graph::tests::unreachable_source` | ❌ W0 | ⬜ pending |
| 26-01-13 | 01 | 1 | EXP-06 | unit | `cargo test --lib graph::tests::fk_pk_count_mismatch` | ❌ W0 | ⬜ pending |
| 26-02-01 | 02 | 2 | EXP-02 | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 26-02-02 | 02 | 2 | EXP-04 | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 26-02-03 | 02 | 2 | EXP-06 | integration | `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/graph.rs` — new module with RelationshipGraph, validate_graph(), toposort(), all validation functions + unit tests
- [ ] `test/sql/phase26_join_resolution.test` — sqllogictest integration tests for PK/FK join synthesis, transitive inclusion, error cases
- [ ] Update `tests/expand_proptest.rs` — add property tests for PK/FK join definitions

*Wave 0 stubs all test files needed by plans.*

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
