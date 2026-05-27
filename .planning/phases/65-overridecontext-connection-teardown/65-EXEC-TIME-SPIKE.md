# Phase 65 — Exec-Time `context.Query` Spike Evidence

**Spike:** Option A from `65-03-SUMMARY.md` — run `context.Query(native_sql)` inside a TableFunction's `func` (execute-time) callback, where the TableFunction is the one returned by `sv_plan_function` inside a `ParserExtensionPlanResult`.

**Goal:** Determine whether Option (i) from `65-03-SUMMARY.md` ("build a `TableFunction` whose `func` runs `context.Query(native_sql)` at execute time — `context_lock` is released by then per the A2 spike post-mortem") is viable. Specifically: does the rewritten INSERT issued via `context.Query` at TableFunction exec time participate in the caller's `BEGIN`/`ROLLBACK`/`COMMIT` (D-20 transactional DDL semantics), and does it terminate at all (no deadlock against `ClientContext::context_lock`)?

This spike sits in the lifecycle-phase grid one step deeper than the A2 spike (`65-02-SPIKES.md` `A2-DEADLOCK`): A2 ruled out `context.Query` at *plan* time; this spike asks the same question at *exec* time, on the optimistic assumption recorded in `65-03-SUMMARY.md` ("the A2 deadlock is specifically about plan-time re-entry; exec-time runs AFTER `Binder::Bind` returns and `context_lock` has been released").

---

## EXEC-TIME

