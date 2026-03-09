---
phase: 22
slug: documentation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 22 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest (DuckDB runner) + cargo test |
| **Config file** | Makefile (test-sql target) |
| **Quick run command** | `just test-sql` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Visual review of README changes
- **After every plan wave:** Run `just test-all` (confirm no regressions)
- **Before `/gsd:verify-work`:** Full suite must be green + README review
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 22-01-01 | 01 | 1 | DOC-01 | manual-only | `just test-all` (regression only) | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

*Existing infrastructure covers all phase requirements. No test infrastructure needed — this is a documentation-only phase.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| README includes DDL syntax reference with all 7 verbs | DOC-01 | Documentation content quality cannot be automated | Verify all 7 DDL verbs appear with examples, lifecycle example present |
| SQL code blocks in README are syntactically valid | DOC-01 | Cross-reference with test files | Compare README examples against passing sqllogictest cases |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
