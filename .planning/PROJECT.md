# DuckDB Semantic Views

## What This Is

A DuckDB extension written in Rust that implements semantic views — a declarative layer for defining measures, dimensions, relationships, facts, and derived metrics directly in DuckDB. Users register semantic views via native `CREATE SEMANTIC VIEW` DDL with SQL keyword clauses (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS), then query them with `FROM semantic_view('view', dimensions := [...], metrics := [...])`. The extension expands semantic view references into concrete SQL (with GROUP BY, JOINs from PK/FK declarations, typed output columns, fan trap detection, role-playing dimension support, semi-additive snapshot aggregation, and window function metrics) and hands the result to DuckDB for execution. Full Snowflake-aligned introspection via SHOW/DESCRIBE commands, SHOW TERSE/COLUMNS, IN SCHEMA/DATABASE scoping, GET_DDL round-trip, and metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC).

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
- ✓ HIERARCHIES clause: drill-down path metadata with define-time dimension validation — v0.5.3 (removed in quick task 260318-fzu)
- ✓ Derived metrics: metric-on-metric composition with stacked inlining and aggregate prohibition — v0.5.3
- ✓ Fan trap detection: cardinality-aware blocking errors for one-to-many aggregation fan-out — v0.5.3
- ✓ Role-playing dimensions: same table via multiple named relationships with scoped aliases — v0.5.3
- ✓ USING RELATIONSHIPS: explicit join path selection per metric with ambiguity detection — v0.5.3
- ✓ DESCRIBE extended to 8 columns (facts + hierarchies) with backward-compatible null-to-[] fallback — v0.5.3

- ✓ UNIQUE table constraint + Snowflake-style cardinality inference (remove explicit cardinality keywords) — v0.5.4
- ✓ Multi-version DuckDB support (main=1.5.x latest, duckdb/1.4.x LTS) with dual CI — v0.5.4
- ✓ DDL surface parity with Snowflake: ALTER SEMANTIC VIEW RENAME TO, SHOW SEMANTIC DIMENSIONS/METRICS/FACTS, FOR METRIC (fan-trap-aware) — v0.5.4
- ✓ SHOW command filtering: LIKE, STARTS WITH, LIMIT on all SHOW SEMANTIC commands — v0.5.4
- ✓ Sphinx + Shibuya documentation site on GitHub Pages with CI/CD — v0.5.4
- ✓ Community Extension registry descriptor + MAINTAINER.md with multi-branch workflow — v0.5.4
- ✓ Module refactoring: expand.rs (4,299 lines) and graph.rs (2,333 lines) split into module directories with single-responsibility submodules — v0.5.5
- ✓ Shared utility extraction: util.rs and errors.rs leaf modules breaking circular dependencies — v0.5.5
- ✓ Metadata storage: created_on timestamp, database_name, schema_name captured at define time with backward-compatible deserialization — v0.5.5
- ✓ Snowflake-aligned SHOW commands: 5-column SHOW VIEWS, 6-column SHOW DIMS/METRICS/FACTS, 4-column SHOW DIMS FOR METRIC with BOOLEAN required — v0.5.5
- ✓ Snowflake-aligned DESCRIBE: property-per-row format with 5 columns and 6 object kinds — v0.5.5
- ✓ Persistence hardened: TOCTOU fix (single write lock), parameterized prepared statements replacing string interpolation — v0.5.5
- ✓ COMMENT annotation on views and all objects (tables, dimensions, metrics, facts) in CREATE DDL — v0.6.0
- ✓ WITH SYNONYMS annotation on all objects in CREATE DDL — v0.6.0
- ✓ PRIVATE/PUBLIC access modifiers on facts and metrics; PRIVATE items excluded from query results — v0.6.0
- ✓ Backward-compatible metadata persistence: pre-v0.6.0 stored views load without error — v0.6.0
- ✓ SHOW output includes synonyms and comment columns populated from stored metadata — v0.6.0
- ✓ DESCRIBE output includes COMMENT, SYNONYMS, and ACCESS_MODIFIER properties — v0.6.0
- ✓ SHOW SEMANTIC VIEWS IN SCHEMA/DATABASE scope filtering — v0.6.0
- ✓ SHOW TERSE SEMANTIC VIEWS reduced column set — v0.6.0
- ✓ SHOW COLUMNS IN SEMANTIC VIEW unified dims/facts/metrics with kind column — v0.6.0
- ✓ ALTER SEMANTIC VIEW SET/UNSET COMMENT — modify view-level comments after creation — v0.6.0
- ✓ GET_DDL('SEMANTIC_VIEW', 'name') — round-trip DDL reconstruction from stored definitions — v0.6.0
- ✓ Wildcard dimension/metric selection (table_alias.*) with PRIVATE exclusion — v0.6.0
- ✓ Queryable FACTS — row-level unaggregated query mode via facts parameter, mutual exclusion with metrics — v0.6.0
- ✓ Semi-additive metrics (NON ADDITIVE BY) — CTE-based ROW_NUMBER snapshot selection, effectively-regular classification, conditional aggregation for mixed queries, fan trap skip — v0.6.0
- ✓ Window function metrics (PARTITION BY EXCLUDING) — CTE-based inner aggregation + outer window SELECT, EXCLUDING resolution at query time, window/aggregate mixing error, fan trap skip, SHOW DIMS required=TRUE — v0.6.0
- ✓ Security & correctness hardening: FFI catch_unwind on all 25 entry points, graceful lock-poison handling, cycle detection + MAX_DERIVATION_DEPTH=64 for derived metrics/facts, bounds-checked test helpers — v0.6.0
- ✓ Code quality: unit tests for join_resolver/fan_trap/facts (38 new tests), resolve_names generic helper, DimensionName/MetricName newtypes with case-insensitive semantics, NaGroup named struct, dead code removal, property-based test assertions — v0.6.0

