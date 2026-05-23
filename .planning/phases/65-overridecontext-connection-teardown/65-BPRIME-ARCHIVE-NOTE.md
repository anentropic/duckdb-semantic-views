---
phase: 65-overridecontext-connection-teardown
type: archive-note
created: 2026-05-23
---

# B-prime architecture — archived

The B-prime architecture for Phase 65 (planned 2026-05-22..23) was empirically eliminated by two spikes on 2026-05-23. The plans, CONTEXT, RESEARCH, and PATTERNS files describing it have been renamed with `BPRIME` infix and `.archived` suffix, matching the prior `PRE-BPRIME` archive pattern from the first replan.

## What B-prime was

A migration of the DDL write path from `parser_override` to `parse_function` + `plan_function`, with the goal of opening per-call `C++ Connection(*context.db)` inside `plan_function`'s callback (which receives `ClientContext &`) to do catalog reads, replacing the long-lived `catalog_conn` / `query_conn` handles that cause the in-process RW→RO reopen hang (LIFE-01).

Read path migration (Plans 04-05) was to switch from duckdb-rs's `register_table_function_with_extra_info` to a C++ Catalog API shim (`sv_register_table_function`) so read-side bind callbacks would also have `ClientContext`. That side of B-prime was validated by `65-READ-PATH-SPIKE.md` and remains a viable mechanism.

## Why it failed

The write path needed a mechanism for the rewritten INSERT/DELETE to participate in the caller's `BEGIN/COMMIT` (D-20 transactional DDL). RESEARCH §16.2/§16.4 claimed `plan_function`'s `ParserExtensionPlanResult{TableFunction, …}` return value would "drive the rewritten SQL through the binder onto the caller's conn — preserving the Phase 58 rewrite-to-native pattern." Close reading of `cpp/include/duckdb.cpp:369065-369085` (`Binder::Bind(ExtensionStatement &)`) showed this is structurally wrong — the binder always builds `LogicalGet` from `parse_result.function` via `BindTableFunction`. It does not re-parse `native_sql` into an `InsertStatement`.

The v0.8.0 `parser_override` mechanism achieves transactional DDL specifically by returning `vector<unique_ptr<SQLStatement>>` from `Parser::ParseQuery(native_sql)`, which DuckDB dispatches as `InsertStatement → LogicalInsert` on the caller's conn. That return shape is not available on `plan_function`.

The exec-time `context.Query` workaround (Option A — TableFunction whose `func` callback runs the INSERT) was spiked and self-deadlocked on `ClientContext::context_lock` (`65-EXEC-TIME-SPIKE.md`, `EXEC-TIME-RC1`), structurally identical to the earlier A2-DEADLOCK at plan time. `ExecuteTaskInternal(ClientContextLock &lock, …)` holds the lock by reference through pipeline execution.

Conclusion: no DuckDB v1.5.2 API surface provides both transactional INSERT/DELETE-on-caller's-conn AND per-call `Connection(*context.db)` access from the same callback. B-prime's premise was unachievable.

## What replaces it (v0.10.0)

