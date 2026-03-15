# Milestones

## v0.5.3 Advanced Semantic Features (Shipped: 2026-03-15)

**Phases completed:** 4 phases (29-32), 8 plans, 17 tasks
**Source changes:** 126 files, +18,130 / -2,921 lines
**Commits:** 66
**Tests:** 441 Rust tests + 11 sqllogictest files + 6 DuckLake CI
**Timeline:** 2 days (2026-03-13 → 2026-03-15)

**Delivered:** Advanced semantic modeling capabilities — FACTS clause for named row-level sub-expressions, derived metrics with metric-on-metric composition, hierarchies for drill-down path metadata, cardinality-aware fan trap detection blocking inflated aggregation, role-playing dimensions via multiple named relationships with scoped aliases, and USING RELATIONSHIPS for explicit join path selection per metric.

**Key accomplishments:**
1. FACTS clause with DAG resolution via Kahn's algorithm and word-boundary-safe expression inlining
2. HIERARCHIES clause as pure metadata with define-time validation against declared dimensions
3. Derived metrics with stacked inlining, aggregate prohibition, and transitive join resolution
4. Fan trap detection with LCA-based tree path analysis blocking one-to-many aggregation fan-out
5. Role-playing dimensions with scoped aliases ({alias}__{rel_name}) for same table via multiple relationships
6. USING RELATIONSHIPS for explicit join path selection with ambiguity detection and transitive inheritance

**Requirements:** 24/24 satisfied
**Audit:** Passed — all requirements triple-confirmed across VERIFICATION.md, SUMMARY.md, REQUIREMENTS.md. 2 minor tech debt items (approved FAN-02/03 deviation, proptest arb_view_name edge case).

---

## v0.5.2 SQL DDL & PK/FK Relationships (Shipped: 2026-03-13)

**Phases completed:** 5 active phases (24-28, Phase 24 cancelled), 14 plans
**Source changes:** 144 files, +17,702 / -4,437 lines
**Commits:** 89
**Tests:** 282 Rust tests + 7 sqllogictest files + 6 DuckLake CI + 13 Python crash repro + 3 Python caret
**Timeline:** 5 days (2026-03-09 → 2026-03-13)

**Delivered:** SQL keyword body syntax (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS) replacing function-call DDL, Snowflake-style PK/FK relationship model with automatic JOIN ON synthesis, topological sort ordering, define-time graph validation, and qualified column references. Function-based DDL interface fully retired; native DDL is sole interface.

**Key accomplishments:**
1. SQL keyword body parser with state-machine clause boundary detection for TABLES, RELATIONSHIPS, DIMENSIONS, METRICS
2. Parser robustness hardening: token-based DDL detection, adversarial input safety, fuzz_ddl_parse target
3. RelationshipGraph module with Kahn's algorithm toposort, diamond/cycle detection, define-time validation
4. Alias-based FROM+JOIN expansion with qualified column refs, replacing CTE flattening pattern
5. Function DDL retired: DefineSemanticViewVTab + parse_args.rs removed; native DDL is sole interface
6. README rewritten with AS-body PK/FK syntax examples; 3-table E2E integration test

