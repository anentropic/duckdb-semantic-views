---
phase: 32
slug: role-playing-using
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-14
---

# Phase 32 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (unit + proptest) + sqllogictest-bin + DuckLake CI |
| **Config file** | Cargo.toml (dev-dependencies), justfile (test recipes) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60s

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 32-01-01 | 01 | 1 | JOIN-01 | unit | `cargo test graph::tests::diamond_relaxation` | Wave 0 | ⬜ pending |
| 32-01-02 | 01 | 1 | JOIN-02 | unit + proptest | `cargo test body_parser::tests::parse_metrics_using` | Wave 0 | ⬜ pending |
| 32-01-03 | 01 | 1 | JOIN-04 | unit | `cargo test graph::tests::validate_using` | Wave 0 | ⬜ pending |
| 32-02-01 | 02 | 2 | JOIN-03 | unit | `cargo test expand::tests::using_scoped_aliases` | Wave 0 | ⬜ pending |
| 32-02-02 | 02 | 2 | JOIN-05 | unit | `cargo test expand::tests::ambiguous_path_error` | Wave 0 | ⬜ pending |
| 32-02-03 | 02 | 2 | ROLE-01 | unit | `cargo test expand::tests::role_playing_aliases` | Wave 0 | ⬜ pending |
| 32-02-04 | 02 | 2 | ROLE-02 | unit | `cargo test expand::tests::dimension_using_resolution` | Wave 0 | ⬜ pending |
| 32-02-05 | 02 | 2 | ROLE-03 | sqllogictest | `just test-sql` | Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase32_role_playing.test` — stubs for ROLE-03 end-to-end
- [ ] Unit tests for diamond relaxation, USING parsing, scoped alias generation, ambiguity detection
- [ ] Proptest for USING clause parsing with adversarial input
- [ ] Fuzz target for USING clause parsing (covered by existing `fuzz_ddl_parse`)

*Existing infrastructure covers framework and fixture needs.*

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
