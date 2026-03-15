---
phase: 32-role-playing-using
verified: 2026-03-14T19:53:05Z
status: passed
score: 11/11 must-haves verified
re_verification: true
gaps: []
human_verification: []
---

# Phase 32: Role-Playing Dimensions and USING RELATIONSHIPS Verification Report

**Phase Goal:** Users can join the same physical table via multiple named relationships and select specific join paths per metric
**Verified:** 2026-03-14T19:53:05Z
**Status:** PASSED (after re-verification)
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | Multiple named relationships to the same table are accepted at define time (diamond rejection relaxed) | VERIFIED | `check_no_diamonds` in `src/graph.rs:151` accepts all-named unique relationships; 4 diamond tests pass: `diamond_two_named_relationships_accepted`, `diamond_two_unnamed_relationships_rejected`, `diamond_detected`, `diamond_mixed_named_unnamed_rejected` |
| 2  | Metrics can declare USING (relationship_name) in DDL and the relationship name is stored in the model | VERIFIED | `Metric.using_relationships: Vec<String>` at `src/model.rs:57` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`; `parse_single_metric_entry` extracts USING clause at `src/body_parser.rs:824`; 4-tuple return from `parse_metrics_clause`; serde roundtrip tests pass |
| 3  | USING references to non-existent relationships produce a clear define-time error | VERIFIED | `validate_using_relationships` in `src/graph.rs:1050` checks existence; wired into `define.rs:131`; `validate_using_unknown_relationship_rejected` test passes; sqllogictest Test 5 verifies `unknown relationship` error at define time |
| 4  | Derived metrics with USING produce a define-time error (Snowflake constraint) | VERIFIED | `parse_single_metric_entry` checks `using_relationships.is_empty()` when unqualified at `src/body_parser.rs:878`; `parse_metrics_using_on_derived_produces_error` unit test passes; `validate_using_derived_metric_rejected` graph test passes |
| 5  | Old definitions without using_relationships deserialize with empty Vec (backward compat) | VERIFIED | `#[serde(default)]` on field; `old_json_without_using_relationships_deserializes_with_empty_vec` test passes; sqllogictest Test 7 verifies backward-compatible standard semantic view |
| 6  | Metrics with USING expand with relationship-scoped aliases in generated SQL | VERIFIED | `resolve_joins_pkfk` generates scoped aliases `{to_alias}__{rel_name}` at `src/expand.rs:503,521`; `using_metric_generates_scoped_join_alias` unit test passes; sqllogictest Test 1 produces correct city-grouped results via scoped JOIN |
| 7  | Same physical table joined via different named relationships produces separate LEFT JOINs | VERIFIED | `two_using_metrics_generate_two_scoped_joins` unit test passes; sqllogictest Test 2 verifies both `departure_count` and `arrival_count` work simultaneously with carrier dimension |
| 8  | Dimensions from a role-playing table resolve to correct scoped alias based on co-queried metric's USING | VERIFIED | `find_using_context()` at `src/expand.rs:335` resolves alias; dimension expression rewriting replaces bare alias with scoped alias; `dimension_rewritten_to_scoped_alias` unit test passes; sqllogictest Tests 1 and 10 verify correct city grouping per departure/arrival |
| 9  | Querying a dimension from an ambiguous multi-path table without USING produces an AmbiguousPath error | VERIFIED | `AmbiguousPath` variant in `ExpandError` at `src/expand.rs:79`; `ambiguous_dimension_without_using_produces_error` test passes; sqllogictest Tests 3 and 9 verify `statement error` with `ambiguous` message |
| 10 | Classic flights/airports role-playing pattern works end-to-end through DDL → query → correct results | VERIFIED | `test/sql/phase32_role_playing.test` (241 lines, 10 scenarios); all 11 sqllogictests pass including `phase32_role_playing.test`; Tests 1-10 cover DDL define, departure/arrival metrics, carrier dimension, derived metrics, backward compat, single relationship |
| 11 | Fan trap detection still works correctly with USING-scoped paths | VERIFIED | `fan_trap_detection_works_with_using_paths` unit test passes; `just test-all` passes after `find_keyword_ci` fix (7e9f01e) |

**Score:** 11/11 truths verified

---

### Required Artifacts

#### Plan 01 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/model.rs` | `using_relationships Vec<String>` field on `Metric` struct | VERIFIED | `pub using_relationships: Vec<String>` at line 57; `#[serde(default, skip_serializing_if = "Vec::is_empty")]` present; 3 unit tests in `phase32_using_relationships_tests` module all pass |
| `src/body_parser.rs` | USING clause parsing in `parse_single_metric_entry` | VERIFIED | USING keyword detection at line 826 using `find_keyword_ci`; parenthesized list extraction; 4-tuple return type; 5 unit tests pass including `parse_metrics_using_single_relationship`, `parse_metrics_using_multiple_relationships`, `parse_metrics_using_on_derived_produces_error`, `parse_metrics_without_using_backward_compat`; `parse_keyword_body_with_using_metrics` integration test passes |
| `src/graph.rs` | Relaxed diamond check + `validate_using_relationships` | VERIFIED | `check_no_diamonds` accepts named unique multi-path relationships at line 163; `validate_using_relationships` at line 1050 with 3-constraint validation; 8 unit tests covering all branches pass |
| `src/ddl/define.rs` | Call to `validate_using_relationships` in `bind()` | VERIFIED | `crate::graph::validate_using_relationships(&def)` at line 131; wired after `validate_derived_metrics` in validation chain |

