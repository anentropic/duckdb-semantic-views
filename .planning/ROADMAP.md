# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** -- Phases 1-7 (shipped 2026-02-28)
- ✅ **v0.2.0 Native DDL + Time Dimensions** -- Phases 8-14 (shipped 2026-03-03)
- ✅ **v0.3.0 Zero-Copy Query Pipeline** -- (shipped 2026-03-03)
- ✅ **v0.4.0 Simplified Dimensions** -- (shipped 2026-03-03)
- ✅ **v0.5.0 Parser Extension Spike** -- Phases 15-18 (shipped 2026-03-08)
- ✅ **v0.5.1 DDL Polish** -- Phases 19-23 (shipped 2026-03-09)
- ✅ **v0.5.2 SQL DDL & PK/FK Relationships** -- Phases 24-28 (shipped 2026-03-13)
- ✅ **v0.5.3 Advanced Semantic Features** -- Phases 29-32 (shipped 2026-03-15)
- ✅ **v0.5.4 Snowflake-Parity & Registry Publishing** -- Phases 33-36 (shipped 2026-03-27)
- ✅ **v0.5.5 SHOW/DESCRIBE Alignment & Refactoring** -- Phases 37-42 (shipped 2026-04-05)
- ✅ **v0.6.0 Snowflake SQL DDL Parity** -- Phases 43-50 (shipped 2026-04-14)

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

