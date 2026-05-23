---
phase: 65-overridecontext-connection-teardown
plan: 02
type: execute
wave: 2
depends_on:
  - 65-01
files_modified:
  - src/parse.rs
  - src/lib.rs
  - cpp/src/shim.cpp
  - src/conn_guard.rs
autonomous: false
requirements:
  - LIFE-01
  - LIFE-02
tags:
  - duckdb
  - rust
  - ffi
  - parser_extension
  - parse_function
  - plan_function
  - lifecycle
  - bind-time-architecture
  - option-a

must_haves:
  truths:
    - "Verifiable: `grep -E 'ConnGuard::open\\(ctx\\.db_handle\\)' src/parse.rs` returns 0 matches — the 4× broken parse-time sites in `rewrite_drop_or_alter` / `emit_native_create_sql` existence-check / `emit_native_create_sql` enrichment / `rewrite_yaml_file_create` enrichment are gone (D-10)."
    - "Verifiable: `grep -E 'ext\\.parser_override\\s*=\\s*nullptr' cpp/src/shim.cpp` returns ≥1 match — `sv_parser_override` is deregistered (RESEARCH §16.6 #1)."
    - "Verifiable: `grep -E 'sv_plan_semantic_view_ddl' cpp/src/shim.cpp src/parse.rs` returns ≥2 matches — new C++ caller + Rust definition for the bind/plan-time success path are present."
    - "Verifiable: `grep -F 'PHASE-65-GUARD: do not reintroduce duckdb_connection or CatalogReader field here.' src/parse.rs` returns ≥1 match — the structural marker that Plan 04 B13 test consumes is preserved."
    - "Verifiable: `65-02-SPIKES.md` exists with `## A2` and `## A6-bind` sections; each has a conclusion line (one of `A2-VIABLE` / `A2-DEADLOCK` / `A2-UNEXPECTED` and one of `BIND-THREAD-RC0` / `BIND-THREAD-RC1` respectively)."
    - "Verifiable: `65-02-SPIKES.md` contains a `MECHANISM-CHOSEN:` marker line set to exactly one of `A1`, `A2`, or `A3` — the locked output of the checkpoint:decision in Task 1."
    - "Verifiable: `just test-sql` exits 0 with the same pass count as the v0.9.0 tag — the 43/47 failure state recorded in `65-02-A7-test-sql-evidence.log` is healed."
    - "Verifiable: `just test-caret` exits 0 — Phase 62 caret-rendering tests (`test/integration/test_caret_position.py`) stay green byte-identical for invalid CREATE bodies + near-miss typos."
    - "Verifiable: If `MECHANISM-CHOSEN` is `A1` or `A3`, `grep -E 'TECH-DEBT 25' TECH-DEBT.md` returns ≥1 match (transactional-DDL regression filed). If `MECHANISM-CHOSEN` is `A2`, `git diff TECH-DEBT.md` returns empty (transactional semantics preserved; no entry needed)."
    - "Observable: `sv_parse_function` returns PARSE_SUCCESSFUL (new rc=4 contract) with a populated `SemanticViewParseData` carrier whenever input is recognised semantic-view DDL; it still returns DISPLAY_EXTENSION_ERROR with caret position for invalid bodies and near-misses."
    - "Observable: `sv_plan_function` receives `ClientContext &context`, derives a per-call `duckdb_connection` via the new `sv_get_override_context_db_handle` FFI accessor + `ConnGuard::open`, runs `emit_native_create_sql` / `rewrite_drop_or_alter` against that connection, and drives the rewritten SQL into a `ParserExtensionPlanResult` per the locked mechanism."
  artifacts:
    - path: "src/parse.rs"
      provides: "sv_parse_function_rust upgraded to success-path; sv_plan_semantic_view_ddl new entry that performs catalog reads at bind/plan time; rewrite_* helpers stripped of inline ConnGuard::open"
      contains: "sv_plan_semantic_view_ddl"
    - path: "cpp/src/shim.cpp"
      provides: "sv_parser_override deregistered; sv_parse_function returns PARSE_SUCCESSFUL with SemanticViewParseData; sv_plan_function calls into Rust with OverrideContext-derived duckdb_database"
      contains: "sv_plan_function"
    - path: ".planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md"
      provides: "A2 viability + bind-thread `duckdb_connect` spike outcomes + MECHANISM-CHOSEN marker (the two Wave-0 spikes per RESEARCH §16.6 #2 and #8 + the locked Task 1 decision)"
      contains: "## A2"
  key_links:
    - from: "cpp/src/shim.cpp::sv_plan_function"
      to: "src/parse.rs::sv_plan_semantic_view_ddl"
      via: "extern \"C\" FFI with OverrideContext-derived duckdb_database + SemanticViewParseData payload"
      pattern: "sv_plan_semantic_view_ddl"
    - from: "src/parse.rs::sv_plan_semantic_view_ddl"
      to: "src/conn_guard.rs::ConnGuard"
      via: "ConnGuard::open(db) at bind/plan time"
      pattern: "ConnGuard::open"
    - from: "src/parse.rs::sv_parse_function_rust"
      to: "cpp/src/shim.cpp::SemanticViewParseData"
      via: "manual-LE-encoded payload + raw query stash, returned as PARSE_SUCCESSFUL"
      pattern: "SemanticViewParseData"
---

<objective>
Replace the parse-time per-call ConnGuard surface (falsified by the 43/47 sqllogictest failures recorded in
`65-02-A7-test-sql-evidence.log`) with the Option A bind/plan-time architecture locked by D-11. Catalog reads
move OUT of `parser_override` to `sv_plan_function`, which receives `ClientContext &` and is therefore free of
the parse-thread rc=1 failure mode that broke the prior approach. `parser_override` is deregistered entirely;
`parse_function` is promoted from error-reporting to success-path entry; `plan_function` is promoted from
unreachable-stub to the catalog-read + native-SQL-emission entry.

Purpose: D-10 falsified the parse-time approach empirically. D-11 locks Option A. This plan operationalises Option A:
runs two Wave-0 spikes (A2 viability + bind-thread `duckdb_connect`) BEFORE any production refactor, gates the
A1/A2/A3 mechanism choice on a `checkpoint:decision`, removes the 4× broken sites in `parse.rs::rewrite_*`, and
delivers the new `parse_function` + `plan_function` success path. The transactional-DDL semantics of v0.8.0 / v0.9.0
are preserved if A2 is viable; if A2 deadlocks, a TECH-DEBT entry documenting the regression is filed and the user
approves before shipping (per CONTEXT.md D-01).

Output: parser_override deregistered; sv_parse_function returns PARSE_SUCCESSFUL with SemanticViewParseData;
sv_plan_function performs catalog reads on a per-call ConnGuard derived from ClientContext; Phase 62 transactional
DDL + caret tests flip from 4/47 PASS back to 47/47 PASS byte-identical.
</objective>

<architecture>
Rationale and architectural constraints that frame this plan (NOT verifiable outcomes — those live in
`must_haves.truths`). Sources: CONTEXT.md D-01..D-13 and RESEARCH.md §16.

- **D-01 / D-11**: catalog reads must move OUT of `parser_override` to bind/plan time. `sv_plan_function` is the
  new success-path entry, derived from the `ClientContext &context` it receives.
