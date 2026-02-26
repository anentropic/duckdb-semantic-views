# Requirements: DuckDB Semantic Views

**Defined:** 2026-02-23
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## v0.1 Requirements

### Infrastructure

- [x] **INFRA-01**: Extension scaffold is built using `duckdb/extension-template-rs` with a CMake + Cargo build system producing correctly-exported C symbols
- [x] **INFRA-02**: Multi-platform CI build matrix covers Linux x86_64/arm64, macOS x86_64/arm64, and Windows x86_64
- [x] **INFRA-03**: Scheduled CI job builds against the latest DuckDB release; on failure, opens a GitHub PR mentioning @copilot to investigate and fix the breakage
- [x] **INFRA-04**: CI includes a `LOAD` smoke test (not just `cargo test`) to catch DuckDB ABI version mismatches

### Definition Interface (DDL)

- [x] **DDL-01**: User can register a semantic view via `SELECT define_semantic_view('name', '{definition_json}')`
- [x] **DDL-02**: User can remove a semantic view via `SELECT drop_semantic_view('name')`
- [x] **DDL-03**: User can list all registered semantic views via `FROM list_semantic_views()`
- [x] **DDL-04**: User can inspect a semantic view definition via `FROM describe_semantic_view('name')`
- [x] **DDL-05**: Semantic view definitions persist across DuckDB restarts, stored in a `_semantic_views_catalog` table within the user's `.duckdb` file

### Semantic Model

- [x] **MODEL-01**: User can define named dimensions as arbitrary SQL column expressions (e.g., `region`, `date_trunc('month', created_at) AS month`)
- [x] **MODEL-02**: User can define named metrics as aggregation expressions (e.g., `sum(revenue) AS total_revenue`, `count(*) AS orders`, `count(DISTINCT user_id) AS unique_users`)
- [x] **MODEL-03**: User can specify a base table and define explicit JOIN relationships between multiple source entities
- [x] **MODEL-04**: User can define row-level filter conditions that are always applied when the view is queried

### Query Interface

- [x] **QUERY-01**: User can query a semantic view with named array parameters: `FROM my_view(dimensions := ['region', 'category'], metrics := ['total_revenue'])`
- [x] **QUERY-02**: User-supplied WHERE clauses are AND-composed with the view's row-level filters (user filters do not replace view filters)
- [x] **QUERY-03**: `SELECT *` on a semantic view returns all requested dimensions and metrics; schema is inferred correctly at bind time
- [x] **QUERY-04**: Users can inspect the expanded SQL for a semantic view query via `explain_semantic_view()` for debugging and transparency

### Expansion Engine

- [x] **EXPAND-01**: Extension automatically generates a `GROUP BY` clause containing all requested dimensions
- [x] **EXPAND-02**: Extension infers `JOIN` clauses from the entity relationships defined in the semantic view
- [x] **EXPAND-03**: Extension validates dimension and metric names at query time; invalid member names produce a clear error identifying the semantic view and the unknown member name
- [x] **EXPAND-04**: All generated SQL identifiers are quoted to prevent reserved-word conflicts and SQL injection via user-supplied view and column names

### Test Coverage

- [x] **TEST-01**: Unit tests cover the expansion engine (GROUP BY inference, JOIN generation, SQL emission) without requiring a DuckDB runtime
- [x] **TEST-02**: Property-based tests (using `proptest`) cover expansion engine invariants: all requested dimensions appear in GROUP BY, emitted SQL is syntactically valid
- [x] **TEST-03**: Integration tests load the extension in-process, create semantic views, run real DuckDB SQL queries, and assert correct results
- [x] **TEST-04**: Integration test suite includes at least one test scenario using an Apache Iceberg table source
- [x] **TEST-05**: Fuzz targets (using `cargo-fuzz`) cover the unsafe C FFI boundary and the SQL generation path

### Code Quality

- [x] **STYLE-01**: `rustfmt` is configured with a project-level `rustfmt.toml`; formatting violations fail CI
- [x] **STYLE-02**: `clippy` with pedantic lints is enforced; lint violations fail CI

### Documentation

- [x] **DOCS-01**: `MAINTAINER.md` covers: dev environment setup, build instructions, running tests, loading the extension in a DuckDB shell, updating the DuckDB version pin, running the fuzzer, and publishing to the community extension registry

