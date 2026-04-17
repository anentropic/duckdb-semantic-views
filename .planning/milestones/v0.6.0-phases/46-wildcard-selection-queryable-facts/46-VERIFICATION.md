---
phase: 46
name: Wildcard Selection + Queryable FACTS
status: passed
verified: 2026-04-12
must_haves_verified: 5/5
requirements_covered: 7/7
---

# Phase 46 Verification: Wildcard Selection + Queryable FACTS

## Goal
Users can query with table_alias.* wildcards for dimensions/metrics and can query facts at row level via the table function.

## Must-Haves Verification

### 1. Wildcard expansion for dimensions and metrics
**Status:** VERIFIED

- `expand_wildcards()` in `src/expand/wildcard.rs:21` resolves `alias.*` patterns to concrete names
- Called in `table_function.rs` bind() and `explain.rs` bind() before constructing QueryRequest
- 7 unit tests in wildcard.rs + 8 sqllogictest scenarios in `test/sql/phase46_wildcard.test`
- Covers dimension wildcards (WILD-01) and metric wildcards (WILD-02)

### 2. PRIVATE items excluded from wildcard expansion
**Status:** VERIFIED

- `expand_wildcards()` filters by `AccessModifier::Private` for metrics and facts
- Dimensions have no access field (always PUBLIC) — no filtering needed
- Test in `phase46_wildcard.test` verifies PRIVATE metric `raw_revenue` excluded from `li.*`
- Covers WILD-03

### 3. Fact queries return unaggregated results
**Status:** VERIFIED

- `expand_facts()` in `src/expand/sql_gen.rs:20` generates SELECT without GROUP BY
- Facts are inlined via `inline_facts()` for derived fact support
- `facts` named parameter registered on both `semantic_view()` and `explain_semantic_view()`
- LIMIT 0 type inference for fact queries in VTab bind()
- 12 sqllogictest scenarios in `test/sql/phase46_fact_query.test`
- Covers FACT-01 and FACT-02

### 4. Facts + metrics mutual exclusion
**Status:** VERIFIED

- `FactsMetricsMutualExclusion` error variant in `src/expand/types.rs:64`
- Check at `src/expand/sql_gen.rs:222` — fires before SQL generation
- Unit test at `sql_gen.rs:3379` + sqllogictest error test in `phase46_fact_query.test`
- Covers FACT-03

### 5. Fact query table path validation
**Status:** VERIFIED

- `validate_fact_table_path()` in `src/expand/fan_trap.rs:144` enforces linear path constraint
- Reuses `ancestors_to_root()` infrastructure from fan trap detection
- Called from expand() at `sql_gen.rs:91` when facts are present
- Sqllogictest covers divergent path rejection in `phase46_fact_query.test`
- Covers FACT-04

## Requirements Coverage

| Req ID | Description | Plan | Status |
|--------|-------------|------|--------|
| WILD-01 | Dimension wildcard (table_alias.*) | 46-01 | Complete |
| WILD-02 | Metric wildcard (table_alias.*) | 46-01 | Complete |
| WILD-03 | PRIVATE exclusion in wildcards | 46-01 | Complete |
| FACT-01 | Query facts via semantic_view() | 46-02 | Complete |
| FACT-02 | Unaggregated (row-level) results | 46-02 | Complete |
| FACT-03 | Facts + metrics mutual exclusion error | 46-01 | Complete |
| FACT-04 | Same logical table path constraint | 46-02 | Complete |

## Quality Gate

- `cargo test`: 583 tests passed (494 unit + 5 proptest + 36 integration + 42 misc + 5 fuzz + 1 doctest)
- `just test-sql`: 28 sqllogictest files passed (including new phase46_wildcard.test, phase46_fact_query.test)
- `just test-ducklake-ci`: DuckLake integration tests passed
- `just test-all`: PASSED (confirmed by both executor agents during plan execution)

## Key Files Created/Modified

### Created
- `src/expand/wildcard.rs` — wildcard expansion module
- `test/sql/phase46_wildcard.test` — 8 wildcard integration scenarios
- `test/sql/phase46_fact_query.test` — 12 fact query integration scenarios

### Modified
- `src/expand/types.rs` — QueryRequest.facts, 4 new ExpandError variants
- `src/expand/sql_gen.rs` — expand_facts(), dispatch logic, mutual exclusion check
- `src/expand/fan_trap.rs` — validate_fact_table_path()
- `src/expand/mod.rs` — wildcard module declaration
- `src/query/table_function.rs` — facts parameter, wildcard expansion, LIMIT 0 fact inference
- `src/query/explain.rs` — facts parameter, wildcard expansion
- `src/query/error.rs` — error variant display

## Human Verification

No human verification items — all behaviors are covered by automated tests.