- **D-12**: the structural commits `0d2c0b7` (`db_handle` field swap), `f9caafe` (`sv_register_parser_hooks`
  signature change), and `656bae7` (evidence preservation) STAY — `db_handle` plumbing is the foundation for both
  shape (a) and shape (b) and was correct work done by Plan 02 partial.
- **RESEARCH §16.6 #1**: `sv_parser_override` is removed entirely (set to `nullptr` in `sv_register_parser_hooks`).
  On success, the default DuckDB parser fails on the unrecognised `CREATE SEMANTIC VIEW` prefix and DuckDB then
  calls `parse_function` per RESEARCH §16.2.
- **RESEARCH §16.6 #2**: a Wave-0 A2 spike (run `context.Query(native_sql)` inside `plan_function`) is the
  empirical discriminator for whether transactional DDL semantics survive. The Task 1 `checkpoint:decision` is
  gated on this spike result.
- **RESEARCH §16.6 #8**: a Wave-0 bind-thread spike (open `ConnGuard::open(handle.db)` from inside ONE read-side
  bind callback) confirms `duckdb_connect` rc=0 from the bind thread BEFORE Plan 03 mass refactors 14+2 sites.
- **RESEARCH §16.4 / §16.6 #3**: `SemanticViewParseData` carries `{ query: string, payload: vector<uint8_t> }`.
  The payload is an opaque (to C++) manual-LE-encoded snapshot of `{verb, validated_form, view_name, or_replace,
  if_not_exists, if_exists, byte_offset}` that the Rust side fills at parse and reads at plan.
- **Conditional TECH-DEBT 25**: if the checkpoint:decision forces A1 or A3 (i.e., A2 deadlocks per RESEARCH §16.6
  #7), a TECH-DEBT entry documenting the transactional-DDL regression is added BEFORE the Plan 02 SUMMARY is
  written, and the `checkpoint:human-verify` gates user approval before shipping.
</architecture>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/REQUIREMENTS.md
@.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT.md
@.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
@.planning/phases/65-overridecontext-connection-teardown/65-PATTERNS.md
@.planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md
@.planning/phases/65-overridecontext-connection-teardown/65-01-SUMMARY.md
@.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md
@.planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md
@.planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log
@CLAUDE.md

<interfaces>
<!-- Current OverrideContext (Plan 02 partial — STAYS as-is) — src/parse.rs:46-88 -->
- pub struct OverrideContext { pub db_handle: duckdb_database, pub catalog_table_present: bool, pub is_file_backed: bool }
- PHASE-65-GUARD marker comment present as first line in struct body
- unsafe impl Send + Sync (per Plan 02 partial — keep)
- No custom Drop

<!-- Current parse_function / plan_function (Phase 62 shape — being promoted) — cpp/src/shim.cpp:262-339 -->
- sv_parse_stub: today only returns DISPLAY_EXTENSION_ERROR or DISPLAY_ORIGINAL_ERROR (NEVER PARSE_SUCCESSFUL)
- sv_plan_unreachable: today throws InternalException (asserts contract that parse_function never returns SUCCESSFUL)
- parse_function signature: ParserExtensionParseResult(ParserExtensionInfo*, const string& query)  -- NO ClientContext
- plan_function signature: ParserExtensionPlanResult(ParserExtensionInfo*, ClientContext&, unique_ptr<ParserExtensionParseData>)  -- HAS ClientContext

<!-- Current Rust FFI (Phase 62 + Plan 02 partial — being reshaped) — src/parse.rs:2618 onwards -->
- sv_parser_override_rust(ctx_ptr, query, sql_out, error_out, ...) -> u8  -- TO BE RETIRED FROM PRODUCTION USE
- sv_parse_function_rust(ctx_ptr, query, error_buf, position_out) -> u8  -- TO BE PROMOTED to success path (add payload out-params)
- sv_make_override_context(db, catalog_table_present, is_file_backed) -> *mut c_void  -- STAYS
- sv_drop_override_context(*mut c_void)  -- STAYS

<!-- ParserExtensionPlanResult shape (parser_extension_compat.hpp:108-119) — THE plan_function output -->
- { TableFunction function, vector<Value> parameters, ... }
- bound by DuckDB via BindTableFunction(plan_result.function, std::move(plan_result.parameters)) at duckdb.cpp:369077

<!-- ConnGuard (Plan 01) — STAYS as the per-call primitive -->
- pub(crate) struct ConnGuard { conn: ffi::duckdb_connection }  -- src/conn_guard.rs
- pub(crate) unsafe fn open(db: ffi::duckdb_database) -> Result<Self, String>
- pub(crate) fn raw(&self) -> ffi::duckdb_connection
- impl Drop { duckdb_disconnect on non-null }
- carries #[allow(dead_code)] / #[allow(unused_imports)] today (Plan 04 removes once consumed in production)

<!-- DuckDB C API exposure of ClientContext -> duckdb_connection (per RESEARCH §16.3) -->
- duckdb_connection_get_client_context(connection, out_context)  -- the REVERSE direction; not useful here
- HOWEVER: plan_function receives ClientContext& directly (C++ side); to get a duckdb_database the recommended
  path is via a new FFI accessor `sv_get_override_context_db_handle(ctx_ptr) -> duckdb_database` that reads
  OverrideContext.db_handle. C++ then calls Rust `sv_plan_semantic_view_ddl(ctx, payload, db, ...)`.
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 0a: Wave-0 spike — A2 plan_function context.Query viability</name>
  <files>
    .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
  </files>
  <read_first>
    .planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
    .planning/phases/65-overridecontext-connection-teardown/65-CONTEXT.md
    cpp/src/shim.cpp
    cpp/include/parser_extension_compat.hpp
    cpp/include/duckdb.cpp
    src/parse.rs
  </read_first>
  <action>
    Per RESEARCH §16.6 #2 — the central unresolved question is whether `context.Query(native_sql)` from inside
    `sv_plan_function` deadlocks on `ClientContext::context_lock`. This spike is the smallest experiment that
    distinguishes Option A1 (extra TableFunction indirection — regresses transactional DDL) from Option A2
    (`context.Query(native_sql)` directly — preserves transactional DDL) from Option A3 (typed TableFunction per
    DDL verb — also regresses transactional DDL).

    Write a minimal C++ spike inside `cpp/src/shim.cpp` (or a scratch translation unit) that:

    1. Registers `sv_plan_function_spike` as `ext.plan_function` instead of `sv_plan_unreachable` (temporary).
    2. Inside `sv_plan_function_spike`, before returning a trivial `ParserExtensionPlanResult` (a no-op TableFunction
       returning one constant row), call `auto result = context.Query("SELECT 42 AS spike");` and capture the outcome.
    3. Also test `auto insert_result = context.Query("INSERT INTO __sv_spike VALUES (1)");` after first creating
       `__sv_spike(i INT)` to verify DML on the caller's transaction.
    4. Wire `sv_parse_function` to return PARSE_SUCCESSFUL with a dummy `SemanticViewParseData` whenever input starts
       with `SPIKE_PLAN_PROBE` so the spike fires for that sentinel prefix only — keeps blast radius bounded.

    Spike test driver: a single sqllogictest file at `$TMPDIR/65_02_a2_spike.test` exercising the SPIKE_PLAN_PROBE
    sentinel after CREATE TABLE __sv_spike (i INT). Run via the project's sqllogictest invocation (see `just test-sql`
    recipe in `justfile`); capture both stdout and stderr to `$TMPDIR/65_02_a2_spike.log`.

    Record outcomes in `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` with sections
    `## A2` and verbatim evidence. Three possible outcomes:

    (i) `A2-VIABLE` — both `context.Query("SELECT 42")` and `context.Query("INSERT ...")` succeed, the INSERT's
        effect is visible inside the same outer caller transaction, no deadlock. Plan 02 main path proceeds with
        Option A2 (the clean path).

    (ii) `A2-DEADLOCK` — either query hangs >5s (use a watchdog via kill -9 after sleep 5) or returns "context
        lock held" / similar error. Plan 02 escalates via `checkpoint:decision` below; user picks A1 (extra
        TableFunction with documented transactional regression) or A3 (typed-per-verb TableFunctions, same
        regression).

    (iii) `A2-UNEXPECTED` — any other failure mode (segfault, wrong error message, undocumented behaviour).
         STOP. Record verbatim evidence and request user direction.

    AFTER the spike outcome is recorded: revert the scratch C++ changes (`git checkout cpp/src/shim.cpp` if
    nothing else of value is in flight), leaving only `65-02-SPIKES.md` committed. The scratch code is not
    production-quality; production wiring happens in Task 3 below using the spike's empirical result.

    Note on duckdb-rs availability for the spike: the spike does NOT use `bind.get_extra_info` or any duckdb-rs API
    — it works at the C++ shim level directly, so it is unaffected by the A6 BindInfo limitation (Plan 01 SPIKES).
  </action>
  <verify>
    <automated>test -f .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md && grep -q "## A2" .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md && grep -E "A2-VIABLE|A2-DEADLOCK|A2-UNEXPECTED" .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md</automated>
  </verify>
  <acceptance_criteria>
    - `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` exists with `## A2` section.
    - A2 section states exactly one of `A2-VIABLE`, `A2-DEADLOCK`, or `A2-UNEXPECTED` as the conclusion line.
    - Verbatim stdout/stderr of the spike run is captured in a fenced code block inside the A2 section.
    - For `A2-VIABLE`: the section explicitly states the INSERT was visible inside the outer caller transaction.
    - For `A2-DEADLOCK`: the section captures the watchdog-kill output AND the lldb backtrace from the hung process
      (mirror Plan 01 SPIKES A4 evidence shape).
    - For `A2-UNEXPECTED`: the section captures the full error message, return code, and any segfault/abort signal.
    - `git diff --stat cpp/src/shim.cpp` returns empty AFTER the spike (scratch code reverted).
    - `git diff --stat src/parse.rs` returns empty AFTER the spike (no Rust-side scratch code committed).
    - The only commit produced by this task is `docs(65-02): A2 plan_function viability spike outcome` adding
      `65-02-SPIKES.md`.
  </acceptance_criteria>
  <done>
    A2 viability is empirically pinned to one of three outcomes. The checkpoint:decision below uses this evidence
    to lock the production mechanism choice (A1/A2/A3) without speculation.
  </done>
