# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, and relationships directly in DuckDB. Users register semantic views via native `CREATE SEMANTIC VIEW` DDL with SQL keyword clauses (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS), then query them with `FROM semantic_view('view', dimensions := [...], metrics := [...])`. The extension expands semantic view references into concrete SQL (with GROUP BY, JOINs from PK/FK declarations, typed output columns, and filters) and hands the result to DuckDB for execution.

The project targets open source release via the DuckDB community extension registry, filling a gap that exists in the ecosystem: Snowflake, Databricks, and Cube.dev all have semantic layers, but DuckDB has none.

## Core Value

A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## Requirements

### Validated

- ✓ Extension scaffold with multi-platform CI and automated DuckDB version monitoring — v0.1.0
- ✓ Function-based DDL: define, drop, list, describe semantic views — v0.1.0
- ✓ Semantic view definitions support: dimensions, measures/metrics, base table + joins, row-level filters, entity relationships — v0.1.0
- ✓ Definitions persist across DuckDB restarts (sidecar file + catalog table sync) — v0.1.0
- ✓ Query semantic views with `FROM semantic_query('view', dimensions := [...], metrics := [...])` — v0.1.0
- ✓ Extension expands semantic view references into concrete SQL (GROUP BY, JOINs inferred from definition) — v0.1.0
- ✓ Entity relationships drive join inference with transitive dependency resolution — v0.1.0
- ✓ Name validation with fuzzy "did you mean" suggestions at query time — v0.1.0
- ✓ All generated SQL identifiers quoted to prevent reserved-word conflicts — v0.1.0
- ✓ Unit tests, property-based tests (proptest), integration tests, and fuzz targets — v0.1.0
- ✓ MAINTAINER.md covering complete developer lifecycle — v0.1.0
- ✓ Symbol visibility for extension builds (feature-gated in build.rs) — v0.2.0
- ✓ Time dimensions with date_trunc granularity coarsening (day/week/month/year) and per-query override — v0.2.0
- ✓ DuckDB-native catalog persistence via pragma_query_t (sidecar file eliminated) — v0.2.0
- ✓ Snowflake-aligned STRUCT/LIST DDL syntax (`create_semantic_view`) — v0.2.0
- ✓ `explain_semantic_view` shows DuckDB's full physical query plan for expanded SQL — v0.2.0
- ✓ Typed output columns via zero-copy vector reference (replaced binary-read dispatch) — v0.3.0
- ✓ Removed time_dimensions and granularities; time truncation expressed via dimension expr directly — v0.4.0
- ✓ 36 property-based tests for typed output pipeline covering all scalar/composite types — v0.2.0
- ✓ DuckLake integration tests with CI job and DuckDB version monitor — v0.2.0
- ✓ Native `CREATE SEMANTIC VIEW` DDL syntax via parser extension hooks — v0.5.0
- ✓ C++ shim with vendored DuckDB amalgamation for parser hook registration — v0.5.0
- ✓ Runtime type validation + defensive SQL wrapping for Python crash prevention — v0.5.0
- ✓ `DROP SEMANTIC VIEW [name]` native DDL — v0.5.1
- ✓ `CREATE OR REPLACE SEMANTIC VIEW` native DDL — v0.5.1
- ✓ `CREATE SEMANTIC VIEW IF NOT EXISTS` native DDL — v0.5.1
- ✓ `DESCRIBE SEMANTIC VIEW [name]` — shows dimensions, metrics, types — v0.5.1
- ✓ `SHOW SEMANTIC VIEWS` — lists all defined semantic views — v0.5.1
- ✓ Error location reporting: clause-level hints + character position + "did you mean" suggestions — v0.5.1
- ✓ README DDL syntax reference + worked examples — v0.5.1
- ✓ SQL keyword body syntax: TABLES, RELATIONSHIPS, DIMENSIONS, METRICS clauses — v0.5.2
- ✓ PK/FK relationship model: tables declare aliases + PRIMARY KEY, relationships use FK REFERENCES — v0.5.2
- ✓ JOIN ON clauses synthesized from PK/FK declarations (replaces ON-clause heuristic) — v0.5.2
- ✓ Topological sort ordering and transitive join inclusion — v0.5.2
- ✓ Define-time graph validation: cycles and diamonds rejected — v0.5.2
- ✓ Qualified column references (`alias.column`) in dimension/metric expressions — v0.5.2
- ✓ Function-based DDL retired; native DDL is sole interface — v0.5.2
- ✓ Parser robustness: token-based DDL detection, adversarial input hardening, fuzz_ddl_parse target — v0.5.2

