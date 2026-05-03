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

### 6. ✅ ON-clause substring matching for join dependency detection — RESOLVED in v0.5.2

- **Origin:** Phase 3, decision [03-02] on-clause-substring-matching
- **Decision:** Transitive join dependency detection originally checked whether a table name appears as a substring in the ON clause of other joins.
- **Resolution:** v0.5.2 Phase 26 replaced this with graph-based PK/FK join resolution (`src/graph.rs`). `RelationshipGraph` uses an adjacency list with reverse edges, Kahn's algorithm for topological sort, and `synthesize_on_clause()` to generate ON clauses from declared PK/FK columns. Phase 27 deleted the legacy `resolve_joins` code entirely — PK/FK graph resolution is now the only join path.
- **Action:** None needed.

### 7. ✅ Unqualified column names required in expressions — RESOLVED in v0.5.2

- **Origin:** Phase 4, decision [04-03] unqualified-join-expressions
- **Decision:** Dimension and metric expressions originally required unqualified column names because the CTE-based expansion flattened all source tables into a single `_base` namespace.
- **Resolution:** v0.5.2 Phase 27 replaced CTE flattening with direct `FROM base AS alias LEFT JOIN ...` expansion. Dimension/metric expressions now use qualified column references (`alias.column`) which resolve correctly against the aliased tables. The `_base` CTE was removed entirely (CLN-02).
- **Action:** None needed.

### 8. ✅ Statement rewrite approach for native DDL — RESOLVED in v0.5.2

- **Origin:** v0.5.0 Phase 16-17, parser extension spike
- **Decision:** Native DDL (`CREATE SEMANTIC VIEW name (...)`) was originally implemented as syntactic sugar over function-call syntax (`:=` named parameters with struct/list literals).
- **Resolution:** v0.5.2 Phase 25 ("SQL Body Parser") implemented a full SQL keyword body parser (`src/body_parser.rs`) that accepts conventional SQL syntax: `TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)`. The translation layer in `src/parse.rs` (`rewrite_ddl_keyword_body()`) converts the parsed SQL body into the underlying function-based execution model. No `:=` parameters or struct literals required — pure SQL DDL grammar.
- **Action:** None needed.

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

### 4. ✅ Unqualified column names required in expressions — RESOLVED in v0.5.2

- **What:** Dimension and metric SQL expressions originally required unqualified column names due to CTE-based `_base` flattening.
- **Resolution:** See Accepted Decision 7. v0.5.2 Phase 27 replaced CTE flattening with direct `FROM base AS alias LEFT JOIN ...` expansion. Qualified column references (`alias.column`) now work correctly.

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

## v0.8.1 additions

### 19. ❓ DESCRIBE / SHOW SEMANTIC * read committed state, not the caller's transaction

- **Origin:** v0.8.0 known limitation, re-confirmed at v0.8.1 milestone close.
- **Decision:** Read-side table functions (`describe_semantic_view`, `list_semantic_views`, `show_semantic_*`, `read_yaml_from_semantic_view`, `get_ddl`) are bound to the catalog connection installed at extension load time. They cannot see in-flight writes the caller has made on its own connection — a `BEGIN; CREATE SEMANTIC VIEW v ...; SHOW SEMANTIC VIEWS` will not list `v` until the COMMIT lands. Documented in CHANGELOG since v0.8.0.
- **Why deferred:** Routing read-side functions onto the caller's connection requires `libduckdb-sys` to expose the `BindInfo`'s connection handle. The C API does not currently surface it; binding state holds only `parser_info` and the catalog handle we passed in. A custom shim could probably reach into the C++ side, but the gain (transactional read visibility) doesn't justify another layer of FFI surface to maintain.
- **Action when DuckDB exposes it:** Re-route read-side functions through the executing connection and drop the catalog connection entirely, collapsing onto a single connection per database load.

### 20. ❓ Bounded LRU evictions are silent at the parser-override site

- **Origin:** v0.8.1 B3 (bounded LRU for `parser_override_catalog`).
- **Decision:** The per-extension-load `db_token` → `CatalogReader` map is a 16-entry LRU. A long-lived process that opens more than 16 DuckDB instances will see the oldest token evicted on the 17th load. The next CREATE / DROP / ALTER routed to that token surfaces the friendly error `semantic_views: catalog context for this database has been evicted (process has opened more than 16 databases)`. The eviction itself happens silently inside `parser_override_catalog::set` — there is no log line at the moment of eviction.
- **Why this is acceptable:** The 16-database threshold covers every realistic interactive and CI workload. Daemon processes that load against many databases are the only affected scenario and they get a clear actionable error when they hit the wall.
- **Action if the wall starts to bite:** Either bump the capacity (no other code change needed) or replace the LRU with an explicit registration lifecycle tied to extension-unload (DuckDB does not currently expose an unload hook that we can hook into).

