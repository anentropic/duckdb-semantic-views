# Roadmap: DuckDB Semantic Views

## Overview

Five phases build the extension from a loadable scaffold to a fully queryable semantic layer. Phase 1 establishes the Rust/DuckDB integration foundation and resolves all architectural risks. Phase 2 adds the persistence layer and function-based DDL. Phase 3 implements the core expansion engine in pure Rust with comprehensive tests. Phase 4 wires the expansion engine to DuckDB's query pipeline so users can run `FROM my_view(dimensions := [...], metrics := [...])`. Phase 5 hardens the implementation with fuzz targets and writes maintainer documentation for community distribution. Phases 6-7 close tech debt identified by the v1.0 milestone audit.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Scaffold** - Loadable extension with CI, code quality gates, and architectural decisions locked
- [x] **Phase 2: Storage and DDL** - Persistence layer, function-based DDL, and round-trip survival across restarts
- [x] **Phase 3: Expansion Engine** - Pure Rust expansion logic with unit and property tests against known-answer datasets (completed 2026-02-25)
- [x] **Phase 4: Query Interface** - Replacement scan + table function wiring the expansion engine to DuckDB's query pipeline (completed 2026-02-25)
- [x] **Phase 5: Hardening and Docs** - Fuzz targets covering the FFI boundary, and MAINTAINER.md for community distribution (completed 2026-02-26)
- [ ] **Phase 6: Tech Debt Code Cleanup** - Remove dead code, fix feature-gate inconsistency, fix test idempotency, fix sandbox test failures
- [ ] **Phase 7: Verification & Formal Closure** - Human verification of CI/tests/fuzz, document accepted decisions and architectural limitations

## Phase Details

### Phase 1: Scaffold
**Goal**: A loadable DuckDB extension exists with CI passing, code quality enforced, and all architectural decisions resolved before any business logic is written
**Depends on**: Nothing (first phase)
**Requirements**: INFRA-01, INFRA-02, INFRA-03, INFRA-04, STYLE-01, STYLE-02
**Success Criteria** (what must be TRUE):
  1. Running `LOAD 'semantic_views'` in a DuckDB shell succeeds without error on Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, and Windows x86_64
  2. A CI smoke test loads the extension via the DuckDB CLI (not just `cargo test`) and fails the build if the extension cannot be loaded — catching ABI version mismatches
  3. A scheduled CI job builds against the latest DuckDB release and opens a GitHub PR mentioning @copilot when the build breaks
  4. All Rust code passes `rustfmt` and `clippy` (pedantic lints) — violations fail CI
**Plans**: 3 plans

Plans:
- [ ] 01-01-PLAN.md — Rust extension scaffold (Cargo.toml, src/lib.rs, rustfmt, deny, Justfile, cargo-husky)
- [ ] 01-02-PLAN.md — CI workflows and LOAD smoke test (PullRequestCI, MainDistributionPipeline, CodeQuality, SQLLogicTest)
- [ ] 01-03-PLAN.md — Scheduled DuckDB version monitor workflow

### Phase 2: Storage and DDL
**Goal**: Users can register, inspect, and remove semantic view definitions, and those definitions survive a DuckDB restart
**Depends on**: Phase 1
**Requirements**: DDL-01, DDL-02, DDL-03, DDL-04, DDL-05
**Success Criteria** (what must be TRUE):
  1. `SELECT define_semantic_view('orders', '{...json...}')` registers a definition and returns confirmation; the `semantic_layer._definitions` table contains the row
  2. `SELECT drop_semantic_view('orders')` removes the definition; subsequent `describe_semantic_view('orders')` returns an error
  3. `FROM list_semantic_views()` returns a row for every registered semantic view
  4. `FROM describe_semantic_view('orders')` returns the structured definition fields (name, dimensions, metrics, base table, filters)
  5. After closing and reopening the DuckDB file, all previously registered semantic views are available — definitions survive a restart
