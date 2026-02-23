# Roadmap: DuckDB Semantic Views

## Overview

Five phases build the extension from a loadable scaffold to a fully queryable semantic layer. Phase 1 establishes the Rust/DuckDB integration foundation and resolves all architectural risks. Phase 2 adds the persistence layer and function-based DDL. Phase 3 implements the core expansion engine in pure Rust with comprehensive tests. Phase 4 wires the expansion engine to DuckDB's query pipeline so users can run `FROM my_view(dimensions := [...], metrics := [...])`. Phase 5 hardens the implementation with fuzz targets and writes maintainer documentation for community distribution.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Scaffold** - Loadable extension with CI, code quality gates, and architectural decisions locked
- [ ] **Phase 2: Storage and DDL** - Persistence layer, function-based DDL, and round-trip survival across restarts
- [ ] **Phase 3: Expansion Engine** - Pure Rust expansion logic with unit and property tests against known-answer datasets
- [ ] **Phase 4: Query Interface** - Replacement scan + table function wiring the expansion engine to DuckDB's query pipeline
- [ ] **Phase 5: Hardening and Docs** - Fuzz targets covering the FFI boundary, and MAINTAINER.md for community distribution

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
**Plans**: TBD

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
**Plans**: TBD

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
**Plans**: TBD

### Phase 5: Hardening and Docs
**Goal**: The extension is resilient against malformed inputs at the FFI boundary and is documented well enough for a contributor to set up, build, test, and publish without asking for help
**Depends on**: Phase 4
**Requirements**: TEST-05, DOCS-01
**Success Criteria** (what must be TRUE):
  1. `cargo fuzz run` targets cover the C FFI boundary (input parsing) and the SQL generation path; no undefined behavior is triggered on a corpus of malformed inputs
  2. A contributor following only `MAINTAINER.md` can: set up a dev environment, build the extension, run all tests, load the extension in a DuckDB shell, update the DuckDB version pin, run the fuzzer, and understand the community extension registry publishing process — without needing to ask for clarification
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Scaffold | 0/3 | In progress | - |
| 2. Storage and DDL | 0/? | Not started | - |
| 3. Expansion Engine | 0/? | Not started | - |
| 4. Query Interface | 0/? | Not started | - |
| 5. Hardening and Docs | 0/? | Not started | - |
