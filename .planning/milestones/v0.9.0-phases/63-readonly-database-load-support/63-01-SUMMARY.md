---
phase: 63
plan: 01
subsystem: extension load + catalog reader
tags: [readonly, load, catalog, ffi]
dependency-graph:
  requires:
    - "src/lib.rs::init_extension (Phase 62)"
    - "src/catalog.rs::CatalogReader (Phase 61, Copy semantics)"
  provides:
    - "is_read_only detection at LOAD via current_setting('access_mode') (lowercased)"
    - "init_catalog short-circuit on is_read_only=true"
    - "CatalogReader.catalog_table_present field + 3 reader-method short-circuits"
    - "5 unit tests pinning Phase 63 invariants"
  affects:
    - "src/parse.rs::sv_make_override_context (CatalogReader::new signature update — passes catalog_table_present=true)"
tech-stack:
  added: []
  patterns: ["case-insensitive enum match for setting strings", "fail-open on FFI query error", "null-pointer FFI-null testing for short-circuit verification"]
key-files:
  created:
    - ".planning/phases/63-readonly-database-load-support/63-01-SUMMARY.md"
    - ".planning/phases/63-readonly-database-load-support/deferred-items.md"
  modified:
    - "src/lib.rs (init_extension lines 339-410, tests module +43 lines)"
    - "src/catalog.rs (init_catalog signature + early-return, CatalogReader new field + 3 short-circuits, tests gating + 5 new tests)"
    - "src/parse.rs (sv_make_override_context CatalogReader::new call site, OverrideContext drop test call site)"
    - "src/ddl/describe.rs (drop stale base_table field reference in window_spec_property_row_emitted test)"
decisions:
  - "Pass `catalog_table_present=true` from sv_make_override_context — keeps src/parse.rs UNCHANGED in spirit and routes DDL writes through DuckDB's native read-only error path (RO-05). Fresh-read-only-DB DDL pre-checks may surface catalog errors instead, covered by RO-05 'or the closest equivalent' wording per RESEARCH §3 Q5."
  - "Gate pre-existing in-memory `Connection::open*` tests `#[cfg(not(feature = \"extension\"))]` (Rule 3 — blocking). Mirrors lib.rs::test_helpers gate pattern."
  - "Fix stale `SemanticViewDefinition::base_table` field reference inline in describe.rs::window_spec_property_row_emitted (Rule 3 — blocking). Field was removed in cbacbed prior to Phase 63."
metrics:
  duration: "~25 min"
  completed: 2026-05-15
---

# Phase 63 Plan 01: Read-Only LOAD Core Summary

Read-only DuckDB host DBs are now first-class for `LOAD semantic_views`. The extension probes `current_setting('access_mode')` at load (lowercased per duckdb.cpp:301163-301167; case-insensitive match for future-proofing), short-circuits `init_catalog` to skip the schema/table CREATE plus the v0.1.0 companion-file migration when read-only, and threads a second probe of `information_schema.tables` into `CatalogReader` so reader paths return `Ok(None)` / empty `Vec` cleanly when `semantic_layer._definitions` is absent. DDL on read-only DBs continues to fail naturally with DuckDB's standard read-only error via the parser_override → caller-connection write path (no extension-side wrapping).

## What Shipped

**Files modified (4):**

- `src/lib.rs` — `init_extension`:
  - Lines 358-372: new `is_read_only` detection block via `current_setting('access_mode')` with `eq_ignore_ascii_case("read_only")` and `unwrap_or(false)` fail-open.
  - Line 375: `init_catalog(con, &db_path, is_read_only)?;`
  - Lines 388-401: new `catalog_table_present` probe (only when `is_read_only=true`), then `CatalogReader::new(catalog_conn, catalog_table_present)`.
  - Lines 631-688: two new unit tests in the `tests` module pinning the access-mode contract.

- `src/catalog.rs`:
  - Lines 25-39: `init_catalog` signature gains `is_read_only: bool`; early-returns `Ok(())` when true.
  - Lines 96-105: `CatalogReader` gains `catalog_table_present: bool` field (kept `Clone, Copy`).
  - Lines 108-117: `CatalogReader::new(conn, catalog_table_present)` constructor.
  - Lines 132-180: `lookup`/`list_all`/`list_names` short-circuit on `!self.catalog_table_present` BEFORE the unsafe FFI call.
  - Lines 419-423, 555-557: existing test call sites updated to pass `false`.
  - Lines 532-617: 5 new unit tests (2 bundled-only `init_catalog_*`, 3 extension-only `lookup`/`list_all`/`list_names` short-circuit tests).
  - Lines 351-486: pre-existing in-memory tests gated `#[cfg(not(feature = "extension"))]` to preserve the `--features extension --no-default-features` build (Rule 3 — blocking; mirrors lib.rs::test_helpers gate pattern).

- `src/parse.rs`:
  - Lines 2425-2447: `sv_make_override_context` passes `catalog_table_present=true` to `CatalogReader::new`. Documented why (RO-05 + RESEARCH §3 Q5).
  - Line 2886: drop test (`override_context_drop_does_not_disconnect`) updated to pass `true`.

- `src/ddl/describe.rs`:
  - Line 630: drop stale `base_table: "orders".to_string(),` (field removed in cbacbed). Pre-existing breakage that prevented the extension-feature build from compiling — fixed inline as Rule 3 blocker.

## New Tests (5 total)

