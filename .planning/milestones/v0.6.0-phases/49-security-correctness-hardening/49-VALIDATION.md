---
phase: 49
slug: security-correctness-hardening
status: approved
nyquist_compliant: true
wave_0_complete: true
created: 2026-04-14
---

# Phase 49 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (#[test] + proptest), sqllogictest, pytest (DuckLake CI) |
| **Config file** | Cargo.toml, test/sql/*.test, test/integration/*.py |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~90 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 90 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 49-01-01 | 01 | 1 | SEC-02 | T-49-01, T-49-02 | Poisoned RwLock/Mutex returns error, not panic | unit | `cargo test -- catalog_insert_poisoned catalog_delete_poisoned catalog_upsert_poisoned catalog_delete_if_exists_poisoned catalog_rename_poisoned` | ✅ | ✅ green |
| 49-01-02 | 01 | 1 | SEC-04 | T-49-03 | Out-of-bounds row_idx triggers debug_assert | structural | `cargo test` (all data-path tests exercise read_typed_value) | ✅ | ✅ green |
| 49-02-01 | 02 | 2 | SEC-03 | T-49-08, T-49-09, T-49-10 | Cyclic derived metrics return error; depth > 64 returns error | unit | `cargo test -- toposort_derived_detects_cycle inline_derived_metrics_cycle inline_derived_metrics_depth max_derivation_depth` | ✅ | ✅ green |
| 49-02-02 | 02 | 2 | SEC-01 | T-49-04, T-49-05, T-49-06, T-49-07 | Rust panics caught at FFI boundary, not unwound through C++ | structural | `cargo test && just build` (31 catch_unwind sites verified by grep) | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements.

---

## Manual-Only Verifications

All phase behaviors have automated verification.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 90s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-14