## v0.2 Requirements

Deferred to next milestone. Not in current roadmap.

### Query Interface

- **QUERY-V2-01**: Native `CREATE SEMANTIC VIEW` DDL syntax (requires C++ shim for DuckDB parser hooks)
- **QUERY-V2-02**: Time dimensions with explicit granularity coarsening (day → week → month → year queries)
- **QUERY-V2-03**: Native `EXPLAIN FROM semantic_query(...)` shows expanded SQL instead of physical plan (requires C++ shim for EXPLAIN hook interception)

### Distribution

- **DIST-V2-01**: Published to DuckDB community extension registry (`INSTALL semantic_views FROM community`)
- **DIST-V2-02**: Real-world demo using TPC-H or similar dataset with documented example queries

## Future Milestone Requirements

Not scheduled. Acknowledged for future planning.

### Semantic Model

- **MODEL-FUT-01**: Derived/ratio metrics that reference other metrics (e.g., `profit_margin = revenue - cost`)
- **MODEL-FUT-02**: Hierarchies with drill-down paths (e.g., country → region → city)
- **MODEL-FUT-03**: YAML definition format as an alternative to SQL DDL

### Performance

- **PERF-FUT-01**: Pre-aggregation selection: match semantic queries to materialized tables and substitute table references
- **PERF-FUT-02**: Multi-stage aggregations (nested aggregations, period-over-period calculations)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Custom query engine | DuckDB is the engine — the extension is a preprocessor only |
| BI tool HTTP API | Not a DuckDB extension concern; Cube.dev handles this use case |
| Cross-view optimisation | Each semantic view expands independently by design; non-goal |
| Column-level security | Beyond the row-level filter scope; DuckDB handles column access |
| e-graph / equality saturation | Solves a different problem (multi-engine SQL normalisation); not needed here |
| Cube Store / custom columnar store | DuckDB's Parquet/Iceberg support is the storage layer |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| INFRA-01 | Phase 1 — Scaffold | Complete |
| INFRA-02 | Phase 1 — Scaffold | Complete |
| INFRA-03 | Phase 1 — Scaffold | Complete |
| INFRA-04 | Phase 1 — Scaffold | Complete |
| STYLE-01 | Phase 1 — Scaffold | Complete |
| STYLE-02 | Phase 1 — Scaffold | Complete |
| DDL-01 | Phase 2 — Storage and DDL | Complete |
| DDL-02 | Phase 2 — Storage and DDL | Complete |
| DDL-03 | Phase 2 — Storage and DDL | Complete |
| DDL-04 | Phase 2 — Storage and DDL | Complete |
| DDL-05 | Phase 2 — Storage and DDL | Complete |
| MODEL-01 | Phase 3 — Expansion Engine | Complete |
| MODEL-02 | Phase 3 — Expansion Engine | Complete |
| MODEL-03 | Phase 3 — Expansion Engine | Complete |
| MODEL-04 | Phase 3 — Expansion Engine | Complete |
| EXPAND-01 | Phase 3 — Expansion Engine | Complete |
| EXPAND-02 | Phase 3 — Expansion Engine | Complete |
| EXPAND-03 | Phase 3 — Expansion Engine | Complete |
| EXPAND-04 | Phase 3 — Expansion Engine | Complete |
| TEST-01 | Phase 3 — Expansion Engine | Complete |
| TEST-02 | Phase 3 — Expansion Engine | Complete |
| QUERY-01 | Phase 4 — Query Interface | Complete |
| QUERY-02 | Phase 4 — Query Interface | Complete |
| QUERY-03 | Phase 4 — Query Interface | Complete |
| QUERY-04 | Phase 4 — Query Interface | Complete |
| TEST-03 | Phase 4 — Query Interface | Complete |
| TEST-04 | Phase 4 — Query Interface | Complete |
| TEST-05 | Phase 5 — Hardening and Docs | Complete |
| DOCS-01 | Phase 5 — Hardening and Docs | Complete |

**Coverage:**
- v0.1 requirements: 28 total
- Mapped to phases: 28
- Unmapped: 0 ✓

---
*Requirements defined: 2026-02-23*
*Last updated: 2026-02-25 after Phase 4 completion*
