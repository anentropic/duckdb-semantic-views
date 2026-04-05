---
phase: 42
slug: refactor-tidy-ups-and-test-reorganisation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-04
---

# Phase 42 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) + proptest 1.x + sqllogictest (Python runner) |
| **Config file** | Cargo.toml `[dev-dependencies]` + Justfile test targets |
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

| Task ID | Plan | Wave | Item | Test Type | Automated Command | File Exists | Status |
|---------|------|------|------|-----------|-------------------|-------------|--------|
| 42-01-01 | 01 | 1 | TOCTOU fix | unit | `cargo test catalog` | ✅ existing | ⬜ pending |
| 42-01-02 | 01 | 1 | Parameterized queries | integration | `just test-sql` | ✅ existing | ⬜ pending |
| 42-02-01 | 02 | 1 | Test fixture extraction | unit | `cargo test expand` | ✅ existing | ⬜ pending |
| 42-02-02 | 02 | 1 | Body parser comments | N/A | N/A | N/A | ⬜ pending |
| 42-03-01 | 03 | 2 | File-backed round-trip test | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 42-03-02 | 03 | 2 | Transmute layout guard | unit | `cargo test value_layout` | ❌ W0 | ⬜ pending |
| 42-03-03 | 03 | 2 | Suggestion proptest | proptest | `cargo test suggest` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase42_persistence.test` — file-backed catalog round-trip test
- [ ] Transmute layout assertion test (in existing table_function.rs test module)
- [ ] `suggest_closest` property test (in src/util.rs tests)

*Existing infrastructure covers most phase requirements. Three new test files/blocks needed.*

---

## Manual-Only Verifications

| Behavior | Item | Why Manual | Test Instructions |
|----------|------|------------|-------------------|
| Body parser invariant comments | Comments task | Documentation only, no behavior change | Code review: verify comments explain paren-balance invariant |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
