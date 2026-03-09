---
phase: 21
slug: error-location-reporting
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 21 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `#[test]` + sqllogictest (DuckDB test runner) |
| **Config file** | `test/sql/*.test` for sqllogictest; inline `#[cfg(test)] mod tests` for Rust |
| **Quick run command** | `cargo test -- parse` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -- parse`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 21-01-01 | 01 | 1 | ERR-01 | unit | `cargo test -- validate_ddl` | ❌ W0 | ⬜ pending |
| 21-01-02 | 01 | 1 | ERR-01 | integration | `just test-sql` (phase21_error_reporting.test) | ❌ W0 | ⬜ pending |
| 21-01-03 | 01 | 1 | ERR-02 | integration | `just test-sql` (phase21_error_reporting.test) | ❌ W0 | ⬜ pending |
| 21-01-04 | 01 | 1 | ERR-02 | unit | `cargo test -- parse_error_position` | ❌ W0 | ⬜ pending |
| 21-01-05 | 01 | 1 | ERR-03 | unit | `cargo test -- near_miss` | ❌ W0 | ⬜ pending |
| 21-01-06 | 01 | 1 | ERR-03 | unit | `cargo test -- clause_typo` | ❌ W0 | ⬜ pending |
| 21-01-07 | 01 | 1 | ERR-03 | integration | `just test-sql` (existing phase20 tests) | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase21_error_reporting.test` — integration tests for caret rendering and error messages through full extension load
- [ ] Rust unit tests in `src/parse.rs` for `validate_ddl_body()`, `detect_near_miss()`, position calculation

*Existing infrastructure covers ERR-03 view name suggestions (phase20 ViewNotFound tests).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Caret alignment in terminal | ERR-02 | Visual alignment depends on terminal rendering | Run malformed DDL in DuckDB CLI, verify caret points to correct position |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
