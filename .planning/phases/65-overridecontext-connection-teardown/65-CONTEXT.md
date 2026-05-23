# Phase 65: OverrideContext Connection Teardown — Context

**Gathered:** 2026-05-23
**Status:** Ready for planning (third re-plan; under read-elimination architecture per `65-BPRIME-ARCHIVE-NOTE.md`)
**Supersedes:** `65-CONTEXT-PRE-BPRIME.md` (locked under per-call ConnGuard-inside-parser_override premise, falsified by D-10/A7), `65-CONTEXT-BPRIME.md.archived` (locked under `parse_function`+`plan_function` premise, falsified by `65-EXEC-TIME-SPIKE.md` EXEC-TIME-RC1).

<domain>
## Phase Boundary

Retire both long-lived extension-owned `duckdb_connection` handles (H1 `catalog_conn` at `src/lib.rs:386-410` and H2 `query_conn` at `src/lib.rs:498-507`) so the in-process `connect(path) → LOAD → CREATE SEMANTIC VIEW → close → connect(path, read_only=True)` hang resolves (LIFE-01..04). Preserve v0.8.0 transactional DDL semantics byte-identical.

Achieved by **eliminating the catalog reads inside `parser_override`** rather than relocating them (the B-prime premise that failed), plus migrating the 12 read-side table-function callbacks to the C++ Catalog API registration shim so their bind callbacks gain `ClientContext &` and can open per-call `Connection(*context.db)`.

Architecture is locked by ROADMAP entry for Phase 65 (the "Architecture (locked)" block) and by spike evidence in `65-OPTION-B-SPIKE.md` (Connection from extension callbacks works), `65-READ-PATH-SPIKE.md` (C++ Catalog API bind has ClientContext), `65-EXEC-TIME-SPIKE.md` (context.Query alternatives self-deadlock), and `65-ALTER-REWRITE-SPIKE.md` (UPDATE-with-TF-subquery pattern viable for ALTER).

**Out of scope (Phase 66):** ADBC query-path testing, surviving expansion-qualification work (if any after H2 retires — likely dissolves), CHANGELOG, version bump, milestone close.

</domain>

<decisions>
## Implementation Decisions

### Baseline (revert before rebuild)

- **D-01** — **Hard-revert Plan 02 partial commits.** Roll `src/parse.rs` `OverrideContext` and `cpp/src/shim.cpp` / `shim.hpp` `sv_register_parser_hooks` signature back to v0.9.0 (Phase 62) shape with the `INTENTIONAL LEAK` comment temporarily restored. The new architecture removes that leak by retiring H1 entirely (no replacement); the `db_handle` plumbing introduced by commit `0d2c0b7` / `f9caafe` is NOT reusable foundation under read-elimination, because no Rust-side or parser_override-side code under the new architecture needs `db_handle`. Helper TFs and read-side bind callbacks get `ClientContext` directly under the C++ Catalog API path. Reverts D-12 (PRE-BPRIME) which preserved these as "reusable".
- **D-02** — **Delete `src/conn_guard.rs` (Plan 01's ConnGuard RAII module).** No Rust consumer materializes under read-elimination — all connection-needing work moves to C++ bind callbacks under the Catalog API shim. Rationale lives only in this CONTEXT and the SUMMARY for the slimming plan; no "we tried this" marker in source.
- **D-03** — **Keep Plan 01's watchdog tests intact** (`test_in_process_bootstrap_then_readonly_fresh`, `..._existing`, `test_in_process_load_only_then_readonly`, `test_in_process_readonly_then_readwrite`, `test_repeated_load_close_no_busy_spin` in `test/integration/test_readonly_load.py`). These remain the LIFE-01 / success-criterion-1 evidence: they fail on v0.9.0 baseline and on the intermediate slimming/ALTER plans, and MUST flip green by the lifecycle close-out plan. The `_connect_with_watchdog` helper stays as-is.
- **D-03b** — **Extend post-reopen coverage in Plan 06** to satisfy ROADMAP success criterion 3 in full. Plan 01's B1/B2 only exercise `list_semantic_views()` after the RO reopen; add post-reopen variants that exercise:
  1. `SELECT … FROM semantic_view('v', dimensions := [...], metrics := [...])` — the main expansion path's bind callback (Plan 05's last migration target) on the reopened RO connection.
  2. `SELECT * FROM describe_semantic_view('v')` — read-side bind that today routes through query_conn.
  3. At least one `SHOW SEMANTIC …` (pick `SHOW SEMANTIC DIMENSIONS FROM v` as the representative — covers the show_semantic_dimensions read path).
  4. `SELECT * FROM get_ddl('v')` — round-trip DDL emission on the RO reopened conn.

  Each new test wraps the post-reopen call in `_connect_with_watchdog` (or an analog) and asserts the call returns within the watchdog budget AND returns the expected data. These are the regression guard for Plan 05's read-path migration: if any of the 12 callbacks still hold or open a stale handle past the original close, these tests catch it.
