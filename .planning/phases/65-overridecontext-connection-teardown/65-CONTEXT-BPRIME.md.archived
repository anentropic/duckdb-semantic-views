# Phase 65: OverrideContext Connection Teardown — Context (B-prime architecture)

**Gathered:** 2026-05-23
**Source:** `/gsd-discuss-phase 65` after empirical falsification of Option A (D-10 / A2-DEADLOCK / BIND-THREAD-RC1) and validation of B-prime (Option B + read-path spikes)
**Status:** Ready for planning. Plan 01 shipped; Plan 02 partial commits to be reverted; Plans 02/03/04 (pre-B-prime) archived to `*-PRE-BPRIME-*.md`
**Predecessor context:** `65-CONTEXT-PRE-BPRIME.md` (D-01..D-13 — historical record of the Option A path)

<domain>
## Phase Boundary

Eliminate ALL long-lived extension-owned `duckdb_connection` handles from the `semantic_views` extension. After Phase 65, zero `duckdb_connection` survives at extension-LOAD scope; every connection is per-call, derived from the caller's live `ClientContext`, scope-bounded by a `ConnGuard`.

This is a **scope expansion** from the pre-B-prime framing (which targeted only the parser_override write path). The read-path spike on 2026-05-22 (`65-READ-PATH-SPIKE.md`, conclusion `READ-BIND-RC0`) empirically confirmed that C++ `Connection(*context.db)` succeeds from read-side bind callbacks when registration uses the C++ Catalog API directly. REQUIREMENTS.md's escape clause — *"If [BindInfo exposure] becomes possible mid-v0.9.1, fold into scope"* — is now active. Phase 65 absorbs both H1 (catalog_conn / parser_override write path) and H2 (query_conn / 14 read-side table functions + 2 scalars).

**Consequence for consumers:**
- In-process `connect(path) → LOAD → CREATE SEMANTIC VIEW → close → connect(path, read_only=True)` returns instantly (LIFE-01).
- `CREATE/DROP/ALTER SEMANTIC VIEW` inside a user `BEGIN; ... COMMIT;` continues to participate in the transaction (v0.8.0 transactional DDL preserved; non-negotiable per user constraint).
- ADBC and other clients whose catalog/schema search path diverges from the extension's old `query_conn` no longer hit `Catalog Error: Table with name X does not exist` for `SELECT … FROM semantic_view(...)` — because there is no extension-owned conn, all queries run on the caller's conn with the caller's search path. (Likely absorbs Phase 66 EXPAND-CTX-01..03; see scope fence D-19.)

Out of scope: ADBC test coverage, CHANGELOG, version bump, milestone close — those stay Phase 66 territory.
</domain>

<decisions>
## Implementation Decisions

### Architecture (LOCKED — replaces pre-B-prime D-01 through D-13)

