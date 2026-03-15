---
phase: 32
slug: role-playing-using
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-14
audited: 2026-03-15
---

# Phase 32 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (unit + proptest) + sqllogictest-bin + DuckLake CI |
| **Config file** | Cargo.toml (dev-dependencies), justfile (test recipes) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60s

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 32-01-01 | 01 | 1 | JOIN-01 | unit | `cargo test diamond` | src/graph.rs | ✅ green |
| 32-01-02 | 01 | 1 | JOIN-02 | unit + proptest | `cargo test parse_metrics_using` | src/body_parser.rs, tests/parse_proptest.rs | ✅ green |
| 32-01-03 | 01 | 1 | JOIN-04 | unit | `cargo test validate_using` | src/graph.rs | ✅ green |
| 32-02-01 | 02 | 2 | JOIN-03 | unit | `cargo test scoped_join\|using_metric_generates` | src/expand.rs | ✅ green |
| 32-02-02 | 02 | 2 | JOIN-05 | unit | `cargo test ambiguous` | src/expand.rs | ✅ green |
| 32-02-03 | 02 | 2 | ROLE-01 | unit | `cargo test two_using_metrics\|using_metric_generates` | src/expand.rs | ✅ green |
| 32-02-04 | 02 | 2 | ROLE-02 | unit | `cargo test dimension_rewritten\|base_table_dimension` | src/expand.rs | ✅ green |
| 32-02-05 | 02 | 2 | ROLE-03 | sqllogictest | `just test-sql` | test/sql/phase32_role_playing.test | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `test/sql/phase32_role_playing.test` — stubs for ROLE-03 end-to-end
- [x] Unit tests for diamond relaxation, USING parsing, scoped alias generation, ambiguity detection
- [x] Proptest for USING clause parsing with adversarial input
- [x] Fuzz target for USING clause parsing (covered by existing `fuzz_ddl_parse`)

*All Wave 0 requirements satisfied during phase execution.*

---

## Test Coverage Summary

| Layer | Count | Files |
|-------|-------|-------|
| Unit (model.rs) | 3 using_relationships serde tests | src/model.rs |
| Unit (body_parser.rs) | 6 USING parsing tests | src/body_parser.rs |
| Unit (graph.rs) | 7 diamond/validate_using tests | src/graph.rs |
| Unit (expand.rs) | 13 role-playing expansion tests | src/expand.rs |
| Proptest | 1 USING clause generator | tests/parse_proptest.rs |
| sqllogictest | 10 end-to-end scenarios | test/sql/phase32_role_playing.test |

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** complete

---

## Validation Audit 2026-03-15

| Metric | Count |
|--------|-------|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