</task>

<task type="auto">
  <name>Task 0b: Wave-0 spike — bind-thread duckdb_connect rc check</name>
  <files>
    .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
  </files>
  <read_first>
    .planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
    src/ddl/list.rs
    src/conn_guard.rs
    src/lib.rs
  </read_first>
  <action>
    Per RESEARCH §16.6 #8 — D-10 falsified `duckdb_connect` from inside `parser_override` (parse-thread rc=1). The
    bind thread is a different lifecycle phase (post-parse, inside `Binder::Bind(TableFunctionRef&)`), and there is
    no evidence it suffers the same failure — but D-10 implicitly demands we verify cheaply before mass-refactoring
    14 read-side bind callbacks + 2 scalars in Plan 03.

    Pick ONE read-side bind callback as the test bed — recommend `src/ddl/list.rs::ListSemanticViewsVTab::bind`
    (canonical 2-liner per PATTERNS.md). Modify it locally (scratch, NOT committed-to-main) to:

    1. Get the `CatalogReader` from extra_info as today.
    2. Open a fresh `ConnGuard::open(<db_handle>)` from inside the bind callback. Today's `CatalogReader` carries
       `conn: duckdb_connection` (NOT `db_handle`) — to get `db_handle` for the spike, the cleanest path is to
       register a NEW spike table function `list_semantic_views_spike()` whose extra_info is a tuple of
       `(CatalogReader, duckdb_database)` (the latter captured at init_extension time) and call
       `ConnGuard::open(db)` inside its bind.
    3. Log the rc and the `Result<ConnGuard, String>` outcome to stderr via `eprintln!`.
    4. Build (`just build`) and run a sqllogictest that calls `SELECT * FROM list_semantic_views_spike()`. Capture
       stderr output.

    Record outcomes in `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` (append `## A6-bind`
    section after Task 0a's `## A2`). Two possible outcomes:

    (i) `BIND-THREAD-RC0` — `duckdb_connect` from inside the bind callback returns rc=0; ConnGuard is constructed
        and `guard.raw()` is a non-null `duckdb_connection`. Plan 03 proceeds with shape (b) per Plan 01 SPIKES
        A6: `CatalogHandle { db, catalog_table_present }` in extra_info; per-bind `ConnGuard::open(handle.db)`.

    (ii) `BIND-THREAD-RC1` — `duckdb_connect` from inside the bind callback returns rc=1, same failure mode as
         parse-thread. STOP. Plan 03's read-side architecture is constrained too; escalate via a separate
         `checkpoint:decision` (added at the END of Task 0b if this case fires) with options including
         "scalar function with custom State" (`type State = (CatalogReader, db_handle)`; ConnGuard inside
         `invoke`) and "documented limitation: read-path keeps long-lived query_conn for v0.9.1, fix in v0.9.2".

    AFTER recording: revert the scratch changes (`git checkout src/ddl/list.rs src/lib.rs` for any scratch
    registrations). Only `65-02-SPIKES.md` is committed.

    Budget: <1 day per RESEARCH §16.6 #8.
  </action>
  <verify>
    <automated>grep -q "## A6-bind" .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md && grep -E "BIND-THREAD-RC0|BIND-THREAD-RC1" .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md</automated>
  </verify>
  <acceptance_criteria>
    - `## A6-bind` section appended to `65-02-SPIKES.md` (Task 0a created `## A2`; this task appends `## A6-bind`).
    - Conclusion line is exactly one of `BIND-THREAD-RC0` or `BIND-THREAD-RC1`.
    - Verbatim stderr captures of the rc are recorded in a fenced code block inside the section.
    - For `BIND-THREAD-RC1`: an additional `## A6-bind-checkpoint-decision` subsection is added laying out the
      Plan 03 escalation options (scalar State workaround / documented limitation / hybrid).
    - `git diff --stat src/ src/lib.rs` returns empty (scratch reverted).
    - The only commit produced by this task is `docs(65-02): A6-bind bind-thread duckdb_connect spike outcome`.
  </acceptance_criteria>
  <done>
    The Plan 03 read-side architecture is unblocked (BIND-THREAD-RC0) or constrained-with-evidence (BIND-THREAD-RC1).
    No mass refactor proceeds in Plan 03 before this evidence is in hand.
  </done>
</task>

<task type="checkpoint:decision" gate="blocking">
  <name>Task 1: Decision — A1 vs A2 vs A3 plan_function mechanism</name>
  <decision>
    Which mechanism does `sv_plan_function` use to drive the rewritten native SQL into DuckDB's execution path?
  </decision>
  <context>
    Task 0a's spike result determines the available options. The transactional-DDL behaviour of v0.8.0 / v0.9.0
    (CREATE SEMANTIC VIEW inside a user `BEGIN;...COMMIT` participates in the transaction) is at stake. CONTEXT.md
    D-01 forbids documented-limitation fallbacks unless the root-cause path is empirically impossible. A2 is the
    only option that preserves the transactional behaviour without architectural compromise; A1/A3 introduce a
    TableFunction indirection layer that breaks transactional CREATE.

    The spike outcome (Task 0a `## A2`) gates which options are presented here.

    **IMPORTANT — record the locked decision in `65-02-SPIKES.md`:** after the user selects an option, append the
    line `MECHANISM-CHOSEN: A1` (or `A2` / `A3`) to `65-02-SPIKES.md` as a top-level marker (not inside a fenced
    block). Task 3's automated verify reads this marker to branch its acceptance assertions. The marker must be
    present BEFORE Task 2 starts work.
  </context>
  <options>
    <option id="a2-clean">
      <name>Option A2 — context.Query(native_sql) inside sv_plan_function (the clean path)</name>
      <pros>Preserves Phase 58 / 62 transactional DDL semantics; smallest API surface change; matches RESEARCH §16.6 #2 recommendation; no new TableFunctions need to be registered; sv_plan_function returns a trivial sentinel result and the actual work happens via context.Query on the caller's connection.</pros>
      <cons>Requires Task 0a A2-VIABLE outcome; if spike showed A2-DEADLOCK, this option is unavailable. Slight semantic drift: the plan_function's "result" is now a side-effect plus a sentinel projection, which is unusual for the API. Need to render sv_plan_function's TableFunction as a "completed work" stub.</cons>
    </option>
    <option id="a1-extra-tf">
      <name>Option A1 — register __sv_execute_native(json_view_name TEXT, native_sql TEXT) table function</name>
      <pros>Available even if A2 deadlocks; mechanism is straightforward (sv_plan_function emits parameters into the new TableFunction; bind opens its own connection and runs the SQL).</pros>
      <cons>Regresses transactional DDL: the actual INSERT runs on a different connection than the caller, so CREATE inside a user transaction no longer participates in that transaction. Per RESEARCH §16.2 Option A1: "REJECTED by extension because it regresses Phase 58 transactional DDL". A TECH-DEBT entry MUST be filed before shipping this. CONTEXT.md D-01 admits this only as a last resort.</cons>
    </option>
    <option id="a3-typed-per-verb">
      <name>Option A3 — register __sv_create_view / __sv_drop_view / __sv_alter_view_* TableFunctions</name>
      <pros>Typed at the C++ side (cleaner shim signatures); avoids treating SQL as opaque text in TableFunction parameters.</pros>
      <cons>Same transactional-regression problem as A1 plus 4-6 new TableFunctions to register and maintain. Strictly worse than A1 on code-volume; same downside on transactional DDL.</cons>
    </option>
    <option id="escalate">
      <name>Escalate to /gsd:discuss-phase --assumptions (re-research)</name>
      <pros>If Task 0a returned A2-UNEXPECTED, this is the right escape hatch — we have an unknown failure mode that needs a different research direction.</pros>
      <cons>Pushes back the v0.9.1 ship date; v0.9.1 milestone branch stays broken (43/47 sqllogictests red on `milestone/v0.9.1`).</cons>
    </option>
  </options>
  <resume-signal>Select: a2-clean (if Task 0a returned A2-VIABLE), a1-extra-tf (if A2-DEADLOCK and user accepts transactional regression), a3-typed-per-verb (if A2-DEADLOCK and user prefers typed shape), or escalate (if A2-UNEXPECTED). After selection, append `MECHANISM-CHOSEN: <A1|A2|A3>` to `65-02-SPIKES.md`.</resume-signal>
</task>

<task type="auto" tdd="false">
  <name>Task 2: Remove the 4× broken parse-time ConnGuard sites; deregister sv_parser_override</name>
  <files>
    src/parse.rs
    cpp/src/shim.cpp
  </files>
  <read_first>
    src/parse.rs
    cpp/src/shim.cpp
    .planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log
    .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
  </read_first>
  <action>
    This task removes the known-broken parse-time surface BEFORE adding the new bind/plan-time surface, so the
    intermediate commit is "broken in a different way" (parse_function/plan_function not yet promoted) rather than
    "broken in the same way" (rc=1 from parser_override). Task 3 immediately restores green by promoting
    parse_function/plan_function.

    Step 1 — Remove the 4× `ConnGuard::open(ctx.db_handle)` call sites inside `src/parse.rs`. Per the grep evidence
    (`src/parse.rs:1801, 1941, 1974, 2095`), the sites are:
    - `rewrite_drop_or_alter` body (~`src/parse.rs:1800-1805`): the `let guard = ... ConnGuard::open(ctx.db_handle)`
      block + the `let catalog = CatalogReader::new(guard.raw(), ctx.catalog_table_present)` line.
    - `emit_native_create_sql` existence-check block (~`src/parse.rs:1940-1969`): the entire scoped block around
      `ConnGuard::open` for the existence pre-check.
    - `emit_native_create_sql` enrichment block (~`src/parse.rs:1973-1994`): the second `ConnGuard::open` + the
      `enrich_definition_for_create(..., guard.raw(), ...)` call.
    - `rewrite_yaml_file_create` enrichment (~`src/parse.rs:2094-2099`): the `yaml_guard = ConnGuard::open` block.

    For each removal, restructure the function body so it produces a `ParseOutcome` carrier (NEW enum, defined in
    Step 2 below) holding ONLY the validated form + verb-specific flags (no catalog reads). Each `rewrite_*` becomes
    a pure structural rewrite: parse the body, validate keywords, produce the validated form, return it for the
    plan_function path to consume.

    Step 2 — Define the `ParseOutcome` enum in `src/parse.rs` (near the existing `DdlKind` definition, ~line 100):
    - `pub enum ParseOutcome { Create { name: String, def: SemanticViewDefinition, or_replace: bool, if_not_exists: bool }, Drop { name: String, if_exists: bool }, AlterRename { old_name: String, new_name: String, if_exists: bool }, AlterComment { name: String, comment: Option<String>, if_exists: bool }, Describe { name: String }, Show { kind: ShowKind, filter: Option<String> }, List { terse: bool }, YamlFileCreate { file_path: String, kind_str: String, name: String, comment: String } }`
    - Add a `serialize_le(&self) -> Vec<u8>` method that does manual little-endian encoding (per RESEARCH §16.4):
      first byte = discriminator, then per-variant fields encoded as `u32-length + utf8-bytes` for strings and
      a single `u8` for booleans, with a final JSON blob for the `SemanticViewDefinition` in Create.
    - Add a `deserialize_le(bytes: &[u8]) -> Result<Self, String>` method for the plan_function side.
    - Add a unit test in `#[cfg(test)] mod tests` that round-trips each variant: `let bytes = outcome.serialize_le(); let back = ParseOutcome::deserialize_le(&bytes).unwrap(); assert_eq!(outcome, back);` (derive PartialEq on the enum).

    Step 3 — Refactor the `rewrite_*` helpers (`rewrite_to_native_sql`, `rewrite_drop_or_alter`, `rewrite_create`,
    `rewrite_yaml_file_create`) so they shift from `Result<Option<String>, ParseError>` (returning native SQL) to
    `Result<Option<ParseOutcome>, ParseError>` (returning the carrier). Move the catalog-reading bodies of
    `emit_native_create_sql` and `rewrite_drop_or_alter` into new helpers on the plan-function side:
    - New `plan_emit_native_create_sql(conn: duckdb_connection, ctx: PlanContext, pending: PendingCreate) -> Result<String, ParseError>` in `src/parse.rs` (or a new sibling module — planner discretion). Takes a live connection (the per-call ConnGuard's raw handle) and emits the final native SQL.
    - Similar `plan_emit_drop_or_alter_sql(conn, ctx, drop_or_alter_kind) -> Result<String, ParseError>`.
    - `plan_emit_yaml_file_create(conn, ctx, ...)` mirroring `rewrite_yaml_file_create`'s enrichment block (the `read_text` call moves here too).

    Step 4 — In `cpp/src/shim.cpp::sv_register_parser_hooks` (~lines 372-385), set `ext.parser_override = nullptr;`
    (do NOT pass `sv_parser_override`). Remove the FALLBACK_OVERRIDE config-set call
    (`config.SetOption("allow_parser_override_extension", ...)`) — irrelevant once parser_override is deregistered.
    Update the surrounding comment block to reference RESEARCH §16.6 #1 and the locked Option A architecture.

    Step 5 — Mark `sv_parser_override_rust` in `src/parse.rs` as `#[allow(dead_code)]` if Rust complains about the
    no-longer-called function. Do NOT delete it yet — Task 3 may reuse its `rewrite_to_native_sql` internals via the
    new `ParseOutcome` carrier. Add a doc-comment header: `// Phase 65 v0.9.1 Plan 02A: retained only as a reference
    for the structural rewrite logic; production caller is sv_parse_function_rust + sv_plan_semantic_view_ddl per
    RESEARCH §16.2.`

    Step 6 — `just build` and `just test-sql`. The expected post-Task-2 state: build succeeds; test-sql is STILL
    red because parse_function/plan_function don't yet handle the success path (Task 3's job). Record the exact
    failure message in summary for Task 3 to address.

    Commit message: `refactor(65-02): remove broken parse-time per-call ConnGuard surface (D-10)`.

    DO NOT proceed past this task if `just build` fails — that means the ParseOutcome refactor introduced a type
    error and Task 3 will not be able to consume the new shape.
  </action>
  <verify>
    <automated>just build 2>&1 | tee $TMPDIR/65_02_t2_build.log | tail -10 && ! grep -E "ConnGuard::open\(ctx\.db_handle\)" src/parse.rs && grep -E "ext\.parser_override\s*=\s*nullptr" cpp/src/shim.cpp</automated>
  </verify>
  <acceptance_criteria>
    - `just build` exits 0 (extension binary produced); the build is allowed to be functionally regressed for tests but MUST compile.
    - `grep -E "ConnGuard::open\(ctx\.db_handle\)" src/parse.rs` returns 0 matches (all 4 broken sites removed).
    - `grep -E "ext\.parser_override\s*=\s*nullptr" cpp/src/shim.cpp` returns ≥1 match (parser_override deregistered).
    - `grep -E 'config\.SetOption."allow_parser_override_extension' cpp/src/shim.cpp` returns 0 matches (FALLBACK_OVERRIDE config removed).
    - `cargo build --features extension --no-default-features` succeeds.
    - `cargo test --lib` exits 0 (Rust unit tests still green — parse-side structural code is exercised via existing tests like `sv_parse_function_rust_returns_*` AND the new `ParseOutcome` round-trip test).
    - `grep -E "pub enum ParseOutcome" src/parse.rs` returns ≥1.
    - `just test-sql` is EXPECTED to be RED at the end of this task — the regression is intentional and Task 3 restores it. The exact failure pattern is recorded in summary.
    - Single commit subject matches `refactor(65-02): remove broken parse-time per-call ConnGuard surface (D-10)`.
  </acceptance_criteria>
  <done>
    The known-broken surface is gone. `OverrideContext` carries `db_handle` (Plan 02 partial held) but nothing
    consumes `db_handle` at parse time anymore. The build still links. `ParseOutcome` carrier is in place. Task 3
    wires the new success path.
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: Promote sv_parse_function + sv_plan_function to the success path</name>
  <files>
    src/parse.rs
    cpp/src/shim.cpp
    src/lib.rs
  </files>
  <read_first>
    src/parse.rs
    cpp/src/shim.cpp
    cpp/include/parser_extension_compat.hpp
    .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
    .planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
    src/conn_guard.rs
    src/lib.rs
  </read_first>
  <behavior>
    - `sv_parse_function` (C++ symbol may stay `sv_parse_stub` or be renamed to `sv_parse_function` — recommend rename
      for clarity) returns `PARSE_SUCCESSFUL` with a populated `SemanticViewParseData` whenever the input is
      recognised semantic-view DDL. It still returns `DISPLAY_EXTENSION_ERROR` with caret position for invalid
      bodies and near-misses (preserve Phase 62 caret behaviour).
    - `SemanticViewParseData` gains a `payload: vector<uint8_t>` field alongside the existing `query: string` field;
      payload is an opaque (to C++) manual-LE-encoded snapshot per RESEARCH §16.4. `Copy()` clones both fields.
    - `sv_plan_function` (C++ symbol may stay `sv_plan_unreachable` or be renamed) receives
      `ClientContext &context, unique_ptr<ParserExtensionParseData> parse_data` and:
        (1) downcasts `parse_data` to `SemanticViewParseData`;
        (2) extracts `payload` bytes;
        (3) resolves `db: duckdb_database` via the new `sv_get_override_context_db_handle(rust_state)` FFI accessor;
        (4) calls `sv_plan_semantic_view_ddl(rust_state, payload_ptr, payload_len, db, error_buf, error_buf_len,
            native_sql_out, native_sql_len_out) -> u8`;
        (5) on rc=0 (success): drives the rewritten SQL into DuckDB per Task 1 decision (A2: `context.Query`; A1/A3:
            TableFunction parameters);
        (6) on rc=1 (error): throws `ParserException` / `BinderException` with caret position.
    - The Rust FFI `sv_plan_semantic_view_ddl` opens a per-call `ConnGuard::open(db)` on the bind thread (Task 0b
      proved BIND-THREAD-RC0; if BIND-THREAD-RC1 was observed, Task 1's escalate option was chosen and we are not
      here).
    - Phase 62 caret-rendering tests still pass: invalid CREATE bodies / near-misses still hit
      `DISPLAY_EXTENSION_ERROR` with `result.error_location = optional_idx(position)`.
    - All Phase 62 transactional DDL sqllogictests pass byte-identical (under A2) or under a documented
      transactional-DDL regression (under A1/A3, with TECH-DEBT entry).
  </behavior>
  <action>
    Step 1 — Augment `SemanticViewParseData` in `cpp/src/shim.cpp:116-126` with `vector<uint8_t> payload;` and update
    `Copy()` to clone both `query` and `payload`. Constructor signature becomes
    `SemanticViewParseData(string q, vector<uint8_t> p)`.

    Step 2 — Update `sv_parse_function_rust` in `src/parse.rs` (currently at ~`src/parse.rs:2722` onwards) to support
    a PARSE_SUCCESSFUL return code (rc=4 — extends the existing 0/1/2/3 contract documented at the function's doc
    comment). New signature adds `out_payload_ptr: *mut *mut u8, out_payload_len: *mut usize` out-params.
    When the input is recognised valid DDL:
    - Call the existing structural validation helpers (`detect_ddl_kind` + body parse) to produce a `ParseOutcome`
      (from Task 2's enum).
    - Serialize via `outcome.serialize_le()`.
    - Move the bytes onto the heap (`Box::into_raw(boxed_slice)`); write pointer+length to the out-params.
    - Return rc=4.
    On invalid: rc=1 + error_out + position_out (unchanged behaviour).

    Step 3 — Update C++ `sv_parse_stub` (or renamed `sv_parse_function`) at `cpp/src/shim.cpp:262-318` to:
    - Allocate stack buffers for error_buf, position, and the new payload pointer/length out-params.
    - Call `sv_parse_function_rust(ctx, query.c_str(), query.size(), error_buf, sizeof(error_buf), &position,
      &payload_ptr, &payload_len)`.
    - On rc=4 (new PARSE_SUCCESSFUL): copy `payload_ptr[0..payload_len]` into a `vector<uint8_t>`, free the Rust
      buffer via `sv_free_buffer`, construct `make_uniq<SemanticViewParseData>(query, std::move(payload_vec))`,
      and return `ParserExtensionParseResult(std::move(parse_data))`.
    - On rc=1, 2, 3: unchanged (error / defer / hint paths).

    Step 4 — Replace `sv_plan_unreachable` with the new `sv_plan_function` body in `cpp/src/shim.cpp`. Signature
    unchanged. Body:
    - `auto *sv_data = dynamic_cast<SemanticViewParseData *>(parse_data.get());` — if null, throw
      `InternalException` (defensive — sv_parse_function should always produce this concrete type).
    - Extract `query` and `payload` from `sv_data`.
    - `auto *sv_info = dynamic_cast<SemanticViewsParserInfo *>(info);` — if null/no rust_state, throw.
    - `duckdb_database db = sv_get_override_context_db_handle(sv_info->rust_state);` — new FFI accessor from Step 6.
    - Allocate `native_sql_out` / `native_sql_len_out` + `error_buf` buffers.
    - Call `sv_plan_semantic_view_ddl(sv_info->rust_state, payload.data(), payload.size(), db, error_buf,
      sizeof(error_buf), &native_sql_out, &native_sql_len_out)`.
    - On rc=1: throw `ParserException` with the error_buf message and the original query for caret rendering.
    - On rc=0: convert native_sql_out bytes into `string native_sql`; free Rust buffer; then:
        - **A2 path:** `auto result = context.Query(native_sql); if (result->HasError()) throw ...;` then
          construct a sentinel `ParserExtensionPlanResult` (register a tiny `__sv_plan_sentinel(view_name TEXT)`
          TableFunction in `sv_register_parser_hooks` that emits a single-row VARCHAR result whose value is the
          extracted view name from the rewritten SQL).

          **A2 sentinel/bind-time interaction (RESEARCH §16.2 row sentinel-design):** DuckDB calls `__sv_plan_sentinel`'s
          bind AFTER `sv_plan_function` returns its `ParserExtensionPlanResult`. By that point the DDL side-effect
          has already been executed via `context.Query(native_sql)` above — i.e., the catalog row has been
          inserted/dropped/altered on the caller's connection. The sentinel TableFunction's bind/func then return
          a single-row VARCHAR result containing the view name (the caller observes this as the statement's
          result), matching DuckDB's standard CREATE statement contract where CREATE returns a small success
          indicator rather than no rows. The sentinel does NOT re-do the DDL — it is purely a result-shape carrier
          so DuckDB's plan-binding pipeline has a TableFunction to bind. See RESEARCH §16.2 row "sentinel design"
          for the rationale on why a TableFunction stub is required by `ParserExtensionPlanResult`'s shape (the
          struct's `function` field is non-optional) and why a side-effect-plus-sentinel is the cleanest A2
          encoding given that constraint.
        - **A1 path:** construct `ParserExtensionPlanResult { __sv_execute_native, {Value(json_view_name),
          Value(native_sql)} }`. Register `__sv_execute_native` in `sv_register_parser_hooks` — its bind opens a
          connection via `ConnGuard::open(db)`, runs the SQL, projects a one-row result.
        - **A3 path:** dispatch on verb encoded in payload; emit verb-typed parameters into the matching
          `__sv_<verb>` TableFunction.

    Step 5 — Add the new Rust FFI entry in `src/parse.rs`:
    `pub unsafe extern "C" fn sv_plan_semantic_view_ddl(ctx_ptr: *const c_void, payload_ptr: *const u8, payload_len: usize, db: duckdb_database, error_buf: *mut u8, error_buf_len: usize, native_sql_out: *mut *mut u8, native_sql_len_out: *mut usize) -> u8`.
    Body:
    - panic-guard via `catch_unwind(AssertUnwindSafe(...))`.
    - Slice `payload` into a `&[u8]` and call `ParseOutcome::deserialize_le(bytes)`.
    - `let ctx = &*(ctx_ptr as *const OverrideContext);`
    - `let guard = ConnGuard::open(db).map_err(...)?;` — bind-thread `duckdb_connect` per Task 0b. If 0b returned
      BIND-THREAD-RC1, Task 1's escalate option should have been chosen.
    - Dispatch on `ParseOutcome`:
        - `Create { ... }`: call `plan_emit_native_create_sql(guard.raw(), ctx, pending)`.
        - `Drop { ... }` / `AlterRename { ... }` / `AlterComment { ... }`: call analogous helpers extracted from
          old `rewrite_drop_or_alter`.
        - `YamlFileCreate { ... }`: call `plan_emit_yaml_file_create(guard.raw(), ctx, ...)`.
        - `Describe { ... }` / `Show { ... }` / `List { ... }`: structural passthrough — emit `SELECT * FROM
          <read_side_table_function>(...)` directly without catalog reads.
    - Write the rewritten SQL bytes into a heap-allocated buffer via `Box::into_raw(boxed_slice)`; assign to
      out-params; return rc=0.
    - On error: write to `error_buf`; return rc=1.

    Step 6 — Add `sv_get_override_context_db_handle(ctx_ptr) -> duckdb_database` FFI accessor in `src/parse.rs`
    near `sv_make_override_context` / `sv_drop_override_context` (~`src/parse.rs:2541-2582`). Panic-guarded,
    returns null pointer on error.

    Step 7 — Wire the new accessor into `cpp/src/shim.cpp` (Step 4 above already references it).

    Step 8 — If Task 1 picked A1 or A3: register the `__sv_execute_native` / `__sv_<verb>` table functions in
    `sv_register_parser_hooks` (or a sibling init helper). Their bind/func use the per-bind ConnGuard pattern
    proved by Task 0b.

    Step 9 — Build + test:
    - `cargo build` (bundled) succeeds.
    - `cargo build --features extension --no-default-features` succeeds.
    - `just build` succeeds.
    - `just test-sql` exits 0 with the same pass count as the v0.9.0 tag. Compare log against
      `65-02-A7-test-sql-evidence.log` to confirm the 43 failures are healed.
    - `just test-caret` (Phase 62 caret tests) exits 0.
    - Save logs to `$TMPDIR/65_02_t3_test_sql.log` and `$TMPDIR/65_02_t3_test_caret.log`.

    Step 10 — If A1/A3 was picked: add a TECH-DEBT entry to the repo's `TECH-DEBT.md` describing the
    transactional-DDL regression. Format per existing TECH-DEBT entries. Title:
    `TECH-DEBT 25 - CREATE SEMANTIC VIEW inside user transaction no longer atomic (Phase 65 v0.9.1, A1/A3 path)`.
    Include forward-pointer to RESEARCH §16.2 + 65-02-SPIKES.md A2.

    Commit message: `feat(65-02): promote sv_parse_function + sv_plan_function to Option A success path` for the
    main commit. If TECH-DEBT entry added: separate commit
    `docs(65-02): file TECH-DEBT 25 for A1/A3 transactional regression`.
  </action>
  <verify>
    <automated>set -e; SPIKES=.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md; just build 2>&1 | tee $TMPDIR/65_02_t3_build.log | tail -5; just test-sql 2>&1 | tee $TMPDIR/65_02_t3_test_sql.log | tail -20; grep -qE "sv_plan_function|sv_plan_semantic_view_ddl" cpp/src/shim.cpp src/parse.rs; MECH=$(grep -E '^MECHANISM-CHOSEN:' "$SPIKES" | head -1 | awk '{print $2}'); echo "MECHANISM=$MECH"; case "$MECH" in A2) grep -qE 'context\.Query|context_query' cpp/src/shim.cpp && ! grep -q 'TECH-DEBT 25' TECH-DEBT.md ;; A1|A3) grep -q 'TECH-DEBT 25' TECH-DEBT.md ;; *) echo "MECHANISM-CHOSEN marker missing or invalid in $SPIKES" >&2; exit 1 ;; esac</automated>
  </verify>
  <acceptance_criteria>
    - `cargo build` (bundled) succeeds.
    - `cargo build --features extension --no-default-features` succeeds.
    - `just build` succeeds.
    - `just test-sql` exits 0 with the same pass count as the v0.9.0 tag (pre-Plan-02 baseline). Verify via diff
      against `65-02-A7-test-sql-evidence.log`: every test listed as failing in the evidence file now passes.
      Record the diff result in summary.
    - `just test-caret` exits 0 (Phase 62 caret tests, both `LINE 1: ... ^` rendering for CREATE syntax errors
      AND near-miss `CRETAE`-style typos).
    - `SemanticViewParseData` in `cpp/src/shim.cpp` has both `query: string` and `payload: vector<uint8_t>` fields.
      Verify with `grep -E "vector<uint8_t>\s+payload|payload\(std::move\(p\)\)" cpp/src/shim.cpp` ≥1.
    - `sv_plan_function` (or sv_plan_unreachable replaced) is registered as `ext.plan_function` in
      `sv_register_parser_hooks` AND its body calls `sv_plan_semantic_view_ddl`. Verify with
      `grep -E "sv_plan_semantic_view_ddl" cpp/src/shim.cpp` ≥1.
    - `sv_plan_semantic_view_ddl` extern "C" entry exists in `src/parse.rs`. Verify with
      `grep -E 'pub unsafe extern "C" fn sv_plan_semantic_view_ddl' src/parse.rs` ≥1.
    - `sv_get_override_context_db_handle` extern "C" entry exists in `src/parse.rs`. Verify with
      `grep -E 'pub unsafe extern "C" fn sv_get_override_context_db_handle' src/parse.rs` ≥1.
    - **Mechanism-branched assertion (from `<verify>` `MECHANISM-CHOSEN:` marker in `65-02-SPIKES.md`):**
        - If `MECHANISM-CHOSEN: A2`: `grep -qE 'context\.Query|context_query' cpp/src/shim.cpp` passes (A2 path uses
          `context.Query`) AND `! grep -q 'TECH-DEBT 25' TECH-DEBT.md` (no TECH-DEBT 25 needed — transactional
          semantics preserved).
        - If `MECHANISM-CHOSEN: A1` OR `MECHANISM-CHOSEN: A3`: `grep -q 'TECH-DEBT 25' TECH-DEBT.md` passes (the
          transactional-DDL-regression TECH-DEBT entry is mandatory under A1/A3, per CONTEXT.md D-01 and the
          checkpoint:human-verify in Task 4).
        - The marker MUST be present in `65-02-SPIKES.md`; if missing the verify fails with `MECHANISM-CHOSEN marker
          missing or invalid` — Task 1's resume-signal told the user to append the marker, so absence indicates
          Task 1 was skipped or recorded incorrectly.
    - PHASE-65-GUARD marker comment still present inside OverrideContext struct body (Plan 04's B13 test depends on
      it). Verify with `grep -F 'PHASE-65-GUARD: do not reintroduce duckdb_connection or CatalogReader field here.' src/parse.rs` ≥1.
  </acceptance_criteria>
  <done>
    sv_parse_function returns PARSE_SUCCESSFUL with a SemanticViewParseData carrier; sv_plan_function performs
    catalog reads on a per-call ConnGuard derived from `OverrideContext.db_handle`; rewrites are emitted via the
    mechanism locked by Task 1's checkpoint:decision (A2 / A1 / A3); Phase 62 transactional DDL + caret tests are
    GREEN again. `just test-sql` flips from 4/47 PASS (Plan 02 partial baseline) to the same pass count as v0.9.0.
  </done>