- **D-14** — **Architecture: per-call C++ `Connection(*context.db)` from every callback that has `ClientContext &`.** No long-lived extension-owned connection anywhere. Catalog reads and DDL emission both run on per-call transient connections derived from the caller's live `DatabaseInstance`. ConnGuard (Plan 01, `src/conn_guard.rs`) is the RAII wrapper; the connection drops at end of guard scope before the callback returns. This is the "B-prime" architecture — Option B (per-call connect from plan_function) extended uniformly to the read path via C++ Catalog API registration.

  **Why this over alternatives:**
  - Empirically validated end-to-end: write path (`65-OPTION-B-SPIKE.md` Probe 1 → `PLAN-THREAD-RC0`) and read path (`65-READ-PATH-SPIKE.md` → `READ-BIND-RC0`). Both probes ran on the bundled DuckDB v1.5.2 with the current `--features extension` build.
  - Safe: no long-lived state to leak past `close()`; no thread-affinity issues; ConnGuard's `Drop` impl makes lifetime errors structurally impossible.
  - Efficient: one Connection ctor + dtor per DDL or read-side query — measured in microseconds; negligible vs. the cost of the actual SQL execution.
  - Correct for consumers: preserves v0.8.0 transactional DDL (writes still flow through the rewrite-to-native pattern onto the caller's conn); eliminates the close()-then-reopen hang; no user-facing API changes.
  - Beats Option A (ExtensionCallback + registered_state): A requires more machinery and doesn't help the write path (still needs ClientContext at plan time). B-prime is strictly simpler.
  - Beats Option C (direct C++ catalog API): our catalog (`semantic_layer._definitions`) is a regular DuckDB table; reading it via `Catalog::GetEntry` + manual row scan would be a bigger rewrite than running SELECT, with no efficiency gain.
  - Beats Option D (StorageExtension + ATTACH rewrite): D breaks the user-facing `CREATE SEMANTIC VIEW` syntax. Off the table for v0.9.1 (multi-milestone effort, user-visible breakage).

- **D-15** — **The cached-`db_handle` pattern is the root Phase 62 defect** (not just "long-lived connection leak"). `65-OPTION-B-SPIKE.md` Probe 2 isolated this: `duckdb_connect(stashed_db_handle)` fails rc=1 from request lifecycle threads while `Connection(*context.db)` succeeds. The `duckdb_database` cached in `OverrideContext` at `init_extension` time becomes stale/invalid by the time queries run (likely a `DatabaseWrapper` identity mismatch between extension-load context and live ClientContext). **B-prime deliberately bypasses this defect** by never caching `db_handle` — always derive from the live `ClientContext`. **TECH-DEBT 25** is filed for the cached-db_handle defect itself as a separate finding (per [[feedback-bounded-scope-with-signal-surfacing]]).

- **D-16** — **Read-side scope: FOLD IN.** The 14 read-side table functions (in `src/ddl/*.rs` and `src/query/*.rs`) plus 2 scalars (`get_ddl`, `read_yaml_from_semantic_view`) are re-registered through the C++ Catalog API directly (bypassing duckdb-rs's `register_table_function_with_extra_info` which marshals `ClientContext &` away). Function bodies stay in Rust behind FFI; only the registration layer moves into `cpp/src/shim.cpp`. Empirical viability confirmed by `65-READ-PATH-SPIKE.md`. Estimated cost: ~Phase 58 parser_override C++ shim scale (meaningful but bounded refactor). Activates REQUIREMENTS.md's "fold into scope" clause.

- **D-17** — **Plan 02 partial commits (`0d2c0b7`, `f9caafe`, `656bae7`) get reverted as part of the new Plan 02.** Under B-prime, `OverrideContext` no longer needs a `db_handle` field (we use `ClientContext.db` directly), and the `sv_register_parser_hooks(duckdb_database, bool, bool)` signature change is dead code (B-prime deregisters `sv_parser_override` per the prior Option A replan's intent, then promotes parse_function + plan_function for the success path). The new Plan 02 starts from a clean v0.9.0 baseline + Plan 01's ConnGuard + watchdog tests. This supersedes pre-B-prime D-12.

- **D-18** — **Plan 01 (shipped) stays intact.** `src/conn_guard.rs` ConnGuard RAII, the watchdog helper, and the B1..B4 + B11 failing-on-baseline tests are still the right primitives. ConnGuard's consumer under B-prime is the per-call site inside each ClientContext-bearing callback (plan_function + each read-side bind callback registered via C++ Catalog API). The watchdog tests are the LIFE-03 SC-3 evidence — they MUST flip green at Phase 65 close. Supersedes pre-B-prime D-13.

### Scope fence (LOCKED)

- **D-19** — **Phase 66 scope to be re-evaluated AFTER Phase 65 lands.** Strong prior that B-prime's read-path retirement eliminates the H2 long-lived-conn that causes ADBC's catalog-search-path divergence (per memory: "fact-query/semi-additive/window/materialization paths still emit raw `quote_table_ref`, so unqualified `FROM "table"` references leak through `query_conn` — separate ext-owned conn from `src/lib.rs:493-508`"). Under B-prime, all those paths run on the caller's conn with the caller's search path; EXPAND-CTX-01..03 may become unnecessary. **Do not pre-commit to a Phase 66 shape.** Phase 66 plan creation deferred until Phase 65 is verified and we can re-test ADBC end-to-end against the new architecture.

- **D-20** — **Transactional DDL semantics are non-negotiable** (per saved [[feedback-transactional-ddl-non-negotiable]]). Any mechanism that runs catalog writes on a different connection than the caller's is forbidden. B-prime preserves transactional DDL via the existing Phase 58/62 rewrite-to-native pattern (the rewritten `INSERT INTO semantic_layer._definitions ... RETURNING ...` is driven through the binder onto the caller's conn at plan-function time). New Plan 02 must explicitly verify this property — the existing Phase 58 transactional DDL tests (`test_adbc_transactions.py` and equivalents) must stay byte-identical green.

- **D-21** — **No time pressure on v0.9.1** (per saved [[feedback-no-time-pressure-get-it-right]]). The fold-in scope expansion is acceptable. Documented-limitation fallbacks are forbidden as effort-cost hedges.

- **D-22** — **Bounded scope with signal surfacing** (per saved [[feedback-bounded-scope-with-signal-surfacing]]). The cached-db_handle defect (D-15) gets a separate TECH-DEBT 25 entry; the duckdb-rs BindInfo gap that necessitates the C++ registration workaround is documented (likely closes/links to existing TECH-DEBT 19); any other long-lived-native-handle findings discovered during implementation get filed as TECH-DEBT entries or follow-up phase proposals — not silently absorbed.

### Open implementation questions (planner & researcher to resolve)

- The exact C++ Catalog API surface for registering Rust-backed table functions (likely `system_catalog.CreateTableFunction(CreateTableFunctionInfo{...})` per the read-path spike); the registration template that lets us register 14+2 functions without duplicating boilerplate; how bind data is transported back across the C++/Rust FFI boundary (since the bind callback is now C++ but the bind body stays Rust). All planner/researcher territory — empirical viability already proven by the spike, just needs production-quality wiring.
- Whether to deregister `sv_parser_override` entirely (cleanest per RESEARCH §16.2 path (a)) or demote it to validation-only (rc=2 to defer to default parser → parse_function). Likely (a). Planner to decide based on caret-rendering test coverage.
- Test scaffolding for the read-path: extend B1..B14 with new tests proving the 14+2 read-side functions don't leak Database lifetime — variants of the existing in-process RW→RO watchdog tests using SELECT against semantic views and list/describe/show functions.

### Plan structure (planner's discretion; sketch only)

Likely 6 plans under B-prime. Final breakdown is planner's call, but the natural shape:

1. **Plan 02 (NEW):** Revert Plan 02 partial commits (`0d2c0b7`, `f9caafe`, `656bae7`); add C++ Catalog API registration helper to `cpp/src/shim.cpp`; introduce `sv_register_table_function` shim that takes a Rust bind callback + extra_info + signature and registers via `Catalog::GetSystemCatalog(db).CreateTableFunction`. No production refactor yet — just the infrastructure piece.
2. **Plan 03:** Port the write path: deregister `sv_parser_override`; promote `sv_parse_function` to success-path entry (structural parse only, stash query into `SemanticViewParseData`); promote `sv_plan_function` to do catalog reads + emit native SQL via `ConnGuard::open(*context.db)`; preserve transactional DDL (Phase 58 tests stay green).
3. **Plan 04:** Port the read path (first half — 7 of 14 functions, the `list_*` / `show_*` family). Re-register via C++ Catalog API; bind callback runs Rust body with a fresh per-call ConnGuard from `*context.db`; existing `CatalogReader` calls get `guard.raw()` instead of long-lived conn.
4. **Plan 05:** Port the read path (second half — 7 more functions + 2 scalars: `describe_*` / `get_ddl` / `read_yaml_from_semantic_view` / `semantic_view` / `explain_semantic_view`).
5. **Plan 06:** Retire `init_extension`'s `catalog_conn` and `query_conn` opens at `src/lib.rs:493-508` (only safe after Plans 03-05 land); structural grep guard that asserts zero `duckdb_connect` calls in `init_extension` body; LIFE-04 update of `deferred-items.md`; B1..B11 + new read-side watchdog tests flip green; `just test-all` green; file TECH-DEBT 25.
6. (Optionally) **Plan 07:** Close-out — verify v0.8.0 transactional DDL tests byte-identical; cleanup of dead code (OverrideContext fields, sv_register_parser_hooks signature reverts); SUMMARY.

The planner may merge or split these — the architecture is what's locked, not the exact plan boundaries.
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents (researcher, planner, executor) MUST read these before acting.**

### Empirical evidence (load-bearing for all B-prime decisions)
- `.planning/phases/65-overridecontext-connection-teardown/65-OPTION-B-SPIKE.md` — Option B spike, conclusion `PLAN-THREAD-RC1` for C-API path / `PLAN-THREAD-RC0` for C++ direct path. Isolates the cached-db_handle defect.
- `.planning/phases/65-overridecontext-connection-teardown/65-READ-PATH-SPIKE.md` — Read-path spike, conclusion `READ-BIND-RC0`. Confirms B-prime extends uniformly to read-side callbacks via C++ Catalog API registration.
- `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` — Plan 02 (replanned) Wave-0 spikes: `A2-DEADLOCK` (context.Query from plan_function deadlocks on context_lock) + `BIND-THREAD-RC1` (duckdb_connect from bind callback rc=1, C-API path). Falsifies Option A.
- `.planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log` — 43/47 sqllogictest regression evidence from Plan 02 partial (D-10).
- `.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md` — Plan 01 spikes: A4 (busy-spin diagnosis confirmed), A6 (BindInfo does NOT expose db_handle via duckdb-rs — this is why D-16's C++ Catalog API registration is needed).

### Historical context (decision trail)
- `.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT-PRE-BPRIME.md` — D-01 through D-13. Explains why we landed on B-prime by elimination of Option A.
- `.planning/phases/65-overridecontext-connection-teardown/65-02-PARTIAL-SUMMARY.md` — Plan 02 partial outcome (the commits to be reverted in new Plan 02).
- `.planning/phases/65-overridecontext-connection-teardown/65-02-PRE-BPRIME-PLAN.md` / `65-03-PRE-BPRIME-PLAN.md` / `65-04-PRE-BPRIME-PLAN.md` — archived plans built on Option A. Reference only; do NOT execute. New plans replace them.

### Phase 65 still-relevant artifacts (NOT archived)
- `.planning/phases/65-overridecontext-connection-teardown/65-01-PLAN.md` + `65-01-SUMMARY.md` — Plan 01 shipped. ConnGuard + watchdog tests stay valid under B-prime.
- `.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md` — §16 (bind/plan-time architecture) is partially falsified, but §§1-15 (busy-spin diagnosis, lifecycle surface, extension survey, canonical patterns audit) remain authoritative. Researcher should refresh §16 to document B-prime's empirical foundation.
- `.planning/phases/65-overridecontext-connection-teardown/65-PATTERNS.md` — codebase pattern map; still relevant.
- `.planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md` — B1..B14 test scaffold; extends naturally to read-side watchdog tests.

### Source files (extension-owned connection wiring being retired)
- `src/lib.rs:425-515` — registers 14 read-side table functions + 2 scalars via duckdb-rs (`register_table_function_with_extra_info` / `register_scalar_function_with_state`). Under D-16, these registration sites move to C++ Catalog API via a new `sv_register_table_function` shim.
- `src/lib.rs:493-508` — opens `catalog_conn` (H1) and `query_conn` (H2). Both eliminated under B-prime.
- `src/parse.rs` — `OverrideContext` struct + `parser_override` callback + `rewrite_*` helpers. Under B-prime, `parser_override` deregistered; `OverrideContext` reverts to validation-only shape (no `db_handle` field — D-17 reverts Plan 02 partial).
- `src/catalog.rs` — `CatalogReader { conn, catalog_table_present }`. Under B-prime, `conn` is the per-call `guard.raw()` from `ConnGuard::open(*context.db)`, NOT a long-lived shared pointer.
- `src/query/table_function.rs` + `src/query/explain.rs` — `semantic_view` + `explain_semantic_view` table functions. Re-registered via C++ Catalog API; bind opens per-call ConnGuard for execution.
- `src/ddl/*.rs` (10 files) — list / describe / show / get_ddl / read_yaml bodies. Bodies stay Rust; registration moves to C++ shim.
- `cpp/src/shim.cpp` — current ParserExtension wiring (Phase 58/62). B-prime adds a `sv_register_table_function` helper here.

### DuckDB internals (read once, anchored to v1.5.2)
- `cpp/include/duckdb.cpp:275773-275778` — `Connection::Connection(DatabaseInstance&)` ctor. The empirically-validated entry point for B-prime's per-call connect.
- `cpp/include/duckdb.cpp:276149-276178` — `ExtensionCallback` (OnConnectionOpened / OnConnectionClosed hooks). NOT used by B-prime, but useful background.
- `cpp/include/duckdb.cpp:347253-347300` — parse / `parser_override` / `parse_function` dispatch. Why parser_override deregistration is safe (default parser fails on `CREATE SEMANTIC VIEW` prefix, triggers parse_function next).
- `cpp/include/duckdb.cpp:369065-369085` — `Binder::Bind(ExtensionStatement&)` → `plan_function` dispatch. The point where `ClientContext &` becomes available.
- `cpp/include/duckdb.cpp:266432-266447` — `duckdb_connect` C-API implementation. Shows the wrapper dereference path that breaks under cached-db_handle (D-15).

### REQUIREMENTS.md trace
- LIFE-01 / LIFE-02 / LIFE-03 / LIFE-04 — all satisfied by B-prime per D-14. LIFE-02 explicitly admits (a) deterministic teardown as acceptable; B-prime is the stronger form (no connection to tear down).
- REQUIREMENTS.md "Out of scope" section, last item — *"Re-routing read-side table functions … blocked on the same BindInfo connection-handle exposure as TECH-DEBT #19. If that exposure becomes possible mid-v0.9.1, fold into scope"* — fold-in trigger activated by `65-READ-PATH-SPIKE.md` (the C++ Catalog API registration is the unblocking mechanism).

### Project conventions
- `CLAUDE.md` (repo root) — quality gate `just test-all`; foreground builds only; per-phase milestone branch; testing requirements.
- `MEMORY.md` (auto-memory) — load-bearing feedback entries: `feedback-transactional-ddl-non-negotiable`, `feedback-no-time-pressure-get-it-right`, `feedback-root-cause-over-hacks`, `feedback-bounded-scope-with-signal-surfacing`, `feedback-no-parallel-builds`, `feedback-worktree-isolation`, `feedback-no-background-agents`, `feedback-documented-limitations`.
</canonical_refs>

<specifics>
## Specific Implementation Notes

### B-prime per-call pattern (canonical shape — write path)

```cpp
// Inside sv_plan_function (cpp/src/shim.cpp), receives ClientContext &context
auto *parse_data = static_cast<SemanticViewParseData*>(parse_data_ptr.get());
{
    // C++ direct — empirically validated by 65-OPTION-B-SPIKE.md Probe 1
    Connection probe(*context.db);
    duckdb_connection conn = reinterpret_cast<duckdb_connection>(&probe);

    // Call Rust side: existing rewrite_* / emit_native_create_sql helpers
    // run against this per-call connection, see only committed state (acceptable
    // per existing TECH-DEBT 19), drop the guard at end of scope.
    sv_emit_native_sql_rust(conn, parse_data->validated_form, &native_sql_out, ...);

    // probe destructs here -> Connection dtor -> ConnectionManager::RemoveConnection
}
// Build ParserExtensionPlanResult that drives native_sql_out through the binder
// onto the caller's connection (transactional DDL preserved — see D-20).
return build_plan_result(native_sql_out);
```

### B-prime per-call pattern (canonical shape — read path)

```cpp
// Inside C++ bind callback registered via system_catalog.CreateTableFunction,
// receives ClientContext &context, TableFunctionBindInput &input
auto bind_data = make_uniq<SemanticViewsBindData>();
{
    // C++ direct — empirically validated by 65-READ-PATH-SPIKE.md
    Connection probe(*context.db);
    duckdb_connection conn = reinterpret_cast<duckdb_connection>(&probe);

    // Call Rust side: existing CatalogReader / list / describe / show body
    // runs against this per-call conn, returns column types + names + bind data
    sv_list_semantic_views_bind_rust(conn, &input, &return_types_out, &names_out, ...);

    // probe destructs here
}
// Return bind_data; init/execute will run on the caller's connection through
// the normal table function lifecycle — no extension-owned conn needed.
return bind_data;
```

### What changes in src/lib.rs

Before (current):
```rust
// init_extension opens H1 and H2 at extension-load time
let catalog_conn = duckdb_connect(db_handle)?;   // H1
let query_conn = duckdb_connect(db_handle)?;     // H2
// ... 14 register_table_function_with_extra_info calls + 2 scalars
con.register_table_function_with_extra_info::<ListSemanticViewsVTab, _>(&catalog_reader)?;
// ... etc
```

After (B-prime):
```rust
// init_extension: NO long-lived duckdb_connect at all
// Catalog table presence check: probe via per-call ConnGuard if needed at init,
// then drop immediately (one-time at load, then nothing extension-owned survives).
//
// Registration calls move to C++ via a new shim helper:
sv_register_table_function_shim(db_handle, "list_semantic_views",
                                 list_semantic_views_bind_rust,
                                 list_semantic_views_init_rust,
                                 list_semantic_views_main_rust)?;
// ... 14 + 2 calls total, all through the shim
```

### What gets reverted from Plan 02 partial

Commits `0d2c0b7`, `f9caafe`, `656bae7` revert. The reverts produce:
- `OverrideContext` returns to its pre-Plan-02 shape (`catalog: CatalogReader` field, NOT `db_handle`).
- `sv_register_parser_hooks` signature returns to pre-Plan-02 form.
- The `INTENTIONAL LEAK` comment in `cpp/src/shim.cpp` returns (and then gets removed by the new Plan 02's deregister-sv_parser_override work — the parser_override callback is gone entirely under B-prime).

After the revert, `just test-sql` returns to 47/47 PASS (v0.9.0 baseline). New Plan 02 starts from this clean state plus Plan 01's ConnGuard + watchdog tests.

### Broader audit (per D-22)

After the implementation, RESEARCH.md gets a "Long-lived native handles audit (post-B-prime)" section verifying that ZERO extension-owned `duckdb_connection` / prepared-statement / parser-info pointers survive beyond a single callback's scope. Anything that does becomes a TECH-DEBT entry or follow-up phase proposal.
</specifics>

<deferred>
## Deferred Ideas (NOT in Phase 65)

- **Phase 66 scope re-evaluation:** revisit EXPAND-CTX-01..03 after Phase 65 lands. Likely shrinks to just REL-01 + REL-02 + ADBC verification test, given B-prime eliminates the H2 catalog-search-path divergence root cause. Do not pre-commit until empirically verified.
- **CHANGELOG / version bump / milestone tagging** — Phase 66.
- **RO→RW reverse direction** (if it has the same hang shape) — per pre-B-prime D-09: surface as finding if discovered; not in Phase 65 scope unless covered as a side effect of B-prime doing the right thing.
- **TECH-DEBT 25 (cached-db_handle Phase 62 defect)** — filed as part of new Plan 06 close-out per D-15 and D-22. Resolution: "naturally resolved by Phase 65 B-prime architecture (no cached db_handle anywhere)."
- **TECH-DEBT 19 (DESCRIBE/SHOW read committed state)** — under B-prime, per-call ConnGuard still sees only committed state from `*context.db` (transient connections inherit the database's committed state at the moment they're opened). Behaviour unchanged from v0.9.0; TECH-DEBT 19 stays open. Documented in the new Plan 06 SUMMARY.
- **In-memory DB path verification** (per pre-B-prime RESEARCH §9.3) — under B-prime, `:memory:` works the same way: `*context.db` is a live `shared_ptr<DatabaseInstance>` regardless of storage. Add a smoke test in new Plan 06.
</deferred>

---

*Phase: 65-overridecontext-connection-teardown*
*Context gathered: 2026-05-23 via /gsd-discuss-phase 65 after Option A falsification + B-prime validation*
*Supersedes: 65-CONTEXT-PRE-BPRIME.md (D-01..D-13)*
