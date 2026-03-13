# Tech Debt & Deferred Items (v0.1.0 -> v0.5.0)

This document catalogs all accepted design decisions, deferred requirements, known architectural limitations, and test coverage gaps across milestones v0.1.0 through v0.5.0. It serves as the canonical record of trade-offs so that future work does not need to re-discover decisions made during earlier milestones.

## Accepted Design Decisions

These are intentional trade-offs made during v0.1.0 development. Each was the best available option given constraints at the time.

### 1. ✅ Sidecar file persistence instead of SQL-based catalog writes

- **Origin:** Phase 2, decision [02-04] sidecar-persistence
- **Decision:** DuckDB holds execution locks during scalar `invoke()`, which prevents any SQL execution from within DDL functions (`define_semantic_view`, `drop_semantic_view`). Both `try_clone()` (same-instance locks) and `Connection::open(path)` (file-level lock) deadlock or block. The extension writes catalog changes to a `.semantic_views` sidecar file using plain file I/O with atomic rename (write-to-tmp-then-rename). On next extension load, `init_catalog` reads the sidecar and syncs definitions into the `semantic_layer._definitions` DuckDB table.
- **Action:** Resolved in v0.2.0 with `pragma_query_t` using a separate `persist_conn` (write-first pattern). The sidecar file was eliminated. The C++ shim was subsequently removed in v0.4.0 (it was a no-op stub) -- all persistence is handled in pure Rust.

### 2. ✅ Catalog table naming: `semantic_layer._definitions`

- **Origin:** Phase 2 audit item (catalog table name deviation)
- **Decision:** REQUIREMENTS.md originally specified `_semantic_views_catalog` as the table name. The implementation uses `semantic_layer._definitions` (a dedicated schema with a prefixed table name). This provides better namespace isolation and follows DuckDB conventions for extension-owned objects. The requirement text (DDL-05) was updated to match the implementation.
- **Action:** None needed. The naming is accepted as correct.

### 3. ✅ All output columns are VARCHAR — RESOLVED

- **Origin:** Phase 4, decision [04-03] varchar-output-columns
- **Decision:** Originally declared all columns as VARCHAR to avoid type mismatch panics. Resolved in v0.2.0 with typed output, then further simplified post-v0.2.0 with zero-copy vector references (`duckdb_vector_reference_vector`). Type mismatches are now handled at SQL generation time via `build_execution_sql` casts.
- **Action:** None needed. Output is fully typed with zero-copy transfer.

### 4. ✅ Manual FFI entrypoint instead of macro

- **Origin:** Phase 4, decision [04-01] manual-ffi-entrypoint
- **Decision:** Replaced the `#[duckdb_entrypoint_c_api]` macro with a hand-written FFI entrypoint function. This was necessary to capture the raw `duckdb_database` handle, which enables creating an independent `duckdb_connection` via `duckdb_connect`. The independent connection is used by `semantic_query()` to execute expanded SQL without lock conflicts with the host connection.
- **Action:** None needed unless the `duckdb-rs` macro adds database handle capture in a future release.

### 5. ❌ Native EXPLAIN deferred to v0.2.0

- **Origin:** Phase 4, QUERY-04 (reworded); tracked as QUERY-V2-03
- **Decision:** The `explain_semantic_view()` table function provides expanded SQL inspection as a workaround. Native `EXPLAIN FROM semantic_query(...)` would show the expanded SQL instead of the DuckDB physical plan, but this would require intercepting the EXPLAIN hook, which is not accessible from a loadable extension (Python DuckDB uses `-fvisibility=hidden`).
- **Action:** The C++ shim was removed in v0.4.0 (it was a no-op stub). Native EXPLAIN interception remains architecturally blocked -- it would require DuckDB to expose EXPLAIN hooks via the C API or extension loading mechanism.

### 6. ❓ ON-clause substring matching for join dependency detection

- **Origin:** Phase 3, decision [03-02] on-clause-substring-matching
- **Decision:** Transitive join dependency detection checks whether a table name appears as a substring in the ON clause of other joins. This is a sufficient heuristic for v0.1.0 where users declare joins in dependency order and ON clauses reference table names directly. It avoids the complexity of a full SQL parser for join clause analysis.
- **Action:** If v0.2.0 supports more complex join patterns (subqueries in ON clauses, aliased references), this may need to be replaced with proper SQL parsing using `sqlparser-rs` or similar.

