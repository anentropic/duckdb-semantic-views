---
phase: 27
slug: alias-based-query-expansion
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 27 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` + `sqllogictest` runner |
| **Config file** | `justfile` (project root) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just build && just test-sql`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 27-01-01 | 01 | 0 | EXP-05 | slt | `just test-sql` | ❌ W0 | ⬜ pending |
| 27-01-02 | 01 | 0 | CLN-01 | unit+slt | `cargo test -- parse && just test-sql` | ❌ W0 | ⬜ pending |
| 27-01-03 | 01 | 0 | CLN-03 | unit | `cargo test -- expand` | ❌ W0 | ⬜ pending |
| 27-02-01 | 02 | 1 | CLN-01 | unit+slt | `cargo test -- parse` | ✅ existing | ⬜ pending |
| 27-02-02 | 02 | 1 | CLN-03 | unit | `cargo test -- expand` | ✅ existing | ⬜ pending |
| 27-03-01 | 03 | 1 | EXP-05 | slt | `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase27_qualified_refs.test` — sqllogictest for EXP-05: dot-qualified expressions in SELECT, GROUP BY correctness
- [ ] Unit test in `src/expand.rs` asserting `expand()` output contains qualified column refs verbatim
- [ ] Audit old `.test` files that use paren-body DDL syntax (prerequisite for CLN-01)

*Existing test infrastructure covers most paths; Wave 0 adds targeted tests for cleanup verification.*

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