#### Plan 02 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/expand.rs` | `AmbiguousPath` error variant, USING-aware `resolve_joins_pkfk`, scoped alias generation, dimension resolution | VERIFIED | `AmbiguousPath` variant at line 79; `find_using_context()` at line 335; `collect_derived_metric_using()` at line 402; `synthesize_on_clause_scoped()` at line 286; `resolve_joins_pkfk` with scoped alias generation at lines 487-523; 13 Phase 32 unit tests all pass |
| `test/sql/phase32_role_playing.test` | End-to-end sqllogictest, min 50 lines | VERIFIED | 241 lines; 10 test scenarios (Tests 1-10); all pass via `just test-sql`; covers ROLE-03 core, JOIN-04, JOIN-05, backward compat, single relationship, arrival metric |

#### Proptest Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `tests/parse_proptest.rs` | `metric_using_clause_roundtrip` proptest for USING clause | STUB/BROKEN | Proptest EXISTS but FAILS due to broken `metric_name` regex `[b-z][a-z0-9_]{1,15}` generating names like `b_as` containing embedded AS keyword — 0/0 successes, fails immediately |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/body_parser.rs` | `src/model.rs` | `parse_metrics_clause` returns 4-tuple with `using_relationships` | WIRED | Grep confirms `using_relationships` set when constructing `Metric` in callers; tests exercise the full parse → model path |
| `src/ddl/define.rs` | `src/graph.rs` | `validate_using_relationships` called after `validate_derived_metrics` | WIRED | `crate::graph::validate_using_relationships(&def)` at `define.rs:131`; sqllogictest Test 5 exercises the full DDL → validation → error path |
| `src/graph.rs` | `src/model.rs` | `check_no_diamonds` reads `j.name` for named relationship detection | WIRED | `j.name.is_some()` at `graph.rs:164`; `j.name.as_ref().unwrap()` at line 170; diamond unit tests exercise this path |
| `src/expand.rs` | `src/model.rs` | Reads `metric.using_relationships` for scoped aliases | WIRED | `&met.using_relationships` iterated at `expand.rs:356,431`; `resolve_joins_pkfk` generates scoped aliases from USING data |
| `src/expand.rs` | `src/graph.rs` | `RelationshipGraph` reverse map identifies multi-path tables | WIRED | `RelationshipGraph::from_definition` called inside `find_using_context` at line 340; `reverse` map used to detect role-playing tables |
| `test/sql/phase32_role_playing.test` | `src/expand.rs` | End-to-end test exercises full DDL → query → expansion pipeline | WIRED | `semantic_view('p32_flights_view', ...)` calls in all 10 test scenarios; all pass via `just test-sql` |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| JOIN-01 | 32-01-PLAN.md | Multiple named relationships between same table pair are accepted | SATISFIED | `check_no_diamonds` relaxed; `diamond_two_named_relationships_accepted` test; sqllogictest Test 1 defines 2 named relationships to airports |
| JOIN-02 | 32-01-PLAN.md | Metrics can declare `USING (relationship_name)` to select a specific join path | SATISFIED | USING clause parsing in `parse_single_metric_entry`; `Metric.using_relationships` field; sqllogictest DDL with `f.departure_count USING (dep_airport) AS COUNT(*)` |
| JOIN-03 | 32-02-PLAN.md | Expansion generates separate JOINs with relationship-scoped aliases when USING is specified | SATISFIED | `resolve_joins_pkfk` generates `a__dep_airport`, `a__arr_airport`; `two_using_metrics_generate_two_scoped_joins` test; sqllogictest Tests 1 and 2 |
| JOIN-04 | 32-01-PLAN.md | Define-time validation rejects USING references to non-existent relationships | SATISFIED | `validate_using_relationships` with `unknown relationship` error; `validate_using_unknown_relationship_rejected` test; sqllogictest Test 5 |
| JOIN-05 | 32-02-PLAN.md | Querying a dimension from an ambiguous multi-path table without USING produces a clear error | SATISFIED | `AmbiguousPath` error in `find_using_context`; sqllogictest Tests 3 and 9 with `ambiguous` error message |
| ROLE-01 | 32-02-PLAN.md | Same physical table joined via different named relationships produces distinct aliases in expanded SQL | SATISFIED | Scoped aliases `{alias}__{rel_name}` in `resolve_joins_pkfk`; `using_metric_generates_scoped_join_alias` test; `two_using_metrics_generate_two_scoped_joins` test |
| ROLE-02 | 32-02-PLAN.md | Dimensions from a role-playing table resolve to the correct alias based on co-queried metric's USING | SATISFIED | `find_using_context()` resolver; dimension expression rewriting; `dimension_rewritten_to_scoped_alias` test; sqllogictest Tests 1 and 10 |
| ROLE-03 | 32-02-PLAN.md | Classic role-playing pattern works end-to-end (flights with departure/arrival airports) | SATISFIED | `test/sql/phase32_role_playing.test` with flights/airports tables; 10 scenarios including define, query, verify correct aggregations |