| Test | Module | Feature gate | What it pins |
|------|--------|--------------|--------------|
| `access_mode_lowercased_on_readonly_open` | `lib::tests` | `not(feature = "extension")` | DuckDB returns lowercased `"read_only"` for read-only DBs (catches future rendering changes at CI bump time) |
| `access_mode_writable_returns_automatic_or_read_write` | `lib::tests` | `not(feature = "extension")` | In-memory DBs do NOT match `read_only` (sibling negative case) |
| `init_catalog_skips_writes_on_readonly` | `catalog::tests` | `not(feature = "extension")` | `is_read_only=true` does NOT create the `semantic_layer` schema (verified via `information_schema.schemata`) |
| `init_catalog_writes_when_writable` | `catalog::tests` | `not(feature = "extension")` | Writable path still creates the `_definitions` table (regression guard) |
| `lookup_returns_none_when_table_missing` | `catalog::tests` | `feature = "extension"` | `catalog_table_present=false` returns `Ok(None)` BEFORE the unsafe FFI call (uses `null_mut()` conn — segfault if short-circuit reordered) |
| `list_all_returns_empty_when_table_missing` | `catalog::tests` | `feature = "extension"` | Same null-conn trick for `list_all` |
| `list_names_returns_empty_when_table_missing` | `catalog::tests` | `feature = "extension"` | Same null-conn trick for `list_names` |

(Listed as "5 new" per the plan's Tasks 1+2 success criteria; 7 distinct test functions total when counting both access-mode tests separately. The plan's Task 1 specified two access-mode tests + Task 2 specified five tests covering init_catalog + 3 reader-paths.)

## Verification (per plan §verification)

| Step | Command | Result |
|------|---------|--------|
| 1 | `cargo test --lib` | **758 passed**, 0 failed |
| 2 | `cargo test --lib --features extension --no-default-features` | **764 passed**, 0 failed |
| 3 | `just build` | extension binary produced at `build/debug/semantic_views.duckdb_extension` |
| 4 | `cargo clippy --all-targets -- -D warnings` | **89 pre-existing errors**; none introduced by Phase 63 (see Deferred Issues) |
| 5 | `cargo fmt --check` | clean |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] Pre-existing `base_table` field reference in `src/ddl/describe.rs:630`**

- **Found during:** Task 2 verification (`cargo test --lib --features extension --no-default-features`)
- **Issue:** `SemanticViewDefinition::base_table` was removed in commit `cbacbed` ("remove vestigial filters field from SemanticViewDefinition"); the `window_spec_property_row_emitted` test still referenced it, breaking the `--features extension --no-default-features` build entirely. Verified pre-existing by stashing all Phase 63 work and re-running — same compile error.
- **Fix:** One-line removal of the stale field assignment.
- **Files modified:** `src/ddl/describe.rs:630`
- **Commit:** `c9e6f6b` (folded into Tasks 1+2 commit since they could not be verified without it)

**2. [Rule 3 — Blocking] Pre-existing in-memory tests panic under `extension` feature**

- **Found during:** Task 2 verification (after fixing #1 above)
- **Issue:** `catalog::tests::{two_statement_guard_then_dml_smoke, init_catalog_creates_schema_and_table, pragma_database_list_*, persist_02_rollback_leaves_catalog_unchanged}` use `Connection::open*` which panics under the `extension` feature with "DuckDB API not initialized or DuckDB feature omitted". Verified pre-existing.
- **Fix:** Add `#[cfg(not(feature = "extension"))]` to each affected test (and to `in_memory_con()` helper). Mirrors the existing `src/lib.rs:23` `test_helpers` gate pattern.
- **Files modified:** `src/catalog.rs` (multiple test attribute additions), `src/lib.rs` (gates on the two new access-mode tests for the same reason)
- **Commit:** `c9e6f6b`

**3. [Rule 3 — Architectural drop-back] `src/parse.rs::sv_make_override_context` passes `catalog_table_present=true`**

- **Issue:** Plan said "src/parse.rs is UNCHANGED" but the new `CatalogReader::new` signature requires the second arg. The only sites are `sv_make_override_context` (LOAD path) and one drop test.
- **Fix:** Pass `true` at both sites — keeps current behaviour of catalog pre-checks during DDL rewrites. RESEARCH §3 Q5 covers the implication: a fresh read-only DB DDL pre-check may surface a catalog error instead of the read-only error, which is acceptable per RO-05's "or the closest equivalent" wording. The bootstrapped read-only DB case (the one the ROADMAP explicitly cares about) still gets DuckDB's verbatim read-only error from the rewritten DML on the caller's connection.
- **Files modified:** `src/parse.rs:2425-2447, 2886`
- **Commit:** `c9e6f6b`

### Deferred Issues (out of scope per scope-boundary rule)

- **Pre-existing clippy backlog (89 errors):** Documented in `.planning/phases/63-readonly-database-load-support/deferred-items.md`. None introduced by Phase 63. Recommend separate quick-task to either fix or relax the gate.

## Authentication Gates

None — no auth steps required for this plan.

## Branch + Hand-off

- **Branch:** `milestone/v0.9.0` (verified before commit)
- **Commits:**
  - `c9e6f6b` — `feat(63-01): read-only LOAD support core (lib.rs + catalog.rs)`
- **Hand-off:** Plan 02 (sqllogictest + Python integration test for read-only paths) and Plan 03 (docs + example) can now build against the working extension binary at `build/debug/semantic_views.duckdb_extension`.

## Self-Check: PASSED

Verified files exist:
- FOUND: `src/lib.rs` (modified)
- FOUND: `src/catalog.rs` (modified)
- FOUND: `src/parse.rs` (modified)
- FOUND: `src/ddl/describe.rs` (modified)
- FOUND: `build/debug/semantic_views.duckdb_extension` (built)
- FOUND: `.planning/phases/63-readonly-database-load-support/63-01-SUMMARY.md` (this file)
- FOUND: `.planning/phases/63-readonly-database-load-support/deferred-items.md`

Verified commit:
- FOUND: `c9e6f6b` — `feat(63-01): read-only LOAD support core (lib.rs + catalog.rs)`
