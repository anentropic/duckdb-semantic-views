---
phase: 33-unique-constraints-cardinality-inference
verified: 2026-03-15T21:00:00Z
status: passed
score: 20/20 must-haves verified
re_verification: false
---

# Phase 33: UNIQUE Constraints & Cardinality Inference Verification Report

**Phase Goal:** Users declare UNIQUE constraints on tables and the extension infers relationship cardinality automatically -- no explicit cardinality keywords needed
**Verified:** 2026-03-15T21:00:00Z
**Status:** PASSED
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | TableRef stores zero or more UNIQUE constraints as Vec<Vec<String>> | VERIFIED | `src/model.rs:20` — `pub unique_constraints: Vec<Vec<String>>` with `skip_serializing_if = "Vec::is_empty"` |
| 2 | Join stores ref_columns (resolved target-side columns for ON clause synthesis) | VERIFIED | `src/model.rs:154` — `pub ref_columns: Vec<String>` with `skip_serializing_if = "Vec::is_empty"` |
| 3 | Cardinality enum has exactly two variants: ManyToOne (default) and OneToOne | VERIFIED | `src/model.rs:107-111` — enum has only `ManyToOne` and `OneToOne`; `old_json_with_one_to_many_is_rejected` test asserts `serde_json::from_str::<Cardinality>(r#""OneToMany""#)` returns `Err` |
| 4 | Parser accepts UNIQUE (col, ...) after PRIMARY KEY in TABLES entries | VERIFIED | `src/body_parser.rs:536` — `fn find_unique` and `unique_constraints` loop at lines 495-530; sqllogictest Test 1 exercises this end-to-end |
| 5 | Parser accepts multiple UNIQUE constraints on one table entry | VERIFIED | `src/body_parser.rs` multiple_unique_constraints test; sqllogictest Test 2: `UNIQUE (email) UNIQUE (name)` |
| 6 | Parser no longer recognizes MANY TO ONE / ONE TO ONE / ONE TO MANY keywords | VERIFIED | `src/body_parser.rs:694` — function comment; `old_cardinality_keywords_rejected` test; sqllogictest Test 7 errors with "no longer supported" |
| 7 | Parser accepts REFERENCES target(col, ...) with explicit column list | VERIFIED | `src/body_parser.rs:802-816` — `ref_columns` parsed from paren content; `references_with_column_list` and `references_multi_column_list` tests |
| 8 | Parser accepts REFERENCES target with no column list (PK implicit) | VERIFIED | `src/body_parser.rs:803` — `if after_to.starts_with('(')` else branch stores empty ref_columns; resolved by `infer_cardinality` |
| 9 | PRIMARY KEY is optional on table entries (fact tables need no PK) | VERIFIED | `src/body_parser.rs` — pk_pos optional branch; `table_without_primary_key` test; sqllogictest Test 14 (`f AS p33_line_items` with no PK) |
| 10 | Cardinality is inferred: FK matches PK/UNIQUE on from-side = OneToOne, bare FK = ManyToOne | VERIFIED | `src/parse.rs:519` — `fn infer_cardinality` with HashSet comparison; `infers_one_to_one_from_pk_match`, `infers_one_to_one_from_unique_match`, `infers_many_to_one_when_fk_is_bare` tests |
| 11 | REFERENCES target with no column list when target has no PK produces define-time error | VERIFIED | `src/parse.rs:548-557` — error branch "has no PRIMARY KEY"; sqllogictest Test 10 asserts error |
| 12 | FK referenced columns must match a declared PK or UNIQUE on the target table -- error at define time if not | VERIFIED | `src/graph.rs:225` — `fn validate_fk_references`; sqllogictest Test 3 asserts "does not match any PRIMARY KEY or UNIQUE constraint" |
| 13 | Composite FK referencing a subset of a composite PK is rejected (exact match required) | VERIFIED | `src/graph.rs:234-261` — HashSet exact equality check; `composite_fk_subset_rejected` test; sqllogictest Test 12 |
| 14 | Fan trap detection works correctly with two-variant cardinality (ManyToOne reverse = fan-out, OneToOne = safe) | VERIFIED | `src/expand.rs:1169-1212` — `check_path_up` and `check_path_down` check only `Cardinality::ManyToOne`; no OneToMany branches; sqllogictest Tests 4 and 11 |
| 15 | ON clause synthesis uses Join.ref_columns instead of hardcoded pk_columns lookup | VERIFIED | `src/expand.rs:291-301` — `synthesize_on_clause_scoped` prefers `join.ref_columns`, falls back to pk_columns for backward compat |
| 16 | Old-format JSON (v0.5.3) is rejected on load with clear error message | VERIFIED | `src/ddl/define.rs:115-123` — guard before `validate_graph`; rejects joins with `fk_columns` but empty `ref_columns` |
| 17 | DESCRIBE shows UNIQUE constraints in tables JSON and inferred cardinality in joins JSON | VERIFIED | DESCRIBE flows through serde serialization automatically (no code change needed in describe.rs); sqllogictest Test 13 verifies no error and returns 1 row |
| 18 | Existing sqllogictests updated to use new syntax and pass | VERIFIED | `test/sql/phase31_fan_trap.test` has no MANY TO ONE / ONE TO ONE / ONE TO MANY DDL keywords; all 12 test files pass |
| 19 | New phase33 sqllogictest covers all CARD requirements end-to-end | VERIFIED | `test/sql/phase33_cardinality_inference.test` — 14 tests covering CARD-01 through CARD-09 with expected outputs |
| 20 | `just test-all` passes | VERIFIED | 468 cargo tests pass; 12 sqllogictest files pass; DuckLake CI 6/6 pass |