- **D-04** — **Keep the C++ Catalog API registration shim infrastructure from Plan 02 partial** (`sv_register_table_function` in `cpp/src/shim.cpp`) — this is the surviving half of B-prime and is load-bearing for the read-path migration plan. Plan 02's commits that touch ONLY this shim (not `OverrideContext` / parser_override) stay.

### PK auto-inference removal (breaking change)

- **D-05** — **Delete `src/ddl/define.rs::resolve_pk_from_catalog` and its caller at `define.rs:105`.** Snowflake-aligned: PKs in semantic views are LOGICAL user assertions, not physical-catalog imports. The auto-fallback against `duckdb_constraints()` is removed.
- **D-06** — **Hard error at CREATE / ALTER with actionable message.** When a TABLES entry has no `PRIMARY KEY (cols)` or `UNIQUE (cols)` declared AND another TABLES entry FK-references it (via explicit `REFERENCES target(cols)`), CREATE / ALTER fails with a message of shape:
  > `Table 'X' has no PRIMARY KEY declared but is referenced by FK in 'Y'. Add PRIMARY KEY (cols) or UNIQUE (cols) to the TABLES clause for X. (v0.10.0: physical-catalog PK auto-inference removed — see CHANGELOG.)`

  Fail-fast at the point of mutation. No deferral to query-time. No WARN-only transitional window.
- **D-07** — **Existing persisted definitions untouched at LOAD.** Definitions in `semantic_layer._definitions` written under v0.9.0 with inferred PKs continue to load, query (`semantic_view()`), and introspect (`SHOW`/`DESCRIBE`) successfully on v0.10.0. The new validation triggers only on re-CREATE or ALTER. Zero-friction upgrade for views the user isn't modifying.
- **D-08** — **CHANGELOG breaking-change note required.** Listed under `### Changed` in the `## [0.10.0]` section (release-prep work belongs to Phase 66). Reason: success criterion 5 in ROADMAP mandates documenting this.

### ALTER architecture (write-path on the caller's connection)

