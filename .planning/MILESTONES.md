# Milestones

## v0.8.0 Transactional DDL & Architectural Unification (Shipped: 2026-05-06)

**Phases completed:** 5 phases (58-62), 8 plans
**Timeline:** 2026-05-02 → 2026-05-06 (5 days, of which phases 58-61 were retroactive reconstruction of ad-hoc work consolidated 2026-05-05)
**Audit:** ✅ passed (5/5 phases, 7/7 integration, 0/0 requirements — interior architecture milestone with no REQ-IDs)

**Delivered:** All `CREATE` / `DROP` / `ALTER SEMANTIC VIEW` DDL forms now participate in the caller's transaction via a single `parser_override` entry point. Legacy `parse_function` / `sv_ddl_internal` table-function fallback retired (~1500 LOC net deletion). Race guards, FFI UTF-8 hardening, multi-DB isolation, and parser-error caret rendering all delivered as paired structural improvements.

**Key accomplishments:**

1. **Transactional DDL (Phase 58)** — All four `CREATE` body variants (keyword AS, FROM YAML inline, FROM YAML FILE, OR REPLACE / IF NOT EXISTS) plus DROP and ALTER participate in the caller's transaction. `parser_override` rewrites recognised DDL into native INSERT/UPDATE/DELETE against `_definitions` and re-parses on the caller's connection. CatalogState HashMap removed; single CatalogReader path; heap-owned FFI buffers; per-load db_token; ADBC end-to-end test added.
2. **Architectural unification (Phase 59)** — `parser_override` becomes the sole DDL entry; legacy `parse_function` / `sv_ddl_internal` table-function fallback retired (~1500 LOC removed). Single execution path → uniform transactional semantics + uniform Bison/PEG parser compatibility.
3. **Race guards & FFI hardening (Phase 60)** — Two-statement DROP/ALTER guard (`SELECT CASE WHEN NOT EXISTS THEN error() ELSE TRUE; DELETE … RETURNING`) emits `semantic view '<name>' was concurrently dropped` if a writer races between snapshot and apply. Checked `from_utf8` on FFI input. `parse_table_function_call` rejects malformed argument shapes. `static_assert` pins `ParserOptions` size.
4. **Bounded multi-DB isolation, RAII, tests & docs (Phase 61)** — Per-DB token→catalog map capped at 16 entries (TECH-DEBT 20, deliberately introduced as known limitation). `CatalogReader` adopts RAII guards (`PreparedStmt`, `QueryResult`). ADBC + concurrent-CREATE Python tests, `INSERT OR REPLACE` row-count + byte-identical rollback sqllogictest, type-inference inside transaction, FFI fuzz target.
5. **Caret restoration + LRU removal (Phase 62)** — TECH-DEBT 22 + 20 resolved structurally. `parse_function` reintroduced purely as the error-reporting layer (`parser_override` keeps the success path); validation errors render as `Parser Error: ... LINE 1: ... ^` with the caret aligned to the offending token. The bounded LRU is gone — `OverrideContext` direct-attached to `SemanticViewsParserInfo`, lifetime-tied to `DBConfig`. New actionable error when `allow_parser_override_extension` is `DEFAULT`/`STRICT` (e.g. after `disable_peg_parser`).
6. **Test infrastructure** — 7 sqllogictest fixtures pinning caret rendering across CREATE/DROP/ALTER/multi-line/UTF-8/multi-DB/extension-reload paths; tightened Python caret assertions (column-position now asserted); 17-DB and 50-DB sequential isolation tests; rc=3 actionable-error case in `peg_compat.test`; `static_assert` on `ParserExtensionParseResult` layout.
7. **Documentation** — New `docs/explanation/transactional-ddl-and-limitations.rst` explanation page; transactional notes added to CREATE / DROP / ALTER / DESCRIBE / SHOW reference pages; FROM YAML FILE transactional caveat documented; "Transactional DDL" subsection in Snowflake comparison; concurrent-DDL error catalogue in `error-messages.rst`.

**Tech debt resolved:** TECH-DEBT 20 (silent LRU eviction class), TECH-DEBT 22 (FALLBACK_OVERRIDE drops `DISPLAY_EXTENSION_ERROR`)
**Tech debt added:** TECH-DEBT 19 (read-side functions see committed state — out of scope), 21 (`disable_peg_parser` resets override setting — out of scope), 23 (cross-process `CREATE IF NOT EXISTS` race surfaces as PK violation — DuckDB API limitation)

**Requirements:** vacuous (0/0) — interior architecture milestone with no REQ-IDs assigned. See `.planning/v0.8.0-MILESTONE-AUDIT.md` for full audit.

---

## v0.7.0 YAML Definitions & Materialization Routing (Shipped: 2026-04-24)

**Phases completed:** 7 phases, 7 plans, 15 tasks

**Key accomplishments:**

- yaml_serde 0.10 integration with from_yaml/from_yaml_with_size_cap, PartialEq on all model structs, 11 unit tests + 256-case proptest proving YAML-JSON equivalence, and fuzz target
- Dollar-quoted FROM YAML syntax wired into DDL parser with full CREATE/REPLACE/IF NOT EXISTS support, cardinality inference, and 21 unit + 13 integration tests
- Two-layer file loading via sentinel protocol: Rust detects FROM YAML FILE syntax, C++ reads file via DuckDB read_text() with automatic enable_external_access enforcement
- MATERIALIZATIONS clause added to semantic view DDL with TABLE/DIMENSIONS/METRICS sub-clauses, YAML support, define-time validation, and backward-compatible persistence
- Pure-function materialization routing engine with exact-match HashSet comparison, semi-additive/window exclusion, and 17 unit + 8 integration test sections
- READ_YAML_FROM_SEMANTIC_VIEW scalar function with field stripping, FQN resolution, and round-trip YAML export
- Materialization awareness added to EXPLAIN, DESCRIBE, and new SHOW SEMANTIC MATERIALIZATIONS command with 7-column VTab pair, parser integration, and 12 new unit tests + 1 sqllogictest file

