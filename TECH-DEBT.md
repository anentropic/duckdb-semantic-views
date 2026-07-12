# Tech Debt & Deferred Items (v0.1.0 -> v0.10.0)

This document catalogs all accepted design decisions, deferred requirements, known architectural limitations, and test coverage gaps across milestones v0.1.0 through v0.10.0. It serves as the canonical record of trade-offs so that future work does not need to re-discover decisions made during earlier milestones.

## Accepted Design Decisions

These are intentional trade-offs made during v0.1.0 development. Each was the best available option given constraints at the time.

### 1. ✅ Sidecar file persistence instead of SQL-based catalog writes

- **Origin:** Phase 2, decision [02-04] sidecar-persistence
- **Decision:** DuckDB holds execution locks during scalar `invoke()`, which prevents any SQL execution from within DDL functions (`define_semantic_view`, `drop_semantic_view`). Both `try_clone()` (same-instance locks) and `Connection::open(path)` (file-level lock) deadlock or block. The extension writes catalog changes to a `.semantic_views` sidecar file using plain file I/O with atomic rename (write-to-tmp-then-rename). On next extension load, `init_catalog` reads the sidecar and syncs definitions into the `semantic_layer._definitions` DuckDB table.
- **Action:** Resolved. The sidecar file was eliminated in v0.2.0; the v0.2-era `persist_conn` (`pragma_query_t` write-first pattern) was itself superseded in v0.8.0 by the `parser_override` design, which rewrites catalog writes to native DML that runs on the caller's own connection (`persist_conn` no longer exists). A no-op C++ shim was removed in v0.4.0, and a **new** C++ shim was reintroduced in v0.5.0 for the parser-override hook + read-side Catalog-API table functions — persistence itself is pure-Rust native SQL, but "no C++ shim exists" is no longer true.

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
- **Decision:** The extension uses a hand-written FFI entrypoint (`src/lib.rs`, mirroring what `#[duckdb_entrypoint_c_api]` generates) rather than the macro, so it can capture the raw `duckdb_database` handle. The handle is used to register the parser-override hook and the read-side/query table functions via the C++ Catalog API.
- **Note (rationale updated):** The original entry justified the manual entrypoint by "creating an independent `duckdb_connection` for `semantic_query()`." That is no longer how queries run: since Phase 65 `semantic_view()` (and the read-side TFs) execute on a **fresh per-call `Connection(*context.db)` opened inside the C++ bind callback**, not a long-lived independent connection. The extension/runtime path opens no independent connection via `duckdb_connect` (the only `duckdb_connect` call in `src/` is in the `#[cfg(not(feature = "extension"))]` unit-test helper `test_helpers::RawDb`, which never runs in the loaded extension). The manual entrypoint is retained for the database-handle capture the registration path needs.
- **Action:** None needed.

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

### 9. ✅ DDL connection isolation pattern — SUPERSEDED (historical)

- **Origin:** v0.5.0 Phase 17, DDL execution
- **Decision (historical):** The v0.5-era plan_function executed rewritten DDL SQL on a separate `duckdb_connection` created at extension init time and stored as a file-scope static (`sv_ddl_conn`) in `shim.cpp`, to avoid deadlocking the main connection's ClientContext lock held during the bind phase.
- **Superseded:** This mechanism no longer exists — `sv_ddl_conn` has zero occurrences in the codebase. Since v0.8.0 (the `parser_override` unification), write-side DDL is rewritten to **native DML that runs on the caller's own connection**, so it participates in the caller's transaction (the ADBC `autocommit = false` fix). See the accurate entries #23 and #27 for the current DROP/ALTER/CREATE concurrency semantics, and `src/parse/native_sql.rs` for the rewrite. No separate DDL connection is used or needed.

### 10. ✅ Amalgamation compilation via cc crate

- **Origin:** v0.5.0 Phase 15, C++ shim infrastructure
- **Decision:** `duckdb.cpp` (the DuckDB amalgamation, ~23MB / ~300K lines) is compiled alongside `shim.cpp` via the `cc` crate. First build takes ~2.5 minutes; cached on subsequent builds. This provides ALL DuckDB C++ symbols (constructors, RTTI, vtables) without manual stubs. Symbol visibility restricts exports to the entry point only, preventing ODR conflicts with the host DuckDB process. The amalgamation must be version-pinned to match `TARGET_DUCKDB_VERSION`.
- **Action:** The compilation cost is one-time per clean build. `build.rs` feature-gates the C++ compilation behind `CARGO_FEATURE_EXTENSION`, so `cargo test` (default features) is unaffected.

