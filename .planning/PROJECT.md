# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, and relationships directly in DuckDB. Users register semantic views via function-based DDL (`define_semantic_view()`), then query them with `FROM semantic_query('view', dimensions := [...], metrics := [...])`. The extension expands semantic view references into concrete SQL (with GROUP BY, JOINs, and filters) and hands the result to DuckDB for execution.

The project targets open source release via the DuckDB community extension registry, filling a gap that exists in the ecosystem: Snowflake, Databricks, and Cube.dev all have semantic layers, but DuckDB has none.

## Core Value

A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## Requirements

### Validated

- ✓ Extension scaffold with multi-platform CI and automated DuckDB version monitoring — v1.0
- ✓ Function-based DDL: define, drop, list, describe semantic views — v1.0
- ✓ Semantic view definitions support: dimensions, measures/metrics, base table + joins, row-level filters, entity relationships — v1.0
- ✓ Definitions persist across DuckDB restarts (sidecar file + catalog table sync) — v1.0
- ✓ Query semantic views with `FROM semantic_query('view', dimensions := [...], metrics := [...])` — v1.0
- ✓ Extension expands semantic view references into concrete SQL (GROUP BY, JOINs inferred from definition) — v1.0
- ✓ Entity relationships drive join inference with transitive dependency resolution — v1.0
- ✓ Name validation with fuzzy "did you mean" suggestions at query time — v1.0
- ✓ All generated SQL identifiers quoted to prevent reserved-word conflicts — v1.0
- ✓ Unit tests, property-based tests (proptest), integration tests, and fuzz targets — v1.0
- ✓ MAINTAINER.md covering complete developer lifecycle — v1.0

### Active

- [ ] Native `CREATE SEMANTIC VIEW` DDL syntax (requires C++ shim for parser hooks)
- [ ] Time dimensions with granularity coarsening (day → week → month → year)
- [ ] Extension installable as a DuckDB community extension (`INSTALL semantic_views FROM community`)
- [ ] Real-world demo using TPC-H or similar dataset
- [ ] Native `EXPLAIN FROM semantic_query(...)` shows expanded SQL (requires C++ EXPLAIN hook)
- [ ] Replace sidecar persistence with `pragma_query_t` pattern (via C++ shim)

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone (v0.2+)
- YAML definition format — SQL DDL first; YAML is a future path
- Derived metrics (metric-on-metric, e.g., profit = revenue - cost) — future milestone
- Hierarchies (drill-down paths, e.g., country → region → city) — future milestone
- Cross-view optimisation (sharing materialised tables across views) — non-goal by design
- Custom query engine — DuckDB is the engine; the extension is a preprocessor only
- BI tool HTTP API — not a DuckDB extension concern; Cube.dev handles this use case
- Column-level security — beyond row-level filter scope; DuckDB handles column access

## Context

**Shipped v1.0** with 6,628 LOC Rust across 58 files in 6 days.
**Tech stack:** Rust, duckdb-rs 1.4.4, serde_json, strsim, proptest, cargo-fuzz.
**Architecture:** Extension is a preprocessor — expands semantic view queries into concrete SQL, DuckDB handles all execution. v1.0 implements expansion only; pre-aggregation selection is deferred.
**Persistence:** Sidecar file (`<db>.semantic_views`) bridges DuckDB's execution-lock limitation in scalar `invoke`; synced into `_semantic_views_catalog` table on next load. v0.2 C++ shim replaces this with `pragma_query_t` (FTS pattern).
**Known limitations:** See TECH-DEBT.md at repo root for 7 accepted decisions, 6 deferred items, and 4 architectural limitations.

**Design research:** A detailed design doc lives in `_notes/semantic-views-duckdb-design-doc.md`. It covers prior art (Cube.dev internals, Snowflake semantic views, Databricks metric views), the two-phase architecture (expansion → pre-aggregation selection), and why `egg`/e-graph rewriting is not needed for this approach.

**Comparable systems:**
- Snowflake `SEMANTIC_VIEW` — SQL-native DDL + table function query syntax (closest design reference)
- Databricks metric views — YAML-defined, similar semantic model
- Cube.dev — the most mature OSS semantic layer, but deeply coupled to its own runtime; not reusable

## Constraints

- **Language**: Rust — all extension code written in Rust
- **Target**: DuckDB extension — must integrate with DuckDB's extension loading mechanism
- **v0.1 scope**: Expansion only, no pre-aggregation — keeps the problem tractable and ships value faster
- **Correctness over performance**: Expansion must produce correct results; DuckDB handles optimisation

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Function-based DDL for v0.1 | Parser hooks not exposed to Rust via C API; native DDL deferred to v0.2 C++ shim | ✓ Good — shipped quickly, clean API |
| SQL DDL before YAML | SQL is simpler to implement as a single interface; YAML adds a second definition path | ✓ Good — JSON definition via SQL function works well |
| Expansion-only v0.1 | Pre-aggregation is orthogonal complexity; ship the semantic layer first | ✓ Good — validated the core value without extra complexity |
| DuckDB is the execution engine | Extension is a preprocessor; avoids building a query engine | ✓ Good |
| Sidecar file for v0.1 catalog persistence | DuckDB locks prevent SQL from scalar `invoke`; sidecar (plain JSON file I/O) is the only deadlock-free pure-Rust option | ✓ Accepted (temporary) — works reliably; v0.2 replaces with pragma_query_t |
| Cargo feature split (bundled/extension) | Enables `cargo test` with in-memory DuckDB while cdylib builds use loadable-extension stubs | ✓ Good — resolved the fundamental DuckDB Rust extension testing problem |
| Manual FFI entrypoint | Replaced macro to capture raw duckdb_database handle for independent query connection | ✓ Good — solved execution lock deadlock |
| Independent query connection via duckdb_connect | semantic_query uses separate connection to avoid lock conflicts during expanded SQL execution | ✓ Good — critical for table function to work |
| VARCHAR output columns | All semantic_query output columns declared as VARCHAR; avoids type mismatch panics | ⚠️ Revisit — typed output columns would be better UX in v0.2 |
| CTE-based expansion | All source tables flattened into single `_base` CTE; dimensions/metrics reference flat namespace | ✓ Good — simple and correct; requires unqualified column names in expressions |

---
*Last updated: 2026-02-28 after v1.0 milestone*