**Question (CONTEXT.md D-11 / `65-03-SUMMARY.md` Option (i) / RESEARCH §16.6 #2 — exec-time variant):** When a `ParserExtensionPlanResult` returned by `sv_plan_function` carries a `TableFunction` whose `func` callback invokes `context.Query(native_sql)`, does the embedded `Query`:

1. **Terminate** (no `ClientContext::context_lock` self-deadlock — the optimistic premise that the lock is dropped by exec time)?
2. **Participate in the caller's transaction** (so that `BEGIN; <call sentinel TF that runs INSERT>; ROLLBACK;` removes the row)?

If both: Plan 03 can be revised around Option (i). If either fails: Option (i) is dead and a different mechanism is required.

---

### API contract verification

Before running the spike, re-read the live `Binder::Bind(ExtensionStatement &)` to confirm `65-03-SUMMARY.md`'s claim that the binder always binds `parse_result.function` as a `LogicalGet` table scan (and does NOT re-parse `native_sql`):

`cpp/include/duckdb.cpp:369065-369085`:

```cpp
BoundStatement Binder::Bind(ExtensionStatement &stmt) {
    // perform the planning of the function
    D_ASSERT(stmt.extension.plan_function);
    auto parse_result =
        stmt.extension.plan_function(stmt.extension.parser_info.get(), context, std::move(stmt.parse_data));

    auto &properties = GetStatementProperties();
    properties.modified_databases = parse_result.modified_databases;
    properties.requires_valid_transaction = parse_result.requires_valid_transaction;
    properties.return_type = parse_result.return_type;

    // create the plan as a scan of the given table function
    auto result = BindTableFunction(parse_result.function, std::move(parse_result.parameters));
    D_ASSERT(result.plan->type == LogicalOperatorType::LOGICAL_GET);
    ...
}
```

**Confirmed.** The halt-state's reading is exact: `Binder::Bind(ExtensionStatement &)` always builds `LogicalGet` from `parse_result.function`. There is no branch that extracts SQL statements from the `ParserExtensionPlanResult` to re-dispatch through the native binder.

Then check `ExecuteTaskInternal` (the pipeline driver that fires TableFunction `func` callbacks) for whether `context_lock` is still held during pipeline execution:

`cpp/include/duckdb.cpp:273110-273151`:

```cpp
PendingExecutionResult ClientContext::ExecuteTaskInternal(ClientContextLock &lock, BaseQueryResult &result,
                                                          bool dry_run) {
    ...
    auto query_result = active_query->executor->ExecuteTask(dry_run);
    ...
}
```

The function takes `ClientContextLock &lock` by reference — i.e. it executes pipeline tasks (including TableFunction `func` invocations) *with the lock still held*. The lock is unlocked only when the outer `unique_ptr<ClientContextLock>` returned by `LockContext()` goes out of scope — which happens *after* the entire `Query`/`Execute` call returns to the caller.

`ClientContext::Query`'s own implementation at `cpp/include/duckdb.cpp:273503-273566`:

```cpp
unique_ptr<QueryResult> ClientContext::Query(const string &query, QueryParameters query_parameters) {
    auto lock = LockContext();   // <-- acquires context_lock
    ...
}
```

So a `context.Query(native_sql)` from inside a TableFunction `func` would call `LockContext()` *while the outer `lock` from the enclosing `Query` is still alive* — re-entering a non-recursive `std::mutex`. **Predicted outcome: deadlock at the inner `LockContext` call, identical in shape to the A2-DEADLOCK pattern but one lifecycle phase later.**

The halt-state SUMMARY's optimistic note ("`context_lock` is released by then per the A2 spike post-mortem") is therefore **wrong**: the A2 post-mortem said specifically that `connections_lock` is not the same lock as `context_lock`; it did NOT claim `context_lock` is released during pipeline execution. `context_lock` is held throughout the lifetime of the outer `ClientContext::Query` (or `ClientContext::Execute`) call, including the pipeline-execution phase that runs TableFunction `func` callbacks.

Run the spike anyway to convert the prediction into empirical evidence (the existing planner did not catch this on close reading, so the empirical run is what guarantees the conclusion).

---

### Spike scaffold

All scratch code was added to `cpp/src/shim.cpp` and `test/sql/65_exec_time_spike.test`, wrapped in `// SPIKE-EXEC-TIME-65-03 — REVERT BEFORE COMMIT` markers, and reverted before this SPIKE.md was committed. Pasting the load-bearing pieces for the record:

**1. Sentinel detection in `sv_parse_stub`:** if the query starts with `SPIKE_EXEC_TIME_INJECT__`, return `PARSE_SUCCESSFUL` with the trailing SQL string stashed on `SemanticViewParseData`.

```cpp
static constexpr const char *kExecTimeSpikePrefix = "SPIKE_EXEC_TIME_INJECT__";
...
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *info, const string &query) {
    {
        size_t prefix_len = std::strlen(kExecTimeSpikePrefix);
        if (query.size() > prefix_len &&
            query.compare(0, prefix_len, kExecTimeSpikePrefix) == 0) {
            string payload = query.substr(prefix_len);
            // strip trailing ';', '\n', ' '
            ...
            return ParserExtensionParseResult(
                make_uniq_base<ParserExtensionParseData,
                               SemanticViewParseData>(payload));
        }
    }
    /* fall through to existing rc-handling logic */
}
```

**2. Replacement `plan_function` (was `sv_plan_unreachable`):** returns a `ParserExtensionPlanResult` whose `TableFunction.func` runs `context.Query(native_sql)`. The `native_sql` is plumbed through `parameters`/`bind_data` so the TableFunction sees it at exec time.

```cpp
struct ExecTimeSpikeBindData : public TableFunctionData {
    string native_sql;
};
struct ExecTimeSpikeGlobalState : public GlobalTableFunctionState {
    bool emitted = false;
};
static unique_ptr<FunctionData> sv_exec_time_spike_bind(
    ClientContext &, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    auto bind_data = make_uniq<ExecTimeSpikeBindData>();
    bind_data->native_sql = input.inputs[0].GetValue<string>();
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("sentinel");
    return std::move(bind_data);
}
static unique_ptr<GlobalTableFunctionState> sv_exec_time_spike_init_global(
    ClientContext &, TableFunctionInitInput &) {
    return make_uniq<ExecTimeSpikeGlobalState>();
}
static void sv_exec_time_spike_function(
    ClientContext &context, TableFunctionInput &data_p, DataChunk &output) {
    auto &bind_data = data_p.bind_data->Cast<ExecTimeSpikeBindData>();
    auto &gstate = data_p.global_state->Cast<ExecTimeSpikeGlobalState>();
    if (gstate.emitted) { output.SetCardinality(0); return; }
    fprintf(stderr, "[EXEC-SPIKE] func entered (exec-time)\n");
    fprintf(stderr, "[EXEC-SPIKE] func: about to call context.Query(\"%s\")\n",
            bind_data.native_sql.c_str());
    auto result = context.Query(bind_data.native_sql, false);   // <-- THE PROBE
    if (result->HasError()) {
        throw IOException("exec-time spike: context.Query failed: " +
                          result->GetError());
    }
    output.SetValue(0, 0, Value("exec_time_spike_ok"));
    output.SetCardinality(1);
    gstate.emitted = true;
}
static ParserExtensionPlanResult sv_exec_time_spike_plan(
    ParserExtensionInfo *, ClientContext &,
    unique_ptr<ParserExtensionParseData> parse_data) {
    auto *spike_data = static_cast<SemanticViewParseData *>(parse_data.get());
    TableFunction tf("__sv_exec_time_spike", {LogicalType::VARCHAR},
                     sv_exec_time_spike_function, sv_exec_time_spike_bind,
                     sv_exec_time_spike_init_global);
    ParserExtensionPlanResult result;
    result.function = tf;
    result.parameters.push_back(Value(spike_data->query));
    result.requires_valid_transaction = false;
    result.return_type = StatementReturnType::QUERY_RESULT;
    return result;
}
```

**3. Wiring:** `ext.plan_function = sv_exec_time_spike_plan;` (replaced `sv_plan_unreachable`) in `sv_register_parser_hooks`. `parse_function`, `parser_override`, `parser_info` all unchanged.

**4. Test driver** (`test/sql/65_exec_time_spike.test`): see Methodology below.

No Rust-side changes. The full scaffold lived entirely in `cpp/src/shim.cpp` + the scratch sqllogictest file.

---

### Test methodology

The test driver runs three independent probes and asserts the expected outcome of each:

```sql
require semantic_views
statement ok
LOAD semantic_views;

# Smoke 1: trivial SELECT through the sentinel
query T
SPIKE_EXEC_TIME_INJECT__SELECT 1
----
exec_time_spike_ok

# Smoke 2: autocommit-ON INSERT through the sentinel
statement ok
SPIKE_EXEC_TIME_INJECT__INSERT INTO semantic_layer._definitions (name, definition) VALUES ('smoke_view', 'smoke');

# Probe A: BEGIN ... INSERT-via-sentinel ... ROLLBACK ... assert row absent
statement ok
BEGIN;
statement ok
SPIKE_EXEC_TIME_INJECT__INSERT INTO semantic_layer._definitions (name, definition) VALUES ('spike_rollback_view', 'spike: rollback');
query I
SELECT COUNT(*) FROM semantic_layer._definitions WHERE name = 'spike_rollback_view';
----
1
statement ok
ROLLBACK;
query I
SELECT COUNT(*) FROM semantic_layer._definitions WHERE name = 'spike_rollback_view';
----
0

# Probe B: BEGIN ... INSERT-via-sentinel ... COMMIT ... assert row present
statement ok
BEGIN;
statement ok
SPIKE_EXEC_TIME_INJECT__INSERT INTO semantic_layer._definitions (name, definition) VALUES ('spike_commit_view', 'spike: commit');
statement ok
COMMIT;
query I
SELECT COUNT(*) FROM semantic_layer._definitions WHERE name = 'spike_commit_view';
----
1
```

**Why the seed step differs from the spike protocol description.** The original protocol suggested seeding the catalog by running production `CREATE SEMANTIC VIEW seed_view AS ...` to ensure `semantic_layer._definitions` exists. This was attempted but failed on the first run with `Parser Error: catalog connection failed: duckdb_connect failed (rc=1)` — the production `parser_override` path hits the known-broken `ConnGuard::open(db_handle)` baseline inherited from Plan 02 partial (the same D-10 / PLAN-THREAD-RC1 / BIND-THREAD-RC1 failure mode that B-prime is trying to retire). The workaround: skip the production DDL and rely on `LOAD semantic_views;` having materialised the table via `init_catalog` (`src/catalog.rs:35-41`), which uses the high-level rusqlite `Connection` (the caller's, NOT a `ConnGuard`). The schema is `(name VARCHAR PRIMARY KEY, definition VARCHAR)`.

The smoke probes verified the sentinel infrastructure is wired correctly before attempting the transactional-DDL test:

- **Smoke 1** (trivial SELECT): expected to succeed. If it hangs, the deadlock is global and even read-only `context.Query` from exec time is dead — no point running Probe A or B.
- **Smoke 2** (autocommit-ON INSERT): expected to succeed. If it errors, the sentinel's INSERT can't reach the catalog at all, and Probe A/B would be uninterpretable.

**Decision rule for the test outcome:**

| Outcome of Smoke 1 | Verdict |
|--------------------|---------|
| Deadlock (sqllogictest hits external timeout) | **EXEC-TIME-RC1**: Option A dead. context.Query from exec time deadlocks just like plan time. |
| Returns "exec_time_spike_ok" | Continue to Probe A. |
| Returns an error | Surface as anomalous; describe in this SPIKE.md. |

| Outcome of Probe A (rollback test) | Outcome of Probe B (commit test) | Verdict |
|-----------------------------------|----------------------------------|---------|
| Row absent after ROLLBACK | Row present after COMMIT | **EXEC-TIME-RC0**: Option A viable. Plan 03 can replan around it. |
| Row PRESENT after ROLLBACK (autocommit semantics) | Row present after COMMIT | **EXEC-TIME-RC1** (variant): no deadlock, but `context.Query` runs on a different/autocommit conn → D-20 violation. |
| Deadlock at Probe A | n/a | **EXEC-TIME-RC1** (variant): the `BEGIN` apparently changes the lock state such that the trivial Smoke 1 worked but a transactional INSERT does not. |

---

### Empirical outcomes

`just build` reproduced the scaffolded shim in 1m 53s with no warnings. The test driver was invoked exactly as the prior spikes were:

```bash
timeout 30 ./configure/venv/bin/python3 -u -m duckdb_sqllogictest \
  --test-dir test/sql \
  --file-list <(echo test/sql/65_exec_time_spike.test) \
  --external-extension build/debug/semantic_views.duckdb_extension
```

**Outcome: external `timeout 30` fired with `exit=124` (timeout, SIGTERM).** The stderr trace through Smoke 1:

```
[1/1] test/sql/65_exec_time_spike.test
[EXEC-SPIKE] sv_parse_stub: sentinel detected, payload="SELECT 1"
[EXEC-SPIKE] sv_parse_stub: sentinel detected, payload="SELECT 1"
[EXEC-SPIKE] plan_function entered
[EXEC-SPIKE] plan_function: native_sql="SELECT 1"
[EXEC-SPIKE] plan_function: returning ParserExtensionPlanResult
[EXEC-SPIKE] func entered (exec-time)
[EXEC-SPIKE] func: about to call context.Query("SELECT 1")
```

The "[EXEC-SPIKE] func: context.Query returned OK" trace that should follow the `about to call` line never appeared — `context.Query("SELECT 1")` did not return within 30 seconds.

**Conclusion line:** **`EXEC-TIME-RC1`**

(The `sv_parse_stub` lines fire twice because DuckDB's parser dispatch tries the default parser first — fails on the unrecognised verb — then calls `parse_function` once; the duplicate appears to be a sqllogictest internal effect, the same way the read-path spike saw 3× bind invocations per query. Either way, only one `plan_function entered` line fires, confirming the dispatch flowed through correctly to exactly one TableFunction.func invocation.)

**LLDB backtrace** (from a second run with `lldb -p <pid>` attaching after `sleep 8` while the process was hung; relevant frames only, threads #2-#15 omitted as they were all `semaphore_wait_trap` worker-thread idles):

```
* thread #1, queue = 'com.apple.main-thread'
  * frame #0: __psynch_mutexwait + 8
    frame #1: _pthread_mutex_firstfit_lock_wait + 84
    frame #2: _pthread_mutex_firstfit_lock_slow + 220
    frame #3: std::__1::mutex::lock() + 16
    frame #4: std::__1::lock_guard<std::__1::mutex>::lock_guard
              at lock_guard.h:33:10
    frame #5: std::__1::lock_guard<std::__1::mutex>::lock_guard
              at lock_guard.h:32:19
    frame #6: duckdb::ClientContextLock::ClientContextLock(
                this=…, context_lock=0x00000001326ec2c8)
              at duckdb.hpp:41818:52
    frame #7: duckdb::ClientContextLock::ClientContextLock(
                this=…, context_lock=0x00000001326ec2c8)
              at duckdb.hpp:41818:79
    frame #8: duckdb::TemplatedUniqueIf<duckdb::ClientContextLock,true>::
              templated_unique_single_t
              duckdb::make_uniq<duckdb::ClientContextLock, std::__1::mutex&>(
                args=0x00000001326ec2c8)
              at duckdb.hpp:2005:76
    frame #9: duckdb::ClientContext::LockContext(this=0x00000001326ec148)
              at duckdb.cpp:272659:9
    frame #10: duckdb::ClientContext::Query(
                this=0x00000001326ec148,
                query="SELECT 1",
                query_parameters=(output_type = FORCE_MATERIALIZED,
                                  memory_type = IN_MEMORY))
              at duckdb.cpp:273504:14
    frame #11: sv_exec_time_spike_function(
                context=0x00000001326ec148,
                data_p=0x000000016ba98c88,
                output=0x0000000152732050)
              at shim.cpp:303:27
    frame #12-40: [_duckdb.cpython pipeline / python-binding / interpreter frames]
```

The deadlock site is identical in shape to the A2-DEADLOCK pattern documented in `65-02-SPIKES.md`:

| Frame | A2 (plan-time) | EXEC-TIME (this spike) |
|-------|----------------|------------------------|
| std::mutex::lock | ✓ | ✓ |
| lock_guard ctor | ✓ | ✓ |
| ClientContextLock ctor | ✓ at `duckdb.hpp:41818:52` | ✓ at `duckdb.hpp:41818:52` |
| make_uniq<ClientContextLock> | ✓ at `duckdb.hpp:2005:76` | ✓ at `duckdb.hpp:2005:76` |
| ClientContext::LockContext | ✓ at `duckdb.cpp:272659:9` | ✓ at `duckdb.cpp:272659:9` |
| ClientContext::Query | ✓ at `duckdb.cpp:273504:14` (`"SELECT 42 AS spike"`) | ✓ at `duckdb.cpp:273504:14` (`"SELECT 1"`) |
| extension callsite | `sv_plan_unreachable at shim.cpp:385:27` (PLAN time) | `sv_exec_time_spike_function at shim.cpp:303:27` (EXEC time) |

The lifecycle phase moved from plan time to exec time; the lock that is held by the *outer* `ClientContext::Query` (which is what dispatched the user's `SPIKE_EXEC_TIME_INJECT__SELECT 1` statement) is the *same* `std::mutex` the inner `context.Query("SELECT 1")` from the TableFunction's `func` is now blocking on. `std::mutex` is non-recursive; the lock is held throughout the lifetime of the outer `unique_ptr<ClientContextLock>` returned from `LockContext()` — which spans the entire pipeline-execution phase (including `func` callbacks), not just the parse/bind/plan phases.

---

### Verdict — **EXEC-TIME-RC1**

Option A from `65-03-SUMMARY.md` is **dead**. `context.Query(native_sql)` from inside a TableFunction's `func` callback (exec time, post-`Binder::Bind(ExtensionStatement &)`) self-deadlocks on `ClientContext::context_lock`, exactly the same way A2-DEADLOCK self-deadlocked at plan time. The lifecycle-phase grid for `context.Query` against the caller's `ClientContext` is now complete:

| Lifecycle phase                   | `context.Query` outcome              |
|-----------------------------------|--------------------------------------|
| Parse thread                      | not applicable — no `ClientContext &` |
| Plan thread (`plan_function`)     | **A2-DEADLOCK** (`65-02-SPIKES.md`) |
| Bind thread (table-fn bind)       | not yet probed (same `context_lock` held; would deadlock identically — extrapolated) |
| Exec thread (TableFunction `func`)| **EXEC-TIME-RC1** (this spike)      |

Every lifecycle phase where `ClientContext &` is reachable holds `context_lock`. `context.Query` is structurally incompatible with re-entry from any extension callback against the caller's `ClientContext`.

(Footnote: `Connection probe(*context.db); probe.Query(native_sql);` would NOT deadlock — it builds a fresh `ClientContext` with its own `context_lock`, validated empirically by Option B spike Probe 1 + the read-path spike. But a fresh `Connection` has its own transaction state — it does NOT participate in the caller's `BEGIN`/`COMMIT`. So Connection-based execution preserves liveness but loses D-20. This is the fundamental dichotomy the v0.9.1 architecture must reckon with.)

---

### Implications for Plan 03

`65-03-SUMMARY.md` identified three readings of RESEARCH §16.4's "drives the rewritten INSERT/DELETE through the binder onto the caller's conn" prescription. This spike resolves them empirically:

- **Option (i)** — TableFunction whose `func` runs `context.Query(native_sql)` at exec time. **DEAD** (this spike: `EXEC-TIME-RC1`). The optimistic claim in `65-03-SUMMARY.md` that `context_lock` is released by exec time is structurally wrong; the lock is held throughout `ClientContext::Query`'s entire scope, including pipeline execution. `ClientContext::ExecuteTaskInternal(ClientContextLock &lock, ...)` takes the lock by reference precisely because the lock outlives pipeline execution.
- **Option (ii)** — TableFunction whose `bind` re-parses `native_sql` and returns SQLStatements back to the binder. **NON-EXISTENT** (confirmed `65-03-SUMMARY.md` — no such API in `TableFunction` / `TableFunctionBindInput`).
- **Option (iii)** — `sv_plan_function` returning `unique_ptr<TableRef>`. **NON-EXISTENT** (confirmed `65-03-SUMMARY.md` — wrong return type vs `plan_function_t` typedef in `parser_extension_compat.hpp:121-122`).

**Plan 03 as written cannot be revived under the `parse_function` + `plan_function` mechanism on DuckDB v1.5.2.** The four mechanisms by which `ParserExtensionPlanResult.function` could carry a write-side effect to the caller's connection have all been definitively eliminated:

1. ❌ `context.Query` from plan time (A2-DEADLOCK)
2. ❌ `context.Query` from bind time of the inner TableFunction (extrapolated from same lock; uninteresting to spike further)
3. ❌ `context.Query` from exec time of the inner TableFunction (this spike, `EXEC-TIME-RC1`)
4. ❌ `Connection(*context.db).Query` at any lifecycle phase — would work liveness-wise but uses a fresh transaction (D-20 violation)

The user must now pick from the options surfaced in `65-03-SUMMARY.md`'s "What's Needed To Resolve":

- **Option C — Accept the v0.8.0 baseline (defer v0.9.1)** — wait for DuckDB upstream to expose either (a) a `Binder::Bind(ExtensionStatement &)` path that extracts SQLStatements from `ParserExtensionPlanResult` and re-dispatches them through the native binder, (b) a per-`Connection` weak handle that survives B-prime's per-call `ConnGuard` pattern, or (c) a recursive `context_lock` / explicit pipeline-execution lock relinquishment API. None are present in v1.5.2.
- **Option D — Adopt a `StorageExtension` + `ATTACH` model** (large architectural change; CONTEXT.md D-14 places it in v0.10.0 territory). Off the table for v0.9.1 unless the user re-opens scope.
- **Re-research via `/gsd-discuss-phase --assumptions`** with the specific framing: "given that `context.Query` deadlocks at every lifecycle phase where `ClientContext &` is reachable, what mechanism preserves D-20 transactional DDL while running catalog reads on `Connection(*context.db)` from inside `sv_plan_function`?"

The architectural concern surfaced in `65-03-SUMMARY.md` is now no longer a hypothesis — it is empirically pinned. The B-prime architecture as specified relies on a mechanism that does not exist in DuckDB v1.5.2 for the write-path execution side.

**Reading the Connection-based path again, more carefully:** even though `Connection(*context.db).Query(native_sql)` would not deadlock, it constructs a fresh `ClientContext` with its own transaction state. The caller's `BEGIN`/`COMMIT` does not propagate; any catalog mutation issued via `Connection(*context.db)` runs in its own auto-managed subtransaction. This is exactly the failure mode `feedback-transactional-ddl-non-negotiable` forbids ("never accept a mechanism that runs the catalog write on a different connection than the caller's, even with TECH-DEBT documentation"). So the Connection-based path doesn't rescue Plan 03 either.

The mechanism shown by `cpp/src/shim.cpp::sv_parser_override` at v0.8.0 — `ParserOverrideResult(vector<unique_ptr<SQLStatement>>)` produced by re-parsing `native_sql` through `Parser::ParseQuery` and returned to the caller's parser dispatch — remains the **only** DuckDB v1.5.2 API surface that delivers transactional INSERT/DELETE on the caller's connection. But `parser_override` does not receive `ClientContext &`, so it cannot use the C++ `Connection(*context.db)` mechanism that B-prime relies on for catalog reads. The v0.8.0 path is the only one with transactional DDL; the B-prime read path is the only one with per-call connections. **No single DuckDB v1.5.2 mechanism provides both.** This is the hard architectural blocker now backed by complete empirical evidence.

---

### Caveats / blast radius

1. **The deadlock fires within ~milliseconds of the `func` callback being entered.** No race conditions, no ordering ambiguity, no platform-specific reproduction concerns. `std::mutex::lock` is straightforward: it sees the same mutex it already owns and blocks. Reproduces identically across runs.
2. **The deadlock is `pthread_mutex_firstfit_lock_wait`, not a spurious resource starvation.** All worker threads are idle (`semaphore_wait_trap`), the executor is parked on `pthread_mutex_firstfit_lock_wait` at the inner `ClientContextLock` ctor, and the outer `Query` frame is still on the stack as proven by frame #10 → frame #11 → frame #11's caller chain. This is unambiguous self-deadlock.
3. **Smoke 1 used `SELECT 1` — a read-only statement.** No transaction-coupling tricks could explain the deadlock; even a pure read deadlocks because the lock is held independent of statement type. No further probe (Smoke 2, Probe A, Probe B) is informative — they would all deadlock identically.
4. **The `requires_valid_transaction = false` + empty `modified_databases` configuration was a workaround for an earlier Invalid Input Error (`Database "" not found`)** seen on the first build, before reverting to the minimal shape. The deadlock occurs regardless of that field configuration — `ClientContext::Query`'s `LockContext` call is unconditional and fires before `properties.modified_databases` is even read by the inner statement's planner.
5. **The error_location / caret rendering surface (Phase 62) is untouched by this spike** — `sv_parse_stub` continues to dispatch through the existing rc-handling logic when the sentinel prefix doesn't match. The spike's only mutation to `sv_parse_stub` was a leading short-circuit for the literal `SPIKE_EXEC_TIME_INJECT__` prefix. Caret tests remain at the same pre-spike baseline (broken — known from Plan 02 partial; not regressed further).
6. **Build artefacts and reverted state.** `git diff --stat cpp/src/shim.cpp src/parse.rs src/lib.rs test/sql/` returns empty after `git checkout cpp/src/shim.cpp` (the test file was scratch and was deleted via `rm`). A final `just build` post-revert succeeded cleanly in ~1m. The broken `just test-sql` baseline (4/47 PASS from Plan 02 partial, preserved per D-12) is unchanged. The previously-committed `.planning/STATE.md` modification visible in `git status --short` is unrelated to this spike (pre-existing user edit before this agent's session).

---

### Self-Check: PASSED

Verified post-write (before final commit):

- `.planning/phases/65-overridecontext-connection-teardown/65-EXEC-TIME-SPIKE.md` — present (this file, about to be committed)
- `git diff --stat cpp/src/shim.cpp src/parse.rs src/lib.rs test/sql/` — empty (no production code changes after revert)
- `test/sql/65_exec_time_spike.test` — not present (scratch file removed)
- `git status --short` shows only this new SPIKE.md among planning-tracked files (the `.planning/STATE.md M` line and `.DS_Store` / `.cache/` `??` lines are pre-existing and unrelated)
- Post-revert `just build` — PASS in ~1m

---

*Phase: 65-overridecontext-connection-teardown*
*Spike: Exec-time `context.Query` from TableFunction `func` callback (Option A from `65-03-SUMMARY.md`)*
*Outcome: `EXEC-TIME-RC1` — Option A dead; `context.Query` deadlocks at exec time on `context_lock`, same shape as A2-DEADLOCK at plan time*
*Date: 2026-05-23*