### Active

## Current Milestone: v0.7.0 YAML Definitions & Materialization Routing

**Goal:** Add YAML as a second definition format (mirroring the existing JSON schema) alongside SQL DDL, and a materialization routing engine that transparently redirects queries to pre-existing aggregated tables when they cover the requested dimensions and metrics.

**Target features:**
- YAML definition format: inline `FROM YAML $$ ... $$` and file-based `FROM YAML FILE '...'`, mirroring existing SemanticViewDefinition JSON schema
- GET_DDL round-trip export to YAML format
- Materialization routing engine: new MATERIALIZATIONS clause declaring pre-existing aggregated tables with covered dimensions and metrics
- Transparent query routing to materializations when they cover the requested dims+metrics, with re-aggregation for subset matches
- Fallback to raw table expansion when no materialization matches (current behavior)

### Out of Scope

- Pre-aggregation / materialization selection — deferred to future milestone
- YAML definition format — SQL DDL first; YAML is a future path
- Fan trap auto-deduplication — changes query semantics; detection + blocking is the 80/20
- ASOF / temporal relationships — complex temporal join semantics; standard equi-joins cover 95% of cases
- Aggregate facts (COUNT in FACTS) — blurs row-level boundary; aggregation belongs in METRICS
- Cross-view optimisation (sharing materialised tables across views) — non-goal by design
- Custom query engine — DuckDB is the engine; the extension is a preprocessor only
- BI tool HTTP API — not a DuckDB extension concern; Cube.dev handles this use case
- Column-level security — beyond row-level filter scope; DuckDB handles column access
- Fiscal calendar / Sunday-start weeks — ISO 8601 only for now
- Backward compatibility for old DDL syntax — pre-release; finding the right design
- Cube.dev-style Dijkstra path selection — explicit USING is more deterministic and Snowflake-aligned

## Context

