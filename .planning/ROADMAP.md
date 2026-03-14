# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** -- Phases 1-7 (shipped 2026-02-28)
- ✅ **v0.2.0 Native DDL + Time Dimensions** -- Phases 8-14 (shipped 2026-03-03)
- ✅ **v0.3.0 Zero-Copy Query Pipeline** -- (shipped 2026-03-03)
- ✅ **v0.4.0 Simplified Dimensions** -- (shipped 2026-03-03)
- ✅ **v0.5.0 Parser Extension Spike** -- Phases 15-18 (shipped 2026-03-08)
- ✅ **v0.5.1 DDL Polish** -- Phases 19-23 (shipped 2026-03-09)
- ✅ **v0.5.2 SQL DDL & PK/FK Relationships** -- Phases 24-28 (shipped 2026-03-13)
- **v0.5.3 Advanced Semantic Features** -- Phases 29-32 (in progress)

## Phases

<details>
<summary>v0.1.0 MVP (Phases 1-7) -- SHIPPED 2026-02-28</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-02-24
- [x] Phase 2: Storage and DDL (4/4 plans) -- completed 2026-02-24
- [x] Phase 3: Expansion Engine (3/3 plans) -- completed 2026-02-25
- [x] Phase 4: Query Interface (3/3 plans) -- completed 2026-02-25
- [x] Phase 5: Hardening and Docs (2/2 plans) -- completed 2026-02-26
- [x] Phase 6: Tech Debt Code Cleanup (1/1 plan) -- completed 2026-02-26
- [x] Phase 7: Verification & Formal Closure (2/2 plans) -- completed 2026-02-27