### 7. ❓ Unqualified column names required in expressions

- **Origin:** Phase 4, decision [04-03] unqualified-join-expressions
- **Decision:** Dimension and metric expressions must use unqualified column names (e.g., `region` not `orders.region`) because the CTE-based expansion flattens all source tables into a single `_base` namespace. Qualified names would reference tables that do not exist in the CTE scope.
- **Action:** If v0.2.0 changes the expansion strategy away from CTEs, qualified column names could be supported.

### 8. ❓ Statement rewrite approach for native DDL (not custom grammar)

- **Origin:** v0.5.0 Phase 16-17, parser extension spike
- **Decision:** Native DDL (`CREATE SEMANTIC VIEW name (...)`) is implemented via DuckDB's parser hook fallback mechanism. The parse_function detects the `CREATE SEMANTIC VIEW` prefix, and the plan_function rewrites it to `SELECT * FROM create_semantic_view('name', ...)` which executes against the existing function-based DDL path. The DDL body uses DuckDB function-call syntax (`:=` named parameters with struct/list literals) because `rewrite_ddl` passes the body verbatim to the underlying function call. This means the "native DDL" is syntactic sugar over function calls, not a true SQL DDL syntax.
- **Gap:** The phase 21 validation layer (`scan_clause_keywords`) can parse a conventional SQL-style body (`TABLES (...), DIMENSIONS (...), METRICS (...)`) but there is no translation layer to convert it into executable function-call syntax. A Snowflake-style SQL DDL grammar (without `:=` and struct literals) was the original intent but was never implemented.
- **Action:** Next milestone (v0.6.0) — implement proper SQL DDL syntax parsing so that `CREATE SEMANTIC VIEW` accepts conventional SQL keyword syntax, not function-call syntax. The function-based interface remains as the internal execution target.

### 9. ✅ DDL connection isolation pattern

- **Origin:** v0.5.0 Phase 17, DDL execution
- **Decision:** The plan_function executes rewritten DDL SQL on a separate `duckdb_connection` created at extension init time and stored as a file-scope static in `shim.cpp`. This avoids deadlocking the main connection's ClientContext lock, which is held during the bind phase when plan_function is called.
- **Action:** None needed. The pattern is safe as long as DDL is not executed concurrently (DuckDB is single-writer). If concurrent DDL becomes a requirement, a connection pool or mutex would be needed.

### 10. ✅ Amalgamation compilation via cc crate

- **Origin:** v0.5.0 Phase 15, C++ shim infrastructure
- **Decision:** `duckdb.cpp` (the DuckDB amalgamation, ~23MB / ~300K lines) is compiled alongside `shim.cpp` via the `cc` crate. First build takes ~2.5 minutes; cached on subsequent builds. This provides ALL DuckDB C++ symbols (constructors, RTTI, vtables) without manual stubs. Symbol visibility restricts exports to the entry point only, preventing ODR conflicts with the host DuckDB process. The amalgamation must be version-pinned to match `TARGET_DUCKDB_VERSION`.
- **Action:** The compilation cost is one-time per clean build. `build.rs` feature-gates the C++ compilation behind `CARGO_FEATURE_EXTENSION`, so `cargo test` (default features) is unaffected.

### 11. ✅ C_STRUCT_UNSTABLE ABI (evaluated, kept)

- **Origin:** v0.5.0 Phase 18, ABI evaluation
- **Decision:** Evaluated switching from `C_STRUCT_UNSTABLE` to `CPP` ABI for community extension registry compatibility. Rejected: CPP entry point failed in Phase 15 because `ExtensionLoader` referenced non-inlined C++ symbols unavailable under Python DuckDB's `-fvisibility=hidden`. `C_STRUCT_UNSTABLE` pins the binary to an exact DuckDB version (same as CPP in practice). Compatible with the community extension registry (`rusty_quack` uses the same approach). The version-pinning cost is mitigated by the DuckDB Version Monitor CI workflow.
- **Action:** No change. Re-evaluate if DuckDB stabilizes the C API or adds a new ABI type for mixed Rust+C++ extensions.

### 12. ❓ DDL pipeline uses all-VARCHAR result forwarding