All 8 requirements (JOIN-01, JOIN-02, JOIN-03, JOIN-04, JOIN-05, ROLE-01, ROLE-02, ROLE-03) are satisfied at the implementation level. No orphaned requirements were found — all 8 declared in plan frontmatter match the REQUIREMENTS.md traceability table.

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `tests/parse_proptest.rs` | ~1159 | `metric_name in proptest::string::string_regex("[b-z][a-z0-9_]{1,15}")` — allows embedded AS keyword (e.g. `b_as`) | Blocker | `metric_using_clause_roundtrip` proptest fails deterministically with `name = "b_as"` (0 successes), blocking `just test-all` quality gate |
| `tests/parse_proptest.rs` | ~1099 | `name in arb_view_name()` — relationship name generator produces `as_` which confuses AS keyword parser | Blocker | `relationship_cardinality_keyword_variants` and `relationship_no_cardinality_defaults` fail; these are pre-existing from Phase 31 but remain unresolved |

No `TODO`, `FIXME`, `HACK`, `PLACEHOLDER`, `todo!()`, or `unimplemented!()` markers found in any of the source files modified by Phase 32 (`src/model.rs`, `src/body_parser.rs`, `src/graph.rs`, `src/ddl/define.rs`, `src/expand.rs`). No stub implementations in production code.

---

### Human Verification Required

None — all behaviors are verifiable programmatically through unit tests and sqllogictest integration tests.

---

## Quality Gate Results

Per `CLAUDE.md`, the full `just test-all` suite must pass:

| Test Category | Command | Result | Details |
|---------------|---------|--------|---------|
| Rust unit + proptest + doc tests | `cargo test` | PASSED | All tests pass after `find_keyword_ci` fix (7e9f01e) — underscore now treated as identifier char in keyword boundary detection |
| SQL logic tests | `just test-sql` | PASSED | 11/11 sqllogictests pass, including `phase32_role_playing.test` |
| DuckLake CI tests | `just test-ducklake-ci` | PASSED | 6/6 DuckLake integration tests pass |

**Quality gate status: PASSED** — `just test-all` passes after `find_keyword_ci` fix (commit 7e9f01e).

### Root Cause Analysis

The 3 proptest failures (`metric_using_clause_roundtrip`, `relationship_cardinality_keyword_variants`, `relationship_no_cardinality_defaults`) were caused by a real parser bug in `find_keyword_ci` (body_parser.rs:556). The function used `is_ascii_alphanumeric()` for word boundary detection, but underscore is a valid SQL identifier character. Identifiers like `b_as` and `as_` were incorrectly matched as containing the `AS` keyword because `_` was treated as a word boundary.

Fix: `find_keyword_ci` now checks `!c.is_ascii_alphanumeric() && c != b'_'` for boundary detection. All 45 proptests pass.

---

## Gaps Summary

Phase 32 goal is substantively achieved: the implementation correctly handles multiple named relationships, USING clause parsing and storage, define-time validation, scoped alias generation, dimension expression rewriting, AmbiguousPath detection, and the flights/airports end-to-end pattern. All 8 requirements are satisfied and all 16 Phase 32 unit tests pass, all 11 sqllogictests pass, and DuckLake CI passes.

However, `just test-all` (the quality gate per CLAUDE.md) fails with 3 proptest failures:

1. **`metric_using_clause_roundtrip`** — A Phase 32 proptest with a broken metric name regex that allows embedded AS keywords. This is a new defect in the test infrastructure.

2. **`relationship_cardinality_keyword_variants`** and **`relationship_no_cardinality_defaults`** — Pre-existing failures from Phase 31 (arb_view_name can generate `as_` as a relationship name). Not caused by Phase 32, but Phase 32 SUMMARY's claim that these were "pre-existing and out of scope" does not exempt the quality gate from passing.

The root cause was a production parser bug in `find_keyword_ci` — underscore was not treated as an identifier character in word boundary detection. Fixed in commit 7e9f01e. All 45 proptests and full `just test-all` suite now pass.

---

_Verified: 2026-03-14T19:53:05Z_
_Re-verified: 2026-03-14 after find_keyword_ci fix_
_Verifier: Claude (gsd-verifier)_
