---
phase: 15
slug: entry-point-poc
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 15 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | sqllogictest (Python runner) + cargo test (Rust) + DuckLake CI (Python) |
| **Config file** | `test/sql/TEST_LIST` (sqllogictest), `Cargo.toml` (Rust tests) |
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
| 15-01-01 | 01 | 1 | BUILD-01 | build | `cargo build --no-default-features --features extension` | Implicit | ⬜ pending |
| 15-01-02 | 01 | 1 | BUILD-02 | smoke | `nm -gU target/debug/libsemantic_views.dylib \| grep duckdb_cpp_init` | Wave 0 | ⬜ pending |
| 15-01-03 | 01 | 1 | ENTRY-02 | smoke | `just build && duckdb -cmd "LOAD 'build/debug/semantic_views.duckdb_extension'"` | Wave 0 | ⬜ pending |
| 15-01-04 | 01 | 1 | ENTRY-03 | integration | `just test-all` | Existing | ⬜ pending |
| 15-01-05 | 01 | 1 | ENTRY-01 | manual | Only attempted if Option B fails | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] No new automated tests required for Phase 15 (spike) per user decision
- [ ] Phase 16 will add sqllogictest coverage for parser hook behavior
- [ ] Manual verification: `LOAD` extension in DuckDB CLI, run `CREATE SEMANTIC VIEW test ...` to trigger stub

*Phase 15 is a spike — formal test coverage deferred to Phase 16 per user decision. Existing infrastructure (`just test-all`) covers regression testing.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Stub parser hook fires on `CREATE SEMANTIC VIEW` | ENTRY-02 | Spike — no parser test infrastructure yet | `LOAD` extension, run `CREATE SEMANTIC VIEW test ...`, verify stub response |
| Symbol visibility correct | BUILD-02 | Build artifact inspection | `nm -gU target/debug/libsemantic_views.dylib \| grep duckdb_cpp_init` |
| Go/no-go decision recorded | ENTRY-01 | Documentation output | Check `_notes/entry-point-decision.md` exists with rationale |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