### Active

## Current Milestone: v0.5.3 Advanced Semantic Features

**Goal:** Add advanced semantic modeling capabilities — FACTS clause, derived metrics, hierarchies, fan trap detection, role-playing dimensions, semi-additive metrics, and multiple join paths.

**Target features:**
- FACTS clause (named row-level sub-expressions for metrics)
- Derived metrics (metric referencing other metrics)
- Hierarchies / drill-down paths
- Fan trap detection and deduplication warnings
- Role-playing dimensions (same table joined via different relationships)
- Semi-additive metrics (NON ADDITIVE BY)
- Multiple join paths (USING RELATIONSHIPS — relaxes diamond rejection)

## Shipped: v0.5.2 SQL DDL & PK/FK Relationships (2026-03-13)

SQL keyword body syntax (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS) replacing function-call DDL, Snowflake-style PK/FK relationship model with automatic JOIN ON synthesis, topological sort ordering, define-time graph validation, and qualified column references. Function-based DDL interface fully retired; native DDL is sole interface. 282 Rust tests + 7 sqllogictest + 6 DuckLake CI green.

## Shipped: v0.5.1 DDL Polish (2026-03-09)

Complete native DDL surface — all 7 DDL verbs (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW) via parser extension hooks. Error location reporting with clause-level hints, character positions, and "did you mean" suggestions.

## Shipped: v0.5.0 Parser Extension Spike (2026-03-08)

Native `CREATE SEMANTIC VIEW` DDL syntax achieved via DuckDB parser extension hooks. C++ shim compiled via `cc` crate against vendored DuckDB amalgamation.

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone
- YAML definition format — SQL DDL first; YAML is a future path
- Derived metrics (metric-on-metric, e.g., profit = revenue - cost) — future milestone
- Hierarchies (drill-down paths, e.g., country → region → city) — future milestone
- Cross-view optimisation (sharing materialised tables across views) — non-goal by design
- Custom query engine — DuckDB is the engine; the extension is a preprocessor only
- BI tool HTTP API — not a DuckDB extension concern; Cube.dev handles this use case
- Column-level security — beyond row-level filter scope; DuckDB handles column access
- Fiscal calendar / Sunday-start weeks — ISO 8601 only for now
- Backward compatibility for old DDL syntax — pre-release; finding the right design
- Multiple join paths between same tables — error on diamonds; defer explicit paths

## Context

**Shipped v0.5.2** — SQL keyword DDL body, PK/FK relationship model, graph-based join resolution, function DDL retired. Native DDL is sole interface.
**Tech stack:** Rust + C++ shim (vendored DuckDB amalgamation via cc crate), duckdb-rs 1.4.4, serde_json, strsim, proptest.
**Architecture:** Extension is a preprocessor — expands semantic view queries into concrete SQL with typed output columns. DuckDB handles all execution. Query results stream via zero-copy vector references (`duckdb_vector_reference_vector`). Persistence via `pragma_query_t` with separate connection (write-first pattern). Parser hook via C++ shim: `parse_function` fallback detects all 7 DDL forms, Rust `DdlKind` enum dispatches rewrite. DDL body parsed by `body_parser.rs` state machine into TableRef/Join/Dimension/Metric structs with PK/FK annotations. `RelationshipGraph` validates tree structure and topologically sorts joins. Expansion generates `FROM base AS alias LEFT JOIN t AS alias ON pk=fk` with qualified column references.
**Tests:** 282+ total — Rust unit tests, property-based tests (proptest), sqllogictest integration tests (7), DuckLake CI tests (6), Python crash repro (13), Python caret tests (3), 4 fuzz targets.
**Known limitations:** See TECH-DEBT.md at repo root for accepted decisions and deferred items.
**Source LOC:** 8,217 Rust (src/).

