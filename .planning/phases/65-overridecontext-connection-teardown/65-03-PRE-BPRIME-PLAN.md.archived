---
phase: 65-overridecontext-connection-teardown
plan: 03
type: execute
wave: 3
depends_on:
  - 65-02
files_modified:
  - src/lib.rs
  - src/catalog.rs
  - src/query/table_function.rs
  - src/query/explain.rs
files_audited:
  - src/ddl/list.rs
  - src/ddl/describe.rs
  - src/ddl/show_columns.rs
  - src/ddl/show_dims.rs
  - src/ddl/show_dims_for_metric.rs
  - src/ddl/show_metrics.rs
  - src/ddl/show_facts.rs
  - src/ddl/show_materializations.rs
  - src/ddl/get_ddl.rs
  - src/ddl/read_yaml.rs
autonomous: false
requirements:
  - LIFE-01
  - LIFE-02
tags:
  - duckdb
  - rust
  - ffi
  - table-function
  - read-side
  - lifecycle
  - h2-removal

must_haves:
  truths:
    - "D-11 (continuation): the read-side bind callbacks open per-bind `ConnGuard` from inside the bind thread — Plan 02 Task 0b spike confirmed `BIND-THREAD-RC0` (rc=0 from `duckdb_connect` inside a read-side bind), so this plan is unblocked"
    - "RESEARCH §16.3 / §16.6 #4: shape (b) — `CatalogReader` refactored to carry `db: duckdb_database` and open its own ConnGuard per method — keeps the 13 read-side `register_table_function_with_extra_info` sites + 2 scalar `invoke` sites SYNTACTICALLY UNCHANGED at the call sites. Per-file audit (Step 2 of Task 3) asserts zero changes were required across the 10 src/ddl/*.rs files."
    - "`src/lib.rs::init_extension` no longer calls `duckdb_connect` for either `catalog_conn` (H1) or `query_conn` (H2). Both long-lived connections are eliminated. Verify with `grep -nE 'duckdb_connect\\(' src/lib.rs` returns 0 matches inside the `init_extension` function body."
    - "`CatalogReader` (shape b) carries `db: duckdb_database` instead of `conn: duckdb_connection`; each public method opens+closes a `ConnGuard` internally. `raw()` accessor is removed."
    - "`QueryState` carries `db_handle: duckdb_database` + `catalog_table_present: bool`; no cached connection. Send + Sync preserved with updated SAFETY comments."
    - "`SemanticViewBindData` owns a `ConnGuard` constructed in `bind` and dropped when DuckDB destroys the bind data; `func()` calls `execute_sql_raw(bind_data.conn_guard.raw(), ...)` instead of `state.conn`."
    - "`explain_semantic_view` mirrors the pattern — if it does no SQL execution, only the bind-side `CatalogReader::new(state.db_handle, ...)` change is needed; otherwise its bind data also gains a `conn_guard` field."
    - "B1..B4 + B11 tests in `test/integration/test_readonly_load.py` (planted by Plan 01) now PASS — the in-process RW→RO reopen returns in <5s on both fresh and previously-bootstrapped DBs. This is the LIFE-01 SC-3 evidence."
    - "Phase 62 transactional DDL, caret, multi-DB, concurrent, and ADBC tests stay green byte-identical."
    - "TECH-DEBT 19 (DESCRIBE/SHOW see only committed state) is NOT regressed by this plan — per-call ConnGuard still sees committed state only. RESEARCH §9.2 makes this explicit; record in summary."
  artifacts:
    - path: "src/catalog.rs"
      provides: "CatalogReader shape (b): db: duckdb_database; per-method ConnGuard"
      contains: "db: ffi::duckdb_database"
    - path: "src/query/table_function.rs"
      provides: "QueryState carries db_handle + catalog_table_present; SemanticViewBindData owns per-query ConnGuard"
      contains: "conn_guard"
    - path: "src/lib.rs"
      provides: "init_extension no longer owns H1 or H2 long-lived duckdb_connection"
      contains: "register_table_function_with_extra_info"
  key_links:
    - from: "src/lib.rs::init_extension"
      to: "src/catalog.rs::CatalogReader::new(db_handle, catalog_table_present)"
      via: "extra_info registration (shape b — no conn opened at init time)"
      pattern: "CatalogReader::new"
    - from: "src/query/table_function.rs::bind"
      to: "src/conn_guard.rs::ConnGuard"
      via: "ConnGuard::open(state.db_handle) stored on SemanticViewBindData"
      pattern: "ConnGuard::open"
---

<objective>
Remove the second long-lived `duckdb_connection` (H2 / `query_conn`) AND the first (H1 / `catalog_conn` — left
intact by Plan 02 because the 13 read-side + 2 scalar registrations still consumed it). Refactor `CatalogReader`
to shape (b) (carries `db: duckdb_database`, opens ConnGuard per method); `QueryState` to carry `db_handle` +
`catalog_table_present`; `SemanticViewBindData` to own a per-query `ConnGuard` so `execute_sql_raw` keeps using
one stable connection for the duration of the query.

Purpose: After Plan 02 retired the parser_override surface via the bind/plan-time architecture, `catalog_conn` (H1)
is still alive in `init_extension` because the 14 read-side table functions and 2 scalars hold its pointer copy via
`extra_info`. `query_conn` (H2) is the remaining `shared_ptr<DatabaseInstance>` keeper used by `semantic_view`
and `explain_semantic_view`. Both must be eliminated for the in-process RW→RO reopen to return in <5s. After this
plan lands, ZERO extension-owned `duckdb_connection` handles exist at extension-LOAD scope; all connections are
per-bind / per-call transients.

