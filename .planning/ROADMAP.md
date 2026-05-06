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
- ✅ **v0.7.0 YAML Definitions & Materialization Routing** -- Phases 51-57 (shipped 2026-04-24)
- 🛠 **v0.8.0 Transactional DDL & Architectural Unification** -- Phases 58-62 (in progress; phases 58-61 retroactively GSD'd 2026-05-05)

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

<details>
<summary>v0.7.0 YAML Definitions & Materialization Routing (Phases 51-57) -- SHIPPED 2026-04-24</summary>

- [x] Phase 51: YAML Parser Core (1/1 plans) -- completed 2026-04-18
- [x] Phase 52: YAML DDL Integration (1/1 plans) -- completed 2026-04-18
- [x] Phase 53: YAML File Loading (1/1 plans) -- completed 2026-04-19
- [x] Phase 54: Materialization Model & DDL (1/1 plans) -- completed 2026-04-19
- [x] Phase 55: Materialization Routing Engine (1/1 plans) -- completed 2026-04-19
- [x] Phase 56: YAML Export (1/1 plans) -- completed 2026-04-20
- [x] Phase 57: Introspection & Diagnostics (1/1 plans) -- completed 2026-04-21

Full details: [milestones/v0.7.0-ROADMAP.md](milestones/v0.7.0-ROADMAP.md)

</details>

<details>
<summary>v0.8.0 Transactional DDL & Architectural Unification (Phases 58-62) -- IN PROGRESS</summary>

Phases 58–61 are retroactive reconstructions: the work was originally completed ad-hoc on `milestone/v0.8.0` (PR #28) and a premature `milestone/v0.8.1` branch, then consolidated into a single v0.8.0 milestone on 2026-05-05 (no git tag was ever issued). PLAN.md and VERIFICATION.md for these phases are back-derived from the actual commits, CHANGELOG, and test artefacts; they do not include RESEARCH.md / DISCUSS.md (those records don't exist for ad-hoc work).

- [x] Phase 58: Transactional DDL via parser_override (1/1 plans) -- completed 2026-05-02 (retroactive)

  Goal: CREATE (all four forms), DROP, ALTER SEMANTIC VIEW participate in caller's transaction. parser_override extension hook rewrites recognised DDL into native INSERT/UPDATE/DELETE against `_definitions` and re-parses on the caller's connection. CatalogState HashMap removed; single CatalogReader path.

- [x] Phase 59: Architectural unification (1/1 plans) -- completed 2026-05-03 (retroactive)

  Goal: parser_override becomes the sole DDL entry. Legacy parse_function / sv_ddl_internal table-function fallback retired (~1500 LOC net deletion). Single execution path → uniform transactional semantics + uniform PEG/Bison compatibility. Introduces a known regression — caret rendering lost for validation errors (TECH-DEBT 22) — fixed in Phase 62.

- [x] Phase 60: Race guards & validation hardening (1/1 plans) -- completed 2026-05-03 (retroactive)

  Goal: Non-`IF EXISTS` DROP/ALTER emits snapshot-consistent existence check; FFI UTF-8 input validated (no UB on malformed bytes); `parse_table_function_call` rejects malformed argument lists; `ParserOptions` size pinned by static assert.

- [x] Phase 61: Bounded multi-DB isolation, RAII, tests & docs (1/1 plans) -- completed 2026-05-03 (retroactive)

  Goal: per-DB token→catalog map capped at 16 entries with insertion-order eviction (TECH-DEBT 20 — known limitation, redesigned in Phase 62); `CatalogReader` adopts RAII guards (`PreparedStmt`, `QueryResult`); ADBC end-to-end test, concurrent-CREATE Python test, `INSERT OR REPLACE` row-count + byte-identical rollback sqllogictest, type-inference inside transaction, FFI fuzz target; CHANGELOG, TECH-DEBT, MAINTAINER updates.

- [ ] Phase 62: Caret restoration + LRU removal (planned)

  Goal: Re-introduce `parse_function` purely as the error-reporting layer (parser_override keeps the success/transactional path). Defer error cases from parser_override → default parser fails → parse_function returns `DISPLAY_EXTENSION_ERROR` with `error_location`, restoring `LINE 1: … ^` caret rendering. Concurrently, attach the `CatalogReader` directly to `SemanticViewsParserInfo` (lifetime tied to `DBConfig`), eliminating the bounded LRU and its silent-eviction error class. Resolves TECH-DEBT items 20 + 22.

  **Pre-planning artefacts** (read before `/gsd-plan-phase 62`):
  - `_notes/v0.8.0_phase_62_ultraplan.md` — full architectural design (mermaid flow + per-file changes + reused-helpers + verification list). Contains 4 open research questions for the research subagent to resolve.
  - `_notes/v0.8.0_phase_62_sqllogictest_spike.md` — test blast-radius scoping. Concludes SMALL: zero prefix-bound matchers in 96 `statement error` blocks; ~100 LOC of test changes total.

</details>