Full details: [milestones/v0.1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

<details>
<summary>v0.2.0 Native DDL + Time Dimensions (Phases 8-14) -- SHIPPED 2026-03-03</summary>

- [x] Phase 8: C++ Shim Infrastructure (2/2 plans) -- completed 2026-03-01
- [x] Phase 9: Time Dimensions (2/2 plans) -- completed 2026-03-01
- [x] Phase 10: pragma_query_t Catalog Persistence (3/3 plans) -- completed 2026-03-01
- [x] Phase 11: Scalar Function DDL (4/4 plans) -- completed 2026-03-02
- [x] Phase 11.1: Snowflake-aligned STRUCT/LIST DDL Syntax (5/5 plans) -- completed 2026-03-02
- [x] Phase 12: EXPLAIN + Typed Output (4/4 plans) -- completed 2026-03-02
- [x] Phase 13: Type-mapping + PBTs (2/2 plans) -- completed 2026-03-02
- [x] Phase 14: DuckLake Integration Test + CI (3/3 plans) -- completed 2026-03-02

Full details: [milestones/v0.2-ROADMAP.md](milestones/v0.2-ROADMAP.md)

</details>

<details>
<summary>v0.3.0 Zero-Copy Query Pipeline -- SHIPPED 2026-03-03</summary>

Replaced binary-read dispatch with zero-copy vector references (`duckdb_vector_reference_vector`).
Eliminated ~600 LOC of per-type read/write code. Type mismatches handled at SQL generation time
via `build_execution_sql` cast wrapper. Streaming chunk-by-chunk instead of collect-all-then-write.

</details>

<details>
<summary>v0.4.0 Simplified Dimensions -- SHIPPED 2026-03-03</summary>

Breaking change: removed `time_dimensions` DDL parameter and `granularities` query parameter.
Time truncation expressed via dimension `expr` directly (e.g., `date_trunc('month', created_at)`).
DDL simplified from 6 to 4 named params; query function from 3 to 2 named params.

</details>

<details>
<summary>v0.5.0 Parser Extension Spike (Phases 15-18) -- SHIPPED 2026-03-08</summary>

- [x] Phase 15: Entry Point POC (2/2 plans) -- completed 2026-03-07
- [x] Phase 16: Parser Hook Registration (1/1 plan) -- completed 2026-03-07
- [x] Phase 17: DDL Execution (1/1 plan) -- completed 2026-03-07
- [x] Phase 17.1: Python vtab crash investigation (2/2 plans) -- completed 2026-03-08
- [x] Phase 18: Verification and Integration (2/2 plans) -- completed 2026-03-08

Full details: [milestones/v0.5-ROADMAP.md](milestones/v0.5-ROADMAP.md)

</details>

<details>
<summary>v0.5.1 DDL Polish (Phases 19-23) -- SHIPPED 2026-03-09</summary>

- [x] Phase 19: Parser Hook Validation Spike (1/1 plan) -- completed 2026-03-09
- [x] Phase 20: Extended DDL Statements (2/2 plans) -- completed 2026-03-09
- [x] Phase 21: Error Location Reporting (3/3 plans) -- completed 2026-03-09
- [x] Phase 22: Documentation (1/1 plan) -- completed 2026-03-09
- [x] Phase 23: Parser Proptests and Caret Integration Tests (2/2 plans) -- completed 2026-03-09

Full details: [milestones/v0.5.1-ROADMAP.md](milestones/v0.5.1-ROADMAP.md)

</details>

<details>
<summary>v0.5.2 SQL DDL & PK/FK Relationships (Phases 24-28) -- SHIPPED 2026-03-13</summary>

- ~~Phase 24: PK/FK Model~~ -- CANCELLED (absorbed into Phase 25-01)
- [x] Phase 25: SQL Body Parser (4/4 plans) -- completed 2026-03-12
- [x] Phase 25.1: Parser Robustness & Security (2/2 plans) -- completed 2026-03-13
- [x] Phase 26: PK/FK Join Resolution (2/2 plans) -- completed 2026-03-13
- [x] Phase 27: Alias-Based Query Expansion (3/3 plans) -- completed 2026-03-13
- [x] Phase 28: Integration Testing & Docs (3/3 plans) -- completed 2026-03-13

Full details: [milestones/v0.5.2-ROADMAP.md](milestones/v0.5.2-ROADMAP.md)

</details>

### v0.5.3 Advanced Semantic Features (In Progress)

**Milestone Goal:** Add advanced semantic modeling capabilities -- FACTS clause, derived metrics, hierarchies, fan trap detection, role-playing dimensions, and multiple join paths (USING RELATIONSHIPS).

- [x] **Phase 29: FACTS Clause & Hierarchies** - Named row-level sub-expressions and drill-down path metadata (completed 2026-03-14)
- [ ] **Phase 30: Derived Metrics** - Metric-on-metric composition with DAG resolution
- [ ] **Phase 31: Fan Trap Detection** - Structural correctness warnings for one-to-many aggregation
- [ ] **Phase 32: Role-Playing Dimensions & USING RELATIONSHIPS** - Same table via multiple join paths with explicit path selection

## Phase Details

### Phase 29: FACTS Clause & Hierarchies
**Goal**: Users can declare reusable row-level sub-expressions (facts) and drill-down paths (hierarchies) within semantic views
**Depends on**: Phase 28 (v0.5.2 complete)
**Requirements**: FACT-01, FACT-02, FACT-03, FACT-04, FACT-05, HIER-01, HIER-02, HIER-03
**Success Criteria** (what must be TRUE):
  1. User can declare a FACTS clause with named row-level expressions and reference them in metric expressions, producing correct query results
  2. Facts that reference other facts resolve correctly through multi-level inlining with proper parenthesization (operator precedence preserved)
  3. Defining a semantic view with fact cycles or references to non-existent facts produces a clear error at define time
  4. User can declare a HIERARCHIES clause with drill-down paths, and DESCRIBE SEMANTIC VIEW shows both facts and hierarchies alongside dimensions and metrics
  5. Defining a semantic view with a hierarchy referencing a non-existent dimension produces a clear error at define time
  6. Unit tests for fact parsing, fact inlining, hierarchy validation; proptests for FACTS/HIERARCHIES clause parsing with adversarial input; sqllogictest for end-to-end FACTS+HIERARCHIES DDL and query; fuzz target for FACTS clause parsing
  7. `just test-all` passes
**Plans**: 2 plans

Plans:
- [x] 29-01-PLAN.md -- Parse FACTS/HIERARCHIES clauses, model wiring, and define-time validation
- [ ] 29-02-PLAN.md -- Fact inlining in expansion, DESCRIBE output updates, end-to-end tests

### Phase 30: Derived Metrics
**Goal**: Users can compose metrics from other metrics without writing raw aggregate expressions
**Depends on**: Phase 29 (facts must be available for derived metrics to reference)
**Requirements**: DRV-01, DRV-02, DRV-03, DRV-04, DRV-05
**Success Criteria** (what must be TRUE):
  1. User can declare a derived metric without a table prefix (e.g., `profit AS revenue - cost`) and query it alongside regular metrics, producing correct results
  2. Derived metrics that reference other derived metrics (stacking) resolve correctly through multi-level inlining with word-boundary-safe substitution
  3. Defining a semantic view with derived metric cycles, references to non-existent metrics, or aggregation functions inside a derived metric produces a clear error at define time
  4. Unit tests for metric DAG construction, cycle detection, expression inlining with word-boundary safety; proptests for derived metric expression substitution edge cases; sqllogictest for end-to-end derived metrics with stacking and error cases
  5. `just test-all` passes
**Plans**: TBD

### Phase 31: Fan Trap Detection
**Goal**: Users receive warnings when query structure risks inflating aggregation results due to one-to-many fan-out
**Depends on**: Phase 29 (needs relationship model, no dependency on derived metrics)
**Requirements**: FAN-01, FAN-02, FAN-03
**Success Criteria** (what must be TRUE):
  1. User can optionally declare cardinality type (one_to_one, one_to_many, many_to_one) on relationships in the DDL
  2. When a query aggregates a metric across a one-to-many boundary, expansion produces a warning message describing the fan trap risk
  3. Fan trap warnings do not block query execution -- the query succeeds and returns results alongside the warning
  4. Unit tests for cardinality parsing, fan trap graph analysis, warning generation; proptests for cardinality clause parsing; sqllogictest for end-to-end fan trap warning scenarios (with and without cardinality declarations)
  5. `just test-all` passes
**Plans**: TBD

### Phase 32: Role-Playing Dimensions & USING RELATIONSHIPS
**Goal**: Users can join the same physical table via multiple named relationships and select specific join paths per metric
**Depends on**: Phase 31 (all simpler features stable before relaxing graph invariant)
**Requirements**: JOIN-01, JOIN-02, JOIN-03, JOIN-04, JOIN-05, ROLE-01, ROLE-02, ROLE-03
**Success Criteria** (what must be TRUE):
  1. User can declare multiple named relationships between the same table pair without triggering diamond rejection
  2. Metrics with `USING (relationship_name)` expand with the correct join path and relationship-scoped aliases in generated SQL
  3. Querying a dimension from an ambiguous multi-path table without USING produces a clear error explaining which relationships are available
  4. Classic role-playing pattern works end-to-end (e.g., flights with departure_airport and arrival_airport joining the same airports table via different relationships)
  5. Define-time validation rejects USING references to non-existent relationships
  6. Unit tests for diamond relaxation logic, USING parsing, relationship-scoped alias generation, ambiguity detection; proptests for USING clause parsing; sqllogictest for end-to-end role-playing pattern (flights/airports) and error cases; fuzz target for USING clause parsing
  7. `just test-all` passes
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 29 -> 30 -> 31 -> 32

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v0.1.0 | 3/3 | Complete | 2026-02-24 |
| 2. Storage and DDL | v0.1.0 | 4/4 | Complete | 2026-02-24 |
| 3. Expansion Engine | v0.1.0 | 3/3 | Complete | 2026-02-25 |
| 4. Query Interface | v0.1.0 | 3/3 | Complete | 2026-02-25 |
| 5. Hardening and Docs | v0.1.0 | 2/2 | Complete | 2026-02-26 |
| 6. Tech Debt Code Cleanup | v0.1.0 | 1/1 | Complete | 2026-02-26 |
| 7. Verification & Formal Closure | v0.1.0 | 2/2 | Complete | 2026-02-27 |
| 8. C++ Shim Infrastructure | v0.2.0 | 2/2 | Complete | 2026-03-01 |
| 9. Time Dimensions | v0.2.0 | 2/2 | Complete | 2026-03-01 |
| 10. pragma_query_t Catalog Persistence | v0.2.0 | 3/3 | Complete | 2026-03-01 |
| 11. Scalar Function DDL | v0.2.0 | 4/4 | Complete | 2026-03-02 |
| 11.1. Snowflake-aligned DDL Syntax | v0.2.0 | 5/5 | Complete | 2026-03-02 |
| 12. EXPLAIN + Typed Output | v0.2.0 | 4/4 | Complete | 2026-03-02 |
| 13. Type-mapping + PBTs | v0.2.0 | 2/2 | Complete | 2026-03-02 |
| 14. DuckLake Integration + CI | v0.2.0 | 3/3 | Complete | 2026-03-02 |
| Zero-Copy Query Pipeline | v0.3.0 | -- | Complete | 2026-03-03 |
| Simplified Dimensions | v0.4.0 | -- | Complete | 2026-03-03 |
| 15. Entry Point POC | v0.5.0 | 2/2 | Complete | 2026-03-07 |
| 16. Parser Hook Registration | v0.5.0 | 1/1 | Complete | 2026-03-07 |
| 17. DDL Execution | v0.5.0 | 1/1 | Complete | 2026-03-07 |
| 17.1. Python vtab crash investigation | v0.5.0 | 2/2 | Complete | 2026-03-08 |
| 18. Verification and Integration | v0.5.0 | 2/2 | Complete | 2026-03-08 |
| 19. Parser Hook Validation Spike | v0.5.1 | 1/1 | Complete | 2026-03-09 |
| 20. Extended DDL Statements | v0.5.1 | 2/2 | Complete | 2026-03-09 |
| 21. Error Location Reporting | v0.5.1 | 3/3 | Complete | 2026-03-09 |
| 22. Documentation | v0.5.1 | 1/1 | Complete | 2026-03-09 |
| 23. Parser Proptests + Caret Tests | v0.5.1 | 2/2 | Complete | 2026-03-09 |
| 24. PK/FK Model | v0.5.2 | -- | Cancelled | - |
| 25. SQL Body Parser | v0.5.2 | 4/4 | Complete | 2026-03-12 |
| 25.1. Parser Robustness & Security | v0.5.2 | 2/2 | Complete | 2026-03-13 |
| 26. PK/FK Join Resolution | v0.5.2 | 2/2 | Complete | 2026-03-13 |
| 27. Alias-Based Query Expansion | v0.5.2 | 3/3 | Complete | 2026-03-13 |
| 28. Integration Testing & Docs | v0.5.2 | 3/3 | Complete | 2026-03-13 |
| 29. FACTS Clause & Hierarchies | 2/2 | Complete   | 2026-03-14 | - |
| 30. Derived Metrics | v0.5.3 | 0/? | Not started | - |
| 31. Fan Trap Detection | v0.5.3 | 0/? | Not started | - |
| 32. Role-Playing & USING | v0.5.3 | 0/? | Not started | - |
