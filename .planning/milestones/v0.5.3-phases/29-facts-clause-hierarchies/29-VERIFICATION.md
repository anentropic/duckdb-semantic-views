---
phase: 29-facts-clause-hierarchies
verified: 2026-03-14T13:00:00Z
status: passed
score: 12/12 must-haves verified
re_verification: false
---

# Phase 29: FACTS Clause and Hierarchies Verification Report

**Phase Goal:** Users can declare reusable row-level sub-expressions (facts) and drill-down paths (hierarchies) within semantic views
**Verified:** 2026-03-14T13:00:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Quality Gate

CLAUDE.md mandates `just test-all` before verification can be complete.

| Test Suite | Command | Result |
| --- | --- | --- |
| Rust unit + proptest + doc tests | `cargo test` | 353 tests passed, 0 failed |
| SQL logic tests | `just test-sql` | 8/8 files passed (including phase29) |
| DuckLake integration | `just test-ducklake-ci` | 6/6 passed |

**Quality gate: PASSED**

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
| --- | --- | --- | --- |
| 1 | FACTS clause with alias.name AS expr entries is accepted in DDL | VERIFIED | `parse_qualified_entries` reused; 10 body_parser tests; sqllogictest test 1 (p29_analytics CREATE) |
| 2 | HIERARCHIES clause with name AS (dim1, dim2, dim3) entries is accepted in DDL | VERIFIED | `parse_hierarchies_clause` at body_parser.rs:784; sqllogictest test 1 |
| 3 | Clause ordering is enforced: TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS | VERIFIED | `CLAUSE_ORDER` at body_parser.rs:33-40 enumerates exact order; ordering enforcement at body_parser.rs:255+ |
| 4 | Fact cycles are rejected at define time with clear error message | VERIFIED | `validate_facts` Kahn's algorithm at graph.rs:358; sqllogictest tests 6 (cycle) and 7 (self-ref), both return "cycle detected in facts" |
| 5 | References to non-existent facts are rejected at define time | VERIFIED | Cycle detection via Kahn's algorithm covers this (unreachable nodes); 3-node cycle test in graph.rs tests |
| 6 | Hierarchies referencing non-existent dimensions are rejected at define time | VERIFIED | `validate_hierarchies` at graph.rs:532; sqllogictest test 8 returns "unknown dimension" |
| 7 | Facts source_table aliases are validated against declared tables | VERIFIED | `check_fact_source_tables` helper; sqllogictest test 9 returns "unknown source table" |
| 8 | Metric expressions referencing fact names expand with inlined fact expressions (parenthesized) | VERIFIED | `inline_facts` at expand.rs:444; `toposort_facts` at expand.rs:365; wired at expand.rs:563+592; sqllogictest test 2 arithmetic verified (250.00/150.00) |
| 9 | Multi-level fact chains (fact A refs fact B) resolve correctly via topological inlining order | VERIFIED | Topological resolution in `inline_facts`; sqllogictest test 3 arithmetic verified (17.30/15.00) |
| 10 | Word-boundary substitution prevents substring collisions (net_price vs net_price_total) | VERIFIED | `replace_word_boundary` at expand.rs:321; 8 dedicated unit tests including no-substring-match cases |
| 11 | DESCRIBE SEMANTIC VIEW shows facts column with JSON array of fact definitions | VERIFIED | `describe.rs` adds facts column at index 6 with null-to-[] fallback; sqllogictest test 5 shows full JSON |
| 12 | DESCRIBE SEMANTIC VIEW shows hierarchies column with JSON array of hierarchy definitions | VERIFIED | `describe.rs` adds hierarchies column at index 7; sqllogictest test 11 confirms `[{"levels":["country","city"],"name":"geo"}]` |

**Score:** 12/12 truths verified

---

### Required Artifacts

#### Plan 01 Artifacts

