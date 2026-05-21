---
phase: 65
slug: overridecontext-connection-teardown
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-21
---

# Phase 65 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Derived from RESEARCH.md §7. Anchored to DuckDB v1.5.2.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust unit + proptest, default `bundled` feature); `just test-sql` (sqllogictest); Python `unittest`/pytest-style integration tests under `test/integration/` |
| **Config file** | `Cargo.toml`, `justfile`, `test/integration/conftest.py` (if present) — no new framework |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~3–5 min for `just test-all`; ~30 s for `cargo test`; ~10 s for the new in-process RO suite |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full `just ci` must be green (lint + test-all + fuzz compile + docs-check)
- **Max feedback latency:** ~30 s (per-task `cargo test`)

---

## Per-Task Verification Map

Tasks not yet assigned — the planner populates Task IDs in PLAN frontmatter and references back into this table. The behaviours below are the verification contract the planner must thread tasks through. Each B-ID maps to a phase requirement (LIFE-NN) and either a Wave 0 test stub or an existing test.

| B-ID | Requirement | Behaviour | Test Type | Automated Command | File Exists |
|------|-------------|-----------|-----------|-------------------|-------------|
| B1 | LIFE-01 | After RW close+drop, `duckdb.connect(path, read_only=True)` returns within 5 s on a freshly bootstrapped DB | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_fresh` | ❌ W0 — new |
| B2 | LIFE-01 | After RW close+drop, RO reopen returns within 5 s on a previously-bootstrapped DB | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_existing` | ❌ W0 — new |
| B3 | LIFE-01 (isolation) | RW with **only `LOAD semantic_views`** (no CREATE) → close → RO reopen returns within 5 s. Proves leak is in extension load, not CREATE | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_load_only_then_readonly` | ❌ W0 — new |
| B4 | D-09 (CONTEXT.md) | RO→RW reverse direction also returns within 5 s (side-effect of root-cause cure) | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_readonly_then_readwrite` | ❌ W0 — new |
| B5 | LIFE-03 | Existing subprocess-bootstrap tests stay green (deployment-style smoke remains) | Python integration | `uv run test/integration/test_readonly_load.py::test_bootstrapped_readonly_query_works` (and siblings) | ✅ exists |
| B6 | Regression guard (Phase 62) | Phase 62 transactional DDL tests pass byte-identical | sqllogictest | `just test-sql` (`test/sql/v080_transactional_ddl.test`) | ✅ exists |
| B7 | Regression guard (Phase 62) | Phase 62 caret-rendering tests still pass | Python integration | `just test-caret` | ✅ exists |
| B8 | Regression guard | All read-side table functions (`list_semantic_views`, `describe_semantic_view`, `show_*`, `get_ddl`, `read_yaml_from_semantic_view`) return correct results under per-call connection | sqllogictest | `just test-sql` (various) | ✅ exists |
| B9 | Regression guard (Phase 61) | Multi-DB isolation test passes | Python integration | `just test-multi-db` | ✅ exists |
| B10 | Regression guard (Phase 60) | Concurrent CREATE behaviour unchanged | Python integration | `just test-concurrent` | ✅ exists |
| B11 | LIFE-02 (audit) | Repeated LOAD+close in one process (50 file-backed DBs) — no busy-spin, RSS bounded | Python integration | new test in `test_readonly_load.py` or `test_multi_db_isolation.py` | ❌ W0 — new |
| B12 | Regression guard (Phase 58) | ADBC transactional DDL still passes | Python integration | `just test-adbc` | ✅ exists |
| B13 | LIFE-02 (structural) | `OverrideContext` carries `db_handle: duckdb_database`, NOT `duckdb_connection` / `CatalogReader`. Static guard or compile-time / grep audit | Rust unit + grep audit | `cargo test --lib --features extension` + `rg "OverrideContext.*conn:\|catalog: CatalogReader"` returns nothing | ❌ W0 — new |
| B14 | LIFE-02 (structural) | RAII guard type wraps `duckdb_connect` / `duckdb_disconnect`; Drop closes exactly once | Rust unit + proptest | new test in `src/parse.rs` or new `src/conn_guard.rs` | ❌ W0 — new |