Output: Zero long-lived extension-owned `duckdb_connection` handles. LIFE-01 satisfied (B1..B4 + B11 flip green).
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT.md
@.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
@.planning/phases/65-overridecontext-connection-teardown/65-PATTERNS.md
@.planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md
@.planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md
@.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
@CLAUDE.md

<interfaces>
Current CatalogReader (src/catalog.rs:97-170) — being refactored to shape (b):
- struct CatalogReader { conn: ffi::duckdb_connection, catalog_table_present: bool } with derive(Clone, Copy)
- methods: new(conn, present), raw() -> conn, lookup, list_all, list_names, exists
- unsafe impl Send + Sync (SAFETY: opaque pointer; DuckDB synchronises internally)
- Short-circuit pattern: `if !self.catalog_table_present { return Ok(None); }` at the start of lookup / list_all /
  list_names (the RO-04 fast path for read-only host DBs without the semantic_layer._definitions table).

Target shape (b):
- struct CatalogReader { db: ffi::duckdb_database, catalog_table_present: bool } with derive(Clone, Copy)
- new(db, present)
- raw() REMOVED
- each public method opens ConnGuard::open(self.db)? then calls existing free-function helper
  (prepared_lookup / execute_list_all / execute_list_names / prepared_exists) with guard.raw()
- guard drops at method return -> duckdb_disconnect

Current QueryState (src/query/table_function.rs:32-43):
- struct QueryState { catalog: CatalogReader, conn: ffi::duckdb_connection } with derive(Clone)
- unsafe impl Send + Sync

Target QueryState:
- struct QueryState { db_handle: ffi::duckdb_database, catalog_table_present: bool } with derive(Clone, Copy)
- unsafe impl Send + Sync preserved (SAFETY comment updated)

Current SemanticViewBindData (src/query/table_function.rs:49-68):
- struct SemanticViewBindData { expanded_sql, execution_sql, column_names, column_type_ids }
- unsafe impl Send + Sync (all fields are Send+Sync)
- NO Clone derive today

Target SemanticViewBindData (Phase 65):
- struct SemanticViewBindData { expanded_sql, execution_sql, column_names, column_type_ids, conn_guard: ConnGuard }
- still unsafe impl Send + Sync (ConnGuard is Send-not-Sync, but SemanticViewBindData itself is owned exclusively
  by DuckDB's bind-data lifecycle so the !Sync of ConnGuard is fine — the outer Sync impl is for the bind-data
  pointer not for shared-borrow access)
- Drop chain naturally closes ConnGuard when DuckDB drops bind data

13 read-side register sites (src/lib.rs:425-486):
ListSemanticViewsVTab, ListTerseSemanticViewsVTab, ShowColumnsInSemanticViewVTab, DescribeSemanticViewVTab,
ShowSemanticDimensionsVTab, ShowSemanticDimensionsAllVTab, ShowDimensionsForMetricVTab,
ShowSemanticMetricsVTab, ShowSemanticMetricsAllVTab, ShowSemanticFactsVTab, ShowSemanticFactsAllVTab,
ShowSemanticMaterializationsVTab, ShowSemanticMaterializationsAllVTab

2 scalar registrations (src/lib.rs:489-495):
GetDdlScalar (type State = CatalogReader; calls state.lookup(...))
ReadYamlFromSemanticViewScalar (type State = CatalogReader; calls state.lookup(...))

ConnGuard (src/conn_guard.rs — Plan 01):
- pub(crate) struct ConnGuard { conn: ffi::duckdb_connection }
- pub(crate) unsafe fn open(db: ffi::duckdb_database) -> Result<Self, String>
- pub(crate) fn raw(&self) -> ffi::duckdb_connection
- impl Drop calls duckdb_disconnect on non-null
- unsafe impl Send (no Sync, no Clone)
- Today carries #[allow(dead_code)] / #[allow(unused_imports)] — Plan 04 Task 1 removes those once production
  consumes the API (this plan IS the production consumer).
</interfaces>
</context>

<tasks>

<task type="checkpoint:human-verify" gate="blocking">
  <name>Task 0: Confirm Plan 02 Task 0b returned BIND-THREAD-RC0 in 65-02-SPIKES.md</name>
  <what-built>
    Plan 02 ran two Wave-0 spikes. The bind-thread `duckdb_connect` rc check (Task 0b) recorded its outcome in
    `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` under `## A6-bind`. This plan's entire
    read-side architecture (shape (b): `CatalogReader { db: duckdb_database }` + per-method ConnGuard inside the
    bind callbacks of 13 read-side table functions + 2 scalars) is gated on that spike returning `BIND-THREAD-RC0`.
    If the spike returned `BIND-THREAD-RC1`, shape (b) is empirically impossible and this plan cannot proceed
    autonomously — a fresh `checkpoint:decision` is required.
  </what-built>
  <how-to-verify>
    1. Open `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` and find the `## A6-bind`
       section.

    2. Locate the conclusion line — it MUST be exactly one of:
       - `BIND-THREAD-RC0` — `duckdb_connect` from inside the bind callback returns rc=0. Plan 03 is unblocked.
       - `BIND-THREAD-RC1` — `duckdb_connect` from inside the bind callback returns rc=1 (same failure mode as
         parse-thread). Plan 03 cannot proceed under shape (b).

    3. Run this automated check to confirm the marker is present:
       ```
       grep -E '^BIND-THREAD-RC0|^BIND-THREAD-RC1|^- BIND-THREAD-RC0|^- BIND-THREAD-RC1|BIND-THREAD-RC0$|BIND-THREAD-RC1$' \
         .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
       ```
       Exactly one match expected.

    4. Action by outcome:
       - **BIND-THREAD-RC0 confirmed:** type `approved` and Plan 03 proceeds with Tasks 1-3 as written.
       - **BIND-THREAD-RC1 confirmed:** type `halt — BIND-THREAD-RC1` (or describe the exact marker found). HALT
         this plan. Surface the constraint to the user: "Plan 03 cannot proceed under shape (b) because bind-thread
         `duckdb_connect` is also rc=1. A fresh `checkpoint:decision` is required for the read-side architecture —
         options include (i) scalar-function workaround with custom State carrying `(CatalogReader, db_handle)` and
         ConnGuard opened inside `invoke` rather than `bind`, (ii) documented limitation keeping the long-lived
         `query_conn` for v0.9.1 with TECH-DEBT for v0.9.2, or (iii) re-research per RESEARCH §16.6 #8 follow-ups."
         Do NOT proceed to Task 1 in this case — the executor should exit and request the user re-plan via
         `/gsd:plan-phase --reviews` or `/gsd:discuss-phase --assumptions`.
       - **Marker missing or malformed:** type `halt — marker missing`. Plan 02 did not complete its Task 0b
         spike. Halt and instruct the user to re-run Plan 02 Task 0b.

    5. If approved: confirm `git branch --show-current` returns `milestone/v0.9.1` before proceeding.
  </how-to-verify>
  <resume-signal>Type "approved" only if `## A6-bind` section contains exactly `BIND-THREAD-RC0`. Otherwise type `halt — BIND-THREAD-RC1` or `halt — marker missing` and surface the constraint per step 4 above.</resume-signal>