---

## v0.6.0 Snowflake SQL DDL Parity (Shipped: 2026-04-14)

**Phases completed:** 8 phases (43-50), 16 plans, 35 tasks
**Source changes:** 166 files, +35,682 / -4,023 lines
**Commits:** 114
**Tests:** 705 Rust tests + 32 sqllogictest files + 6 DuckLake CI
**Timeline:** 10 days (2026-04-05 → 2026-04-14)

**Delivered:** Closed all remaining feature gaps against Snowflake's SQL DDL semantic views -- metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC), ALTER DDL, GET_DDL round-trip, semi-additive metrics, window function metrics, queryable FACTS, wildcard selection, and introspection enhancements. Plus security hardening and code quality improvements.

**Key accomplishments:**

1. Metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC) on all DDL objects with backward-compatible persistence
2. Introspection enhancements: SHOW TERSE, IN SCHEMA/DATABASE scoping, SHOW COLUMNS, metadata columns in SHOW/DESCRIBE
3. ALTER SET/UNSET COMMENT + GET_DDL round-trip DDL reconstruction from stored definitions
4. Wildcard selection (table_alias.*) with PRIVATE exclusion + queryable FACTS (row-level unaggregated mode with table path validation)
5. Semi-additive metrics (NON ADDITIVE BY) with CTE-based ROW_NUMBER snapshot selection, effectively-regular classification, and mixed-metric support
6. Window function metrics (PARTITION BY EXCLUDING) with CTE-based inner aggregation + outer window SELECT, mixing guard, and fan trap skip
7. Security hardening: FFI catch_unwind on all 25 entry points, graceful lock-poison handling, cycle detection + MAX_DERIVATION_DEPTH=64
8. Code quality: 38 new unit tests for untested modules, resolve_names generic helper, DimensionName/MetricName newtypes, dead code removal

**Requirements:** 34/34 satisfied
**Audit:** Passed -- all requirements verified across VERIFICATION.md, SUMMARY.md, and implementation. 4 E2E flows validated.

---

## v0.5.5 SHOW/DESCRIBE Alignment & Refactoring (Shipped: 2026-04-05)

**Phases completed:** 6 phases, 11 plans, 12 tasks

**Key accomplishments:**

- Two leaf modules (util.rs, errors.rs) extracted to break expand<->graph and parse<->body_parser circular dependencies with zero behavior changes across 482 tests
- Split expand.rs (4,299 lines) into 7 single-responsibility submodules with 86 tests distributed to correct locations, zero behavior changes
- Split src/graph.rs (2,333 lines) into 5 single-responsibility submodules with mod.rs re-exports, preserving exact public API and all 59 tests
- Snowflake-aligned column schemas for all 4 SHOW SEMANTIC VTabs: list.rs (5-col), show_dims/metrics/facts.rs (6-col each), with expr and source_table columns removed
- 4-column SHOW DIMS FOR METRIC with BOOLEAN required column, plus all 5 sqllogictest files updated for new SHOW command schemas across the full test suite
- Snowflake-aligned property-per-row DESCRIBE SEMANTIC VIEW with 5 VARCHAR columns, 6 object kinds, and comprehensive sqllogictest coverage

---

## v0.5.4 Snowflake-Parity & Registry Publishing (Shipped: 2026-03-27)

**Phases completed:** 6 phases, 12 plans, 25 tasks

**Key accomplishments:**

- UNIQUE constraints on TableRef, ref_columns on Join, two-variant Cardinality enum, and PK/UNIQUE-based cardinality inference at parse time
- FK reference validation (CARD-03/09) via PK/UNIQUE set matching, fan trap with inferred cardinality, ON clause using ref_columns, old-JSON guard, and 14-test phase33 sqllogictest
- Full DuckDB 1.5.0 upgrade: version pins, C++ parser extension compat header, ParserExtension::Register API migration, per-process test runner, PEG parser compatibility test
- CI updated for DuckDB 1.5.0 with extension-ci-tools@v1.5.0, duckdb/1.4.x LTS branch created with v1.4.4 pins, Version Monitor rewritten for dual-track latest+LTS monitoring
- ALTER SEMANTIC VIEW ... RENAME TO DDL with parser detection, catalog mutation, persistence, and 8 sqllogictest scenarios
- Three SHOW commands with parser detection, VTab implementations for single-view and cross-view introspection, 6 registered table functions, and sqllogictest coverage
- Fan-trap-aware SHOW SEMANTIC DIMENSIONS FOR METRIC via LCA path analysis reusing expand.rs graph helpers
- LIKE/STARTS WITH/LIMIT clause parsing for all SHOW SEMANTIC commands via parser-level WHERE/LIMIT injection
- GitHub Pages deployment workflow with Sphinx -W validation, PR build checks, and README documentation link
- CE description.yml with self-contained native DDL hello_world, LICENSE fixed from BSD-3-Clause to MIT, Cargo.toml bumped to 0.5.4
- Updated MAINTAINER.md with multi-branch strategy, CE registry publishing process, native DDL examples; created examples/snowflake_parity.py demonstrating v0.5.4 features

---

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