### 11. ✅ C_STRUCT_UNSTABLE ABI (evaluated, kept)

- **Origin:** v0.5.0 Phase 18, ABI evaluation
- **Decision:** Evaluated switching from `C_STRUCT_UNSTABLE` to `CPP` ABI for community extension registry compatibility. Rejected: CPP entry point failed in Phase 15 because `ExtensionLoader` referenced non-inlined C++ symbols unavailable under Python DuckDB's `-fvisibility=hidden`. `C_STRUCT_UNSTABLE` pins the binary to an exact DuckDB version (same as CPP in practice). Compatible with the community extension registry (`rusty_quack` uses the same approach). The version-pinning cost is mitigated by the DuckDB Version Monitor CI workflow.
- **Action:** No change. Re-evaluate if DuckDB stabilizes the C API or adds a new ABI type for mixed Rust+C++ extensions.

### 12. ✅ DDL pipeline uses all-VARCHAR result forwarding — SUPERSEDED (historical)

- **Origin:** v0.5.1 Phase 20, C++ result forwarding for DESCRIBE/SHOW
- **Decision (historical):** The v0.5-era DDL parser hook pipeline (`sv_ddl_bind`/`sv_ddl_execute` executing on `sv_ddl_conn`) read results via `duckdb_value_varchar` into `vector<vector<string>>` and declared all output columns as VARCHAR.
- **Superseded:** That pipeline no longer exists — `sv_ddl_bind`, `sv_ddl_execute`, and `sv_ddl_conn` have zero occurrences in `shim.cpp`. Since Phase 65 the read-side table functions (DESCRIBE / SHOW / GET_DDL / READ_YAML) are real C++ Catalog-API table functions whose Rust dispatchers assemble rows and serialize them over the AR-3 self-describing wire format (`src/ddl/read_ffi.rs`). DESCRIBE still declares a deliberate 5-column VARCHAR schema (`object_kind, object_name, parent_entity, property, property_value`) — that is the intended Snowflake-shaped property table, not the retired forwarding pipeline. No further action.

## Deferred Requirements

Requirements originally deferred from v0.1.0. Updated to reflect status as of v0.5.0.

| | ID | Description | Status |
|---|---|-------------|--------|
| ✅ | QUERY-V2-01 | Native `CREATE SEMANTIC VIEW` DDL syntax | Resolved in v0.5.0 via statement rewrite (Phase 16-17). |
| ✅ | QUERY-V2-02 | Time dimensions with granularity coarsening | Resolved in v0.4.0 — removed; users write `date_trunc()` directly in dimension `expr`. |
| ❌ | QUERY-V2-03 | Native `EXPLAIN` interception for `semantic_query()` | Architecturally blocked: EXPLAIN hooks not exposed to loadable extensions. |
| ✅ | DIST-V2-01 | Published to DuckDB community extension registry | Published: `INSTALL semantic_views FROM community; LOAD semantic_views;` (see CHANGELOG for the per-DuckDB-version rebuilds). |
| ❓ | DIST-V2-02 | Real-world TPC-H demo with documented example queries | Pending — no TPC-H demo in `examples/` yet (registry publishing, its former blocker, has since landed). |
| ✅ | — | Replace sidecar file persistence with SQL-based catalog writes | Resolved in v0.2.0 (sidecar eliminated); the write path was again reworked in v0.8.0 to native DML on the caller's connection. See Accepted Decision #1. |

## Known Architectural Limitations

Constraints inherent to the current approach that affect users or maintainers.

### 1. ❓ FFI execution layer not fuzz-covered

- **What:** The `execute_sql_raw` function and `duckdb_vector_reference_vector` call in `src/query/table_function.rs` contain the remaining unsafe code in the extension. The query pipeline uses zero-copy vector references to stream result chunks directly into output, replacing the previous binary-read dispatch.
- **Why:** These functions require the DuckDB loadable-extension function-pointer stubs, which are only initialized at runtime when DuckDB loads the extension via `LOAD`. They cannot run in a standalone test binary.
- **Impact:** The unsafe surface area is significantly smaller than v0.2.0's binary-read dispatch — only `execute_sql_raw` (query execution) and `duckdb_vector_reference_vector` (shared vector ownership) remain in the hot path. Type mismatches are handled at SQL generation time via `build_execution_sql` casts, not at read/write time.
- **Mitigation:** SQLLogicTest integration tests exercise these paths with real data. `tests/vector_reference_test.rs` validates the zero-copy mechanism directly (lifetime safety, multi-chunk, complex types). The 36 PBTs in `tests/output_proptest.rs` still validate end-to-end type correctness via `test_helpers`.

