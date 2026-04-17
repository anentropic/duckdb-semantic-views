---
phase: 45
slug: alter-comment-get-ddl
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-11
---

# Phase 45 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest-rs + cargo test (Rust unit/proptests) |
| **Config file** | `test/sql/TEST_LIST` for sqllogictest |
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

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 45-01-01 | 01 | 1 | ALT-01 | integration (slt) | `just test-sql` | ❌ W0 | ⬜ pending |
| 45-01-02 | 01 | 1 | ALT-02 | integration (slt) | `just test-sql` | ❌ W0 | ⬜ pending |
| 45-02-01 | 02 | 1 | SHOW-07 | integration (slt) + unit | `just test-sql` + `cargo test` | ❌ W0 | ⬜ pending |
| 45-02-02 | 02 | 1 | SHOW-08 | integration (slt) | `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase45_alter_comment.test` — covers ALT-01, ALT-02
- [ ] `test/sql/phase45_get_ddl.test` — covers SHOW-07, SHOW-08
- [ ] Unit tests for `render_create_ddl()` in `ddl/get_ddl.rs` — covers SHOW-07, SHOW-08
- [ ] Unit tests for ALTER parsing in `parse.rs` — covers ALT-01, ALT-02
- [ ] TEST_LIST update to include new .test files

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
