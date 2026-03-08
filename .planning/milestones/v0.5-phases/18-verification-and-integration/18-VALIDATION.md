---
phase: 18
slug: verification-and-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-08
---

# Phase 18 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust), `sqllogictest` (Python runner), `uv` (Python scripts) |
| **Config file** | `test/sql/TEST_LIST` (sqllogictest), `Cargo.toml` (Rust tests) |
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
| 18-01-01 | 01 | 1 | VERIFY-01 | integration | `just test-all` | Yes | ⬜ pending |
| 18-01-02 | 01 | 1 | VERIFY-02 | sqllogictest | `just test-sql` | Yes (feat branch) | ⬜ pending |
| 18-01-03 | 01 | 1 | BUILD-04 | structural | `cargo test` | Yes (build.rs) | ⬜ pending |
| 18-01-04 | 01 | 1 | BUILD-05 | smoke | `strings build/debug/*.duckdb_extension \| grep C_STRUCT_UNSTABLE` | Yes | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test-vtab-crash` Justfile target — add to `test-all` dependency chain
- [ ] `test/sql/TEST_LIST` — verify `phase16_parser.test` entry present after cherry-pick

*Existing infrastructure covers most phase requirements.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| ABI footer type | BUILD-05 | Binary inspection | `strings build/debug/*.duckdb_extension \| grep C_STRUCT_UNSTABLE` |
| No CMake dependency | BUILD-05 | Build system check | Verify no `CMakeLists.txt` in build output |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
