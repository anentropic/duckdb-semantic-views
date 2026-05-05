---
phase: 61-bounded-isolation-quality-docs
verified: 2026-05-03T19:48:00Z
status: passed
score: 5/5 must-haves verified
re_verification: false
note: Back-derived. The bounded LRU in this phase is intentionally a known limitation (TECH-DEBT 20), not a structural fix — Phase 62 redesigns it via `SemanticViewsParserInfo` lifetime ownership.
---

# Phase 61: Bounded Multi-DB Isolation, RAII, Tests & Docs Verification Report

## Goal Achievement

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Per-DB token→catalog map capped at 16 entries with insertion-order eviction; long-lived processes don't leak | VERIFIED | `src/parse.rs` LRU implementation; unit test in `src/parse.rs::tests` (later removed by Phase 62 redesign). |
| 2 | Evicted-token reuse surfaces the actionable error `catalog context for this database has been evicted` | VERIFIED | Error path in `rewrite_*` functions checks `LRU_MISS` and emits the documented message. |
| 3 | `CatalogReader` uses RAII guards; no manual `duckdb_destroy_*` along error paths | VERIFIED | `PreparedStmt` and `QueryResult` Drop impls in `src/catalog.rs`; `prepared_lookup` and `execute_list_all` simplified to early-return without leaks. |
| 4 | FFI buffer freed along every return path from C++ `sv_parser_override` | VERIFIED | Commit c27a170; covered by `fuzz_parser_override_ffi` (no leaks under sustained random input). |
| 5 | Documentation refreshed: CHANGELOG, TECH-DEBT, MAINTAINER, Sphinx reference pages | VERIFIED | Files modified per PLAN.md; `just docs-check` passes. |

**Score:** 5/5 truths verified.

## Required Artifacts

| Artifact | Status |
|----------|--------|
| Bounded LRU in `src/parse.rs` (16-entry cap, insertion-order eviction) | VERIFIED |
| `PreparedStmt` / `QueryResult` RAII guards in `src/catalog.rs` | VERIFIED |
| `fuzz/fuzz_targets/fuzz_parser_override_ffi.rs` | VERIFIED — `cargo +nightly fuzz check` passes |
| `test/integration/test_concurrent_ddl.py` (just test-concurrent) | VERIFIED |
| `test/integration/test_type_inference.py` BEGIN/COMMIT cases | VERIFIED |
| `test/sql/v080_transactional_ddl.test` D-series additions | VERIFIED |
| `examples/race_guards_and_unification.py` runs end-to-end | VERIFIED |
| Sphinx reference page refresh + new `transactional-ddl-and-limitations.rst` | VERIFIED — `just docs-check` passes |

## Behavioral Spot-Checks

| Behavior | Result |
|----------|--------|
| `cargo test` post-phase | All pass |
| `just test-all` post-phase | All pass (Rust + sqllogictest + DuckLake CI) |
| `just test-adbc` | All pass |
| `just test-concurrent` | All pass |
| `just ci` post-phase | All pass (lint + test-all + check-fuzz + docs-check) |
| Multi-DB regression (16+ DuckDB instances in one process) | Memory bounded; eviction error surfaces as documented |

## Known Limitations Documented

| Limitation | TECH-DEBT | Resolution Plan |
|------------|-----------|-----------------|
| Bounded LRU evictions are silent at allocation time; surfaced only on next CREATE through evicted token. | Item 20 | Phase 62 — `OverrideContext` attached to `SemanticViewsParserInfo` (lifetime tied to `DBConfig`), eliminating LRU entirely. |
| All Phase 60 limitations carried forward (caret rendering, `disable_peg_parser`, IF NOT EXISTS race). | Items 21, 22, 23 | Items 22 and 20 → Phase 62. Items 21 and 23 → out-of-scope (require DuckDB-side hooks). |

## Milestone-Close Status

This is the closing phase of the v0.8.0 work that originally landed on `milestone/v0.8.1` before the 2026-05-05 consolidation. After Phase 61 verification, the milestone awaited tagging — but Phase 62 (caret restoration) was identified as in-scope for v0.8.0 instead of v0.8.1, deferring the tag. Cargo.toml + description.yml were rolled back to 0.8.0 during the 2026-05-05 reorganisation.
