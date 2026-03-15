---
phase: 30
slug: derived-metrics
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-14
audited: 2026-03-15
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
| 30-01-01 | 01 | 1 | DRV-01 | unit | `cargo test parse_metrics_clause` | src/body_parser.rs | ✅ green |
| 30-01-02 | 01 | 1 | DRV-01 | unit | `cargo test parse_keyword_body_with_derived` | src/body_parser.rs | ✅ green |
| 30-01-03 | 01 | 1 | DRV-01 | unit | `cargo test parse_keyword_body_only_derived` | src/body_parser.rs | ✅ green |
| 30-02-01 | 02 | 1 | DRV-04 | unit | `cargo test validate_derived_metrics_cycle` | src/graph.rs | ✅ green |
| 30-02-02 | 02 | 1 | DRV-04 | unit | `cargo test validate_derived_metrics_unknown` | src/graph.rs | ✅ green |
| 30-02-03 | 02 | 1 | DRV-05 | unit | `cargo test contains_aggregate` | src/graph.rs | ✅ green |
| 30-02-04 | 02 | 1 | DRV-05 | unit | `cargo test validate_derived_metrics_aggregate` | src/graph.rs | ✅ green |
| 30-03-01 | 03 | 2 | DRV-02 | unit | `cargo test inline_derived` | src/expand.rs | ✅ green |
| 30-03-02 | 03 | 2 | DRV-02 | unit | `cargo test expand_derived_metric_with_facts` | src/expand.rs | ✅ green |
| 30-03-03 | 03 | 2 | DRV-03 | unit | `cargo test inline_derived_stacked` | src/expand.rs | ✅ green |
| 30-03-04 | 03 | 2 | ALL | unit | `cargo test resolve_joins_includes_transitive` | src/expand.rs | ✅ green |
| 30-04-01 | 04 | 3 | ALL | sqllogictest | `just test-sql` | test/sql/phase30_derived_metrics.test | ✅ green |
| 30-04-02 | 04 | 3 | ALL | proptest | `cargo test derived_metric_parsing_no_panic` | tests/parse_proptest.rs | ✅ green |
| 30-04-03 | 04 | 3 | ALL | sqllogictest | `just test-sql` (DESCRIBE) | test/sql/phase30_derived_metrics.test | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `test/sql/phase30_derived_metrics.test` — end-to-end derived metrics DDL, query, stacking, error cases
- [x] Unit tests for mixed qualified/unqualified metric parsing in body_parser.rs
- [x] Unit tests for derived metric inlining (single-level, multi-level stacking) in expand.rs
- [x] Unit tests for cycle detection, unknown reference, aggregate rejection in graph.rs
- [x] Unit tests for join resolution with derived metrics
- [x] Proptest for derived metric expression substitution edge cases
- [x] Update TEST_LIST with phase30_derived_metrics.test

*All Wave 0 requirements satisfied during phase execution.*

---

## Test Coverage Summary

| Layer | Count | Files |
|-------|-------|-------|
| Unit (body_parser.rs) | 10 parse_metrics_clause tests | src/body_parser.rs |
| Unit (graph.rs) | 16 validate/aggregate/extract tests | src/graph.rs |
| Unit (expand.rs) | 8 inline_derived/toposort/collect tests | src/expand.rs |
| Proptest | 3 adversarial derived metric generators | tests/parse_proptest.rs |
| sqllogictest | 12 end-to-end cases | test/sql/phase30_derived_metrics.test |

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
