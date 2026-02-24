---
phase: 02-storage-and-ddl
plan: "04"
subsystem: database
tags: [duckdb, persistence, sidecar, pragma, restart, ddl]

# Dependency graph
requires:
  - phase: 02-storage-and-ddl
    provides: "catalog init_catalog, catalog_insert, catalog_delete; DDL scalar functions; SQLLogicTest harness"
provides:
  - "DDL-05 satisfied: semantic view definitions survive file-backed DuckDB restart via sidecar persistence"
  - "Host DB path resolution via PRAGMA database_list at entrypoint time"
  - "Sidecar file pattern: write_sidecar/read_sidecar for cross-restart persistence without DuckDB SQL from invoke"
affects: [03-expansion-engine, 04-query-interface, 05-hardening]

# Tech tracking
tech-stack:
  added: [serde_json (sidecar serialization)]
  patterns: [sidecar-file-persistence, pragma-database-list-path-resolution, atomic-rename-write]

key-files:
  created: []
  modified:
    - src/lib.rs
    - src/catalog.rs
    - src/ddl/define.rs
    - src/ddl/drop.rs
    - test/sql/phase2_ddl.test
    - .gitignore

key-decisions:
  - "sidecar-persistence: invoke cannot execute DuckDB SQL (execution locks deadlock); sidecar file (<db>.semantic_views) written with plain fs I/O bridges the gap; init_catalog reads sidecar on next load and syncs into DuckDB table"
  - "pragma-database-list-path: entrypoint queries PRAGMA database_list to resolve the host DB file path; takes first row with non-empty file (not filtered by name='main' because Python DuckDB names DBs by filename stem)"
  - "atomic-rename-write: sidecar writes use write-to-tmp-then-rename pattern for POSIX atomicity"

patterns-established:
  - "Sidecar persistence: scalar invoke writes HashMap as JSON to <db>.semantic_views; init_catalog merges sidecar into DuckDB table on next load"
  - "PRAGMA database_list path resolution: entrypoint queries pragma to get real file path; empty string maps to :memory: sentinel"
  - "Test idempotency: restart tests must clean up sidecar artifacts (drop views after verification)"

requirements-completed: [DDL-05]

# Metrics
duration: 5min
completed: 2026-02-24
---

# Phase 2 Plan 4: DDL-05 Gap Closure Summary

**Sidecar file persistence for DDL-05: definitions survive file-backed DuckDB restart via PRAGMA database_list path resolution and atomic JSON sidecar writes from invoke**

## Performance

- **Duration:** 5 min (verification and bug fix pass; code was implemented in prior session)
- **Started:** 2026-02-24T22:39:07Z
- **Completed:** 2026-02-24T22:44:05Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- DDL-05 requirement satisfied: semantic view definitions survive a real file-backed DuckDB restart, proven end-to-end by SQLLogicTest `restart` statement
- Host DB path resolved at entrypoint via `PRAGMA database_list` instead of hardcoded `:memory:` sentinel
- Sidecar persistence pattern (`write_sidecar`/`read_sidecar`) bridges the DuckDB execution-lock gap during invoke
- 16 Rust unit tests passing (including sidecar round-trip and pragma path tests)
- Both SQLLogicTest files passing (including restart section), idempotent across repeated runs

## Task Commits

Each task was committed atomically (code was implemented in prior session, this session verified and finalized):

1. **Task 1: Resolve host DB path via PRAGMA database_list** - `1e24914` (fix) + `824c60e` (fix: sidecar persistence)
2. **Task 2: Add restart section to SQLLogicTest** - `824c60e` (fix) + `13d4819` (fix: test idempotency)

**Plan metadata:** (pending)

## Files Created/Modified
- `src/lib.rs` - Extension entrypoint now queries `PRAGMA database_list` to resolve host DB file path; passes real path to `DefineState` and `DropState`
- `src/catalog.rs` - Added sidecar persistence: `write_sidecar`, `read_sidecar`, `sidecar_path`, `sync_table_from_map`; `init_catalog` reads sidecar and merges into DuckDB table on load
- `src/ddl/define.rs` - `invoke` writes sidecar after `catalog_insert` for file-backed databases
- `src/ddl/drop.rs` - `invoke` writes sidecar after `catalog_delete` for file-backed databases
- `test/sql/phase2_ddl.test` - Added section 10: DDL-05 restart test with file-backed DB, plus cleanup for idempotency
- `.gitignore` - Added `*.semantic_views` to exclude sidecar artifacts from version control

## Decisions Made
- **Sidecar persistence over Connection::open from invoke**: DuckDB holds execution locks during scalar `invoke`; opening a second connection deadlocks. Sidecar file (plain JSON written with filesystem I/O) is deadlock-free. `init_catalog` syncs sidecar into DuckDB table on next load.
- **PRAGMA database_list with non-empty file filter**: The main database name is NOT always "main" (Python DuckDB names it by filename stem). Filtering by first row with non-empty `file` column is more robust.
- **Atomic rename for sidecar writes**: Write to `.tmp` file then `rename()` -- atomic on POSIX, prevents partial writes.
- **v0.2 pragma_query_t**: The sidecar pattern is a v0.1 workaround. v0.2 C++ shim can use `pragma_query_t` callbacks to return SQL that DuckDB executes after the callback (no locks held), eliminating the sidecar.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Test not idempotent: sidecar persisted across test runs**
- **Found during:** Task 2 verification (restart section test)
- **Issue:** The SQLLogicTest runner's `delete_database` only removes `.db` and `.wal` files, not `.semantic_views` sidecar files. On second test run, `init_catalog` reads the leftover sidecar, finds the view already exists, and `define_semantic_view` fails with "already exists".
- **Fix:** Added `drop_semantic_view('restart_test')` after the post-restart assertion to empty the sidecar. Also added `*.semantic_views` to `.gitignore`.
- **Files modified:** `test/sql/phase2_ddl.test`, `.gitignore`
- **Verification:** `just test-sql` passes twice in a row (idempotent)
- **Committed in:** `13d4819`

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential for test reliability. No scope creep.

## Issues Encountered
- The sidecar file persistence pattern was not in the original plan (which assumed `Connection::open(db_path)` from invoke would work). The prior session discovered that DuckDB execution locks prevent SQL from invoke, and implemented the sidecar workaround. This session verified and finalized that work.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 2 fully complete: all 5 DDL requirements (DDL-01 through DDL-05) satisfied
- Ready for Phase 3 (Expansion Engine): `SemanticViewDefinition` model, catalog CRUD, and DDL functions are stable
- Sidecar persistence pattern documented for future phases to be aware of
- Risk: sidecar is a v0.1 workaround; v0.2 should replace with `pragma_query_t` C++ shim

## Self-Check: PASSED

- All 6 modified files exist on disk
- All 3 commit hashes (1e24914, 824c60e, 13d4819) verified in git log
- `cargo build`, `cargo clippy`, `cargo test` (16 passing), `just test-sql` (2/2 SUCCESS) all green

---
*Phase: 02-storage-and-ddl*
*Completed: 2026-02-24*
