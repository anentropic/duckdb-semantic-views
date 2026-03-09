---
phase: 20-extended-ddl-statements
verified: 2026-03-09T13:00:00Z
status: passed
score: 6/6 must-haves verified
re_verification: null
gaps: []
human_verification: []
---

# Phase 20: Extended DDL Statements Verification Report

**Phase Goal:** Users can manage semantic views entirely through native DDL syntax -- create, replace, drop, inspect, and list
**Verified:** 2026-03-09
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths (from ROADMAP.md Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | User can run `DROP SEMANTIC VIEW name` and the view is removed from the catalog | VERIFIED | `phase20_extended_ddl.test` lines 97-103: drops `sv_replace_test` then confirms error on `describe_semantic_view('sv_replace_test')` with "does not exist" |
| 2 | User can run `DROP SEMANTIC VIEW IF EXISTS name` without error when the view does not exist | VERIFIED | `phase20_extended_ddl.test` lines 119-122: `DROP SEMANTIC VIEW IF EXISTS sv_nonexistent` is `statement ok` |
| 3 | User can run `CREATE OR REPLACE SEMANTIC VIEW name (...)` and the existing view is updated in place | VERIFIED | `phase20_extended_ddl.test` lines 46-63: creates, replaces with new metric (`avg_amount`), queries new metric, confirms only 1 catalog entry |
| 4 | User can run `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` and no error occurs when the view already exists | VERIFIED | `phase20_extended_ddl.test` lines 79-89: creates, creates again with different body, both are `statement ok`; only 1 catalog entry remains |
| 5 | User can run `DESCRIBE SEMANTIC VIEW name` and see dimensions, metrics, and types | VERIFIED | `phase20_extended_ddl.test` lines 184-193: returns 6-column row with `name, base_table, dimensions_json, metrics_json, filters, joins`; exact JSON content asserted |
| 6 | User can run `SHOW SEMANTIC VIEWS` and see all defined semantic views | VERIFIED | `phase20_extended_ddl.test` lines 229-241: two views created, both returned in `rowsort` query; repeated with lowercase `show semantic views` |

**Score:** 6/6 truths verified

### Required Artifacts

#### Plan 20-01 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/parse.rs` | DdlKind enum, multi-prefix detection, multi-form rewrite, DDL-aware name extraction | VERIFIED | `DdlKind` enum with 7 variants at line 20; `detect_semantic_view_ddl` at line 75; `detect_ddl_kind` at line 42; `rewrite_ddl` at line 208; `extract_ddl_name` at line 251 -- all substantive, 957 lines total including full test suite |
| `cpp/src/shim.cpp` | Generic error message for all DDL forms | VERIFIED | Line 125: `throw BinderException("Semantic view DDL failed: %s", error_buf)` -- covers all DDL forms |
| `test/sql/phase20_extended_ddl.test` | Integration tests for DROP, DROP IF EXISTS, CREATE OR REPLACE, CREATE IF NOT EXISTS | VERIFIED | 312-line test file with DDL-03 through DDL-06 plus case-insensitivity and backward-compatibility tests |

#### Plan 20-02 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `cpp/src/shim.cpp` | Result-forwarding sv_ddl_bind with dynamic column schemas | VERIFIED | `SvDdlBindData` stores `vector<vector<string>> rows` and `vector<string> col_names`; `sv_ddl_bind` calls `sv_rewrite_ddl_rust` then `duckdb_query`, reads column metadata dynamically at lines 138-173 |
| `src/parse.rs` | New FFI function sv_rewrite_ddl_rust returning rewritten SQL without executing | VERIFIED | `sv_rewrite_ddl_rust` at lines 334-371, `#[cfg(feature = "extension")]`, returns rewritten SQL via `sql_out` buffer, calls `rewrite_ddl(query)` internally |
| `test/sql/phase20_extended_ddl.test` | Integration tests for DESCRIBE SEMANTIC VIEW and SHOW SEMANTIC VIEWS | VERIFIED | Lines 171-311 cover DDL-07 and DDL-08 with full column assertions, case-insensitivity, error cases, and a complete lifecycle test (create -> describe -> show -> replace -> describe -> drop -> verify gone) |

### Key Link Verification

#### Plan 20-01 Key Links

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/parse.rs` | `cpp/src/shim.cpp` | `sv_parse_rust` FFI: `sv_parse_rust` calls `detect_semantic_view_ddl` | WIRED | `shim.cpp` line 63: `sv_parse_rust(query.c_str(), query.size())`; `parse.rs` line 288: `pub extern "C" fn sv_parse_rust` calling `detect_semantic_view_ddl` |
| `src/parse.rs` | `src/parse.rs` | `rewrite_ddl` calls `detect_ddl_kind` internally | WIRED | `parse.rs` line 213: `detect_ddl_kind(trimmed).ok_or_else(...)` inside `rewrite_ddl` |

#### Plan 20-02 Key Links

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `cpp/src/shim.cpp` | `src/parse.rs` | `sv_rewrite_ddl_rust` FFI: C++ calls Rust to get rewritten SQL, C++ executes it | WIRED | `shim.cpp` line 119: `sv_rewrite_ddl_rust(query.c_str(), query.size(), sql_buf, sizeof(sql_buf), error_buf, sizeof(error_buf))`; `parse.rs` lines 334-371: `pub extern "C" fn sv_rewrite_ddl_rust` |
| `cpp/src/shim.cpp` | DuckDB C API | `duckdb_query(sv_ddl_conn, sql_buf, &result)` executes rewritten SQL and captures result | WIRED | `shim.cpp` line 130: `if (duckdb_query(sv_ddl_conn, sql_buf, &result) != DuckDBSuccess)` followed by column count/name/data read at lines 138-173 |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| DDL-03 | 20-01 | User can drop a semantic view with `DROP SEMANTIC VIEW name` | SATISFIED | Integration test: `statement ok DROP SEMANTIC VIEW sv_replace_test`, confirmed removed by `statement error` on describe |
| DDL-04 | 20-01 | User can drop a semantic view idempotently with `DROP SEMANTIC VIEW IF EXISTS name` | SATISFIED | Integration test: `statement ok DROP SEMANTIC VIEW IF EXISTS sv_nonexistent` succeeds silently |
| DDL-05 | 20-01 | User can replace a semantic view with `CREATE OR REPLACE SEMANTIC VIEW name (...)` | SATISFIED | Integration test: creates, replaces with different metric, queries new metric successfully, confirms count=1 |
| DDL-06 | 20-01 | User can create a semantic view idempotently with `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | SATISFIED | Integration test: two identical `statement ok` creation attempts, count=1 |
| DDL-07 | 20-02 | User can inspect a semantic view with `DESCRIBE SEMANTIC VIEW name` showing dimensions, metrics, and types | SATISFIED | Integration test: `query TTTTTT DESCRIBE SEMANTIC VIEW desc_test` returns exact 6-column row with name, base_table, dimensions JSON, metrics JSON, filters, joins |
| DDL-08 | 20-02 | User can list all semantic views with `SHOW SEMANTIC VIEWS` | SATISFIED | Integration test: `query TT rowsort SHOW SEMANTIC VIEWS` returns both created views' names and base_tables |

All 6 requirements in the phase scope (DDL-03 through DDL-08) are fully satisfied.

**Requirement traceability check:** REQUIREMENTS.md shows DDL-03 through DDL-08 mapped to Phase 20 with status "Complete". No orphaned or unclaimed requirements for this phase.

### Anti-Patterns Found

No anti-patterns detected in modified files:

- `src/parse.rs`: No TODO/FIXME/placeholder comments; no stub implementations (957 lines of substantive logic and tests)
- `cpp/src/shim.cpp`: No TODO/FIXME; implementations are substantive (result forwarding, row emission with offset tracking)
- `test/sql/phase20_extended_ddl.test`: No TODO/FIXME; all test blocks have concrete assertions

### Quality Gate

All three tiers of the `just test-all` quality gate passed:

| Test Suite | Command | Result |
|------------|---------|--------|
| Rust unit + proptest + doc tests | `cargo test` | 186 passed, 0 failed (138 + 6 + 36 + 5 + 1 across test suites) |
| SQL logic tests | `just test-sql` | 6/6 files passed (including phase20_extended_ddl.test) |
| DuckLake CI integration | `just test-ducklake-ci` | 6/6 passed |

### Human Verification Required

None. All success criteria are mechanically verifiable via the test suite, which was executed and passed.

The lifecycle test in the integration file (`phase20_extended_ddl.test` lines 254-311) covers the full user journey end-to-end: create -> describe -> show -> replace -> describe updated state -> drop -> verify gone from describe and show. This substitutes for manual user testing.

### Gaps Summary

No gaps. All 6 observable truths from ROADMAP.md success criteria are verified. All 6 required artifacts exist and are substantive (not stubs) and wired together. All 6 requirements (DDL-03 through DDL-08) are satisfied. The full quality gate (`just test-all`) passes with zero failures.

**Notable engineering details confirmed in code:**

1. **Longest-first prefix ordering** in `detect_ddl_kind`: "create or replace semantic view" (31 bytes) and "create semantic view if not exists" (34 bytes) are checked before "create semantic view" (20 bytes) to prevent prefix shadowing.

2. **Statement cache disabled** (`SupportStatementCache() -> false` in `SvDdlBindData`): required because the sv_ddl_internal table function returns different column counts per DDL form (1 for CREATE/DROP, 6 for DESCRIBE, 2 for SHOW).

3. **sqllogictest runner patched** (`scripts/patch_sqllogictest.py`): DuckDB reports parser extension results as `StatementType.EXTENSION` which the stock runner misclassified as CHANGED_ROWS; the patch ensures multi-column results from parser extension DDL are treated as query results.

4. **`sv_execute_ddl_rust` retained** for backward compatibility but no longer called from C++; the new pipeline uses `sv_rewrite_ddl_rust` (rewrite only) + `duckdb_query` (C++ executes) to enable full result forwarding.

---

_Verified: 2026-03-09T13:00:00Z_
_Verifier: Claude (gsd-verifier)_
