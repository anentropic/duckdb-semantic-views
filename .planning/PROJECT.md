# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, relationships, facts, hierarchies, and derived metrics directly in DuckDB. Users register semantic views via native `CREATE SEMANTIC VIEW` DDL with SQL keyword clauses (TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS), then query them with `FROM semantic_view('view', dimensions := [...], metrics := [...])`. The extension expands semantic view references into concrete SQL (with GROUP BY, JOINs from PK/FK declarations, typed output columns, fan trap detection, and role-playing dimension support) and hands the result to DuckDB for execution.

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
- ✓ FACTS clause: named row-level sub-expressions with DAG validation and word-boundary-safe inlining — v0.5.3
- ✓ HIERARCHIES clause: drill-down path metadata with define-time dimension validation — v0.5.3
- ✓ Derived metrics: metric-on-metric composition with stacked inlining and aggregate prohibition — v0.5.3
- ✓ Fan trap detection: cardinality-aware blocking errors for one-to-many aggregation fan-out — v0.5.3
- ✓ Role-playing dimensions: same table via multiple named relationships with scoped aliases — v0.5.3
- ✓ USING RELATIONSHIPS: explicit join path selection per metric with ambiguity detection — v0.5.3
- ✓ DESCRIBE extended to 8 columns (facts + hierarchies) with backward-compatible null-to-[] fallback — v0.5.3

### Active

(Next milestone requirements to be defined via `/gsd:new-milestone`)

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone
- YAML definition format — SQL DDL first; YAML is a future path
- Fan trap auto-deduplication — changes query semantics; detection + blocking is the 80/20
- Window function metrics — requires expansion without GROUP BY; orthogonal to aggregation model
- ASOF / temporal relationships — complex temporal join semantics; standard equi-joins cover 95% of cases
- Aggregate facts (COUNT in FACTS) — blurs row-level boundary; aggregation belongs in METRICS
- Cross-view optimisation (sharing materialised tables across views) — non-goal by design
- Custom query engine — DuckDB is the engine; the extension is a preprocessor only
- BI tool HTTP API — not a DuckDB extension concern; Cube.dev handles this use case
- Column-level security — beyond row-level filter scope; DuckDB handles column access
- Fiscal calendar / Sunday-start weeks — ISO 8601 only for now
- Backward compatibility for old DDL syntax — pre-release; finding the right design
- Cross-view hierarchies — keep hierarchies within single semantic view's dimension space
- Cube.dev-style Dijkstra path selection — explicit USING is more deterministic and Snowflake-aligned

## Context

**Shipped v0.5.3** — FACTS clause, derived metrics, hierarchies, fan trap detection, role-playing dimensions, USING RELATIONSHIPS. All advanced semantic modeling features complete.
**Tech stack:** Rust + C++ shim (vendored DuckDB amalgamation via cc crate), duckdb-rs 1.4.4, serde_json, strsim, proptest.
**Architecture:** Extension is a preprocessor — expands semantic view queries into concrete SQL with typed output columns, fan trap detection, and role-playing dimension support. DuckDB handles all execution. Query results stream via zero-copy vector references (`duckdb_vector_reference_vector`). Persistence via `pragma_query_t` with separate connection (write-first pattern). Parser hook via C++ shim: `parse_function` fallback detects all 7 DDL forms, Rust `DdlKind` enum dispatches rewrite. DDL body parsed by `body_parser.rs` state machine into TableRef/Join/Dimension/Metric/Fact/Hierarchy structs with PK/FK annotations and cardinality. `RelationshipGraph` validates tree structure and topologically sorts joins. Expansion generates `FROM base AS alias LEFT JOIN t AS alias ON pk=fk` with qualified column references, fact inlining, derived metric resolution, and USING-aware scoped aliases.
**Tests:** 441 Rust tests + 11 sqllogictest files + 6 DuckLake CI tests + Python crash repro + Python caret tests + 4 fuzz targets.
**Known limitations:** See TECH-DEBT.md at repo root for accepted decisions and deferred items.
**Source LOC:** 13,451 Rust (src/).

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
| FACTS reuse parse_qualified_entries | Same alias.name AS expr pattern as dims/metrics; consistent syntax | ✓ Good — no new parser needed |
| Hierarchies as pure metadata | Only validated against dimensions, not used in expansion | ✓ Good — clean separation of concerns |
| Word-boundary matching for expression substitution | is_word_boundary_byte prevents substring collisions in fact/metric inlining | ✓ Good — correct identifier handling |
| Fact/derived metric DAG via Kahn's algorithm | Same toposort pattern as relationship graph; naturally detects cycles | ✓ Good — consistent algorithm choice |
| Fan trap detection as blocking error | User chose blocking errors over warnings for one-to-many fan-out | ✓ Good — safer default |
| LCA-based tree path analysis for fan traps | Parent-walking + lowest common ancestor to check edge cardinalities | ✓ Good — correct graph analysis |
| Scoped aliases for role-playing ({alias}__{rel_name}) | Double-underscore separator for uniqueness; aliases are quoted | ✓ Good — deterministic disambiguation |
| Diamond relaxation for named relationships | Allow multiple paths when all relationships have unique names | ✓ Good — enables role-playing pattern |
| USING controls dimension alias, not metric aggregation | COUNT(*) counts base rows regardless of USING path | ✓ Good — correct semantics |
| Semi-additive metrics deferred to v0.5.4 | Only feature requiring expansion pipeline structural change | — Pending |

---
*Last updated: 2026-03-15 after v0.5.3 milestone*
