# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, and relationships directly in DuckDB. Users register semantic views via native `CREATE SEMANTIC VIEW` DDL syntax or `create_semantic_view()` function with typed STRUCT/LIST parameters (Snowflake-aligned syntax), then query them with `FROM semantic_view('view', dimensions := [...], metrics := [...])`. The extension expands semantic view references into concrete SQL (with GROUP BY, JOINs, typed output columns, and filters) and hands the result to DuckDB for execution.

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
- ✓ Symbol visibility for extension builds (feature-gated in build.rs) — v0.2.0 (C++ shim removed in v0.4.0)
- ✓ Time dimensions with date_trunc granularity coarsening (day/week/month/year) and per-query override — v0.2.0
- ✓ DuckDB-native catalog persistence via pragma_query_t (sidecar file eliminated) — v0.2.0
- ✓ Snowflake-aligned 6-arg STRUCT/LIST DDL syntax (`create_semantic_view`) — v0.2.0
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

### Active

See: .planning/REQUIREMENTS.md

## Current Milestone: v0.5.2 SQL DDL & PK/FK Relationships

**Goal:** Replace function-call DDL syntax with proper SQL keyword syntax and adopt Snowflake-style PK/FK relationship model with table aliases, eliminating ON-clause heuristics and enabling qualified column names.

**Target features:**
- Proper SQL DDL syntax: `CREATE SEMANTIC VIEW` accepts `TABLES (...)`, `DIMENSIONS (...)`, `METRICS (...)` keyword clauses
- PK/FK relationship model: tables declare aliases + PRIMARY KEY, relationships use FK REFERENCES
- JOIN inference from PK/FK declarations (replaces ON-clause substring matching)
- Qualified column names in expressions (`orders.revenue` works naturally via table aliases)

**Deferred to future milestones:**
- Community extension registry publication (`INSTALL semantic_views FROM community`)
- Real-world TPC-H demo notebook
- ~~WEEK and QUARTER time granularities~~ — removed in v0.4.0

## Shipped: v0.5.1 DDL Polish (2026-03-09)

Complete native DDL surface — all 7 DDL verbs (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW) via parser extension hooks. Error location reporting with clause-level hints, character positions, and "did you mean" suggestions. README updated with DDL reference and lifecycle example. 209 Rust tests + 7 SQL logic tests + 6 DuckLake CI tests green.

## Shipped: v0.5.0 Parser Extension Spike (2026-03-08)

Native `CREATE SEMANTIC VIEW` DDL syntax achieved via DuckDB parser extension hooks. C++ shim compiled via `cc` crate against vendored DuckDB amalgamation. Parser fallback hook detects `CREATE SEMANTIC VIEW` statements and rewrites them to function-based DDL. Both DDL interfaces coexist. 172 tests green.

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone (v0.5.0+)
- YAML definition format — SQL DDL first; YAML is a future path
- Derived metrics (metric-on-metric, e.g., profit = revenue - cost) — future milestone
- Hierarchies (drill-down paths, e.g., country → region → city) — future milestone
- Cross-view optimisation (sharing materialised tables across views) — non-goal by design
- Custom query engine — DuckDB is the engine; the extension is a preprocessor only
- BI tool HTTP API — not a DuckDB extension concern; Cube.dev handles this use case
- Column-level security — beyond row-level filter scope; DuckDB handles column access
- Fiscal calendar / Sunday-start weeks — ISO 8601 only for now
- Native `CREATE SEMANTIC VIEW` via dynamic C++ symbol resolution — impossible (`-fvisibility=hidden`); solved via static linking in v0.5.0

## Context

