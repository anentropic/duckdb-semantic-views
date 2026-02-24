# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, and relationships directly in DuckDB. Users define semantic views with SQL DDL (`CREATE SEMANTIC VIEW`), then query them with a natural `FROM my_view(...)` syntax. The extension expands semantic view references into concrete SQL and hands the result to DuckDB for execution.

The project targets open source release via the DuckDB community extension registry, filling a gap that exists in the ecosystem: Snowflake, Databricks, and Cube.dev all have semantic layers, but DuckDB has none.

## Core Value

A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] User can define a semantic view with SQL DDL (`CREATE SEMANTIC VIEW`)
- [ ] Semantic view definitions support: dimensions, measures/metrics, base table + joins, row-level filters, time dimensions, entity relationships
- [ ] User can query a semantic view with `SELECT ... FROM my_view(DIMENSIONS ... METRICS ...)`
- [ ] Extension expands semantic view references into concrete SQL (GROUP BY, JOINs inferred from definition)
- [ ] Time dimensions support granularity coarsening (day → month queries)
- [ ] Entity relationships drive join inference in expansion
- [ ] Granularity validation at expansion time (clear errors for invalid combinations)
- [ ] Extension is installable as a DuckDB community extension
- [ ] Real-world demo using TPC-H or similar dataset

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone (v0.2+)
- YAML definition format — SQL DDL first; YAML is a future path
- Derived metrics (metric-on-metric, e.g., profit = revenue - cost) — future milestone
- Hierarchies (drill-down paths, e.g., country → region → city) — future milestone
- Cross-view optimisation (sharing materialised table across two semantic view references) — non-goal by design

### v0.2 Requirements (Deferred)

- [ ] **Replace sidecar persistence with `pragma_query_t` pattern.** v0.1 uses a `.semantic_views` sidecar file for catalog persistence because DuckDB holds execution locks during scalar `invoke`, preventing any SQL execution (same connection, `try_clone`, or `Connection::open` all deadlock/block). The C++ shim planned for `CREATE SEMANTIC VIEW` DDL must also register define/drop as `PragmaFunction` with `pragma_query_t` callbacks — these return SQL strings that DuckDB executes after the callback returns (during parsing, before execution locks), which is the blessed pattern used by the FTS extension. This eliminates the sidecar file entirely.

## Context

**Design research:** A detailed design doc lives in `_notes/semantic-views-duckdb-design-doc.md`. It covers prior art (Cube.dev internals, Snowflake semantic views, Databricks metric views), the two-phase architecture (expansion → pre-aggregation selection), and why `egg`/e-graph rewriting is not needed for this approach.

**Architecture decision:** Semantic views are syntax sugar for parameterised analytic queries. The extension is a preprocessor — DuckDB handles all execution. v0.1 implements only Step 1 (semantic view expansion); Step 2 (pre-aggregation selection) is deferred.

**Comparable systems:**
- Snowflake `SEMANTIC_VIEW` — SQL-native DDL + table function query syntax (closest design reference)
- Databricks metric views — YAML-defined, similar semantic model
- Cube.dev — the most mature OSS semantic layer, but deeply coupled to its own runtime; not reusable

**DuckDB extension development:** Extensions are built against the DuckDB C++ extension SDK. Rust extensions use the `duckdb-rs` crate and/or the official DuckDB extension template adapted for Rust.

## Constraints

- **Language**: Rust — all extension code written in Rust
- **Target**: DuckDB extension — must integrate with DuckDB's extension loading mechanism
- **v0.1 scope**: Expansion only, no pre-aggregation — keeps the problem tractable and ships value faster
- **Correctness over performance**: Expansion must produce correct results; DuckDB handles optimisation

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Native DDL syntax (`CREATE SEMANTIC VIEW`) | More natural than table function; integrates with DuckDB's schema object model | — Pending |
| SQL DDL before YAML | SQL is simpler to implement as a single interface; YAML adds a second definition path | — Pending |
| Expansion-only v0.1 | Pre-aggregation is orthogonal complexity; ship the semantic layer first | — Pending |
| DuckDB is the execution engine | Extension is a preprocessor; avoids building a query engine | ✓ Good |
| Sidecar file for v0.1 catalog persistence | DuckDB locks prevent SQL from scalar `invoke`; `try_clone` deadlocks, `Connection::open` blocks on file lock. Sidecar (plain JSON file I/O) is the only deadlock-free pure-Rust option. v0.2 C++ shim replaces this with `pragma_query_t` (FTS pattern). | ✓ Accepted (temporary) |

---
*Last updated: 2026-02-23 after initialization*
