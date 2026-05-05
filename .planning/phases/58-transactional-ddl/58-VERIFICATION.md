---
phase: 58-transactional-ddl
verified: 2026-05-02T16:30:00Z
status: passed
score: 4/4 must-haves verified (retroactive)
re_verification: false
note: Back-derived verification record — the original ad-hoc v0.8.0 work shipped on 2026-05-02 (commits 680a967 → 28b3291 on milestone/v0.8.0, PR #28). This document records the artefacts and tests that prove goal achievement.
---

# Phase 58: Transactional DDL Verification Report

**Phase Goal:** CREATE/DROP/ALTER SEMANTIC VIEW participate in caller's transaction via `parser_override`.

## Goal Achievement

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `BEGIN; CREATE SEMANTIC VIEW ...; ROLLBACK;` leaves catalog unchanged (committed state) | VERIFIED | `test/sql/v080_transactional_ddl.test` rollback-then-`list_semantic_views` cases; `test/integration/test_adbc_transactions.py::test_inline_rollback`. |
| 2 | `BEGIN; CREATE SEMANTIC VIEW ...; COMMIT;` persists across new connection | VERIFIED | Same files — commit-then-fresh-connection round-trip cases. |
| 3 | All four `CREATE` forms transactional (inline AS, inline FROM YAML, FROM YAML FILE, OR REPLACE / IF NOT EXISTS) | VERIFIED | `v080_transactional_ddl.test` covers each variant; `test_adbc_transactions.py` covers inline + FROM YAML FILE + ALTER + DROP. |
| 4 | Non-matching SQL falls through to default parser unchanged | VERIFIED | `parser_override` returns default-constructed `ParserOverrideResult` for non-matches; existing test suite (~700+ tests) unaffected by hook registration. |

**Score:** 4/4 truths verified

## Required Artifacts

| Artifact | Status | Notes |
|----------|--------|-------|
| `parser_override` hook registration in `cpp/src/shim.cpp` | VERIFIED | `sv_register_parser_hooks` calls `config.SetOption("allow_parser_override_extension", "FALLBACK")` |
| `sv_parser_override_rust` FFI entry in `src/parse.rs` | VERIFIED | Catches unwind, dispatches to `validate_and_rewrite`, returns rewritten SQL via heap buffer |
| `CatalogReader` replacing `CatalogState` HashMap | VERIFIED | Commit 7c9b4df removes the HashMap; `src/catalog.rs` provides direct-`_definitions` reads |
| `test/sql/v080_transactional_ddl.test` | VERIFIED | Listed in `test/sql/TEST_LIST`; passes |
| `test/integration/test_adbc_transactions.py` (just test-adbc) | VERIFIED | Justfile `test-adbc` recipe; passes against installed ADBC driver |

## Behavioral Spot-Checks

| Behavior | Result |
|----------|--------|
| `cargo test` post-phase | All Rust unit + property tests pass |
| `just test-sql` post-phase | All sqllogictest files pass, including `v080_transactional_ddl.test` |
| `just test-adbc` | Inline / FROM YAML FILE / ALTER / DROP rollback + commit cases all pass |
| Multi-DB isolation regression test | Two in-process DuckDB instances see independent catalogs |

## Known Limitations Documented

- `semantic_view(...)` queries do not see uncommitted writes to user tables in the same transaction — expansion runs on a separate `query_conn`. Workaround documented in CHANGELOG; revisited when DuckDB 2.0 PEG grammar-extension API ships.
- Same-transaction `SHOW SEMANTIC VIEWS` does not list an in-flight `CREATE` until commit (HashMap is gone). Workaround documented; tracked as TECH-DEBT item 19; addressed only by future scalar-UDF rewrite (out of v0.8.0 scope).

## Follow-Ups (handled in later phases)

- Pre-rewrite catalog snapshot is read on a separate connection → race window for DROP/ALTER → **Phase 60**.
- Per-DB token map is unbounded → **Phase 61** (bounded LRU).
- `DISPLAY_EXTENSION_ERROR` silently dropped under `FALLBACK_OVERRIDE` mode → exposed by **Phase 59**'s unification (which retired the `parse_function` fallback that previously caught these); workaround in **Phase 60**; structural fix in **Phase 62**.