### 2. ❓ DuckDB version pinning (exact)

- **What:** The `duckdb` / `libduckdb-sys` crate dependencies are pinned to an exact version in `Cargo.toml` (currently `= 1.10504.0`, the duckdb-rs release tracking DuckDB **v1.5.4**; `.duckdb-version` holds the matching `v1.5.4`).
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
- **Mitigation:** `test/integration/test_ducklake.py` covers the same semantic query functionality against DuckLake tables. It is run via `just test-ducklake` (local) / `just test-ducklake-ci` (synthetic-data CI variant) and exercises the full round-trip: define semantic view, query with dimensions and metrics, assert correct results.

### 2. ❓ FFI execution layer not fuzz-testable standalone

- **Origin:** Phase 5 audit item (TEST-05 partial scope)
- **Reason:** The loadable-extension function-pointer stubs (`duckdb_query`, `duckdb_value_varchar`, etc.) are only available at runtime when DuckDB loads the extension. A standalone fuzz binary cannot initialize these stubs.
- **Mitigation:** Eight fuzz targets cover the non-FFI attack surface: `fuzz_json_parse`, `fuzz_yaml_parse`, `fuzz_ddl_parse`, `fuzz_keyword_body`, `fuzz_sql_expand`, `fuzz_query_names`, `fuzz_render_roundtrip`, and `fuzz_parser_override_ffi` (see `fuzz/fuzz_targets/`). SQLLogicTest provides integration coverage of the FFI layer. Post-v0.2.0, the FFI unsafe surface is much smaller — the zero-copy vector reference approach eliminated all per-type binary read/write code; only `execute_sql_raw` and `duckdb_vector_reference_vector` remain in the hot path. `tests/vector_reference_test.rs` validates zero-copy lifetime safety under `cargo test`.

### 3. ✅ Sandbox test portability (resolved in Phase 6)

- **Origin:** Phase 3 audit item (3 catalog sidecar tests failed in sandbox)
- **Reason:** Three Rust tests creating temporary files used hardcoded paths that were inaccessible in sandboxed environments.
- **Resolution:** Phase 6 (decision [06-01] temp-dir-pattern) updated all tests to use `std::env::temp_dir()` for portable temporary file paths. This gap is now closed.

### 4. ✅ DDL prefix whitespace — RESOLVED in Phase 25.1

- **Origin:** Phase 25 proptest surfaced this
- **Resolution:** Phase 25.1 replaced `starts_with_ci` literal prefix matching with
  `match_keyword_prefix` token-based scanning. Every DDL prefix form (the
  `DdlKind` set has since grown to ~15: CREATE / OR REPLACE / IF NOT EXISTS,
  DROP / IF EXISTS, ALTER / IF EXISTS, DESCRIBE, SHOW SEMANTIC
  VIEWS/TERSE/DIMENSIONS/METRICS/FACTS/MATERIALIZATIONS, SHOW COLUMNS) now
  tolerates arbitrary ASCII whitespace (space, tab, newline, carriage return,
  vertical tab, form feed) between keywords. The `prefix_len()` static function
  was replaced by the dynamic byte count returned by `detect_ddl_prefix(query)`.
- **Scope:** ASCII whitespace only (6 characters: 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20).
  Unicode whitespace is handled by DuckDB before the hook fires.

---

## v0.8.0 additions

### 19. ❓ Read-side table functions see committed state / the primary catalog, not the caller's transaction