Full details: [milestones/v1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

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

<details>
<summary>v0.5.3 Advanced Semantic Features (Phases 29-32) -- SHIPPED 2026-03-15</summary>

- [x] Phase 29: FACTS Clause & Hierarchies (2/2 plans) -- completed 2026-03-14
- [x] Phase 30: Derived Metrics (2/2 plans) -- completed 2026-03-14
- [x] Phase 31: Fan Trap Detection (2/2 plans) -- completed 2026-03-14
- [x] Phase 32: Role-Playing Dimensions & USING RELATIONSHIPS (2/2 plans) -- completed 2026-03-14

Full details: [milestones/v0.5.3-ROADMAP.md](milestones/v0.5.3-ROADMAP.md)

</details>

<details>
<summary>v0.5.4 Snowflake-Parity & Registry Publishing (Phases 33-36) -- SHIPPED 2026-03-27</summary>

- [x] Phase 33: UNIQUE Constraints & Cardinality Inference (2/2 plans) -- completed 2026-03-15
- [x] Phase 34: DuckDB 1.5 Upgrade & LTS Branch (2/2 plans) -- completed 2026-03-16
- [x] Phase 34.1: DDL Surface Parity (3/3 plans) -- completed 2026-03-22
- [x] Phase 34.1.1: SHOW Command Filtering (1/1 plan) -- completed 2026-03-22
- [x] Phase 35: Documentation Site (1/1 plan) -- completed 2026-03-27
- [x] Phase 36: Registry Publishing & Maintainer Docs (2/2 plans) -- completed 2026-03-27

Full details: [milestones/v0.5.4-ROADMAP.md](milestones/v0.5.4-ROADMAP.md)

</details>

<details>
<summary>v0.5.5 SHOW/DESCRIBE Alignment & Refactoring (Phases 37-42) -- SHIPPED 2026-04-05</summary>

- [x] Phase 37: Extract Shared Utilities (1/1 plan) -- completed 2026-04-01
- [x] Phase 38: Module Directory Splits (2/2 plans) -- completed 2026-04-01
- [x] Phase 39: Metadata Storage (1/1 plan) -- completed 2026-04-02
- [x] Phase 40: SHOW Command Alignment (2/2 plans) -- completed 2026-04-02
- [x] Phase 41: DESCRIBE Rewrite (2/2 plans) -- completed 2026-04-02
- [x] Phase 42: Refactor & Test Reorg (3/3 plans) -- completed 2026-04-05

Full details: [milestones/v0.5.5-ROADMAP.md](milestones/v0.5.5-ROADMAP.md)

</details>

<details>
<summary>v0.6.0 Snowflake SQL DDL Parity (Phases 43-50) -- SHIPPED 2026-04-14</summary>

- [x] Phase 43: Metadata Foundation (2/2 plans) -- completed 2026-04-10
- [x] Phase 44: SHOW/DESCRIBE Metadata Surface + Enhancements (2/2 plans) -- completed 2026-04-11
- [x] Phase 45: ALTER COMMENT + GET_DDL (2/2 plans) -- completed 2026-04-11
- [x] Phase 46: Wildcard Selection + Queryable FACTS (2/2 plans) -- completed 2026-04-12
- [x] Phase 47: Semi-Additive Metrics (2/2 plans) -- completed 2026-04-12
- [x] Phase 48: Window Function Metrics (2/2 plans) -- completed 2026-04-12
- [x] Phase 49: Security & Correctness Hardening (2/2 plans) -- completed 2026-04-14
- [x] Phase 50: Code Quality & Test Coverage (2/2 plans) -- completed 2026-04-14

Full details: [milestones/v0.6.0-ROADMAP.md](milestones/v0.6.0-ROADMAP.md)

</details>

### v0.7.0 YAML Definitions & Materialization Routing (In Progress)

**Milestone Goal:** Add YAML as a second definition format alongside SQL DDL, and a materialization routing engine that transparently redirects queries to pre-existing aggregated tables when they cover the requested dimensions and metrics.

- [x] **Phase 51: YAML Parser Core** (1 plan) - yaml_serde dependency, YAML-to-SemanticViewDefinition conversion, shared validation, size cap (completed 2026-04-18)
- [x] **Phase 52: YAML DDL Integration** (1 plan) - FROM YAML $$ dollar-quoting, parser hook detection, CREATE/REPLACE/IF NOT EXISTS modifiers (completed 2026-04-18)
- [x] **Phase 53: YAML File Loading** - FROM YAML FILE with DuckDB file abstraction and enable_external_access security (completed 2026-04-19)
- [x] **Phase 54: Materialization Model & DDL** (1 plan) - Materialization struct, MATERIALIZATIONS clause in body parser, persistence, YAML support (completed 2026-04-19)
- [ ] **Phase 55: Materialization Routing Engine** - Query-time routing with exact-match set containment, fallback, semi-additive/window exclusion
- [ ] **Phase 56: YAML Export** - READ_YAML_FROM_SEMANTIC_VIEW scalar function, round-trip including materializations
- [ ] **Phase 57: Introspection & Diagnostics** - explain_semantic_view routing info, DESCRIBE materializations, SHOW SEMANTIC MATERIALIZATIONS

## Phase Details

### Phase 51: YAML Parser Core
**Goal**: Users can parse YAML definitions into identical internal representations as SQL DDL
**Depends on**: Nothing (first phase of v0.7.0)
**Requirements**: YAML-03, YAML-05, YAML-09
**Success Criteria** (what must be TRUE):
  1. A YAML string containing tables, relationships, dimensions, metrics, facts, and metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC) deserializes into the same SemanticViewDefinition as equivalent SQL DDL
  2. A YAML string exceeding the size cap (1MB) is rejected with a clear error before parsing begins
  3. The same define-time validation (graph validation, expression checks, DAG resolution) runs identically for YAML-originated and SQL-originated definitions
**Plans:** 1/1 plans complete
Plans:
- [x] 51-01-PLAN.md -- yaml_serde dependency, PartialEq derives, from_yaml/from_yaml_with_size_cap, comprehensive test suite

### Phase 52: YAML DDL Integration
**Goal**: Users can create semantic views from inline YAML via native DDL
**Depends on**: Phase 51
**Requirements**: YAML-01, YAML-06
**Success Criteria** (what must be TRUE):
  1. `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$` creates a semantic view that is queryable via `semantic_view()`
  2. `CREATE OR REPLACE SEMANTIC VIEW name FROM YAML $$ ... $$` replaces an existing view
  3. `CREATE SEMANTIC VIEW IF NOT EXISTS name FROM YAML $$ ... $$` is a no-op when the view already exists
  4. The parser hook correctly detects `FROM YAML` and routes through the YAML parsing path
**Plans:** 1/1 plans complete
Plans:
- [x] 52-01-PLAN.md -- dollar-quote extraction, YAML-to-JSON rewrite, FROM YAML detection, unit tests, sqllogictest integration tests

