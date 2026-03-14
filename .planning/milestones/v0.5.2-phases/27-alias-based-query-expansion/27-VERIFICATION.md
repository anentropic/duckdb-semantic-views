---
phase: 27-alias-based-query-expansion
verified: 2026-03-13T18:00:00Z
status: passed
score: 5/5 must-haves verified
re_verification:
  previous_status: gaps_found
  previous_score: 4/5
  gaps_closed:
    - "Full test suite (just test-all) passes per CLAUDE.md quality gate"
  gaps_remaining: []
  regressions: []
---

# Phase 27: Alias-Based Query Expansion Verification Report

**Phase Goal:** Query expansion generates direct FROM+JOIN SQL with qualified column references instead of CTE flattening
**Verified:** 2026-03-13T18:00:00Z
**Status:** passed
**Re-verification:** Yes -- after gap closure (Plan 03 fixed Python caret tests and simplified error message)

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|---------|
| 1 | Expanded SQL uses `FROM physical_table AS alias LEFT JOIN physical_table AS alias ON ...` instead of `_base` CTE pattern | VERIFIED | `src/expand.rs` lines 427-448 emit `LEFT JOIN ... AS ... ON ...` unconditionally via `resolve_joins_pkfk`; no `WITH` or `_base` patterns in production code |
| 2 | Expressions containing qualified column references (`alias.column`) resolve correctly in generated SQL | VERIFIED | `test/sql/phase27_qualified_refs.test` passes with `c.name` and `sum(o.amount)` verbatim; unit tests `test_expand_qualified_column_refs_verbatim` and `test_expand_multiple_qualified_refs_different_tables` pass |
| 3 | The old `:=`/struct-literal DDL body parsing code is removed | VERIFIED | `parse_create_body`, `parse_ddl_text`, `validate_brackets`, `scan_clause_keywords`, `validate_clauses`, `check_close_bracket` all absent from `src/parse.rs`; `validate_create_body` returns "Expected 'AS' keyword" error for non-AS-body syntax |
| 4 | The CTE-based `_base` flattening expansion path is removed | VERIFIED | No `WITH` or `_base` patterns in `src/expand.rs` production code; pre-satisfied from Phase 26 |
| 5 | Full test suite (`just test-all`) passes per CLAUDE.md quality gate | VERIFIED | 281 Rust tests, 8/8 sqllogictests, 6/6 DuckLake CI, 13/13 vtab crash, 3/3 Python caret tests -- all pass |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/expand.rs` | Clean expand module with only PK/FK join resolution | VERIFIED | `resolve_joins`, `append_join_on_clause`, `has_pkfk` all absent; `resolve_joins_pkfk` at line 255 is sole path; 27 unit tests pass |
| `test/sql/phase27_qualified_refs.test` | sqllogictest verifying dot-qualified expressions end-to-end | VERIFIED | File exists, contains `c.name` and `sum(o.amount)`, passes `just test-sql` |
| `src/parse.rs` | Parse module with paren-body code path removed, simplified error message | VERIFIED | All 6 paren-body functions absent; error message is "Expected 'AS' keyword after view name. Use: ..." |
| `test/sql/phase20_extended_ddl.test` | Extended DDL tests rewritten to AS-body syntax | VERIFIED | Header says "Rewritten for AS-body keyword syntax"; passes sqllogictest |
| `test/sql/phase21_error_reporting.test` | Error tests updated -- "Expected 'AS' keyword" match | VERIFIED | Section 1 uses "Expected 'AS' keyword" substring; passes sqllogictest |
| `tests/parse_proptest.rs` | Proptest assertion updated to "Expected 'AS' keyword" | VERIFIED | `position_invariant_paren_body_rejected` asserts `err.message.contains("Expected 'AS' keyword")` |
| `test/integration/test_caret_position.py` | Python caret tests rewritten for AS-body error scenarios | VERIFIED | `test_caret_missing_paren` tests missing `(` after `TABLES` (caret at 38); `test_caret_clause_typo` tests misspelled `TBLES` keyword (caret at 26); all 3 tests pass |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/expand.rs::resolve_joins_pkfk` | `src/graph.rs::RelationshipGraph` | `RelationshipGraph::from_definition` | WIRED | Line 260: `let Ok(graph) = RelationshipGraph::from_definition(def)` |
| `src/parse.rs::validate_create_body` | `src/body_parser.rs::parse_keyword_body` | `rewrite_ddl_keyword_body` dispatch | WIRED | Line 432: `return rewrite_ddl_keyword_body(kind, name, after_name_trimmed, body_offset)` |
| `expand.rs` production code | FROM+JOIN SQL emission | No `WITH`/CTE clauses | WIRED | Lines 427-448: unconditional `LEFT JOIN ... AS ... ON ...` pattern |
| `src/parse.rs` error message | `test/integration/test_caret_position.py` | "Expected 'AS' keyword" text and caret position | WIRED | Test 1 asserts `"Expected '('"` at pos 38; Test 2 asserts `"TABLES"` suggestion at pos 26 |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|---------|
| EXP-01 | 27-01 | Query expansion uses FROM+JOIN (no CTE flattening) | SATISFIED | `src/expand.rs` produces `LEFT JOIN` SQL; no `_base` CTE; sqllogictest passes |
| EXP-05 | 27-01 | Qualified column references work in generated SQL | SATISFIED | Unit tests confirm verbatim emission; `phase27_qualified_refs.test` confirms end-to-end |
| CLN-01 | 27-02, 27-03 | Old `:=`/struct-literal DDL body parsing removed; all test suites pass | SATISFIED | Parse code removed; Python caret tests rewritten for AS-body; `just test-all` passes |
| CLN-02 | 27-01 | CTE-based `_base` flattening expansion path removed | SATISFIED | Pre-satisfied from Phase 26; confirmed absent in expand.rs |
| CLN-03 | 27-01 | ON-clause substring matching join heuristic removed | SATISFIED | `resolve_joins` and `append_join_on_clause` absent; `has_pkfk` conditional absent |

**Orphaned requirements check:** REQUIREMENTS.md traceability table maps EXP-01, EXP-05, CLN-01, CLN-02, CLN-03 to Phase 27. All are claimed in the plans. No orphaned requirements.

### Anti-Patterns Found

None -- all previous blockers resolved. The Python caret tests that previously used removed paren-body syntax have been rewritten to exercise AS-body error paths with correct caret positions.

### Human Verification Required

None -- all checks are automated. The quality gate passes deterministically via `just test-all`.

### Re-Verification Gap Closure Summary

**Gap that was open:** Two Python caret integration tests (`test_caret_missing_paren` and `test_caret_clause_typo`) used paren-body DDL that was removed by CLN-01, causing `just test-caret` to fail and the CLAUDE.md quality gate (`just test-all`) to not pass.

**How the gap was closed (Plan 03):**

1. The error message in `src/parse.rs::validate_create_body` was simplified: removed "old paren-body syntax" and "no longer supported" language per user directive (syntax was never publicly released). New message: "Expected 'AS' keyword after view name. Use: CREATE SEMANTIC VIEW name AS TABLES (...) DIMENSIONS (...) METRICS (...)"

2. All 4 test assertions on the error message were updated: 3 Rust unit tests in `src/parse.rs` and 1 proptest in `tests/parse_proptest.rs` now assert `err.message.contains("Expected 'AS' keyword")`.

3. The `phase21_error_reporting.test` Section 1 sqllogictest assertions were updated from "no longer supported" to "Expected 'AS' keyword".

4. `test_caret_missing_paren` was rewritten: old query `"CREATE SEMANTIC VIEW myview tables := []"` replaced with `"CREATE SEMANTIC VIEW myview AS TABLES x"`, which hits the body_parser "Expected '(' after clause keyword 'TABLES'" error at caret position 38.

5. `test_caret_clause_typo` was rewritten: old query using paren-body `(tbles := [...])` replaced with `"CREATE SEMANTIC VIEW x AS TBLES (...)"`, which hits the body_parser "Unknown clause keyword 'TBLES'; did you mean 'TABLES'?" error at caret position 26.

**Quality gate result:** `just test-all` passes -- 281 Rust tests, 8/8 sqllogictests, 6/6 DuckLake CI tests, 13/13 vtab crash tests, 3/3 Python caret tests.

---

_Verified: 2026-03-13T18:00:00Z_
_Verifier: Claude (gsd-verifier)_
