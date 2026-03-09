---
phase: 23
slug: parser-proptests-and-caret-integration-tests
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 23 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | proptest 1.9 (Rust) + Python duckdb (caret integration) |
| **Config file** | `tests/parse_proptest.rs` (new), `test/integration/test_caret_position.py` (new) |
| **Quick run command** | `cargo test parse_proptest` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test parse_proptest`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 23-01-01 | 01 | 1 | GAP-01 | proptest | `cargo test parse_proptest::detect` | ❌ W0 | ⬜ pending |
| 23-01-02 | 01 | 1 | GAP-02 | proptest | `cargo test parse_proptest::rewrite` | ❌ W0 | ⬜ pending |
| 23-01-03 | 01 | 1 | GAP-03 | proptest | `cargo test parse_proptest::position` | ❌ W0 | ⬜ pending |
| 23-01-04 | 01 | 1 | GAP-04 | proptest | `cargo test parse_proptest::near_miss` | ❌ W0 | ⬜ pending |
| 23-01-05 | 01 | 1 | GAP-05 | proptest | `cargo test parse_proptest::brackets` | ❌ W0 | ⬜ pending |
| 23-01-06 | 01 | 1 | GAP-06 | integration | `uv run test/integration/test_caret_position.py` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/parse_proptest.rs` — proptest PBTs for parser functions (GAP-01 through GAP-05)
- [ ] `test/integration/test_caret_position.py` — Python caret position verification (GAP-06)
- [ ] Update `justfile` if needed to include caret test in `test-all`

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