### Phase 53: YAML File Loading
**Goal**: Users can create semantic views from external YAML files with proper security boundaries
**Depends on**: Phase 52
**Requirements**: YAML-02, YAML-07
**Success Criteria** (what must be TRUE):
  1. `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'` creates a semantic view from the file contents
  2. When `SET enable_external_access = false`, `FROM YAML FILE` is rejected with a security error
  3. File loading uses DuckDB's file abstraction (read_text), not direct filesystem access
**Plans:** 1/1 plans complete
Plans:
- [x] 53-01-PLAN.md -- FROM YAML FILE detection, C++ file reading, integration tests

### Phase 54: Materialization Model & DDL
**Goal**: Users can declare materializations as part of a semantic view definition
**Depends on**: Phase 51 (YAML support for MAT-06 requires parser core)
**Requirements**: MAT-01, MAT-06, MAT-07
**Success Criteria** (what must be TRUE):
  1. `MATERIALIZATIONS` clause in SQL DDL accepts named materializations with TABLE, DIMENSIONS, and METRICS sub-clauses
  2. `MATERIALIZATIONS` section in YAML definitions produces the same internal representation as the SQL DDL clause
  3. Materialization metadata persists across DuckDB restarts (stored and loaded with backward compatibility for pre-v0.7.0 views)
  4. Define-time validation ensures materialization dimensions and metrics reference declared names in the semantic view
**Plans:** 1/1 plans complete
Plans:
- [x] 54-01-PLAN.md -- Materialization struct, body parser clause, parse.rs wiring, render_ddl reconstruction, define-time validation, YAML support, sqllogictest integration tests

### Phase 55: Materialization Routing Engine
**Goal**: Queries are transparently routed to pre-existing aggregated tables when materializations cover the request
**Depends on**: Phase 54
**Requirements**: MAT-02, MAT-03, MAT-04, MAT-05
**Success Criteria** (what must be TRUE):
  1. When a materialization exactly covers the requested dimensions and metrics, the query reads from the materialization table instead of expanding raw sources
  2. When no materialization matches, the query falls back to raw table expansion with no error and no observable behavior change
  3. Queries involving semi-additive metrics (NON ADDITIVE BY) or window function metrics (PARTITION BY EXCLUDING) always fall back to raw expansion regardless of materialization coverage
  4. A user who has not declared any materializations sees zero behavior change from this feature
**Plans**: TBD

### Phase 56: YAML Export
**Goal**: Users can export stored semantic views as YAML for version control and round-trip workflows
**Depends on**: Phase 54 (materializations must be in model for export to include them)
**Requirements**: YAML-04, YAML-08
**Success Criteria** (what must be TRUE):
  1. `SELECT READ_YAML_FROM_SEMANTIC_VIEW('name')` returns a YAML string representing the stored definition, including materializations if declared
  2. The exported YAML can be fed back into `CREATE SEMANTIC VIEW ... FROM YAML $$ ... $$` to recreate an identical semantic view (lossless round-trip)
  3. Fully qualified names (database.schema.view_name) are supported in the function argument
**Plans**: TBD

### Phase 57: Introspection & Diagnostics
**Goal**: Users can inspect materialization routing decisions and materialization metadata through existing introspection commands
**Depends on**: Phase 55
**Requirements**: INTR-01, INTR-02, INTR-03
**Success Criteria** (what must be TRUE):
  1. `explain_semantic_view()` output includes which materialization was selected (or "none") and the expanded SQL reflects the routed table
  2. `DESCRIBE SEMANTIC VIEW` includes materialization entries showing each materialization's name, table, covered dimensions, and covered metrics
  3. `SHOW SEMANTIC MATERIALIZATIONS IN view_name` lists all declared materializations with their covered dimensions and metrics
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 51 -> 52 -> 53 -> 54 -> 55 -> 56 -> 57

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 51. YAML Parser Core | v0.7.0 | 1/1 | Complete    | 2026-04-18 |
| 52. YAML DDL Integration | v0.7.0 | 1/1 | Complete    | 2026-04-18 |
| 53. YAML File Loading | v0.7.0 | 1/1 | Complete    | 2026-04-19 |
| 54. Materialization Model & DDL | v0.7.0 | 1/1 | Complete    | 2026-04-19 |
| 55. Materialization Routing Engine | v0.7.0 | 0/0 | Not started | - |
| 56. YAML Export | v0.7.0 | 0/0 | Not started | - |
| 57. Introspection & Diagnostics | v0.7.0 | 0/0 | Not started | - |
