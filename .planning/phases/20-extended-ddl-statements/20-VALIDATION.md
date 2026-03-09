---
phase: 20
slug: extended-ddl-statements
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 20 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test + sqllogictest + DuckLake CI |
| **Config file** | justfile (task runner) |
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
| 20-01-01 | 01 | 1 | DDL-03 | integration (sqllogictest) | `just test-sql` | ❌ W0 | ⬜ pending |
| 20-01-02 | 01 | 1 | DDL-04 | integration (sqllogictest) | `just test-sql` | ❌ W0 | ⬜ pending |
| 20-01-03 | 01 | 1 | DDL-05 | integration (sqllogictest) | `just test-sql` | ❌ W0 | ⬜ pending |
| 20-01-04 | 01 | 1 | DDL-06 | integration (sqllogictest) | `just test-sql` | ❌ W0 | ⬜ pending |
| 20-01-05 | 01 | 1 | DDL-07 | integration (sqllogictest) | `just test-sql` | ❌ W0 | ⬜ pending |
| 20-01-06 | 01 | 1 | DDL-08 | integration (sqllogictest) | `just test-sql` | ❌ W0 | ⬜ pending |
| 20-01-07 | 01 | 1 | -- | unit (cargo test) | `cargo test` | ❌ W0 | ⬜ pending |
| 20-01-08 | 01 | 1 | -- | unit (cargo test) | `cargo test` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase20_extended_ddl.test` — sqllogictest covering DDL-03 through DDL-08
- [ ] Unit tests for `detect_semantic_view_ddl` — all 7 prefixes, case variations, negative cases
- [ ] Unit tests for rewrite functions — all 7 DDL forms, including SHOW (no-name case)
- No framework install needed — test infrastructure exists

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Three-connection lock during DROP | -- | Race condition timing | Run DROP SEMANTIC VIEW in rapid succession; verify no hang |

*All other behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending