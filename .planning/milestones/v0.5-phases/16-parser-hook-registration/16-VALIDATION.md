---
phase: 16
slug: parser-hook-registration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 16 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust std test + DuckDB sqllogictest runner |
| **Config file** | Cargo.toml `[dev-dependencies]` + `test/sql/TEST_LIST` |
| **Quick run command** | `cargo test parse` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test parse`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 16-01-01 | 01 | 1 | PARSE-02 | unit | `cargo test parse` | Wave 0 | ⬜ pending |
| 16-01-02 | 01 | 1 | PARSE-05 | unit | `cargo test parse` | Wave 0 | ⬜ pending |
| 16-01-03 | 01 | 1 | PARSE-04 | integration | `just test-sql` | Wave 0 | ⬜ pending |
| 16-01-04 | 01 | 1 | PARSE-01 | integration | `just test-sql` | Wave 0 | ⬜ pending |
| 16-01-05 | 01 | 1 | PARSE-03 | integration | `just test-sql` | Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/parse.rs` — new module with `detect_create_semantic_view()` + FFI entry point
- [ ] `test/sql/phase16_parser.test` — sqllogictest exercising parser hook chain
- [ ] `test/sql/TEST_LIST` — add `test/sql/phase16_parser.test` entry
- [ ] `src/lib.rs` — add `pub mod parse;` declaration

*Wave 0 creates test stubs alongside implementation.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Python DuckDB LOAD + CREATE SEMANTIC VIEW | PARSE-01 | Requires Python client with `-fvisibility=hidden` | `python -c "import duckdb; c=duckdb.connect(); c.execute('LOAD ...'); c.execute('CREATE SEMANTIC VIEW test ...')"` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
