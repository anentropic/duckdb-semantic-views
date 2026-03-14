---
phase: 25
slug: sql-body-parser
status: verified
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-11
updated: 2026-03-11
---

# Phase 25 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (proptest 1.9) + sqllogictest + Python integration |
| **Config file** | Cargo.toml, justfile |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** ~60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 25-01-01 | 01 | 0 | DDL-01, DDL-02, DDL-03, DDL-04, DDL-05, DDL-07 | unit | `cargo test body_parser` | ✅ | ✅ green |
| 25-01-02 | 01 | 0 | DDL-01, DDL-07 | integration | `just test-sql` | ✅ | ✅ green |
| 25-01-03 | 01 | 0 | DDL-07 | proptest | `cargo test parse_proptest` | ✅ | ✅ green |
| 25-02-01 | 02 | 1 | DDL-01, DDL-02 | unit | `cargo test body_parser::tests` | ✅ | ✅ green |
| 25-02-02 | 02 | 1 | DDL-03 | unit | `cargo test body_parser::tests` | ✅ | ✅ green |
| 25-02-03 | 02 | 1 | DDL-04, DDL-05 | unit | `cargo test body_parser::tests` | ✅ | ✅ green |
| 25-03-01 | 03 | 2 | DDL-01, DDL-07 | integration | `just test-sql` | ✅ | ✅ green |
| 25-03-02 | 03 | 2 | DDL-07 | proptest | `cargo test parse_proptest` | ✅ | ✅ green |
| 25-04-01 | 04 | 3 | DDL-01, DDL-02, DDL-03, DDL-04, DDL-05, DDL-07 | integration | `just test-all` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `src/body_parser.rs` — create module with unit test stubs for TABLES, RELATIONSHIPS, DIMENSIONS, METRICS parsers
- [x] `test/sql/phase25_keyword_body.test` — sqllogictest integration: CREATE with keyword body, query, all 7 DDL verbs
- [x] `tests/parse_proptest.rs` — extend with proptest block for AS-body DDL round-trip and position invariants
- [x] `cpp/src/shim.cpp` — fix `char sql_buf[4096]` → `std::string(65536, '\0')` in both `sv_ddl_bind` and `sv_parse_stub`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Error caret position in DuckDB CLI output | DDL-01 | Visual inspection of terminal output needed | Run `CREATE SEMANTIC VIEW foo AS TABLSE (...) ...` in DuckDB CLI; verify caret points to "TABLSE" with correct offset |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** automated tests green (2026-03-11); human visual verification of caret position in DuckDB CLI approved (2026-03-12)
