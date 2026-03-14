---
phase: 26-pk-fk-join-resolution
verified: 2026-03-13T15:00:00Z
status: passed
score: 4/4 must-haves verified
re_verification: false
---

# Phase 26: PK/FK Join Resolution Verification Report

**Phase Goal:** JOIN ON clauses are deterministically synthesized from PK/FK declarations, with invalid graphs rejected at define time
**Verified:** 2026-03-13
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (from ROADMAP.md Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Given PK/FK declarations, expansion engine generates correct `ON a.fk = b.pk` clauses without user-written ON expressions | VERIFIED | `synthesize_on_clause()` in expand.rs:341 zips `join.fk_columns` with `table_ref.pk_columns`; unit test `test_pkfk_on_clause_simple` and sqllogictest Test 1 (p26_sales) confirm end-to-end |
| 2 | Requesting dims from A and C connected through B auto-joins through B (transitive inclusion) | VERIFIED | `resolve_joins_pkfk()` in expand.rs:370 walks reverse edges to root; unit test `test_pkfk_transitive_join_inclusion` and sqllogictest Test 2 (p26_detailed, 3-table) confirm |
| 3 | Defining a view with cyclic or diamond relationship graph produces a clear error at define time | VERIFIED | `validate_graph()` in graph.rs:270 called in both `DefineSemanticViewVTab::bind()` (define.rs:120) and `DefineFromJsonVTab::bind()` (define.rs:276) before persisting; sqllogictest Tests 5 confirms "cycle detected" and "cannot reference itself" errors |
| 4 | Join ordering follows topological sort, producing deterministic SQL regardless of declaration order | VERIFIED | `toposort()` in graph.rs:90 uses Kahn's algorithm with deterministic seeding (root first, others sorted); unit test `test_pkfk_topological_order` reverses declaration order and verifies emission order unchanged |

**Score:** 4/4 truths verified

---

## Required Artifacts

### Plan 01 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/graph.rs` | RelationshipGraph struct, validate_graph(), toposort(), all validation functions | VERIFIED | 732 lines; exports `RelationshipGraph`, `validate_graph`; contains `toposort`, `check_no_diamonds`, `check_no_orphans`, `check_fk_pk_counts`, `check_source_tables_reachable`; 14 unit tests |
| `src/lib.rs` | `pub mod graph` declaration | VERIFIED | Line 4: `pub mod graph;` |
| `src/ddl/define.rs` | `validate_graph()` call in both bind() functions | VERIFIED | Line 120 (`DefineSemanticViewVTab::bind`) and line 276 (`DefineFromJsonVTab::bind`) both call `crate::graph::validate_graph` before type inference and persisting |

### Plan 02 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/expand.rs` | Graph-based `resolve_joins_pkfk()`, `synthesize_on_clause()`, LEFT JOIN emission | VERIFIED | Contains `use crate::graph::RelationshipGraph` (line 4), `synthesize_on_clause` (line 341), `resolve_joins_pkfk` (line 370); LEFT JOIN emitted in both PK/FK path (line 563) and legacy path (line 574); 8 new unit tests in `phase26_pkfk_expand_tests` module |
| `test/sql/phase26_join_resolution.test` | Integration tests for PK/FK join synthesis, transitive inclusion, error cases | VERIFIED | 143 lines; 5 tests: basic PK/FK join, transitive 3-table, pruning, LEFT JOIN NULL preservation, cycle + self-reference errors |
| `test/sql/TEST_LIST` | `phase26_join_resolution.test` registered | VERIFIED | Line 9: `test/sql/phase26_join_resolution.test` |

---

## Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/ddl/define.rs` | `src/graph.rs` | `crate::graph::validate_graph()` call in both bind functions | WIRED | Lines 120 and 276 in define.rs; before type inference and before persist in both code paths |
| `src/graph.rs` | `src/model.rs` | Reads `SemanticViewDefinition.tables`, `.joins`, `.dimensions`, `.metrics` | WIRED | `from_definition()` iterates `def.tables` and `def.joins`; `check_source_tables_reachable()` iterates `def.dimensions` and `def.metrics` |
| `src/expand.rs` | `src/graph.rs` | `RelationshipGraph::from_definition` + `toposort` for join ordering | WIRED | `use crate::graph::RelationshipGraph` at line 4; `RelationshipGraph::from_definition(def)` at line 375; `graph.toposort()` at line 419 |
| `src/expand.rs` | `src/model.rs` | `Join.fk_columns` + `TableRef.pk_columns` for ON clause synthesis | WIRED | `synthesize_on_clause()` at line 341 zips `join.fk_columns` with `table_ref.pk_columns`; `has_pkfk` detection at line 500 uses `j.fk_columns.is_empty()` |
| `test/sql/phase26_join_resolution.test` | `src/expand.rs` | End-to-end CREATE + query validating generated SQL | WIRED | `semantic_view('p26_sales', ...)` queries exercise both DDL path and expansion path; sqllogictest all 9 files pass |

---

## Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| EXP-02 | 26-01, 26-02 | JOIN ON clauses synthesized from PK/FK declarations | SATISFIED | `synthesize_on_clause()` generates `from_alias.fk = to_alias.pk` pairs; verified by unit tests and sqllogictest Test 1 |
| EXP-03 | 26-01 | Join ordering via topological sort of relationship graph | SATISFIED | `toposort()` (Kahn's algorithm) in graph.rs; `resolve_joins_pkfk()` filters toposort output; `test_pkfk_topological_order` verifies declaration-order independence |
| EXP-04 | 26-01, 26-02 | Transitive join inclusion (dims from A and C auto-joins through B) | SATISFIED | Reverse-edge walking in `resolve_joins_pkfk()`; `test_pkfk_transitive_join_inclusion` and sqllogictest Test 2 (3-table chain) |
| EXP-06 | 26-01 | Define-time validation: relationship graph must be a tree (error on diamonds/cycles) | SATISFIED | 6 validation checks in `validate_graph()`; wired into both DDL bind paths; sqllogictest Tests 5 (cycle, self-ref) |

### Orphaned Requirements Check

REQUIREMENTS.md traceability maps EXP-01 and CLN-02 to Phase 26 as "Complete". These IDs do not appear in either plan's `requirements:` frontmatter field. Reviewing the evidence:

- **EXP-01** ("Query expansion generates alias-based `FROM base AS alias LEFT JOIN t AS alias ON ...`"): The CTE wrapper was removed in Plan 02 as an auto-fixed bug (expand.rs doc comment is stale but actual output is flat SELECT/FROM/JOIN). Evidence: no `WITH ... AS` or `_base` patterns in expand.rs output code; sqllogictest passes with correct results.
- **CLN-02** ("Remove CTE-based `_base` flattening expansion path"): Confirmed removed in Plan 02. The `WITH "_base" AS (...)` block no longer exists in expand.rs.

Both EXP-01 and CLN-02 are incidentally satisfied as part of Phase 26's Plan 02 CTE-removal bug fix. They are not gaps — they are completed. However, they were not declared in plan frontmatter `requirements:` fields.

| Requirement | Traceability | Declared in Plan | Status | Notes |
|-------------|-------------|------------------|--------|-------|
| EXP-01 | Phase 26 | Not in 26-01 or 26-02 frontmatter | SATISFIED | Flat query pattern is the actual output; CTE was removed during Plan 02 |
| CLN-02 | Phase 26 | Not in 26-01 or 26-02 frontmatter | SATISFIED | No `_base` CTE in expand.rs |

---

## Anti-Patterns Found

No anti-patterns detected in any phase 26 modified files.

| File | Pattern | Severity | Result |
|------|---------|----------|--------|
| `src/graph.rs` | TODO/FIXME/placeholder | Info | None found |
| `src/expand.rs` | TODO/FIXME/placeholder | Info | None found |
| `src/ddl/define.rs` | Empty implementations | Info | None found |
| `src/expand.rs` | Stale doc comment line 428 says "CTE-wrapped" | Info | Minor stale comment; does not affect behavior |

---

## Quality Gate

Per CLAUDE.md, all phases must pass the full test suite.

| Test Suite | Command | Result |
|------------|---------|--------|
| Rust unit + proptest + doc tests | `cargo test` | PASSED (240 + 6 + 36 + 45 + 5 + 1 = 333 tests, 0 failed) |
| Graph module tests | `cargo test --lib graph` | PASSED (14 tests) |
| Expand module tests | `cargo test --lib expand` | PASSED (54 tests, 8 new phase26 tests) |
| SQL logic tests | `just build && just test-sql` | PASSED (9/9 files including phase26_join_resolution.test) |
| DuckLake CI tests | `just test-ducklake-ci` | PASSED (6/6 tests) |

---

## Human Verification Required

None. All phase behaviors have automated verification coverage via unit tests and sqllogictest integration tests.

---

## Commits Verified

All commits referenced in SUMMARY files exist in git history:

| Commit | Message | Plan |
|--------|---------|------|
| `9e4c52f` | feat(26-01): add relationship graph module with validation and topological sort | 26-01 Task 1 |
| `2df3048` | feat(26-01): wire validate_graph into both DDL define paths | 26-01 Task 2 |
| `c123c0f` | test(26-02): add failing tests for PK/FK join resolution (TDD RED) | 26-02 Task 1 RED |
| `1f26983` | feat(26-02): graph-based PK/FK join resolution with LEFT JOIN emission | 26-02 Task 1 GREEN |
| `b5e1fdd` | feat(26-02): end-to-end PK/FK join resolution with sqllogictest integration | 26-02 Task 2 |

---

## Summary

Phase 26 goal is achieved. The codebase delivers:

1. **Graph validation at define time** — `validate_graph()` in graph.rs runs 6 checks (self-reference, cycles, diamonds, orphans, FK/PK count mismatch, unreachable source tables) and is wired into both DDL paths (`DefineSemanticViewVTab` and `DefineFromJsonVTab`) before type inference and persisting.

2. **Deterministic JOIN ON synthesis** — `synthesize_on_clause()` generates `from_alias.fk = to_alias.pk` pairs from declarations; no user-written ON expressions required.

3. **Transitive join inclusion** — `resolve_joins_pkfk()` walks reverse edges from needed aliases back to the root, collecting all intermediate aliases automatically.

4. **Topological join ordering** — Kahn's algorithm with deterministic seeding ensures join emission order is root-outward regardless of declaration order.

5. **LEFT JOIN everywhere** — Both PK/FK and legacy paths emit LEFT JOIN.

6. **CTE wrapper removed** — Flat SELECT/FROM/JOIN pattern replaces `WITH "_base" AS (...)`, satisfying EXP-01 and CLN-02 incidentally.

All 4 ROADMAP success criteria are verified. All 4 declared requirements (EXP-02, EXP-03, EXP-04, EXP-06) are satisfied. Full quality gate (`just test-all`) passes.

---

_Verified: 2026-03-13_
_Verifier: Claude (gsd-verifier)_
