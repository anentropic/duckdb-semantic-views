---
phase: 24
slug: pk-fk-model
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 24 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (proptest 1.9) + sqllogictest + Python integration |
| **Config file** | Cargo.toml, justfile |
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
| 24-01-01 | 01 | 1 | MDL-01 | unit | `cargo test model::tests::phase24 -x` | ❌ W0 | ⬜ pending |
| 24-01-02 | 01 | 1 | MDL-02 | unit | `cargo test model::tests::phase24 -x` | ❌ W0 | ⬜ pending |
| 24-01-03 | 01 | 1 | MDL-03 | unit | `cargo test model::tests::phase24 -x` | ❌ W0 | ⬜ pending |
| 24-01-04 | 01 | 1 | MDL-04 | unit | `cargo test model::tests::phase24 -x` | ❌ W0 | ⬜ pending |
| 24-01-05 | 01 | 1 | MDL-05 | unit | `cargo test model::tests::phase24 -x` | ❌ W0 | ⬜ pending |
| 24-01-06 | 01 | 1 | DDL-06 | unit | `cargo test ddl::parse_args -x` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/model.rs` — add `phase24_model_tests` test module with serde round-trip tests for new fields
- [ ] `src/model.rs` — add backward compat tests (old JSON without new fields deserializes correctly)
- [ ] `src/ddl/parse_args.rs` — add unit tests for qualified name parsing and new struct field extraction

*Existing infrastructure covers framework setup; only test stubs needed.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
