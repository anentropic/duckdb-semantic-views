# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, and relationships directly in DuckDB. Users register semantic views via `create_semantic_view()` with typed STRUCT/LIST parameters (Snowflake-aligned syntax), then query them with `FROM semantic_view('view', dimensions := [...], metrics := [...])`. The extension expands semantic view references into concrete SQL (with GROUP BY, JOINs, typed output columns, and filters) and hands the result to DuckDB for execution.

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
- ✓ C++ shim infrastructure with feature-gated cc crate compilation and symbol visibility — v0.2.0
- ✓ Time dimensions with date_trunc granularity coarsening (day/week/month/year) and per-query override — v0.2.0
- ✓ DuckDB-native catalog persistence via pragma_query_t (sidecar file eliminated) — v0.2.0
- ✓ Snowflake-aligned 6-arg STRUCT/LIST DDL syntax (`create_semantic_view`) — v0.2.0
- ✓ `explain_semantic_view` shows DuckDB's full physical query plan for expanded SQL — v0.2.0
- ✓ Typed output columns with binary-read dispatch (BIGINT, DOUBLE, DATE, TIMESTAMP, etc.) — v0.2.0
- ✓ 36 property-based tests for typed output pipeline covering all scalar/composite types — v0.2.0
- ✓ DuckLake integration tests with CI job and DuckDB version monitor — v0.2.0

### Active

- [ ] Community extension registry publication (`INSTALL semantic_views FROM community`)
- [ ] Real-world TPC-H demo notebook
- [ ] WEEK and QUARTER time granularities
- [ ] Native `CREATE SEMANTIC VIEW` DDL syntax (blocked: Python DuckDB `-fvisibility=hidden`)

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone (v0.3.0+)
- YAML definition format — SQL DDL first; YAML is a future path
- Derived metrics (metric-on-metric, e.g., profit = revenue - cost) — future milestone
- Hierarchies (drill-down paths, e.g., country → region → city) — future milestone
- Cross-view optimisation (sharing materialised tables across views) — non-goal by design
- Custom query engine — DuckDB is the engine; the extension is a preprocessor only
- BI tool HTTP API — not a DuckDB extension concern; Cube.dev handles this use case
- Column-level security — beyond row-level filter scope; DuckDB handles column access
- Fiscal calendar / Sunday-start weeks — ISO 8601 only for now
- Native `CREATE SEMANTIC VIEW` parser hook — architecturally impossible when loaded via Python DuckDB (`-fvisibility=hidden` hides all C++ symbols)

## Context

**Shipped v0.2.0** with 7,462 LOC Rust across 21 .rs files in 3 days (125 commits).
**Tech stack:** Rust, C++ (shim), duckdb-rs 1.4.4, cc crate, serde_json, strsim, proptest.
**Architecture:** Extension is a preprocessor — expands semantic view queries into concrete SQL with typed output columns. DuckDB handles all execution. Persistence via `pragma_query_t` with separate connection (write-first pattern).
**Tests:** 136 total — Rust unit tests, property-based tests (proptest), sqllogictest integration tests, DuckLake CI tests.
**Known limitations:** See TECH-DEBT.md at repo root for accepted decisions and deferred items.

**Design research:** A detailed design doc lives in `_notes/semantic-views-duckdb-design-doc.md`. It covers prior art (Cube.dev internals, Snowflake semantic views, Databricks metric views), the two-phase architecture (expansion → pre-aggregation selection), and why `egg`/e-graph rewriting is not needed for this approach.

**Comparable systems:**
- Snowflake `SEMANTIC_VIEW` — SQL-native DDL + table function query syntax (closest design reference)
- Databricks metric views — YAML-defined, similar semantic model
- Cube.dev — the most mature OSS semantic layer, but deeply coupled to its own runtime; not reusable

## Constraints

- **Language**: Rust + C++ — Rust for extension logic, C++ shim for pragma callbacks (parser hooks proven impossible)
- **Target**: DuckDB extension — must integrate with DuckDB's extension loading mechanism
- **Correctness over performance**: Expansion must produce correct results; DuckDB handles optimisation
- **Python DuckDB compatibility**: All extension entry points must use C API function pointers only — C++ symbols are hidden

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Function-based DDL for v0.1.0 | Parser hooks not exposed to Rust via C API; native DDL deferred to v0.2.0 C++ shim | ✓ Good — shipped quickly, clean API |
| SQL DDL before YAML | SQL is simpler to implement as a single interface; YAML adds a second definition path | ✓ Good — JSON definition via SQL function works well |
| Expansion-only v0.1.0 | Pre-aggregation is orthogonal complexity; ship the semantic layer first | ✓ Good — validated the core value without extra complexity |
| DuckDB is the execution engine | Extension is a preprocessor; avoids building a query engine | ✓ Good |
| Sidecar file for v0.1.0 catalog persistence | DuckDB locks prevent SQL from scalar `invoke`; sidecar (plain JSON file I/O) is the only deadlock-free pure-Rust option | ✓ Replaced — v0.2.0 pragma_query_t eliminates sidecar |
| Cargo feature split (bundled/extension) | Enables `cargo test` with in-memory DuckDB while cdylib builds use loadable-extension stubs | ✓ Good — resolved the fundamental DuckDB Rust extension testing problem |
| Manual FFI entrypoint | Replaced macro to capture raw duckdb_database handle for independent query connection | ✓ Good — solved execution lock deadlock |
| Independent query connection via duckdb_connect | semantic_query uses separate connection to avoid lock conflicts during expanded SQL execution | ✓ Good — critical for table function to work |
| CTE-based expansion | All source tables flattened into single `_base` CTE; dimensions/metrics reference flat namespace | ✓ Good — simple and correct; requires unqualified column names in expressions |
| Scalar function DDL as permanent v0.2.0 interface | C++ parser hook impossible in Python DuckDB (`-fvisibility=hidden`); scalar functions work via C API function pointers | ✓ Good — discovered architectural limitation early |
| Vendor full duckdb/src/include/ header tree | duckdb.hpp includes subdirectory headers that must be present; sourced from cargo build cache | ✓ Good — no network dependency |
| pragma_query_t for catalog persistence | Write-first pattern with separate persist_conn avoids execution lock deadlock | ✓ Good — transactional, sidecar eliminated |
| Snowflake-aligned STRUCT/LIST DDL syntax | 6-arg typed parameters instead of raw JSON string; aligns with Snowflake semantic view concepts | ✓ Good — cleaner API, better IDE support |
| Binary-read dispatch for typed output | Direct chunk reads per type instead of VARCHAR cast intermediary; fixes TIMESTAMP/BOOLEAN/DECIMAL bugs | ✓ Good — correct types, validated by 36 PBTs |
| LIMIT 0 type inference at define time | Query source tables with LIMIT 0 to infer column types without reading data | ✓ Good — zero-cost type discovery |

---
*Last updated: 2026-03-03 after v0.2.0 milestone*