A different architectural premise: keep `parser_override` (it's the only surface that delivers transactional DDL), and instead **eliminate the reads** that drive the need for a connection inside `parser_override`. Sketched moves:

- Drop `resolve_pk_from_catalog` — Snowflake PKs in semantic views are logical/user-asserted, not physical-catalog imports. Removing the auto-inference is a correctness improvement, not a regression. Users who want auto-PK declare it explicitly via `PRIMARY KEY (cols)` or `UNIQUE (cols)` in TABLES.
- Move metadata capture (`now()`, `current_database()`, `current_schema()`) from extension-side SQL execution to SQL expressions inside the rewritten INSERT, evaluated by DuckDB itself on the caller's conn.
- Fold existence checks into `INSERT … ON CONFLICT` semantics; rely on `DELETE … RETURNING name` race-guard pattern (already used by Phase 60) for DROP/ALTER postcondition checks.
- Defer type inference from CREATE-time to read-side bind callbacks, which under the C++ Catalog API registration (the surviving half of B-prime) have `ClientContext` and can probe via per-call `Connection(*context.db)`.
- Use the rewrite-to-UPDATE-with-table-function-subquery pattern for ALTER and `CREATE FROM YAML FILE` — `parser_override` emits `UPDATE _definitions SET definition = (SELECT new_def FROM __sv_compute_*(args)) WHERE name = ?`; the inner table function (registered via C++ Catalog API) has `ClientContext`, opens a per-call Connection to compute the new value, and emits it; the outer UPDATE writes on the caller's conn transactionally. Validated by `65-ALTER-REWRITE-SPIKE.md` (`ALTER-RC0`).

The result: both long-lived connections retired, transactional DDL preserved, read-path migrated to per-call connections — without requiring a new DuckDB API.

## What's preserved as evidence

These files remain in the phase directory as the empirical record:

- `65-OPTION-B-SPIKE.md` — `PLAN-THREAD-RC0` proves `Connection(*context.db)` opens cleanly from extension callbacks that have `ClientContext`. Still load-bearing for v0.10.0.
- `65-READ-PATH-SPIKE.md` — `READ-BIND-RC0` proves C++ Catalog API bind callbacks have usable `ClientContext`. Still load-bearing.
- `65-02-SPIKES.md` — `A2-DEADLOCK`. The `context.Query` re-entry deadlock at plan time. Combined with `EXEC-TIME-RC1` below, closes the lifecycle-phase grid.
- `65-EXEC-TIME-SPIKE.md` — `EXEC-TIME-RC1`. The exec-time variant of the same deadlock. Kills Option (i) for B-prime.
- `65-ALTER-REWRITE-SPIKE.md` — `ALTER-RC0`. The rewrite-to-UPDATE-with-TF-subquery pattern works. Foundational for v0.10.0's ALTER and CREATE FROM YAML FILE handling.
- `65-03-SUMMARY.md` — the halt-state SUMMARY from the executor that identified the `Binder::Bind(ExtensionStatement &)` mismatch and stopped before any production code change.
- `65-01-*` (PLAN + SUMMARY + SPIKES) — Plan 01 landed `ConnGuard` RAII + watchdog tests; still relevant for v0.10.0.
- `65-02-*` (PLAN + SUMMARY + PARTIAL-SUMMARY + SPIKES + test-sql evidence log) — Plan 02 landed `sv_register_table_function` C++ Catalog API shim infrastructure (partial). The shim itself is reusable in v0.10.0 for the read-path migration; the parser_override-side changes from the partial commit may need to be reverted before v0.10.0 plans execute.
- `65-VALIDATION.md` — phase-wide validation tracker. Will likely need refresh under v0.10.0 plans but kept as a starting point.

## What's archived

- `65-03-BPRIME-PLAN.md.archived` — write-path port to `parse_function`+`plan_function` (the load-bearing B-prime plan; killed by `EXEC-TIME-RC1`).
- `65-04-BPRIME-PLAN.md.archived`, `65-05-BPRIME-PLAN.md.archived` — read-path port plans. The read-path *mechanism* survives (C++ Catalog API registration via `sv_register_table_function`) but the plans as written assumed Plan 03's write-path mechanism as a dependency and dispatched ConnGuard/Connection wiring through it. Need re-planning under the new premise.
- `65-06-BPRIME-PLAN.md.archived`, `65-07-BPRIME-PLAN.md.archived` — lifecycle teardown + close-out plans. Both depended on Plans 03-05's mechanism. Re-plan needed.
- `65-CONTEXT-BPRIME.md.archived` — B-prime CONTEXT.md (locked decisions including D-17 "OverrideContext slim shape", D-20 "transactional DDL non-negotiable", D-22 "remove or mark dead the parser_override path"). D-20 carries forward unchanged; the rest are superseded.
- `65-RESEARCH-BPRIME.md.archived` — §16/§17 architecture write-up. The lifecycle-phase grid analysis is preserved as evidence in the SPIKE docs above.
- `65-PATTERNS-BPRIME.md.archived` — pattern map for B-prime file changes. Some patterns (ConnGuard RAII, FFI catch_unwind, etc.) carry forward; others are B-prime-specific.

## Pre-existing archives (from the earlier replan, untouched)

- `65-02-PRE-BPRIME-PLAN.md.archived`
- `65-03-PRE-BPRIME-PLAN.md.archived`
- `65-04-PRE-BPRIME-PLAN.md.archived`
- `65-CONTEXT-PRE-BPRIME.md`

These document the architecture that preceded B-prime (the per-call `ConnGuard::open(ctx.db_handle)` attempt that surfaced D-10 / `BIND-THREAD-RC1`). They were already archived when B-prime was planned; they remain in place as the earlier history.

## Next steps

1. Milestone reframed v0.9.1 → v0.10.0 (this commit).
2. `/gsd-discuss-phase 65` to produce a fresh CONTEXT.md under the read-elimination architecture.
3. `/gsd-plan-phase 65` to produce new plans.
4. Plan 66 (expansion qualification across all paths) — scope re-evaluation under the new architecture; the H2 catalog-search-path divergence root cause may dissolve once `query_conn` is retired.
