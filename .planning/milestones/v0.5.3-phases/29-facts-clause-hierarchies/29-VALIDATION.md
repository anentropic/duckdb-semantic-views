---
phase: 29
slug: facts-clause-hierarchies
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-14
audited: 2026-03-15
---

# Phase 29 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (unit + proptest) / sqllogictest / just test-ducklake-ci |
| **Config file** | Cargo.toml (proptest), tests/sql/ (sqllogictest) |
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
| 29-01-01 | 01 | 1 | FACT-01 | unit + sqllogictest | `cargo test fact && just test-sql` | src/graph.rs, src/body_parser.rs, test/sql/phase29 | ✅ green |
| 29-01-02 | 01 | 1 | FACT-02 | unit + sqllogictest | `cargo test fact_inline && just test-sql` | src/expand.rs, test/sql/phase29 | ✅ green |
| 29-01-03 | 01 | 1 | FACT-03 | unit + sqllogictest | `cargo test toposort_facts && just test-sql` | src/expand.rs, test/sql/phase29 | ✅ green |
| 29-01-04 | 01 | 1 | FACT-04 | unit + sqllogictest | `cargo test validate_facts && just test-sql` | src/graph.rs, test/sql/phase29 | ✅ green |
| 29-01-05 | 01 | 1 | FACT-05 | sqllogictest | `just test-sql` | test/sql/phase29 (Test 5) | ✅ green |
| 29-02-01 | 02 | 1 | HIER-01 | unit + sqllogictest | `cargo test hierarchy && just test-sql` | src/graph.rs, src/body_parser.rs, test/sql/phase29 | ✅ green |
| 29-02-02 | 02 | 1 | HIER-02 | unit + sqllogictest | `cargo test validate_hierarchies && just test-sql` | src/graph.rs, test/sql/phase29 (Test 8) | ✅ green |
| 29-02-03 | 02 | 1 | HIER-03 | sqllogictest | `just test-sql` | test/sql/phase29 (Test 5) | ✅ green |
| 29-03-01 | 03 | 1 | FACT-01..05 | proptest | `cargo test proptest` | tests/parse_proptest.rs | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] Test stubs for FACTS parsing, inlining, cycle detection
- [x] Test stubs for HIERARCHIES parsing, validation
- [x] Proptest generators for FACTS/HIERARCHIES clause input
- [x] sqllogictest files for end-to-end DDL + query

*All Wave 0 requirements satisfied during phase execution.*

---

## Test Coverage Summary

| Layer | Count | Files |
|-------|-------|-------|
| Unit (graph.rs) | 18 fact/hierarchy validation tests | src/graph.rs |
| Unit (expand.rs) | 23+ inline/replace/toposort tests | src/expand.rs |
| Unit (body_parser.rs) | 10 parsing tests | src/body_parser.rs |
| Proptest | 4 adversarial clause generators | tests/parse_proptest.rs |
| sqllogictest | 11 end-to-end cases | test/sql/phase29_facts_hierarchies.test |
| Fuzz seeds | 3 corpus entries | fuzz/seeds/fuzz_ddl_parse/ |

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