**Plans**: 4 plans

Plans:
- [ ] 02-01-PLAN.md — SemanticViewDefinition model, serde_json dep, CatalogState, init_catalog, catalog_insert, catalog_delete
- [ ] 02-02-PLAN.md — VScalar DDL functions (define, drop) + VTab functions (list, describe) + entrypoint wiring
- [ ] 02-03-PLAN.md — SQL logic test for full DDL round-trip including persistence (DDL-05)
- [ ] 02-04-PLAN.md — Gap closure: fix DDL-05 by resolving host DB path via PRAGMA database_list; add restart SQLLogicTest section

### Phase 3: Expansion Engine
**Goal**: The expansion engine correctly generates SQL for all metric types, single-hop joins, GROUP BY inference, row filter composition, and identifier quoting — verified by unit and property tests against known-answer datasets
**Depends on**: Phase 2
**Requirements**: MODEL-01, MODEL-02, MODEL-03, MODEL-04, EXPAND-01, EXPAND-02, EXPAND-03, EXPAND-04, TEST-01, TEST-02
**Success Criteria** (what must be TRUE):
  1. The `expand()` function, called with a semantic view definition and a dimension+metric selection, produces a SQL string where every requested dimension appears in the GROUP BY clause
  2. The `expand()` function correctly infers JOIN clauses from the entity relationships declared in the definition; requesting a metric from a joined table generates the correct JOIN without the user specifying it
  3. Requesting a dimension or metric name that does not exist in the definition produces a clear error message identifying the semantic view name and the unknown member name
  4. All SQL identifiers (view name, column names, table names) in emitted SQL are quoted with double-quotes, preventing reserved-word conflicts
  5. Property-based tests (proptest) verify that for any combination of valid dimensions and metrics, all requested dimensions appear in GROUP BY and the emitted SQL is syntactically valid
**Plans**: 3 plans

Plans:
- [ ] 03-01-PLAN.md — Model struct updates + expand() core: CTE generation, GROUP BY inference, filter composition, identifier quoting (TDD)
- [ ] 03-02-PLAN.md — Join dependency resolution + name validation with fuzzy matching (TDD)
- [ ] 03-03-PLAN.md — Property-based tests with proptest for expansion invariants

### Phase 4: Query Interface
**Goal**: Users can query any registered semantic view with `FROM view_name(dimensions := [...], metrics := [...])` and receive correct results — the full round-trip from definition to DuckDB result set works
**Depends on**: Phase 3
**Requirements**: QUERY-01, QUERY-02, QUERY-03, QUERY-04, TEST-03, TEST-04
**Success Criteria** (what must be TRUE):
  1. `FROM orders_view(dimensions := ['region'], metrics := ['total_revenue'])` executes against a DuckDB database and returns a result set with one row per region containing the correct aggregate value
  2. A user-supplied `WHERE` clause is AND-composed with the semantic view's row-level filters — the view's filters are never dropped when the user adds their own filter
  3. `SELECT *` on a semantic view query returns all requested dimensions and metrics with correct column names and types inferred at bind time
  4. `EXPLAIN` on a semantic view query shows the expanded SQL string rather than just the DuckDB physical plan, enabling users to inspect what SQL the extension generated
  5. Integration tests define a semantic view over a real DuckDB database (including at least one Apache Iceberg table source), run queries, and assert that result sets match known-correct values
**Plans**: 3 plans

Plans:
- [ ] 04-01-PLAN.md — expand() dimensions-only support + semantic_query table function with FFI SQL execution
- [ ] 04-02-PLAN.md — explain_semantic_view table function for EXPLAIN output
- [ ] 04-03-PLAN.md — Integration tests (SQLLogicTest + DuckLake/Iceberg setup)

