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

- [x] **LIFE-01** (satisfied — Plan 06 retired H1 + 12/12 watchdog tests PASS on milestone/v0.10.0): After a writable DuckDB connection that did `LOAD semantic_views` is closed in the **same Python process**, a subsequent `duckdb.connect(path, read_only=True)` returns within 5 seconds (vs. the v0.9.0 indefinite hang). Both fresh-bootstrap-then-RO and previously-bootstrapped-then-RO scenarios verified. Watchdog evidence base: B1-B4 + B11 (Plan 01) + 4 D-03b post-reopen tests (Plan 06 Task 3) = 9 in-process tests + 3 subprocess-style baseline tests = 12/12 PASS. Mechanism in `65-06-SUMMARY.md`.
- [x] **LIFE-02** (satisfied — Plan 05; mechanism path): The fix is either (a) deterministic teardown of the `OverrideContext` `duckdb_connection` at extension-unload / DBConfig destruction, or (b) detect access-mode mismatch on reopen inside `init_extension` and surface an actionable error instead of hanging. **Resolution:** the read-side `OverrideContext` lifecycle is now per-call `Connection(*context.db)` at bind time — no extension-owned `duckdb_connection` is held past a single read. Plan 06 closed the H1 catalog_conn that still imitated the v0.9.0 leak. Mechanism documented in `65-05-SUMMARY.md` + `65-06-SUMMARY.md` + 65-RESEARCH.md §1.3 + 65-05-SPIKE-SUMMARY.md.
- [x] **LIFE-03** (satisfied — Plan 01 `4da68eb` landed the test; Plan 05 verified the test surface stays semantically correct; Plan 06 extended with 4 D-03b post-reopen tests covering all major read paths): `test/integration/test_readonly_load.py` gains a `test_in_process_bootstrap_then_readonly` scenario that does the bootstrap-then-RO sequence **without** the subprocess workaround and asserts the read-only connect returns under a watchdog. The existing subprocess-based tests stay as deployment-style smoke.
- [x] **LIFE-04** (satisfied — Plan 06 commit `06246dc`): Phase 63's `deferred-items.md` entry "In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)" is updated with "Status: RESOLVED in v0.10.0" + forward pointer to `.planning/phases/65-overridecontext-connection-teardown/` + commit SHAs for H1 retirement (`964b0bf`) and H2 retirement (Plan 05). Watchdog evidence cross-referenced.

### Cross-Connection Expansion Qualification (EXPAND-CTX)

- [x] **EXPAND-CTX-01**: Every expansion site that emits a physical table reference uses `qualify_and_quote_table_ref` (or an equivalent qualifier) — not the raw `quote_table_ref`. Sites in scope: `src/expand/sql_gen.rs:181, 224, 244` (fact-query path), `src/expand/semi_additive.rs:195, 220, 238`, `src/expand/window.rs:156, 181, 199`, `src/expand/materialization.rs:157`. After the fix, no unqualified `FROM "<table>"` shape appears in the expansion output of any view that has `database_name` / `schema_name` metadata.
- [x] **EXPAND-CTX-02**: ADBC end-to-end query test (`test/integration/test_adbc_queries.py`, runnable via `just test-adbc-queries`) covers `SELECT … FROM semantic_view(...)` through `adbc_driver_duckdb` for at least: (a) main expansion path, (b) a view with FACTS, (c) a view with a semi-additive metric, (d) a view with a window metric, (e) a multi-database `ATTACH 'other.db' AS db2; CREATE SEMANTIC VIEW db2.main.v AS …; FROM semantic_view('db2.main.v', …)` scenario. The test must fail without the EXPAND-CTX-01 changes and pass with them.
- [x] **EXPAND-CTX-03**: `_notes/error_with_adbc.md` is either deleted (resolved) or updated with the resolution and a pointer to the v0.9.1 fix.

### Release (REL)

- [ ] **REL-01**: `CHANGELOG.md` has a `## [0.10.0]` section describing the milestone in user-facing terms (LIFE-01..04 read-only-reopen fix; EXPAND-CTX-01..03 expansion qualification defense-in-depth); the `[Unreleased]` block is reset to `_No unreleased changes yet._`; the bottom-of-file compare link is updated (`[0.10.0]` and updated `[Unreleased]` link target). (Texts above mention `v0.9.1` because the milestone was reframed mid-flight; user-facing release is `v0.10.0`.)
- [ ] **REL-02**: `Cargo.toml` and `description.yml` bumped to `0.10.0`. `just test-all` and `just ci` green on the milestone branch.
- [ ] **REL-03**: New Python example file under `examples/` demonstrating the v0.10.0 read-only-reopen story and the cross-connection expansion fix (matching the style of `basic_ddl_and_query.py`, `advanced_features.py`, etc.).
- [ ] **REL-04**: DuckDB dependency bumped from `v1.5.2` → `v1.5.3`: `.duckdb-version`, `.github/workflows/BuildAll.yml` and `BuildQuick.yml` ref tags + `duckdb_version` / `ci_tools_version`, `Cargo.toml` (`duckdb = "=1.10503.0"`, `libduckdb-sys = "=1.10503.0"`), and `Cargo.lock`. The DuckDB Version Monitor flagged v1.5.3 compat break (`FlatVector::as_mut_slice` became `unsafe`) on PR #34 against `main`; the failing call site was eliminated by Phase 65's read-elimination architecture, so the milestone-close bump should land cleanly. PR #34 closed as superseded.

## Traceability

| Requirement   | Phase    | Status                              |
|---------------|----------|-------------------------------------|
| LIFE-01       | Phase 65 | **Satisfied (Plan 06 — 12/12 watchdog PASS)** |
| LIFE-02       | Phase 65 | **Satisfied (Plan 05 mechanism)**   |
| LIFE-03       | Phase 65 | **Satisfied (Plan 01 test landed; Plan 06 extended)** |
| LIFE-04       | Phase 65 | **Satisfied (Plan 06 ledger close)** |
| EXPAND-CTX-01 | Phase 66 | Complete |
| EXPAND-CTX-02 | Phase 66 | Complete |
| EXPAND-CTX-03 | Phase 66 | Complete |
| REL-01        | Milestone close | Pending |
| REL-02        | Milestone close | Pending |
| REL-03        | Milestone close | Pending |
| REL-04        | Milestone close | Pending |

Coverage: 11/11 requirements mapped (100%).

## v2 / Future / Out of Scope

Out of scope for this milestone (carried over from prior milestones unless renegotiated):

- TECH-DEBT #19 (DESCRIBE/SHOW read committed state) — blocked on `libduckdb-sys` exposing `BindInfo`'s connection handle.
- TECH-DEBT #21 (`disable_peg_parser` resets override setting) — upstream DuckDB change required.
- TECH-DEBT #23 (`CREATE IF NOT EXISTS` cross-process PK race) — DuckDB PK enforcement; not a parser_override hook.
- TECH-DEBT #24 (whitespace inside quoted source-table names in `TABLES (...)`) — vanishingly rare; deferred per v0.9.0 Phase 64 scope decision.
- Re-routing read-side table functions (`list_semantic_views`, `describe_semantic_view`, `show_semantic_*`, `get_ddl`, `read_yaml_from_semantic_view`) onto the caller's connection — blocked on the same `BindInfo` connection-handle exposure as TECH-DEBT #19. If that exposure becomes possible mid-v0.9.1, fold into scope; otherwise leave for a later milestone that also retires the catalog connection.