- **Origin:** v0.8.0 known limitation; blocker re-evaluated at v0.10.x (AR-6, code-review 2026-07-02).
- **Decision:** The read-side table functions (`describe_semantic_view`, `list_semantic_views`, `show_semantic_*`, `read_yaml_from_semantic_view`, `get_ddl`, and `semantic_view()` itself) read `semantic_layer._definitions` on a **fresh `Connection(*context.db)`** opened inside each C++ bind callback (`cpp/src/shim.cpp`), not on the caller's connection. A fresh connection runs its own transaction, so reads (a) see only **committed** state — a `BEGIN; CREATE SEMANTIC VIEW v ...; SHOW SEMANTIC VIEWS` will not list `v` until the COMMIT lands (**FF-2**), and (b) resolve against the primary/default catalog, ignoring `USE <attached_db>` (the read side of the single-catalog limitation, **TECH-DEBT #26**). Both share this one root cause. Documented for users in CHANGELOG since v0.8.0 and on the transactional-DDL explanation page.
- **Why not the caller's context (AR-6 re-evaluation — corrects the original entry):** The v0.8.0-era note blamed the C API for not exposing `BindInfo`'s connection. That was stale and only half the story. The Phase 65 C++ Catalog-API binds now have the caller's `ClientContext &` in hand, and reads **still** cannot run on it: `ClientContext::context_lock` is a **non-recursive `std::mutex`** the caller holds continuously from parse → bind → exec of the outer query, so the bind callback runs *inside* that held lock. Any re-entrant `context.Query(...)` re-acquires the same mutex and **self-deadlocks** — deterministic, within ~milliseconds, empirically proven by the Phase 65 spikes (`A2-DEADLOCK` at plan time, `EXEC-TIME-RC1` at exec time; the bind phase sits inside the identical held lock; the fresh-connection path is the one that works, `READ-BIND-RC0`). Opening a fresh connection is safe because it takes `connections_lock` (free), not `context_lock`. So exposing the caller's context is **necessary but not sufficient**: no single DuckDB v1.5.4 mechanism provides both liveness and the caller's transaction for a catalog read from inside a bind.
- **Action if DuckDB ever provides both primitives:** Route reads onto the caller's transaction only once upstream provides **both** (a) `ClientContext` / `DatabaseInstance` access through duckdb-rs `BindInfo`, **and** (b) a recursive / relinquishable `context_lock` (or a re-dispatch path that runs the read through the caller's own binder without re-entering `ClientContext::Query`). Note this is not a pure API-surface change: (a) alone is insufficient, because the `context_lock` self-deadlock (b) is the binding constraint. With both, reads move onto the caller's transaction, fixing FF-2 and the read side of #26 together and letting the fresh per-call connection be dropped. Until then this is an accepted limitation grounded in a DuckDB liveness constraint, not an incidental FFI gap.

### 20. ✅ Bounded LRU evictions are silent at the parser-override site

**Resolved in Phase 62 (v0.8.0), simplified further in Phase 65 / AR-7.** The bounded LRU is gone: multi-DB processes can load the extension into arbitrarily many DuckDB instances without eviction. The Phase-62 design held a per-DB `OverrideContext` (with an intentionally-leaked `duckdb_connection`), but Phase 65 Plan 06's H1 emptied that struct and **AR-7 removed it entirely** — `SemanticViewsParserInfo` is now an empty marker struct (`cpp/src/shim.cpp`), the parser hook carries no per-DB Rust state, and the read side uses a fresh per-call `Connection(*context.db)` instead of any long-lived/leaked connection. So there is no longer any per-DB context to evict *or* leak.

**Original limitation (preserved for archaeology):**

- **Origin:** v0.8.0 B3 (bounded LRU for `parser_override_catalog`).
- **Decision:** The per-extension-load `db_token` → `CatalogReader` map is a 16-entry LRU. A long-lived process that opens more than 16 DuckDB instances will see the oldest token evicted on the 17th load. The next CREATE / DROP / ALTER routed to that token surfaces the friendly error `semantic_views: catalog context for this database has been evicted (process has opened more than 16 databases)`. The eviction itself happens silently inside `parser_override_catalog::set` — there is no log line at the moment of eviction.
- **Why this is acceptable:** The 16-database threshold covers every realistic interactive and CI workload. Daemon processes that load against many databases are the only affected scenario and they get a clear actionable error when they hit the wall.
- **Action if the wall starts to bite:** Either bump the capacity (no other code change needed) or replace the LRU with an explicit registration lifecycle tied to extension-unload (DuckDB does not currently expose an unload hook that we can hook into).

### 21. ❓ `CALL disable_peg_parser()` resets `allow_parser_override_extension`

