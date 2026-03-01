---
phase: 10-pragma-query-t-catalog-persistence
plan: 02
subsystem: database
tags: [duckdb, rust, ffi, pragma, ddl, persistence]

requires:
  - phase: 10-01
    provides: Rust FFI declarations (shim::ffi::semantic_views_pragma_define/drop)

provides:
  - DefineState with persist_conn replacing db_path — write-first pragma FFI pattern
  - DropState with persist_conn replacing db_path — write-first pragma FFI pattern
  - lib.rs creates persist_conn for file-backed databases at init time

affects: [10-03, 11-create-semantic-view-ddl]

tech-stack:
  added: []
  patterns: [write-first persist, separate-connection DDL pattern, cfg-feature-gated FFI call sites]

key-files:
  created: []
  modified:
    - src/ddl/define.rs
    - src/ddl/drop.rs
    - src/lib.rs

key-decisions:
  - "Write-first ordering: FFI call before HashMap update ensures HashMap never diverges from committed table state"
  - "persist_conn = None for :memory: databases — no-op FFI path, HashMap is sole source of truth"
  - "unsafe impl Send + Sync on DefineState/DropState — duckdb_connection raw pointer safety delegation to DuckDB"
  - "#[cfg(feature = 'extension')] gates FFI call sites — cargo test (bundled feature) compiles without shim.cpp"

patterns-established:
  - "Write-first: persist to table BEFORE HashMap update; on FFI error, HashMap unchanged"
  - "persist_conn pattern: separate duckdb_connection for invoke DDL writes to avoid execution lock deadlock"
  - "Feature-gated FFI: call sites wrapped in #[cfg(feature = 'extension')] matching FFI declaration gate"

requirements-completed: [PERSIST-01, PERSIST-02]

duration: 15min
completed: 2026-03-01
---

# Plan 10-02: Replace sidecar writes with pragma FFI calls in define/drop

**write_sidecar removed from invoke; write-first pragma FFI pattern with separate persist_conn connection**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-03-01T00:15:00Z
- **Completed:** 2026-03-01T00:30:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Removed `write_sidecar` from `DefineSemanticView` and `DropSemanticView` invoke
- Replaced `db_path: Arc<str>` with `persist_conn: Option<duckdb_connection>` in both state structs
- Implemented write-first pattern: FFI call before HashMap update ensures consistency
- lib.rs creates `persist_conn` at init time for file-backed databases; None for :memory:

## Task Commits

1. **Task 1: define.rs + drop.rs** - `0d7d08d` (feat)
2. **Task 2: lib.rs** - `911c93a` (feat)

## Files Created/Modified
- `src/ddl/define.rs` - DefineState has persist_conn; invoke writes to table first via FFI
- `src/ddl/drop.rs` - DropState has persist_conn; invoke deletes from table first via FFI
- `src/lib.rs` - Creates persist_conn via duckdb_connect; passes to both states

## Decisions Made
- Write-first ordering: if FFI fails, HashMap stays unchanged (PERSIST-02 write-first semantics)
- In-memory databases get `persist_conn = None` — they use HashMap only (correct: in-memory doesn't persist anyway)
- `unsafe impl Send + Sync` on state structs: DuckDB handles concurrent connection access internally

## Deviations from Plan
- Pre-commit rustfmt hook reformatted `unsafe { ... }` block on one line — auto-fixed by running `cargo fmt` before staging

## Issues Encountered
- Pre-commit hook (rustfmt) reformatted `unsafe { crate::shim::ffi::semantic_views_pragma_drop(...) }` differently from what was written — resolved by running `cargo fmt` before staging

## Next Phase Readiness
- Plan 10-03 can proceed: sidecar write path removed; now clean up sidecar read/functions from catalog.rs

---
*Phase: 10-pragma-query-t-catalog-persistence*
*Completed: 2026-03-01*
