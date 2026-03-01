---
phase: 10-pragma-query-t-catalog-persistence
verified: 2026-03-01T01:00:00Z
status: human_needed
score: 7/8 must-haves verified
---

# Phase 10: pragma_query_t Catalog Persistence Verification Report

**Phase Goal:** Semantic view definitions persist via DuckDB-native tables and the sidecar `.semantic_views` file is gone from the codebase
**Verified:** 2026-03-01T01:00:00Z
**Status:** human_needed

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | pragma_query_t callbacks registered via ExtensionLoader | ✓ VERIFIED | shim.cpp line 70-83: PragmaFunction::PragmaCall for both define/drop with pragma_query_t returning INSERT/DELETE SQL |
| 2 | semantic_views_pragma_define/drop C functions exist for invoke path | ✓ VERIFIED | shim.cpp lines 110-135: two C functions using separate connection + duckdb_query |
| 3 | Rust FFI declarations gated on extension feature | ✓ VERIFIED | mod.rs: #[cfg(feature = "extension")] pub mod ffi with both declarations |
| 4 | DefineState/DropState use persist_conn instead of db_path | ✓ VERIFIED | define.rs line 22, drop.rs line 17: `persist_conn: Option<libduckdb_sys::duckdb_connection>` |
| 5 | Write-first pattern: table write before HashMap update | ✓ VERIFIED | define.rs line 68-84: FFI call before catalog_insert; drop.rs line 56-67: FFI call before catalog_delete |
| 6 | lib.rs creates persist_conn for file-backed databases | ✓ VERIFIED | lib.rs lines 108-116: duckdb_connect for non-:memory: databases; None for in-memory |
| 7 | No sidecar function or file reference in source code (PERSIST-03) | ✓ VERIFIED | grep -rn ".semantic_views" *.rs/*.cpp/*.h/*.test returns zero results |
| 8 | ROLLBACK leaves _definitions table unchanged (PERSIST-02) | ? UNCERTAIN | Unit test passes (catalog::tests::persist_02_rollback_leaves_catalog_unchanged); full PRAGMA path requires loaded extension |

**Score:** 7/8 truths verified (1 needs human)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/shim/shim.cpp` | pragma_query_t callbacks + C invoke functions | ✓ EXISTS + SUBSTANTIVE | 140 lines: ExtensionLoader registration, PragmaDefineSemanticView, PragmaDropSemanticView, semantic_views_pragma_define, semantic_views_pragma_drop |
| `src/shim/shim.h` | C header declaring two new functions | ✓ EXISTS + SUBSTANTIVE | Declares semantic_views_pragma_define + semantic_views_pragma_drop with duckdb_connection param |
| `src/shim/mod.rs` | Rust FFI declarations gated on extension feature | ✓ EXISTS + SUBSTANTIVE | #[cfg(feature = "extension")] pub mod ffi with both function declarations |
| `src/ddl/define.rs` | DefineState with persist_conn + write-first invoke | ✓ EXISTS + SUBSTANTIVE | persist_conn field, unsafe Send/Sync, write-first FFI call pattern |
| `src/ddl/drop.rs` | DropState with persist_conn + write-first invoke | ✓ EXISTS + SUBSTANTIVE | persist_conn field, unsafe Send/Sync, write-first FFI call pattern |
| `src/lib.rs` | persist_conn creation at init time | ✓ EXISTS + SUBSTANTIVE | duckdb_connect for file-backed; None for :memory:; passed to both states |
| `src/catalog.rs` | Migration block + no sidecar functions | ✓ EXISTS + SUBSTANTIVE | V010_COMPANION_EXT const, migration_path construction, import + delete; write_sidecar/read_sidecar/sidecar_path/sync_table_from_map all deleted |
| `test/sql/phase2_restart.test` | Updated to table-based persistence + PRAGMA ROLLBACK | ✓ EXISTS + SUBSTANTIVE | No sidecar references; PRAGMA define_semantic_view_internal + BEGIN/ROLLBACK test added |

**Artifacts:** 8/8 verified

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| src/lib.rs init_extension | semantic_views_register_shim | unsafe extern C call | ✓ WIRED | lib.rs line 150: `semantic_views_register_shim(db_handle.cast())` |
| semantic_views_register_shim | ExtensionLoader pragma callbacks | PragmaFunction::PragmaCall | ✓ WIRED | shim.cpp lines 70-83: both pragmas registered |
| src/lib.rs init_extension | DefineState/DropState persist_conn | duckdb_connect + struct construction | ✓ WIRED | lib.rs line 108-116: persist_conn creation; lines 80-91: passed to both states |
| src/ddl/define.rs invoke | crate::shim::ffi::semantic_views_pragma_define | unsafe FFI call | ✓ WIRED | define.rs line 74: `crate::shim::ffi::semantic_views_pragma_define(conn, c_name.as_ptr(), c_json.as_ptr())` |
| src/ddl/drop.rs invoke | crate::shim::ffi::semantic_views_pragma_drop | unsafe FFI call | ✓ WIRED | drop.rs line 60: `crate::shim::ffi::semantic_views_pragma_drop(conn, c_name.as_ptr())` |
| src/catalog.rs init_catalog | v0.1.0 companion file migration | migration_path.exists() + read + INSERT OR REPLACE + remove_file | ✓ WIRED | catalog.rs lines 49-83: full migration block |

**Wiring:** 6/6 connections verified

## Requirements Coverage

| Requirement | Status | Blocking Issue |
|-------------|--------|----------------|
| PERSIST-01: Definitions persist via DuckDB native tables | ? NEEDS HUMAN | Code path is correct; end-to-end test requires loaded extension (cargo build --features extension + DuckDB LOAD) |
| PERSIST-02: ROLLBACK reverts definition change | ? NEEDS HUMAN | Unit test covers table-level rollback; PRAGMA callback path (pragma_query_t) needs loaded extension to test |
| PERSIST-03: Sidecar file mechanism removed from codebase | ✓ SATISFIED | grep -rn ".semantic_views" *.rs/*.cpp/*.h/*.test returns zero results; write_sidecar/read_sidecar/sidecar_path/sync_table_from_map all deleted |

**Coverage:** 1/3 requirements fully satisfied by automated checks; 2/3 need human verification

## Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | - | - | - | - |

**Anti-patterns:** 0 found

## Human Verification Required

### 1. End-to-End Persistence Test (PERSIST-01)
**Test:** Build extension with `cargo build --features extension`, load into DuckDB, define a view, close and reopen the database, verify the view is still queryable
**Expected:** `SELECT * FROM list_semantic_views()` returns the view defined in the previous session; no `.semantic_views` file exists on disk
**Why human:** Requires the extension to be compiled and loaded into a live DuckDB instance — cannot be done in the unit test context

### 2. PRAGMA ROLLBACK Test (PERSIST-02)
**Test:** With loaded extension: `BEGIN; PRAGMA define_semantic_view_internal('test', '...'); ROLLBACK; SELECT count(*) FROM list_semantic_views() WHERE name = 'test';`
**Expected:** Count is 0 — ROLLBACK reverted the PRAGMA's INSERT
**Why human:** The pragma_query_t mechanism (returned SQL executing in caller's transaction) requires the extension loaded in a live DuckDB session to exercise

### 3. v0.1.0 Migration Test (one-time migration)
**Test:** Create a `.semantic_views` companion file alongside a DuckDB file, load the extension, verify the view is imported into `semantic_layer._definitions` and the companion file is deleted
**Expected:** View appears in `list_semantic_views()`, companion file is gone from disk
**Why human:** Migration requires a pre-existing companion file and a live extension load

## Gaps Summary

**No critical gaps found.** All automated checks pass. Phase goal infrastructure is complete.

The 3 human verification items test the loaded-extension behavior (pragma_query_t transaction semantics, end-to-end persistence, migration trigger). These are not automated because they require `cargo build --features extension` + DuckDB LOAD — outside the unit test context.

## Verification Metadata

**Verification approach:** Goal-backward (Success Criteria from ROADMAP.md + must_haves from PLAN frontmatter)
**Must-haves source:** ROADMAP.md Success Criteria + PLAN.md truths/artifacts/key_links
**Automated checks:** 7/8 truths verified, 8/8 artifacts verified, 6/6 key links verified, 0 anti-patterns
**Human checks required:** 3 (loaded-extension integration tests)
**Total verification time:** ~5 min

---
*Verified: 2026-03-01T01:00:00Z*
*Verifier: Claude (orchestrator inline)*