- **Origin:** v0.8.0 milestone close, surfaced by `peg_compat.test`.
- **Decision:** DuckDB's `disable_peg_parser` pragma resets `allow_parser_override_extension` to its `default` value (`DEFAULT_OVERRIDE`), which silently bypasses our hook entirely. Subsequent semantic DDL on that connection produces the default parser's `Parser Error: syntax error at or near "SEMANTIC"`. Working around this requires the caller to explicitly re-set `allow_parser_override_extension='FALLBACK'` after disabling PEG.
- **Why deferred:** `disable_peg_parser` is a built-in pragma; parser_override does not see it. The cleanest fix would be a DuckDB-side change so that disabling PEG preserves whatever parser_override setting was in effect.
- **Mitigation:** `peg_compat.test` includes the `SET` workaround and CHANGELOG / MAINTAINER document the gotcha.

### 22. ✅ FALLBACK_OVERRIDE silently drops `DISPLAY_EXTENSION_ERROR`

**Resolved in Phase 62 (v0.8.0).** `parse_function` was re-introduced purely as the error-reporting layer. `parser_override` now defers all validation errors with `DISPLAY_ORIGINAL_ERROR` (rc=2); the default parser fails on the unrecognised DDL prefix; DuckDB calls our `sv_parse_stub`; we re-run validation and return `DISPLAY_EXTENSION_ERROR` with `error_location` set to the byte offset of the offending token. `ParserException::SyntaxError` renders `LINE 1: ... ^` for free. The synthesised `SELECT error('...')` workaround (`sql_throwing`) has been deleted. See `.planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md` §Q1 (position-tracking contract) and §6 rows B1–B7 for the per-DDL caret coverage.

**Original limitation (preserved for archaeology):**

- **Origin:** v0.8.0 milestone close, surfaced when investigating the post-unification sqllogictest failures (see `sql_throwing` helper in `src/parse.rs`).
- **Decision:** DuckDB's `ParseInternal` (verified in the v1.5.2 amalgamation) ignores any `parser_override` result that isn't `PARSE_SUCCESSFUL` when `allow_parser_override_extension` is `FALLBACK`. That means a Rust-side validation error returned via `DISPLAY_EXTENSION_ERROR` (rc=1 on the FFI boundary) is dropped, and the user sees the default parser's syntax error instead of our message. We work around this by synthesising a `SELECT error('<msg>')` statement and returning it as `PARSE_SUCCESSFUL`, so DuckDB raises the message at execution time. The rc=1 path on the FFI boundary is now dead but kept for forward-compat with `STRICT_OVERRIDE`.
- **Why deferred:** Switching to `STRICT_OVERRIDE` would cause every non-semantic SQL statement to round-trip through our hook with `DISPLAY_ORIGINAL_ERROR`, which is fine semantically but slightly costlier. The synthesised-error workaround has zero overhead on success cases and gives identical user experience.
- **Action if DuckDB ever fixes FALLBACK to honour `DISPLAY_EXTENSION_ERROR`:** Replace `sql_throwing` with a direct `write_error_to_buffer` + rc=1 path; one fewer SQL statement to plan.

### 23. ❓ Cross-connection `CREATE IF NOT EXISTS` race surfaces as PK violation

- **Origin:** v0.8.0 PR #29 ultrareview follow-up; surfaced by the new IF NOT EXISTS path in `test/integration/test_concurrent_ddl.py`.
- **Decision:** `CREATE SEMANTIC VIEW IF NOT EXISTS` rewrites to `INSERT OR IGNORE` against `semantic_layer._definitions(name)`. This atomically absorbs duplicates that are visible in the caller's own MVCC snapshot — same-transaction duplicates and any racing committer that landed before the caller's transaction began. It does **not** absorb duplicates from a transaction that committed *after* the caller's snapshot was taken: both connections evaluate INSERT against snapshots in which the row is absent, both attempt the INSERT, and DuckDB's PK constraint raises a write-write conflict on the second commit. The loser sees `Constraint Error: Duplicate key "name: <view>" violates primary key constraint`, the same shape plain `CREATE` produces.
- **Why this is acceptable:** DuckDB's PK enforcement happens at row insert / commit time and is not a hook we can intercept from within `parser_override`. The pragmatic alternatives — application-level retry-on-conflict, a coarse table-level lock, or a serializable isolation upgrade — all sit outside the parser-override SQL path. The current behaviour is no worse than plain `CREATE` and the loser receives a clear, actionable message rather than corrupting data. The in-snapshot silent no-op contract (the case users hit far more often: re-running an idempotent setup script in a single process) is fully preserved.
- **Mitigation for callers writing parallel bootstrap scripts:** wrap the `CREATE IF NOT EXISTS` in a try/except and treat a constraint violation on the target name as success. `test/integration/test_concurrent_ddl.py::test_concurrent_create_if_not_exists_serializes` pins the failure shape so this caller-side workaround stays valid across releases.
- **Action if DuckDB ever exposes a hook to retry-on-conflict from a parser_override callback:** add an automatic retry loop and convert this entry to ✅ resolved.