</task>

<task type="auto" tdd="true">
  <name>Task 1: Refactor CatalogReader to shape (b) — db: duckdb_database + per-method ConnGuard</name>
  <files>
    src/catalog.rs
  </files>
  <read_first>
    src/catalog.rs
    src/conn_guard.rs
    .planning/phases/65-overridecontext-connection-teardown/65-PATTERNS.md
    .planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
  </read_first>
  <behavior>
    - `CatalogReader`'s only data field changes from `conn: ffi::duckdb_connection` to `db: ffi::duckdb_database`.
      `catalog_table_present: bool` stays.
    - Constructor signature becomes `pub fn new(db: ffi::duckdb_database, catalog_table_present: bool) -> Self`.
    - `lookup`, `list_all`, `list_names`, `exists` each:
        (1) preserve the existing short-circuit on `!self.catalog_table_present` (returns Ok(None) / Ok(vec![]) /
            Ok(false));
        (2) open `let guard = unsafe { crate::conn_guard::ConnGuard::open(self.db) }.map_err(|e| e)?;`
        (3) call the existing free-function helper (`prepared_lookup`, `execute_list_all`, etc.) with `guard.raw()`;
        (4) let the guard drop at method return (duckdb_disconnect fires).
    - `raw()` accessor is REMOVED. Any caller that still needs the raw connection (none should after Plan 02 +
      this task) must own its own ConnGuard.
    - `Clone + Copy` derives preserved (db is a pointer, Copy is correct).
    - `Send + Sync` impls preserved with updated SAFETY comments.
    - Internal helpers (`prepared_lookup` at `src/catalog.rs:255`, `execute_list_all` at `src/catalog.rs:287`,
      `execute_list_names` at `src/catalog.rs:313`, `prepared_exists` near the other helpers) continue to accept
      `conn: ffi::duckdb_connection` and remain unchanged.
    - Existing unit tests for the short-circuit fast path (`lookup_returns_none_when_table_missing` at
      `src/catalog.rs:584`, `list_all_returns_empty_when_table_missing` at line 601, `list_names_returns_empty_when_table_missing`
      at line 615) still pass — they construct `CatalogReader::new(<some_conn>, false)`. After the field swap they
      construct `CatalogReader::new(<some_db>, false)`; the short-circuit ensures no FFI is touched even if
      `<some_db>` is null.
  </behavior>
  <action>
    Step 1 — In `src/catalog.rs`, replace the `conn: ffi::duckdb_connection` field on `CatalogReader` (~line 99)
    with `db: ffi::duckdb_database`. Keep `catalog_table_present: bool` and `#[derive(Clone, Copy)]`.

    Step 2 — Update `CatalogReader::new` (~line 115) signature to
    `pub fn new(db: ffi::duckdb_database, catalog_table_present: bool) -> Self`.

    Step 3 — Remove `pub fn raw(&self) -> ffi::duckdb_connection` (~line 122). Verify no remaining caller of
    `CatalogReader::raw()` exists via `grep -rn "\.raw()" src/ | grep -E "catalog|reader"` — there should be none
    (Plan 02 already removed the parse.rs callers). Any remaining match is a Plan 03 bug.

    Step 4 — For each public method inside `impl CatalogReader` (`lookup`, `list_all`, `list_names`, `exists`):
    - Preserve the existing `if !self.catalog_table_present { return Ok(<empty-shape>); }` short-circuit.
    - Open `let guard = unsafe { crate::conn_guard::ConnGuard::open(self.db) }.map_err(|e| e)?;` (or with the
      specific error-mapping the method already uses — current pattern uses `String` errors directly).
    - Replace any existing `self.conn` reference inside the method body with `guard.raw()` passed into the existing
      free-function helper. For example, `unsafe { prepared_lookup(self.conn, name) }` becomes
      `unsafe { prepared_lookup(guard.raw(), name) }`.

    Step 5 — Update SAFETY comments on `unsafe impl Send for CatalogReader` and `unsafe impl Sync for CatalogReader`
    (~line 108-112) to reflect the new shape: "db is a non-owning duckdb_database pointer. The underlying
    DatabaseInstance owns its own synchronisation; duckdb_connect / duckdb_disconnect are documented thread-safe
    against a shared duckdb_database. The per-call ConnGuard isolates per-thread reads."

    Step 6 — Update the existing `#[cfg(not(feature = "extension"))]` short-circuit unit tests
    (`lookup_returns_none_when_table_missing` and siblings at ~line 584-630) to construct
    `CatalogReader::new(std::ptr::null_mut(), false)` (the null pointer is fine because the short-circuit returns
    before ConnGuard::open is reached). Verify the tests still pass under `cargo test --lib`.

    Step 7 — Add ONE new unit test inside `#[cfg(test)] mod tests`:
    `catalog_reader_with_present_table_attempts_conn_open` (gated on `#[cfg(feature = "extension")]`) — constructs
    a `CatalogReader::new(some_real_db, true)` against a real `duckdb::Connection::open_in_memory()`-derived
    `duckdb_database` and calls `lookup("nonexistent")` to exercise the full open → query → drop path. Asserts
    `Ok(None)` (lookup returns None for missing names without erroring).

    Step 8 — Build:
    - `cargo build` (bundled) succeeds.
    - `cargo build --features extension --no-default-features` succeeds.
    - `cargo test --lib` exits 0.
    - `cargo test --lib --features extension --no-default-features` exits 0.
  </action>
  <verify>
    <automated>cargo build 2>&1 | tail -5 && cargo build --features extension --no-default-features 2>&1 | tail -5 && grep -E "db: (ffi::|libduckdb_sys::)?duckdb_database" src/catalog.rs && ! grep -E "pub fn raw\s*\(&self\)\s*->\s*ffi::duckdb_connection" src/catalog.rs</automated>
  </verify>
  <acceptance_criteria>
    - `CatalogReader` struct's data field is `db: ffi::duckdb_database`. Verify with
      `grep -E "db: (ffi::|libduckdb_sys::)?duckdb_database" src/catalog.rs` ≥1.
    - Old `conn: ffi::duckdb_connection` field is gone from CatalogReader. Verify with
      `grep -nE "conn: ffi::duckdb_connection" src/catalog.rs | grep -v PreparedStmt | grep -v QueryResult`
      returns no match inside the `struct CatalogReader` block.
    - `raw()` accessor removed: `grep -E "pub fn raw\s*\(&self\)\s*->\s*ffi::duckdb_connection" src/catalog.rs`
      returns 0 matches.
    - Per-method ConnGuard usage: `grep -c "ConnGuard::open" src/catalog.rs` ≥4 (lookup, list_all, list_names, exists).
    - Short-circuit preserved: `grep -c "if !self.catalog_table_present" src/catalog.rs` ≥4.
    - `cargo build` and `cargo build --features extension --no-default-features` both succeed.
    - `cargo test --lib` exits 0 (existing tests + new `catalog_reader_with_present_table_attempts_conn_open`).
    - `cargo test --lib --features extension --no-default-features` exits 0.
  </acceptance_criteria>
  <done>
    CatalogReader is shape (b). Internal free-function helpers unchanged. Existing short-circuit tests pass against
    the null-pointer constructor. The 13 read-side bind callbacks + 2 scalars can consume the new shape without any
    per-file change (Task 3 verifies this via grep + git-diff audit).
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 2: Refactor QueryState + SemanticViewBindData for per-query ConnGuard; mirror in explain</name>
  <files>
    src/query/table_function.rs
    src/query/explain.rs
  </files>
  <read_first>
    src/query/table_function.rs
    src/query/explain.rs
    src/conn_guard.rs
    src/catalog.rs
    .planning/phases/65-overridecontext-connection-teardown/65-PATTERNS.md
  </read_first>
  <behavior>
    - `QueryState` field set becomes `{ db_handle: duckdb_database, catalog_table_present: bool }` with derive
      `Clone, Copy` (today only Clone is derived; add Copy since both fields are Copy). `Send + Sync` preserved.
    - In the `bind` callback (~`src/query/table_function.rs:516` area), after reading `state` via
      `bind.get_extra_info::<QueryState>`, construct a fresh `ConnGuard::open(state.db_handle)`; construct a
      transient `CatalogReader::new(state.db_handle, state.catalog_table_present)` for the JSON lookup;
      after building the `execution_sql`, MOVE the `ConnGuard` into the new `SemanticViewBindData::conn_guard`
      field so the connection survives until DuckDB destroys the bind data.
    - `SemanticViewBindData` gains a `conn_guard: ConnGuard` field. Do NOT add `Clone` (ConnGuard is intentionally
      not Clone); audit existing code to ensure bind data is never cloned (today there is no Clone derive — verify).
    - In `func()` (~`src/query/table_function.rs:763-773`), replace `state.conn` with `bind_data.conn_guard.raw()`
      for the `execute_sql_raw` call. If `state` is no longer referenced in `func()` after this change, remove the
      `state` lookup entirely.
    - When DuckDB drops `SemanticViewBindData`, Rust's Drop chain naturally closes the `ConnGuard` →
      `duckdb_disconnect` fires.
    - `explain_semantic_view` (`src/query/explain.rs`): apply the same QueryState shape change. If
      `explain_semantic_view` only reads the catalog (no SQL execution), the bind-callback transient
      `CatalogReader::new(state.db_handle, ...)` + the reader's internal per-method ConnGuard is sufficient — no
      per-query `conn_guard` in its bind data. If it does execute SQL, mirror the bind-data ConnGuard pattern.
      Read the file first to determine which path applies.
    - All existing semantic_view query results are identical (sqllogictest evidence in summary).
  </behavior>
  <action>
    Step 1 — In `src/query/table_function.rs` (~lines 32-43), replace `QueryState`:
    - Remove `pub catalog: CatalogReader` and `pub conn: ffi::duckdb_connection`.
    - Add `pub db_handle: ffi::duckdb_database` and `pub catalog_table_present: bool`.
    - Change `#[derive(Clone)]` to `#[derive(Clone, Copy)]`.
    - Update the SAFETY comment block above `unsafe impl Send / Sync` to: "QueryState holds a non-owning
      duckdb_database pointer + a bool. DuckDB manages internal synchronisation on the underlying DatabaseInstance.
      Per-query ConnGuards (held on SemanticViewBindData) isolate per-query connection ownership."

    Step 2 — Add `conn_guard` field to `SemanticViewBindData` (~`src/query/table_function.rs:50-64`):
    - New field: `pub(crate) conn_guard: crate::conn_guard::ConnGuard,` with a doc comment "Per-query connection.
      Lives for the duration of bind→func→destroy; Drop closes it (Phase 65 H2 removal)."
    - Audit the file for `Clone` on `SemanticViewBindData` — there is no derive today, so no change needed. If any
      `.clone()` call on bind data appears (search via `grep -nE "SemanticViewBindData.*\.clone\(\)" src/`),
      refactor.
    - The existing `unsafe impl Send + Sync for SemanticViewBindData` (~line 67) needs SAFETY update: "ConnGuard is
      Send-not-Sync, but SemanticViewBindData is held by DuckDB exclusively via a Box<BindData>; no concurrent
      shared-borrow access occurs through this pointer. The Sync impl is therefore safe via the structural
      argument."

    Step 3 — Update the `bind` callback (search for `state.catalog.lookup` in the file — current site is around
    `src/query/table_function.rs:516-525`):
    - `let state = unsafe { &*bind.get_extra_info::<QueryState>() };` (unchanged at this line).
    - `let guard = unsafe { crate::conn_guard::ConnGuard::open(state.db_handle) }.map_err(|e| Box::<dyn std::error::Error>::from(e))?;`
    - `let catalog = crate::catalog::CatalogReader::new(state.db_handle, state.catalog_table_present);`
    - `let json_str = catalog.lookup(&view_name).map_err(|e| Box::<dyn std::error::Error>::from(e))?;` (catalog
      internally opens its own short-lived ConnGuard — a brief overlap with `guard` for ~µs, acceptable per
      RESEARCH §6.5).
    - When constructing the returned `SemanticViewBindData`, MOVE `guard` into its `conn_guard` field.

    Step 4 — Update the `func` callback (~`src/query/table_function.rs:763-773`):
    - Replace `state.conn` with `bind_data.conn_guard.raw()` in the `execute_sql_raw(...)` call.
    - If the `state` lookup is no longer needed in `func()` after this change, remove the
      `let state = unsafe { &*func.get_extra_info::<QueryState>() };` line.

    Step 5 — Edit `src/query/explain.rs`:
    - Apply the same `QueryState` consumer changes. Read the file first (one-pass) to determine whether
      `explain_semantic_view` executes SQL (would need bind-data ConnGuard) or just renders the expanded SQL
      string (no execution → only catalog lookup → transient ConnGuard inside `CatalogReader` is sufficient).
    - If it does execute SQL: mirror Steps 2-4 of this task in `src/query/explain.rs`.
    - If it does not execute SQL: only update the catalog lookup site —
      `let catalog = crate::catalog::CatalogReader::new(state.db_handle, state.catalog_table_present); catalog.lookup(...)`.

    Step 6 — Audit any other consumer of `QueryState::catalog` or `QueryState::conn` via
    `grep -rn "QueryState" src/ | grep -v "//"` and `grep -rn "state\.conn\|state\.catalog" src/query/ src/lib.rs`.
    Any remaining match must transition to the new shape.

    Step 7 — Build and sqllogictest spot-check:
    - `cargo build --features extension --no-default-features` succeeds.
    - `just build` succeeds.
    - Run `just test-sql` for any test that exercises `FROM semantic_view(...)` or `EXPLAIN semantic_view(...)`.
      Exit 0 with no diffs. Save log to `$TMPDIR/65_03_t2_sqllogic.log`.
  </action>
  <verify>
    <automated>cargo build --features extension --no-default-features 2>&1 | tail -10 && grep -E "db_handle: (ffi::|libduckdb_sys::)?duckdb_database" src/query/table_function.rs && ! grep -E "pub conn: ffi::duckdb_connection" src/query/table_function.rs && grep -c "conn_guard:" src/query/table_function.rs</automated>
  </verify>
  <acceptance_criteria>
    - `QueryState` carries `db_handle: duckdb_database` and `catalog_table_present: bool`. Verify with
      `grep -E "db_handle: (ffi::|libduckdb_sys::)?duckdb_database" src/query/table_function.rs` ≥1.
    - Old `pub conn: ffi::duckdb_connection` and `pub catalog: CatalogReader` fields removed from QueryState.
      Verify with `grep -E "pub conn: ffi::duckdb_connection|pub catalog: CatalogReader" src/query/table_function.rs`
      showing 0 matches inside the `struct QueryState` block.
    - `SemanticViewBindData` has a `conn_guard: ConnGuard` field. Verify with
      `grep -E "conn_guard:.*ConnGuard" src/query/table_function.rs` ≥1.
    - `func()` uses `bind_data.conn_guard.raw()` (not `state.conn`). Verify with
      `grep -E "bind_data\.conn_guard\.raw\(\)|conn_guard\.raw\(\)" src/query/table_function.rs` ≥1 AND
      `grep -nE "state\.conn[^_]" src/query/table_function.rs` returns 0 matches.
    - `src/query/explain.rs` updated to use `state.db_handle` + `CatalogReader::new(state.db_handle, ...)`.
      Verify with `grep -E "state\.db_handle|state\.catalog_table_present" src/query/explain.rs` ≥1.
    - `cargo build --features extension --no-default-features` succeeds.
    - `just build` succeeds.
    - `just test-sql` for query-side tests exits 0. Evidence path recorded in summary.
  </acceptance_criteria>
  <done>
    H2 ownership has moved out of `QueryState` (which is now `db_handle` + flag) and into per-query
    `SemanticViewBindData::conn_guard`. The connection's lifetime is now coupled to the query, not the extension.
    `explain_semantic_view` mirrors the pattern. Query-side regression tests stay green.
  </done>