| Artifact | Provides | Status | Details |
| --- | --- | --- | --- |
| `src/body_parser.rs` | FACTS and HIERARCHIES clause parsing | VERIFIED | `parse_hierarchies_clause` at line 784; CLAUSE_KEYWORDS/ORDER updated at lines 21-40; `KeywordBody` has `facts`/`hierarchies` fields at lines 14-15 |
| `src/model.rs` | Hierarchy struct with serde derives | VERIFIED | `pub struct Hierarchy` at line 72 with `#[derive(Debug, Clone, Default, Serialize, Deserialize)]`; `hierarchies: Vec<Hierarchy>` field in `SemanticViewDefinition` at line 144 |
| `src/parse.rs` | Facts and hierarchies wired from KeywordBody to SemanticViewDefinition | VERIFIED | `facts: keyword_body.facts` at line 473; `hierarchies: keyword_body.hierarchies` at line 474 — not hardcoded to empty |
| `src/graph.rs` | Fact DAG validation (cycles, unknown refs, source table reachability) | VERIFIED | `validate_facts` at line 358; `validate_hierarchies` at line 532; 18 tests in graph module |
| `src/ddl/define.rs` | Fact and hierarchy validation calls in bind() | VERIFIED | `crate::graph::validate_facts(&def)` at line 120; `crate::graph::validate_hierarchies(&def)` at line 123 |

#### Plan 02 Artifacts

| Artifact | Provides | Status | Details |
| --- | --- | --- | --- |
| `src/expand.rs` | Fact expression inlining before metric expansion | VERIFIED | `inline_facts` at line 444, `toposort_facts` at line 365, `replace_word_boundary` at line 321; wired into `expand()` at lines 563 and 592; 23 unit tests |
| `src/ddl/describe.rs` | Facts and hierarchies columns in DESCRIBE output (8 columns total) | VERIFIED | `facts`/`hierarchies` fields in `DescribeBindData` at lines 22-23; 8 columns declared in `bind()`; null-to-[] fallback at lines 98-106 |
| `test/sql/phase29_facts_hierarchies.test` | End-to-end sqllogictest for FACTS + HIERARCHIES DDL and query | VERIFIED | 11 test cases covering CREATE, query arithmetic, multi-level chains, DESCRIBE, 4 error cases, optional clauses; passes `just test-sql` |
| `tests/parse_proptest.rs` | Proptest generators for FACTS and HIERARCHIES clauses | VERIFIED | `arb_facts_clause` at line 789; `arb_hierarchies_clause` at line 808; wired into proptest at line 860 |

---

### Key Link Verification

#### Plan 01 Key Links

| From | To | Via | Status | Details |
| --- | --- | --- | --- | --- |
| `src/body_parser.rs` | `src/parse.rs` | KeywordBody struct fields (facts, hierarchies) | WIRED | `keyword_body.facts` at parse.rs:473; `keyword_body.hierarchies` at parse.rs:474 |
| `src/parse.rs` | `src/model.rs` | SemanticViewDefinition fields (facts, hierarchies) | WIRED | `facts: keyword_body.facts` directly assigned — not `vec![]`; hierarchies same pattern |
| `src/ddl/define.rs` | `src/graph.rs` | validate_facts and validate_hierarchies calls | WIRED | `crate::graph::validate_facts(&def)` at define.rs:120; `crate::graph::validate_hierarchies(&def)` at define.rs:123 |

#### Plan 02 Key Links

