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
- 🚧 **v0.10.0 Connection-Lifecycle & Catalog-Context Fixes** -- Phases 65-66 (in progress; originally scoped as v0.9.1 patch, reframed 2026-05-23 after B-prime architecture eliminated by EXEC-TIME-RC1 spike — read-elimination architecture replaces it)

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
<summary>🚧 v0.10.0 Connection-Lifecycle & Catalog-Context Fixes (Phases 65-66) -- IN PROGRESS</summary>

**Originally scoped as v0.9.1 patch milestone.** Reframed to v0.10.0 on 2026-05-23 after the B-prime architecture for Phase 65 was empirically eliminated by `65-EXEC-TIME-SPIKE.md` (EXEC-TIME-RC1). The follow-on `65-ALTER-REWRITE-SPIKE.md` (ALTER-RC0) validated a different architectural premise: preserve `parser_override` (only DuckDB v1.5.2 mechanism that delivers transactional DDL) and eliminate the catalog reads inside it. See `.planning/phases/65-overridecontext-connection-teardown/65-BPRIME-ARCHIVE-NOTE.md` for the full pivot rationale.

- [ ] Phase 65: OverrideContext Connection Teardown — EXECUTING under read-elimination architecture (3/6 plans complete: 65-01 ConnGuard + watchdog [carried], 65-02 sv_register_table_function shim partial [reverted by 65-03], 65-03 parser_override slimming wave [parser_override CREATE path zero catalog reads; conn_guard deleted; resolve_pk_from_catalog deleted; metadata-via-SQL via json_merge_patch on caller's conn; D-06 hard error]. Plans 65-04 ALTER architecture, 65-05 read-path migration, 65-06 lifecycle close-out remaining.)
- [ ] Phase 66: Expansion Qualification Across All Paths + ADBC Tests (0/? plans) -- scope to be revisited after Phase 65 lands. The H2 catalog-search-path divergence root cause likely dissolves once `query_conn` is retired by the read-elimination architecture; final scope pending Phase 65 plan shape.

### Phase Details

### Phase 65: OverrideContext Connection Teardown (read-elimination architecture)
**Goal**: Retire both long-lived extension-owned `duckdb_connection` handles (H1 catalog_conn AND H2 query_conn) so the in-process RW→RO reopen hang resolves, while preserving v0.8.0 transactional DDL semantics byte-identical. Achieved by eliminating the catalog reads inside `parser_override` rather than porting `parser_override` to a different callback shape, plus migrating read-path callbacks to the C++ Catalog API registration shim so they gain ClientContext for per-call Connection.
**Depends on**: Nothing (first phase of v0.10.0)
**Requirements**: LIFE-01, LIFE-02, LIFE-03, LIFE-04
**Architecture (locked, pending fresh /gsd-discuss-phase formalization)**:
  - `parser_override` PRESERVED. It is the only DuckDB v1.5.2 mechanism returning `vector<unique_ptr<SQLStatement>>` to the binder so writes land on the caller's transaction. Empirically: every alternative (`parse_function`+`plan_function`, `context.Query` from any extension callback, `Connection(*context.db).Query` for writes) fails one of the two non-negotiable constraints (D-20 transactional DDL or liveness via context_lock). See 65-OPTION-B-SPIKE.md, 65-EXEC-TIME-SPIKE.md.
  - **Catalog reads INSIDE `parser_override` are eliminated**, not relocated:
    - PK auto-inference from `duckdb_constraints()` deleted (`src/ddl/define.rs::resolve_pk_from_catalog`). Snowflake-aligned: PK in semantic views is a logical user assertion, not a physical catalog import. Users declare `PRIMARY KEY (cols)` or `UNIQUE (cols)` explicitly in TABLES, or use explicit `REFERENCES target(cols)` shorthand. Removing the auto-fallback is a correctness improvement.
    - Metadata capture (`now()`, `current_database()`, `current_schema()`) moves from extension-side SQL execution to SQL expressions inside the rewritten INSERT, evaluated by DuckDB on the caller's connection.
    - Existence checks for CREATE OR REPLACE / IF NOT EXISTS fold into `INSERT … ON CONFLICT` semantics; DROP/ALTER postcondition checks use `DELETE … RETURNING name` race-guard (Phase 60 pattern, already shipped).
    - DDL-time type inference (LIMIT 0 probes, typeof per fact) defers to read-side bind callbacks (which have ClientContext under the C++ Catalog API registration). SHOW/DESCRIBE probe lazily on first use.
  - **ALTER and CREATE FROM YAML FILE** use the rewrite-to-UPDATE-with-table-function-subquery pattern (ALTER-RC0): `parser_override` emits `UPDATE _definitions SET definition = (SELECT new_def FROM __sv_compute_*(args)) WHERE name = ?`; inner table function (registered via C++ Catalog API) has ClientContext, opens per-call `Connection(*context.db)` to read current state and compute new value; outer UPDATE writes transactionally on caller's conn. Validated empirically by 65-ALTER-REWRITE-SPIKE.md.
  - **Read-path callbacks** (`list`, `describe`, `show_columns`, `show_dims`, `show_dims_for_metric`, `show_metrics`, `show_facts`, `show_materializations`, `get_ddl`, `read_yaml`, `semantic_view`, `explain_semantic_view`) migrate from duckdb-rs's `register_table_function_with_extra_info` to the C++ Catalog API shim `sv_register_table_function` (Plan 02 partial). Bind callbacks gain ClientContext; each opens per-call `Connection(*context.db)`. `query_conn` retires once all 12 callbacks migrate.
**Success Criteria** (what must be TRUE):
  1. In the same Python process, after a writable connection that did `LOAD semantic_views` and `CREATE SEMANTIC VIEW` is closed, a subsequent `duckdb.connect(path, read_only=True)` against the same path returns within 5 seconds — verified both on a freshly bootstrapped DB and on a previously bootstrapped DB (LIFE-01).
  2. v0.8.0 transactional DDL semantics preserved byte-identical (CREATE/DROP/ALTER inside user BEGIN/COMMIT still participate in the transaction); existing Phase 58 ADBC transactional tests stay green.
  3. `test/integration/test_readonly_load.py` includes new `test_in_process_bootstrap_then_readonly` scenarios that exercise CREATE-then-close-then-reopen-readonly + read-side variants (SELECT through `semantic_view()`, `list`/`describe`/`show` after close); all guarded by watchdog; all fail on v0.9.0 baseline and pass on v0.10.0.
  4. Both long-lived extension-owned `duckdb_connection` handles (H1 catalog_conn, H2 query_conn) are retired from `init_extension`. Structural Rust unit test fails CI if anyone re-introduces a long-lived native handle.
  5. PK auto-inference removal documented in CHANGELOG as a behavior change (users relying on the fallback get a clear error pointing to the explicit-declaration alternative).
**Plans**:
  - 65-01 (DONE — `65-01-SUMMARY.md`): ConnGuard scaffolding + 5 watchdog tests (B1..B4 + B11) — ConnGuard later deleted by 65-03 D-02; watchdog tests retained for Plan 06 verification.
  - 65-02 (PARTIAL — `65-02-PARTIAL-SUMMARY.md`): sv_register_table_function C++ Catalog API shim; OverrideContext db_handle plumbing rewritten back to v0.9.0 shape by 65-03 D-01. Shim infrastructure surviving for Plans 04/05 consumption.
  - 65-03 (DONE — `65-03-SUMMARY.md`): parser_override slimming wave. Reverted Plan 02 partial damage; deleted conn_guard.rs (D-02), resolve_pk_from_catalog (D-05); moved CREATE-time metadata to SQL via json_merge_patch on caller's conn (D-16, metadata-via-SQL); added D-06 hard error path; deferred type inference to read-side (D-17). parser_override CREATE path has ZERO catalog reads. H1 catalog_conn allocation still present at src/lib.rs:386-410 but unused by CREATE path; Plan 06 retires the allocation. 49/49 sqllogictest; 933/933 nextest; 6/6 ADBC transactions; D-03 watchdog tests still TimeoutError as expected (flip green at Plan 06).
  - 65-04 (NEXT): ALTER architecture wave — RENAME/SET COMMENT/UNSET COMMENT pure-SQL json_merge_patch UPDATEs; CREATE FROM YAML FILE via `__sv_compute_create_from_yaml` helper TF registered through Plan 02's surviving C++ Catalog API shim.
  - 65-05: Read-path migration wave — 15 table-function + 2 scalar callbacks migrate to C++ Catalog API; per-call `Connection(*context.db)` from ClientContext; H2 query_conn retires.
  - 65-06: Lifecycle close-out — H1 catalog_conn retires; structural Rust unit test against re-introduction; extended post-reopen watchdog tests (D-03b); LIFE-04 ledger close.

### Phase 66: Expansion Qualification Across All Paths + ADBC Tests
**Goal**: Make `FROM semantic_view(...)` work through ADBC and any other client whose catalog/schema search path diverges from the extension's `query_conn` — and ship the milestone (CHANGELOG, version bump, CI green).
**Depends on**: Phase 65
**Requirements**: EXPAND-CTX-01, EXPAND-CTX-02, EXPAND-CTX-03, REL-01, REL-02, REL-03 (REL-03 was the v0.9.1-specific release prep; carries over to v0.10.0)
**Scope reassessment pending Phase 65**: The original EXPAND-CTX-01..03 requirements assumed `query_conn` (the long-lived H2 handle) would still exist and the fix was qualifying table references at SQL-emission time to bridge the catalog-search-path gap between query_conn and the caller's conn. Under read-elimination, `query_conn` retires entirely and read-path callbacks open per-call `Connection(*context.db)` from the caller's ClientContext — so they inherit the caller's catalog/schema search path natively. The EXPAND-CTX root cause likely dissolves. Empirical verification via the ADBC test suite (REL-01..02) will confirm; the phase shape may shrink to test scaffolding + release prep only.
**Success Criteria** (what must be TRUE):
  1. Through `adbc_driver_duckdb`, `SELECT … FROM semantic_view(...)` returns rows (not a `Catalog Error: Table with name X does not exist`) against semantic views exercising the main expansion path, FACTS, semi-additive metrics, window metrics, and a multi-database `ATTACH` scenario.
  2. A new `test/integration/test_adbc_queries.py` (runnable via `just test-adbc-queries`) covers those five scenarios end-to-end; it fails on the v0.9.0 baseline and passes on v0.10.0, serving as the regression guard.
  3. `_notes/error_with_adbc.md` is either removed or updated to point at the v0.10.0 fix.
  4. A user who installs v0.10.0 sees a `## [0.10.0]` section in `CHANGELOG.md` describing both fixes under `### Fixed` (plus the PK auto-inference removal under `### Changed`), with the `[Unreleased]` block reset and compare links updated; `Cargo.toml` and `description.yml` report `0.10.0`; `just test-all` and `just ci` are green on `milestone/v0.10.0`.
**Plans**: TBD

</details>