**Requirements:** 16/16 active satisfied, 6 cancelled (DDL-06 won't-do, MDL-01-05 delivered implicitly)
**Audit:** Tech debt — no blockers, 2 minor cleanup items (stale doc comment, stale REQUIREMENTS checkboxes)

---

## v0.5.1 DDL Polish (Shipped: 2026-03-09)

**Phases completed:** 5 phases (19-23), 9 plans
**Source changes:** 71 files, +12,112 / -1,800 lines
**Tests:** 222 total (Rust unit + proptest + sqllogictest + DuckLake CI + Python integration)
**Timeline:** 2 days (2026-03-08 → 2026-03-09)

**Delivered:** Complete native DDL surface — all 7 DDL verbs (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW) via parser extension hooks. Error location reporting with clause-level hints, character positions, and "did you mean" suggestions. 33 property-based tests for parser module + Python caret integration tests through full extension pipeline.

**Key accomplishments:**
1. Empirically confirmed all 7 DDL prefixes trigger parser fallback hook — full native DDL scope validated
2. `DdlKind` enum with multi-prefix detection/rewrite for DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW
3. C++ result-forwarding pipeline with dynamic column schemas per DDL form (DESCRIBE: 6 cols, SHOW: 2 cols)
4. Full error reporting — clause hints, byte-accurate positions, "did you mean" suggestions via tri-state FFI
5. README updated with DDL syntax reference and full lifecycle example (create → query → describe → show → drop)
6. 33 property-based tests for all 7 parser functions + Python caret integration test through full extension pipeline

**Requirements:** 16/16 satisfied
**Audit:** Passed — all requirements triple-confirmed across VERIFICATION.md, SUMMARY.md, REQUIREMENTS.md

---

## v0.5.0 Parser Extension Spike (Shipped: 2026-03-08)

**Phases completed:** 5 phases (15-18, including 17.1), 8 plans
**Source changes:** 14 files, +1,769 / -112 lines
**Commits:** 45
**Timeline:** 2 days (2026-03-07 → 2026-03-08)

**Delivered:** Native `CREATE SEMANTIC VIEW` DDL syntax via DuckDB parser extension hooks. C++ shim compiled via `cc` crate against vendored DuckDB amalgamation, with Rust FFI trampoline for statement detection and rewriting. Extension preserves full backward compatibility with function-based DDL.

**Key accomplishments:**
1. Vendored DuckDB amalgamation + cc crate C++ build pipeline for parser hook compilation
2. C_STRUCT entry point + C++ helper for parser hook registration (Option A — CPP entry rejected due to `-fvisibility=hidden`)
3. Rust FFI parse function with `catch_unwind` panic safety and C++ trampoline for `CREATE SEMANTIC VIEW` detection
4. Native DDL execution via statement rewriting to function-based DDL with dedicated DDL connection
5. Runtime type validation + defensive SQL wrapping preventing Python client crashes
6. Registry-ready binary verified: C_STRUCT_UNSTABLE ABI, 172 tests green, no CMake dependency

**Requirements:** 18/18 functionally satisfied (5 had verification documentation gap, resolved by downstream phases)

**Known Gaps:**
- Phase 15 VERIFICATION.md was retroactively created (gaps confirmed resolved by Phase 16-18 work)
- Nyquist compliance: all 5 phases have VALIDATION.md but none formally marked `nyquist_compliant: true`

---

## v0.4.0 Simplified Dimensions (Shipped: 2026-03-03)

**Delivered:** Breaking change — removed `time_dimensions` DDL parameter and `granularities` query parameter. Time truncation is now expressed via the dimension `expr` directly (e.g., `date_trunc('month', created_at)`). Simplified DDL from 6 to 4 named params (`tables`, `relationships`, `dimensions`, `metrics`) and query function from 3 to 2 named params (`dimensions`, `metrics`). Removed `dim_type`, `granularity` from `Dimension` struct and `granularity_overrides` from `QueryRequest`.

---

## v0.3.0 Zero-Copy Query Pipeline (Shipped: 2026-03-03)

**Delivered:** Replaced binary-read dispatch with zero-copy vector references. The table function now streams result chunks directly into output via `duckdb_vector_reference_vector`, eliminating ~600 lines of per-type read/write code. Type mismatches (HUGEINT→BIGINT, STRUCT/MAP→VARCHAR) handled at SQL generation time via `build_execution_sql` cast wrapper. Streaming is chunk-by-chunk instead of collect-all-then-write.

**Key changes:**
1. Zero-copy vector transfer — `duckdb_vector_reference_vector` shares buffer ownership between source and output chunks
2. Streaming execution — chunks processed one at a time via `StreamingState` with `Mutex`, reducing peak memory
3. SQL-time type casting — `build_execution_sql` wraps expanded SQL with explicit casts for mismatched columns
4. Removed: `TypedValue` enum, `read_typed_from_vector`, `read_varchar_from_raw_vector`, `write_typed_column` (all ~600 LOC)
5. New tests: `tests/vector_reference_test.rs` validates lifetime safety, multi-chunk streaming, and complex types (LIST, STRUCT)

**LOC:** 5,660 Rust (src) + 1,492 (tests) — net reduction of ~600 LOC from v0.2.0

---

## v0.2.0 Native DDL + Time Dimensions (Shipped: 2026-03-03)

**Phases completed:** 8 phases (8-14, including 11.1), 25 plans
**Lines of code:** 7,462 Rust (across 21 files)
**Commits:** 125
**Tests:** 136 (unit + proptest + sqllogictest + DuckLake CI)
**Timeline:** 3 days (2026-02-28 → 2026-03-02)

**Delivered:** Typed DDL interface with Snowflake-aligned STRUCT/LIST syntax, time dimension support with granularity coarsening, DuckDB-native catalog persistence via pragma_query_t, typed output columns with binary-read dispatch, and DuckLake CI integration.

**Key accomplishments:**
1. C++ shim infrastructure — cc crate build with vendored DuckDB headers, feature-gated compilation, symbol visibility
2. Time dimensions — date_trunc codegen with granularity coarsening (day/week/month/year) and per-query override
3. pragma_query_t catalog persistence — transactional DuckDB-native storage, sidecar file fully eliminated
4. Snowflake-aligned DDL — 6-arg STRUCT/LIST `create_semantic_view` syntax, `semantic_view` table function rename
5. Typed output columns — binary-read dispatch replacing all-VARCHAR, 36 property-based tests for type dispatch
6. DuckLake CI integration — test refresh to v0.2.0 API, parallel CI job, DuckDB version monitor

**Requirements:** 14/16 satisfied
**Known gaps:**
- DDL-01 (`CREATE SEMANTIC VIEW` native syntax) — architecturally impossible: Python DuckDB compiles C++ with `-fvisibility=hidden`, hiding all parser hook symbols
- DDL-02 (`DROP SEMANTIC VIEW` native syntax) — same root cause as DDL-01

---

## v1.0 MVP (Shipped: 2026-02-28)

**Phases completed:** 7 phases, 18 plans
**Lines of code:** 6,628 Rust
**Commits:** 99
**Timeline:** 6 days (2026-02-22 → 2026-02-28)

**Delivered:** A fully functional DuckDB extension in Rust implementing semantic views — users define dimensions, metrics, joins, and filters once, then query with `FROM view(dimensions := [...], metrics := [...])` without writing GROUP BY or JOIN logic by hand.

**Key accomplishments:**
1. Loadable DuckDB extension in Rust with multi-platform CI (5 targets) and automated DuckDB version monitoring
2. Function-based DDL (define/drop/list/describe) with sidecar-file persistence across restarts
3. Pure Rust expansion engine with GROUP BY inference, join dependency resolution, and identifier quoting
4. `semantic_query` table function with FFI SQL execution via independent DuckDB connection
5. Three cargo-fuzz targets, proptest property-based tests, and comprehensive MAINTAINER.md
6. Tech debt cleanup and formal verification with TECH-DEBT.md documenting accepted decisions

**Requirements:** 28/28 satisfied
**Audit:** Passed with tech debt — all requirements met, 15 deferred items documented

---