---

## v0.9.0 additions

### 24. ✅ Body parser's TABLES clause splits on whitespace inside quoted source-table names — RESOLVED in Phase 67

- **Origin:** v0.9.0 Phase 64 RESEARCH §Pitfall 5; surfaced as a known limitation during the quoted-identifier fix.
- **Decision:** `parse_single_table_entry` in `src/body_parser.rs` uses a whitespace-based tokenizer to peel off the alias, the `AS`, and the source-table name from each entry in the `TABLES (...)` clause. If a source-table name has whitespace INSIDE a quoted part (e.g. `TABLES (o AS "my db"."schema"."t" PRIMARY KEY (id))`), the tokenizer truncates mid-name and the parse fails. The Phase 64 quoted-identifier fix tightens identifier handling at every other capture site (CREATE / DROP / ALTER / DESCRIBE / SHOW COLUMNS view-name slot, the runtime `semantic_view()` positional arg, and the expansion-side `quote_table_ref`) but leaves this body-parser path on the legacy whitespace tokenizer.
- **Why deferred:** The bug-report reproduction (`"memory"."main"."orders_sv"` as the VIEW name) doesn't exercise this path. Whitespace inside quoted source-table parts is vanishingly rare — physical-table names almost never contain spaces, and even when they do (`"sales 2024"`), the user had the option of writing them as `"sales_2024"` or aliasing them outside the semantic-views layer. Fixing this requires `src/body_parser.rs` to adopt the `src/ident.rs` `find_identifier_end` helper at multiple parse points, which is non-trivial body-parser surgery for a vanishingly rare case.
- **Action if a user hits this:** Provide an alias in DuckDB itself (e.g. `CREATE VIEW orders_clean AS SELECT * FROM "my db"."schema"."t"`) and reference the alias in the semantic view's `TABLES` clause. If the case becomes common, port `find_identifier_end` into `src/body_parser.rs::parse_single_table_entry` and surrounding helpers.
- **Resolution:** Resolved in Phase 67 Plan 02 (commits `256ae65` body-parser surgery + `5fb2ed4` sqllogictest fixture). `parse_single_table_entry` (now in `src/body_parser/tables.rs`, after the AR-1 split of the former single-file `src/body_parser.rs` into a module directory) consumes the source-table-name slot via dot-separated `find_identifier_end`-driven advancement BEFORE running the `PRIMARY KEY` / `UNIQUE` keyword scan over the post-name slice. The canonical pathological case (`TABLES (o AS "weird PRIMARY KEY name" PRIMARY KEY (id))`) now parses correctly. Regression coverage: Rust unit tests in `src/body_parser/mod.rs::tests::test_parse_single_table_entry_*` + a sqllogictest fixture at `test/sql/phase67_quoted_source_tables.test`. The `src/ident.rs` helper itself was not modified (D-09).

---

## v0.10.0 additions

### 25. ✅ Body parser's `NON ADDITIVE BY` and OVER `ORDER BY` clauses split on whitespace for the dimension/column reference — RESOLVED in Phase 68 B1

- **Origin:** v0.10.0 Phase 67 Plan 02 audit-grep (D-10). Sibling pattern to TECH-DEBT #24.
- **Decision (historical):** `parse_non_additive_dims` and the OVER `ORDER BY` parser both peeled the reference off the entry with `split_whitespace()`, so a quoted reference with internal whitespace (`NON ADDITIVE BY ("my dim" ASC)`) split mid-name.
- **Resolution:** Phase 68 Plan 03 (B1) captures the reference slot via `find_identifier_end` + `is_quoting_balanced` in both sites (`src/body_parser/metrics.rs::parse_non_additive_dims` — its doc comment cites this entry — and the OVER `ORDER BY` parser in `src/body_parser/window.rs`). `split_whitespace` now runs only over the *trailing modifier* slice (ASC/DESC/NULLS FIRST|LAST), which contains no identifiers. Quoted-with-whitespace references parse correctly.
- **Remaining sibling slots in the same whitespace-tokeniser class (still open, low-risk):** the `TABLES` alias slot (`tables.rs:42`, `split_first_token`), the `MATERIALIZATIONS` name (`materializations.rs:40`), the relationship target alias (`relationships.rs:119,134`), and the SHOW name slots `IN <view>` / `IN SCHEMA` / `FOR METRIC` (`show_clauses.rs:101,133,192`) still peel their identifier on the first whitespace, so a quoted-with-whitespace value there would split. Same vanishingly-rare-case argument as #24 applies; the fix, if ever needed, is to route each through `find_identifier_end`.