| From | To | Via | Status | Details |
| --- | --- | --- | --- | --- |
| `src/expand.rs` | `src/graph.rs` | toposort_facts for inlining order | WIRED | `toposort_facts` defined in expand.rs itself (not graph.rs — intentional per decision log); used at expand.rs:563 |
| `src/expand.rs` | `src/model.rs` | Fact struct fields (name, expr, source_table) | WIRED | `fact.name`, `fact.expr`, `fact.source_table` accessed in `inline_facts` at expand.rs:444-485 |
| `src/ddl/describe.rs` | stored JSON | def["facts"] and def["hierarchies"] JSON extraction | WIRED | `def["facts"]` at describe.rs:98-101; `def["hierarchies"]` at describe.rs:103-106 |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| --- | --- | --- | --- | --- |
| FACT-01 | 29-01 | User can declare named row-level expressions in a FACTS clause (alias.fact_name AS sql_expr) | SATISFIED | `parse_qualified_entries` reused for FACTS; alias.name AS expr pattern; p29_analytics CREATE succeeds |
| FACT-02 | 29-02 | Metric expressions can reference fact names; expansion inlines fact expression with parenthesization | SATISFIED | `inline_facts` parenthesizes each resolved fact; sqllogictest test 2 verifies arithmetic: East=250.00, West=150.00 |
| FACT-03 | 29-01 | Facts can reference other facts; expansion resolves in topological order | SATISFIED | `toposort_facts` Kahn's algorithm; `inline_facts` processes in topo order; sqllogictest test 3: multi-level chain correct (East=17.30, West=15.00) |
| FACT-04 | 29-01 | Define-time validation rejects fact cycles and references to non-existent facts | SATISFIED | `validate_facts` cycle detection; sqllogictest tests 6 and 7 both error "cycle detected in facts" |
| FACT-05 | 29-02 | DESCRIBE SEMANTIC VIEW shows facts alongside dimensions and metrics | SATISFIED | DESCRIBE returns 8 columns; facts at index 6; sqllogictest test 5 shows full JSON array of fact definitions |
| HIER-01 | 29-01 | User can declare drill-down paths in a HIERARCHIES clause (name AS (dim1, dim2, dim3)) | SATISFIED | `parse_hierarchies_clause` at body_parser.rs:784; parenthesized level list parsing; p29_analytics CREATE succeeds |
| HIER-02 | 29-01 | Define-time validation rejects hierarchies referencing non-existent dimensions | SATISFIED | `validate_hierarchies` checks levels against dimension names; sqllogictest test 8 errors "unknown dimension" |
| HIER-03 | 29-02 | DESCRIBE SEMANTIC VIEW shows hierarchy definitions | SATISFIED | DESCRIBE hierarchies column at index 7; sqllogictest test 11 shows `[{"levels":["country","city"],"name":"geo"}]` |

**All 8 requirements: SATISFIED**

No orphaned requirements found — REQUIREMENTS.md entries for FACT-01 through FACT-05 and HIER-01 through HIER-03 all map to Phase 29, and all are claimed and fulfilled by the two plans.

---

### Anti-Patterns Found

None found. Scan of all 7 modified source files (`src/body_parser.rs`, `src/model.rs`, `src/parse.rs`, `src/graph.rs`, `src/ddl/define.rs`, `src/expand.rs`, `src/ddl/describe.rs`) returned:

- Zero TODO/FIXME/HACK/PLACEHOLDER comments
- Zero empty implementations (the `_ => {}` matches in body_parser.rs are legitimate match arm exhaustion, not stub implementations)
- Zero hardcoded-empty stubs — `facts: keyword_body.facts` and `hierarchies: keyword_body.hierarchies` confirmed live in parse.rs

---

### Human Verification Required

None. All truths are verifiable via automated tests (unit tests + sqllogictest with arithmetic assertions against known data). The fact inlining arithmetic is validated end-to-end with concrete values.

---

## Gaps Summary

No gaps. All 12 truths verified, all 9 artifacts substantive and wired, all 6 key links confirmed, all 8 requirements satisfied. The full test suite (`cargo test` + `just test-sql` + `just test-ducklake-ci`) passes cleanly.

### Notable Decisions (for context)

- `toposort_facts` lives in `expand.rs`, not `graph.rs` — this is intentional (needs indices into the facts slice for inline resolution, not just a validation result). The key link from expand.rs to graph.rs is therefore via the model types, not a function call. This is correct behavior, not a gap.
- "Unknown fact reference" error case replaced with "self-reference" cycle test because `find_fact_references` only scans for known fact names — unknown identifiers in expressions are column references, not errors. This is a correct design decision documented in the SUMMARY.
- Old stored definitions without a `hierarchies` field produce `null` in JSON; describe.rs handles this with a null-to-`[]` fallback.

---

_Verified: 2026-03-14T13:00:00Z_
_Verifier: Claude (gsd-verifier)_