*Status legend (filled in during execution): ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/integration/test_readonly_load.py` — add B1, B2, B3, B4, B11 + `_connect_with_watchdog` helper (RESEARCH §7.3). Keep subprocess-based tests intact (B5).
- [ ] `src/conn_guard.rs` (new) OR `src/parse.rs` extension — RAII guard wrapping `duckdb_connect`/`duckdb_disconnect`, Drop closes once. proptest coverage that Drop is idempotent and exception-safe.
- [ ] `src/lib.rs` — refactor `init_extension` so it does **not** own a long-lived `duckdb_connection`. Pass `db_handle: duckdb_database` into `OverrideContext` and into `QueryState`. Rewire all read-side `register_table_function_with_extra_info` call sites.
- [ ] `src/catalog.rs::CatalogReader` — refactor: accept `db_handle` and connect/disconnect per method, or accept a borrowed `duckdb_connection` whose lifetime is owned by the caller's `ConnGuard`.
- [ ] **Structural guard (B13)** — either a `#[deny]`-style compile-time check, a doc-test that fails to compile if `OverrideContext` re-acquires a connection field, or a `cargo test` that greps the binary for the offending field name.

---

## Manual-Only Verifications

| Behaviour | Requirement | Why Manual | Test Instructions |
|-----------|-------------|------------|-------------------|
| LIFE-04 ledger update | LIFE-04 | Editing `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` is a documentation deliverable, not a runtime behaviour | Verify the entry "In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)" is marked resolved with a forward pointer to v0.9.1 / commit SHA |
| Watchdog-thread leak documentation | LIFE-01 (pre-fix only) | The Python watchdog cannot kill a busy-spinning DuckDB thread; on the v0.9.0 baseline the watchdog test leaks its thread for the process lifetime. Documented behaviour, not failure | Confirm test docstring + comment in `_connect_with_watchdog` explains the leak and recommends running these tests last in the file under `pytest-timeout` |

---

## Watchdog Test Pattern (RESEARCH §7.3 reference)

The new in-process tests MUST use a daemon-thread watchdog (sketched in RESEARCH §7.3) rather than blocking the main thread on `duckdb.connect`. On v0.9.0 baseline:

- `duckdb.connect(path, read_only=True)` busy-spins in C++; Python cannot interrupt it.
- The daemon thread leaks for the rest of the process lifetime — **acceptable** for a fail-once regression test.
- Run these tests **last in the file** and under `pytest-timeout` (or equivalent) to keep test-suite hygiene.

On v0.9.1 the thread returns cleanly. The leak-on-failure behaviour is the cost of having a regression test that cannot be killed; documented inline in the test file.

---

## Pinned Regressions to Re-run After Fix

After the structural refactor lands, re-run these existing scenarios — they exercise paths that touched the leaked `catalog_conn` / `query_conn`:

1. `just test-sql` — Phase 62 transactional DDL (`test/sql/v080_transactional_ddl.test`)
2. `just test-caret` — Phase 62 caret rendering
3. `just test-multi-db` — Phase 61 LRU removal sequel
4. `just test-concurrent` — Phase 60 race guards
5. `just test-adbc` — Phase 58 ADBC autocommit=false
6. All read-side table-function sqllogictests (the `&catalog_reader` Copy path)

---

## Validation Sign-Off

- [ ] All tasks have automated verify or are wired to a Wave 0 dependency above
- [ ] Sampling continuity: no 3 consecutive tasks without an automated verify
- [ ] Wave 0 covers all ❌ rows in the per-task table
- [ ] No watch-mode flags in commands
- [ ] Feedback latency under ~30 s for per-task `cargo test`
- [ ] `nyquist_compliant: true` set in frontmatter once tasks are mapped

**Approval:** pending
