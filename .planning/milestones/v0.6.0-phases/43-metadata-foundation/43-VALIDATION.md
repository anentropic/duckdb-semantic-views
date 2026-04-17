---
phase: 43
slug: metadata-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-10
---

# Phase 43 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | proptest + sqllogictest + cargo test |
| **Config file** | Cargo.toml (proptest), Makefile (sqllogictest) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 43-01-01 | 01 | 1 | META-01 | — | N/A | unit + sqllogictest | `cargo test model::tests` + `just test-sql` | ❌ W0 | ⬜ pending |
| 43-01-02 | 01 | 1 | META-06 | T-43-01 | Parameterized persistence prevents SQL injection via COMMENT | unit + sqllogictest | `cargo test model::tests` + restart test in sqllogictest | ❌ W0 | ⬜ pending |
| 43-01-03 | 01 | 1 | META-07 | — | N/A | unit | `cargo test model::tests::pre_v060` | ❌ W0 | ⬜ pending |
| 43-02-01 | 02 | 2 | META-02 | — | N/A | unit + sqllogictest | `cargo test body_parser::tests` + `just test-sql` | ❌ W0 | ⬜ pending |
| 43-02-02 | 02 | 2 | META-03 | — | N/A | unit + sqllogictest | `cargo test body_parser::tests` + `just test-sql` | ❌ W0 | ⬜ pending |
| 43-02-03 | 02 | 2 | META-04 | — | N/A | unit + sqllogictest | `cargo test body_parser::tests` + `just test-sql` | ❌ W0 | ⬜ pending |
| 43-02-04 | 02 | 2 | META-05 | — | PRIVATE items excluded from query results | unit | `cargo test expand::` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase43_metadata.test` — stubs for META-01 through META-06 DDL and query tests
- [ ] Backward compat unit tests in model.rs for pre-v0.6.0 JSON (META-07)
- [ ] Expansion unit tests for PRIVATE rejection in expand/sql_gen.rs tests (META-05)

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
