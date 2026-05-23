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
- ✅ **v0.8.0 Transactional DDL & Architectural Unification** -- Phases 58-62 (shipped 2026-05-06)
- ✅ **v0.9.0 Read-Only Database LOAD Support + Quoted Identifier Bugfix** -- Phases 63-64 (shipped 2026-05-17)
- 🚧 **v0.9.1 Connection-Lifecycle & Catalog-Context Fixes** -- Phases 65-66 (in progress)

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
<summary>✅ v0.8.0 Transactional DDL & Architectural Unification (Phases 58-62) -- SHIPPED 2026-05-06</summary>

- [x] Phase 58: Transactional DDL via parser_override (1/1 plans) -- completed 2026-05-02 (retroactive)
- [x] Phase 59: Architectural unification (1/1 plans) -- completed 2026-05-03 (retroactive)
- [x] Phase 60: Race guards & validation hardening (1/1 plans) -- completed 2026-05-03 (retroactive)
- [x] Phase 61: Bounded multi-DB isolation, RAII, tests & docs (1/1 plans) -- completed 2026-05-03 (retroactive)
- [x] Phase 62: Caret restoration + LRU removal (4/4 plans) -- completed 2026-05-06

Phases 58–61 are retroactive reconstructions: the work was originally completed ad-hoc on `milestone/v0.8.0` (PR #28) and a premature `milestone/v0.8.1` branch, then consolidated into a single v0.8.0 milestone on 2026-05-05 (no v0.8.1 tag was ever issued).

Full details: [milestones/v0.8.0-ROADMAP.md](milestones/v0.8.0-ROADMAP.md)

</details>

<details>
<summary>✅ v0.9.0 Read-Only Database LOAD Support + Quoted Identifier Bugfix (Phases 63-64) -- SHIPPED 2026-05-17</summary>

- [x] Phase 63: Read-Only Database LOAD Support (4/4 plans) -- completed 2026-05-17
- [x] Phase 64: Fix CREATE SEMANTIC VIEW quoted identifier handling (4/4 plans) -- completed 2026-05-17

Full details: [milestones/v0.9.0-ROADMAP.md](milestones/v0.9.0-ROADMAP.md)

</details>

<details open>
<summary>🚧 v0.9.1 Connection-Lifecycle & Catalog-Context Fixes (Phases 65-66) -- IN PROGRESS</summary>

- [ ] Phase 65: OverrideContext Connection Teardown (1/7 plans; Plans 02-07 created 2026-05-23 under B-prime architecture; scope folded in 14+2 read-side functions per REQUIREMENTS.md escape clause)
- [ ] Phase 66: Expansion Qualification Across All Paths + ADBC Tests (0/? plans) -- scope to be revisited after Phase 65 lands (likely shrinks to REL-only since B-prime eliminates the H2 catalog-search-path divergence root cause; do not pre-commit shape until empirically verified)

### Phase Details

### Phase 65: OverrideContext Connection Teardown (B-prime architecture)
**Goal**: Eliminate ALL long-lived extension-owned `duckdb_connection` handles (H1 catalog_conn AND H2 query_conn). Every catalog read and DDL emission runs on a per-call C++ `Connection(*context.db)` opened from the caller's live `ClientContext`, wrapped in `ConnGuard` for scope-bounded teardown. After Phase 65, ZERO `duckdb_connection` survives at extension-LOAD scope.
**Depends on**: Nothing (first phase of v0.9.1)
**Requirements**: LIFE-01, LIFE-02, LIFE-03, LIFE-04 (read-side fold-in activates the REQUIREMENTS.md "If [BindInfo exposure] becomes possible mid-v0.9.1, fold into scope" clause; the C++ Catalog API registration is the unblocking mechanism, proven by 65-READ-PATH-SPIKE.md)
**Success Criteria** (what must be TRUE):
  1. In the same Python process, after a writable connection that did `LOAD semantic_views` and `CREATE SEMANTIC VIEW` is closed, a subsequent `duckdb.connect(path, read_only=True)` against the same path returns within 5 seconds — verified both on a freshly bootstrapped DB and on a previously bootstrapped DB (LIFE-01).
  2. The chosen mechanism (B-prime: per-call C++ `Connection(*context.db)` from every callback that has `ClientContext &`) is documented with reasoning in the phase's RESEARCH.md, with the empirical evidence trail (65-02-SPIKES.md A2-DEADLOCK + BIND-THREAD-RC1, 65-OPTION-B-SPIKE.md PLAN-THREAD-RC0, 65-READ-PATH-SPIKE.md READ-BIND-RC0) preserved as the trade-off record.
  3. `test/integration/test_readonly_load.py` includes a new `test_in_process_bootstrap_then_readonly` scenario plus read-side variants that exercise SELECT through semantic_view() and list/describe/show after close; all guarded by watchdog; all fail on v0.9.0 baseline and pass on v0.9.1.
  4. `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` is updated in place with the resolution and a forward pointer to v0.9.1.
  5. v0.8.0 transactional DDL semantics preserved byte-identical (CREATE inside user BEGIN/COMMIT still participates in the transaction); existing Phase 58 ADBC transactional tests stay green.
**Plans**: 7 plans
  - [x] 65-01-PLAN.md — Wave-0 spikes + ConnGuard RAII + watchdog test scaffolding (B1-B4, B11) — SHIPPED
  - [ ] 65-02-PLAN.md — Revert Plan 02 partial commits + add C++ sv_register_table_function helper (infrastructure only, no production wiring)
  - [ ] 65-03-PLAN.md — Deregister sv_parser_override; promote sv_parse_function + sv_plan_function (per-call C++ Connection probe for catalog reads); retire H1 catalog_conn; preserve transactional DDL byte-identical
  - [ ] 65-04-PLAN.md — Port 7 read-side functions (list/show_columns/show_dims/show_dims_for_metric/show_metrics/show_facts incl _all variants) to C++ Catalog API
  - [ ] 65-05-PLAN.md — Port remaining 6+2 functions (describe/show_materializations/scalars/semantic_view/explain) + add sv_register_scalar_function helper; retire H2 query_conn
  - [ ] 65-06-PLAN.md — Retire TEMP-PLAN-04 catalog_conn_temp; structural PHASE-65-GUARD Rust unit test; LIFE-04 ledger close-out; TECH-DEBT 25 + 26 filed; just test-all green
  - [ ] 65-07-PLAN.md — Close-out: dead VTab/VScalar impl deletion + Plan 01 allow-attribute cleanup; just ci regression evidence; phase-level SUMMARY.md

### Phase 66: Expansion Qualification Across All Paths + ADBC Tests
**Goal**: Make `FROM semantic_view(...)` work through ADBC and any other client whose catalog/schema search path diverges from the extension's `query_conn`, by emitting fully-qualified `db.schema.table` references from every expansion site — and ship the milestone (CHANGELOG, version bump, CI green).
**Depends on**: Phase 65
**Requirements**: EXPAND-CTX-01, EXPAND-CTX-02, EXPAND-CTX-03, REL-01, REL-02
**Success Criteria** (what must be TRUE):
  1. Through `adbc_driver_duckdb`, `SELECT … FROM semantic_view(...)` returns rows (not a `Catalog Error: Table with name X does not exist`) against semantic views that exercise the main expansion path, FACTS, semi-additive metrics, window metrics, and a multi-database `ATTACH` scenario.
  2. A new `test/integration/test_adbc_queries.py` (runnable via `just test-adbc-queries`) covers those five scenarios end-to-end; it fails on the v0.9.0 baseline and passes on v0.9.1, serving as the regression guard.
  3. `_notes/error_with_adbc.md` is either removed or updated to point at the v0.9.1 fix, so the downstream report no longer reads as an open upstream bug.
  4. A user who installs v0.9.1 sees a `## [0.9.1]` section in `CHANGELOG.md` describing both fixes under `### Fixed`, with the `[Unreleased]` block reset and compare links updated; `Cargo.toml` and `description.yml` report `0.9.1`; `just test-all` and `just ci` are green on `milestone/v0.9.1`.
**Plans**: TBD

</details>