</task>

<task type="checkpoint:human-verify" gate="blocking">
  <name>Task 4: Human verification — A2 transactional semantics OR A1/A3 TECH-DEBT acknowledgement</name>
  <what-built>
    Plan 02 replaced the parse-time per-call ConnGuard surface with the Option A bind/plan-time architecture:
    `sv_parser_override` deregistered, `sv_parse_function` returns PARSE_SUCCESSFUL with a SemanticViewParseData
    carrier, and `sv_plan_function` performs catalog reads on a per-call ConnGuard derived from ClientContext.
    The 43/47 sqllogictest failure state recorded in `65-02-A7-test-sql-evidence.log` is healed.
  </what-built>
  <how-to-verify>
    1. If Task 1 picked A2 (transactional path preserved):
       a. Run `just test-sql` and confirm the same pass count as the v0.9.0 tag.
       b. Run `just test-caret` and confirm GREEN.
       c. Run a manual transactional smoke test in a fresh Python REPL: connect, LOAD semantic_views,
          CREATE TABLE t (i INT), BEGIN, CREATE SEMANTIC VIEW v AS ..., ROLLBACK, then assert
          `list_semantic_views()` returns empty. Approve only if the assert holds — CREATE inside BEGIN/ROLLBACK
          must be rolled back (transactional DDL).
       d. Confirm NO new entry in `TECH-DEBT.md` (`git diff TECH-DEBT.md` is empty).

    2. If Task 1 picked A1 or A3 (transactional regression accepted):
       a. Run `just test-sql` and confirm pass count matches v0.9.0 EXCEPT for any test that exercises CREATE-then-
          ROLLBACK on a semantic view (those will now leave the view persistent — expected under A1/A3).
       b. Read the new `TECH-DEBT 25` entry in `TECH-DEBT.md`. Confirm it explicitly states:
          - The transactional regression (CREATE inside user txn no longer atomic).
          - The trigger (A1/A3 path was forced because A2 spike returned A2-DEADLOCK or A2-UNEXPECTED).
          - The forward direction (revisit when DuckDB 1.6+ exposes a context.Query path that does not deadlock,
            OR replace ParserExtension with StorageExtension entirely per RESEARCH §16.5).
       c. Confirm the regression is documented in the v0.9.1 CHANGELOG.md draft (or note for Phase 66 to add).

    3. Either way, confirm the in-process test (`uv run test/integration/test_readonly_load.py`) still shows
       B1..B4 + B11 FAILING (they fail on baseline; they will not flip green until Plan 03 removes H2 query_conn).
       Do NOT expect them to pass yet.
  </how-to-verify>
  <resume-signal>Type "approved" to proceed to Plan 03, or describe specific issues (failing test, missing TECH-DEBT, broken transactional behaviour).</resume-signal>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Rust ↔ C++ shim | `sv_parse_function_rust` returns a heap-owned payload buffer that C++ copies into `SemanticViewParseData`; `sv_free_buffer` releases the Rust allocation. New `sv_plan_semantic_view_ddl` returns a heap-owned native-SQL buffer the same way. |
