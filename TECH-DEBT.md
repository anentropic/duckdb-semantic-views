# Tech Debt & Deferred Items (v0.1.0 -> v0.2.0)

This document catalogs all accepted design decisions, deferred requirements, known architectural limitations, and test coverage gaps from the v0.1.0 milestone. It serves as the sole input for v0.2.0 milestone planning so that future work does not need to re-discover trade-offs made during v0.1.0.

## Accepted Design Decisions

These are intentional trade-offs made during v0.1.0 development. Each was the best available option given constraints at the time.

### 1. Sidecar file persistence instead of SQL-based catalog writes

- **Origin:** Phase 2, decision [02-04] sidecar-persistence
- **Decision:** DuckDB holds execution locks during scalar `invoke()`, which prevents any SQL execution from within DDL functions (`define_semantic_view`, `drop_semantic_view`). Both `try_clone()` (same-instance locks) and `Connection::open(path)` (file-level lock) deadlock or block. The extension writes catalog changes to a `.semantic_views` sidecar file using plain file I/O with atomic rename (write-to-tmp-then-rename). On next extension load, `init_catalog` reads the sidecar and syncs definitions into the `semantic_layer._definitions` DuckDB table.
- **Action:** Replace with `pragma_query_t` callbacks in the v0.2.0 C++ shim. The `pragma_query_t` pattern (used by the FTS extension) returns a SQL string that DuckDB executes after the callback returns, during parsing before execution locks are held. This eliminates the sidecar file entirely.

### 2. Catalog table naming: `semantic_layer._definitions`

- **Origin:** Phase 2 audit item (catalog table name deviation)
- **Decision:** REQUIREMENTS.md originally specified `_semantic_views_catalog` as the table name. The implementation uses `semantic_layer._definitions` (a dedicated schema with a prefixed table name). This provides better namespace isolation and follows DuckDB conventions for extension-owned objects. The requirement text (DDL-05) was updated to match the implementation.
- **Action:** None needed. The naming is accepted as correct.

### 3. All output columns are VARCHAR

- **Origin:** Phase 4, decision [04-03] varchar-output-columns
- **Decision:** The `semantic_query()` table function declares all output columns as VARCHAR and wraps expanded SQL in `SELECT CAST(... AS VARCHAR)`. This avoids type mismatch panics when writing string data to typed DuckDB output vectors through the FFI layer. The `duckdb_string_t` inline/pointer union is read directly from vector memory (decision [04-03] direct-string-t-decode).
- **Action:** v0.2.0 may restore typed output columns by implementing proper type-specific vector writes in the FFI layer. Consumers currently must cast numeric columns themselves.

### 4. Manual FFI entrypoint instead of macro

- **Origin:** Phase 4, decision [04-01] manual-ffi-entrypoint
- **Decision:** Replaced the `#[duckdb_entrypoint_c_api]` macro with a hand-written FFI entrypoint function. This was necessary to capture the raw `duckdb_database` handle, which enables creating an independent `duckdb_connection` via `duckdb_connect`. The independent connection is used by `semantic_query()` to execute expanded SQL without lock conflicts with the host connection.
- **Action:** None needed unless the `duckdb-rs` macro adds database handle capture in a future release.

### 5. Native EXPLAIN deferred to v0.2.0

- **Origin:** Phase 4, QUERY-04 (reworded); tracked as QUERY-V2-03
- **Decision:** The `explain_semantic_view()` table function provides expanded SQL inspection as a workaround. Native `EXPLAIN FROM semantic_query(...)` would show the expanded SQL instead of the DuckDB physical plan, but this requires a C++ shim to intercept the EXPLAIN hook.
- **Action:** Implement as QUERY-V2-03 when the C++ shim is built for native DDL.

### 6. ON-clause substring matching for join dependency detection

- **Origin:** Phase 3, decision [03-02] on-clause-substring-matching
- **Decision:** Transitive join dependency detection checks whether a table name appears as a substring in the ON clause of other joins. This is a sufficient heuristic for v0.1.0 where users declare joins in dependency order and ON clauses reference table names directly. It avoids the complexity of a full SQL parser for join clause analysis.
- **Action:** If v0.2.0 supports more complex join patterns (subqueries in ON clauses, aliased references), this may need to be replaced with proper SQL parsing using `sqlparser-rs` or similar.

### 7. Unqualified column names required in expressions

- **Origin:** Phase 4, decision [04-03] unqualified-join-expressions
- **Decision:** Dimension and metric expressions must use unqualified column names (e.g., `region` not `orders.region`) because the CTE-based expansion flattens all source tables into a single `_base` namespace. Qualified names would reference tables that do not exist in the CTE scope.
- **Action:** If v0.2.0 changes the expansion strategy away from CTEs, qualified column names could be supported.

## Deferred to v0.2.0

Requirements explicitly moved to the next milestone. These are documented in REQUIREMENTS.md under "v0.2.0 Requirements."

