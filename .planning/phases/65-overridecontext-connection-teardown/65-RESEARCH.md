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

## §16 — Bind/Plan-Time Architecture (Option A — added 2026-05-22)

**Researched:** 2026-05-22 (post Plan 02 falsification + D-11 lock-in)
**Scope:** Targeted refresh answering the three sub-questions opened by D-11 — *not* a full re-research. §§1–15 above remain authoritative except where noted in §16.1.
**DuckDB version anchored to:** v1.5.2 (`DUCKDB_VERSION` in `cpp/include/duckdb.hpp`; extension API `1.10502.0` per `Cargo.toml`)

---

### 16.1 Status of prior assumptions

| Section | Status | Notes |
|---------|--------|-------|
| §1 Executive Summary "viable shape is D-07-1" | **PARTIALLY FALSIFIED** | The *direction* (don't cache; eliminate the long-lived handle) is correct and locked by D-11. The *location* ("inside `sv_parser_override_rust`") is the falsified part — Plan 02's empirical test proved `duckdb_connect` from inside `parser_override` returns rc=1 (43/47 sqllogictests). The new location is bind/plan time. |
| §2 Busy-spin diagnosis | **STILL VALID** | Plan 01 Spike A4 confirmed the busy-spin (99.4 % CPU, `ldr/cmn/b.ne` loop, `___lldb_unnamed_symbol####` depth matches `DBInstanceCache::GetInstanceInternal`). Root cause and fix target unchanged. |
| §3.1 Lifecycle surface table | **STILL VALID** | All hooks enumerated still exist; `parse_function`/`plan_function` row gains new prominence (see §16.2). |
| §3.2 "What we cannot do" | **STILL VALID** | No DB-shutdown notification, no weak handle, no `ClientContext` from `parser_override`. |
| **§3.3 / §6.5 standalone-library argument that `connections_lock` is per-`ConnectionManager` and does not gate parse-thread re-entry** | **FALSIFIED (per D-10)** | Empirically: opening a fresh `duckdb_connection` from inside `Parser::ParseQuery` on the bundled DuckDB 1.5.2 returns rc=1. The structural argument (`connections_lock` scope) may still be technically correct, but *some other* gating prevents the second connect — likely `DBInstanceCache::GetOrCreateInstance` interaction with an in-flight parse, or the `PostgresParser` non-reentrancy noted at `duckdb.cpp:347253` (*"Creating a new scope to allow extensions to use PostgresParser, which is not reentrant"*). Root cause of the rc=1 was not isolated past the failure signal; deeper instrumentation deferred — *the bind-time architecture sidesteps the question entirely.* DO NOT delete §3.3 / §6.5 — they remain the trail explaining why bind-time is the chosen target. |
| §4 Canonical pattern survey | **STILL VALID** | duckdb-postgres / duckdb-iceberg / duckdb-mysql still don't own long-lived `duckdb_connection`. Added datapoint: **duckdb-delta also avoids `ParserExtension`** entirely (uses `StorageExtension` + table functions per WebFetched 2026-05-22 source); semantic-views is the outlier on every axis. |
| §5 Long-lived handles audit (H1, H2) | **STILL VALID** | H1 and H2 are still the two leaked connections to eliminate. Disposition for H1 changes: catalog reads move to bind/plan time (a new entry point), not per-call inside parser_override. Disposition for H2 unchanged (per-call inside `semantic_view` table function's bind). |
| §6.1 "open a fresh `duckdb_connection` per parser_override invocation" | **FALSIFIED** | The connect-per-parser_override-call is the precise pattern that hit rc=1. Replaced by §16.2's promote-`parse_function` plan. |
| §6.2–6.4 (why not D-07-2/3/4) | **STILL VALID** | All three rejections still hold. |
| §7 Validation Architecture (B1..B14) | **STILL VALID, B13 GREP TARGETS UPDATED** | B1..B12 unchanged. B13 (the structural grep guard) still asserts "no long-lived `duckdb_connection` field on `OverrideContext`" — that's already satisfied (Plan 02 commits `0d2c0b7`, `f9caafe` landed and stay). B14 (ConnGuard Drop test) still applies; consumer of ConnGuard shifts from `rewrite_*` to `sv_plan_function_rust` + bind callbacks. |
| §9 Surfaced findings | **STILL VALID — §9.1 PROMOTED TO IN-SCOPE** | §9.1 *"route catalog reads through caller's `ClientContext` via `plan_function`"* was tagged "milestone-sized refactor, deferred." With D-11 it is no longer deferred — it IS Plan 02A's shape. The forward-looking finding is now the active plan. §9.2 (TECH-DEBT 19) and §9.3 (`:memory:` smoke test) unchanged. |
| §13 Open questions (A6, A7) | **CLOSED** | A6 → `BindInfo` does NOT expose `duckdb_database` (confirmed by Plan 01 SPIKES). A7 → re-entrancy-unsafe (confirmed by Plan 02). Both questions are answered; the planner has the empirical evidence to choose architecture without further spikes. |

**One-line summary:** the parse-time per-call connect path is dead; the bind/plan-time path is alive and is the only path that has (a) a `ClientContext &` parameter and (b) demonstrated freedom from the rc=1 failure mode. §16.2–16.5 detail what that path looks like.

---

### 16.2 Sub-question 1 — Promoting `parse_function` to the success path

**The current shape (Phase 62, in `cpp/src/shim.cpp:262-318`):** `sv_parse_stub` is the registered `parse_function`. It is called by DuckDB *only when* the default Postgres parser fails on an unrecognised prefix AND `parser_override` returned non-PARSE_SUCCESSFUL (the call site is `cpp/include/duckdb.cpp:347286`). Its sole purpose today is rendering caret-aware errors via `ParserExtensionParseResult{ string error_message }` with `result.error_location = optional_idx(byte_offset)`. It NEVER returns `PARSE_SUCCESSFUL` (and `sv_plan_unreachable` enforces that contract by throwing if `plan_function` ever fires).

**The proposed shape:** Move the success path from `parser_override` → `parse_function` → `plan_function`. Three callbacks, three roles:

| Callback | Role under Option A | Receives |
|----------|---------------------|----------|
| `sv_parser_override` | Either (a) **deregister entirely** (set `ext.parser_override = nullptr`) or (b) **demote to validation-only** — detect the prefix, do structural parse, return `DISPLAY_ORIGINAL_ERROR` (rc=2) so the default parser fails on the prefix and DuckDB calls `parse_function` next. **Path (a) is cleaner.** Cost of removing it: the `FALLBACK_OVERRIDE` setting at `shim.cpp:399` (`config.SetOption("allow_parser_override_extension", Value("FALLBACK"));`) becomes irrelevant, but the default parser will still fail on `CREATE SEMANTIC VIEW`, which still triggers the per-statement `parse_function` loop at `duckdb.cpp:347281-347300`. **PLAN-OPEN:** confirm path (a) preserves all error-message paths — TECH-DEBT 21 (`disable_peg_parser` resets the override setting) becomes moot if we remove the override entirely; the caret tests in `test/integration/test_caret_position.py` must still pass via `parse_function` alone. |
| `sv_parse_function` | **Promote to success-path entry.** Detect prefix → structural parse (re-use `validate_and_rewrite` from `src/parse.rs:963`) → stash the validated form into `SemanticViewParseData` → return `PARSE_SUCCESSFUL` with the unique_ptr. On structural failure: return `DISPLAY_EXTENSION_ERROR` with caret position (same as today). The `parse_function` runs at line 347286, *after* the inner `PostgresParser another_parser` scope at line 347253-347277 has fully destructed — so it is NOT subject to the rc=1 re-entrancy failure that broke `parser_override`. **NOTE:** `parse_function` still does NOT receive `ClientContext` (signature is `(ParserExtensionInfo *info, const string &query)`). So **no catalog reads happen here either** — only structural parse + carrier stash. | `info: ParserExtensionInfo*`, `query: const string&` |
| `sv_plan_function` | **Promote from sv_plan_unreachable to the catalog-read + emission entry.** Receives `ClientContext &context` (the one place we get one). Algorithm: (1) extract the validated form from `parse_data`, (2) get `DatabaseInstance& db = *context.db` (or use the `DatabaseInstance::GetDatabase(context)` helper at `duckdb.cpp:179497, 246031`), (3) open a per-call `duckdb_connection` via `ConnGuard::open(db_handle)` where `db_handle` is reconstructed from `db` (see §16.3 for the C-API path), (4) run the existing `rewrite_create` / `rewrite_drop_or_alter` / `emit_native_create_sql` logic on the new connection, (5) drop the guard, (6) return a `ParserExtensionPlanResult` that wraps the work as a TableFunction. | `info, ClientContext &context, unique_ptr<ParserExtensionParseData> parse_data` |

**The hard constraint to design around:** `ParserExtensionPlanResult` does NOT return SQL statements — it returns `{ TableFunction function, vector<Value> parameters, ... }` (see `parser_extension_compat.hpp:108-119`). At `duckdb.cpp:369077` the binder calls `BindTableFunction(parse_result.function, std::move(parse_result.parameters))`. So the `plan_function` result is a SINGLE table-function invocation that runs as the entire bound statement. Today's CREATE path emits `INSERT INTO semantic_layer._definitions ... RETURNING name AS view_name` (a DML statement), not a table-function call. **This is the central design question for Plan 02A.**

Two options for how `sv_plan_function` reconciles "emit SQL" with "must return a TableFunction":

- **Option A1 (emit-then-execute table function):** Register a new internal table function `__sv_execute_native(json_view_name TEXT, native_sql TEXT)` whose bind opens its own connection on `ClientContext.db`, executes `native_sql` (via `Connection::Query(native_sql)`), and projects the result. `sv_plan_function` emits the native SQL string into the parameters vector. Cost: one extra layer of indirection; the actual `INSERT … RETURNING` runs on a *different* connection than the caller, **which loses the transactional behaviour Phase 58 / 62 specifically engineered for.** REJECTED by extension because it regresses Phase 58 transactional DDL (CREATE inside a user transaction can no longer participate).
- **Option A2 (execute via embedded C++ `Connection::Query` on the caller's `ClientContext`):** Inside `sv_plan_function`, *before* returning the `ParserExtensionPlanResult`, directly run the catalog reads + emit native SQL, then call `context.Query(native_sql)` (using the actual `ClientContext &` we were given — it IS the caller's). Return a `ParserExtensionPlanResult` whose `TableFunction` is a trivial "empty result" projection (or a single-row `SELECT view_name`). This *works* but feels like fighting the API — the "result" of the plan is a side-effect, and the returned TableFunction is a sentinel. **PLAN-OPEN:** verify whether running `context.Query(native_sql)` inside `Binder::Bind(ExtensionStatement&)` (which is itself called from `ClientContext::ParseStatementsInternal` / friends) re-enters a lock. Per `duckdb.cpp:272658` `ClientContext::LockContext` acquires `context_lock` — if `Bind(ExtensionStatement&)` is called with `context_lock` already held, `context.Query` will deadlock.
- **Option A3 (route through a new TableFunction that takes the *typed parameters* of the DDL):** Register `__sv_create_view(name TEXT, json_def TEXT)`, `__sv_drop_view(name TEXT, if_exists BOOL)`, `__sv_alter_view_rename(old TEXT, new TEXT, if_exists BOOL)`, `__sv_alter_view_comment(name TEXT, comment TEXT NULL, if_exists BOOL)`. `sv_plan_function` does catalog read + enrichment + JSON build, then returns `ParserExtensionPlanResult { __sv_create_view, ["v", "{...json...}"] }`. The table function's bind/init then executes the actual INSERT on a connection it opens itself. **Same transactional-regression problem as A1** unless we route through the caller's connection somehow — which brings us back to the question A2 raised.

**The deepest question is whether the bind-time `ClientContext &` lets us run DML on the caller's transaction.** If yes (A2 works), the architecture is clean. If no (lock re-entry), we need to either (a) accept losing transactional DDL behaviour (regression vs. v0.8.0) or (b) find a different mechanism — e.g., return `ParserExtensionPlanResult` whose TableFunction's bind happens to run on the caller's connection by virtue of being a normal bind, and emit DML from within that bind via a side-channel. This is gnarly.

**Plan 02A spike requirement (BEFORE any production code):** Reproduce the simplest possible Option A2 — a 30-LOC `sv_plan_function` that does `context.Query("SELECT 1")` inside the bind, returns a trivial TableFunction result, and confirm whether (i) the inner query succeeds, (ii) the bound caller's transaction sees its effect, (iii) no deadlock. **If A2 works → Plan 02A proceeds with A2. If A2 deadlocks → escalate to user via `checkpoint:decision` (likely A3 with documented transactional regression, or revisit whether the upstream `parser_override` rc=1 has a workaround we missed).**

**Plan 02A should:** start with a 1–2 day spike on Option A2 viability (per above); if green, refactor `sv_parse_stub` → `sv_parse_function` (success path) and `sv_plan_unreachable` → `sv_plan_function` (catalog reads + emission via `context.Query`); remove the 4× `ConnGuard::open` call sites inside `parse.rs::rewrite_*` and re-home them inside `sv_plan_function`; preserve `SemanticViewParseData` as the parse-time carrier (see §16.4). If A2 spike fails, return to user with concrete failure evidence and the A1/A3 trade-off menu.

**PLAN-OPEN:** The exact mechanism by which `sv_plan_function` runs the native SQL (Option A1/A2/A3) is the central unresolved question. The spike named above is the smallest experiment that distinguishes them.

---

### 16.3 Sub-question 2 — `ClientContext` / `duckdb_connection` access from bind callbacks

**Read-path scope:** 14 table functions registered with `register_table_function_with_extra_info` + 2 scalar functions registered with `register_scalar_function_with_state` (all in `src/lib.rs:425-495`). These are NOT `parser_override` callbacks — they are bind callbacks on user table-function calls like `SELECT * FROM list_semantic_views()`. Plan 01 Spike A6 already confirmed `BindInfo` does NOT expose `duckdb_database` in `duckdb-rs 1.10502.0`.

**Confirmed via verbatim grep of `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/duckdb-1.10502.0/`:**

```bash
$ grep -rn "client_context\|connection_get\|get_connection\|database_handle\|duckdb_database\b" \
    src/vtab/ src/vscalar/ src/extension.rs src/core/ 2>/dev/null
src/inner_connection.rs:21:    db: ffi::duckdb_database,
src/inner_connection.rs:25:// `duckdb_database` is an opaque C pointer. We share it via `Arc` so the database
src/inner_connection.rs:32:    pub fn new(db: ffi::duckdb_database, close_on_drop: bool) -> Self {
src/inner_connection.rs:37:    pub fn raw(&self) -> ffi::duckdb_database {
src/inner_connection.rs:91:            let mut db: ffi::duckdb_database = ptr::null_mut();
src/inner_connection.rs:104:    pub(crate) unsafe fn new_from_raw_db(raw: ffi::duckdb_database, close_on_drop: bool) -> Self
src/lib.rs:287:    pub unsafe fn open_from_raw(raw: ffi::duckdb_database) -> Self
```

Zero hits inside `vtab/`, `vscalar/`, `extension.rs`, `core/`. The full method surface of `BindInfo` (verbatim from `src/vtab/function.rs:27-113`) is `{ add_result_column, set_error, set_bind_data, get_parameter_count, get_parameter, get_named_parameter, set_cardinality, get_extra_info }`. `InitInfo` and `TableFunctionInfo` are similarly bare.

**Confirmed via grep of libduckdb-sys C bindings:**

```bash
$ grep -n "pub fn duckdb_bind_\|pub fn duckdb_function_\|pub fn duckdb_init_" \
    ~/.cargo/registry/src/index.crates.io-*/libduckdb-sys-1.10502.0/src/bindgen_bundled_version.rs
2555:pub fn duckdb_bind_get_extra_info(info: duckdb_bind_info) -> *mut c_void;
2563:pub fn duckdb_bind_add_result_column(...)
2571:pub fn duckdb_bind_get_parameter_count(info: duckdb_bind_info) -> idx_t;
2575:pub fn duckdb_bind_get_parameter(info, index) -> duckdb_value;
2579:pub fn duckdb_bind_get_named_parameter(...)
2584:pub fn duckdb_bind_set_bind_data(...)
2592:pub fn duckdb_bind_set_cardinality(...)
2596:pub fn duckdb_bind_set_error(...)
2600:pub fn duckdb_init_get_extra_info(...)
2604:pub fn duckdb_init_get_bind_data(...)
2632:pub fn duckdb_function_get_extra_info(...)
2636:pub fn duckdb_function_get_bind_data(...)
2640:pub fn duckdb_function_get_init_data(...)
2644:pub fn duckdb_function_get_local_init_data(...)
```

**No `duckdb_bind_get_client_context`, `duckdb_function_get_client_context`, or `duckdb_init_get_client_context` in the C API.** This is a *DuckDB upstream gap*, not a duckdb-rs binding gap — the C-API for table-function bind callbacks just doesn't expose a `ClientContext`. (Contrast: `parser_extension`'s `plan_function` DOES receive a `ClientContext &` because it's invoked directly from `Binder::Bind(ExtensionStatement &stmt)` in C++ at `duckdb.cpp:369065-369085`, never through the C ABI.)

**Note on `duckdb_connection_get_client_context`:** the C API DOES have `duckdb_connection_get_client_context(connection, out_context)` (line 921 of `bindgen_bundled_version.rs`), but that's the *reverse* direction — connection → client_context. It does not help here because we don't have a connection at bind time; we're trying to derive one.

**Conclusion:** The bind/plan-time architecture for the read-path 14 table functions + 2 scalars must use the `extra_info` payload to carry `db_handle`. This is exactly what Plan 01 Spike A6 named as "shape (a)" — `CatalogHandle { db: duckdb_database, catalog_table_present: bool }` passed to `register_table_function_with_extra_info`; each bind callback calls `bind.get_extra_info::<CatalogHandle>()` then `ConnGuard::open(handle.db)`. The guard drops at bind end before the bind callback returns, so no long-lived connection survives.

**Re-litigation of the D-10 falsification for the bind thread:** the rc=1 failure was inside `parser_override`, which runs on the parse thread *during* `Parser::ParseQuery` execution (`duckdb.cpp:347194`). The bind callbacks for table functions run *after* parsing is complete, during `Binder::Bind(TableFunctionRef&)` — a different lifecycle phase with no `PostgresParser` in scope. **There is no evidence that `duckdb_connect` from a bind-callback thread suffers the same rc=1 failure.** Specifically: the existing v0.8.0 `init_extension` (`src/lib.rs:387, 499`) already calls `duckdb_connect` twice during extension load, succeeds, and the long-lived connections work fine — they only fail on TEARDOWN (the busy-spin). So `duckdb_connect` itself works in non-parse contexts on DuckDB 1.5.2.

**Plan 02A should:** introduce `CatalogHandle { db: duckdb_database, catalog_table_present: bool }` in a new module (`src/catalog_handle.rs` or fold into `src/conn_guard.rs`); update the 14 + 2 register sites in `src/lib.rs` to pass `&catalog_handle` instead of `&catalog_reader`; refactor each bind callback (in `src/query/`, `src/show/`, etc.) to (i) `bind.get_extra_info::<CatalogHandle>()`, (ii) `ConnGuard::open(handle.db)`, (iii) construct a `CatalogReader::new(guard.raw(), handle.catalog_table_present)` for the existing helpers, (iv) drop guard before bind returns. **A small spike (~10 LOC change to ONE of the 14 read functions) should validate the pattern before mass refactor — this is the bind-thread analogue of Plan 02's parser-thread spike and is what D-10 implicitly demands we run to confirm rc=1 is not universal.**

**PLAN-OPEN:** Whether the existing `CatalogReader` struct can be refactored to carry an *optional* lifetime-bounded `duckdb_connection` reference (so it borrows from the bind-scope `ConnGuard` instead of owning a Copy-pointer to a leaked one), or whether `CatalogReader::new` is called fresh each bind. The latter is simpler; the former preserves `CatalogReader`'s existing prepared-statement caching (if any). Audit `src/catalog.rs` for any per-CatalogReader state that benefits from instance reuse — if none, the simpler shape wins.

---

### 16.4 Sub-question 3 — `SemanticViewParseData` carrier shape

**Current shape (`cpp/src/shim.cpp:116-126`):**

```cpp
struct SemanticViewParseData : public ParserExtensionParseData {
    string query;
    explicit SemanticViewParseData(string q) : query(std::move(q)) {}

    unique_ptr<ParserExtensionParseData> Copy() const override {
        return make_uniq<SemanticViewParseData>(query);
    }
    string ToString() const override {
        return query;
    }
};
```

**Today this struct is never instantiated in practice** — `sv_parse_stub` only ever returns `DISPLAY_EXTENSION_ERROR` or `DISPLAY_ORIGINAL_ERROR`, never `PARSE_SUCCESSFUL`. The struct exists for *type-system layout reasons* because `ParserExtensionParseResult(unique_ptr<ParserExtensionParseData>)` requires *some* concrete subclass to compile.

**Under Option A, `SemanticViewParseData` becomes the actual carrier between `parse_function` and `plan_function`.** What `sv_plan_function` needs to do its work (per §16.2):

| Field | Source at parse time | Used by `plan_function` for |
|-------|----------------------|-----------------------------|
| `query: string` | The raw input string (already present) | Caret rendering on later-discovered semantic errors; error_location offsets are relative to this string |
| `verb: DdlKind` (enum: `Create`, `Drop`, `Alter`, `Show`, `Describe`, `List`, …) | `crate::parse::detect_ddl_kind(query)` already returns this | Dispatch in `sv_plan_function` to `rewrite_create` vs. `rewrite_drop_or_alter` vs. read-side passthrough |
| `validated_form: String` (the table-function-style intermediate emitted by `validate_and_rewrite`) | `validate_and_rewrite(query)?` — already runs in current `sv_parser_override_rust` | Skip re-parsing the body in `plan_function`; pass directly to `rewrite_to_native_sql`-equivalent |
| `or_replace: bool` / `if_not_exists: bool` / `if_exists: bool` (verb-specific flags) | Extracted during `detect_ddl_kind` + `rewrite_ddl` | Disambiguate CREATE/CREATE-OR-REPLACE/CREATE-IF-NOT-EXISTS and DROP/DROP-IF-EXISTS without re-parsing |
| `view_name: String` (the bare view identifier, already normalised) | `normalize_view_name` from `src/ident.rs` | Catalog existence check |
| Source-location metadata: a single `byte_offset: optional<u32>` for the start of the verb keyword | Tracked during `detect_ddl_kind` byte-walking | Caret rendering if `plan_function` discovers a catalog-level error (e.g., "view already exists") and wants to point at the verb |

**The pragmatic shape:** keep `query: string` (already there) and add a small `payload: vector<uint8_t>` opaque to C++ that the Rust side fills with a CBOR/JSON/manual-serialized snapshot of `{verb, validated_form, view_name, or_replace, if_not_exists, if_exists, byte_offset}`. C++ never inspects `payload`; it just round-trips it from `parse_function` to `plan_function`. The Rust FFI for `sv_plan_function_rust` reads `payload`, deserializes, and dispatches. **This is the cleanest cross-FFI carrier — opaque bytes — and avoids declaring 6+ separate fields in `cpp/src/shim.cpp` that C++ doesn't need to know about.**

Alternative: declare each field explicitly in C++ (`uint8_t verb; string view_name; bool or_replace; …`). Pros: no Rust-side serialization. Cons: the C++ shim grows by ~50 LOC of plumbing every time the Rust side needs a new piece of parse-time information; updates require ABI-synchronised changes to two files instead of one.

**Plan 02A should:** add `payload: vector<uint8_t>` to `SemanticViewParseData` (alongside the existing `query: string`); update `Copy()` to clone both; on the Rust side use `bincode` or a manual little-endian layout (no external deps needed for a 6-field record) — see `src/parse.rs`'s existing `unescape_sql_arg`/`escape_sql_arg` style for a precedent of avoiding new dependencies. The carrier's serialization format is internal to the extension; no compatibility constraint.

**Re-parsing avoidance:** `validate_and_rewrite(query)` is the heaviest step in the current `sv_parser_override_rust` (full body parse for CREATE; lighter for DROP/ALTER). Stashing its output in `payload` ensures `sv_plan_function_rust` does ONLY the catalog reads + native-SQL emission — it does not redo body parsing. This is both a performance win (parse the body once, not twice) and a correctness property (the validated form is what's stashed; any drift between parse and plan is impossible by construction).

**PLAN-OPEN:** the encoding format for `payload` (manual little-endian vs. `bincode` 1.3.x vs. `postcard`). Manual LE is ~30 LOC, zero new dependencies, easy to audit. Recommend manual LE for v0.9.1; revisit if the carrier grows past ~10 fields.

---

### 16.5 Sub-question 4 — Community extension survey (timeboxed, ~20 min spent)

The Phase 65 original research surveyed `duckdb-postgres`, `duckdb-mysql`, `duckdb-iceberg` and concluded none use `ParserExtension` for DDL — they all use `StorageExtension` (ATTACH) or table functions. The refresh checked one additional extension and re-verified the negative claim:

| Extension | Uses `ParserExtension`? | DDL mechanism | Catalog-read-during-DDL pattern |
|-----------|-------------------------|---------------|--------------------------------|
| `duckdb-delta` (delta_kernel-rs) | **No** | `StorageExtension::Register(config, "delta", …)` + `DeltaCatalogAttach` for `ATTACH … (TYPE delta)`; table functions for scans | `DeltaCatalog` owns metadata; catalog reads happen inside the catalog implementation, not in any extension callback. Source: `src/delta_extension.cpp` via WebFetch 2026-05-22. |
| `duckdb-postgres` (verified §4.1) | **No** | `StorageExtension::Register` + ATTACH | No catalog-read-during-DDL because no DDL surface; secrets go through `SecretManager`. |
| `duckdb-mysql` (verified §4.2) | **No** | Same as postgres | Same. |
| `duckdb-iceberg` (verified §4.3) | **No** | `StorageExtension::Register` + `IRCStorageExtension`; ATTACH-based | Same. |
| RFC: RBAC extension (gist `dufferzafar/f12081d4`) | **Yes** — proposed `parse_function` + `plan_function` for GRANT/REVOKE | RFC explicitly notes `plan_function` returns *"a TableFunction that executes the DDL"* — matches §16.2 Option A1/A3 problem | Catalog reads via `ClientContextState` cached at `OnConnectionOpened` (the duckdb-postgres pattern). RFC is exploratory; no shipped code. |
| `duckdb_extension_parser_tools` (hotdata-dev / zfarrell) | **Yes** — uses `ParserExtension` to expose SQL parsing as user-facing scalar functions (`parse_tables()`, etc.) | Pure parsing surface; no DDL emission, no catalog reads. Not a useful analog. | N/A |

**What none of the surveyed extensions provide:** a worked example of `parse_function` + `plan_function` that emits DML on the caller's transaction. The RBAC RFC says "return a TableFunction" without resolving the transaction question; the parser_tools extension only does pure parsing. **The semantic-views extension is solving a problem the surveyed extension corpus hasn't solved.** This validates D-01 / D-05's framing ("find the *correct* model") — we are doing original architectural work, not copying a known pattern.

**Implication for Plan 02A's spike (§16.2):** no upstream reference implementation exists to crib from. The Option A2 viability spike (`context.Query("SELECT 1")` from inside `plan_function`) is the smallest experiment that proves the architectural shape.

**DEFERRED:** deeper read of how httpfs's `CREATE SECRET` is implemented (`SecretManager`-based, not `ParserExtension`-based, so different mechanism entirely). Confirmed via earlier WebSearch results that secrets do NOT go through `ParserExtension`. Research budget exhausted on community survey — additional digging would not change the conclusion.

---

### 16.6 Planner inputs

Concrete decisions the planner needs to make for Plan 02A:

1. **`sv_parser_override` disposition:** remove entirely (set to nullptr) OR keep as a validation-only no-op that returns `DISPLAY_ORIGINAL_ERROR` to force fall-through to `parse_function`. **Recommendation:** remove entirely; one less callback to reason about, TECH-DEBT 21 (`disable_peg_parser` resetting the override setting) becomes moot. Verify caret tests still pass via `parse_function` alone.

2. **`plan_function` execution mechanism (Option A1 vs. A2 vs. A3 from §16.2):** spike Option A2 first (run `context.Query(native_sql)` inside `plan_function` — preserves transactional semantics). If it deadlocks, escalate via `checkpoint:decision` with A1/A3 trade-off summary. **This spike MUST land before any production refactor.**

3. **`SemanticViewParseData` carrier encoding (manual LE vs. bincode):** recommend manual LE for v0.9.1 (zero new dependencies, ~30 LOC, easy audit). Revisit only if carrier grows past ~10 fields.

4. **Read-path refactor shape (`CatalogHandle` vs. refactored `CatalogReader`):** Plan 01 Spike A6 already recommended shape (a) (`CatalogHandle { db, catalog_table_present }` in extra_info). Honour that choice. Run a 10-LOC spike against ONE of the 14 read functions to confirm `duckdb_connect` from a bind-callback thread does not hit the same rc=1 as `parser_override` did (no evidence it would, but verify cheaply before mass refactor — this is the bind-thread analogue D-10 implicitly demands).

5. **Plan 02 commits to keep vs. revert:** keep `0d2c0b7`, `f9caafe`, `656bae7` (per D-12 — `db_handle` plumbing is foundation; signature update is correct for both shapes; evidence log is the audit trail). Remove the 4× `ConnGuard::open` call sites inside `parse.rs::rewrite_*` (the known-broken surface).

6. **Plan re-scoping:**
   - **Plan 02A (NEW):** the bind/plan-time reshape (this section's output). Replaces the broken parse-time per-call ConnGuard surface with `parse_function` + `plan_function` success path.
   - **Plan 03 (read-path):** likely unchanged in spirit but verify shape — H2 (`query_conn`) removal still uses `CatalogHandle` in `extra_info` + per-bind `ConnGuard` (matches Plan 01 A6 outcome).
   - **Plan 04 (ledger + structural guards):** B13 grep targets unchanged (`OverrideContext` still must not regrow a `duckdb_connection` field — Plan 02 already removed it).

7. **Transactional regression risk:** if the spike forces us to A1/A3, document the transactional DDL regression explicitly as a TECH-DEBT entry (CREATE inside user transaction no longer atomic). **Surface to user before shipping**, per CONTEXT.md D-01 — "documenting the limitation" is admissible only if (a)-direction is impossible, and A2 spike is the discriminator.

8. **A6 follow-up spike (bind-thread `duckdb_connect`):** before the 14 + 2 mass refactor, run a one-file spike opening `ConnGuard::open(handle.db)` from inside the `list_semantic_views` bind callback and confirm rc=0. If rc=1 (i.e., D-10 generalises to bind threads), the planner must escalate immediately — the read path becomes architecturally constrained too.

---

*End of §16. §§1–15 remain authoritative except where flagged in §16.1.*

---

## RESEARCH COMPLETE

**Phase:** 65 — OverrideContext Connection Teardown
**Confidence:** HIGH

### Key findings
- **Root cause confirmed via direct amalgamation read:** the ">45s hang" is a CPU busy-spin in `DBInstanceCache::GetInstanceInternal` (duckdb.cpp:278022-278024), driven by the extension's two long-lived `duckdb_connection`s (`catalog_conn` at `src/lib.rs:383-387` and `query_conn` at `src/lib.rs:494-498`) each holding a `shared_ptr<DatabaseInstance>` via `ClientContext::db`. Same path keeps `DBConfig::db_cache_entry` alive, so `weak_cache_entry.expired()` never returns true.
- **Phase 62 §Q2's "bounded leak" framing was wrong about impact:** the leak is bounded in bytes but unbounded in functional consequence (DB unreopenable in same process). The structural conclusion (no safe teardown point at `~SemanticViewsParserInfo`) is correct; the conclusion drawn from it ("just leak") was the wrong fix.
- **Canonical pattern (duckdb-postgres) does not own long-lived `duckdb_connection` at all** — state lives in `ClientContext::registered_state` (per-connection). The semantic-views extension is the outlier.
- **Recommended fix: D-07 candidate 1** — short-lived per-DDL `duckdb_connect`+`duckdb_disconnect`. Store `db_handle: duckdb_database` (not `duckdb_connection`) in `OverrideContext`. Same for the `semantic_view` table function's `QueryState`. Eliminates the lifetime question entirely. Per-call overhead is ~µs (negligible). Lock-contention behaviour unchanged.
- **D-07 candidates 2-4 rejected** with evidence: (2) UAF + no unload hook + `OnConnectionClosed` deadlock; (3) C-API does not expose weak handles; (4) CONTEXT.md D-01 forbids when (1) is viable.
- **Long-lived handles audit (D-03):** only H1+H2 (the two leaked connections) are broken; both are this phase's fix targets. No new TECH-DEBT or follow-up phases required for adjacent lifecycle issues. One forward-looking finding surfaced: route catalog reads through caller's `ClientContext` via `plan_function` — strategic, not v0.9.1 scope (§9.1).
- **RO→RW reverse direction (D-09)** is fixed for free by the same change — same root cause, same cure.

### File created
`.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md`

### Confidence assessment

| Area | Level | Reason |
|------|-------|--------|
| Root cause (busy-spin) | HIGH | Traced through vendored DuckDB v1.5.2 amalgamation with file:line citations |
| DuckDB lifecycle surface | HIGH | Direct read of `ExtensionCallback`, `~DatabaseInstance`, `ConnectionManager` |
| Canonical pattern | HIGH | Verified against duckdb-postgres source; structural argument backs the conclusion |
| Per-DDL cost analysis | MEDIUM | Order-of-magnitude reasoning, not benchmarked; assumption tagged A3 |
| `BindInfo` exposure of `db_handle` | MEDIUM | Wave 0 spike recommended (A6) — both possible code shapes work |
| Parser-override re-entrancy safety | MEDIUM | Wave 0 smoke test recommended (A7) |

### Open questions remaining
- A6: confirm via Wave 0 spike whether `BindInfo` exposes `db_handle` directly or whether the new handle struct must carry it.
- A7: Wave 0 smoke test that nested `duckdb_connect`+`duckdb_disconnect` from inside `parser_override` is safe.
- §9.3: confirm `:memory:` DBs work end-to-end with the per-call connect.

None are blocking — all can be answered in <5 minute Wave 0 spikes by the implementer.

### Ready for planning
Planner can now create PLAN files. Suggested wave structure:
- **Wave 0:** verify A4 (busy-spin via lldb), A6 (BindInfo surface), A7 (parser_override re-entrancy); add Wave-0 watchdog scaffolding.
- **Wave 1:** introduce RAII `ConnGuard` type in `src/conn_guard.rs` (or in `src/parse.rs`); refactor `OverrideContext` to carry `db_handle`; refactor `sv_parser_override_rust` to open/close per call.
- **Wave 2:** refactor `QueryState` and `semantic_view` / `explain_semantic_view` to open/close per query (bind/init/Drop).
- **Wave 3:** refactor read-side table functions (list / describe / show_* / get_ddl / read_yaml) to use the new handle type.
- **Wave 4:** add the 5 new tests (B1-B4 + B11) in `test/integration/test_readonly_load.py`; update LIFE-04 deferred-items.md in place.
- **Wave 5:** full `just test-all` + `just ci` green gate.

Sources:
- [Connection Management | duckdb/duckdb-mysql | DeepWiki](https://deepwiki.com/duckdb/duckdb-mysql/3.1-connection-management)
- [How to (un)lock the Database connection? · duckdb/duckdb · Discussion #10397](https://github.com/duckdb/duckdb/discussions/10397)
- [Extension Architecture | duckdb/duckdb | DeepWiki](https://deepwiki.com/duckdb/duckdb/4.1-extension-architecture)
- [DuckDB HTTPFS extension](https://github.com/duckdb/duckdb-httpfs)
- [Connection pooling in ducklake · duckdb/ducklake · Discussion #299](https://github.com/duckdb/ducklake/discussions/299)