| C++ `sv_plan_function` ↔ DuckDB `ClientContext` | Under A2, calling `context.Query(native_sql)` from inside `Binder::Bind(ExtensionStatement&)` may re-enter `context_lock` (per RESEARCH §16.2). The Task 0a spike is the empirical discriminator. |
| Bind-thread ↔ DuckDB `ConnectionManager` | New `duckdb_connect` call from inside `sv_plan_function` opens a per-call `Connection` against the same `DatabaseInstance`. Task 0b verifies rc=0. |
| OverrideContext.db_handle pointer | A non-owning `duckdb_database` pointer that survives across parse → plan → execution. Stays valid for the lifetime of `DBConfig` per RESEARCH §3.3. |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-65-04 | Tampering | Rust→C++ payload buffer (`sv_parse_function_rust` → `SemanticViewParseData::payload`) — wrong free function or double-free | mitigate | Reuse `sv_free_buffer` (existing Phase 62 pattern); C++ side copies into `vector<uint8_t>` then immediately frees the Rust buffer; ownership is transferred at the copy boundary. |
| T-65-05 | Denial of Service | `context.Query(native_sql)` deadlock under A2 | mitigate via spike-first | Task 0a A2 spike is BLOCKING; checkpoint:decision (Task 1) gates the production wiring on the spike result. If A2 deadlocks, A1/A3 path is taken with TECH-DEBT 25 entry and user approval (checkpoint:human-verify Task 4). |
| T-65-06 | Tampering | `OverrideContext.db_handle` used post-`~DBConfig` UAF | accept | `OverrideContext` Box is freed inside `~SemanticViewsParserInfo` which fires DURING `~DBConfig`; by the time `db_handle` could be dangling, no parse/plan/bind callback can fire (the parser_info is gone). Same lifetime guarantee as Plan 02 partial. |
| T-65-07 | Information Disclosure | Manual LE encoding of payload may expose field-order metadata to attackers reading `SemanticViewParseData::payload` | accept | Payload is internal to the extension; no external surface; no PII. |
| T-65-08 | Repudiation | If A1/A3 path is taken and TECH-DEBT 25 is not filed | mitigate | Task 3 Step 10 + Task 4 acceptance criteria check for the entry; checkpoint:human-verify reviews it. |
| T-65-SC | Tampering | No new package installs in this plan | accept | Plan 02 modifies in-tree Rust + C++ only; no new crates added. Cargo.toml unchanged. No legitimacy gate required. |
</threat_model>

