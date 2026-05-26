# Phase 66: Expansion Qualification Across All Paths + ADBC Tests - Context

**Gathered:** 2026-05-26
**Status:** Ready for planning
**Source:** `/gsd-discuss-phase 66 --assumptions` (decisions confirmed inline)

<domain>
## Phase Boundary

Make `FROM semantic_view(...)` resolve correctly through any client (ADBC and other clients whose catalog/schema context state differs from the extension's per-call Connection defaults) by ensuring every expansion path emits fully-qualified table references.

This is the technical milestone-finisher for v0.10.0 — the surviving Phase 66 requirements are EXPAND-CTX-01, EXPAND-CTX-02, EXPAND-CTX-03.

**Out of scope (deferred to milestone close, NOT this phase):**
- REL-01: `## [0.10.0]` CHANGELOG section + `[Unreleased]` reset + compare-link updates
- REL-02: `Cargo.toml` + `description.yml` bump to `0.10.0`
- REL-03: milestone example file under `examples/`
- Tagging, squash-merge to `main`, `just clean-stale`

Rationale: per user feedback `feedback_defer_release_tasks.md` — release prep happens at milestone close, not folded into the last technical phase. Even though ROADMAP.md lists REL-* against Phase 66, the phase is scoped to technical work only.

</domain>

<decisions>
## Implementation Decisions

### Architectural premise (root-cause analysis)

*All decisions in this section are `[informational]` — they document the WHY behind the migration, not actionable plan line-items. They drive D-04 onwards.*

- **D-01:** [informational] Phase 65 retired both long-lived extension-owned `duckdb_connection` handles. Read-side bind callbacks now open per-call `Connection(*context.db)` from the caller's `ClientContext`. This connection **borrows the DatabaseInstance** so it inherits all `ATTACH`ed databases and their schemas, but **does NOT inherit the caller's `ClientContext` state** — meaning the caller's `USE db.schema` and any session `search_path` do not propagate. The per-call Connection's defaults snap to `*context.db`'s defaults (typically the main DB it was opened against, schema `main`).
- **D-02:** [informational] Therefore, `quote_table_ref(name)` → `"name"` (unqualified) resolves correctly **iff** the physical table lives in `*context.db`'s default catalog + `main` schema. Anywhere else (non-default schema base table, attached-DB base table, materialization target in a non-default location), unqualified emission fails with `Catalog Error: Table with name X does not exist`. `qualify_and_quote_table_ref(name, def)` → `"database"."schema"."name"` (qualified, using stored `database_name`/`schema_name` metadata) resolves regardless of search-path state.
- **D-03:** [informational] Phase 64 added `qualify_and_quote_table_ref` and wired it into 3 sites in the main expansion path (`src/expand/sql_gen.rs:499, 530, 550`) — that fix was scoped to the v0.9.0 quoted-identifier bug-reporter's path. The remaining 7 unmigrated sites are an **incomplete Phase 64**, not an intentional asymmetry. The fully-qualified emission is what stored `database_name`/`schema_name` metadata is FOR; the qualified form is the safe + correct emission across all contexts.

### EXPAND-CTX-01: Migrate all 7 unmigrated sites

- **D-04:** Migrate all 7 sites mechanically from `quote_table_ref(name)` to `qualify_and_quote_table_ref(name, def)`:
  - `src/expand/sql_gen.rs:181, 224, 244` (fact-query path)
  - `src/expand/semi_additive.rs:195, 220, 238` (semi-additive metric inner subqueries)
  - `src/expand/window.rs:156, 181, 199` (window metric inner subqueries)
  - `src/expand/materialization.rs:157` (materialization routing target)
- **D-05:** [informational] This is defense-in-depth completion of Phase 64's intent, NOT speculative. Each migration corresponds to a concrete failure mode (cross-schema base table, cross-DB base table, cross-schema materialization target). Bounded test-driven migration was rejected because the roadmap's 5 success-criteria scenarios all happen to create tables in `memory.main` — they don't exercise the failure mode at all, so "passes the current test set" would be a false-negative signal.
- **D-06:** Each migration site's `def` argument is already available in the surrounding code (the helper is a method-equivalent that needs the `SemanticViewDefinition` for the qualifier metadata). Researcher should verify the `def` binding is in scope at each call site; this is a mechanical refactor with predictable failure modes.

### EXPAND-CTX-02: ADBC end-to-end query test

- **D-07:** Create `test/integration/test_adbc_queries.py` using `adbc_driver_duckdb` Python bindings. Pattern matches the existing `test_adbc_transactions.py` shape.
- **D-08:** Test matrix expanded beyond the roadmap's 5 baseline scenarios. The roadmap scenarios alone (a-e) are insufficient — they all create tables in default catalog/schema, so they don't exercise the unqualified-resolution failure mode. **Minimum scenarios:**
  1. **Main expand path, default schema** — baseline; should pass on `v0.9.0` HEAD too if the Phase 65 dissolution hypothesis holds.
  2. **Main expand path, non-default schema base table** — `CREATE SCHEMA staging; CREATE TABLE staging.t; CREATE SEMANTIC VIEW v (TABLES (staging.t AS x) ...);` — exercises whether main path qualification still works under ADBC.
  3. **FACTS feature path, non-default schema base table** — regression guard for `sql_gen.rs:181/224/244` migration.
  4. **Semi-additive metric, non-default schema base table** — regression guard for `semi_additive.rs:195/220/238`.
  5. **Window metric, non-default schema base table** — regression guard for `window.rs:156/181/199`.
  6. **Materialization routing to non-default-schema target** — regression guard for `materialization.rs:157`. Setup: `CREATE SCHEMA agg; CREATE TABLE agg.daily_revenue; CREATE SEMANTIC VIEW v (... MATERIALIZATIONS (m (target_table => 'agg.daily_revenue', ...)));` — query that routes to the materialization.
  7. **Multi-DB ATTACH** — `ATTACH 'other.db' AS db2; CREATE TABLE db2.main.sales; CREATE SEMANTIC VIEW db2.main.v (...); FROM semantic_view('db2.main.v', ...);` — exercises whether per-call `Connection(*context.db)` correctly resolves attached-DB tables. Extend this scenario to also use a FACTS or semi-additive metric so the unwired-path failure surface is exercised across catalogs.
- **D-09:** Each scenario must (a) fail on a v0.9.0 baseline (pre-Phase-64 main-path wiring + pre-Phase-65 query_conn divergence) for at least one scenario, AND (b) fail on a pre-EXPAND-CTX-01 milestone/v0.10.0 baseline for the FACTS/semi-additive/window/materialization scenarios specifically, AND (c) pass after the EXPAND-CTX-01 migration lands.
- **D-10:** New `just test-adbc-queries` recipe added to `justfile`. Added to `test-all` aggregate (the recipe at `justfile:149`). Pattern: mirror the existing `test-adbc` recipe at `justfile:111` (which runs `test_adbc_transactions.py`).

### EXPAND-CTX-03: `_notes/error_with_adbc.md` close-out

- **D-11:** Update `_notes/error_with_adbc.md` with a closing note pointing at the v0.10.0 fix (rather than deleting). The note has historical value as the downstream-reporter context. Add a header section: `## Resolution (v0.10.0)` summarising the fix in 2-3 sentences. Optionally archive to `_notes/archive/` if that pattern exists.

### Risk surface to investigate during planning (NOT decisions, surface for researcher)

- **R-01:** Existing sqllogictests may assert unqualified `FROM "name"` shape in FACTS/semi-additive/window/materialization expansion. Grep up-front: `rg 'FROM "[^.]*"$' test/sql/` and similar shapes. If found, decide per-test: update fixture to expect qualified shape, or argue the original was over-specified.
- **R-02:** `qualify_and_quote_table_ref` behavior with partial metadata: what does it emit when `database_name` is set but `schema_name` is empty, or vice versa? The existing 3 wired sites (`sql_gen.rs:499, 530, 550`) have presumably exercised these — researcher should confirm and reference the helper's source for the fallback ordering. If undefined, the planner needs to call this out as an edge case.
- **R-03:** ADBC bootstrap pattern. `test_adbc_transactions.py` is the template. The Phase 63 bootstrap-in-subprocess pattern was specifically to avoid the OverrideContext-driven in-process hang — Phase 65 retired that whole hazard class, so in-process bootstrap should be safe now. Researcher to confirm there's no remaining ADBC-specific reason to subprocess-bootstrap.
- **R-04:** `Connection(*context.db)` does NOT inherit caller's `USE` state. If a caller did `USE staging` and then `FROM semantic_view('memory.main.v', ...)`, the per-call Connection still defaults to `*context.db`'s default catalog+schema. This is the architectural reason qualified emission is necessary — it's not a bug we can fix on the Connection side; it's why D-04 exists. The researcher should NOT propose a "propagate ClientContext state to per-call Connection" workaround.

### Claude's discretion (planner decides)

- Whether the 7 migrations land as one plan or split per file (single coordinated plan is likely cleaner — the migrations are mechanically identical and the test suite covers them together)
- Specific test file structure within `test_adbc_queries.py` (one scenario per function with shared fixtures, vs. parameterized)
- Whether to add additional scenarios beyond the 7 in D-08 if research surfaces other failure modes
- Order of plans (likely: Plan 01 = test scaffolding written against current HEAD demonstrating failures; Plan 02 = migration of 7 sites + sqllogictest fixture updates; Plan 03 = `_notes/error_with_adbc.md` close-out — but planner may collapse Plans 02-03 if small)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase 65 deliverables (the architecture that dissolves the original root cause)
- `.planning/phases/65-overridecontext-connection-teardown/65-VERIFICATION.md` — Phase 65 closeout; both long-lived connections retired
- `.planning/phases/65-overridecontext-connection-teardown/65-06-SUMMARY.md` — final lifecycle close-out; per-call `Connection(*context.db)` model in place
- `cpp/src/shim.cpp` — `sv_register_table_function`, all 17 read-side bind callbacks using BORROW contract (`reinterpret_cast<duckdb_connection>(Connection*)`)
- `tests/no_long_lived_conn.rs` — structural guard that no extension code re-introduces long-lived `duckdb_connection`

### Phase 64 deliverables (the helper being completed)
- `src/ident.rs` — quoted-identifier parser introduced by Phase 64
- `src/expand/resolution.rs` — defines `quote_table_ref` and `qualify_and_quote_table_ref`; researcher MUST read both implementations and confirm `qualify_and_quote_table_ref`'s metadata-fallback semantics (R-02)
- `src/expand/sql_gen.rs:499, 530, 550` — the 3 already-wired sites; treat as the migration template for D-04

### Migration target sites (the 7 unmigrated call sites)
- `src/expand/sql_gen.rs:181, 224, 244` — fact-query path
- `src/expand/semi_additive.rs:195, 220, 238`
- `src/expand/window.rs:156, 181, 199`
- `src/expand/materialization.rs:157`

### Test scaffolding references
- `test/integration/test_adbc_transactions.py` — direct shape template for `test_adbc_queries.py`
- `test/integration/test_multi_db_isolation.py` — multi-DB ATTACH test pattern (DDL isolation, not expansion correctness — but the ATTACH setup is reusable)
- `justfile:111` — `test-adbc` recipe shape; mirror for new `test-adbc-queries`
- `justfile:149` — `test-all` aggregate; new recipe inserted here

### Source context (downstream report)
- `_notes/error_with_adbc.md` — original downstream bug report ("Catalog Error: Table with name sales_data does not exist! Did you mean memory.sales_data?"); EXPAND-CTX-03 close-out target

### Project-wide rules
- `CLAUDE.md` — build/test command rules (Rule 1: no bare `tail -N` on long-running commands — applies to ADBC test runs which may take several seconds; Rule 2: sandbox bypass for build/test list)
- Quality gate is `just test-all`. New `test-adbc-queries` recipe is part of this aggregate.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- **`qualify_and_quote_table_ref` helper**: already implemented in `src/expand/resolution.rs`. Used at 3 sites; the 7 migration targets call the sibling `quote_table_ref`. Mechanical swap.
- **`SemanticViewDefinition` `def` parameter availability**: the helper needs `&def` for the metadata. All 7 call sites are inside functions that already have `def` in scope (they're expanding a specific view) — researcher to confirm trivially.
- **ADBC test shape**: `test_adbc_transactions.py` provides bootstrap+connect+ADBC-driver+assertion pattern. Test runner via `uv run` (see `justfile:112`).
- **`test_multi_db_isolation.py`** ATTACH scenario setup is directly reusable for the EXPAND-CTX-02 multi-DB scenario.

### Established Patterns
- **Migration unit**: each site is `sql.push_str(&quote_table_ref(table_var))` → `sql.push_str(&qualify_and_quote_table_ref(table_var, def))`. No surrounding logic changes.
- **sqllogictest fixture style**: existing tests in `test/sql/` use the block form for `statement error` assertions (per CLAUDE.md). New tests added to `test/sql/TEST_LIST` per Phase 63 lesson.
- **Per-call Connection model**: every read-side bind callback in `cpp/src/shim.cpp` opens `Connection conn(*context.db);` and runs SQL on it. The connection's catalog state is `*context.db`'s defaults — this is the architectural fact behind D-02.

### Integration Points
- `src/expand/sql_gen.rs` — entry point for expansion; multiple feature paths branch from `build_inner_sql` or equivalent
- `src/expand/resolution.rs` — `quote_table_ref` and `qualify_and_quote_table_ref` both live here; researcher should NOT propose a third helper
- `justfile` — recipe additions, see `test-adbc` shape

</code_context>

<deferred>
## Deferred Ideas

- **Propagate caller's `ClientContext` state (USE / search_path) into per-call Connection** — explicitly rejected (R-04). The qualified-emission approach is the architecturally correct fix. ClientContext-propagation would couple the extension to DuckDB internals and provide no benefit over the qualified-emission approach for the scenarios in scope.
- **Type-level enforcement that expansion code can't call `quote_table_ref` directly** — could enforce qualified-emission via making `quote_table_ref` private or renaming, but adds friction without proven need. If a future regression re-introduces unqualified emission, the ADBC test suite catches it. Defer as TECH-DEBT if it ever happens again.
- **REL-01 / REL-02 / REL-03 work** — milestone-close release tasks per `feedback_defer_release_tasks.md`: `## [0.10.0]` CHANGELOG section, `Cargo.toml` + `description.yml` bump, milestone example file under `examples/`. These happen at milestone close (after Phase 66 verification passes), NOT as part of Phase 66.
- **Cross-DB materialization scenarios beyond the single multi-DB ATTACH scenario in D-08** — the test matrix already covers the failure mode; additional cross-DB permutations are extra surface area without new signal. If research surfaces a specific edge case, planner may add; otherwise out of scope.

</deferred>

---

*Phase: 66-expansion-qualification-adbc-tests*
*Context gathered: 2026-05-26 via `/gsd-discuss-phase 66 --assumptions`*
