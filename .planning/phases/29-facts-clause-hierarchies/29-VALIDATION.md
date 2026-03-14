---
phase: 29
slug: facts-clause-hierarchies
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-14
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
| 29-01-01 | 01 | 1 | FACT-01 | unit + sqllogictest | `cargo test fact && just test-sql` | ❌ W0 | ⬜ pending |
| 29-01-02 | 01 | 1 | FACT-02 | unit | `cargo test fact_inline` | ❌ W0 | ⬜ pending |
| 29-01-03 | 01 | 1 | FACT-03 | unit | `cargo test fact_topo` | ❌ W0 | ⬜ pending |
| 29-01-04 | 01 | 1 | FACT-04 | unit + sqllogictest | `cargo test fact_cycle` | ❌ W0 | ⬜ pending |
| 29-01-05 | 01 | 1 | FACT-05 | sqllogictest | `just test-sql` | ❌ W0 | ⬜ pending |
| 29-02-01 | 02 | 1 | HIER-01 | unit + sqllogictest | `cargo test hierarchy && just test-sql` | ❌ W0 | ⬜ pending |
| 29-02-02 | 02 | 1 | HIER-02 | unit | `cargo test hierarchy_valid` | ❌ W0 | ⬜ pending |
| 29-02-03 | 02 | 1 | HIER-03 | sqllogictest | `just test-sql` | ❌ W0 | ⬜ pending |
| 29-03-01 | 03 | 1 | FACT-01..05 | proptest + fuzz | `cargo test proptest_fact` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Test stubs for FACTS parsing, inlining, cycle detection
- [ ] Test stubs for HIERARCHIES parsing, validation
- [ ] Proptest generators for FACTS/HIERARCHIES clause input
- [ ] sqllogictest files for end-to-end DDL + query

*Existing cargo test + sqllogictest + proptest infrastructure covers framework needs. Only new test files/cases required.*

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