**Shipped v0.6.0** (2026-04-14) — Snowflake SQL DDL Parity: metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC), ALTER SET/UNSET COMMENT, GET_DDL round-trip, semi-additive metrics (NON ADDITIVE BY with CTE-based snapshot selection), window function metrics (PARTITION BY EXCLUDING), queryable FACTS (row-level mode), wildcard selection (table_alias.*), SHOW TERSE/COLUMNS/IN SCHEMA/DATABASE, FFI catch_unwind on all 25 entry points, DimensionName/MetricName newtypes. 8 phases (43-50), 16 plans, 34 requirements satisfied.
**Tech stack:** Rust + C++ shim (vendored DuckDB amalgamation via cc crate), duckdb-rs 1.10500.0 (DuckDB 1.5.0), serde_json, strsim, proptest.
**Architecture:** Extension is a preprocessor — expands semantic view queries into concrete SQL with typed output columns, fan trap detection, role-playing dimension support, semi-additive snapshot aggregation, and window function metrics. DuckDB handles all execution. Query results stream via zero-copy vector references (`duckdb_vector_reference_vector`). Persistence via `pragma_query_t` with parameterized prepared statements and single-lock check-and-mutate pattern. Parser hook via C++ shim with `parser_extension_compat.hpp`: `parse_function` fallback detects all DDL forms (CREATE, DROP, ALTER, DESCRIBE, SHOW variants), Rust `DdlKind` enum dispatches rewrite. DDL body parsed by `body_parser.rs` state machine into TableRef/Join/Dimension/Metric/Fact structs with PK/FK/UNIQUE annotations, metadata (COMMENT/SYNONYMS/PRIVATE), and inferred cardinality. `RelationshipGraph` validates tree structure and topologically sorts joins. Expansion generates `FROM base AS alias LEFT JOIN t AS alias ON pk=fk` with qualified column references, fact inlining, derived metric resolution, USING-aware scoped aliases, CTE-based semi-additive/window metric pipelines, and wildcard expansion. Code organized into module directories: `expand/` (7 submodules), `graph/` (5 submodules), with shared `util.rs` and `errors.rs` leaf modules. DimensionName/MetricName newtypes with case-insensitive semantics for query resolution.
**Codebase:** 25,983 LOC Rust across src/ and tests/. 705 Rust tests + 32 sqllogictest files + 6 DuckLake CI tests + Python crash repro + Python caret tests + 4 fuzz targets.
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
| Semi-additive via CTE ROW_NUMBER (not LAST_VALUE) | DuckDB LTS crash bug with LAST_VALUE IGNORE NULLS; ROW_NUMBER approach is portable | ✓ Good — correct results, avoids LTS bug |
| Effectively-regular classification for semi-additive | When all NA dimensions are in query, skip CTE and aggregate normally (Snowflake semantics) | ✓ Good — avoids unnecessary CTE overhead |
| Window/aggregate mixing prohibition | Window metrics and aggregate metrics cannot coexist in same query | ✓ Good — clear semantics, avoids ambiguous GROUP BY |
| CTE-based window expansion (inner agg + outer window) | __sv_agg CTE aggregates inner metrics; outer SELECT applies window functions | ✓ Good — correct partition semantics |
| PARTITION BY EXCLUDING as set difference | Window partitions computed at query time by subtracting EXCLUDING dims from queried dims | ✓ Good — flexible, Snowflake-aligned |
| FFI catch_unwind on all entry points | AssertUnwindSafe justified: panics caught at boundary, no partially-mutated state | ✓ Good — prevents UB from Rust panics through C++ |
| MAX_DERIVATION_DEPTH=64 | Prevents stack overflow from linear metric chains passing cycle detection | ✓ Good — safe limit with clear error |
| Graceful lock-poison via map_err | .map_err() with descriptive string instead of into_inner() recovery | ✓ Good — simpler than recovery, correct for crash scenarios |
| DimensionName/MetricName newtypes | Case-insensitive Eq/Hash via AsRef<str> + Deref for seamless interop | ✓ Good — compile-time guarantees, consolidated comparison |
| resolve_names generic helper (9 closure params) | Closures for error construction avoid trait objects while deduplicating 4 resolution loops | ✓ Good — -150 LOC, single responsibility |
| Module directory pattern with mod.rs re-exports | Submodules use pub(super) for internal sharing; mod.rs re-exports full prior public API | ✓ Good — zero-breakage split of 6,632 lines |
| Define-time metadata capture via DuckDB SQL | `now()`, `current_database()`, `current_schema()` via catalog_conn; not stored in JSON to avoid stale aliases | ✓ Good — accurate context metadata |
| Property-per-row DESCRIBE format | Snowflake-aligned: each row is one property of one object; replaces single-row JSON blob | ✓ Good — standard introspection pattern |
| Parameterized prepared statements for persistence | `duckdb_prepare`/`duckdb_bind_varchar` replacing string interpolation in all write paths | ✓ Good — eliminates SQL injection surface |
| Single write lock for catalog mutations | Check-and-mutate under one lock eliminates TOCTOU race in catalog_insert/catalog_delete | ✓ Good — correct concurrency pattern |
| Snowflake-style cardinality inference | UNIQUE constraints + PK/FK matching replaces explicit cardinality keywords; two-variant enum (ManyToOne/OneToOne) | ✓ Good — cleaner model, aligns with Snowflake |
| Separate TU with compat header for DuckDB 1.5.0 | parser_extension_compat.hpp re-declares types moved from duckdb.hpp to duckdb.cpp | ✓ Good — avoids libpg_query macro pollution |
| Per-process sqllogictest execution | DuckDB 1.5.0 parser extension lifecycle causes segfaults in multi-database processes | ✓ Good — necessary workaround |
| Parser-level SHOW filtering | WHERE/LIMIT injection into generated SQL vs VTab-level filtering | ✓ Good — zero VTab changes needed |
| MIT license (replacing BSD-3-Clause) | Simpler, more permissive, matches Cargo.toml canonical license | ✓ Good — cleaner for CE submission |
| PLACEHOLDER_COMMIT_SHA in description.yml | Replaced with real SHA after squash-merge to main | ✓ Good — correct workflow for CE submission |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd:transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd:complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-04-18 after Phase 51 (YAML Parser Core) complete — yaml_serde dependency, from_yaml/from_yaml_with_size_cap, PartialEq derives, 716 tests green.*
