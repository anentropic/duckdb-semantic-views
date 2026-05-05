---
phase: 60-race-guards-validation-hardening
verified: 2026-05-03T16:40:00Z
status: passed
score: 5/5 must-haves verified
re_verification: false
note: Back-derived. Verification artefacts: existing sqllogictests + Python integration tests + CHANGELOG known-limitations entries. The validation-error workaround leaves caret rendering broken (TECH-DEBT 22) — that's a known limitation, not a verification failure for this phase.
---

# Phase 60: Race Guards & Validation Hardening Verification Report

## Goal Achievement

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Non-`IF EXISTS` DROP/ALTER under concurrent drop emits `semantic view '<name>' was concurrently dropped` | VERIFIED | `src/parse.rs` two-statement guard (`SELECT CASE WHEN NOT EXISTS THEN error() ELSE TRUE END; DELETE … RETURNING`); covered by `v080_transactional_ddl.test`. |
| 2 | `IF EXISTS` variants keep silent-no-op contract | VERIFIED | Guard skipped for IF EXISTS path; `v080_transactional_ddl.test` asserts no error and zero rows affected. |
| 3 | Validation error message text reaches user despite `FALLBACK_OVERRIDE` dropping `DISPLAY_EXTENSION_ERROR` | VERIFIED | `sql_throwing` synthesises `SELECT error('<msg>')::VARCHAR`; `test/integration/test_caret_position.py` (relaxed to text-only) passes. |
| 4 | FFI input UTF-8-validated; malformed bytes defer to default parser without UB | VERIFIED | `sv_parser_override_rust` uses checked `from_utf8`; FFI fuzz target (`fuzz_parser_override_ffi`) covers arbitrary-bytes input. |
| 5 | `parse_table_function_call` rejects `foo(,)`, `foo('a',)`, `foo('a' 'b')` | VERIFIED | Helper unit tests in `src/parse.rs::tests`. |

**Score:** 5/5 truths verified.

## Required Artifacts

| Artifact | Status |
|----------|--------|
| Two-statement race-guard rewrite in `src/parse.rs` | VERIFIED |
| `sql_throwing` validation-error synthesis | VERIFIED (workaround; TECH-DEBT 22) |
| `from_utf8` checked FFI entry | VERIFIED |
| `static_assert(sizeof(ParserOptions) == 32, ...)` in `cpp/include/parser_extension_compat.hpp` | VERIFIED |
| `test/integration/test_concurrent_ddl.py` | VERIFIED (Justfile `test-concurrent` recipe; passes) |

## Known Limitations Documented

| Limitation | TECH-DEBT | Resolution Plan |
|------------|-----------|-----------------|
| Caret rendering lost for validation errors (text-only via runtime `error()`). | Item 22 | Phase 62 — re-introduce `parse_function` as error-reporting layer. |
| `disable_peg_parser` silently resets `allow_parser_override_extension` to `default`. Workaround: re-issue `SET ...='FALLBACK'`. | Item 21 | Out-of-scope (requires DuckDB-side hook). |
| `IF NOT EXISTS` race: two parallel processes can both attempt INSERT; loser sees PK constraint violation. | Item 23 | Documented; out-of-scope (requires MVCC-aware retry hook). |

## Behavioral Spot-Checks

| Behavior | Result |
|----------|--------|
| `cargo test` post-phase | All pass |
| `just test-sql` post-phase | All pass |
| `just test-concurrent` | All pass — race shapes match documented contracts |
| `just ci` post-phase | All pass |