**Score:** 20/20 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/model.rs` | TableRef.unique_constraints, Join.ref_columns, two-variant Cardinality enum | VERIFIED | All three present; `phase33_model_tests` module with 7 tests; backward-compat serde |
| `src/body_parser.rs` | UNIQUE parsing, REFERENCES(cols) parsing, cardinality keyword removal | VERIFIED | `fn find_unique` at line 536; `ref_columns` populated at line 803; `parse_cardinality_tokens` deleted; `old_cardinality_keywords_rejected` test |
| `src/parse.rs` | `infer_cardinality` function called from `rewrite_ddl_keyword_body` | VERIFIED | `fn infer_cardinality` at line 519; called at line 462 via `let mut keyword_body`; `phase33_inference_tests` module with 8 tests |
| `src/graph.rs` | `validate_fk_references` -- CARD-03/09 FK reference validation | VERIFIED | `fn validate_fk_references` at line 225; called in `validate_graph` at line 360; `phase33_fk_reference_tests` module with 6 tests |
| `src/expand.rs` | Fan trap detection without OneToMany, ON clause using ref_columns | VERIFIED | No `Cardinality::OneToMany` in production code; `synthesize_on_clause_scoped` uses `ref_columns`; fan trap error uses "many-to-one cardinality, inferred" |
| `src/ddl/define.rs` | Old-JSON detection guard before validate_graph | VERIFIED | Guard at lines 115-123; string "created with an older version" present |
| `test/sql/phase33_cardinality_inference.test` | End-to-end sqllogictest for CARD-01 through CARD-09 | VERIFIED | 14 tests; covers all 9 CARD requirements; all assertions match expected error strings |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/body_parser.rs` | `src/model.rs` | `TableRef.unique_constraints` populated by parser | WIRED | `unique_constraints.push(cols)` at line 518; returned in `TableRef` struct at line 530 |
| `src/parse.rs` | `src/model.rs` | `infer_cardinality` sets `Join.ref_columns` and `Join.cardinality` | WIRED | `join.ref_columns.clone_from(&t.pk_columns)` at line 543; `join.cardinality = Cardinality::OneToOne/ManyToOne` at lines 599-613 |
| `src/body_parser.rs` | `src/model.rs` | `parse_single_relationship_entry` populates `Join.ref_columns` from REFERENCES(cols) | WIRED | `ref_columns` parsed at line 803; stored in returned `Join` at line 836 |
| `src/graph.rs` | `src/model.rs` | `validate_fk_references` reads `TableRef.unique_constraints` and `Join.ref_columns` | WIRED | `join.ref_columns` at line 238; `target.unique_constraints` at line 254 |
| `src/expand.rs` | `src/model.rs` | `synthesize_on_clause_scoped` uses `Join.ref_columns` | WIRED | `join.ref_columns` at line 301; falls back to `pk_columns` for legacy joins |
| `src/ddl/define.rs` | `src/model.rs` | Old-JSON check reads `Join.ref_columns` emptiness | WIRED | `join.ref_columns.is_empty()` at line 117 |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| CARD-01 | 33-01 | TABLES clause supports UNIQUE (col, ...) alongside PRIMARY KEY | SATISFIED | `find_unique` + `unique_constraints` loop in `body_parser.rs`; sqllogictest Test 1 |
| CARD-02 | 33-01 | A table can have one PK and multiple UNIQUE constraints | SATISFIED | Loop parses all UNIQUE occurrences; `multiple_unique_constraints` test; sqllogictest Test 2 |
| CARD-03 | 33-02 | Referenced columns must match declared PK or UNIQUE -- error at define time | SATISFIED | `validate_fk_references` in `graph.rs`; sqllogictest Test 3 |
| CARD-04 | 33-01 | Cardinality inferred from constraints: FK=PK/UNIQUE -> OneToOne, bare FK -> ManyToOne | SATISFIED | `infer_cardinality` in `parse.rs`; sqllogictest Tests 4, 5, 6 |
| CARD-05 | 33-01 | Explicit cardinality keywords removed from parser | SATISFIED | `parse_cardinality_tokens` deleted; trailing tokens rejected; sqllogictest Test 7 |
| CARD-06 | 33-01 | ManyToMany variant removed from Cardinality enum | SATISFIED | Cardinality enum has exactly ManyToOne and OneToOne in `model.rs`; was never explicitly `ManyToMany` in codebase (OneToMany was the prior third variant) |
| CARD-07 | 33-01 | REFERENCES target resolves to PK; REFERENCES target(col) resolves to matching PK or UNIQUE | SATISFIED | `infer_cardinality` PK resolution at line 543; explicit ref_columns passed through; sqllogictest Tests 8, 9, 10 |
| CARD-08 | 33-02 | Fan trap detection works using inferred cardinality values | SATISFIED | `check_path_up` and `check_path_down` check `Cardinality::ManyToOne`; sqllogictest Tests 4 and 11 |
| CARD-09 | 33-02 | Composite FK referencing subset of composite PK rejected (exact match only) | SATISFIED | HashSet equality (not subset) in `validate_fk_references`; sqllogictest Test 12 |

