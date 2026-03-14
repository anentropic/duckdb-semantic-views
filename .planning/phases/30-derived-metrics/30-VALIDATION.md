---
phase: 30
slug: derived-metrics
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-14
---

# Phase 30 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (unit + proptest), sqllogictest, DuckLake CI |
| **Config file** | Cargo.toml, test/sql/TEST_LIST, justfile |
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
| 30-01-01 | 01 | 1 | DRV-01 | unit | `cargo test body_parser::tests::parse_derived_metric` | ❌ W0 | ⬜ pending |
| 30-01-02 | 01 | 1 | DRV-01 | unit | `cargo test body_parser::tests::parse_mixed_metrics` | ❌ W0 | ⬜ pending |
| 30-01-03 | 01 | 1 | DRV-01 | unit | `cargo test model::tests::derived_metric_no_source_table` | ❌ W0 | ⬜ pending |
| 30-02-01 | 02 | 1 | DRV-04 | unit | `cargo test graph::tests::derived_metric_cycle` | ❌ W0 | ⬜ pending |
| 30-02-02 | 02 | 1 | DRV-04 | unit | `cargo test graph::tests::derived_metric_unknown_ref` | ❌ W0 | ⬜ pending |
| 30-02-03 | 02 | 1 | DRV-05 | unit | `cargo test graph::tests::derived_metric_has_aggregate` | ❌ W0 | ⬜ pending |
| 30-02-04 | 02 | 1 | DRV-05 | unit | `cargo test graph::tests::derived_metric_no_aggregate_ok` | ❌ W0 | ⬜ pending |
| 30-03-01 | 03 | 2 | DRV-02 | unit | `cargo test expand::tests::inline_derived_metric` | ❌ W0 | ⬜ pending |
| 30-03-02 | 03 | 2 | DRV-02 | unit | `cargo test expand::tests::facts_then_derived` | ❌ W0 | ⬜ pending |
| 30-03-03 | 03 | 2 | DRV-03 | unit | `cargo test expand::tests::derived_metric_stacking` | ❌ W0 | ⬜ pending |
| 30-03-04 | 03 | 2 | ALL | unit | `cargo test expand::tests::derived_metric_join_resolution` | ❌ W0 | ⬜ pending |
| 30-04-01 | 04 | 3 | ALL | sqllogictest | `just test-sql` | ❌ W0 | ⬜ pending |
| 30-04-02 | 04 | 3 | ALL | proptest | `cargo test body_parser::tests::proptest_derived_metric` | ❌ W0 | ⬜ pending |
| 30-04-03 | 04 | 3 | ALL | sqllogictest | `just test-sql` (DESCRIBE) | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase30_derived_metrics.test` — end-to-end derived metrics DDL, query, stacking, error cases
- [ ] Unit tests for mixed qualified/unqualified metric parsing in body_parser.rs
- [ ] Unit tests for derived metric inlining (single-level, multi-level stacking) in expand.rs
- [ ] Unit tests for cycle detection, unknown reference, aggregate rejection in graph.rs
- [ ] Unit tests for join resolution with derived metrics
- [ ] Proptest for derived metric expression substitution edge cases
- [ ] Update TEST_LIST with phase30_derived_metrics.test

*Existing infrastructure covers framework and fixture needs.*

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