- **Origin:** v0.5.1 Phase 20, C++ result forwarding for DESCRIBE/SHOW
- **Decision:** The DDL parser hook pipeline (`sv_ddl_bind`/`sv_ddl_execute` in `shim.cpp`) executes rewritten SQL on `sv_ddl_conn`, reads results via `duckdb_value_varchar` into `vector<vector<string>>`, and declares all output columns as VARCHAR. This works but loses native types — DESCRIBE and SHOW return VARCHAR columns even though the underlying functions have known, static schemas.
- **Why not fix now:** The schema for each `DdlKind` variant is known at detection time (e.g., DESCRIBE always returns 6 specific columns, SHOW returns 2). We could declare native types per variant and skip VARCHAR serialization. However, this creates schema coupling — any change to what `describe_semantic_view()` or `list_semantic_views()` returns would need updating in two places (the VTab bind in Rust and the DDL schema declaration). DDL results are single-digit rows, so the performance difference is immeasurable.
- **Action:** If the DDL result schemas stabilize and a cleaner type contract is desired, declare static types per `DdlKind` in Rust, pass them across the FFI boundary, and use native types in `sv_ddl_bind`. Zero-copy vector transfer (as in the query path) is not worth the complexity here — the C++ execute callback receives a C++ `DataChunk&` while `sv_ddl_conn` results come from the C API, making bridging awkward for negligible gain.

## Deferred Requirements

Requirements originally deferred from v0.1.0. Updated to reflect status as of v0.5.0.

| | ID | Description | Status |
|---|---|-------------|--------|
| ✅ | QUERY-V2-01 | Native `CREATE SEMANTIC VIEW` DDL syntax | Resolved in v0.5.0 via statement rewrite (Phase 16-17). |
| ✅ | QUERY-V2-02 | Time dimensions with granularity coarsening | Resolved in v0.4.0 — removed; users write `date_trunc()` directly in dimension `expr`. |
| ❌ | QUERY-V2-03 | Native `EXPLAIN` interception for `semantic_query()` | Architecturally blocked: EXPLAIN hooks not exposed to loadable extensions. |
| ❓ | DIST-V2-01 | Published to DuckDB community extension registry | Pending upstream PR to `duckdb/community-extensions`. |
| ❓ | DIST-V2-02 | Real-world TPC-H demo with documented example queries | Pending — deferred to align with registry publishing. |
| ✅ | — | Replace sidecar file persistence with `pragma_query_t` callbacks | Resolved in v0.2.0 with `persist_conn` write-first pattern. |

## Known Architectural Limitations

Constraints inherent to the current approach that affect users or maintainers.

### 1. ❓ FFI execution layer not fuzz-covered

- **What:** The `execute_sql_raw` function and `duckdb_vector_reference_vector` call in `src/query/table_function.rs` contain the remaining unsafe code in the extension. The query pipeline uses zero-copy vector references to stream result chunks directly into output, replacing the previous binary-read dispatch.
- **Why:** These functions require the DuckDB loadable-extension function-pointer stubs, which are only initialized at runtime when DuckDB loads the extension via `LOAD`. They cannot run in a standalone test binary.
- **Impact:** The unsafe surface area is significantly smaller than v0.2.0's binary-read dispatch — only `execute_sql_raw` (query execution) and `duckdb_vector_reference_vector` (shared vector ownership) remain in the hot path. Type mismatches are handled at SQL generation time via `build_execution_sql` casts, not at read/write time.
- **Mitigation:** SQLLogicTest integration tests exercise these paths with real data. `tests/vector_reference_test.rs` validates the zero-copy mechanism directly (lifetime safety, multi-chunk, complex types). The 36 PBTs in `tests/output_proptest.rs` still validate end-to-end type correctness via `test_helpers`.

### 2. ❓ DuckDB version pinning (`= 1.4.4`)

- **What:** The `duckdb` crate dependency is pinned to an exact version (`= 1.4.4`) in `Cargo.toml`.
- **Why:** DuckDB's ABI is not stable across minor versions. An extension built against one version may crash or fail to load with a different DuckDB runtime.
- **Impact:** Every DuckDB release requires a version bump, rebuild, and re-test of the extension. The `DuckDBVersionMonitor.yml` CI workflow automates detection and opens a PR when a new DuckDB version is available.
- **Mitigation:** The version monitor workflow (Phase 1, INFRA-03) detects new releases and opens a PR with `@copilot` mention for automated investigation. Manual version bumps follow the process documented in MAINTAINER.md.