All 9 CARD requirements accounted for. No orphaned requirements.

### Anti-Patterns Found

No blocking or warning anti-patterns detected.

| File | Pattern | Severity | Assessment |
|------|---------|----------|------------|
| `tests/parse_proptest.rs` | 3 dead_code warnings (unused helper functions from earlier proptests) | Info | Non-blocking; unrelated to phase 33 changes |
| `src/expand.rs:292` | Comment typo: `/ Phase 33:` (missing leading `/` for doc comment) | Info | Non-blocking cosmetic; does not affect compilation or tests |
| `src/parse.rs:461` | Comment typo: `/ Phase 33:` (missing leading `/`) | Info | Non-blocking cosmetic |

### Human Verification Required

None. All observable behaviors are verified programmatically:
- UNIQUE constraint parsing verified via unit tests and sqllogictest DDL execution
- Cardinality inference verified via Rust unit tests with direct `infer_cardinality` calls
- FK reference validation verified via graph unit tests and sqllogictest error assertions
- Fan trap detection verified via sqllogictest error assertions
- Full quality gate (`just test-all`) passes

## Quality Gate

Per `CLAUDE.md`, the full quality gate is `just test-all`:

| Gate | Result |
|------|--------|
| `cargo test` (unit + proptest + doc tests) | 468 tests pass (376 + 6 + 36 + 44 + 5 + 1) |
| `just test-sql` (12 sqllogictest files) | 12/12 SUCCESS (including `phase33_cardinality_inference.test`) |
| `just test-ducklake-ci` (DuckLake integration) | 6/6 PASS |

## Task Commits Verified

| Commit | Description |
|--------|-------------|
| `b7bfce9` | feat(33-01): extend model with UNIQUE constraints, ref_columns, two-variant Cardinality |
| `8f38735` | feat(33-01): update parser -- UNIQUE parsing, remove cardinality keywords, REFERENCES(cols) |
| `705e517` | feat(33-01): implement cardinality inference in parse.rs |
| `f2d2cad` | feat(33-02): CARD-03/09 validation, fan trap inference, ON clause ref_columns, old-JSON guard |
| `9d19b5f` | test(33-02): update sqllogictests for Phase 33 cardinality inference |

All 5 commits present in git history on branch `main`.

## Gaps Summary

No gaps. All 20 must-haves verified. All 9 CARD requirements satisfied. Full quality gate passes.

---

_Verified: 2026-03-15T21:00:00Z_
_Verifier: Claude (gsd-verifier)_