<verification>
After all tasks in this plan complete:

1. `cargo build` and `cargo build --features extension --no-default-features` both succeed.
2. `just build` produces the extension binary.
3. `just test-sql` exits 0 with the same pass count as the v0.9.0 tag — the 43/47 failure recorded in
   `65-02-A7-test-sql-evidence.log` is healed.
4. `just test-caret` exits 0 (Phase 62 caret tests preserved).
5. `grep -E "ext\.parser_override\s*=\s*nullptr" cpp/src/shim.cpp` returns ≥1 match (sv_parser_override deregistered).
6. `grep -E "sv_plan_semantic_view_ddl" cpp/src/shim.cpp src/parse.rs` returns ≥2 matches (C++ caller + Rust definition).
7. `grep -E "ConnGuard::open\(ctx\.db_handle\)" src/parse.rs` returns 0 matches (the 4 broken sites are gone).
8. `grep -F 'PHASE-65-GUARD: do not reintroduce duckdb_connection or CatalogReader field here.' src/parse.rs` ≥1.
9. `65-02-SPIKES.md` exists with `## A2` and `## A6-bind` sections, each with a conclusion line, plus a top-level
   `MECHANISM-CHOSEN:` marker line set to one of `A1` / `A2` / `A3`.
10. If A1/A3 path: `TECH-DEBT 25` entry present in `TECH-DEBT.md`; if A2 path: no new entry.
11. In-process tests (B1..B4 + B11) STILL FAIL — they wait on Plan 03 removing the H2 `query_conn`. This is
    expected and NOT a Plan 02 regression.