</task>

<task type="auto" tdd="true">
  <name>Task 3: Drop H1 + H2 from init_extension; audit-confirm 10 read-side ddl/*.rs files need no change</name>
  <files>
    src/lib.rs
  </files>
  <read_first>
    src/lib.rs
    src/ddl/list.rs
    src/ddl/describe.rs
    src/ddl/show_columns.rs
    src/ddl/show_dims.rs
    src/ddl/show_dims_for_metric.rs
    src/ddl/show_metrics.rs
    src/ddl/show_facts.rs
    src/ddl/show_materializations.rs
    src/ddl/get_ddl.rs
    src/ddl/read_yaml.rs
    test/integration/test_readonly_load.py
    .planning/phases/65-overridecontext-connection-teardown/65-PATTERNS.md
    .planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md
  </read_first>
  <behavior>
    - `init_extension` no longer calls `duckdb_connect` to create `catalog_conn` or `query_conn`. It constructs
      `CatalogReader::new(db_handle, catalog_table_present)` (shape b) directly from `db_handle`, then registers it
      as extra_info for all 13 read-side table functions and both scalars (call sites SYNTACTICALLY UNCHANGED).
    - `init_extension` constructs `QueryState { db_handle, catalog_table_present }` and registers it as extra_info
      for both `SemanticViewVTab` and `ExplainSemanticViewVTab` (call sites SYNTACTICALLY UNCHANGED).
    - After this task, `grep -nE "duckdb_connect\\(" src/lib.rs` returns 0 matches inside the `init_extension`
      function body.
    - The 10 `src/ddl/*.rs` read-side files are AUDIT-ONLY under shape (b) (per `files_audited` frontmatter). Each
      file's bind callback or scalar `invoke` does `bind.get_extra_info::<CatalogReader>()` then calls
      `reader.lookup(...)` / `reader.list_all()` — with shape (b) these calls work unchanged because
      `CatalogReader::lookup` etc. now open their own ConnGuard internally.
    - All pre-existing sqllogictests pass byte-identical.
    - Plan 01's failing B1..B4 + B11 in-process tests NOW PASS (the fix is complete at this point).
  </behavior>
  <action>
    Step 1 — Edit `src/lib.rs::init_extension`. The current shape (post Plan 02 partial) is at lines ~378-518
    (see Read above). Remove the H1 `catalog_conn` block:
    - Delete lines ~386-390:
      ```
      let mut catalog_conn: ffi::duckdb_connection = ptr::null_mut();
      let rc = unsafe { ffi::duckdb_connect(db_handle, &mut catalog_conn) };
      if rc != ffi::DuckDBSuccess {
          return Err("Failed to create catalog connection".into());
      }
      ```
    - Construct `catalog_reader` directly from `db_handle` (replace line ~408-409):
      `let catalog_reader = crate::catalog::CatalogReader::new(db_handle, catalog_table_present);`

    Remove the H2 `query_conn` block (current lines ~497-502):
    - Delete:
      ```
      let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
      let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
      if rc != ffi::DuckDBSuccess {
          return Err("Failed to create query connection for semantic_view".into());
      }
      ```
    - Construct `query_state` from `db_handle` (replace lines ~505-508):
      `let query_state = QueryState { db_handle, catalog_table_present };`

    The 13 `con.register_table_function_with_extra_info::<...VTab, _>(..., &catalog_reader)` calls and the 2
    `con.register_scalar_function_with_state::<...Scalar>(..., &catalog_reader)` calls are SYNTACTICALLY UNCHANGED
    — they pass `&catalog_reader`. Since `CatalogReader` is now shape (b), this just works.

    Similarly the 2 `con.register_table_function_with_extra_info::<SemanticViewVTab, _>("semantic_view", &query_state)`
    and `ExplainSemanticViewVTab` calls are unchanged.

    The Plan 02 `sv_register_parser_hooks(db_handle, catalog_table_present, is_file_backed)` call is unchanged.

    Step 2 — Audit the 10 read-side files (`src/ddl/list.rs`, `describe.rs`, `show_columns.rs`, `show_dims.rs`,
    `show_dims_for_metric.rs`, `show_metrics.rs`, `show_facts.rs`, `show_materializations.rs`, `get_ddl.rs`,
    `read_yaml.rs`). These are AUDIT-ONLY under shape (b) — see `files_audited` frontmatter. The audit is codified
    as discrete grep + git-diff gates in this task's `<acceptance_criteria>` block; do NOT edit these files unless
    an audit gate fails (in which case reclassify the file from `files_audited` to `files_modified` and update
    SUMMARY).

    Audit gates (all must hold — see acceptance_criteria below for the prose-checked items):
    1. `grep -l 'CatalogReader::new' src/ddl/*.rs` returns 0 lines — only `src/lib.rs` constructs `CatalogReader`.
       If any ddl/*.rs file constructs one independently, replace `CatalogReader::new(conn, ...)` with
       `CatalogReader::new(db, ...)` and move that file out of `files_audited` into `files_modified`.
    2. `grep -nE '\.raw\(\)' src/ddl/*.rs | grep -v PreparedStmt | grep -v duckdb_destroy_result` returns 0 matches
       against any `CatalogReader` instance — the `raw()` accessor is removed in this plan, and any match needs
       refactoring to open a `ConnGuard` locally.
    3. `grep -n 'get_extra_info::<CatalogReader>' src/ddl/*.rs` returns ≥10 matches (one per file, sometimes two —
       confirms the call sites are still present and shape-(b)-compatible).
    4. `git diff --numstat src/ddl/` reports zero changed lines across all 10 audit-only files. THIS is the
       strongest assertion that shape (b) genuinely required zero per-file change. If any of the 10 files shows any
       change, STOP and reclassify per gate 1's procedure.

    Step 3 — Build and full-suite test:
    - `cargo build` (bundled) succeeds.
    - `cargo build --features extension --no-default-features` succeeds.
    - `just build` succeeds.
    - `just test-sql` exits 0 byte-identical (Phase 62 transactional DDL, all read-side table function tests,
      caret tests, semantic_view query tests, materialization tests, etc.).
    - `uv run test/integration/test_readonly_load.py` — the B1..B4 + B11 tests planted by Plan 01 must now PASS
      along with the pre-existing subprocess tests. The test harness prints `RESULT: PASS` per test (see
      `test/integration/test_readonly_load.py:224`). Capture log to `$TMPDIR/65_03_t3_readonly.log`. Wrap in
      `timeout 120` so a hung run fails fast rather than waiting for the per-test 5s watchdog × 50 = 250s
      worst-case (B11 RAII loop iteration count).
    - `just test-multi-db` exits 0 (Phase 61 regression).
    - `just test-concurrent` exits 0 (Phase 60 regression).
    - `just test-adbc` exits 0 (Phase 58 regression).
    - `just test-caret` exits 0 (Phase 62 regression).

    Step 4 — Verify no `duckdb_connect` calls remain in `src/lib.rs::init_extension`:
    - `grep -nE "duckdb_connect\\(" src/lib.rs` should return 0 matches inside `init_extension`. (Matches outside
      that function — e.g. in an unrelated helper or test — are acceptable; the only ones in init_extension were
      H1+H2, both deleted.)

    Commit message: `feat(65-03): remove H1+H2 long-lived duckdb_connection (LIFE-01 SC-3 evidence)`.
  </action>
  <verify>
    <automated>just build 2>&1 | tee $TMPDIR/65-03-t3-build.log; tail -5 $TMPDIR/65-03-t3-build.log; grep -qE 'Compiling|Finished' $TMPDIR/65-03-t3-build.log && timeout 120 uv run test/integration/test_readonly_load.py 2>&1 | tee $TMPDIR/65-03-t3-tests.log; tail -20 $TMPDIR/65-03-t3-tests.log; grep -qE 'RESULT: PASS' $TMPDIR/65-03-t3-tests.log && ! grep -qE 'RESULT: (FAIL|ERROR)' $TMPDIR/65-03-t3-tests.log</automated>
  </verify>
  <acceptance_criteria>
    - `grep -nE "duckdb_connect\\(" src/lib.rs` returns 0 matches (both H1 and H2 connects are gone). **Prose-checked
      structural assertion** — verify by inspecting the grep output as part of summary writing.
    - `grep -E "let catalog_reader = .*CatalogReader::new\(db_handle" src/lib.rs` ≥1 (constructed from db_handle directly).
    - `grep -E "QueryState\s*\{\s*db_handle" src/lib.rs` ≥1.
    - **Prose-checked audit gate (Step 2 gate 1):** `grep -n "CatalogReader::new" src/ddl/*.rs` returns 0 matches
      (only lib.rs constructs CatalogReader). Run this manually and record the result in summary.
    - **Prose-checked audit gate (Step 2 gate 2):** `grep -nE '\.raw\(\)' src/ddl/*.rs | grep -v PreparedStmt |
      grep -v duckdb_destroy_result` returns 0 matches against any `CatalogReader` instance.
    - **Prose-checked audit gate (Step 2 gate 3):** `grep -n 'get_extra_info::<CatalogReader>' src/ddl/*.rs` returns
      ≥10 matches (confirms call sites are present and shape-(b)-compatible).
    - **Prose-checked audit gate (Step 2 gate 4):** `git diff --numstat src/ddl/` returns 0 changed lines across
      all 10 audit-only files. THIS is the strongest assertion that shape (b) genuinely required zero per-file
      change. If any of the 10 files shows any change, STOP and reclassify per Step 2 gate 1's procedure.
    - `cargo build`, `cargo build --features extension --no-default-features`, and `just build` all succeed
      (build success is included in the `<automated>` verify).
    - `just test-sql` exits 0 byte-identical (no diffs vs the pass count at end of Plan 02 Task 3).
    - `uv run test/integration/test_readonly_load.py` — ALL tests pass, INCLUDING B1..B4 + B11:
      `test_in_process_bootstrap_then_readonly_fresh`, `test_in_process_bootstrap_then_readonly_existing`,
      `test_in_process_load_only_then_readonly`, `test_in_process_readonly_then_readwrite`,
      `test_repeated_load_close_no_busy_spin`. The test harness emits `RESULT: PASS` per passing test and
      `RESULT: FAIL` / `RESULT: ERROR` per failing test (per `test/integration/test_readonly_load.py:224-230`);
      the `<automated>` verify asserts `RESULT: PASS` is present AND `RESULT: FAIL|ERROR` is NOT present.
      The wrapping `timeout 120` prevents B11's 50-iteration worst-case (50×5s=250s watchdog budget) from hanging.
    - `just test-multi-db`, `just test-concurrent`, `just test-adbc`, `just test-caret` all exit 0
      (prose-checked — run each manually and record exit codes in summary).
    - Single commit subject matches `feat(65-03): remove H1+H2 long-lived duckdb_connection (LIFE-01 SC-3 evidence)`.
  </acceptance_criteria>
  <done>
    Both H1 and H2 long-lived connections are eliminated. CatalogReader and QueryState carry `db_handle`. All 13
    read-side table functions + 2 scalars work against the new shape with no per-file changes. In-process RO reopen
    tests pass. Phase 62/61/60/58 regression suites stay green. LIFE-01 satisfied.
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Rust ↔ DuckDB C-API | Each bind callback now opens+closes its own `duckdb_connection` via `ConnGuard`; per-query connection lifetime now tied to bind data, per-method lifetime for CatalogReader internal opens. |
| Extension ↔ caller's connection | The extension's connection is short-lived (per query / per method) instead of long-lived (process). Lock-contention behaviour at boundary unchanged otherwise (RESEARCH §6.5). |
| ddl/*.rs callers ↔ CatalogReader shape | The 10 audit-only files depend on the shape-b invariant that `CatalogReader::lookup` / `list_all` open their own ConnGuard internally. Any future change that exposes `raw()` or returns the connection would break the H1 elimination. |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-65-09 | Tampering | `SemanticViewBindData` Clone derive — if Clone is later added, the non-Clone `ConnGuard` field will fail to compile (compile-time guard); but if someone removes the field, the H2 leak returns | mitigate | Task 2 explicitly verifies no Clone derive present today; Plan 04 B13 structural test guards OverrideContext; consider analogous guard for SemanticViewBindData in a follow-up TECH-DEBT entry. |
| T-65-10 | Denial of Service | Per-query ConnGuard lifetime — if DuckDB drops bind data before `func()` completes, the connection drops mid-query | accept | DuckDB's BindData lifetime is bind→init→func→destroy in that order, established convention; structurally safe per duckdb-rs binding. |
| T-65-11 | Elevation of Privilege | None — connections still have the same caller-context-derived privileges | accept | n/a |
| T-65-12 | Information Disclosure | None — internal lifecycle change only | accept | n/a |
| T-65-SC | Tampering | No new package installs in this plan | accept | Plan 03 modifies in-tree Rust only; no new crates added. Cargo.toml unchanged. No legitimacy gate required. |
</threat_model>

<verification>
After all tasks in this plan complete:

1. Task 0 checkpoint:human-verify confirmed `BIND-THREAD-RC0` in `65-02-SPIKES.md` — Plan 03 was authorised to
   proceed under shape (b). If the marker showed `BIND-THREAD-RC1`, Plan 03 would have halted before Task 1.
2. `cargo build` and `cargo build --features extension --no-default-features` both succeed.
3. `just build` succeeds.
4. `grep -nE "duckdb_connect\\(" src/lib.rs` returns 0 matches (init_extension is clean — H1 and H2 both removed).
5. `grep -E "ConnGuard::open" src/catalog.rs src/query/table_function.rs src/query/explain.rs` returns matches in
   each file (per-method/per-query open pattern uniformly applied).
6. `just test-sql` exits 0 byte-identical (same pass count as end of Plan 02 Task 3).
7. `uv run test/integration/test_readonly_load.py` — all tests pass, including the five Plan 01 B1..B4 + B11 tests
   that previously failed on baseline. Wrapped in `timeout 120` per Task 3 verify.
8. `just test-multi-db`, `just test-concurrent`, `just test-adbc`, `just test-caret` all exit 0.
9. `git diff --stat src/ cpp/` shows changes confined to `src/lib.rs`, `src/catalog.rs`, `src/query/table_function.rs`,
   `src/query/explain.rs`. The 10 src/ddl/*.rs files have ZERO changes (audit gate per Task 3).
</verification>

<success_criteria>
- Zero long-lived extension-owned `duckdb_connection` handles. H1 and H2 are both eliminated.
- `CatalogReader` (shape b) carries `db_handle`; per-method ConnGuard open.
- `QueryState` carries `db_handle` + `catalog_table_present`; per-query ConnGuard lives in `SemanticViewBindData`.
- B1..B4 + B11 in-process tests PASS. LIFE-01 satisfied.
- All Phase 58/60/61/62 regression suites stay green byte-identical.
- The 10 src/ddl/*.rs files needed ZERO changes — shape (b) audit confirms the design choice.
</success_criteria>

<output>
Create `.planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md` when done. Summary MUST include:
- Confirmation that Task 0 checkpoint passed with `BIND-THREAD-RC0` (cite the line read from `65-02-SPIKES.md`).
- Final `CatalogReader` field list and per-method ConnGuard pattern (one-line description).
- Final `QueryState` field list and the `SemanticViewBindData::conn_guard` field placement.
- Confirmation that `grep -nE "duckdb_connect\\(" src/lib.rs` returns 0.
- Log path showing all 5 in-process tests (B1-B4, B11) now passing (`RESULT: PASS` markers).
- Log path showing Phase 58/60/61/62 regression suites green.
- The audit-gate evidence: `git diff --numstat src/ddl/` reports 0 changed lines across all 10 audit-only files.
- Any deviation from PATTERNS.md prescribed shape and why.
</output>
</content>
</invoke>