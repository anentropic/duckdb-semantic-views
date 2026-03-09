---
phase: 19
slug: parser-hook-validation-spike
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 19 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test + sqllogictest + DuckLake CI |
| **Config file** | justfile (task runner) |
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
| 19-01-01 | 01 | 1 | SC-1 (7 prefixes tested) | integration | `just build && just test-sql` | No -- spike creates | ⬜ pending |
| 19-01-02 | 01 | 1 | SC-2 (error types recorded) | integration | `just build && just test-sql` | No -- spike creates | ⬜ pending |
| 19-01-03 | 01 | 1 | SC-3 (scope decision documented) | manual | Review SPIKE-RESULTS.md | No -- spike creates | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase19_parser_hook_validation.test` — sqllogictest covering all 7 DDL prefixes
- No framework install needed — test infrastructure exists

*Existing infrastructure covers all phase requirements.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Scope decision documented | SC-3 | Human judgment on scope | Review 19-SPIKE-RESULTS.md for completeness |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