### 21. ❓ `CALL disable_peg_parser()` resets `allow_parser_override_extension`

- **Origin:** v0.8.1 milestone close, surfaced by `peg_compat.test`.
- **Decision:** DuckDB's `disable_peg_parser` pragma resets `allow_parser_override_extension` to its `default` value (`DEFAULT_OVERRIDE`), which silently bypasses our hook entirely. Subsequent semantic DDL on that connection produces the default parser's `Parser Error: syntax error at or near "SEMANTIC"`. Working around this requires the caller to explicitly re-set `allow_parser_override_extension='FALLBACK'` after disabling PEG.
- **Why deferred:** `disable_peg_parser` is a built-in pragma; parser_override does not see it. The cleanest fix would be a DuckDB-side change so that disabling PEG preserves whatever parser_override setting was in effect.
- **Mitigation:** `peg_compat.test` includes the `SET` workaround and CHANGELOG / MAINTAINER document the gotcha.

### 22. ❓ FALLBACK_OVERRIDE silently drops `DISPLAY_EXTENSION_ERROR`

- **Origin:** v0.8.1 milestone close, surfaced when investigating the post-unification sqllogictest failures (see `sql_throwing` helper in `src/parse.rs`).
- **Decision:** DuckDB's `ParseInternal` (verified in the v1.5.2 amalgamation) ignores any `parser_override` result that isn't `PARSE_SUCCESSFUL` when `allow_parser_override_extension` is `FALLBACK`. That means a Rust-side validation error returned via `DISPLAY_EXTENSION_ERROR` (rc=1 on the FFI boundary) is dropped, and the user sees the default parser's syntax error instead of our message. We work around this by synthesising a `SELECT error('<msg>')` statement and returning it as `PARSE_SUCCESSFUL`, so DuckDB raises the message at execution time. The rc=1 path on the FFI boundary is now dead but kept for forward-compat with `STRICT_OVERRIDE`.
- **Why deferred:** Switching to `STRICT_OVERRIDE` would cause every non-semantic SQL statement to round-trip through our hook with `DISPLAY_ORIGINAL_ERROR`, which is fine semantically but slightly costlier. The synthesised-error workaround has zero overhead on success cases and gives identical user experience.
- **Action if DuckDB ever fixes FALLBACK to honour `DISPLAY_EXTENSION_ERROR`:** Replace `sql_throwing` with a direct `write_error_to_buffer` + rc=1 path; one fewer SQL statement to plan.

### 23. ❓ Cross-connection `CREATE IF NOT EXISTS` race surfaces as PK violation

- **Origin:** v0.8.1 PR #29 ultrareview follow-up; surfaced by the new IF NOT EXISTS path in `test/integration/test_concurrent_ddl.py`.
- **Decision:** `CREATE SEMANTIC VIEW IF NOT EXISTS` rewrites to `INSERT OR IGNORE` against `semantic_layer._definitions(name)`. This atomically absorbs duplicates that are visible in the caller's own MVCC snapshot — same-transaction duplicates and any racing committer that landed before the caller's transaction began. It does **not** absorb duplicates from a transaction that committed *after* the caller's snapshot was taken: both connections evaluate INSERT against snapshots in which the row is absent, both attempt the INSERT, and DuckDB's PK constraint raises a write-write conflict on the second commit. The loser sees `Constraint Error: Duplicate key "name: <view>" violates primary key constraint`, the same shape plain `CREATE` produces.
- **Why this is acceptable:** DuckDB's PK enforcement happens at row insert / commit time and is not a hook we can intercept from within `parser_override`. The pragmatic alternatives — application-level retry-on-conflict, a coarse table-level lock, or a serializable isolation upgrade — all sit outside the parser-override SQL path. The current behaviour is no worse than plain `CREATE` and the loser receives a clear, actionable message rather than corrupting data. The in-snapshot silent no-op contract (the case users hit far more often: re-running an idempotent setup script in a single process) is fully preserved.
- **Mitigation for callers writing parallel bootstrap scripts:** wrap the `CREATE IF NOT EXISTS` in a try/except and treat a constraint violation on the target name as success. `test/integration/test_concurrent_ddl.py::test_concurrent_create_if_not_exists_serializes` pins the failure shape so this caller-side workaround stays valid across releases.
- **Action if DuckDB ever exposes a hook to retry-on-conflict from a parser_override callback:** add an automatic retry loop and convert this entry to ✅ resolved.

---

**Date:** 2026-05-03
**Milestone:** v0.8.1
**Audit report:** `.planning/milestones/v1.0-MILESTONE-AUDIT.md`