**Design research:** A detailed design doc lives in `_notes/semantic-views-duckdb-design-doc.md`. It covers prior art (Cube.dev internals, Snowflake semantic views, Databricks metric views), the two-phase architecture (expansion → pre-aggregation selection), and why `egg`/e-graph rewriting is not needed for this approach.

**Comparable systems:**
- Snowflake `SEMANTIC_VIEW` — SQL-native DDL + table function query syntax (closest design reference)
- Databricks metric views — YAML-defined, similar semantic model
- Cube.dev — the most mature OSS semantic layer, but deeply coupled to its own runtime; not reusable

## Constraints

- **Language**: Rust + C++ shim — parser hooks require C++ (static-linked DuckDB amalgamation bypasses `-fvisibility=hidden`)
- **Target**: DuckDB extension — must integrate with DuckDB's extension loading mechanism
- **Correctness over performance**: Expansion must produce correct results; DuckDB handles optimisation
- **Python DuckDB compatibility**: All extension entry points must use C API function pointers only — C++ symbols are hidden

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Function-based DDL for v0.1.0 | Parser hooks not exposed to Rust via C API; native DDL architecturally impossible | ✓ Good — shipped quickly; retired in v0.5.2 |
| SQL DDL before YAML | SQL is simpler to implement as a single interface; YAML adds a second definition path | ✓ Good — JSON definition via SQL function works well |
| Expansion-only v0.1.0 | Pre-aggregation is orthogonal complexity; ship the semantic layer first | ✓ Good — validated the core value without extra complexity |
| DuckDB is the execution engine | Extension is a preprocessor; avoids building a query engine | ✓ Good |
| Cargo feature split (bundled/extension) | Enables `cargo test` with in-memory DuckDB while cdylib builds use loadable-extension stubs | ✓ Good — resolved the fundamental DuckDB Rust extension testing problem |
| pragma_query_t for catalog persistence | Write-first pattern with separate persist_conn avoids execution lock deadlock | ✓ Good — transactional, sidecar eliminated |
| Zero-copy vector reference for typed output | `duckdb_vector_reference_vector` streams result chunks directly into output; type mismatches handled by `build_execution_sql` casts | ✓ Good — correct types, zero overhead |
| Parser extension via static-linked C++ shim | Dynamic C++ symbol resolution impossible; static linking against amalgamation bypasses `-fvisibility=hidden` | ✓ Good — v0.5.0 shipped with C_STRUCT_UNSTABLE ABI |
| Statement rewriting for DDL | Rewrite `CREATE SEMANTIC VIEW` to internal function call instead of custom parser grammar | ✓ Good — simpler than custom grammar |
| DuckDB amalgamation compilation | Full DuckDB compiled into extension binary (~20MB) for parser hook symbol access | ⚠️ Revisit — binary size concern; investigate selective linking in future |
| Runtime type validation before vector reference | Defensive type check before `duckdb_vector_reference_vector` returns recoverable error instead of SIGABRT | ✓ Good — prevents Python crashes |
| SQL keyword body syntax (TABLES/RELATIONSHIPS/DIMENSIONS/METRICS) | Proper SQL DDL instead of function-call body; aligns with Snowflake semantic view syntax | ✓ Good — clean, readable DDL |
| PK/FK relationship model with graph validation | Tables declare PKs, relationships use FK REFERENCES; graph validated at define time (cycles/diamonds rejected) | ✓ Good — deterministic JOIN synthesis, no heuristics |
| Kahn's algorithm for topological sort | Naturally detects cycles via leftover nodes; simpler than DFS for relationship graphs | ✓ Good — clean implementation |
| Function DDL retirement in v0.5.2 | Single interface reduces maintenance; native DDL is strictly superior | ✓ Good — -400 LOC removed, sole DDL path |
| FROM+JOIN expansion replacing CTE flattening | Direct `FROM base LEFT JOIN t ON pk=fk` instead of `_base` CTE; enables qualified column refs | ✓ Good — correct scoping, cleaner SQL |

---
*Last updated: 2026-03-14 after v0.5.3 milestone start*