| ID | Description | Reason |
|----|-------------|--------|
| QUERY-V2-01 | Native `CREATE SEMANTIC VIEW` DDL syntax | Requires C++ shim for DuckDB parser hooks (not exposed to Rust via C API) |
| QUERY-V2-02 | Time dimensions with granularity coarsening (day/week/month/year) | Scoped out of v0.1.0 to reduce complexity |
| QUERY-V2-03 | Native `EXPLAIN` interception for `semantic_query()` | Requires C++ shim for EXPLAIN hook |
| DIST-V2-01 | Published to DuckDB community extension registry | Requires upstream PR to `duckdb/community-extensions` repository |
| DIST-V2-02 | Real-world TPC-H demo with documented example queries | Documentation deliverable deferred to align with registry publishing |
| (sidecar replacement) | Replace sidecar file persistence with `pragma_query_t` callbacks | Requires C++ shim; see Accepted Decision 1 above |

## Known Architectural Limitations

Constraints inherent to the current v0.1.0 approach that affect users or maintainers.

### 1. FFI execution layer not fuzz-covered

- **What:** The `execute_sql_raw` and `read_varchar_from_vector` functions in `src/query/table_function.rs` contain the highest-risk unsafe code in the extension. They handle raw DuckDB C API calls for query execution and result reading.
- **Why:** These functions require the DuckDB loadable-extension function-pointer stubs, which are only initialized at runtime when DuckDB loads the extension via `LOAD`. They cannot run in a standalone test binary.
- **Impact:** Malformed data from DuckDB's result vectors could cause undefined behavior that would not be caught by the existing fuzz targets.
- **Mitigation:** SQLLogicTest integration tests exercise these paths with real data across multiple query patterns. A future `fuzz_varchar_read` target could be added if a test harness for the loadable-extension stubs is built.

### 2. DuckDB version pinning (`= 1.4.4`)

- **What:** The `duckdb` crate dependency is pinned to an exact version (`= 1.4.4`) in `Cargo.toml`.
- **Why:** DuckDB's ABI is not stable across minor versions. An extension built against one version may crash or fail to load with a different DuckDB runtime.
- **Impact:** Every DuckDB release requires a version bump, rebuild, and re-test of the extension. The `DuckDBVersionMonitor.yml` CI workflow automates detection and opens a PR when a new DuckDB version is available.
- **Mitigation:** The version monitor workflow (Phase 1, INFRA-03) detects new releases and opens a PR with `@copilot` mention for automated investigation. Manual version bumps follow the process documented in MAINTAINER.md.

### 3. All output columns are VARCHAR

- **What:** The `semantic_query()` table function returns all columns as VARCHAR regardless of the underlying data types.
- **Why:** See Accepted Decision 3 above. The FFI layer writes all values as strings to avoid type mismatch panics with DuckDB's typed output vectors.
- **Impact:** Consumers must cast numeric columns (e.g., `CAST(total_revenue AS DECIMAL)`) for arithmetic operations. Sorting on numeric columns produces lexicographic rather than numeric ordering unless cast.
- **Mitigation:** The `explain_semantic_view()` function shows the expanded SQL, which consumers can run directly if typed output is needed.

### 4. Unqualified column names required in expressions

- **What:** Dimension and metric SQL expressions must use unqualified column names (e.g., `sum(revenue)` not `sum(orders.revenue)`).
- **Why:** See Accepted Decision 7 above. The CTE-based expansion strategy flattens all tables into a single `_base` namespace.
- **Impact:** Users defining semantic views over tables with identically-named columns must use aliases in the join ON clause or rename columns to disambiguate.
- **Mitigation:** Error messages from DuckDB at query time identify ambiguous column references. The `explain_semantic_view()` function helps users debug expansion issues.

## Test Coverage Gaps

Areas where test coverage is reduced compared to ideal, with justification.

### 1. Iceberg integration test uses Python instead of SQLLogicTest

- **Origin:** Phase 4 audit item; decision [04-03] python-ducklake-test
- **Reason:** The DuckDB SQLLogicTest runner cannot dynamically install extensions (DuckLake, httpfs). The integration test requires loading these extensions to create Iceberg-backed tables.
- **Mitigation:** `test/integration/test_ducklake.py` covers the same semantic query functionality against DuckLake tables. It is run via `just test-iceberg` and exercises the full round-trip: define semantic view, query with dimensions and metrics, assert correct results.

### 2. FFI execution layer not fuzz-testable standalone

- **Origin:** Phase 5 audit item (TEST-05 partial scope)
- **Reason:** The loadable-extension function-pointer stubs (`duckdb_query`, `duckdb_value_varchar`, etc.) are only available at runtime when DuckDB loads the extension. A standalone fuzz binary cannot initialize these stubs.
- **Mitigation:** Three fuzz targets cover the non-FFI attack surface: `fuzz_json_parse` (definition JSON parsing), `fuzz_sql_expand` (expansion engine SQL generation), `fuzz_query_names` (dimension/metric name validation). SQLLogicTest provides integration coverage of the FFI layer.

### 3. Sandbox test portability (resolved in Phase 6)

- **Origin:** Phase 3 audit item (3 catalog sidecar tests failed in sandbox)
- **Reason:** Three Rust tests creating temporary files used hardcoded paths that were inaccessible in sandboxed environments.
- **Resolution:** Phase 6 (decision [06-01] temp-dir-pattern) updated all tests to use `std::env::temp_dir()` for portable temporary file paths. This gap is now closed.

---

**Date:** 2026-02-26
**Milestone:** v0.1.0
**Audit report:** `.planning/milestones/v1.0-MILESTONE-AUDIT.md`
