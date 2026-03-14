---
phase: 25-sql-body-parser
verified: 2026-03-12T00:00:00Z
status: passed
score: 13/13 must-haves verified
gaps: []
human_verification:
  - test: "Run 'CREATE SEMANTIC VIEW foo AS TABLSE (...) ...' in DuckDB CLI; verify caret points to 'TABLSE'"
    expected: "Error with caret at the 'TABLSE' token, message mentioning 'did you mean TABLES'"
    why_human: "Visual inspection of terminal caret output required; automated caret position verified via Python integration test"
    status: approved_2026-03-12
---

# Phase 25: SQL Body Parser Verification Report

**Phase Goal:** Users can write `CREATE SEMANTIC VIEW` with SQL keyword clauses instead of function-call syntax
**Verified:** 2026-03-12
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `CREATE SEMANTIC VIEW name AS TABLES (...) DIMENSIONS (...) METRICS (...)` is accepted and stored | VERIFIED | `phase25_keyword_body.test` passes [8/8] via `just test-sql`; sqllogictest queries return correct data |
| 2 | TABLES clause parses `alias AS physical_table PRIMARY KEY (col1, col2)` into `TableRef` | VERIFIED | `parse_tables_clause` — 5 unit tests green including schema-qualified, composite PK, error cases |
| 3 | RELATIONSHIPS clause parses `name AS from_alias(fk_cols) REFERENCES to_alias` into `Join` | VERIFIED | `parse_relationships_clause` — 4 unit tests green including empty body, composite FK, error cases |
| 4 | DIMENSIONS clause parses `alias.name AS sql_expr` into `Dimension` | VERIFIED | `parse_qualified_entries` — 5 unit tests green including nested parens, trailing commas |
| 5 | METRICS clause parses `alias.metric_name AS agg_expr` with the same parser | VERIFIED | Same `parse_qualified_entries` function used for both DIMENSIONS and METRICS; sqllogictest confirms |
| 6 | All 7 DDL verbs work with new AS keyword body syntax | VERIFIED | `phase25_keyword_body.test`: CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW all pass |
| 7 | Clause typo (TABLSE) produces positioned error pointing at the typo | VERIFIED | `as_body_position_invariant_clause_typo` proptest: 36 proptest passes; Python caret test passes |
| 8 | Rewritten SQL routes through `create_semantic_view_from_json` VTab | VERIFIED | `as_body_validate_and_rewrite_succeeds` proptest confirms `SELECT * FROM create_semantic_view_from_json(` prefix; `DefineFromJsonVTab` registered in `lib.rs` under 3 function names |
| 9 | C++ DDL buffer can hold 64 KB rewritten SQL without truncation | VERIFIED | `shim.cpp` line 138: `std::string sql_str(65536, '\0')` — no `char sql_buf[4096]` remains |
| 10 | Old paren-body DDL path still works alongside new AS-body path | VERIFIED | `phase25_parse_tests::old_paren_body_still_works` passes; all 7 prior sqllogictest files [1-7/8] still pass |
| 11 | Trailing commas in clause entry lists tolerated | VERIFIED | `split_trailing_comma_discarded` and `parse_qualified_entries_trailing_comma` unit tests green |
| 12 | Expressions with nested parens parse correctly | VERIFIED | `parse_qualified_entries_nested_parens` unit test: `SUM(l_extendedprice * (1 - l_discount))` captured verbatim |
| 13 | `just test-all` (full suite) passes | VERIFIED | `cargo test`: 203+6+36+36+5+1 = 287 tests green; `just test-sql` [8/8] SUCCESS; `just test-ducklake-ci`: ALL PASSED; Python caret tests: ALL PASSED |