</verification>

<success_criteria>
- The two Wave-0 spikes (A2 viability, bind-thread duckdb_connect) ran BEFORE any production refactor; outcomes
  are recorded in `65-02-SPIKES.md`.
- The 4× broken `ConnGuard::open(ctx.db_handle)` call sites in `parse.rs::rewrite_*` are removed.
- `sv_parser_override` is deregistered (`ext.parser_override = nullptr`).
- `sv_parse_function` promoted to success path, returning PARSE_SUCCESSFUL with `SemanticViewParseData` carrier.
- `sv_plan_function` promoted to catalog-read + emission entry, deriving a per-call `ConnGuard` via the
  `OverrideContext.db_handle` FFI accessor.
- Phase 62 transactional DDL + caret tests stay green (A2) or under documented TECH-DEBT 25 (A1/A3).
- Plan 02 leaves `milestone/v0.9.1` in a state where `just test-sql` is back to the v0.9.0 baseline pass count.
</success_criteria>

<output>
Create `.planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md` (OVERWRITING the existing
`PARTIAL` summary). Summary MUST include:
- The two Wave-0 spike outcomes (A2 conclusion, A6-bind conclusion) with verbatim evidence pointers.
- The Task 1 checkpoint:decision outcome (a2-clean / a1-extra-tf / a3-typed-per-verb / escalate) and the
  `MECHANISM-CHOSEN:` marker value written to `65-02-SPIKES.md`.
- Final `sv_parse_function` rc contract (including new rc=4 PARSE_SUCCESSFUL).
- Final `sv_plan_function` mechanism description per the locked option.
- `SemanticViewParseData` final field list and Copy() shape.
- Path to the `just test-sql` log proving the v0.9.0-baseline pass count is restored.
- If A1/A3 path: TECH-DEBT 25 entry verbatim + forward-pointer to resolution.
- Count of `ConnGuard::open` call sites in `src/parse.rs` (should be 0 inside `rewrite_*`).
- Any deviation from the planner's prescribed shape and why.
</output>
</content>
</invoke>