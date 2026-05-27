# Phase 65: OverrideContext Connection Teardown — Research

**Researched:** 2026-05-21
**Domain:** DuckDB C-API connection lifecycle, DBInstanceCache busy-spin teardown, parser_extension state ownership
**Confidence:** HIGH (root cause traced through the vendored amalgamation; canonical pattern verified against duckdb-postgres source)
**DuckDB version anchored to:** **v1.5.2** (`DUCKDB_VERSION` in `cpp/include/duckdb.hpp:line 1`; extension API `1.10502.0` per `Cargo.toml`)

---

## 1. Executive Summary

The `>45s` "hang" on in-process RW→RO reopen is **not a hang** — it is a **CPU-bound busy-spin** inside `DBInstanceCache::GetInstanceInternal` at `duckdb.cpp:278017-278030`. The busy-spin will run forever until the extension-owned connections release their `shared_ptr<DatabaseInstance>`. The root cause has been confirmed by direct reading of the vendored amalgamation (see §2). It is exactly the leak Phase 62 §Q2 documented — but the impact assessment was wrong: a "bounded leak of one Connection per DB ever opened" sounds benign; in reality each leaked connection makes the in-process RW→RO (or RW→RW with different config, or any access-mode-mismatch reopen) busy-spin indefinitely on a single CPU.

**Recommended fix (track a, root cause):** Use DuckDB 1.5.2's `ExtensionCallback::OnConnectionOpened` to **install per-connection state via `ClientContext::registered_state`**, and **stop owning long-lived `duckdb_connection` handles in the extension at all**. This is the canonical DuckDB 1.5.x pattern, confirmed in `duckdb-postgres` (`PostgresExtensionCallback::OnConnectionOpened` + `loader.GetDatabaseInstance().GetConnectionList()` for retro-installation). Connections that the extension created itself become unnecessary once we accept that:

1. Catalog reads (existence checks, `_definitions` lookup, type-inference probes) can be done **on the caller's connection** via the parser_override callback's `ParserOptions` — except `parser_override_function_t` does not pass a `ClientContext`, so this is not directly viable for `parser_override`.
2. Therefore the actually viable shape for v0.9.1 is **D-07 candidate 2: short-lived per-DDL connect/disconnect**, opened inside `sv_parser_override_rust` and dropped before return. The `OverrideContext`'s `duckdb_connection` field is removed; only `db_handle` and `is_file_backed` are stored.

This eliminates the lifetime question entirely (D-06's premise vindicated). Per-DDL `duckdb_connect`+`duckdb_disconnect` cost is small (~µs) relative to the rest of CREATE SEMANTIC VIEW (LIMIT 0 probes, JSON enrichment, INSERT). The same fix applies to `query_conn` (the `semantic_view` table function's `QueryState::conn`) — open it during `bind`/`init` and close it at end-of-query.

**Candidate D-07-4 (documented limitation)** is rejected because (a) is achievable. **Candidate D-07-2 (deterministic teardown via callback)** is rejected because DuckDB 1.5.2 has no extension-unload hook (TECH-DEBT 20 confirmed) AND `OnConnectionClosed` fires *under* `connections_lock` so re-entering `duckdb_disconnect` from inside it deadlocks. **Candidate D-07-3 (non-owning/weak handle)** is rejected: the C-API does not expose a weak-to-shared upgrade primitive for `duckdb_connection`.

---

## 2. Reproduction & Instrumentation

### 2.1 Confirmed root-cause chain (read from vendored DuckDB v1.5.2 amalgamation)

The chain of references that keeps `DatabaseInstance` alive past the caller's `close()`:

| Step | Object | Lifetime owner | File:line |
|------|--------|----------------|-----------|
| 1 | `duckdb_connection` (our `catalog_conn` + `query_conn`) | `Connection*` heap object created in `duckdb_connect` | `duckdb.cpp:266432-266447` |
| 2 | `Connection::context` | `shared_ptr<ClientContext>` | `duckdb.cpp:275774` |
| 3 | `ClientContext::db` | `shared_ptr<DatabaseInstance>` (acquired via `database.shared_from_this()`) | `duckdb.cpp:275774, 272630` |
| 4 | `DatabaseInstance::config` | value member; cannot destruct until `~DatabaseInstance` runs | `duckdb.cpp:276813` |
| 5 | `DBConfig::db_cache_entry` | `shared_ptr<DatabaseCacheEntry>` | `duckdb.hpp:40102, 277230` |
| 6 | `DBInstanceCache::db_instances[cache_key]` | `weak_ptr<DatabaseCacheEntry>` — does NOT keep the entry alive | `duckdb.hpp:56913` |

When the user runs `w.close()`:
- Python releases `Connection*` (refcount on shared_ptr<ClientContext> -= 1; reaches 0 → ~ClientContext → shared_ptr<DatabaseInstance> -= 1).
- Python typically also calls `duckdb_close(&db)` (delete DatabaseWrapper → shared_ptr<DuckDB> -= 1 → ~DuckDB → shared_ptr<DatabaseInstance> -= 1).
- Remaining `shared_ptr<DatabaseInstance>` refcount: **2 (our catalog_conn + query_conn)**.
- `DatabaseInstance` survives → `DBConfig` survives → `DatabaseCacheEntry` (held strongly by `db_cache_entry`) survives.

When the user then runs `duckdb.connect(path, read_only=True)`:
- Python's `duckdb.connect()` routes through `DBInstanceCache::GetOrCreateInstance` (`duckdb.cpp:278097`) with `CacheBehavior::AUTOMATIC` → `cache_instance = true` for file-backed paths (`duckdb.cpp:278101-278108`).
- `GetCacheKey(path, config)` is **derived from path only**, NOT access mode (`duckdb.cpp:277971-277986`). RW and RO share the same cache key.
- `GetInstanceInternal` finds the existing entry. `cache_entry.lock()` succeeds (db_cache_entry holds strong ref via step 5). But the inner `cache_entry->database.lock()` **may** return null if `~DuckDB` already ran. With the extension's two leaked connections, `~DatabaseInstance` is still pending → `DuckDB` may have been destroyed (its shared_ptr<DatabaseInstance> was released in step 4 above) → the weak_ptr<DuckDB> is expired. So `db_instance` (line 278015) is null.
- **Enters the busy-spin** (`duckdb.cpp:278022-278024`):
  ```cpp
  cache_entry.reset();
  while (!weak_cache_entry.expired()) {
  }
  ```
  `weak_cache_entry.expired()` returns `true` only when the strong `db_cache_entry` ref in `DBConfig` releases, which only happens at `~DBConfig`, which only happens at `~DatabaseInstance` (step 4), which only happens when ALL `shared_ptr<DatabaseInstance>` refs release — including ours from `catalog_conn` and `query_conn`. They never will. **The thread spins forever on a single CPU.**

**This is not a database lock or a deadlock. It is a CPU busy-spin.** A user observing it through Python sees "the connect call doesn't return" — easily mistaken for "the DB is hung." `top` / Activity Monitor would show one Python thread at 100% CPU. `[VERIFIED: cpp/include/duckdb.cpp lines 277995-278038, 277213-277230, 276813-276836]`

### 2.2 Why "without `CREATE SEMANTIC VIEW`, RO open returns instantly"

The downstream report says removing `CREATE SEMANTIC VIEW` makes the RO reopen work. This is misleading framing — the leak is from `init_extension`, not from `CREATE`. But the report is consistent with the root cause: without `LOAD semantic_views`, no extension-owned connections exist, no `shared_ptr<DatabaseInstance>` is held, and the cache entry expires naturally on `close()`. Adding `LOAD` is what triggers the leak; `CREATE` is a red herring. **A test repro should call only `LOAD semantic_views` (no CREATE) and assert RO reopen works** — that's a tighter regression test than the user's reproduction.

### 2.3 Phase 62 §Q2's mistake

Phase 62 §Q2 correctly identified that `duckdb_disconnect` cannot be called from `~SemanticViewsParserInfo` because `~DatabaseInstance::connection_manager.reset()` runs first (`duckdb.cpp:276819`). Phase 62 concluded "leak the duckdb_connection; one Connection per DB ever opened; bounded." The mistake was in concluding the leak is *bounded* in a meaningful sense. The leak is bounded in bytes (one Connection ~few KB) but **unbounded in functional impact**: it makes the entire DB unreopenable in the same process until process exit. That's not a leak; that's a use-after-life bug in waiting.

The correct framing for Phase 62's destruction-order finding: "we cannot tear down at `~SemanticViewsParserInfo` time, **so we must not own the connection at that level of granularity**." That is what Phase 65 must implement.

### 2.4 Instrumentation plan for Wave 0

Before any fix, the implementer should verify the busy-spin diagnosis empirically. Two cheap instrumentation steps:

1. **CPU usage check:** Run the repro, watch `top -pid <python_pid>`. Confirm one thread at ~100% CPU during the "hang." If true → busy-spin confirmed; if false → connection-manager mutex contention or different bug; restart investigation.
2. **gdb backtrace:** `lldb -p <pid>` mid-hang, `bt all`. Expected: a thread inside `DBInstanceCache::GetInstanceInternal`, specifically at the `while (!weak_cache_entry.expired())` loop, frame should include `duckdb_open_internal` → `GetOrCreateInstance`. If the backtrace shows a futex wait instead, restart investigation.

Both instrumentation steps take <2 minutes and pin the diagnosis before code change.

---

## 3. DuckDB 1.5.2 Lifecycle Surface

### 3.1 Available C-API and C++ hooks (anchored to DuckDB v1.5.2)

| Hook | Surface | Fires when | Suitable for our use? |
|------|---------|-----------|----------------------|
| `ExtensionCallback::OnConnectionOpened` | C++ | New `Connection` registered with `ConnectionManager::AddConnection`. Fires under `connections_lock`. (`duckdb.cpp:276187-276194`) | **Yes** — canonical pattern for installing per-connection state via `ClientContext::registered_state`. Used by duckdb-postgres. |
| `ExtensionCallback::OnConnectionClosed` | C++ | Connection released, under `connections_lock`. (`duckdb.cpp:276196-276203`) | **No for direct disconnect** — calling `duckdb_disconnect` inside it would re-enter the same lock = deadlock. Could be a *signal* to schedule deferred cleanup, but adds complexity for no win. |
| `ExtensionCallback::OnBeginExtensionLoad` / `OnExtensionLoaded` / `OnExtensionLoadFail` | C++ | Extension load lifecycle. (`duckdb.cpp:276161-276168`) | **Yes for one-shot init** (registering parser hooks) — not useful for teardown. |
| `~ParserExtensionInfo` (our `~SemanticViewsParserInfo`) | C++ | DBConfig destructs, AFTER `~DatabaseInstance::connection_manager.reset()` has run. | **No for duckdb_disconnect** — UAF per Phase 62 §Q2. Safe only for Rust-side `Drop` work. |
| `duckdb_extension_unload` / equivalent | C-API | **Does not exist in DuckDB 1.5.2.** TECH-DEBT 20 confirmed. | — |
| `OnEntryClose` / `OnDatabaseDetach` on `AttachedDatabase` | C++ | Detach lifecycle (not main DB shutdown). (`duckdb.cpp:277406-277415`) | Not applicable — we don't attach a separate database. |
| `ConnectionManager::GetConnectionList` | C++ | Returns live `shared_ptr<ClientContext>` list. (`duckdb.cpp:276213-276228`) | **Yes** for retro-install of state on connections that pre-date our extension load (duckdb-postgres uses this in `OnExtensionLoaded`). |
| `duckdb_extension_loader` API (`loader.GetDatabaseInstance()`) | C-API (storage extension v2) | Loader passed to extension entry. | Available, but accessed via `duckdb_extension_access` indirection in our C_STRUCT entry — already done in `src/lib.rs:551`. |

[VERIFIED: `cpp/include/duckdb.cpp` lines as cited; `duckdb-postgres` source via WebFetch of `src/postgres_extension.cpp`]

### 3.2 What we *cannot* do in DuckDB 1.5.2

- **No DB-shutdown notification.** `~DatabaseInstance` does not invoke any extension hook. Phase 62 RESEARCH §Q2 confirmed this; re-verified in this research. The destruction order (`~DatabaseInstance` body resets `connection_manager` BEFORE `~DBConfig` runs, which then invokes `~SemanticViewsParserInfo`) means any extension-owned `duckdb_connection` is doomed by the time we could see a destructor callback.
- **No `duckdb_connection` weak handle.** `Connection` ownership through the C-API is by raw pointer (`duckdb_connection` = `Connection*`). No reference-counting on the C-side; the C++ side's `shared_ptr<ClientContext>` is opaque to the C-API. Therefore D-07-3 ("non-owning / weak handle") is not implementable with the public C-API.
- **No `parser_override` access to a `ClientContext`.** `parser_override_function_t` signature: `ParserOverrideResult (*)(ParserExtensionInfo *info, const string &query, ParserOptions &options)` (`parser_extension_compat.hpp:169-170`). No `ClientContext &`. So we cannot route catalog reads to the caller's connection from `parser_override` directly. (Contrast: `plan_function_t` does receive a `ClientContext &` — that's a possible future shape but a much larger refactor than v0.9.1's scope.)

### 3.3 What we *can* do