**Score:** 13/13 truths verified

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `cpp/src/shim.cpp` | 64 KB `std::string` buffer in `sv_ddl_bind`; 16 KB in `sv_parse_stub`; no `char sql_buf[4096]` | VERIFIED | Line 70: `std::string sql_str(16384, '\0')`. Line 138: `std::string sql_str(65536, '\0')`. Uses `sql_str.data()` and `sql_str.c_str()` correctly. |
| `src/body_parser.rs` | Complete implementation of 5 public/crate-public parser functions; 28 unit tests; no `todo!()` or `#[should_panic]` | VERIFIED | 28 tests green; grep shows 0 `todo!` and 0 `should_panic`; all clause parsers implemented with full recursive descent |
| `test/sql/phase25_keyword_body.test` | All 7 DDL verb tests passing; `require semantic_views` header; error cases for missing TABLES and missing DIMENSIONS/METRICS | VERIFIED | File exists; registered in `TEST_LIST`; `[8/8] SUCCESS` in `just test-sql` |
| `tests/parse_proptest.rs` | TEST-06 block with 3 AS-body proptest properties using full round-trip assertions | VERIFIED | Lines 665-736: `as_body_detected_for_create_forms`, `as_body_validate_and_rewrite_succeeds` (full `Ok(Some(sql))` check), `as_body_position_invariant_clause_typo` (actual position assertion) |
| `src/parse.rs` | `rewrite_ddl_keyword_body` function; AS dispatch in `validate_create_body`; `sv_rewrite_ddl_rust` calls `validate_and_rewrite` | VERIFIED | `parse_keyword_body` imported and called; `rewrite_ddl_keyword_body` routes to `create_semantic_view_from_json`; `sv_rewrite_ddl_rust` at line 865 calls `validate_and_rewrite` |
| `src/ddl/define.rs` | `DefineFromJsonVTab` struct with full VTab implementation accepting `(name VARCHAR, json VARCHAR)` | VERIFIED | `DefineFromJsonVTab` at line 250; deserializes JSON, runs DDL-time type inference, persists to catalog |
| `src/lib.rs` | 3 `_from_json` VTab functions registered: `create_semantic_view_from_json`, `create_or_replace_semantic_view_from_json`, `create_semantic_view_if_not_exists_from_json` | VERIFIED | Lines 384-398: all 3 registrations present using `DefineFromJsonVTab` with existing `DefineState` instances |
| `src/model.rs` | `pk_columns` on `TableRef`; `from_alias`, `fk_columns`, `name` on `Join` | VERIFIED | Lines 14, 94, 95, 98, 102: all fields present with `#[serde(default, skip_serializing_if)]` for backward compat |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `cpp/src/shim.cpp::sv_parse_stub` | `sv_validate_ddl_rust` | `sql_str.data()`, `sql_str.size()` | WIRED | Line 75-80: correct call with 16 KB buffer |
| `cpp/src/shim.cpp::sv_ddl_bind` | `sv_rewrite_ddl_rust` | `sql_str.data()`, `sql_str.size()`, then `sql_str.c_str()` | WIRED | Lines 138-153: 64 KB buffer; `duckdb_query(sv_ddl_conn, sql_str.c_str(), ...)` |
| `src/parse.rs::validate_create_body` | `src/body_parser.rs::parse_keyword_body` | AS detection route; calls `rewrite_ddl_keyword_body` which calls `parse_keyword_body` | WIRED | Line 636: `rewrite_ddl_keyword_body` called on AS path; line 681: `parse_keyword_body(body_text, body_offset)?` |
| `src/parse.rs::sv_rewrite_ddl_rust` | `validate_and_rewrite` | FFI dispatch calling `validate_and_rewrite` not old `rewrite_ddl` | WIRED | Line 865: `validate_and_rewrite(query)` — bug fix from Plan 04 |
| `src/ddl/define.rs::DefineFromJsonVTab` | `src/model.rs::SemanticViewDefinition::from_json` | JSON parameter deserialized via `from_json` | WIRED | `crate::model::SemanticViewDefinition::from_json(&name, &json)` in bind function |
| `src/body_parser.rs` | `src/model.rs` | Imports `Dimension`, `Join`, `Metric`, `TableRef` | WIRED | Line 6: `use crate::model::{Dimension, Join, Metric, TableRef}` |
| `test/sql/phase25_keyword_body.test` | `src/ddl/define.rs::DefineFromJsonVTab` | sqllogictest drives full extension pipeline end-to-end | WIRED | File is in `TEST_LIST` and `[8/8] SUCCESS` confirms the full DDL pipeline executes |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| DDL-01 | 25-01, 25-03, 25-04 | `CREATE SEMANTIC VIEW` accepts SQL keyword body: `TABLES (...)`, `RELATIONSHIPS (...)`, `DIMENSIONS (...)`, `METRICS (...)` | SATISFIED | `phase25_keyword_body.test` creates views with AS body syntax and queries them successfully |
| DDL-02 | 25-02, 25-04 | TABLES clause parses `alias AS physical_table PRIMARY KEY (col, ...)` | SATISFIED | `parse_tables_clause`: 5 unit tests green; sqllogictest end-to-end |
| DDL-03 | 25-02, 25-04 | RELATIONSHIPS clause parses `[name AS] from_alias(fk_cols) REFERENCES to_alias` | SATISFIED | `parse_relationships_clause`: 4 unit tests green; multi-table sqllogictest test passes |
| DDL-04 | 25-02, 25-04 | DIMENSIONS clause parses `alias.dim_name AS sql_expr` | SATISFIED | `parse_qualified_entries`: 5 unit tests green; dimension values verified in sqllogictest query result |
| DDL-05 | 25-02, 25-04 | METRICS clause parses `alias.metric_name AS agg_expr` | SATISFIED | Same `parse_qualified_entries` used; metrics verified in sqllogictest query result (SUM(revenue)) |
| DDL-07 | 25-01, 25-03, 25-04 | All 7 DDL verbs work with new syntax (CREATE, CREATE OR REPLACE, IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW) | SATISFIED | All 7 verbs tested in `phase25_keyword_body.test` and all pass |

Note: DDL-06 (Function-based `create_semantic_view()` accepts equivalent PK/FK model parameters) is assigned to Phase 24, not Phase 25. It is excluded from this verification.

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/body_parser.rs` | 419 | `let _ = after_pk_offset;` — computed but unused | Info | Cosmetic; offset tracked for future use per comment; no functional impact |
| `src/body_parser.rs` | 565 | `let _ = after_as_offset;` — computed but unused | Info | Same pattern; no functional impact |

No blockers or warnings found. The two `let _ =` suppressions are documented suppressions for future position tracking, not stubs.

---

### Human Verification Required

#### 1. Caret Error Position in DuckDB CLI

**Test:** Run `CREATE SEMANTIC VIEW v AS TABLSE (o AS orders PRIMARY KEY (id)) DIMENSIONS (o.r AS r) METRICS (o.m AS SUM(1));` in DuckDB CLI with the built extension loaded
**Expected:** Error with caret pointing at "TABLSE", message mentioning "did you mean TABLES"
**Why human:** Visual inspection of terminal caret output required to confirm UX experience
**Status:** APPROVED 2026-03-12 — Python integration test `test_caret_clause_typo` also programmatically verified caret position (ALL PASSED)

---

### Gaps Summary

No gaps. All 13 observable truths verified. All artifacts exist, are substantive, and are correctly wired.

The one auto-fixed bug from Plan 04 (`sv_rewrite_ddl_rust` calling `rewrite_ddl` instead of `validate_and_rewrite`) was correctly identified and fixed before the phase was complete. The full `just test-all` suite confirms the fix is working end-to-end.

---

_Verified: 2026-03-12_
_Verifier: Claude (gsd-verifier)_