**Shipped v0.5.1** — complete native DDL surface (all 7 DDL verbs), error location reporting, 33 parser proptests, Python caret integration tests. v0.5.0 native `CREATE SEMANTIC VIEW` DDL syntax remains foundation.
**Tech stack:** Rust + C++ shim (vendored DuckDB amalgamation via cc crate), duckdb-rs 1.4.4, serde_json, strsim, proptest.
**Architecture:** Extension is a preprocessor — expands semantic view queries into concrete SQL with typed output columns. DuckDB handles all execution. Query results stream via zero-copy vector references (`duckdb_vector_reference_vector`). Persistence via `pragma_query_t` with separate connection (write-first pattern). Parser hook via C++ shim: `parse_function` fallback detects all 7 DDL forms, Rust `DdlKind` enum dispatches rewrite, C++ `sv_ddl_bind` executes and forwards result sets. Error validation via tri-state `sv_validate_ddl_rust` FFI.
**Tests:** 222+ total — Rust unit tests, 33 property-based tests (proptest), sqllogictest integration tests (7), DuckLake CI tests (6), Python caret integration tests.
**Known limitations:** See TECH-DEBT.md at repo root for accepted decisions and deferred items.

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
| Function-based DDL for v0.1.0 | Parser hooks not exposed to Rust via C API; native DDL architecturally impossible (C++ shim removed in v0.4.0) | ✓ Good — shipped quickly, clean API |
| SQL DDL before YAML | SQL is simpler to implement as a single interface; YAML adds a second definition path | ✓ Good — JSON definition via SQL function works well |
| Expansion-only v0.1.0 | Pre-aggregation is orthogonal complexity; ship the semantic layer first | ✓ Good — validated the core value without extra complexity |
| DuckDB is the execution engine | Extension is a preprocessor; avoids building a query engine | ✓ Good |
| Sidecar file for v0.1.0 catalog persistence | DuckDB locks prevent SQL from scalar `invoke`; sidecar (plain JSON file I/O) is the only deadlock-free pure-Rust option | ✓ Replaced — v0.2.0 pragma_query_t eliminates sidecar |
| Cargo feature split (bundled/extension) | Enables `cargo test` with in-memory DuckDB while cdylib builds use loadable-extension stubs | ✓ Good — resolved the fundamental DuckDB Rust extension testing problem |
| Manual FFI entrypoint | Replaced macro to capture raw duckdb_database handle for independent query connection | ✓ Good — solved execution lock deadlock |
| Independent query connection via duckdb_connect | semantic_query uses separate connection to avoid lock conflicts during expanded SQL execution | ✓ Good — critical for table function to work |
| CTE-based expansion | All source tables flattened into single `_base` CTE; dimensions/metrics reference flat namespace | ✓ Good — simple and correct; requires unqualified column names in expressions |
| Scalar function DDL as permanent v0.2.0 interface | C++ parser hook impossible in Python DuckDB (`-fvisibility=hidden`); scalar functions work via C API function pointers | ✓ Good — discovered architectural limitation early |
| ~~Vendor full duckdb/src/include/ header tree~~ | Was needed for C++ shim compilation; removed in v0.4.0 with shim removal | Removed — no longer needed |
| pragma_query_t for catalog persistence | Write-first pattern with separate persist_conn avoids execution lock deadlock | ✓ Good — transactional, sidecar eliminated |
| Snowflake-aligned STRUCT/LIST DDL syntax | 6-arg typed parameters instead of raw JSON string; aligns with Snowflake semantic view concepts | ✓ Good — cleaner API, better IDE support |
| Zero-copy vector reference for typed output | `duckdb_vector_reference_vector` streams result chunks directly into output; type mismatches handled by `build_execution_sql` casts. Replaced binary-read dispatch post-v0.2.0 (-600 LOC). | ✓ Good — correct types, zero overhead, validated by PBTs + vector_reference_test |
| LIMIT 0 type inference at define time | Query source tables with LIMIT 0 to infer column types without reading data | ✓ Good — zero-cost type discovery |

| Parser extension via static-linked C++ shim | Dynamic C++ symbol resolution impossible; static linking against amalgamation bypasses `-fvisibility=hidden` (validated by prql/duckpgq existence proofs) | ✓ Good — v0.5.0 shipped with C_STRUCT_UNSTABLE ABI |
| Statement rewriting for DDL | Rewrite `CREATE SEMANTIC VIEW` to `create_semantic_view()` function call instead of custom parser grammar | ✓ Good — simpler than custom grammar, full backward compatibility |
| DDL connection isolation | Separate connection for DDL execution from parser hook path to avoid lock conflicts | ✓ Good — same pattern as semantic_query |
| DuckDB amalgamation compilation | Full DuckDB compiled into extension binary (~20MB) for parser hook symbol access | ⚠️ Revisit — binary size concern; investigate selective linking in future |
| Runtime type validation before vector reference | Defensive type check before `duckdb_vector_reference_vector` returns recoverable error instead of SIGABRT | ✓ Good — prevents Python crashes |

---
*Last updated: 2026-03-09 after v0.5.2 milestone started*