### 26. ❓ Single-catalog write guard does not cover an attached DB that has its own catalog table

- **Origin:** v0.10.x FF-3 (code-review 2026-07-02). Raised in the PR review of the ATTACH single-catalog guard (`src/catalog/writes.rs::managed_catalog_guard_select`).
- **Decision:** Semantic views are single-catalog: `semantic_layer._definitions` lives in the primary database, and every read runs on a fresh per-call connection that resolves against the primary. The FF-3 write guard raises an actionable error when the caller is `USE`-d into a database that has **no** semantic-view catalog while another database does — the common "USE-d into the wrong database" case. It does **not** fire when the attached database the caller is `USE`-d into ALSO has its own `semantic_layer._definitions` (e.g. it was itself bootstrapped as a primary at some point): the write lands in that catalog while the primary-pinned reads never see it (a silent cross-catalog write).
- **Why deferred:** Firing correctly for that sub-case requires knowing which catalog the read binds actually use (the primary/default database). DuckDB exposes no primary/default-database signal on the caller's connection — there is no `current_setting`, no flag in `duckdb_databases()`, only fragile database-oid ordering — so the guard cannot distinguish "current is the managed catalog" from "current is a second catalog that also happens to hold the table" without a false-positive risk on legitimately attaching a second semantic-views database read-only. The robust fix is to thread the caller's `ClientContext`/current catalog into the read binds so reads and writes agree on one catalog — the reader-context work. **AR-6 (§3.7) re-evaluated this and found it blocked by the same DuckDB liveness constraint as FF-2: reads run on a fresh per-call connection because re-entering the caller's `ClientContext::Query` from a bind callback self-deadlocks on the non-recursive `context_lock` (see the updated entry #19 for the full analysis).** So this sub-case cannot be closed without an upstream DuckDB change; managing two independent semantic-view catalogs from one session is unsupported until then.
- **Action if a user hits this:** Manage semantic views from the single database the extension was loaded into; do not `USE` into an attached database that carries its own `semantic_layer._definitions`.

### 27. ❓ DROP / ALTER existence guards are snapshot-consistent with their DML only inside an explicit transaction

