---
phase: 38
slug: module-directory-splits
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-01
---

# Phase 38 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test framework + proptest + sqllogictest |
| **Config file** | Cargo.toml, .sqllogictest/ directory |
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
| 38-01-* | 01 | 1 | REF-01 | compilation + unit | `cargo test` | Existing (moved) | ⬜ pending |
| 38-02-* | 02 | 1 | REF-02 | compilation + unit | `cargo test` | Existing (moved) | ⬜ pending |
| 38-*-final | * | * | REF-05 | full suite | `just test-all` | Existing | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. This phase moves tests between files but does not need new test files. The 390+ existing tests serve as the regression suite.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| No `expand.rs` or `graph.rs` in `src/` | REF-01, REF-02 | Structural check | `ls src/expand.rs src/graph.rs` should return "No such file" |
| `src/expand/mod.rs` and `src/graph/mod.rs` exist | REF-01, REF-02 | Structural check | `ls src/expand/mod.rs src/graph/mod.rs` should succeed |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