### Phase 5: Hardening and Docs
**Goal**: The extension is resilient against malformed inputs at the FFI boundary and is documented well enough for a contributor to set up, build, test, and publish without asking for help
**Depends on**: Phase 4
**Requirements**: TEST-05, DOCS-01
**Success Criteria** (what must be TRUE):
  1. `cargo fuzz run` targets cover the C FFI boundary (input parsing) and the SQL generation path; no undefined behavior is triggered on a corpus of malformed inputs
  2. A contributor following only `MAINTAINER.md` can: set up a dev environment, build the extension, run all tests, load the extension in a DuckDB shell, update the DuckDB version pin, run the fuzzer, and understand the community extension registry publishing process — without needing to ask for clarification
**Plans**: 2 plans

Plans:
- [ ] 05-01-PLAN.md — Fuzz infrastructure: three cargo-fuzz targets, seed corpus, nightly CI workflow
- [ ] 05-02-PLAN.md — MAINTAINER.md: complete maintainer documentation with architecture overview and worked examples

### Phase 6: Tech Debt Code Cleanup
**Goal**: Eliminate dead code, fix feature-gate inconsistency, and resolve test reliability issues identified by the v1.0 milestone audit
**Depends on**: Phase 5
**Requirements**: None (tech debt closure — all requirements already satisfied)
**Gap Closure:** Closes tech debt from v1.0-MILESTONE-AUDIT.md
**Success Criteria** (what must be TRUE):
  1. No `#[allow(dead_code)]` annotations remain in `table_function.rs` — the dead `logical_type_from_duckdb_type` function and `column_type_ids` field are removed
  2. `pub mod query` in `lib.rs` is gated with `#[cfg(feature = "extension")]` consistent with other modules
  3. `phase2_ddl.test` restart section can be re-run without hanging — sidecar file cleanup added at section start
  4. All 3 catalog sidecar tests pass in both local and sandbox environments
**Plans**: 1 plan

Plans:
- [x] 06-01-PLAN.md — Dead code removal, feature-gate fix, portable test paths, SQLLogicTest idempotency

### Phase 7: Verification & Formal Closure
**Goal**: Complete all human verification tasks and formally document accepted design decisions, deferred items, and architectural limitations before milestone archival
**Depends on**: Phase 6
**Requirements**: None (verification and documentation — all requirements already satisfied)
**Gap Closure:** Closes tech debt from v1.0-MILESTONE-AUDIT.md
**Success Criteria** (what must be TRUE):
  1. CI workflows (PullRequestCI, MainDistributionPipeline, CodeQuality) confirmed passing on GitHub
  2. Full SQLLogicTest suite (`just test-sql`) passes
  3. DuckLake/Iceberg integration test (`just setup-ducklake && just test-iceberg`) passes
  4. DuckDB Version Monitor workflow manually triggered and conditional PR logic confirmed
  5. All 3 fuzz targets run without crashes (`cargo fuzz run` with nightly toolchain)
  6. MAINTAINER.md reviewed for readability by someone unfamiliar with Rust
  7. Accepted design decisions and architectural limitations documented in a TECH-DEBT.md file for v0.2 reference
**Plans**: 2 plans

Plans:
- [ ] 07-01-PLAN.md — Create TECH-DEBT.md documenting accepted decisions, deferred items, and architectural limitations
- [ ] 07-02-PLAN.md — Human verification checklist (CI, tests, fuzz, MAINTAINER.md review) with report

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5 → 6 → 7

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Scaffold | 3/3 | Complete | 2026-02-24 |
| 2. Storage and DDL | 4/4 | Complete | 2026-02-24 |
| 3. Expansion Engine | 0/3 | Complete    | 2026-02-25 |
| 4. Query Interface | 3/3 | Complete | 2026-02-25 |
| 5. Hardening and Docs | 2/2 | Complete | 2026-02-26 |
| 6. Tech Debt Code Cleanup | 1/1 | Complete | 2026-02-26 |
| 7. Verification & Formal Closure | 0/0 | Pending | — |