### 3. ✅ All output columns are VARCHAR — RESOLVED

- **What:** Originally, the `semantic_query()` table function returned all columns as VARCHAR. This was resolved in v0.2.0 with binary-read dispatch, then further improved post-v0.2.0 with zero-copy vector references.
- **Current state:** Output columns are fully typed. The table function uses `duckdb_vector_reference_vector` to stream result chunks directly into output with zero copying. Type mismatches between bind-time inference and runtime (e.g., HUGEINT→BIGINT from optimizer changes, STRUCT/MAP→VARCHAR) are handled by `build_execution_sql`, which wraps the expanded SQL with explicit casts where needed.
- **Impact:** None — consumers receive correctly typed output. No manual casting required.

### 4. ❓ Unqualified column names required in expressions

- **What:** Dimension and metric SQL expressions must use unqualified column names (e.g., `sum(revenue)` not `sum(orders.revenue)`).
- **Why:** See Accepted Decision 7 above. The CTE-based expansion strategy flattens all tables into a single `_base` namespace.
- **Impact:** Users defining semantic views over tables with identically-named columns must use aliases in the join ON clause or rename columns to disambiguate.
- **Mitigation:** Error messages from DuckDB at query time identify ambiguous column references. The `explain_semantic_view()` function helps users debug expansion issues.

## Test Coverage Gaps

Areas where test coverage is reduced compared to ideal, with justification.

### 1. ❓ Iceberg integration test uses Python instead of SQLLogicTest

- **Origin:** Phase 4 audit item; decision [04-03] python-ducklake-test
- **Reason:** The DuckDB SQLLogicTest runner cannot dynamically install extensions (DuckLake, httpfs). The integration test requires loading these extensions to create Iceberg-backed tables.
- **Mitigation:** `test/integration/test_ducklake.py` covers the same semantic query functionality against DuckLake tables. It is run via `just test-iceberg` and exercises the full round-trip: define semantic view, query with dimensions and metrics, assert correct results.

### 2. ❓ FFI execution layer not fuzz-testable standalone

- **Origin:** Phase 5 audit item (TEST-05 partial scope)
- **Reason:** The loadable-extension function-pointer stubs (`duckdb_query`, `duckdb_value_varchar`, etc.) are only available at runtime when DuckDB loads the extension. A standalone fuzz binary cannot initialize these stubs.
- **Mitigation:** Three fuzz targets cover the non-FFI attack surface: `fuzz_json_parse` (definition JSON parsing), `fuzz_sql_expand` (expansion engine SQL generation), `fuzz_query_names` (dimension/metric name validation). SQLLogicTest provides integration coverage of the FFI layer. Post-v0.2.0, the FFI unsafe surface is much smaller — the zero-copy vector reference approach eliminated all per-type binary read/write code; only `execute_sql_raw` and `duckdb_vector_reference_vector` remain in the hot path. `tests/vector_reference_test.rs` validates zero-copy lifetime safety under `cargo test`.

### 3. ✅ Sandbox test portability (resolved in Phase 6)

- **Origin:** Phase 3 audit item (3 catalog sidecar tests failed in sandbox)
- **Reason:** Three Rust tests creating temporary files used hardcoded paths that were inaccessible in sandboxed environments.
- **Resolution:** Phase 6 (decision [06-01] temp-dir-pattern) updated all tests to use `std::env::temp_dir()` for portable temporary file paths. This gap is now closed.

### 4. ✅ DDL prefix whitespace — RESOLVED in Phase 25.1

- **Origin:** Phase 25 proptest surfaced this
- **Resolution:** Phase 25.1 replaced `starts_with_ci` literal prefix matching with
  `match_keyword_prefix` token-based scanning. All 7 DDL prefix forms now tolerate
  arbitrary ASCII whitespace (space, tab, newline, carriage return, vertical tab,
  form feed) between keywords. The `prefix_len()` static function was replaced by
  the dynamic byte count returned by `detect_ddl_prefix(query)`.
- **Scope:** ASCII whitespace only (6 characters: 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20).
  Unicode whitespace is handled by DuckDB before the hook fires.

---

**Date:** 2026-03-12
**Milestone:** v0.5.2 (updated from v0.5.1)
**Audit report:** `.planning/milestones/v1.0-MILESTONE-AUDIT.md`