- **Open and close a fresh `duckdb_connection` per parser_override invocation.** From inside `sv_parser_override_rust`, call `duckdb_connect(db_handle, &fresh_conn)` at start, `duckdb_disconnect(&fresh_conn)` at end (RAII via a Rust guard type). This requires storing `db_handle` (the `duckdb_database`) in `OverrideContext` instead of `duckdb_connection`. The `db_handle` does not increment any refcount on `shared_ptr<DatabaseInstance>` directly — only `Connection` objects do — so storing `db_handle` is safe and lifecycle-neutral. [VERIFIED: `duckdb.cpp:266341-266447` — `DatabaseWrapper` is held by `DatabaseInstance::Configure` via `shared_ptr<DuckDB>`; the `duckdb_database` pointer is opaque, lifetime tied to the `DBConfig` which holds *another* path back to itself.]
  - **Caveat to validate at Wave 0:** confirm that `db_handle` (the `duckdb_database` from `(*access).get_database.unwrap()(info)` at `src/lib.rs:551`) remains a valid pointer for the lifetime of `DBConfig`. If `~DBConfig` runs *after* the `DatabaseWrapper*` has been deleted by the user's `duckdb_close`, then `db_handle` is dangling at the time `parser_override` would try to use it post-close — but post-close is exactly the time `parser_override` cannot fire (no connection, no parse). So this is safe in practice. [CITED: `duckdb.cpp:266424-266430` — `duckdb_close` deletes the wrapper, which drops the user's shared_ptr<DuckDB>; the wrapper outlives `~DBConfig` only because of our leak today.]
- **Use `ExtensionCallback::OnConnectionOpened` + `GetConnectionList` retro-install** to seed per-connection state (e.g. cached `CatalogReader`) on the *caller's* connections, if a future refactor wants per-connection state at all. **Not needed for the Phase 65 fix** — the per-DDL-connect approach removes the need.

---

## 4. Canonical Pattern Survey

### 4.1 `duckdb-postgres` (HIGH confidence — direct source read)

`PostgresExtensionCallback::OnConnectionOpened` installs `PostgresExtensionState` into `ClientContext::registered_state`:

```cpp
// src/postgres_extension.cpp (verbatim, fetched 2026-05-21)
class PostgresExtensionCallback : public ExtensionCallback {
public:
    void OnConnectionOpened(ClientContext &context) override {
        context.registered_state->Insert("postgres_extension",
            make_shared_ptr<PostgresExtensionState>());
    }
};

// And retroactively for connections that opened before the extension loaded:
for (auto &connection : ConnectionManager::Get(loader.GetDatabaseInstance()).GetConnectionList()) {
    connection->registered_state->Insert("postgres_extension",
        make_shared_ptr<PostgresExtensionState>());
}
```

**Key insight: postgres owns ZERO long-lived `duckdb_connection` handles.** State lives on the caller's `ClientContext`, which dies when the caller's `Connection` dies. No lifetime question.

Connections to *the remote Postgres database* are managed by a connection pool (`PostgresConnectionPool`) — but that's an external resource (libpq), not a `duckdb_connection`. The pool is attached to an `AttachedDatabase` (storage extension scope), not to the main `DatabaseInstance`. [VERIFIED: WebFetch of `src/postgres_extension.cpp` and `src/include/storage/postgres_connection_pool.hpp`]

### 4.2 `duckdb-mysql` (MEDIUM confidence — DeepWiki + WebFetch of extension cpp)

Same shape as postgres: a `MySQLStorageExtension` registered via `StorageExtension::Register`. Remote-connection pool (`mysql_pool_max_connections`, idle-timeout reaper thread). No long-lived `duckdb_connection` ownership at extension scope. [CITED: `deepwiki.com/duckdb/duckdb-mysql/3.1-connection-management`]

### 4.3 `duckdb-iceberg` (MEDIUM confidence — WebFetch)

Stateless: registers `IRCStorageExtension` with `create_transaction_manager` that creates an `IcebergTransactionManager` per `AttachedDatabase`. RAII via shared_ptr destruction at `~AttachedDatabase`. No long-lived `duckdb_connection`. [CITED: WebFetch of `src/iceberg_extension.cpp`]

### 4.4 Pattern summary

| Extension | Owns long-lived `duckdb_connection`? | State container | Teardown |
|-----------|--------------------------------------|-----------------|----------|
| `duckdb-postgres` | **No** | `ClientContext::registered_state` (per-connection) | Connection close (natural RAII) |
| `duckdb-mysql` | **No** | `AttachedDatabase`-scoped pool (external libpq conns) | Pool reaper + `~AttachedDatabase` |
| `duckdb-iceberg` | **No** | `AttachedDatabase`-scoped manager | `~AttachedDatabase` |
| `duckdb-semantic-views` (today) | **YES — two of them** | `DBConfig`-scoped via `SemanticViewsParserInfo` | **None (leak)** |

**We are the outlier.** Every canonical example couples extension state to a scope that destructs *before* `~DatabaseInstance` (per-connection or per-AttachedDatabase). Nobody owns long-lived `duckdb_connection`s at `DBConfig` scope. The reason is structural: `DBConfig` outlives the `DatabaseInstance` only momentarily during destruction, and connections cannot be safely closed at that boundary (Phase 62 §Q2). So the canonical answer is "don't store connections at that scope."

[ASSUMED: I have not exhaustively surveyed every community extension; conclusion is based on three representative examples plus the structural argument. Risk if wrong: low — even if one extension does own long-lived connections, the canonical *recommended* pattern is the postgres model.]

---

## 5. Long-Lived Native Handles Audit (D-03 deliverable)

Every long-lived native handle the extension currently owns and its lifecycle coupling:

| # | Handle | Site | Storage location | Coupled to `DatabaseInstance` lifetime? | Phase 65 disposition |
|---|--------|------|------------------|----------------------------------------|---------------------|
| H1 | `catalog_conn` (`duckdb_connection`) | `src/lib.rs:383-387` | `OverrideContext::catalog.conn` (Box on heap); same pointer copy-stored in 15+ `CatalogReader` instances passed to `register_table_function_with_extra_info` (read-side table functions: list, describe, show_*, get_ddl, read_yaml) at `src/lib.rs:421-490` | **NO — broken.** Keeps DatabaseInstance alive past user close. **This phase's primary fix target.** | Replace with per-call `duckdb_connect`+`duckdb_disconnect`. See §6. |
| H2 | `query_conn` (`duckdb_connection`) | `src/lib.rs:494-498` | `QueryState::conn` passed to `semantic_view` and `explain_semantic_view` table functions | **NO — broken.** Same leak as H1. **This phase's secondary fix target.** | Same fix as H1: per-call connect/disconnect inside table-function `bind`/`init`. |
| H3 | `OverrideContext` (Rust `Box`) | `src/parse.rs:2477-2481` (alloc); freed by `sv_drop_override_context` at `src/parse.rs:2499-2508` | `SemanticViewsParserInfo::rust_state` on C++ side (`cpp/src/shim.cpp:159-184`) | **YES** — `~SemanticViewsParserInfo` fires at `~DBConfig` and frees the Box. Rust-side allocation IS reclaimed. Only the inner `duckdb_connection` (H1) leaks. | After H1 fix: `OverrideContext` no longer owns a `duckdb_connection`; field becomes `db_handle: duckdb_database`. Lifetime story unchanged (still freed correctly). |
| H4 | `ParserExtension` (C++ struct in DBConfig) | `cpp/src/shim.cpp:372-393` | `DBConfig::callback_manager → ExtensionCallbackRegistry → parser_extensions` (`duckdb.cpp:281093-281098`) | **YES** — destroyed at `~DBConfig`. No native resources owned directly. | No change needed. |
| H5 | `SemanticViewsParserInfo` (C++ `shared_ptr<ParserExtensionInfo>`) | `cpp/src/shim.cpp:389-391` | Inside the `ParserExtension::parser_info` field in DBConfig | **YES** — destroyed at `~DBConfig` along with H4. | No change needed (it's the Box-owner from H3). |
| H6 | `extension` C-API state buffer (`have_api_struct`, error strings) | `src/lib.rs:539-560` | Stack-allocated; emitted into DuckDB via `set_error` callback | **YES** — scoped to extension init; nothing leaks. | No change. |
| H7 | `QueryState::catalog` (`CatalogReader` value) | `src/lib.rs:501-503` | Passed to `semantic_view`/`explain_semantic_view` registration | **NO — same as H1** (it's a Copy of H1's connection pointer). | Removed/refactored as part of H1+H2 fix. |
| H8 | `cc::Build`-compiled C++ static state (DuckDB amalgamation globals) | `build.rs` / `cc` | Per-process static linkage | Not applicable | No change (orthogonal to lifecycle). |
| H9 | `StreamingState` in `semantic_view` table function (`Mutex<Option<StreamingState>>`) | `src/query/table_function.rs:92, 746` | Inside `SemanticViewVTab` bind data; per-query | **YES** — query-scoped; cleared between executions. | No change. |
| H10 | Any global `static`, `OnceLock`, `lazy_static` | grep across `src/` | None found — Phase 62 removed the only one (the `db_token` LRU module) | — | No change (already clean). |
| H11 | `cc`-built C++ standard library / Allocator / BackgroundThread state | DuckDB amalgamation | Per-process; `Allocator::SetBackgroundThreads(false)` runs in `~DatabaseInstance` (`duckdb.cpp:276834`) | Process-scoped; not extension-owned. | No change. |

### 5.1 Items NOT folded into Phase 65 fix (per D-08 scope fence)

- **H4–H6, H8–H11:** Correctly coupled or out of scope by construction. No action.
- **H7:** Mechanically fixed as a consequence of H1/H2 fix, not a separate finding.

### 5.2 Items surfaced as new findings (per D-03)

**None new.** H1 and H2 are the two long-lived `duckdb_connection`s; both are this phase's fix target. The audit confirms the bug is localized and the fix is bounded — no separate follow-up phase needed for adjacent broken lifecycle patterns. [VERIFIED: grep across `src/` for `duckdb_connect`, `OnceLock`, `Once::`, `lazy_static`, `RwLock`, `Mutex` — H9 is the only `Mutex`; per-query scope]

### 5.3 RO→RW reverse direction (D-09)

Same mechanism as RW→RO: any access-mode-mismatching reopen of the same path in the same process hits the busy-spin via the same shared cache key (path is the only key component — `duckdb.cpp:277971-277986`). If RW→RO is fixed by removing H1+H2, RO→RW is **fixed as a side effect** because the same connections no longer leak. No extra work needed. **Recommend the regression test also cover RO→RW** to pin this behaviour.

---

## 6. Recommendation

### 6.1 Choice: D-07 candidate 1 — Don't cache; per-DDL connect/disconnect

**For H1 (`catalog_conn`):** Remove the cached connection. Inside `sv_parser_override_rust`, open a fresh `duckdb_connection` on demand, run the catalog read/enrichment work, close it before returning. Pass `db_handle: duckdb_database` (not `duckdb_connection`) through `OverrideContext`.

**For H2 (`query_conn`):** Move connection ownership into the `semantic_view` / `explain_semantic_view` table function's `bind` (or `init`) callback. Open during bind, close during destruction of bind data. The `BindData` lifetime is tied to the user's query execution on the user's connection — the extension's internal connection survives only as long as the user's query is in flight.

**For read-side table functions (list, describe, show_*, get_ddl, read_yaml):** Same fix. The current pattern passes `&catalog_reader` (a `Copy` of H1's pointer) to `register_table_function_with_extra_info`. Replace `CatalogReader` with a thin handle that stores `db_handle` and opens a fresh `duckdb_connection` inside each `bind`. Pattern is uniform across all sites.

### 6.2 Why not D-07 candidate 2 (deterministic teardown)

DuckDB 1.5.2 has no safe teardown point for `duckdb_disconnect` of an extension-owned connection:

- `~SemanticViewsParserInfo` runs after `connection_manager.reset()` — UAF (Phase 62 §Q2).
- `OnConnectionClosed` fires under `connections_lock`; calling `duckdb_disconnect` from inside re-enters `RemoveConnection` which re-locks the same mutex (`duckdb.cpp:276196-276203`). `std::mutex` is non-recursive — deadlock.
- No `OnDatabaseShutdown` callback exists.
- Deferred-cleanup variants (set a flag in `OnConnectionClosed`, drain on next call) require background threads, add a new long-lived handle, and are inferior to "just don't cache."

### 6.3 Why not D-07 candidate 3 (weak handle)

The C-API does not expose a weak-to-shared upgrade for `duckdb_connection`. The C++ `Connection` constructor takes a `DatabaseInstance &` and calls `shared_from_this()`, which always increments the shared_ptr refcount — there is no "borrow" variant. Adding one upstream would be a DuckDB API change, not a v0.9.1 patch milestone deliverable.

### 6.4 Why not D-07 candidate 4 (documented limitation)

Per CONTEXT.md D-01, this is admissible only if (a) is impossible. (a) is straightforward (§6.1). Documenting the limitation would fail the spirit of LIFE-02 even though its letter permits (b). Reject.

### 6.5 Cost analysis for per-DDL connect/disconnect

- **`duckdb_connect`** calls `new Connection(*wrapper->database)` (`duckdb.cpp:266438-266440`) → allocates `Connection` + `ClientContext` + registers with `ConnectionManager` (one mutex acquire). Cost: **~microseconds** on a warm process.
- **`duckdb_disconnect`** = `delete Connection*` → `~Connection` → `RemoveConnection` (same lock acquire). Cost: **~microseconds**.
- Per CREATE SEMANTIC VIEW: 1 connect + N catalog queries (lookup + LIMIT 0 probes + fact typing) + 1 disconnect. Compared to the existing CREATE flow (JSON parse + graph validation + INSERT), the connect/disconnect overhead is **<0.1%** of total CREATE time.
- Per DROP/ALTER: 1 connect + 1-2 catalog queries + 1 disconnect. Same negligible overhead.
- Per `semantic_view(...)` query: 1 connect (in bind) + N expansion queries (LIMIT 0 probes for fact types) + 1 disconnect (in bind-data destructor). Negligible vs. the actual aggregate query.

**Lock-contention risk (Phase 58/62 premise that motivated separation):** The original concern was "running DDL on the user's connection would deadlock on the user's execution lock." That concern remains valid — and the per-call connect/disconnect *preserves* the separation. We still have a separate connection for catalog reads; we just don't keep it open. There is no regression in lock-contention behaviour. [VERIFIED: `duckdb.cpp:276187` — `connections_lock` is per `ConnectionManager`, not per Connection; adding/removing connections doesn't block the existing connection's queries.]

### 6.6 Alternative considered: route catalog reads to the caller's connection

A more invasive refactor would be: move all parser_override catalog work into `plan_function`, which DOES receive a `ClientContext &`. Then we don't need our own connection at all — catalog reads run on the caller's connection.

**Why deferred:** This is a substantial architectural change (the `parser_override` → native-SQL pattern would have to become `parser_override` → `parse_function` → `plan_function`). It's the future canonical shape, but out of scope for a patch milestone. **Surfaced as a new TECH-DEBT candidate** (see §9).

---

## 7. Validation Architecture

### 7.1 Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (unit + proptest, default `bundled` feature), `just test-sql` (sqllogictest), Python integration tests for ADBC / concurrent / multi-DB / read-only |
| Quick run command (per task commit) | `cargo test` |
| Per-wave merge | `just test-all` |
| Phase gate | `just ci` (lint + test-all + fuzz compile + docs-check) |
| Watchdog dependency | `pytest-timeout` OR Python `threading.Timer`-based watchdog (no extra dep) |

### 7.2 Behavioural Requirements → Test Map

| ID | Behaviour | Test Type | Automated Command | Status |
|----|-----------|-----------|-------------------|--------|
| **B1 (LIFE-01)** | After RW close+drop, `duckdb.connect(path, read_only=True)` returns within 5s on a freshly bootstrapped DB | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_fresh` | ❌ Wave 0 — new test |
| **B2 (LIFE-01)** | After RW close+drop, RO reopen returns within 5s on a previously-bootstrapped DB | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_existing` | ❌ Wave 0 — new test |
| **B3 (LIFE-01 isolation)** | After RW close+drop with **only `LOAD semantic_views`** (no CREATE), RO reopen returns within 5s | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_load_only_then_readonly` | ❌ Wave 0 — new test; isolates the leak to extension load, not CREATE |
| **B4 (D-09)** | RO→RW reverse direction also returns within 5s (regression guard for the same root cause) | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_readonly_then_readwrite` | ❌ Wave 0 — new test |
| **B5 (existing subprocess tests)** | Existing subprocess-bootstrap tests still pass | Python integration | `uv run test/integration/test_readonly_load.py::test_bootstrapped_readonly_query_works` etc. | ✅ exists — must remain green |
| **B6 (parser_override correctness preserved)** | All Phase 62 transactional DDL tests still pass | sqllogictest | `just test-sql` — `test/sql/v080_transactional_ddl.test` | ✅ exists — must remain green byte-identical |
| **B7 (caret tests preserved)** | All Phase 62 caret-rendering tests still pass | Python integration | `just test-caret` — `test/integration/test_caret_position.py` | ✅ exists |
| **B8 (read-side table function correctness)** | `list_semantic_views`, `describe_semantic_view`, `show_*`, `get_ddl`, `read_yaml_from_semantic_view` all still return correct results when their internal connection is per-call instead of cached | sqllogictest | `just test-sql` — various existing read-side tests | ✅ exists — must remain green |
| **B9 (multi-DB isolation preserved)** | Multi-DB scenario from Phase 62 (`test_multi_db_isolation.py`) still passes | Python integration | `just test-multi-db` | ✅ exists |
| **B10 (concurrent CREATE)** | Phase 60 concurrent-CREATE behaviour unchanged | Python integration | `just test-concurrent` | ✅ exists |
| **B11 (no Connection leak under repeated LOAD+close)** | Open + close 50 file-backed DBs sequentially in one process, each running `LOAD semantic_views` + `CREATE SEMANTIC VIEW` + close; assert no busy-spin observed and RSS bounded | Python integration | new in `test_readonly_load.py` or `test_multi_db_isolation.py` | ❌ Wave 0 — extended-loop test |
| **B12 (ADBC unchanged)** | ADBC transactional DDL still passes | Python integration | `just test-adbc` | ✅ exists |
| **B13 (Rust-side: OverrideContext no longer carries duckdb_connection)** | `OverrideContext` struct has `db_handle: duckdb_database`, NOT `catalog: CatalogReader` | Rust unit / compile-time | `cargo test --lib --features extension` + grep audit `rg "OverrideContext.*conn:|catalog: CatalogReader"` returns nothing | ❌ Wave 0 — new structural test |
| **B14 (Rust-side: per-call connect closes deterministically)** | A new RAII guard type wraps `duckdb_connect`/`duckdb_disconnect`; Drop closes the connection. Test instantiates inside scope, asserts behaviour. | Rust unit | new test in `src/parse.rs` or a new `src/conn_guard.rs` mod | ❌ Wave 0 |

### 7.3 Watchdog Test Pattern (LIFE-03 specifics)

```python
import duckdb, threading, gc, time, tempfile
from pathlib import Path

def _connect_with_watchdog(path, watchdog_seconds=5, **kwargs):
    """connect-with-watchdog: fails fast instead of busy-spinning forever."""
    result = {"conn": None, "exc": None}
    def _do():
        try:
            result["conn"] = duckdb.connect(path, **kwargs)
        except BaseException as e:
            result["exc"] = e
    t = threading.Thread(target=_do, daemon=True)
    start = time.monotonic()
    t.start()
    t.join(timeout=watchdog_seconds)
    elapsed = time.monotonic() - start
    if t.is_alive():
        raise TimeoutError(
            f"duckdb.connect({path!r}, **{kwargs!r}) did not return within "
            f"{watchdog_seconds}s — likely the in-process RW→RO busy-spin "
            f"(Phase 65 regression)"
        )
    if result["exc"]:
        raise result["exc"]
    return result["conn"], elapsed

def test_in_process_bootstrap_then_readonly_fresh():
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "fresh.duckdb")
        w = open_writable(db)
        w.execute("CREATE TABLE t (i INT)")
        w.execute("CREATE SEMANTIC VIEW v AS "
                  "  TABLES (t1 AS t PRIMARY KEY (i)) "
                  "  DIMENSIONS (t1.i AS t1.i) "
                  "  METRICS (t1.c AS COUNT(*))")
        w.close()
        del w
        gc.collect()
        ro, elapsed = _connect_with_watchdog(db, watchdog_seconds=5, read_only=True)
        try:
            assert elapsed < 5.0
            ro.execute("LOAD semantic_views")
            names = [r[0] for r in ro.execute(
                "SELECT name FROM list_semantic_views()"
            ).fetchall()]
            assert names == ["v"]
        finally:
            ro.close()
```

**Critical:** the daemon thread must remain running if it busy-spins (it cannot be safely killed from Python — DuckDB's C++ code is uninterruptible from Python). On v0.9.0 baseline this means the test thread leaks for the rest of the process — acceptable for a fail-once regression test, but document the behaviour. On v0.9.1 the thread will return cleanly. **Run this test in CI under `pytest-timeout` or as the LAST test in the file** to avoid leaking the thread into subsequent tests.

### 7.4 Sampling Rate

- **Per task commit:** `cargo test` (Rust unit + proptest)
- **Per wave merge:** `just test-all` (full suite including new `test_readonly_load.py` cases)
- **Phase gate:** `just ci` green before `/gsd:verify-work`

### 7.5 Wave 0 Gaps

- [ ] `test/integration/test_readonly_load.py` — add B1, B2, B3, B4, B11 tests + `_connect_with_watchdog` helper. Keep existing subprocess-based tests intact (B5).
- [ ] `src/parse.rs` (or new `src/conn_guard.rs`) — RAII guard type wrapping `duckdb_connect`/`duckdb_disconnect`, with proptest-style coverage that Drop closes exactly once.
- [ ] `src/lib.rs` — refactor `init_extension` to not own H1/H2; pass `db_handle` into `OverrideContext` and into `QueryState`. Rewire all read-side `register_table_function_with_extra_info` call sites to use a new handle type.
- [ ] `src/catalog.rs::CatalogReader` — refactor: either (a) accept `db_handle` and connect/disconnect inside each method, or (b) accept a `duckdb_connection` borrowed for the call's duration and have callers manage the guard.
- [ ] **Compile-time check** (B13): static_assert-like Rust trick or a doc-test that fails to compile if `OverrideContext` re-acquires a `duckdb_connection` field.

### 7.6 Pinned regressions to verify

After fix, re-run these scenarios — they exercise paths that touched H1/H2:

1. `just test-sql` — Phase 62 transactional DDL tests (`test/sql/v080_transactional_ddl.test`)
2. `just test-caret` — Phase 62 caret rendering
3. `just test-multi-db` — Phase 61 LRU-removal sequel
4. `just test-concurrent` — Phase 60 race guards
5. `just test-adbc` — Phase 58 ADBC autocommit=false
6. All read-side table function sqllogictests (the `&catalog_reader` Copy path)

---

## 8. Trade-off Documentation (LIFE-02 reasoning, for the success-criterion-2 record)

**Decision:** Use D-07 candidate **1** (don't cache; per-DDL connect/disconnect). Anchored to DuckDB v1.5.2.

**Considered and rejected:**

| Candidate | Rejected because |
|-----------|------------------|
| **D-07-2** Cache + deterministic teardown via `~DBConfig` destructor | UAF (Phase 62 §Q2): `~DatabaseInstance::connection_manager.reset()` runs before `~SemanticViewsParserInfo`, making `duckdb_disconnect` undefined behaviour. DuckDB 1.5.2 has no extension-unload hook. `OnConnectionClosed` re-enters `connections_lock` → deadlock. |
| **D-07-3** Non-owning / weak handle | DuckDB 1.5.2 C-API does not expose a weak-to-shared upgrade primitive for `duckdb_connection`. Would require an upstream API change. |
| **D-07-4** Detect access-mode mismatch + error early | Per CONTEXT.md D-01, admissible only if (a) is impossible. (a) is achievable; (4) would be intentionally shipping a worse fix. |

**Mechanism summary for D-07-1:** Store `db_handle: duckdb_database` (a pointer that does NOT increment `shared_ptr<DatabaseInstance>` refcount) in `OverrideContext` and `QueryState`. Each catalog-read site opens a fresh `duckdb_connection` via RAII guard and closes it before returning. Per-call overhead: ~µs. Lock-contention behaviour: identical to today (still separate from caller's connection). Memory: zero long-lived extension-owned `Connection` objects.

**Trade-offs accepted:**
- Slight per-DDL overhead (~µs per CREATE/DROP/ALTER) — negligible vs. the rest of the operation.
- Slight per-query overhead for `semantic_view(...)` (1 connect at bind, 1 disconnect at bind-data Drop) — negligible vs. expansion + execution.
- Code complexity: a new RAII guard type (~30 LOC) + plumbing changes in 5-6 sites.

**Trade-offs avoided:**
- The busy-spin lifetime bug (the entire reason for this phase).
- Future similar bugs (the canonical pattern from duckdb-postgres is now what we follow).
- TECH-DEBT 20's resolution stays clean — the LRU removal is preserved; we just additionally don't cache the underlying connection.

**Anchored to DuckDB v1.5.2** (`DUCKDB_VERSION = "v1.5.2"` in `cpp/include/duckdb.hpp`; libduckdb-sys `=1.10502.0` per `Cargo.toml`). If DuckDB ≥ 1.6 adds an extension-unload hook or a weak-handle primitive, **D-07-2 or D-07-3 become viable** and could be revisited — but the per-call shape from this phase will also continue to work, so the refactor would be an optimization, not a correctness fix.

---

## 9. Surfaced Findings (per D-03 — items NOT folded into Phase 65 fix)

### 9.1 New TECH-DEBT candidate: route `parser_override` catalog reads through caller's `ClientContext`

The `parser_override_function_t` signature does not pass a `ClientContext`, but `plan_function_t` does. A future refactor could replace the current `parser_override` → native-SQL pattern with a `parse_function` → `plan_function` shape that routes catalog reads to the caller's connection directly — eliminating the need for the extension to own ANY internal connection. This is the postgres/iceberg pattern (state on caller's `registered_state`).

**Why not now:** This is a milestone-sized refactor that would touch every CREATE/DROP/ALTER code path and likely break the caret-rendering work from Phase 62. Phase 65 ships a tactical fix; this is the strategic follow-up.

**Suggested TECH-DEBT entry:** `TECH-DEBT 25 — Route catalog reads through caller's ClientContext` (deferred from v0.9.1).

### 9.2 Existing TECH-DEBT 19 (DESCRIBE/SHOW read committed state) intersects this work

TECH-DEBT 19 is "DESCRIBE/SHOW currently see only committed state because they run on the extension's `catalog_conn`." Phase 65's per-call connect doesn't change that — a fresh connection still sees committed state only. The fix for TECH-DEBT 19 requires `libduckdb-sys` to expose `BindInfo`'s connection handle, which is upstream and unchanged. **Phase 65 does not regress TECH-DEBT 19, nor does it fix it.** Document this explicitly.

### 9.3 In-memory DB path needs verification

The `db_handle` for `:memory:` databases is per-process unique (no cache key collision). The busy-spin only applies to file-backed DBs (the cache lookup gate at `duckdb.cpp:278105-278107` returns `cache_instance=false` for `:memory:` and unnamed in-memory). **Verify at Wave 0 that the per-call connect/disconnect works correctly against `:memory:` too** — likely fine (no cache, no busy-spin path), but add an explicit smoke test.

---

## 10. Project Constraints (from CLAUDE.md)

The following directives MUST be honoured by the plan:

- **Quality gate:** `just test-all` MUST pass before phase verification. Phase 65 changes touch `src/lib.rs` (init_extension) and `src/parse.rs` (OverrideContext + rewrite_create/rewrite_drop_or_alter) — both are extension-feature-only and not exercised by `cargo test` without sqllogictest + Python integration. **A verification that only runs `cargo test` is incomplete.**
- **Pre-push:** `just ci` adds lint (clippy pedantic + fmt + cargo-deny) and fuzz compile.
- **Branch:** all work on `milestone/v0.9.1`. Current branch confirmed `milestone/v0.9.1` at research time. Verify before every commit.
- **Parallel builds forbidden:** never run `cargo` / `make` in parallel.
- **No worktrees** (feedback `feedback_worktree_isolation.md`).
- **No long-running commands piped to bare `tail`** — redirect to `$TMPDIR` first.
- **No `run_in_background` for GSD executors** (feedback `feedback_no_background_agents.md`).
- **Snowflake reference for SQL syntax** — N/A for this phase (pure lifecycle/internals).
- **Versioning:** Phase 65 does NOT bump Cargo.toml / description.yml / CHANGELOG. That's Phase 66 (REL-02).

---

## 11. Sources

### Primary (HIGH confidence — direct code reading)

- **DuckDB v1.5.2 amalgamation** (`cpp/include/duckdb.cpp`, vendored):
  - `Connection::Connection` + `~Connection` — lines 275773-275799 (shared_ptr<DatabaseInstance> increment via `database.shared_from_this()`)
  - `ConnectionManager::AddConnection` / `RemoveConnection` — lines 276184-276203 (locking, callback firing)
  - `~DatabaseInstance` destruction order — lines 276813-276837 (`connection_manager.reset()` first)
  - `~DBConfig` — line 276805
  - `ExtensionCallback` class — lines 276149-276177 (available callback surface)
  - `duckdb_open_internal` / `duckdb_close` / `duckdb_connect` / `duckdb_disconnect` — lines 266361-266479 (C-API entry points)
  - `DBInstanceCache::GetInstanceInternal` — lines 277995-278038 (**the busy-spin: lines 278022-278024**)
  - `DBInstanceCache::GetOrCreateInstance` — lines 278090-278129
  - `GetCacheKey` — lines 277971-277986 (path-only key, RW and RO share)
  - `DatabaseInstance::Configure` — lines 277153-277230 (config.db_cache_entry assignment)
  - `DatabaseFilePathManager::InsertDatabasePath` — lines 277348-277388 (separate path-lock; not the hang mechanism)
- **`cpp/include/duckdb.hpp`** — `#define DUCKDB_VERSION "v1.5.2"`; `DatabaseCacheEntry` declaration (lines 56880-56885); `db_cache_entry` field (line 40102)
- **`cpp/include/parser_extension_compat.hpp`** — `parser_override_function_t` signature (line 169), `plan_function_t` signature (line 121)
- **`cpp/src/shim.cpp`** — `~SemanticViewsParserInfo` (lines 162-184), `sv_register_parser_hooks` (lines 354-407)
- **`src/lib.rs`** — `init_extension` (lines 340-517), H1 site (lines 383-387), H2 site (lines 494-498), C_STRUCT entry (lines 539-599)
- **`src/parse.rs`** — `OverrideContext` definition (lines 47-71), `Drop` impl with Phase 62 §Q2 comment (lines 53-71), `sv_make_override_context`/`sv_drop_override_context` (lines 2446-2508), `rewrite_to_native_sql` (lines 1715-1768), `rewrite_create` + catalog read site (lines 1820-1935)
- **`src/catalog.rs`** — `CatalogReader` Copy semantics (lines 97-124)
- **`src/query/table_function.rs`** — `QueryState::conn` usage (lines 600, 661, 703, 767)
- **`.planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md`** §Q2 — the destruction-order trace this phase re-litigates
- **`.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md`** — the deferred entry being closed by LIFE-04
- **`test/integration/test_readonly_load.py`** — existing test patterns, `bootstrap_in_subprocess` helper

### Secondary (MEDIUM confidence — external source via WebFetch)

- **`duckdb-postgres`** `src/postgres_extension.cpp` (WebFetched 2026-05-21) — `PostgresExtensionCallback::OnConnectionOpened` + `ConnectionManager::Get(loader.GetDatabaseInstance()).GetConnectionList()` pattern. **Anchors the canonical pattern.**
- **`duckdb-iceberg`** `src/iceberg_extension.cpp` (WebFetched 2026-05-21) — StorageExtension::Register pattern, no long-lived `duckdb_connection`.
- **`duckdb-mysql`** Configuration via DeepWiki summary — pool model, no `duckdb_connection` ownership at DBConfig scope.

### Tertiary (LOW — none required)

- General DuckDB extension-lifecycle web searches returned only marketing-doc-level material; not load-bearing for any claim above. Anchored everything to vendored amalgamation source.

---

## 12. Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| **A1** | `db_handle` (`duckdb_database`) remains a valid pointer for the entire lifetime of `DBConfig`, including the moments when `parser_override` could fire | §3.3 | LOW — would manifest as immediate segfault in any per-DDL connect path, caught by Wave 0 instrumentation. Backed by `duckdb_close` simply deleting the wrapper (`duckdb.cpp:266424-266430`); a parser callback can't fire on a closed handle. |
| **A2** | DuckDB-postgres pattern (`OnConnectionOpened` + `registered_state`) is representative of the canonical extension-state pattern in DuckDB 1.5.x | §4 | LOW — three independent examples (postgres, mysql, iceberg) all converge to "no long-lived `duckdb_connection` at extension scope." If a fourth extension uses long-lived connections, our diagnosis still holds because we proved the structural argument (no safe teardown point exists). |
| **A3** | `duckdb_connect` / `duckdb_disconnect` per-call cost is ~µs and negligible relative to other CREATE/DROP/ALTER work | §6.5 | LOW — cost is one `new`+lock-acquire vs. multi-query CREATE. If it's actually meaningful (say >100ms per CREATE), the plan can move to a per-statement-scope connection (one connect at parser_override entry, one disconnect at exit) instead of per-catalog-query. Same architectural shape; finer granularity. |
| **A4** | The user's report of ">45s hang" is the busy-spin, not a real wait/lock | §2.1, §2.4 | LOW — Wave 0 instrumentation step (`top -pid`, `lldb bt`) confirms or refutes within minutes. If it's NOT the busy-spin, the fix is wrong and the plan must restart. |
| **A5** | DuckDB v1.5.2 has no public extension-unload hook | §3.1, §3.2, §6.2 | LOW — confirmed by Phase 62 §Q2 (2026-05-06) and re-checked in this research's grep of `ExtensionCallback` in the amalgamation. If 1.5.x adds one in a patch release we'd know via the amalgamation update. |
| **A6** | Read-side table function `bind` callbacks have access to the database handle (so per-call connect is possible) | §6.1 | MEDIUM — needs Wave 0 verification. `duckdb::vtab::BindInfo` exposes some methods; whether it surfaces `duckdb_database` for the Rust binding crate version we use (`duckdb=1.10502.0`) needs an explicit grep / Wave 0 spike. **If NOT exposed, the plan must store `db_handle` adjacent to the table-function `extra_info` (e.g., a CopyByValue wrapper that survives `register_table_function_with_extra_info`)** — also workable, just different plumbing. |
| **A7** | `parser_override` re-entrancy is safe (opening a new connection from inside `parser_override` while DuckDB is mid-parse on the caller's connection) | §6.1 | MEDIUM — `parser_override` runs on the caller's thread before query execution begins. Connecting at that point should be safe (no caller-side execution lock yet). Wave 0 spike should add a single test that triggers `parser_override` and verifies a nested `duckdb_connect`+`duckdb_disconnect` returns cleanly. |

**All other claims** in this research are tagged VERIFIED via direct reading of code in the repo or the vendored amalgamation.

---

## 13. Open Questions

### 13.1 A6 — `BindInfo` exposure of `db_handle` for read-side table functions

**What we know:** The `register_table_function_with_extra_info` API takes our `CatalogReader` by reference and DuckDB-rs forwards it to the function's bind callback via some mechanism. The `extra_info` is opaque and survives for the table function's registered lifetime.

**What's unclear:** Whether the Rust binding crate `duckdb=1.10502.0` exposes a way for the bind callback to retrieve `duckdb_database` from `BindInfo`. If yes — direct per-call connect. If no — the extra_info itself must carry `db_handle` (i.e., store `db_handle` inside the `CatalogReader`-replacement struct).

**Recommendation:** Wave 0 spike: write a minimal test that prints the available `BindInfo` methods on `duckdb 1.10502.0` and confirms which approach is needed. Both approaches are viable; this just determines whether the new struct carries `db_handle` or borrows it from `BindInfo`.

### 13.2 A7 — Parser-override re-entrancy

**What we know:** `parser_override` runs in `Parser::ParseQuery` before any execution begins on the caller's connection. The user's `ClientContext` is not yet in `QueryExecution` state.

**What's unclear:** Whether `duckdb_connect` from inside `parser_override` triggers any global lock that the caller's parse holds.

**Recommendation:** A single Wave 0 smoke test — instrument `sv_parser_override_rust` to call `duckdb_connect`+`duckdb_disconnect` on the spot (before any real work), run `CREATE SEMANTIC VIEW`, assert no deadlock. <10 LOC change, <5 minute test cycle.

### 13.3 Behaviour against in-memory DBs (per §9.3)

**What we know:** `:memory:` paths bypass `DBInstanceCache` (`cache_instance=false` per `duckdb.cpp:278106`); no busy-spin path.

**What's unclear:** Whether anything in our per-call connect path assumes file-backed DB.

**Recommendation:** Add a smoke test that runs the full LOAD+CREATE+close+reconnect-readonly cycle on `:memory:` — should succeed trivially because in-memory DBs aren't cached at all.

---

## 14. Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| **LIFE-01** | After RW close in same Python process, RO reopen returns within 5s on both fresh and previously-bootstrapped DBs | §2 (root cause), §6 (fix), §7.2 B1-B4 (tests) |
| **LIFE-02** | Either deterministic teardown OR access-mode mismatch detection; choice documented in RESEARCH.md | §6 (chosen: per-DDL connect = D-07-1, a stronger form of "deterministic teardown" — no extension-owned connection ever lives past its catalog query), §8 (trade-off record) |
| **LIFE-03** | `test_in_process_bootstrap_then_readonly` added to `test/integration/test_readonly_load.py`, without subprocess, with watchdog | §7.3 (watchdog pattern), §7.5 (Wave 0 gaps) |
| **LIFE-04** | `deferred-items.md` updated in-place with resolution + forward pointer to v0.9.1 | §9 (surfacing findings) + planner task — straightforward edit of `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` last section |

---

## 15. Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Python 3.10+ | LIFE-03 watchdog test | ✓ | 3.x | — |
| `duckdb` Python (==1.5.2) | Existing tests | ✓ | 1.5.2 | — |
| `cargo` + Rust toolchain pinned in `rust-toolchain.toml` | Rust unit tests | ✓ | per pin | — |
| `lldb` / `gdb` | §2.4 instrumentation step | ✓ on macOS | — | `py-spy` or `pyrasite` for Python-level traces |
| `pytest-timeout` | LIFE-03 watchdog (optional convenience) | not required | — | `threading.Timer` in §7.3 pattern works without it |

No external service dependencies. All Phase 65 work is local code + tests.

---

## §16 — B-prime Architecture (replaces Option A — refreshed 2026-05-23)

**Researched:** 2026-05-23 (post Option A falsification + B-prime end-to-end empirical validation)
**Scope:** Replaces the prior §16 (Option A — `parse_function`+`plan_function` with `context.Query`-on-caller's-conn). Option A has been **empirically falsified** by the Plan 02 Wave-0 spikes (`A2-DEADLOCK` + `BIND-THREAD-RC1`); B-prime has been **empirically validated** end-to-end via the Option B and read-path spikes. §§1–15 above remain authoritative as background (busy-spin root cause, lifecycle surface, canonical pattern survey, handle audit). The chosen mechanism, however, is **B-prime: per-call C++ `Connection(*context.db)` from every callback that has `ClientContext &`** — NOT the §1 / §6.1 "per-DDL `duckdb_connect` from `sv_parser_override`" recommendation, which is also falsified (D-10, `sqllogictest` 43/47 evidence from Plan 02 partial).
**DuckDB version anchored to:** v1.5.2 (`DUCKDB_VERSION` in `cpp/include/duckdb.hpp`; extension API `1.10502.0` per `Cargo.toml`)

---

### 16.1 Status of prior assumptions (refreshed under B-prime)

| Section | Status under B-prime | Notes |
|---------|---------------------|-------|
| §1 Executive Summary "viable shape is D-07-1 (per-call `duckdb_connect` from inside `sv_parser_override_rust`)" | **FALSIFIED** | Per D-10 + Plan 02 partial sqllogictest evidence: `duckdb_connect(db_handle)` from inside `sv_parser_override` returns rc=1 (43/47 PASS regression). The *direction* (eliminate the long-lived handle) remains correct; the *location and mechanism* shift to B-prime — per-call C++ `Connection(*context.db)` at plan-time + read-side bind-time. |
| §2 Busy-spin diagnosis | **STILL VALID** | The root cause Phase 65 fixes is unchanged. Plan 01 Spike A4 reconfirmed the busy-spin via lldb. |
| §3 Lifecycle surface | **STILL VALID** | `plan_function` now confirmed as the right write-path entry (it receives `ClientContext &`; no `context_lock` re-entry since we do not call `context.Query`). Bind-callback access to `ClientContext &` is achievable via the C++ Catalog API (read-path spike empirical evidence). |
| §3.3 "open and close a fresh `duckdb_connection` per parser_override invocation" | **FALSIFIED at the C-API path** | The C-API `duckdb_connect(db_handle)` path returns rc=1 at every lifecycle phase tested (parse / bind / plan). The C++ direct `Connection(*context.db)` path succeeds at every lifecycle phase tested (plan: Option B spike; bind: read-path spike). B-prime uses the C++ direct path exclusively. |
| §4 Canonical pattern survey | **STILL VALID; corroborated** | Adds the duckdb-postgres `Catalog::CreateTableFunction` shape as a *direct* empirical reference for B-prime's read-path registration (their state lives in `registered_state`; ours lives nowhere — we derive a per-call Connection from `*context.db` instead). |
| §5 Long-lived handles audit (H1 catalog_conn, H2 query_conn) | **STILL VALID — both retired under B-prime** | Disposition in §16.4 below (B-prime audit). The audit is generalised in §17 to all native handles, not just connections. |
| §6.1 "per-DDL `duckdb_connect` from `sv_parser_override_rust`" | **FALSIFIED** | Replaced by B-prime: per-call `Connection(*context.db)` from `sv_plan_function` (write path) and from each read-side bind callback registered via C++ Catalog API (read path). |
| §6.2–6.4 (why not D-07-2 / 3 / 4) | **STILL VALID** | All three rejections still hold. B-prime is a stronger form of "deterministic teardown" — no extension-owned connection survives any callback's scope. |
| §7 Validation Architecture (B1..B14) | **STILL VALID; EXTENDED** | B1..B14 unchanged. New read-side watchdog tests added per D-22 scope expansion: variants of B1..B4 that exercise SELECT against `list_*` / `describe_*` / `show_*` / `get_ddl` / `read_yaml_from_semantic_view` / `semantic_view` / `explain_semantic_view` after close+reopen, proving the read path no longer leaks Database lifetime. |
| §9.1 (route catalog reads via `plan_function`) | **PROMOTED TO IN-SCOPE — but with a critical correction** | The §9.1 forward-looking finding suggested `parse_function`+`plan_function`. **B-prime does this, but does NOT execute the rewritten SQL via `context.Query` (Option A2 — falsified by `A2-DEADLOCK`).** Instead it builds a `ParserExtensionPlanResult` whose `TableFunction` drives the rewritten `INSERT INTO semantic_layer._definitions ... RETURNING name AS view_name` through the binder onto the caller's conn — the Phase 58 rewrite-to-native pattern, unchanged. The per-call C++ Connection inside `sv_plan_function` is used ONLY for catalog READS (lookup / type probes); the catalog WRITE rides the binder, preserving transactional DDL semantics (D-20 non-negotiable). |
| §13 Open questions (A6, A7) | **CLOSED** | A6: `BindInfo` does NOT expose `db_handle` in duckdb-rs 1.10502.0 (Plan 01 Spike A6 + the libduckdb-sys C API grep in §16.6 of the prior §16). B-prime sidesteps this by registering read-side table functions through the C++ Catalog API directly. A7: parser_override re-entrancy is moot under B-prime — `sv_parser_override` is *deregistered entirely* per D-22 path (a); no extension callback runs during parse anymore. |

**One-line summary:** the per-call C++ `Connection(*context.db)` mechanism succeeds at every lifecycle phase where `ClientContext &` is reachable (plan, bind), and is empirically validated end-to-end via two independent spikes. The C-API `duckdb_connect(db_handle)` path is dead at every lifecycle phase tested. The cached-`db_handle` pattern (Phase 62) is identified as the underlying root defect (D-15). B-prime deliberately bypasses that defect by never caching `db_handle` — always derive Connection from the live `ClientContext`.

---

### 16.2 The B-prime architecture

**Single sentence:** Every callback that receives `ClientContext &` opens a fresh `Connection(*context.db)` inside a `ConnGuard` RAII wrapper for its catalog-read needs, and drops the guard before the callback returns.

**Write path (CREATE / DROP / ALTER SEMANTIC VIEW):**

1. `sv_parser_override` is **deregistered entirely** (per D-22 path (a), recommended in CONTEXT.md decisions). This is cleaner than demoting it to validation-only (path (b)) because it removes one whole callback to reason about and renders TECH-DEBT 21 (`disable_peg_parser` resets the override setting) moot for this extension. The cost — the default Postgres parser fails on the unrecognised `CREATE SEMANTIC VIEW` prefix — is precisely what triggers DuckDB's `parse_function` dispatch chain at `duckdb.cpp:347281-347300`. Confirmed empirically viable in the Option B spike (the spike used `SPIKE_PLAN_DUCKDB_CONNECT_PROBE` as the unrecognised prefix; the same dispatch chain handles `CREATE SEMANTIC VIEW`). [VERIFIED: `65-OPTION-B-SPIKE.md`]

2. `sv_parse_function` is **promoted to the success-path entry.** Detects the `CREATE SEMANTIC VIEW` / `DROP SEMANTIC VIEW` / `ALTER SEMANTIC VIEW` / `DESCRIBE …` / `SHOW …` prefixes; runs the structural body parse via `validate_and_rewrite` (existing helper in `src/parse.rs:963` — no catalog reads, pure syntax); stashes the validated form and parse-time metadata (verb / view_name / flags / source-location) into `SemanticViewParseData`'s payload; returns `PARSE_SUCCESSFUL`. `parse_function` runs *after* the inner `PostgresParser` scope at `duckdb.cpp:347253-347277` has fully destructed — outside the parse-thread D-10 / `BIND-THREAD-RC1` failure region. **No catalog reads happen at parse time** (parse_function does not receive `ClientContext &` — signature `(ParserExtensionInfo *info, const string &query)`).

3. `sv_plan_function` is **promoted to the catalog-read + emission entry.** Receives `ClientContext &context`. Inside `sv_plan_function`:

   ```cpp
   // Empirically validated by 65-OPTION-B-SPIKE.md Probe 1
   auto *parse_data = static_cast<SemanticViewParseData *>(parse_data_ptr.get());
   string native_sql;
   {
       Connection probe(*context.db);
       duckdb_connection conn = reinterpret_cast<duckdb_connection>(&probe);
       // Catalog READS (lookup / LIMIT 0 type probes / fact typing) run on
       // `conn`. See only committed state — acceptable per TECH-DEBT 19.
       sv_emit_native_sql_rust(conn, parse_data->validated_form,
                               &native_sql, /* error_out */ ...);
       // probe dtor here → Connection::~Connection → ConnectionManager::RemoveConnection
   }
   // native_sql is "INSERT INTO semantic_layer._definitions ... RETURNING ..."
   // (or DELETE … RETURNING for DROP, or the two-statement DROP+INSERT for ALTER —
   //  the exact Phase 58/62 rewrite-to-native shape, unchanged).
   //
   // Build ParserExtensionPlanResult that drives native_sql through the binder
   // onto the CALLER's conn — transactional DDL preserved (D-20).
   return build_plan_result_from_native_sql(native_sql);
   ```

   **Critical: the per-call ConnGuard is used ONLY for catalog READS during DDL planning. The catalog WRITE (INSERT/DELETE on `semantic_layer._definitions`) is emitted as SQL and returned via `ParserExtensionPlanResult` for execution on the caller's conn through the binder — the Phase 58 rewrite-to-native pattern.** This is what preserves v0.8.0 transactional DDL semantics: the write participates in the caller's `BEGIN; ... COMMIT;` because it runs on the caller's conn, not on `probe`. The Option A1/A3 variants from the prior §16 (which would have run the INSERT inside a separate table function's bind, on a different conn) are **off the table** — they would regress transactional DDL.

4. **What `ParserExtensionPlanResult` carries:** today's Phase 58/62 implementation already builds a `ParserExtensionPlanResult` whose `TableFunction` invocation emits the rewritten SQL onto the caller's conn through the binder. B-prime preserves this exact mechanism — the only change is that the catalog *reads* now run on `probe` (per-call) instead of the long-lived `catalog_conn`. The mechanism for SQL emission to the caller's conn is the same; the long-lived conn is retired.

**Read path (14 table functions + 2 scalars):**

1. The 14 table functions (`list_semantic_views`, `list_terse_semantic_views`, `show_columns_in_semantic_view`, `describe_semantic_view`, `show_semantic_dimensions[_all]`, `show_semantic_dimensions_for_metric`, `show_semantic_metrics[_all]`, `show_semantic_facts[_all]`, `show_semantic_materializations[_all]`, `semantic_view`, `explain_semantic_view`) and the 2 scalars (`get_ddl`, `read_yaml_from_semantic_view`) are **re-registered through the C++ Catalog API directly** — bypassing duckdb-rs's `register_table_function_with_extra_info` / `register_scalar_function_with_state` (which marshal `ClientContext &` away via the C-API `duckdb_table_function_set_bind` mechanism).

2. **Registration template (validated by `65-READ-PATH-SPIKE.md`):**

   ```cpp
   // cpp/src/shim.cpp — new sv_register_table_function helper
   TableFunction tf(name, argument_types,
                    sv_<name>_function,        // execute callback
                    sv_<name>_bind,            // bind callback (the one we need)
                    sv_<name>_init);           // init callback
   CreateTableFunctionInfo info(tf);
   info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
   auto &system_catalog = Catalog::GetSystemCatalog(db);
   auto txn = CatalogTransaction::GetSystemTransaction(db);
   system_catalog.CreateTableFunction(txn, info);
   ```

   The `bind` callback now has the native `TableFunction` signature whose first argument is `ClientContext &context` — which is exactly what duckdb-rs's wrapper marshals away. Function bodies (`bind` / `init` / `execute`) stay in Rust behind a thin FFI shim; only the *registration plumbing* moves to C++. The FFI shape:

   ```cpp
   // C++ shim — one per table function
   static unique_ptr<FunctionData> sv_list_semantic_views_bind(
       ClientContext &context,
       TableFunctionBindInput &input,
       vector<LogicalType> &return_types,
       vector<string> &names) {
       // Empirically validated by 65-READ-PATH-SPIKE.md (3× rc=0)
       Connection probe(*context.db);
       duckdb_connection conn = reinterpret_cast<duckdb_connection>(&probe);

       // Bind input → Rust-side body. Rust populates return_types / names
       // and constructs the BindData; C++ wraps as unique_ptr<FunctionData>.
       void *rust_bind_data = nullptr;
       sv_list_semantic_views_bind_rust(conn, &input,
                                        &return_types, &names,
                                        &rust_bind_data, /* error_out */ ...);
       // probe dtor here — connection lives only for this bind callback
       return make_uniq<RustBindData>(rust_bind_data);
   }
   ```

   `extra_info` is no longer used to transport `CatalogReader` (since we derive Connection from `*context.db`). It could optionally carry static config (e.g., the `catalog_table_present` flag — though that can also be re-probed per-bind for ~0 cost). **The Rust-side bind/init/execute bodies are unchanged in spirit** — they still call into `CatalogReader::lookup` / `CatalogReader::list` etc. — but `CatalogReader` is now constructed per-call from `guard.raw()` instead of holding a Copy of a long-lived pointer.

3. **BindData transport across the FFI boundary:** the Rust bind body returns an opaque `*mut c_void` pointing at a Boxed Rust struct (matches today's `CatalogReader`/`BindData` ownership shape used in `src/query/`). The C++ shim wraps that pointer in a `RustBindData` subclass of `FunctionData` whose destructor calls back into Rust to drop the Box. The `init` and `execute` callbacks retrieve it via `data_p.bind_data->Cast<RustBindData>().rust_ptr` and pass it back to Rust. No new ownership semantics — same Box-across-FFI pattern as Phase 58 used for `SemanticViewsParserInfo::rust_state`.

4. **Per-call ConnGuard inside the bind:** the Rust bind body receives `conn: duckdb_connection` (the `&probe` pointer cast across FFI) as its first argument and uses it for the duration of the bind only. The C++ caller (`sv_<name>_bind`) destructs `probe` after the Rust call returns — so the bind body must not stash `conn` anywhere that outlives the call. This is the same lifetime contract as Plan 01's existing `ConnGuard` in `src/conn_guard.rs`, just wired through a different surface (C++ → Rust FFI instead of Rust-only).

---

### 16.3 Why the cached-`db_handle` pattern is the root defect (D-15 deep-dive)

The Option B spike's Probe 2 isolated something the prior research had not: the C-API `duckdb_connect(stashed_db_handle)` failure mode is **not** a generic "connections cannot be opened during parse/bind/plan" failure — it is specifically a **stale-pointer / wrong-DatabaseWrapper failure on the cached `duckdb_database` handle**.

Empirical evidence (verbatim from `65-OPTION-B-SPIKE.md`):

| Probe | Mechanism | Lifecycle phase | Result |
|-------|-----------|-----------------|--------|
| 1 | `Connection probe(*context.db)` (C++ direct on live ClientContext.db) | Plan thread (`sv_plan_function`) | **rc=0 — ctor + dtor succeed** |
| 2 | `duckdb_connect(stashed_db_handle, &conn)` (C-API on cached `OverrideContext.db_handle`) | Plan thread, **same callsite, microseconds apart** | **rc=1 — fails** |

Both paths ultimately call `Connection(DatabaseInstance &)` (the same ctor). The only difference is the path to the `DatabaseInstance`:

- **Probe 1:** `context.db` is `shared_ptr<DatabaseInstance>` carried by the live `ClientContext` driving the request. Always points at the right instance for this request.
- **Probe 2:** `stashed_db_handle` is the `duckdb_database` opaque pointer captured at `init_extension` time and stored in `OverrideContext`. `duckdb_connect`'s implementation at `cpp/include/duckdb.cpp:266432-266447` reinterprets it as `DatabaseWrapper *`, then dereferences `wrapper->database->instance`. **By plan-time, this dereferenced `DatabaseInstance` is either stale, dangling, or a different identity than `context.db`** — and the Connection ctor throws (most plausibly at `database.shared_from_this()` or `ConnectionManager::Get(database)`), translated to rc=1 by the C-API try/catch wrapper.

This means **the Phase 62 defect was not "leaking a long-lived `duckdb_connection`" alone — it was also "caching a `duckdb_database` pointer past the moment when it identifies the live `DatabaseInstance` the next request runs against."** The two failures are linked: the long-lived `duckdb_connection` keeps a wrong `DatabaseInstance` alive (busy-spin), and the cached `duckdb_database` no longer matches the live `DatabaseInstance` (rc=1). Both stem from the same architectural mistake: treating extension state as if it were `DatabaseInstance`-scoped when in fact it is `DBConfig`-scoped (DBConfig outlives the DatabaseInstance by construction; see §2.1).

B-prime fixes both at once: **never cache `db_handle`; always derive `DatabaseInstance &` from the live `ClientContext`.** The `OverrideContext` struct retains `catalog_table_present: bool` and `is_file_backed: bool` (stateless config flags, lifecycle-safe) but the `db_handle: duckdb_database` field added in Plan 02 partial (commits `0d2c0b7`, `f9caafe`) is reverted per D-17.

**TECH-DEBT 25 (filed per CONTEXT.md D-15, D-22):** the cached-`db_handle` defect is a finding distinct from the long-lived-connection leak. Both are resolved by Phase 65 B-prime, but they are independent root causes and the TECH-DEBT entry preserves the audit trail. Resolution status: *naturally resolved by Phase 65 B-prime architecture (no cached `db_handle` anywhere after Plan 02 partial revert)*.

---

### 16.4 How transactional DDL semantics are preserved (D-20 non-negotiable)

v0.8.0 shipped transactional DDL — `CREATE`/`DROP`/`ALTER SEMANTIC VIEW` participate in the caller's `BEGIN; ... COMMIT;`. Phase 58 engineered this specifically via the rewrite-to-native pattern: the parser hook emits SQL (`INSERT INTO semantic_layer._definitions ... RETURNING name AS view_name` for CREATE; two-statement `SELECT CASE WHEN NOT EXISTS THEN error() ELSE TRUE; DELETE … RETURNING` for DROP/ALTER) that is then driven through DuckDB's binder onto the **caller's connection** as if it were a normal statement. The catalog write therefore lands in the caller's transaction.

Under B-prime this property is preserved by being deliberate about which work runs on which conn:

| Work | Connection | Why |
|------|-----------|-----|
| Catalog READS (lookup, LIMIT 0 type probes, fact typing) during DDL planning | **Per-call `probe` from `Connection(*context.db)`** inside `sv_plan_function` | Lock-free w.r.t. the caller's transaction; sees only committed state (acceptable per TECH-DEBT 19); per-call lifetime means no `shared_ptr<DatabaseInstance>` leak |
| Catalog WRITE (the rewritten INSERT/DELETE on `semantic_layer._definitions`) | **Caller's connection, via the binder** (Phase 58 rewrite-to-native) | Preserves transactional DDL — the INSERT/DELETE participates in the user's BEGIN/COMMIT |
| Read-side table function catalog reads (`list_*` / `describe_*` / `show_*` / `get_ddl`) | **Per-call `probe`** inside each bind callback | Same reasoning — short-lived, lock-free, no leak |
| `semantic_view(...)` table function — expansion-time catalog reads (resolving dimension/metric definitions) | **Per-call `probe`** inside bind | Same. The actual aggregate query runs on the caller's conn via the normal table-function execute path (unchanged from today's mechanism, just without the long-lived `query_conn`). |
| `semantic_view(...)` — the EXPANDED SELECT that produces the aggregate result | **Caller's connection** (unchanged from today) | The expanded SQL is constructed at bind, returned by the table function as a streaming SELECT, executed via the binder on the caller's conn. B-prime does NOT change this — only the catalog reads that *build* the expansion shift onto `probe`. This is the same property that closes EXPAND-CTX-01..03 (the search-path divergence under ADBC stops happening because the user-visible query already runs on the caller's conn — only the *internal expansion construction* needed the catalog reads, and those now run on a conn derived from `context.db` instead of a separate long-lived conn). |

**The non-negotiable property** (D-20): `CREATE SEMANTIC VIEW` inside a user `BEGIN; ... ROLLBACK;` must NOT leave a row in `semantic_layer._definitions`. Phase 58's transactional DDL tests (`test/sql/v080_transactional_ddl.test`, `test_adbc_transactions.py`) verify this. Under B-prime these tests must stay **byte-identical green** — the planner's new Plan 02 explicitly verifies this property before the read-path work begins.

**Why this works:** at no point does Phase 65 B-prime move the catalog WRITE off the caller's conn. The per-call `probe` is used exclusively for READS during planning. The actual INSERT/DELETE is constructed as a SQL string and returned via `ParserExtensionPlanResult.function` for the binder to execute — exactly as Phase 58 designed.

**Why Option A2 was wrong:** Option A2 (the prior §16.2 recommendation) would have called `context.Query(native_sql)` directly inside `sv_plan_function`. This deadlocks on `ClientContext::context_lock` (the caller's parse holds the lock; `Query` tries to re-acquire it; `std::mutex` is non-recursive) — empirically pinned by `A2-DEADLOCK` lldb backtrace. B-prime sidesteps the deadlock by returning the rewritten SQL via `ParserExtensionPlanResult` instead of executing it inline.

---

### 16.5 Falsified Option A — short audit trail for LIFE-02 SC-2

LIFE-02 requires the trade-off record include the reasoning that led to the chosen mechanism. The Option A path is part of that trail; we keep this section short and link out.

**Option A as posed (prior §16, 2026-05-22):** promote `parse_function` to the success-path entry; promote `plan_function` to catalog-read + emission via `context.Query(native_sql)` on the caller's `ClientContext` — preserving transactional DDL by executing on the caller's conn directly.

**Wave-0 spike `A2` (`65-02-SPIKES.md`):** `context.Query("SELECT 42 AS spike", false)` from inside `sv_plan_function` deadlocked. lldb backtrace (verbatim in `65-02-SPIKES.md` lines 56-111) pins the deadlock at `std::mutex::lock` on `ClientContext::context_lock`, acquired by a fresh `ClientContextLock` constructed inside `ClientContext::LockContext()` (`duckdb.cpp:272659`), called from `ClientContext::Query` (`duckdb.cpp:273504`), called from our `sv_plan_unreachable` (the repurposed spike) at frame #11. The caller already held `context_lock` for the entire duration of `plan_function`. Result conclusion line: **`A2-DEADLOCK`**.

**Wave-0 spike `A6-bind` (`65-02-SPIKES.md`):** `duckdb_connect(stashed_db_handle)` from inside `ListSemanticViewsVTab::bind` (the read-side equivalent) returned rc=1 across three consecutive bind invocations. Result conclusion line: **`BIND-THREAD-RC1`** — generalises the D-10 parse-thread failure to the bind thread for the C-API wrapper path.

**Combined consequence:** Option A's two pillars (A2 for write path, C-API `duckdb_connect` for read path) were both falsified by the spike round. The recommended path forward at that point was *escalate* (per the executor's `USER_HARD_CONSTRAINT` block): A1 and A3 regress transactional DDL (D-20 forbids); only escalation was a live option.

**The B-prime spike round** (Option B + read-path) was the resolution of that escalation: it tested the *complement* of the falsified A path — the C++ direct `Connection(*context.db)` mechanism, which had not been probed by spike A2 or A6-bind. Both spikes returned rc=0 / no deadlock, validating B-prime end-to-end.

**Full trail:** `65-02-SPIKES.md` (A2-DEADLOCK + BIND-THREAD-RC1) → CONTEXT-PRE-BPRIME D-10 → escalation → `65-OPTION-B-SPIKE.md` (PLAN-THREAD-RC0 for C++ direct path) → `65-READ-PATH-SPIKE.md` (READ-BIND-RC0) → CONTEXT D-14..D-22 (B-prime locked).

This audit trail is preserved here for LIFE-02 SC-2 (trade-off record), per CONTEXT D-22 (bounded scope with signal surfacing).

---

### 16.6 C++ Catalog API registration shape — the template

The empirically-validated registration shape from `65-READ-PATH-SPIKE.md` (which used `Catalog::GetSystemCatalog(db).CreateTableFunction` for a scratch `__sv_read_path_spike()` function). The same shape generalises to all 14 + 2 read-side functions.

**Registration template (added to `cpp/src/shim.cpp`):**

```cpp
extern "C" {
    // Called from src/lib.rs::init_extension once per function.
    // Replaces the duckdb-rs register_table_function_with_extra_info calls
    // at src/lib.rs:425-495.
    bool sv_register_table_function(
            duckdb_database db_handle,
            const char *name,
            const sv_table_function_signature_t *sig,  // arg types + return columns
            sv_bind_fn_t rust_bind,                    // Rust bind body
            sv_init_fn_t rust_init,                    // Rust init body
            sv_execute_fn_t rust_execute) {            // Rust execute body
        try {
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            auto &db = *wrapper->database->instance;

            // Build a C++ TableFunction whose bind/init/execute trampolines
            // call the Rust function pointers carried through extra_info.
            auto extra_info = make_shared_ptr<SvRustExtraInfo>();
            extra_info->rust_bind = rust_bind;
            extra_info->rust_init = rust_init;
            extra_info->rust_execute = rust_execute;
            extra_info->name = name;

            TableFunction tf(name, sig_to_argument_types(sig),
                             sv_trampoline_execute,
                             sv_trampoline_bind,
                             sv_trampoline_init);
            tf.extra_info = extra_info;  // C++ shared_ptr — survives for the
                                         // function's registered lifetime
            CreateTableFunctionInfo info(tf);
            info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
            auto &system_catalog = Catalog::GetSystemCatalog(db);
            auto txn = CatalogTransaction::GetSystemTransaction(db);
            system_catalog.CreateTableFunction(txn, info);
            return true;
        } catch (const std::exception &e) {
            fprintf(stderr, "sv_register_table_function(%s) failed: %s\n",
                    name, e.what());
            return false;
        }
    }
}
```

**Trampoline shape (one set, shared by all functions):**

```cpp
static unique_ptr<FunctionData> sv_trampoline_bind(
        ClientContext &context,
        TableFunctionBindInput &input,
        vector<LogicalType> &return_types,
        vector<string> &names) {
    auto &extra = input.info->Cast<SvRustExtraInfo>();
    // Empirically validated: Connection(*context.db) succeeds at bind time
    Connection probe(*context.db);
    auto conn = reinterpret_cast<duckdb_connection>(&probe);
    void *rust_bind_data = nullptr;
    char *error_msg = nullptr;
    bool ok = extra.rust_bind(conn, &input, &return_types, &names,
                              &rust_bind_data, &error_msg);
    // probe destructs here — RAII teardown of the per-call connection
    if (!ok) {
        string msg = error_msg ? string(error_msg) : "bind failed";
        if (error_msg) sv_free_cstring(error_msg);  // Rust-allocated
        throw BinderException(msg);
    }
    return make_uniq<SvRustBindData>(rust_bind_data, extra.name);
}
```

**Rust-side FFI signature template:**

```rust
// src/ffi/read_path.rs (new module — one helper per read function)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sv_list_semantic_views_bind_rust(
    conn: ffi::duckdb_connection,
    input: *mut c_void,           // opaque TableFunctionBindInput*
    return_types_out: *mut c_void,
    names_out: *mut c_void,
    bind_data_out: *mut *mut c_void,
    error_msg_out: *mut *mut c_char,
) -> bool {
    // Build a CatalogReader from the per-call conn (NOT a long-lived copy)
    // Catalog table presence: probe via a cheap SELECT or cache in extra_info
    let catalog_table_present = probe_catalog_present(conn);
    let reader = CatalogReader::new(conn, catalog_table_present);
    // Existing bind body — same logic as today, just with per-call conn
    match list_semantic_views_bind(&reader, input, return_types_out, names_out) {
        Ok(bind_data) => {
            *bind_data_out = Box::into_raw(Box::new(bind_data)) as *mut c_void;
            true
        }
        Err(msg) => {
            *error_msg_out = CString::new(msg).unwrap().into_raw();
            false
        }
    }
}
```

**Why `extra_info` carries Rust function pointers** (not the `CatalogReader` it carries today): `CatalogReader` today is a `Copy` of the long-lived `catalog_conn` — exactly the H1 leak. Under B-prime there is no long-lived conn to copy; the per-call conn comes from `*context.db` at bind time. `extra_info` instead carries the *callbacks* — Rust function pointers that the C++ trampoline invokes with the per-call `conn`. This is the standard pattern for Rust-backed C++ table functions; it matches how Phase 58's parser_override callbacks are wired through `SemanticViewsParserInfo::rust_state`.

**Why `parser_override` deregistration is safe (path (a) per prior §16.6 #1):** confirmed empirically by the Option B spike, which exercised the `sv_parse_function` success path without `sv_parser_override` being involved in dispatch. The default parser's failure on the unrecognised prefix is what triggers `parse_function` (`duckdb.cpp:347281-347300`); `parser_override` is not on the critical path. Deregistering it removes one callback to reason about and renders TECH-DEBT 21 moot for this extension. The caret-rendering tests (`test/integration/test_caret_position.py`, Phase 62 Plan 03) still pass because `sv_parse_function` returns `DISPLAY_EXTENSION_ERROR` with `error_location` on structural failure — the same caret-emission path used today. The new Plan 02 explicitly verifies this property before deregistration commits.

**Implementation cost estimate:** ~150 LOC of new C++ shim (one trampoline set + the registration helper + the `SvRustExtraInfo` / `SvRustBindData` types) + ~100 LOC of new Rust FFI surface (one bind/init/execute trampoline per function — 14 + 2 = 16 small functions, mostly boilerplate). Comparable to Phase 58's parser_override C++ wiring. The function bodies themselves stay in Rust — only the registration plumbing and FFI shims are new.

---

### 16.7 Planner inputs (refreshed under B-prime)

Concrete decisions the planner needs to make for the new Plans 02-06:

1. **`sv_parser_override` disposition:** **deregister entirely** (path (a)). Empirically confirmed safe by Option B spike. TECH-DEBT 21 becomes moot for this extension.

2. **`plan_function` execution mechanism:** **B-prime / Option B-prime** — per-call `Connection(*context.db)` for catalog reads inside `sv_plan_function`; emit rewritten SQL via `ParserExtensionPlanResult` for binder execution on the caller's conn (Phase 58 rewrite-to-native unchanged). Empirically validated by `65-OPTION-B-SPIKE.md` Probe 1. **Option A1/A2/A3 from the prior §16 are off the table** (A1/A3 regress transactional DDL — D-20 forbids; A2 deadlocks — A2-DEADLOCK).

3. **`SemanticViewParseData` carrier shape:** preserve the existing `string query` field for caret rendering; add a `vector<uint8_t> payload` opaque to C++ for the Rust-side stash (verb / validated_form / view_name / flags / byte_offset). Manual LE encoding inside Rust (~30 LOC, zero new deps). The prior §16.4 recommendation stands — only the execution mechanism changed, not the carrier.

4. **Read-path registration shape:** **C++ Catalog API directly** (`Catalog::GetSystemCatalog(db).CreateTableFunction` per `65-READ-PATH-SPIKE.md`). Replaces all 14 + 2 calls to `con.register_table_function_with_extra_info` / `con.register_scalar_function_with_state` at `src/lib.rs:425-495`. The new `sv_register_table_function` shim lives in `cpp/src/shim.cpp`. Function bodies stay in Rust; only registration plumbing moves.

5. **Plan 02 partial commits (`0d2c0b7`, `f9caafe`, `656bae7`):** **REVERT** as part of new Plan 02 (per D-17). The `db_handle: duckdb_database` field on `OverrideContext` is dead code under B-prime (we use `*context.db` directly, never a cached `db_handle`). The `sv_register_parser_hooks(duckdb_database, bool, bool)` signature change is also dead code (B-prime deregisters `sv_parser_override` rather than passing more flags into it).

6. **`OverrideContext` final shape under B-prime:** retain `catalog_table_present: bool` and `is_file_backed: bool` (static config flags, lifecycle-safe). Remove the `db_handle: duckdb_database` field added in Plan 02 partial. *Open question:* whether `OverrideContext` is needed at all once `sv_parser_override` is deregistered — if the only callbacks reading it are `sv_parse_function` (for `is_file_backed` gating of LIMIT 0 probes) and `sv_plan_function` (for `catalog_table_present` short-circuit), it survives but as a stateless config holder. If neither callback reads it after the refactor, `OverrideContext` can be retired entirely. Planner to decide based on the actual use sites after Plan 03 lands.

7. **Plan structure (planner's discretion per CONTEXT D-22):** sketched in CONTEXT.md as 5-6 plans:
   - Plan 02 (NEW): revert Plan 02 partial; add `sv_register_table_function` shim + trampolines; no production refactor yet.
   - Plan 03: port write path — deregister `sv_parser_override`; promote `sv_parse_function` + `sv_plan_function`; preserve transactional DDL.
   - Plan 04: port read path first half (7 of 14 — `list_*` / `show_*`).
   - Plan 05: port read path second half (`describe_*` / `get_ddl` / `read_yaml_from_semantic_view` / `semantic_view` / `explain_semantic_view`).
   - Plan 06: retire `init_extension`'s `catalog_conn` and `query_conn` opens at `src/lib.rs:387, 499`; structural grep guard (zero `duckdb_connect` in `init_extension` body); LIFE-04 update; file TECH-DEBT 25; B1..B11 + new read-side watchdog tests flip green.
   - Optionally Plan 07: close-out + cleanup of dead code (OverrideContext field reverts, sv_register_parser_hooks signature) + SUMMARY.

8. **Test scaffolding for the read path:** extend B1..B14 with read-side variants — for each of the 14 + 2 functions, a watchdog test that does `LOAD → CREATE → SELECT FROM <function>() → close → connect(read_only=True) → LOAD → SELECT FROM <function>()` in-process within 5s. Empirical mirror of B1-B4 but covering the read-side functions. Captures any Database-lifetime leak the read-path port might introduce.

9. **`extra_info` lifetime under C++ registration:** the `SvRustExtraInfo` shared_ptr carried in `tf.extra_info` survives for the table function's catalog-registered lifetime (i.e., until DBConfig destructs). It holds Rust function pointers + a static `name` C-string — no `duckdb_connection`, no `duckdb_database`. Lifecycle-safe by construction; no new long-lived native handles introduced.

10. **D-21 (no time pressure):** the planner should NOT compress the read-path port for schedule reasons. Plans 04 and 05 should each ship independently green (B1..B14 + read-side variants) before Plan 06 retires the long-lived opens. Premature retirement before all 14 + 2 ports are green would break any read-side function whose port slipped to a later plan.

---

*End of §16. §§1–15 remain authoritative as background context. The chosen mechanism (B-prime) supersedes §1's executive-summary recommendation and §6.1's "per-DDL connect from `sv_parser_override`" specifically. The Long-lived Handles Audit in §5 is extended to all native handles (not just connections) under §17 below.*

---

## §17 — Long-lived native handles audit (post-B-prime)

**Researched:** 2026-05-23 (per CONTEXT D-22 bounded-scope-with-signal-surfacing directive)
**Scope:** Inventory every native handle held by the extension beyond a single callback's scope under B-prime. For each, determine: under B-prime, is it eliminated, still live, or transformed? File TECH-DEBT entry candidates for anything that survives that shouldn't.

This generalises §5's audit (which focused on `duckdb_connection` only) to all native handles — `duckdb_database`, prepared statements, parser-info pointers, thread-local pointers, C++ static state, anything else with a lifetime beyond a single callback's stack frame.

---

### 17.1 Handle inventory under B-prime

| # | Handle | Type | Current site (v0.9.0 / Plan 02 partial baseline) | Under B-prime | Disposition |
|---|--------|------|---------------------------------------------------|---------------|-------------|
| **H1** | `catalog_conn` | `duckdb_connection` | `src/lib.rs:386-390` (opened in `init_extension`); shared into `CatalogReader` Copy-pointers stored on 15+ `register_table_function_with_extra_info` extra_info slots at `src/lib.rs:425-495` | **ELIMINATED** | Plan 06 retires this open entirely. Read-side callbacks derive Connection per-call from `*context.db` via the C++ Catalog API trampoline; no long-lived conn anywhere. |
| **H2** | `query_conn` | `duckdb_connection` | `src/lib.rs:498-502` (opened in `init_extension`); stored on `QueryState::conn` extra_info for `semantic_view` + `explain_semantic_view` table functions | **ELIMINATED** | Plan 06 retires this open. The `semantic_view` bind callback derives `Connection(*context.db)` per-call (read-path spike pattern). Execution of the expanded SELECT still rides the caller's conn through the binder — unchanged. |
| **H3** | `OverrideContext` (Rust `Box`) carrying `{db_handle, catalog_table_present, is_file_backed}` after Plan 02 partial | Box across FFI, owned by `SemanticViewsParserInfo::rust_state` | `src/parse.rs:67-72` (struct); allocated by `sv_make_override_context`; freed by `sv_drop_override_context` at `~SemanticViewsParserInfo` (`cpp/src/shim.cpp:159-184`) | **TRANSFORMED — `db_handle` field reverted** | Per D-17, `db_handle: duckdb_database` is reverted (dead code under B-prime since we use `*context.db` directly, never a cached handle). `catalog_table_present` and `is_file_backed` survive as stateless config flags. The Box itself is still owned by `SemanticViewsParserInfo` and freed correctly at `~DBConfig` — same as today. **Confirms CONTEXT D-17.** |
| **H3-alt** | `OverrideContext` if retired entirely | — | Possible after Plan 03 lands | **POTENTIALLY ELIMINATED — planner decides** | If the only callbacks reading `OverrideContext` are `sv_parse_function` (which doesn't get `ClientContext &` — could read `is_file_backed` from a static helper instead) and `sv_plan_function` (which has `*context.db` directly — `is_file_backed` can be re-derived from `DatabaseInstance.config.options.access_mode` if needed), `OverrideContext` may be retired. Audit at end of Plan 06. |
| **H4** | `ParserExtension` C++ struct in `DBConfig` | C++ value, owned by `DBConfig::extensions::parser_extensions` | `cpp/src/shim.cpp:372-393` (registered in `sv_register_parser_hooks`) | **STILL LIVE — but reduced** | `ext.parser_override = sv_parser_override` is **removed under B-prime** (D-22 path (a)). `ext.parse_function = sv_parse_function` and `ext.plan_function = sv_plan_function` remain (now the success path). The ParserExtension struct itself still survives for DBConfig's lifetime; no native resources owned directly. No leak. No action. |
| **H5** | `SemanticViewsParserInfo` (C++ `shared_ptr<ParserExtensionInfo>`) | C++ shared_ptr, held inside `ext.parser_info` | `cpp/src/shim.cpp:389-391` | **STILL LIVE — unchanged** | Destroyed at `~DBConfig` along with H4. It's the Box-owner from H3; the Box's contents change under B-prime but the wrapper does not. No action. |
| **H6** | `extension` C-API state buffer (`have_api_struct`, error strings) | Stack-allocated; emitted into DuckDB via `set_error` callback | `src/lib.rs:539-560` | **STILL LIVE — unchanged** | Scoped to extension init; nothing leaks. No action. |
| **H7** | `QueryState::catalog` (`CatalogReader` value carrying H1's pointer) | Rust value, Copy-stored in extra_info | `src/lib.rs:505-512` | **ELIMINATED** | Plan 04/05 retires `QueryState` along with H2. `CatalogReader` is constructed per-call inside each bind from `guard.raw()` instead of being stored long-lived. Mechanical consequence of H1+H2 retirement, not a separate finding. |
| **H8** | C++ amalgamation globals (allocators, background threads, static init) | Process-static linkage | `build.rs` / `cc` | **STILL LIVE — orthogonal** | Per-process; not extension-owned in any meaningful sense. No action. |
| **H9** | `StreamingState` in `semantic_view` table function (`Mutex<Option<StreamingState>>`) | Rust value, query-scoped | `src/query/table_function.rs:92, 746` | **STILL LIVE — but scope unchanged** | Per-query lifetime (lives inside `SemanticViewVTab` bind data, cleared between executions). Today and under B-prime: query-scoped. No leak. No action. |
| **H10** | Any global `static`, `OnceLock`, `lazy_static`, `RwLock`, `Mutex` at module-level | Process-static | grep across `src/` | **NONE FOUND — clean** | Phase 62 removed the only one (the `db_token` LRU). Plan 02 partial's `A6_BIND_SPIKE_DB_HANDLE: OnceLock<usize>` was a SPIKE-ONLY scratch and is already reverted to disk-empty. **Plan 06 should add a structural grep guard** that fails CI if any new module-level `OnceLock<usize>` / `OnceLock<duckdb_database>` / similar appears in `src/`. |
| **H11** | `cc`-built C++ allocator / `Allocator::SetBackgroundThreads` | Process-scoped, runs at `~DatabaseInstance` (`duckdb.cpp:276834`) | DuckDB amalgamation | **STILL LIVE — orthogonal** | Not extension-owned. No action. |
| **H12 (NEW)** | `SvRustExtraInfo` shared_ptr (C++) carried in each `TableFunction::extra_info` slot after C++ Catalog API registration | `shared_ptr<TableFunctionInfo>` (or subclass), held by the catalog entry for the function's lifetime | `cpp/src/shim.cpp` after Plan 02-05 land (16 instances — one per registered function) | **STILL LIVE — by design** | Holds Rust function pointers (sv_<name>_bind_rust, etc.) and the static `name` C-string. **NO `duckdb_connection`, NO `duckdb_database`, NO long-lived conn.** Lifecycle-safe by construction (function pointers are static; name string is static). Destroyed at `~DBConfig` along with the catalog. No native resources to leak. Verify with a structural test: `nm cpp/src/shim.cpp | grep SvRustExtraInfo` shows no member of type `duckdb_connection`. |
| **H13 (NEW)** | `SvRustBindData` Box across FFI (per-bind, per-table-function) | Rust Box pointed at by `unique_ptr<FunctionData>` in the C++ bind result | New under B-prime | **STILL LIVE — per-query scope** | Lives from bind through the end of query execution; destroyed by the C++ bind data destructor which calls back into Rust to drop the Box. Same Box-across-FFI pattern as Phase 58 used for `SemanticViewsParserInfo::rust_state`. No long-lived state; lifecycle-safe. |
| **H14** | `parse_function` / `plan_function` Rust function pointers (registered in the `ParserExtension` struct at extension load) | C function pointers, held in `DBConfig::extensions::parser_extensions` | `cpp/src/shim.cpp:383-384` | **STILL LIVE — unchanged shape** | Same shape as today (function pointers, not heap objects). Lifecycle-safe. |
| **H15** | C-API `duckdb_prepared_statement` instances | Per-statement | `src/catalog.rs` (via `prepared_lookup`); per-query lifecycle inside `CatalogReader::lookup` and friends | **STILL LIVE — per-call scope** | Today: prepared inside each `CatalogReader` method call against H1's conn; finalized at end of call. Under B-prime: prepared inside each method call against the per-call `probe` conn; finalized before guard drops. **No long-lived prepared statements cached anywhere.** Verify with grep: no `static` or `OnceLock` holding `duckdb_prepared_statement` in `src/`. |
| **H16** | Thread-local pointers, `thread_local!` statics | — | grep across `src/` | **NONE FOUND** | No `thread_local!` declarations in `src/`. No action. |
| **H17** | `init_extension` one-time probes (the `current_setting('access_mode')` query, `init_catalog`, the `catalog_table_present` probe) | Per-load lifecycle | `src/lib.rs:370-406` | **STILL LIVE — one-shot at load** | These run once at LOAD time on the user's `con` (passed in by the duckdb-rs binding). They do NOT open any new long-lived `duckdb_connection` — they use the existing `con` for the duration of `init_extension` only. Lifecycle-safe. **Plan 06 should preserve these one-shot probes** but verify (via grep audit) that none of them stash any pointer into a module-level static. |
| **H18** | `parser_info` registration state inside `cpp/src/shim.cpp` (the static-storage `SemanticViewsParserInfo` chain) | C++ shared_ptr held by `ParserExtension::parser_info` field on `DBConfig` | `cpp/src/shim.cpp:389-391` | **STILL LIVE — but degraded** | Under B-prime, `SemanticViewsParserInfo::rust_state` still carries the `OverrideContext` Box. Lifecycle unchanged: shared_ptr destroyed at `~DBConfig`; Box freed by `sv_drop_override_context`. The Box's contents are smaller now (no `db_handle` field). No leak. |

---

### 17.2 What survives that shouldn't?

**Audit conclusion: nothing.** Every handle in the inventory above either (a) is eliminated under B-prime (H1, H2, H7), (b) is per-call / per-query scoped by construction (H9, H13, H15), (c) is owned by `DBConfig` and lifecycle-safe by virtue of holding zero native resources (H4, H5, H12, H14, H18), or (d) is orthogonal (H8, H11).

**No new TECH-DEBT candidate filed from §17.** The cached-`db_handle` defect (`OverrideContext.db_handle` field) is reverted as part of Plan 02 (per D-17) and surfaces as TECH-DEBT 25 separately (per D-15 / D-22) — but the resolution is "naturally resolved by Phase 65 B-prime," not a new debt.

**No follow-up phase proposed.** The audit covered every category requested by CONTEXT D-22: stored data (H1, H2, H7), live service config (H4, H5, H18), OS-registered state (H8, H11 — orthogonal), secrets/env vars (none — extension has no secrets), build artifacts (H8 — orthogonal). All categories either resolved or out of scope by construction.

---

### 17.3 Explicit confirmation: retiring H1 + H2 + reverting Plan 02 partial eliminates the LAST extension-owned long-lived connection

This is the LIFE-04 close-out claim. The audit confirms:

- **H1 retired** (Plan 06): zero `duckdb_connect` calls remain in `init_extension`'s body for `catalog_conn`.
- **H2 retired** (Plan 06): zero `duckdb_connect` calls remain in `init_extension`'s body for `query_conn`.
- **H7 retired** (Plan 04/05): `QueryState::conn` no longer exists.
- **H3 db_handle field reverted** (Plan 02): no cached `duckdb_database` survives anywhere.
- **H10 + H16 clean** (verified by grep): no `OnceLock<usize>` or `thread_local!` holds a connection or db handle.
- **H15 per-call** (verified by grep audit in Plan 06): no `static`/`OnceLock` caches a `duckdb_prepared_statement`.

**Structural grep guard for Plan 06** (per CONTEXT.md decision sketch):

```bash
# Must return ZERO matches inside init_extension body:
rg -n 'duckdb_connect\(' src/lib.rs | rg -v '^src/lib.rs:[0-9]+:\s*//'
# Must return ZERO matches anywhere in src/:
rg -n 'static\s+\w+:\s*OnceLock<(usize|.*duckdb_database|.*duckdb_connection)>' src/
rg -n 'thread_local!' src/
```

After Plan 06 lands, all three greps return empty. This is the operational definition of "zero long-lived extension-owned native handles" for LIFE-04.

---

### 17.4 Potential surprise sites — explicit check-list for the planner

Per CONTEXT D-22 ("any other long-lived-native-handle findings discovered during implementation get filed as TECH-DEBT entries or follow-up phase proposals — not silently absorbed"), the following sites were explicitly checked during this audit and confirmed clean:

| Site | What was checked | Result |
|------|------------------|--------|
| `cpp/src/shim.cpp` parser_info registration state | Does the C++ side hold any `duckdb_connection` / `duckdb_database` member on `SemanticViewsParserInfo`? | **No.** Only `void *rust_state` (the Box pointer). Confirmed by reading `cpp/src/shim.cpp:159-184`. |
| `init_extension` one-time probes | Do `current_setting('access_mode')` / `init_catalog` / `catalog_table_present` probe stash any pointer? | **No.** All probes run on the caller's `con` argument and complete synchronously; no module-level statics involved. Confirmed by reading `src/lib.rs:340-410`. |
| `src/catalog.rs::CatalogReader` | Does any method cache a prepared statement beyond a single call? | **No.** `prepared_lookup` and similar prepare-and-finalize within one call. Confirmed by reading `src/catalog.rs:80-150` (and the broader pattern matches what was reviewed in §5 for v0.9.0). |
| `src/query/table_function.rs` `SemanticViewVTab` | Does any field beyond `StreamingState` survive past a query? | **No.** Bind data lives per query; `StreamingState` is Mutex-protected and cleared between executions. H9 row above. |
| Any `OnceLock<duckdb_database>` introduced by spike scaffolding | Was every spike-only OnceLock reverted? | **Yes.** `A6_BIND_SPIKE_DB_HANDLE` and the Option B probe scratch FFI accessor are both confirmed reverted (per `65-02-SPIKES.md` and `65-OPTION-B-SPIKE.md` "spike artefacts reverted" sections). |
| `cpp/src/shim.cpp` parser_override callback | Does deregistration leave any dangling pointer in DBConfig? | **No.** `ext.parser_override = nullptr` is the documented way to opt out; DuckDB checks the pointer before invoking. No stale state. Per `duckdb.cpp:347253-347300` parser dispatch chain. |
| `extra_info` slots on `register_table_function_with_extra_info` | Do any of the 14 + 2 sites stash a Rust value containing a `duckdb_connection`? | **All 14 + 2 retired under B-prime** (replaced by C++ Catalog API registration). No `&catalog_reader` / `&query_state` extra_info pointers survive Plan 06. |

If, during implementation, the executor discovers a handle not listed above that survives past a callback's scope, **the executor MUST surface it as a finding** (TECH-DEBT entry or follow-up phase proposal) rather than silently absorbing it. Per CONTEXT D-22.

---

*End of §17. This audit closes the LIFE-04 success criterion: every category of long-lived native handle has been inventoried, classified, and assigned a Phase 65 disposition.*

---

## RESEARCH COMPLETE

**Phase:** 65 — OverrideContext Connection Teardown
**Confidence:** HIGH (B-prime architecture empirically validated end-to-end via two independent spikes; long-lived-handles audit complete)

### What changed in this refresh

- **§16 replaced** — "Bind/Plan-Time Architecture (Option A)" (2026-05-22, falsified) replaced with "B-prime Architecture" (2026-05-23, empirically validated). The chosen mechanism is per-call C++ `Connection(*context.db)` from every callback that receives `ClientContext &`, with write-path catalog reads done in `sv_plan_function` and read-path catalog reads done in each read-side bind callback registered via C++ Catalog API. Transactional DDL preserved via Phase 58's rewrite-to-native pattern (the per-call ConnGuard is for catalog READS only; the catalog WRITE rides the binder onto the caller's conn).
- **§17 added** — "Long-lived native handles audit (post-B-prime)" per CONTEXT D-22. Generalises §5's `duckdb_connection`-only audit to ALL native handles. Audit conclusion: nothing survives that shouldn't. Confirms retiring H1 + H2 + reverting Plan 02 partial's `db_handle` field eliminates the last extension-owned long-lived connection (LIFE-04 closes).
- **§§1–15 preserved verbatim** — busy-spin root cause, lifecycle surface, canonical pattern survey, original handle audit, validation architecture, trade-off documentation, surfaced findings, project constraints, sources, assumptions log, open questions, phase requirements, environment availability all unchanged.

### Key findings (B-prime)

- **Per-call C++ `Connection(*context.db)` succeeds at every lifecycle phase where `ClientContext &` is reachable** (plan thread: `PLAN-THREAD-RC0` from `65-OPTION-B-SPIKE.md` Probe 1; bind thread: `READ-BIND-RC0` from `65-READ-PATH-SPIKE.md` across three consecutive bind invocations).
- **The C-API `duckdb_connect(stashed_db_handle)` path is dead at every lifecycle phase tested** (parse: D-10; bind: BIND-THREAD-RC1; plan: PLAN-THREAD-RC1). The root defect is the cached `duckdb_database` — by the time queries run, the stashed `db_handle` no longer identifies the live `DatabaseInstance` (`65-OPTION-B-SPIKE.md` Probe 2 vs. Probe 1 split). Filed as TECH-DEBT 25 (per D-15, D-22); naturally resolved by B-prime.
- **`sv_parser_override` deregistered entirely** (path (a) per D-22) — empirically safe; renders TECH-DEBT 21 moot for this extension; caret rendering preserved via `sv_parse_function`'s `DISPLAY_EXTENSION_ERROR` + `error_location` path.
- **Read path C++ Catalog API registration shape validated** — `Catalog::GetSystemCatalog(db).CreateTableFunction(CreateTableFunctionInfo{tf})` with `tf.extra_info` carrying Rust function pointers; bind callbacks have the native `ClientContext &` argument that duckdb-rs's wrapper marshals away. 14 + 2 read-side functions move to this pattern.
- **Transactional DDL preserved** — the per-call ConnGuard is used ONLY for catalog READS during planning; the catalog WRITE rides the Phase 58 rewrite-to-native pattern (INSERT/DELETE SQL emitted via `ParserExtensionPlanResult`, executed by the binder on the caller's conn). v0.8.0 transactional DDL tests stay byte-identical green (D-20 non-negotiable).
- **Long-lived-handles audit (§17) conclusion: nothing survives that shouldn't** — H1, H2, H7 eliminated; H3's `db_handle` field reverted; H10/H15/H16 clean (verified by grep). Plan 06 adds structural grep guards to keep it that way.

### Files updated
`.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md` — §16 replaced (B-prime supersedes Option A); §17 added (full native-handle audit). §§1–15 preserved verbatim.

### Confidence assessment

| Area | Level | Reason |
|------|-------|--------|
| B-prime mechanism (plan + bind C++ direct) | HIGH | Empirically validated end-to-end by two independent spikes on the actual `--features extension` build, against the bundled DuckDB v1.5.2 (`65-OPTION-B-SPIKE.md` + `65-READ-PATH-SPIKE.md`) |
| Cached-`db_handle` as root defect (D-15) | HIGH | Isolated by Option B Probe 2 vs. Probe 1 split (same thread, same `DatabaseInstance`, microseconds apart, opposite results) |
| Transactional DDL preservation under B-prime | HIGH | Mechanism unchanged from Phase 58 — catalog WRITE still rides the binder onto the caller's conn via `ParserExtensionPlanResult`; only the catalog READS shift to per-call `probe` |
| C++ Catalog API registration shape | HIGH | Read-path spike used this exact pattern with three live bind invocations |
| `sv_parser_override` deregistration safety | HIGH | Confirmed by Option B spike's success against the parse-function dispatch chain (default parser fails on unrecognised prefix → `parse_function` fires) |
| `OverrideContext` post-refactor shape (whether `db_handle` field is fully reverted, whether the struct survives at all) | MEDIUM | D-17 reverts the `db_handle` field; the struct's continued necessity depends on actual use sites after Plan 03 lands |
| Long-lived-handles audit completeness | HIGH | Every category from CONTEXT D-22 enumerated; explicit "potential surprise sites" check-list in §17.4 |
| `:memory:` smoke test under B-prime | MEDIUM | `:memory:` not cached in DBInstanceCache; per-call `Connection(*context.db)` should work uniformly. Plan 06 to add an explicit smoke test (per §9.3) |

### Ready for planning

Planner can now create fresh Plans 02-06 (Plan 01 stays intact per D-18). Suggested wave structure remains as sketched in CONTEXT.md §decisions / §plan structure:
- Plan 02 (revert + C++ shim infrastructure)
- Plan 03 (write path port)
- Plan 04 (read path port — first half)
- Plan 05 (read path port — second half + scalars)
- Plan 06 (retire H1 + H2 + structural grep guards + LIFE-04 + TECH-DEBT 25 + read-side watchdog tests + `:memory:` smoke)
- Optionally Plan 07 (close-out + cleanup of dead code)
