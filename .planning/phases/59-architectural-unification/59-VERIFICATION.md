---
phase: 59-architectural-unification
verified: 2026-05-02T23:38:00Z
status: passed-with-known-regression
score: 2/2 must-haves verified; 1 known regression documented (TECH-DEBT item 22)
re_verification: false
note: Back-derived. Source commit 1e7e92b shipped on 2026-05-02; the known regression (caret rendering loss for validation errors) was discovered during Phase 60 work and is documented in CHANGELOG + TECH-DEBT.
---

# Phase 59: Architectural Unification Verification Report

**Phase Goal:** `parser_override` is the sole DDL entry point; legacy `parse_function` / `sv_ddl_internal` retired.

## Goal Achievement

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | All recognised DDL flows through one rewrite path (`validate_and_rewrite`) | VERIFIED | `src/parse.rs` central dispatch; `cpp/src/shim.cpp` no longer registers `parse_function`; `src/ddl/{alter,drop,persist}.rs` files deleted. |
| 2 | All DDL forms work under both Bison and PEG parsers without per-form carve-out | VERIFIED | `test/sql/peg_compat.test` exercises CREATE/DROP/ALTER/DESCRIBE/SHOW under PEG and passes. The Bison-only `DESCRIBE` skip from pre-v0.8.1 is removed. |

**Score:** 2/2 truths verified.

## Net Code Impact

- Net diff: +513 / −1614 lines (≈1500 LOC removed).
- Files deleted: `src/ddl/alter.rs`, `src/ddl/drop.rs`, `src/ddl/persist.rs`.
- Files significantly slimmed: `cpp/src/shim.cpp`, `src/lib.rs`, `src/ddl/define.rs`.

## Known Regression (Documented)

| Regression | Tracked | Fix |
|------------|---------|-----|
| Caret rendering (`LINE 1: … ^`) lost for validation errors. Side effect of DuckDB silently dropping `DISPLAY_EXTENSION_ERROR` from `parser_override` under `FALLBACK_OVERRIDE` mode. | TECH-DEBT item 22; CHANGELOG Known Limitations | Workaround in Phase 60 (synthesised `SELECT error('<msg>')`); structural fix slated for Phase 62 (re-introduce `parse_function` as error-reporting layer). |

## Behavioral Spot-Checks

| Behavior | Result |
|----------|--------|
| `cargo test` post-phase | All Rust tests pass |
| `just test-sql` post-phase | All sqllogictest files pass including updated `peg_compat.test` |
| `just ci` post-phase | lint + test-all + check-fuzz + docs-check pass |

## Follow-Ups

- **Phase 60** ships the validation-error workaround so users at least see message text (caret rendering still missing).
- **Phase 62** restores caret rendering structurally.
