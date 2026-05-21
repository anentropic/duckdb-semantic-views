# Requirements: DuckDB Semantic Views — v0.9.1

**Defined:** 2026-05-21
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## Milestone Goal

Fix two downstream-reported regressions that both stem from the extension owning long-lived connections separate from the caller's:

1. **In-process RW→RO reopen hang** — the `OverrideContext` catalog connection (introduced in Phase 62, v0.8.0) keeps the underlying DuckDB `Database` alive past the caller's `close()`, so reopening the same path in a different access mode in the same process hangs in DuckDB's `DatabaseManager`.
2. **Cross-connection `FROM semantic_view(...)` fails** — the inner expanded SQL runs on the extension's `query_conn` (created at LOAD time), not the caller's connection. Through ADBC, dbt, or any client whose catalog/schema search path diverges from `query_conn`, unqualified table references in the expanded SQL surface as `Catalog Error: Table with name X does not exist`.

Phase 64 (v0.9.0) introduced `qualify_and_quote_table_ref` in `src/expand/resolution.rs`, but wired it into the main `expand()` path only. The fact-query / semi-additive / window / materialization-routing expansion paths still emit unqualified physical table references.

## v1 Requirements (v0.9.1)

### Connection Lifecycle (RO-REOPEN)

- [ ] **LIFE-01**: After a writable DuckDB connection that did `LOAD semantic_views` is closed in the **same Python process**, a subsequent `duckdb.connect(path, read_only=True)` returns within 5 seconds (vs. the current indefinite hang). Apply to both fresh-bootstrap-then-RO and previously-bootstrapped-then-RO.
- [ ] **LIFE-02**: The fix is either (a) deterministic teardown of the `OverrideContext` `duckdb_connection` at extension-unload / DBConfig destruction, or (b) detect access-mode mismatch on reopen inside `init_extension` and surface an actionable error instead of hanging. Choice and reasoning documented in the phase RESEARCH.md.
- [ ] **LIFE-03**: `test/integration/test_readonly_load.py` gains a `test_in_process_bootstrap_then_readonly` scenario that does the bootstrap-then-RO sequence **without** the subprocess workaround and asserts the read-only connect returns under a watchdog. The existing subprocess-based tests stay as deployment-style smoke.
- [ ] **LIFE-04**: Phase 63's `deferred-items.md` entry "In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)" is updated with the resolution and a forward pointer to v0.9.1.

### Cross-Connection Expansion Qualification (EXPAND-CTX)

- [ ] **EXPAND-CTX-01**: Every expansion site that emits a physical table reference uses `qualify_and_quote_table_ref` (or an equivalent qualifier) — not the raw `quote_table_ref`. Sites in scope: `src/expand/sql_gen.rs:181, 224, 244` (fact-query path), `src/expand/semi_additive.rs:195, 220, 238`, `src/expand/window.rs:156, 181, 199`, `src/expand/materialization.rs:157`. After the fix, no unqualified `FROM "<table>"` shape appears in the expansion output of any view that has `database_name` / `schema_name` metadata.
- [ ] **EXPAND-CTX-02**: ADBC end-to-end query test (`test/integration/test_adbc_queries.py`, runnable via `just test-adbc-queries`) covers `SELECT … FROM semantic_view(...)` through `adbc_driver_duckdb` for at least: (a) main expansion path, (b) a view with FACTS, (c) a view with a semi-additive metric, (d) a view with a window metric, (e) a multi-database `ATTACH 'other.db' AS db2; CREATE SEMANTIC VIEW db2.main.v AS …; FROM semantic_view('db2.main.v', …)` scenario. The test must fail without the EXPAND-CTX-01 changes and pass with them.
- [ ] **EXPAND-CTX-03**: `_notes/error_with_adbc.md` is either deleted (resolved) or updated with the resolution and a pointer to the v0.9.1 fix.

### Release (REL)

- [ ] **REL-01**: `CHANGELOG.md` has a `## [0.9.1]` section under `### Fixed` describing both fixes in user-facing terms; the `[Unreleased]` block is reset to `_No unreleased changes yet._`; the bottom-of-file compare link is updated (`[0.9.1]` and updated `[Unreleased]` link target).
- [ ] **REL-02**: `Cargo.toml` and `description.yml` bumped to `0.9.1`. `just test-all` and `just ci` green on the milestone branch.

## Traceability

| Requirement   | Phase    | Status  |
|---------------|----------|---------|
| LIFE-01       | Phase 65 | Pending |
| LIFE-02       | Phase 65 | Pending |
| LIFE-03       | Phase 65 | Pending |
| LIFE-04       | Phase 65 | Pending |
| EXPAND-CTX-01 | Phase 66 | Pending |
| EXPAND-CTX-02 | Phase 66 | Pending |
| EXPAND-CTX-03 | Phase 66 | Pending |
| REL-01        | Phase 66 | Pending |
| REL-02        | Phase 66 | Pending |

Coverage: 9/9 requirements mapped (100%).

## v2 / Future / Out of Scope

Out of scope for this milestone (carried over from prior milestones unless renegotiated):

- TECH-DEBT #19 (DESCRIBE/SHOW read committed state) — blocked on `libduckdb-sys` exposing `BindInfo`'s connection handle.
- TECH-DEBT #21 (`disable_peg_parser` resets override setting) — upstream DuckDB change required.
- TECH-DEBT #23 (`CREATE IF NOT EXISTS` cross-process PK race) — DuckDB PK enforcement; not a parser_override hook.
- TECH-DEBT #24 (whitespace inside quoted source-table names in `TABLES (...)`) — vanishingly rare; deferred per v0.9.0 Phase 64 scope decision.
- Re-routing read-side table functions (`list_semantic_views`, `describe_semantic_view`, `show_semantic_*`, `get_ddl`, `read_yaml_from_semantic_view`) onto the caller's connection — blocked on the same `BindInfo` connection-handle exposure as TECH-DEBT #19. If that exposure becomes possible mid-v0.9.1, fold into scope; otherwise leave for a later milestone that also retires the catalog connection.
