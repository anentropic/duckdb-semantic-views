---
phase: 11-create-semantic-view-parser-hook
plan: 03
subsystem: database
tags: [rust, ffi, ddl, cleanup]

# Dependency graph
requires:
  - phase: 11-create-semantic-view-parser-hook
    plan: 01
    provides: catalog_upsert, catalog_delete_if_exists, FFI catalog functions, updated CatalogState
provides:
  - define.rs and drop.rs deleted (DDL-05)
  - ddl/mod.rs trimmed to describe and list only
  - lib.rs passes catalog_raw_ptr and persist_conn to semantic_views_register_shim
  - shim mod.rs comments updated for Phase 11
affects: [11-04-tests]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Arc::as_ptr for non-owning raw pointer to Rust data passed to C++"

key-files:
  created: []
  modified:
    - src/ddl/mod.rs
    - src/lib.rs
    - src/shim/mod.rs
    - src/catalog.rs
  deleted:
    - src/ddl/define.rs
    - src/ddl/drop.rs

key-decisions:
  - "persist_conn creation moved before shim call so it can be passed to the C++ parser hook"
  - "Arc::as_ptr used for non-owning raw catalog pointer — Arc stays alive via QueryState"

patterns-established:
  - "Pattern: Arc::as_ptr(&state) as *const c_void to pass Rust Arc data to C++ without transferring ownership"

requirements-completed: [DDL-05]

# Metrics
duration: 15min
completed: 2026-03-01
---

# Phase 11 Plan 03: Legacy DDL Cleanup and Shim Wiring Summary

**Deleted define.rs/drop.rs, trimmed ddl/mod.rs, updated lib.rs to pass catalog_raw_ptr and persist_conn to semantic_views_register_shim for Phase 11 parser hook**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-01T00:30:00Z
- **Completed:** 2026-03-01T00:45:00Z
- **Tasks:** 2
- **Files modified:** 4 (plus 2 deleted)

## Accomplishments
- Deleted src/ddl/define.rs and src/ddl/drop.rs (DDL-05 satisfied)
- Updated ddl/mod.rs to only pub mod describe and list
- Updated lib.rs: removed DefineSemanticView/DropSemanticView registrations; updated shim signature; moved persist_conn creation before shim call; pass catalog_raw and raw_persist_conn to C++
- Updated shim/mod.rs: comment reflects Phase 11 parser hook usage pattern
- Fixed catalog.rs error message to reference CREATE OR REPLACE syntax
- All 78 unit tests pass; clippy clean

## Task Commits

1. **Task 1+2: Delete define/drop, trim mod.rs, update lib.rs and shim/mod.rs** - `68d7060` (feat)

## Files Created/Modified
- `src/ddl/mod.rs` - Stripped to pub mod describe and list
- `src/lib.rs` - Updated shim signature, persist_conn wiring, removed scalar DDL registrations
- `src/shim/mod.rs` - Updated comments for Phase 11
- `src/catalog.rs` - Error message updated to reference native DDL
- DELETED: `src/ddl/define.rs`, `src/ddl/drop.rs`

## Decisions Made
- persist_conn moved before shim call: required so it can be passed to the C++ parser hook at registration time
- Arc::as_ptr used for non-owning catalog pointer: the Arc's refcount remains elevated via QueryState (which holds a clone), so the raw pointer is safe for the extension lifetime

## Deviations from Plan
None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Rust wiring complete — the C++ shim (11-02) can now receive catalog_raw_ptr and persist_conn
- After 11-02 completes, 11-04 integration tests can verify the full DDL pipeline

---
*Phase: 11-create-semantic-view-parser-hook*
*Completed: 2026-03-01*
