# Phase 65 — Read-Path Spike Evidence

**Spike:** Read-path counterpart to Option B-prime — `Connection probe1(*context.db)` (C++ direct path, same one that succeeded in `65-OPTION-B-SPIKE.md` from `plan_function`) invoked from inside a read-side table-function `bind` callback (i.e. mid-`Binder::Bind(TableFunctionRef&)`).

**Goal:** Determine whether B-prime is the **unified** architecture for both the write path (CREATE / DROP / ALTER SEMANTIC VIEW via `plan_function`) and the read path (`SELECT * FROM list_semantic_views()` + 13 sibling table functions + 2 scalars). If rc=0, the entire extension can converge on a single per-call mechanism: `Connection(*context.db)`. If rc=1, the read path still needs a different mechanism (Option A — `ExtensionCallback` + `registered_state`).

This spike is the matching cell of the lifecycle-phase grid for the C++ direct path: `65-OPTION-B-SPIKE.md` covered plan-thread (Connection ctor RC0). `BIND-THREAD-RC1` (`65-02-SPIKES.md §A6-bind`) covered bind-thread but only via the C-API path (`duckdb_connect(db_handle)`) — never the C++ direct path. This spike fills the remaining unknown.

---

## READ-BIND

**Question (CONTEXT.md D-11(ii) / RESEARCH §16.6 #8 — bind-thread C++ variant):** Does `Connection(*context.db)` (C++ direct ctor, the path that succeeded from `plan_function` in Option B spike Probe 1) ALSO succeed when invoked from a read-side table-function `bind` callback (post-parse, mid-`Binder::Bind(TableFunctionRef&)`)?

**Why this is non-trivial to spike:** read-side bind callbacks in this extension are registered via duckdb-rs's `register_table_function_with_extra_info`, which wires the C-API `duckdb_table_function_set_bind` mechanism. That mechanism marshals `ClientContext &` away — the callback receives a `BindInfo` (per Plan 01 Spike A6 confirmed not to expose `db_handle` or `ClientContext`). To get `ClientContext &` at bind time we must register the table function DIRECTLY via the C++ `Catalog` API (which uses the native `TableFunction` signature whose `bind` is `unique_ptr<FunctionData>(ClientContext &, TableFunctionBindInput &, vector<LogicalType> &, vector<string> &)`), bypassing duckdb-rs entirely. The spike adds a C++-only scratch table function `__sv_read_path_spike()` for this purpose.

**Setup:**

1. Added a scratch table function in `cpp/src/shim.cpp`: `__sv_read_path_spike()` — zero arguments, returns one BIGINT column `connect_rc`. Registered DIRECTLY via the C++ Catalog API:

   ```cpp
   TableFunction tf("__sv_read_path_spike", {},
                    sv_read_path_spike_function,
                    sv_read_path_spike_bind,
                    sv_read_path_spike_init);
   CreateTableFunctionInfo info(tf);
   info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
   auto &system_catalog = Catalog::GetSystemCatalog(db);
   auto txn = CatalogTransaction::GetSystemTransaction(db);
   system_catalog.CreateTableFunction(txn, info);
   ```

   This bypasses duckdb-rs entirely — `sv_read_path_spike_bind` has the native `ClientContext &` argument that duckdb-rs's wrapper marshals away.

2. The bind callback (`sv_read_path_spike_bind`):
   - Logs `[READ-SPIKE] bind entered` to stderr.
   - **Probe 1:** wraps `Connection probe1(*context.db);` in try/catch. On success logs and sets `rc = 0`. On `std::exception &e` logs `e.what()` and sets `rc = 1`. On unknown throw sets `rc = 1`. Lets `probe1` destruct at end of scope to observe any teardown deadlock (`ConnectionManager::RemoveConnection` re-acquires `connections_lock`, mirroring the Option B spike's Probe 1 success path).
   - Adds one `BIGINT` return column named `connect_rc`.
   - Returns a `ReadPathSpikeBindData` carrying the rc.

3. **Probe 2 deliberately omitted.** The C-API path (`duckdb_connect(db_handle)`) at this lifecycle phase already has a conclusive result: `BIND-THREAD-RC1` from `65-02-SPIKES.md §A6-bind`. Re-running it via a stashed `db_handle` would be subject to the same "stale pointer / wrong DatabaseWrapper" caveat called out in Option B spike Probe 2's interpretation, and would add no new information beyond what §A6-bind already pinned. The novel question this spike answers is specifically whether the C++ direct path (Option B spike Probe 1's successful mechanism) generalises from the plan thread to the bind thread.

4. Registration is wired by adding one line to the existing `sv_register_parser_hooks` C++ entry point (called from `src/lib.rs::init_extension`):

   ```cpp
   sv_register_read_path_spike(db);
   ```

   No Rust changes required — the spike is purely additive on the C++ side.

5. Test driver at `$TMPDIR/65_read_path_spike.test`:

   ```sql
   require semantic_views
   statement ok
   LOAD semantic_views;
   query I
   SELECT connect_rc FROM __sv_read_path_spike();
   ----
   0
   ```

6. Built via `just build` (cargo `--features extension` + cdylib pack). Build completed cleanly in ~1m38s; build log at `$TMPDIR/65_read_path_spike_build.log` (cleaned up after success).

7. Ran via `timeout 30 ./configure/venv/bin/python3 -u -m duckdb_sqllogictest --test-dir test/sql --file-list <(echo $TMPDIR/65_read_path_spike.test) --external-extension build/debug/semantic_views.duckdb_extension` with stderr captured to `$TMPDIR/65_read_path_spike.log`. Process exited cleanly (exit code 0); no deadlock; the query assertion (`connect_rc = 0`) passed.

**Result (conclusion line):** `READ-BIND-RC0`

Probe 1 — `Connection(*context.db)` from inside the read-side bind callback — SUCCEEDED. Both ctor and dtor completed without throwing or deadlocking, on every one of the three bind invocations sqllogictest triggers (one per planning/execution phase that runs through `Binder::Bind(TableFunctionRef&)` for a single user query; matches the 3× pattern observed in `BIND-THREAD-RC1`'s `§A6-bind` evidence).

**Verbatim stderr from the spike run:**

```
[1/1] /tmp/claude-501/65_read_path_spike.test
[READ-SPIKE] __sv_read_path_spike registered
[READ-SPIKE] bind entered
[READ-SPIKE] probe 1: Connection ctor on *context.db
[READ-SPIKE] probe 1: Connection ctor succeeded
[READ-SPIKE] probe 1: post-scope (dtor completed if rc=0)
[READ-SPIKE] bind returning rc=0
[READ-SPIKE] bind entered
[READ-SPIKE] probe 1: Connection ctor on *context.db
[READ-SPIKE] probe 1: Connection ctor succeeded
[READ-SPIKE] probe 1: post-scope (dtor completed if rc=0)
[READ-SPIKE] bind returning rc=0
[READ-SPIKE] bind entered
[READ-SPIKE] probe 1: Connection ctor on *context.db
[READ-SPIKE] probe 1: Connection ctor succeeded
[READ-SPIKE] probe 1: post-scope (dtor completed if rc=0)
[READ-SPIKE] bind returning rc=0
SUCCESS
```

(No hang. No lldb backtrace required — every bind returned synchronously and the sqllogictest exited cleanly.)

**Interpretation:**

The literal answer to the spike's framing question is **yes**: `Connection(*context.db)` succeeds from inside a read-side table-function bind callback, just as it does from `sv_plan_function` (Option B spike Probe 1). On three independent bind invocations against the same `DatabaseInstance` mid-`Binder::Bind(TableFunctionRef&)`, the ctor (which calls `ConnectionManager::AddConnection`, acquiring `connections_lock`) and the matching dtor (which calls `RemoveConnection`, re-acquiring `connections_lock`) both completed cleanly with no throws and no deadlock.

Combined with the Option B spike's `PLAN-THREAD-RC0` Probe 1 result, this gives empirical evidence that the C++ direct path works at BOTH lifecycle phases the extension cares about for per-call connection acquisition:

| Lifecycle phase             | C-API `duckdb_connect(db_handle)`            | C++ `Connection(*context.db)`                    |
|-----------------------------|----------------------------------------------|--------------------------------------------------|
| Parse thread                | rc=1 (D-10)                                  | not yet tested — `context.db` not in scope       |
| Bind thread (table-fn bind) | rc=1 (`BIND-THREAD-RC1`, §A6-bind)           | **rc=0 (this spike)**                            |
| Plan thread (plan_function) | rc=1 (`PLAN-THREAD-RC1`, Option B spike #2)  | rc=0 (Option B spike #1)                         |

The C++ direct path against the live `context.db` works at every lifecycle phase where `ClientContext &` is reachable. The C-API wrapper path fails at every lifecycle phase the extension has been able to probe. The split is consistent: the failure is specific to the C-API wrapper's `reinterpret_cast<DatabaseWrapper *>(db_handle->internal_ptr)` + `Connection(*wrapper->database)` path (as analysed in `65-OPTION-B-SPIKE.md` Interpretation), not to any fundamental binder/parser/planner lock conflict.

**Architectural implication — does B-prime extend uniformly to read+write paths?**

Yes — on the connection-open side. The same per-call mechanism (`Connection(*context.db)` from inside a callback that receives `ClientContext &`) is viable for:

- the write path (CREATE / DROP / ALTER SEMANTIC VIEW via `plan_function` — Option B spike), AND
- the read path (`list_semantic_views()` + 13 sibling table functions + 2 scalars via their respective bind callbacks — this spike).

A unified Phase 65 architecture is therefore possible: per-call C++ `Connection(*context.db)` at every site that today reaches for a long-lived extension-owned `duckdb_connection`, with `OverrideContext::db_handle` and `query_conn` both retired. This is the "Option B-prime applied uniformly" architecture; it's the simplest possible shape that doesn't regress transactional DDL and doesn't keep a connection alive past the caller's `close()`.

**Architectural implication — production-refactor cost for the 14+2 read-side functions:**

This is the load-bearing trade-off. Today the extension registers all 14 read-side table functions + 2 scalars via duckdb-rs's `register_table_function_with_extra_info` / `register_scalar_function_with_state`. duckdb-rs marshals `ClientContext &` away — its bind/exec callbacks see only `BindInfo` / `FunctionInfo` (Plan 01 Spike A6 confirmed neither exposes `db_handle` or `ClientContext`). So to consume B-prime on the read path, the production refactor MUST one of:

1. **Re-register the 14+2 read-side functions directly through the C++ Catalog API** (the pattern this spike used) instead of through duckdb-rs. Move the bind / init / function bodies into C++ (or keep them in Rust behind a thin FFI shim that exposes `ClientContext &` as an opaque pointer to a Rust helper). This is a meaningful refactor — each of the 14 `VTab` impls (`src/ddl/list.rs`, `src/ddl/describe.rs`, `src/ddl/show_*.rs`, `src/ddl/materializations.rs`, etc.) plus the 2 scalars (`get_ddl`, `read_yaml_from_semantic_view`) would need its registration point moved from `con.register_table_function_with_extra_info` to a new C++ shim that wires them through `system_catalog.CreateTableFunction` / `CreateFunction`. The function bodies themselves can stay in Rust — only the registration plumbing changes. Estimate: order of magnitude similar to the original Phase 58 `parser_override` C++ wiring (~150 LOC of new C++ shim plus an FFI surface for each callback).

2. **OR: change duckdb-rs upstream to expose `ClientContext &` (or at least `DatabaseInstance &`) through `BindInfo`** — would be the clean fix but is out of this extension's control and would only land in a future duckdb-rs release.

3. **OR: hybrid — keep duckdb-rs registration for the bodies but inject `db_handle` via a stashed `OnceLock<usize>` (as `BIND-THREAD-RC1` §A6-bind tried) and use it to derive a `DatabaseInstance &` inside the bind closure via a C++ FFI helper.** This is the cheapest refactor on the Rust side (no registration changes) but introduces the same "stale handle" risk as Option B spike Probe 2 — the stashed `duckdb_database` is the init-time one, not necessarily the same `DatabaseInstance` the request is actually running against. Needs a follow-up spike to test whether deriving `Connection(*wrapper->database->instance)` from a stashed `db_handle` AT BIND TIME (rather than at init time) succeeds — a different test than this spike, which uses the live `context.db` instead.

The planner should weigh option 1 (correctness + structural cleanliness, higher LOC) against option 3 (lower LOC, residual stale-handle risk) against waiting on option 2. This spike's positive result on option 1 means option 1 is no longer hypothetical — it is empirically known to work on this DuckDB version. The new evidence shifts the decision from "is B-prime even possible on the read path?" (this spike: yes) to "what is the cleanest refactor shape for landing it in v0.9.1?" (planner's call).

**One additional finding:**

Probe 1's success on three consecutive bind invocations (one per planning/execution phase) confirms `ConnectionManager::AddConnection` + `RemoveConnection` do NOT deadlock against any lock held by the binder during table-function bind — symmetric to Option B spike Probe 1's finding for plan-time. The bind/plan locks the binder holds are NOT the same as `connections_lock`. This generalises the Option B spike's "the deadlock observed in §A2 is specifically about re-entering `ClientContext::Query`, not about acquiring `connections_lock`" conclusion to the bind thread as well.

**Spike artefacts reverted:** `git diff --stat cpp/src/shim.cpp src/lib.rs src/ddl/list.rs src/parse.rs` returns empty after `git checkout cpp/src/shim.cpp` (only `cpp/src/shim.cpp` was modified for this spike; the other three files were untouched). The scratch test driver at `$TMPDIR/65_read_path_spike.test`, build log at `$TMPDIR/65_read_path_spike_build.log`, and run log at `$TMPDIR/65_read_path_spike.log` were removed. Only this `65-READ-PATH-SPIKE.md` is committed from this task. No modifications to STATE.md, ROADMAP.md, plan files, or any other planning artifact. The broken `just test-sql` baseline (4/47 PASS from Plan 02 partial state, preserved per D-12) is unchanged.
