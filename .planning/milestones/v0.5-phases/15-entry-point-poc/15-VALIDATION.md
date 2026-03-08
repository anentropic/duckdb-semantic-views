---
phase: 15
slug: entry-point-poc
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-07
audited: 2026-03-08
---

# Phase 15 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest (Python runner) + cargo test (Rust) + DuckLake CI (Python) |
| **Config file** | `test/sql/TEST_LIST` (sqllogictest), `Cargo.toml` (Rust tests) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Test File | Status |
|---------|------|------|-------------|-----------|-------------------|-----------|--------|
| 15-01-01 | 01 | 1 | BUILD-01 | build | `cargo build --no-default-features --features extension` | Implicit (build gate) | ✅ green |
| 15-01-02 | 01 | 1 | BUILD-02 | integration | `just test-sql` (extension load requires correct symbol) | `test/sql/phase16_parser.test` | ✅ green |
| 15-01-03 | 01 | 1 | ENTRY-02 | integration | `just test-sql` | `test/sql/phase16_parser.test:46` (`CREATE SEMANTIC VIEW`) | ✅ green |
| 15-01-04 | 01 | 1 | ENTRY-03 | integration | `just test-all` | Existing suite (102 Rust + 4 sqllogictest + 6 DuckLake CI) | ✅ green |
| 15-01-05 | 01 | 1 | ENTRY-01 | manual | N/A | `_notes/entry-point-decision.md` | ✅ verified |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] No new automated tests required for Phase 15 (spike) — subsequent phases added coverage
- [x] Phase 16/17 added `test/sql/phase16_parser.test` covering parser hook behavior
- [x] Manual verification superseded by automated tests in later phases

*Phase 15 was a spike. Parser hook coverage was added in Phase 16/17 (`phase16_parser.test`), which retroactively covers ENTRY-02.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Go/no-go decision recorded | ENTRY-01 | Documentation output, not testable behavior | Check `_notes/entry-point-decision.md` exists with rationale |

*BUILD-02 and ENTRY-02 previously manual-only — now covered by `phase16_parser.test` (extension load + `CREATE SEMANTIC VIEW` execution prove symbol visibility and parser hooks work).*

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

## Validation Audit 2026-03-08

| Metric | Count |
|--------|-------|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |

*Retroactive audit: all behavioral requirements covered by tests added in subsequent phases. ENTRY-01 (documentation deliverable) is appropriately manual-only.*
