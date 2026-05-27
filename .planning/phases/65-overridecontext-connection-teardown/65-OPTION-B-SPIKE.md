# Phase 65 — Option B Spike Evidence

**Spike:** Option B (`duckdb_connect(*context.db)` from inside `sv_plan_function`, i.e. post-parse, mid-bind under `Binder::Bind(ExtensionStatement&)`)

**Goal:** Determine whether the simplest possible Phase 65 architecture is viable: open a fresh `duckdb_connection` per-call from inside `sv_plan_function`. If `rc=0`, all of Option A's bind/plan-time machinery (parse-time stash → plan-time enrichment) collapses into a single per-call connection at plan-time, with no long-lived extension state anywhere. If `rc=1` (the same failure mode as D-10 `parser_override` thread and `BIND-THREAD-RC1` bind thread), Option B is dead and we move to Options A / C / D.

This spike is the remaining untested hypothesis in the lifecycle-phase grid: parse-thread (D-10 → rc=1), bind-thread (`BIND-THREAD-RC1` → rc=1), plan-thread (this spike).

---

## B

**Question (CONTEXT.md D-11 / RESEARCH §16.6 #2 — plan-thread variant):** Does `duckdb_connect(db_handle)` (and the equivalent C++ `Connection(*context.db)`) called from inside `sv_plan_function` (post-parse, mid-bind under `Binder::Bind(ExtensionStatement&)`) succeed (rc=0) or fail (rc=1, the same failure mode that broke D-10 / `BIND-THREAD-RC1`)?

**Setup:**

1. Modified `cpp/src/shim.cpp::sv_parse_stub` to detect the `SPIKE_PLAN_DUCKDB_CONNECT_PROBE` sentinel prefix at the top of the function and return `ParserExtensionParseResult(make_uniq_base<ParserExtensionParseData, SemanticViewParseData>(query))` — the success path that triggers `plan_function`. Pre-existing logic untouched.
2. Replaced `sv_plan_unreachable` with a probe body that runs TWO independent probes and returns a trivial sentinel TableFunction:
   - **Probe 1 (C++ direct):** `Connection probe1(*context.db);` wrapped in try/catch. Logs success / what() on throw. Lets destructor fire at end of scope to observe any teardown deadlock.
   - **Probe 2 (C-API):** retrieves the `duckdb_database` handle from `SemanticViewsParserInfo::rust_state` via a new scratch FFI accessor `sv_get_override_context_db_handle`, then calls `duckdb_connect(db_handle, &conn)`. Logs the rc. On rc=0 calls `duckdb_disconnect(&conn)` to clean up.
   - Returns `ParserExtensionPlanResult` whose `TableFunction("__sv_b_spike_sentinel", {}, sv_b_spike_function, sv_b_spike_bind)` declares one VARCHAR column (`spike_b`) and emits zero rows.
3. Added scratch FFI accessor `sv_get_override_context_db_handle(ctx_ptr) -> duckdb_database` at the bottom of `src/parse.rs`. Reads back the first field (`db_handle: duckdb_database`) from the boxed `OverrideContext` and returns it. Confirmed field order from `pub struct OverrideContext { pub db_handle: ..., pub catalog_table_present: bool, pub is_file_backed: bool, }` (`src/parse.rs:67-72`).
4. `fprintf(stderr, "[B-SPIKE] ...")` traces between each probe step.
5. Test driver: `test/sql/65_option_b_spike.test`:

   ```sql
   require semantic_views
   statement ok
   LOAD semantic_views;
   statement ok
   SPIKE_PLAN_DUCKDB_CONNECT_PROBE
   ```
6. Built via `just build` (cargo `--features extension` + cdylib pack). Build completed cleanly in ~1m31s.
7. Ran via `timeout 30 ./configure/venv/bin/python3 -u -m duckdb_sqllogictest --test-dir test/sql --file-list <(echo test/sql/65_option_b_spike.test) --external-extension build/debug/semantic_views.duckdb_extension` with stderr captured to `/tmp/claude/spike-b/spike.log`. Process exited cleanly (exit code 0); no deadlock.

**Result (conclusion line):** `PLAN-THREAD-RC1`

The C-API probe (`duckdb_connect`) failed with rc=1 — the same failure mode as D-10 (parse thread) and `BIND-THREAD-RC1` (bind thread). The C++ direct `Connection(*context.db)` probe SUCCEEDED (ctor + dtor both completed without throwing). The split between the two probes is the most informative finding from this spike (see Interpretation).

**Verbatim stderr from the spike run:**

```
[1/1] test/sql/65_option_b_spike.test
[B-SPIKE] sv_parse_stub: sentinel detected, returning PARSE_SUCCESSFUL
[B-SPIKE] sv_plan_unreachable entered (probe context)
[B-SPIKE] probe 1: Connection ctor on *context.db
[B-SPIKE] probe 1: Connection ctor succeeded
[B-SPIKE] probe 1: Connection destructor completed
[B-SPIKE] probe 2: duckdb_connect via OverrideContext db_handle
[B-SPIKE] probe 2: duckdb_connect rc=1 (0=success, 1=error)
[B-SPIKE] returning sentinel TableFunction
SUCCESS
```

(No hang. No lldb backtrace required — both probes returned synchronously and the sqllogictest exited cleanly.)

**Interpretation:**

The literal answer to the spike's framing question (`duckdb_connect(*context.db)` succeeds or fails from `sv_plan_function`) is **rc=1** — same failure mode as the parse and bind threads, generalising the D-10 / `BIND-THREAD-RC1` falsification to the plan thread. The C-API path is dead at every lifecycle phase we have empirically tested.

But the more interesting finding is the split between Probe 1 and Probe 2 on the same thread, against the same `DatabaseInstance`, microseconds apart:

- **Probe 1** constructed a `duckdb::Connection` directly on `*context.db` (a `DatabaseInstance &` dereferenced from `shared_ptr<DatabaseInstance>`). Both ctor (which calls `ConnectionManager::AddConnection`, acquiring `connections_lock`) and dtor (which calls `RemoveConnection`, re-acquiring `connections_lock`) completed without throwing or deadlocking.
- **Probe 2** called the C-API `duckdb_connect` on the `duckdb_database` handle the extension originally received in `init_extension` (stashed verbatim on `OverrideContext.db_handle`). `duckdb_connect`'s implementation at `cpp/include/duckdb.cpp:266432-266447` reinterprets the handle as `DatabaseWrapper *`, calls `new Connection(*wrapper->database)` (where `wrapper->database` is `shared_ptr<DuckDB>` — the `Connection(DuckDB&)` ctor at `duckdb.cpp:275780` delegates straight to `Connection(*database.instance)`, the same DatabaseInstance ctor Probe 1 uses), and returns `DuckDBError` (rc=1) if anything throws inside that try/catch.

Both paths terminate at the same `Connection(DatabaseInstance&)` ctor in the amalgamation. Probe 1 reached it successfully against the live `context.db` instance. Probe 2 returned rc=1, meaning either:

1. The reinterpret_cast from `duckdb_database` to `DatabaseWrapper *` is yielding a different (stale / re-assigned / wrong) pointer than expected at plan-time, so `wrapper->database` is either null, dangling, or points at a different `DuckDB` than the live `context.db`. The Connection ctor then throws (most plausibly during `database.shared_from_this()` or `ConnectionManager::Get(database)` if the wrapper's database is not the same instance the bind/plan path is running against), and `duckdb_connect` translates the throw to rc=1.
2. Or `wrapper->database` is the right instance, but the C-API wrapper's try/catch suppresses an exception that the direct path would not raise. (Less likely — the ctor itself is the only meaningful thing happening between the two paths.)

Either way the consequence for Option B's architectural premise is the same:

- **The "simple `duckdb_connect` from plan-time" architecture is not viable** as posed (per-call `duckdb_connect(ctx.db_handle)` from `sv_plan_function`). The C-API path that the existing extension code is built on (`ConnGuard::open(db_handle)`, `init_extension`'s `query_conn`, etc. — all C-API based) inherits this failure mode.
- **However**, the C++ direct path (`Connection(*context.db)` from inside `sv_plan_function`) works. This was previously untested and is a new finding. It means an Option B-prime is viable: do the per-call connection in C++ on `*context.db`, not via the C-API on the stashed `duckdb_database` handle. The catalog reads currently done via `CatalogReader` (Rust, C-API-based) would need a parallel C++ implementation, or a way to wrap the freshly-opened C++ `Connection` back into a `duckdb_connection` handle Rust can use. Neither is free; both should be evaluated against Options A / C / D before adopting.
- **Probe 1's success also means** `ConnectionManager::AddConnection` and `RemoveConnection` do NOT deadlock against any lock held by the binder. The deadlock observed in §A2 (`ClientContext::context_lock`) is specifically about re-entering `ClientContext::Query` — NOT about acquiring `connections_lock` to add/remove a connection. This rules out one whole class of "the binder holds locks that prevent any new connection" theory and re-pins the original D-10 / `BIND-THREAD-RC1` failure as something specific to the C-API wrapper path, not to "connections cannot be opened during bind/plan."

**Architectural implication:**

- Option B (per-call C-API `duckdb_connect` from plan-time) is **dead** as literally framed — but only because of the C-API wrapper path, not because of any fundamental binder-lock conflict.
- The candidate that this spike newly surfaces ("B-prime: per-call C++ `Connection(*context.db)` from plan-time") is viable on the connection-open side, but requires a separate research pass on (a) routing the catalog reads through this C++ connection rather than the C-API, and (b) understanding why the C-API wrapper specifically fails when the underlying DatabaseInstance ctor path works. Both should be carried as new inputs to the next `/gsd:discuss-phase` or replan.
- Options A (parse-stash + plan-enrich on the caller's connection), C (direct C++ catalog API), and D (StorageExtension rewrite) remain on the menu for the planner. Option C in particular now has empirical support — the C++ direct path is the only one that works on the live `context.db` during plan-time.

**Spike artefacts reverted:** `git diff --stat cpp/src/shim.cpp src/parse.rs src/lib.rs` returns empty after `git checkout`. The scratch test file `test/sql/65_option_b_spike.test` was removed. Only this `65-OPTION-B-SPIKE.md` is committed from this task. No modifications to STATE.md, ROADMAP.md, plan files, or any other planning artifact. The broken `just test-sql` baseline (4/47 PASS from Plan 02 partial state, preserved per D-12) is unchanged.
