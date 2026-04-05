---
phase: 37
slug: extract-shared-utilities
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-01
---

# Phase 37 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test + sqllogictest + just test-ducklake-ci |
| **Config file** | Cargo.toml (test config), justfile (task runner) |
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

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 37-01-01 | 01 | 1 | REF-04 | unit+integration | `cargo test` | ✅ | ⬜ pending |
| 37-01-02 | 01 | 1 | REF-04 | unit+integration | `cargo test` | ✅ | ⬜ pending |
| 37-02-01 | 02 | 1 | REF-03 | unit+integration | `cargo test` | ✅ | ⬜ pending |
| 37-02-02 | 02 | 1 | REF-03 | unit+integration | `cargo test` | ✅ | ⬜ pending |
| 37-XX-XX | XX | 2 | REF-03,REF-04 | full suite | `just test-all` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. No new test framework or stubs needed — this is a behavior-preserving refactor validated by existing 482+ tests.

---

## Manual-Only Verifications

All phase behaviors have automated verification. The refactor is purely structural — if all existing tests pass, the extraction is correct.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
