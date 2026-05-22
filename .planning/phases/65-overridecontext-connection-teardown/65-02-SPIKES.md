# Phase 65 Plan 02 — Wave-0 Spike Evidence

**Spikes:** A2 (`plan_function` `context.Query` viability) + A6-bind (`duckdb_connect` from bind thread)

**Goal:** Pin the bind/plan-time architecture (Option A per CONTEXT.md D-11) on empirical evidence before any production refactor. Spike A2 is the discriminator between Option A1 / A2 / A3 (RESEARCH §16.2). Spike A6-bind verifies whether D-10's parse-thread rc=1 failure mode generalises to the bind thread (RESEARCH §16.6 #8) before Plan 03 mass-refactors 14+2 read-side sites.

---

## A2

**Question (RESEARCH §16.2 / §16.6 #2):** Does `ClientContext::Query(native_sql)` from inside `sv_plan_function` deadlock on `ClientContext::context_lock`, or does it execute successfully on the caller's connection inside the caller's transaction?

**Setup:**

1. Modified `cpp/src/shim.cpp::sv_parse_stub` to detect the `SPIKE_PLAN_PROBE` sentinel prefix and return `ParserExtensionParseResult(make_uniq_base<ParserExtensionParseData, SemanticViewParseData>(query))` — the success path that triggers `plan_function`.
2. Replaced `sv_plan_unreachable` with `sv_plan_function_spike` body that:
   - Probe 1: `context.Query("SELECT 42 AS spike", false)` — trivial read
   - Probe 2: `context.Query("INSERT INTO __sv_spike VALUES (1)", false)` — DML on caller's transaction
   - Probe 3: `context.Query("SELECT COUNT(*) FROM __sv_spike", false)` — read-back from caller's connection
   - Returns a trivial `ParserExtensionPlanResult` whose `TableFunction` (`__sv_spike_sentinel`) declares one VARCHAR column and emits zero rows
3. `eprintln`-style `fprintf(stderr, "[A2-SPIKE] ...")` traces between each probe so we can see which one (if any) hung.
4. Test driver: `test/sql/65_02_a2_spike.test`:

   ```sql
   require semantic_views
   statement ok
   LOAD semantic_views;
   statement ok
   CREATE TABLE __sv_spike (i INT);
   # Trigger sv_parse_stub → PARSE_SUCCESSFUL → sv_plan_function (the spike body)
   statement ok
   SPIKE_PLAN_PROBE one
   # Assert the INSERT ran on the caller's connection
   query I
   SELECT COUNT(*) FROM __sv_spike;
   ----
   1
   ```
5. Built via `just build` (cargo `--features extension` + cdylib pack).
6. Ran via `python3 -m duckdb_sqllogictest --test-dir test/sql --file-list <(echo test/sql/65_02_a2_spike.test) --external-extension build/debug/semantic_views.duckdb_extension`.

**Result (conclusion line):** `A2-DEADLOCK`

The spike hung on probe 1 (`context.Query("SELECT 42 AS spike", false)`) and never returned. After waiting >15s with the stderr trace stuck at the "probe 1: context.Query(SELECT 42)" message, lldb was attached to the hung sqllogictest worker (PID 24260). The backtrace pins the deadlock to `std::mutex::lock` on `ClientContext::context_lock`, acquired by a fresh `ClientContextLock` constructed inside `ClientContext::LockContext()` (duckdb.cpp:272659) — which is itself called from `ClientContext::Query` (duckdb.cpp:273504:14), invoked from our `sv_plan_unreachable` (the repurposed spike) at frame #11.

**Verbatim stderr from the spike before hang:**

```
[A2-SPIKE] plan_function entered for query=SPIKE_PLAN_PROBE one
[A2-SPIKE] probe 1: context.Query(SELECT 42)
```

(No further output. Probe 2 and probe 3 never fired. `[A2-SPIKE] plan_function returning sentinel TableFunction` never printed.)

**Verbatim lldb backtrace of the hung thread:**

```
(lldb) process attach --pid 24260
Process 24260 stopped
* thread #1, queue = 'com.apple.main-thread', stop reason = signal SIGSTOP
    frame #0: 0x0000000192cf489c libsystem_kernel.dylib`__psynch_mutexwait + 8
(lldb) bt
* thread #1, queue = 'com.apple.main-thread', stop reason = signal SIGSTOP
  * frame #0: 0x0000000192cf489c libsystem_kernel.dylib`__psynch_mutexwait + 8
    frame #1: 0x0000000192d30e14 libsystem_pthread.dylib`_pthread_mutex_firstfit_lock_wait + 84
    frame #2: 0x0000000192d2e840 libsystem_pthread.dylib`_pthread_mutex_firstfit_lock_slow + 220
    frame #3: 0x0000000192c653dc libc++.1.dylib`std::__1::mutex::lock() + 16
    frame #4: 0x0000000118f02050 semantic_views.duckdb_extension`std::__1::lock_guard<std::__1::mutex>::lock_guard
                                                                  (this=0x0000600000ae0150,
                                                                   __m=0x000000011d711588) at lock_guard.h:33:10
    frame #5: 0x00000001181f468c semantic_views.duckdb_extension`std::__1::lock_guard<std::__1::mutex>::lock_guard
                                                                  (this=0x0000600000ae0150,
                                                                   __m=0x000000011d711588) at lock_guard.h:32:19
    frame #6: 0x000000011994a44c semantic_views.duckdb_extension`duckdb::ClientContextLock::ClientContextLock
                                                                  (this=0x0000600000ae0150,
                                                                   context_lock=0x000000011d711588) at duckdb.hpp:41818:52
    frame #7: 0x000000011994a3e4 semantic_views.duckdb_extension`duckdb::ClientContextLock::ClientContextLock
                                                                  (this=0x0000600000ae0150,
                                                                   context_lock=0x000000011d711588) at duckdb.hpp:41818:79
    frame #8: 0x00000001185fd090 semantic_views.duckdb_extension`duckdb::TemplatedUniqueIf<duckdb::ClientContextLock, true>::templated_unique_single_t
                                                                  duckdb::make_uniq<duckdb::ClientContextLock, std::__1::mutex&>
                                                                  (args=0x000000011d711588) at duckdb.hpp:2005:76
    frame #9: 0x00000001185fd058 semantic_views.duckdb_extension`duckdb::ClientContext::LockContext
                                                                  (this=0x000000011d711408) at duckdb.cpp:272659:9
    frame #10: 0x00000001186066b8 semantic_views.duckdb_extension`duckdb::ClientContext::Query
                                                                   (this=0x000000011d711408,
                                                                    query="SELECT 42 AS spike",
                                                                    query_parameters=(output_type = FORCE_MATERIALIZED,
                                                                                      memory_type = IN_MEMORY)) at duckdb.cpp:273504:14
    frame #11: 0x0000000119c92848 semantic_views.duckdb_extension`sv_plan_unreachable
                                                                   ((null)=0x0000600000af9fd0,
                                                                    context=0x000000011d711408,
                                                                    parse_data=unique_ptr<...> @ 0x000000016f2300b0) at shim.cpp:385:27
    frame #12: 0x00000001048c34e8 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol26507 + 72
    frame #13: 0x000000010494eb5c _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol27927 + 160
    frame #14: 0x00000001055ab8e8 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53746 + 544
    frame #15: 0x00000001055ac7fc _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53752 + 1048
    frame #16: 0x00000001055b0b8c _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53791 + 128
    frame #17: 0x00000001055b25ec _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53797 + 276
    frame #18: 0x00000001055b04b0 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53785 + 2024
    frame #19: 0x00000001055b1368 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53793 + 132
    frame #20: 0x00000001055b3e64 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53807 + 220
    frame #21: 0x00000001055bf794 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol53974 + 72
    frame #22: 0x0000000103daa32c _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1774 + 172
    frame #23: 0x0000000103daa95c _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1777 + 256
    frame #24: 0x0000000103dcb63c _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1998 + 68
    frame #25: 0x0000000103dcb2f0 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1995 + 100
    frame #26: 0x0000000103dcb280 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1994 + 24
    frame #27: 0x0000000103d42484 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol709 + 3240
    ...
```

**Interpretation:**

- The caller (Python's `connection.execute(...)`) has already acquired `ClientContext::context_lock` to drive the statement through `Parser::ParseQuery` → `Binder::Bind(ExtensionStatement&)` → `stmt.extension.plan_function(...)` (duckdb.cpp:369065-369069). Our `sv_plan_function` runs **with `context_lock` held**.
- Inside `sv_plan_function`, `context.Query("SELECT 42")` calls `ClientContext::Query(const string &, QueryParameters)` at duckdb.cpp:273503, whose first action is `auto lock = LockContext();` at duckdb.cpp:273504 — and `LockContext()` constructs a fresh `ClientContextLock` whose constructor does `lock_guard<std::mutex>` on the same `context_lock` (duckdb.cpp:272659:9 → duckdb.hpp:41818:52).
- `std::mutex` is NOT recursive. The second `lock_guard` blocks forever.

This **falsifies Option A2** as a viable mechanism for `sv_plan_function` on DuckDB v1.5.2. The empirical chain (frame #4 lock_guard → frame #9 LockContext → frame #10 Query → frame #11 our plan_function called from `Binder::Bind`) is unambiguous: the caller holds `context_lock` for the entire duration of plan_function, and any re-entrant `context.Query` from inside plan_function self-deadlocks.

The implication per RESEARCH §16.2 row "Option A2": the path the planner intended (preserve transactional DDL semantics by running the rewritten SQL on the caller's connection via `context.Query`) is NOT reachable through `ClientContext::Query` on this DuckDB version. A2 is dead.

**Spike artefacts reverted:** `git diff --stat cpp/src/shim.cpp src/parse.rs src/lib.rs` returns empty after revert. The scratch test file `test/sql/65_02_a2_spike.test` was also removed. Only `65-02-SPIKES.md` is committed from this task.

---

## A6-bind

**Question (RESEARCH §16.6 #8):** Does `duckdb_connect(db_handle)` from inside a read-side table-function `bind` callback (a different lifecycle phase than `parser_override` — post-parse, inside `Binder::Bind(TableFunctionRef&)`) suffer the same rc=1 failure mode that D-10 empirically pinned for the parse thread?

**Status:** **Not run** — the A2 spike returned `A2-DEADLOCK`, which per the USER_HARD_CONSTRAINT block forces a Task 1 escalation (no production refactor proceeds in Plan 02 or Plan 03 without re-research). Running A6-bind would consume a build cycle whose evidence we cannot use: the bind-thread refactor in Plan 03 is **transitively blocked** on the parse/plan-time question because Plan 02's parse_function/plan_function plumbing has no viable success-path mechanism. There is no point empirically confirming Plan 03's pattern works when Plan 02 cannot ship its predecessor commits.

**Forward direction:** A6-bind moves into the re-research / next-Plan-02 input pile. If the user picks `escalate` at Task 1, A6-bind should be run as part of `/gsd:discuss-phase --assumptions`'s investigation — preferably alongside any alternative mechanism research (Plan B candidates: rewrite via StorageExtension, or a different DuckDB-1.6+ hook surface, or accept-and-document-with-non-transactional-DDL only if and when the user explicitly approves the regression).

**Deferral rationale (per CLAUDE.md `feedback-bounded-scope-with-signal-surfacing`):** running A6-bind now would be expanding scope past the trigger condition. The bind-thread spike is small (<1 day), but small-but-pointless work still costs the milestone schedule and clutters the spike record with provisional evidence whose conclusion is moot until the parse/plan-time question reopens. Surfacing this as a deferred-but-tracked item is the correct GSD discipline.

---

## MECHANISM-CHOSEN

(Pending Task 1 `checkpoint:decision` resolution. The marker line will be appended by the executor immediately after the user signals their selection on Task 1.)

**Recommendation for Task 1 (per USER_HARD_CONSTRAINT block in the executor prompt):** with `A2-DEADLOCK` empirically pinned, the only path forward consistent with the project's non-negotiable transactional-DDL requirement is to **escalate**: halt Plan 02, surface the dead-end to the user, and re-enter `/gsd:discuss-phase --assumptions` for a new research direction. A1 and A3 are NOT to be presented as live options because both regress transactional DDL — a regression explicitly forbidden by the user's hard constraint. v0.9.1 slip is acceptable; transactional regression is not.

**Note on marker absence vs plan verify:** the plan's `<verify>` for Task 3 expects `MECHANISM-CHOSEN: A1|A2|A3`. If the user accepts escalation, no `MECHANISM-CHOSEN` line of those three values will exist — Task 3 will not run because the plan halts at Task 1. The verify branch that errors with "MECHANISM-CHOSEN marker missing or invalid" is the correct halt signal in that case.
