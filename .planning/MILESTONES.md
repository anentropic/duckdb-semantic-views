# Milestones

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