- **D-09** — **Pure-SQL `json_set` UPDATE rewrites for trivial ALTER variants** — no helper TF needed. Variants covered: `ALTER RENAME TO`, `ALTER SET COMMENT`, `ALTER UNSET COMMENT`, `ALTER SET TAG`, `ALTER MAKE PRIVATE`, `ALTER MAKE PUBLIC`, `ALTER ADD SYNONYMS`. `parser_override` emits e.g. `UPDATE semantic_layer._definitions SET definition = json_set(definition, '$.comment', '<escaped_new>') WHERE name = ?` (with race-guard wrapping per Phase 60 pattern carried forward).
- **D-10** — **Helper TFs for ALTER variants needing catalog reads or YAML parsing.** Variants: `ALTER ADD DIMENSION`, `ALTER ADD METRIC`, `ALTER ADD FACT` (need type inference via LIMIT 0 probe), `ALTER ADD RELATIONSHIP` (needs FK target validation / PK lookup if explicit), `ALTER DROP DIMENSION/METRIC/FACT/RELATIONSHIP` (need existence check that ON-CONFLICT can't express). Each registered via the C++ Catalog API shim; bind callback opens per-call `Connection(*context.db)` (committed-state read trade-off carries over from TECH-DEBT 19 — already documented for DESCRIBE/SHOW).
- **D-11** — **`CREATE FROM YAML FILE` follows the same helper-TF pattern.** Single helper `__sv_compute_create_from_yaml(path, opts)` whose bind reads the YAML, type-infers, returns the final JSON-serialized definition string. Outer `parser_override` emits `INSERT INTO semantic_layer._definitions (name, definition) VALUES (?, (SELECT new_def FROM __sv_compute_create_from_yaml(?, ?))) ON CONFLICT …`.
- **D-12** — **Helper-TF naming convention:** `__sv_compute_<op>` for ALTER variants needing a helper, and `__sv_compute_create_from_yaml` for the YAML path. Granularity is **one helper per variant that needs one**, not one mega dispatcher and not one per every variant. Trivial variants stay pure-SQL.
- **D-13** — **DROP / ALTER race-guard pattern from Phase 60 carries forward unchanged.** Two-statement `SELECT CASE WHEN NOT EXISTS THEN error() ELSE TRUE; DELETE … RETURNING name` (Phase 60's workaround for DuckDB 1.10.502's CTE-with-DML rejection) stays in place. Existence checks for the trivial CREATE OR REPLACE / IF NOT EXISTS paths fold into `INSERT … ON CONFLICT`.

### Read-path migration (12 callbacks)

- **D-14** — **Migrate all 12 read-side table-function callbacks to the C++ Catalog API shim** (`sv_register_table_function`, surviving infrastructure from Plan 02). Callbacks in scope: `list_semantic_views`, `describe_semantic_view`, `show_semantic_views`, `show_semantic_columns`, `show_semantic_dimensions`, `show_semantic_dimensions_for_metric`, `show_semantic_metrics`, `show_semantic_facts`, `show_semantic_materializations`, `get_ddl`, `read_yaml_from_semantic_view`, `semantic_view` (main expansion path). Plus 2 scalars (`explain_semantic_view` and any other) follow the same pattern if they need a connection. Each bind callback opens per-call `Connection(*context.db)`; the long-lived `query_conn` (H2) retires once all 12 are migrated.
- **D-15** — **Migration may be incremental within the read-path plan but H2 retirement is the final atomic step in that plan.** Until the last callback migrates, `query_conn` stays open. The final commit in the read-path plan removes the `query_conn` allocation in `init_extension` (`src/lib.rs:498-507`).

### Type inference deferral (read-side, on demand)

- **D-16** — **`SHOW SEMANTIC VIEW` / `DESCRIBE` runs the `LIMIT 0` type probe on demand at bind time.** Under the C++ Catalog API shim path their bind callback opens `Connection(*context.db)` and probes — same source-of-truth as today's CREATE-time probe, just moved. User-visible behavior at first call: identical to v0.7.1 (data_type column populated). No NULL placeholder, no behavior change. Cost: each cold SHOW / DESCRIBE invocation pays the probe time; subsequent calls in the same session can use a process-local cache (planner's discretion on cache shape — see Claude's Discretion).
- **D-17** — **No persisted type cache in `_definitions`.** Persisting types would defeat the read-elimination goal (would require either a connection back inside `parser_override` at CREATE time, or a post-CREATE write callback we don't have). Stays purely on-demand. DECIMAL stays empty as it did in v0.7.1.

### Plan structure (Phase 65)

- **D-18** — **4 plans for Phase 65, grouped by architectural concern:**
  - **Plan 03 — parser_override slimming wave:** revert D-01 / D-02 commits, delete ConnGuard, remove PK auto-inference (D-05/06/07), move metadata capture (`now()`, `current_database()`, `current_schema()`) from extension-side execution to SQL expressions inside the rewritten INSERT, fold existence checks into `INSERT … ON CONFLICT`. End state: `parser_override` has zero catalog reads; H1 catalog_conn is still allocated (Plan 06 retires it) but is unused by any code path.
  - **Plan 04 — ALTER architecture wave:** pure-SQL `json_set` UPDATE rewrites for trivial ALTER variants (D-09), helper-TF family `__sv_compute_*` for non-trivial variants (D-10) + CREATE FROM YAML FILE (D-11), registered via the C++ Catalog API shim (D-04 surviving infrastructure). End state: all ALTER + YAML CREATE writes ride the caller's connection.
  - **Plan 05 — read-path wave:** migrate all 12 read-side callbacks to the C++ Catalog API shim (D-14), defer type inference to read-side bind (D-16/17), retire H2 query_conn (D-15). End state: H2 gone; all reads on per-call Connection.
  - **Plan 06 — lifecycle close-out wave:** retire H1 catalog_conn (`src/lib.rs:386-410` allocation), structural Rust unit test that fails CI if anyone re-introduces a long-lived native `duckdb_connection` handle in `init_extension` (success criterion 4), **extend post-reopen integration coverage per D-03b** (4 new tests covering `semantic_view()` SELECT, `describe_semantic_view()`, a `SHOW SEMANTIC …`, and `get_ddl()` post-reopen — satisfies ROADMAP success criterion 3 in full), watchdog tests must be green, LIFE-04 ledger close in `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` with forward pointer to v0.10.0.

  Release-prep (CHANGELOG `## [0.10.0]` with `### Changed` PK note per D-08, version bump in Cargo.toml + description.yml, ADBC query tests) lives in **Phase 66**, per the locked 65/66 boundary.
- **D-19** — **Plan numbering continues from 03** (01 and 02 partial already landed). 02-PARTIAL stays as-is; the slimming plan reverts the broken commits from 02 but does not renumber.

### Phase 66 boundary

- **D-20** — **Phase 65 owns lifecycle fix only.** Phase 66 owns EXPAND-CTX-01..03 verification (the catalog-search-path divergence root cause likely dissolves once H2 retires — verify empirically via ADBC query tests; fix any surviving sites), the `test/integration/test_adbc_queries.py` ADBC harness (5 scenarios per ROADMAP success criterion 1), CHANGELOG entry for v0.10.0 covering both phases, version bump, `_notes/error_with_adbc.md` cleanup, and `just ci` green on `milestone/v0.10.0`.

### Non-negotiable invariants (carry forward unchanged)

- **D-21** — **D-20 (PRE-BPRIME) — transactional DDL semantics non-negotiable.** CREATE/DROP/ALTER inside user `BEGIN/COMMIT` MUST continue to participate in the caller's transaction. Verified by existing Phase 58 ADBC transactional tests (`test_adbc_transactions.py`) which MUST stay green throughout all 4 plans. (See [[feedback-transactional-ddl-non-negotiable]].)
- **D-22** — **Bounded scope with signal surfacing.** Phase 65 ships the lifecycle fix and nothing else. Any adjacent broken lifecycle patterns surfaced during implementation become TECH-DEBT entries / deferred-items / Phase 66 follow-ups, not silent absorption. (See [[feedback-bounded-scope-with-signal-surfacing]].)
- **D-23** — **Root-cause investigation over symptom hacks.** D-01 from PRE-BPRIME stays in force: option (b) "detect access-mode mismatch and error" is NOT an acceptable shipping fix. The new architecture is the correct-fix path. (See [[feedback-root-cause-over-hacks]].)
- **D-24** — **No time pressure — get it right.** Even if any plan needs to extend in scope (e.g. helper-TF family ends up larger than expected, or read-path migration uncovers a thirteenth callback), do not contract scope to ship faster. (See [[feedback-no-time-pressure-get-it-right]].)

### Claude's Discretion

- Specific shape of the process-local type-inference cache from D-16 (HashMap keyed on view name + schema fingerprint? OnceCell per session? bounded LRU as v0.8.0's catalog cache used to be?). Planner decides based on simplicity vs cache-coherence risk under DDL between SHOWs.
- Exact JSON path strings for D-09's `json_set` calls (e.g. `'$.comment'` vs `'comment'` vs `'$["comment"]'`) — depends on what the Phase 51 YAML serialization shape actually wrote and what `json_set` accepts. Planner verifies against the persisted format.
- Whether Plan 04's helper-TF family lives in a new `cpp/src/alter_helpers.cpp` translation unit or extends `cpp/src/shim.cpp` directly. Planner decides based on compile-time / file-length pressure.
- How to express the "no long-lived duckdb_connection in init_extension" structural test from Plan 06 — could be a grep-based build-time check, a Rust `proc-macro`-driven inventory, or a runtime introspection. Planner picks the lowest-effort form that meaningfully fails on re-introduction.
- Test layout for Plan 04's ALTER coverage — extend `test/sql/65_alter_*.test` family, add new `test/integration/test_alter_*.py` files, or both. Planner reads existing v0.6.0 ALTER tests to match convention.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents (researcher, planner, executor) MUST read these before researching, planning, or implementing.**

### Phase brief (primary)

- `_notes/v0.9.1_readonly_reopen_hang.md` — original downstream bug report (Item 1 = LIFE-01..04; Item 2 = Phase 66 territory).
- `.planning/REQUIREMENTS.md` — LIFE-01..04 requirements (still apply under v0.10.0 framing; original v0.9.1 milestone goal still names them).
- `.planning/ROADMAP.md` — Phase 65 entry under v0.10.0 milestone has the **locked architecture** block (4-bullet list under "Architecture (locked, pending fresh /gsd-discuss-phase formalization)"). MUST READ before planning.

### Empirical evidence (load-bearing for architecture)

- `.planning/phases/65-overridecontext-connection-teardown/65-BPRIME-ARCHIVE-NOTE.md` — full pivot rationale; what B-prime was, why it failed, what replaces it.
- `.planning/phases/65-overridecontext-connection-teardown/65-OPTION-B-SPIKE.md` — Probe 1 proves `Connection(*context.db)` opens cleanly from extension callbacks with `ClientContext`. Load-bearing for helper-TF + read-path bind.
- `.planning/phases/65-overridecontext-connection-teardown/65-READ-PATH-SPIKE.md` — `READ-BIND-RC0` proves C++ Catalog API bind callbacks have usable `ClientContext`. Load-bearing for read-path migration.
- `.planning/phases/65-overridecontext-connection-teardown/65-EXEC-TIME-SPIKE.md` — `EXEC-TIME-RC1` killed `context.Query` from `TableFunction.func`; documents the `context_lock` self-deadlock. Read to understand why alternatives were ruled out.
- `.planning/phases/65-overridecontext-connection-teardown/65-ALTER-REWRITE-SPIKE.md` — `ALTER-RC0` proves UPDATE-with-TF-subquery is viable on DuckDB v1.5.2; Probes A/B confirm D-20 transactional contract; Probe C confirms committed-only read trade-off (TECH-DEBT 19 carry-over). **Load-bearing for Plan 04.**
- `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` (A2-DEADLOCK + A7-RE-ENTRANCY-UNSAFE) — closes the lifecycle-phase grid; documents what is and isn't safe inside `parser_override`.
- `.planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log` — verbatim 43/47 failures from the Plan 02 partial state; the "do not regress to" baseline.

### Plan history (preserved as evidence — do NOT re-execute)

- `.planning/phases/65-overridecontext-connection-teardown/65-01-PLAN.md` + `65-01-SUMMARY.md` — Plan 01 shipped ConnGuard + watchdog tests (D-02 deletes ConnGuard, D-03 keeps watchdog tests).
- `.planning/phases/65-overridecontext-connection-teardown/65-02-PLAN.md` + `65-02-PARTIAL-SUMMARY.md` — Plan 02 partial; commits `0d2c0b7` (parse.rs) + `f9caafe` (cpp shim) revert per D-01; commit landing `sv_register_table_function` stays per D-04.
- `.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT-PRE-BPRIME.md` — first CONTEXT, locked under per-call-ConnGuard premise. Superseded by this file.
- `.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT-BPRIME.md.archived` — second CONTEXT, locked under `parse_function`+`plan_function` premise. Superseded.
- `.planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md` — phase-wide validation tracker; likely needs refresh under the new plan shape.

### Prior-art and predecessors

- `.planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md` §Q2 — the "intentional bounded leak" being re-litigated and now eliminated.
- `.planning/phases/62-caret-restoration-lru-removal/62-PLAN.md` — how `OverrideContext` was attached to `SemanticViewsParserInfo`.
- `.planning/milestones/v0.8.0-phases/58-transactional-ddl/58-PLAN.md` — the `parser_override` rewrite-to-native-SQL pattern; D-21 contract source.
- `.planning/milestones/v0.8.0-phases/60-race-guards-validation-hardening/60-PLAN.md` — the two-statement `DELETE … RETURNING` race-guard pattern that D-13 carries forward.
- `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` — entry "In-process RW→RO reopen of the same DB hangs" that Plan 06 closes per LIFE-04.
- `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/63-PLAN.md` (Plan 03) — `bootstrap_in_subprocess` pattern; test_readonly_load.py structure that Plan 06 verifies against.

### Source files (key sites for this phase)

- `src/lib.rs:386-410` — H1 `catalog_conn` allocation in `init_extension` (Plan 06 retires).
- `src/lib.rs:498-507` — H2 `query_conn` allocation in `init_extension` (Plan 05 retires).
- `src/parse.rs:67-87` — `OverrideContext` struct (Plan 03 reverts to v0.9.0 shape).
- `src/parse.rs:1788-2100` — 4× `rewrite_*` sites currently containing broken `ConnGuard::open` (Plan 03 reverts).
- `src/parse.rs:2513-2554` — `sv_make_override_context` FFI entry (Plan 03 reverts signature).
- `src/parse.rs:1519, 3863` — `resolve_pk_from_catalog` call sites at bind time (Plan 03 removes per D-05/06).
- `src/ddl/define.rs:19-105` — `resolve_pk_from_catalog` function + caller (Plan 03 removes per D-05).
- `src/conn_guard.rs` — entire file (Plan 03 deletes per D-02).
- `cpp/src/shim.cpp` — `sv_register_parser_hooks` (Plan 03 reverts signature) + `sv_register_table_function` (Plan 05 consumes, do not revert) + `__sv_compute_*` helpers added (Plan 04 / Plan 05).
- `cpp/src/shim.hpp` — header declarations matching shim.cpp changes.

### Read-side table-function inventory (Plan 05 scope)

Each lives currently in the duckdb-rs `register_table_function_with_extra_info` path; Plan 05 migrates each to `sv_register_table_function`:

- `list_semantic_views`
- `describe_semantic_view`
- `show_semantic_views`, `show_semantic_columns`, `show_semantic_dimensions`, `show_semantic_dimensions_for_metric`, `show_semantic_metrics`, `show_semantic_facts`, `show_semantic_materializations`
- `get_ddl`
- `read_yaml_from_semantic_view`
- `semantic_view` (main expansion path)
- Plus any scalar functions still on `query_conn` (e.g. `explain_semantic_view` if applicable — planner verifies inventory completeness during research).

### Project conventions

- `CLAUDE.md` (repo root) — quality gate is `just test-all`; phases need unit tests + proptests + sqllogictest; check current branch (`milestone/v0.10.0`) before committing.
- `MEMORY.md` (auto-memory) — relevant feedback entries: [[feedback-root-cause-over-hacks]], [[feedback-bounded-scope-with-signal-surfacing]], [[feedback-documented-limitations]], [[feedback-transactional-ddl-non-negotiable]], [[feedback-no-time-pressure-get-it-right]], [[feedback-no-parallel-builds]], [[feedback-worktree-isolation]], [[feedback-no-background-agents]], [[feedback-no-tail-on-long-commands]].
- `TECH-DEBT.md` — entry #19 (DESCRIBE/SHOW committed-only state) — D-10/D-16 inherit the same trade-off; entry #23 (CREATE IF NOT EXISTS cross-process PK race) — unchanged; entry #20 (LRU eviction) — resolved by Phase 62, do not re-introduce.

### DuckDB upstream (research must consult)

- `cpp/include/duckdb.cpp:369065-369085` — `Binder::Bind(ExtensionStatement &)`, why `plan_function` can't carry transactional DDL (B-prime falsification source — read to internalize the constraint that keeps `parser_override` in place).
- `cpp/include/duckdb.cpp:370905-370936` — `BindUpdateSet`'s `PlanSubqueries` call, why D-09 / D-10 / D-11's UPDATE-with-TF-subquery pattern binds. Foundational for Plan 04.
- DuckDB 1.5.x C++ Catalog API for table-function registration — pattern used in `65-READ-PATH-SPIKE.md` and `65-ALTER-REWRITE-SPIKE.md`.
- 2–3 other community extensions (planner's choice — httpfs, iceberg, ducklake, postgres scanner) for canonical patterns on `Connection(*context.db)` usage and on-demand catalog reads from bind callbacks.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- **`sv_register_table_function` C++ Catalog API shim** (`cpp/src/shim.cpp`, landed in Plan 02 partial commit per D-04): the registration entry point for all read-side TFs and ALTER helper TFs. Both Plan 04 and Plan 05 consume it. Do NOT revert.
- **Phase 60 race-guard two-statement pattern** (`SELECT CASE WHEN NOT EXISTS THEN error() ELSE TRUE; DELETE … RETURNING name`): adopted unchanged by D-13 for DROP/ALTER postcondition checks. Source: phase 60 plans + existing call sites in `src/parse.rs`.
- **Phase 62 `parser_override` rewrite-to-native-SQL pattern**: `Parser::ParseQuery(native_sql)` returning `vector<unique_ptr<SQLStatement>>` to the binder is the only DuckDB v1.5.2 mechanism delivering transactional DDL (D-21). All new SQL shapes (json_set UPDATEs, UPDATE-with-TF-subquery, INSERT-with-subquery for CREATE FROM YAML, INSERT…ON CONFLICT for existence-check fold) go through this same return shape.
- **Phase 58 ADBC transactional test (`test/integration/test_adbc_transactions.py`)**: regression guard for D-21. Stays green throughout all 4 plans of Phase 65.
- **Plan 01 watchdog tests (`test/integration/test_readonly_load.py` B1..B4 + B11)**: success-criterion-1 evidence. Fail on baseline → expected to flip green when Plan 06 retires H1.

### Established Patterns

- **`parser_override` returns rewritten native SQL** — every DDL verb already goes through this; new ALTER variants extend rather than introduce a new mechanism.
- **`OverrideContext` is a heap-boxed FFI struct attached to `SemanticViewsParserInfo`** — Plan 03 reverts to Phase 62 shape (carrying `catalog: CatalogReader { conn: duckdb_connection, … }` + `catalog_table_present` + `is_file_backed`) before the slimming reduces the catalog-read paths to zero call sites.
- **`init_extension` allocates long-lived connections** — the anti-pattern being retired. Plan 06's structural guard test ensures no future regression re-introduces this shape.
- **`CatalogReader` wraps a `duckdb_connection` with caching** (`src/catalog.rs`) — currently consumed by `parser_override` rewrite paths via `OverrideContext.catalog.conn`. Plans 03/04/05 progressively eliminate every consumer; structurally the type can stay but ends up dead-code (planner decides removal timing).

### Integration Points

- **Phase 64 `qualify_and_quote_table_ref` (`src/expand/resolution.rs`)** — wired into main expand path (`src/expand/sql_gen.rs:499,530,550`). The Phase 66 EXPAND-CTX work assumes this stays; the root cause it bridges (catalog-search-path divergence between query_conn and caller) likely dissolves when H2 retires, but `qualify_and_quote_table_ref` itself remains correct defensive output. Plan 05's read-path migration must NOT regress its call sites.
- **YAML serialization/deserialization (Phase 51-53)** — D-11's `__sv_compute_create_from_yaml` calls into the existing YAML→`SemanticView` parsing then serializes to the JSON shape `_definitions.definition` expects. Reuses Phase 56's bare-name extraction (`rsplit('.')`) for FQN support if needed.
- **Type inference (Phase 51 / v0.7.1)** — `LIMIT 0` probe + `typeof` dispatch from existing code; D-16 moves the call site from CREATE-time to bind-callback-time but keeps the probing logic verbatim. Planner reuses, not rewrites.

</code_context>

<specifics>
## Specific Ideas

### Reproduction (Phase 65 entry test)

The minimal reproducer from the downstream report (already coded as `test_in_process_bootstrap_then_readonly_fresh` in `test/integration/test_readonly_load.py`, with `_connect_with_watchdog` wrapping the read-only connect at 5s timeout):

```python
w = duckdb.connect(path)
w.execute("LOAD semantic_views")
w.execute("CREATE TABLE sales_data (...)")
w.execute("CREATE SEMANTIC VIEW sales_view AS ...")
w.close()
r = duckdb.connect(path, read_only=True)  # currently hangs >45s
```

The five watchdog tests (B1..B4 + B11) MUST flip green when Plan 06 retires H1. They are the empirical fail-on-baseline / pass-after-fix evidence per LIFE-03.

### Plan 03's slimming — required end state

After Plan 03 (and before Plan 04 starts):
- `src/conn_guard.rs` deleted.
- `src/parse.rs::OverrideContext` matches v0.9.0 (Phase 62) shape verbatim.
- `cpp/src/shim.cpp` `sv_register_parser_hooks` signature matches v0.9.0; `sv_register_table_function` stays.
- `src/ddl/define.rs::resolve_pk_from_catalog` deleted.
- `parser_override` rewrite paths emit `INSERT INTO _definitions (name, definition, created_at, database_name, schema_name) VALUES (?, ?, now(), current_database(), current_schema()) ON CONFLICT (name) DO …` — all metadata captured by DuckDB on the caller's conn.
- New CREATE/ALTER with missing PK + FK reference produces the D-06 error.
- `just test-sql` is **green** (back to 47/47 at minimum; plan likely adds new sqllogictests for the slimming surface). The Plan 02 partial 4/47 regression is fixed.
- H1 catalog_conn allocation in `init_extension` is **still present but unused** (Plan 06 retires the allocation).

### Plan 04's ALTER coverage — required SQL shapes

| ALTER variant | Mechanism | Example rewrite shape |
|---|---|---|
| RENAME TO | pure-SQL json_set | `UPDATE _definitions SET name = ?, definition = json_set(definition, '$.name', ?) WHERE name = ?` |
| SET COMMENT | pure-SQL json_set | `UPDATE _definitions SET definition = json_set(definition, '$.comment', ?) WHERE name = ?` |
| UNSET COMMENT | pure-SQL json_set | `UPDATE _definitions SET definition = json_remove(definition, '$.comment') WHERE name = ?` |
| MAKE PRIVATE/PUBLIC | pure-SQL json_set | `UPDATE _definitions SET definition = json_set(definition, '$.privacy', ?) WHERE name = ?` |
| SET TAG | pure-SQL json_set | `UPDATE _definitions SET definition = json_set(definition, '$.tags.<k>', ?) WHERE name = ?` |
| ADD SYNONYMS | pure-SQL json_set | `UPDATE _definitions SET definition = json_set(definition, '$.synonyms', json_array(?, ?, …)) WHERE name = ?` |
| ADD DIMENSION/METRIC/FACT | helper TF | `UPDATE _definitions SET definition = (SELECT new_def FROM __sv_compute_alter_add('<view>', '<op_payload_json>')) WHERE name = ?` — bind opens Connection, type-infers via LIMIT 0, returns new JSON |
| DROP DIMENSION/METRIC/FACT/RELATIONSHIP | helper TF | Same shape; bind validates existence + removes from JSON |
| ADD RELATIONSHIP | helper TF | Same shape; bind validates FK target + PK presence (uses D-06 error path for missing PK) |
| CREATE FROM YAML FILE | helper TF (insert form) | `INSERT INTO _definitions (name, definition, …) VALUES (?, (SELECT new_def FROM __sv_compute_create_from_yaml(?, ?)), …) ON CONFLICT …` |

Exact JSON path strings and `json_set` argument shapes — see Claude's Discretion in `<decisions>`.

### Plan 05's migration sequencing

12 callbacks can migrate in any order within Plan 05. Suggested sequence (researcher / planner refines): simpler / no-bind-args first (`list_semantic_views`, `show_semantic_views`), then SHOW-with-filter family (`show_semantic_columns`, etc.), then `describe_semantic_view` + `get_ddl`, then `read_yaml_from_semantic_view`, then `semantic_view` (main expansion path — highest blast radius, migrate last so any regression is caught in isolation).

The final commit in Plan 05 removes the H2 `query_conn` allocation in `src/lib.rs:498-507`. After that commit, `just test-sql`, `cargo test`, and `just ci` must all be green.

### Plan 06's structural guard

A Rust unit test in `src/lib.rs` (or a dedicated `tests/no_long_lived_conn.rs`) that grep-scans the `init_extension` source for any `duckdb_connect` call site and fails if any remain. Exact mechanism per Claude's Discretion — could be a `build.rs`-emitted line count, a `compile_error!` macro keyed on a feature flag, or a runtime introspection in tests.

</specifics>

<deferred>
## Deferred Ideas

- **EXPAND-CTX-01..03 (qualify_and_quote_table_ref wiring across fact/semi-additive/window/materialization paths)** — Phase 66. Root cause expected to dissolve once H2 query_conn retires (Plan 05); empirical verification via ADBC query tests is Phase 66's job. If any qualification sites survive as actual bugs, fix-up lives in Phase 66.
- **ADBC query test harness (`test/integration/test_adbc_queries.py`, `just test-adbc-queries`)** — Phase 66 per ROADMAP.
- **CHANGELOG `## [0.10.0]` section** with `### Fixed` (LIFE-01..04 + EXPAND-CTX), `### Changed` (PK auto-inference removal per D-06/08), `### Removed` (any APIs the rewrite drops) — Phase 66.
- **Cargo.toml + description.yml version bump to 0.10.0** — Phase 66.
- **`_notes/error_with_adbc.md` and `_notes/v0.9.1_readonly_reopen_hang.md` cleanup** — Phase 66.
- **TECH-DEBT #19 (DESCRIBE/SHOW committed-only state)** — Phase 65 inherits the same trade-off for ALTER helper TFs (D-10) and SHOW/DESCRIBE on-demand probes (D-16). Document the carry-over but do not attempt a fix; same DuckDB 1.5.2 constraint applies.
- **TECH-DEBT #23 (CREATE IF NOT EXISTS cross-process PK race)** — unchanged; not in scope.
- **TECH-DEBT #21 (`disable_peg_parser` resets override setting)** — unchanged upstream-blocked.
- **TECH-DEBT #24 (whitespace inside quoted source-table names)** — unchanged; rare.
- **RO→RW reverse direction hang** — covered by Plan 01 watchdog test B4 (`test_in_process_readonly_then_readwrite`) which fails on baseline and is expected to flip green as a side-effect of D-01 / H1 / H2 retirement. If it does NOT flip green, surface as a Phase 66 follow-up rather than expanding Phase 65 scope. Per D-09 (PRE-BPRIME).
- **Process-local type-inference cache eviction policy** — D-16 leaves the cache shape to planner discretion. If a bounded LRU lands, document it in TECH-DEBT (the Phase 61 / 62 LRU history is relevant prior art).
- **Long-lived native-handle audit across the rest of the extension** — D-22 / D-03 (PRE-BPRIME). Any findings during Plan 03-06 implementation get surfaced as TECH-DEBT entries or Phase 66 follow-ups; not absorbed into Phase 65 fix scope.

</deferred>

---

*Phase: 65-overridecontext-connection-teardown*
*Context gathered: 2026-05-23 via /gsd-discuss-phase 65 (third re-plan; under read-elimination architecture)*