- **Origin:** v0.10.x FF-1 (code-review 2026-07-02, §3.6). The DROP/ALTER sibling of #23 (which covers only CREATE).
- **Decision:** A non-`IF EXISTS` DROP/ALTER rewrites to a multi-statement string — one or more pure-SQL guard `SELECT`s (`existence_guard_select`, `rename_collision_guard_select`) followed by the `DELETE`/`UPDATE` — that DuckDB re-parses and runs on the caller's connection (`src/parse/native_sql.rs::rewrite_drop`/`rewrite_alter_rename`/`rewrite_alter_comment`; guards in `src/catalog/writes.rs`). Whether the guard's check is consistent with the DML depends on the caller's transaction state, and this was verified empirically: **inside an explicit transaction** (`BEGIN … COMMIT`, or an `autocommit = false` ADBC/PG connection) every emitted statement shares the one snapshot, so the check is atomic with the DML; **under autocommit** (the default) DuckDB commits after *each* statement of a multi-statement string, so the guard and the DML run in separate implicit transactions. A different connection that commits in the window between them can invalidate the guard's decision:
  - concurrent DROP — both existence guards pass, both `DELETE`s run; the loser's `DELETE` matches 0 rows and reports success having deleted nothing (a silent no-op, not an error);
  - concurrent RENAME — the loser's collision guard passes, then the `UPDATE` hits DuckDB's primary-key constraint and surfaces a raw `Constraint Error: Duplicate key` instead of the friendly `already exists` wording (the same failure shape as #23).
- **Why this is acceptable:** The behaviour is no worse than the equivalent native-`CREATE`/`DROP` concurrency (#23) and the far more common single-process case (re-running an idempotent setup script) is fully consistent. Closing the window by emitting our own `BEGIN … COMMIT` around the rewrite is **not** viable: DuckDB rejects a nested `BEGIN` (`TransactionContext Error: cannot start a transaction within a transaction`, verified), so the wrapper would fail outright whenever the caller is already in a transaction, and an emitted `COMMIT` would prematurely commit an `autocommit = false` caller's in-flight work — breaking the transaction-participation contract the native-DML rewrite exists to provide (the v0.8.0 ADBC fix). Folding the guards into a single statement (as `CREATE` does via `CASE … error()` inside the `INSERT`) does not generalise to the DROP/ALTER "must (not) exist" pre-checks, which need to raise on the *absence*/*presence* of a row a single `DELETE`/`UPDATE` cannot key off.
- **Mitigation for callers needing atomic check-and-write:** wrap the DDL in an explicit `BEGIN … COMMIT` (or use a connection with `autocommit = false`). All emitted statements then share one snapshot and the guard is consistent with the DML; a concurrent committer instead triggers a serialization/PK conflict at `COMMIT`, which the caller can retry — the same pattern as #23's `CREATE IF NOT EXISTS` mitigation.
- **Action if DuckDB ever exposes the caller's transaction state (or a retry-on-conflict hook) from a `parser_override` callback:** conditionally wrap the rewrite in `BEGIN … COMMIT` only when the caller is under autocommit, and convert this entry to ✅ resolved.

### 28. ❌ Component-name identifier contract applied only at the query-resolution boundary (deferred to review §6.2)

- **Origin:** v0.11 identifier-contract work extending PA-8 (view-name case-sensitivity) to dimension/metric/fact names; code review on that PR (2026-07-12).
- **Limitation:** The Snowflake identifier contract (unquoted → case-insensitive, `"quoted"` → case-sensitive) is applied via `ident::ident_matches` / `ident::normalize_ident_part` at the query-argument resolution boundary (`find_dimension` / `find_metric` / `Fact::find`), the CREATE-time uniqueness key (`graph/names.rs::validate_name_uniqueness`), and `alias.*` wildcard de-duplication (`expand/wildcard.rs`). It is **not** yet applied to the remaining name-comparison / keying sites, which still fold case-insensitively regardless of quoting:
  - materialization routing (`expand/materialization.rs`, two duplicated matchers) and the `explain_semantic_view()` `-- Materialization:` header (`query/explain.rs`) — deliberately left *both* case-insensitive so the header cannot disagree with the routing it describes;
  - `NON ADDITIVE BY` dimension references and window inner-metric references;
  - derived-metric / fact expression inlining (identifier references scanned inside expression *text*);
  - the table-alias slot, output-column aliasing for quoted names, and `CiName`'s own case-insensitive `Eq`.
- **Why this is acceptable (interim):** the common case — unquoted names — is unchanged everywhere (`ident_matches` reduces to `eq_ignore_ascii_case` when neither side is quoted), so the split affects only quoted mixed-case component names, which are rare. The query boundary and CREATE-time uniqueness use the same key, so a quoted name cannot silently shadow (the SG-13 hazard that motivated aligning the uniqueness key).
- **Action:** Unify these onto one quote-aware reference-scanning engine as part of §6.2 of `_notes/code-review-2026-07-11.md` (which also collapses the duplicated materialization matcher and the copies of the word-boundary reference scanner). The expression-text sites specifically need a quote-aware tokenizer, not a name comparison — that engine is the right home. Tracked here rather than only in the PR description so the follow-up is not lost (the "half-migrated abstraction" pattern §7 of that review warns about).

---

**Last updated:** 2026-07-12 (v0.11 unreleased) — added entry #28 (component-name
identifier contract deferred to review §6.2). Prior: 2026-07-11 (v0.10.4) accuracy
sweep against the 2026-07-11
code review: retired the ghost-code descriptions in entries #1, #4, #9, #12, #20
(sidecar `persist_conn`, the independent-query-connection rationale, `sv_ddl_conn`,
the `sv_ddl_bind`/`sv_ddl_execute` VARCHAR-forwarding pipeline, and the
`OverrideContext`, none of which exist in the code anymore); flipped #25 and
DIST-V2-01 to ✅; and corrected the DuckDB pin (v1.5.4, not 1.4.4), the fuzz-target
count (8), and the DuckLake test recipe names.
**Most recent full audit:** v0.8.0 — `.planning/milestones/v0.8.0-MILESTONE-AUDIT.md` (entries #24–#25 added post-audit during v0.10.0)
