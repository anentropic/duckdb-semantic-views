---
phase: 11-create-semantic-view-parser-hook
plan: 01
subsystem: database
tags: [rust, serde, ffi, catalog, model]

# Dependency graph
requires:
  - phase: 10-pragma-query-t-catalog-persistence
    provides: CatalogState type, catalog_insert, catalog_delete, init_catalog
provides:
  - Fact struct with name/expr/source_table for FACTS clause
  - Updated Join struct with from_cols field for RELATIONSHIPS clause
  - facts field on SemanticViewDefinition (serde default empty)
  - catalog_upsert: insert-or-replace without duplicate error
  - catalog_delete_if_exists: silent success when view absent
  - ffi_catalog module: four #[no_mangle] extern C catalog mutation functions
affects: [11-create-semantic-view-parser-hook]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "FFI-safe catalog mutation via #[no_mangle] pub unsafe extern C functions in #[cfg(feature=extension)] mod"
    - "Backward compat struct evolution: add field with #[serde(default)], remove deny_unknown_fields"

key-files:
  created: []
  modified:
    - src/model.rs
    - src/catalog.rs
    - src/expand.rs
    - tests/expand_proptest.rs

key-decisions:
  - "deny_unknown_fields removed from SemanticViewDefinition to allow old stored JSON with extra fields to load"
  - "FFI catalog functions gated on #[cfg(feature=extension)] to exclude from standalone test binaries"
  - "Join struct keeps legacy on field with skip_serializing_if=String::is_empty for backward compat"

patterns-established:
  - "Pattern: #[serde(default, skip_serializing_if = 'String::is_empty')] for backward-compat optional string fields"
  - "Pattern: ffi_catalog inner module with unsafe fn str_from_ptr helper for null-safe C string handling"

requirements-completed: [DDL-03, DDL-04]

# Metrics
duration: 25min
completed: 2026-03-01
---

# Phase 11 Plan 01: Data Model and Catalog FFI Foundation Summary

**Fact struct + evolved Join with from_cols, catalog_upsert/delete_if_exists, and four #[no_mangle] FFI catalog functions enabling the C++ parser hook to mutate Rust's in-memory catalog**

## Performance

- **Duration:** 25 min
- **Started:** 2026-03-01T00:00:00Z
- **Completed:** 2026-03-01T00:25:00Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Added Fact struct (name, expr, source_table with serde default) for Phase 11 FACTS clause
- Evolved Join struct: added from_cols field for FK-based RELATIONSHIPS clause, kept on field for backward compat
- Added facts: Vec<Fact> with #[serde(default)] to SemanticViewDefinition; removed deny_unknown_fields
- Added catalog_upsert (validates JSON, overwrites without duplicate error)
- Added catalog_delete_if_exists (silently removes if present, no error if absent)
- Added ffi_catalog module with four #[no_mangle] pub unsafe extern "C" functions for C++ scan hook
- Fixed expand.rs and tests/expand_proptest.rs struct initializers for new model fields
- All 78 unit + proptest tests pass

## Task Commits

1. **Task 1: model.rs + expand.rs + proptest fixes** - `7bb8b74` (feat)
2. **Task 2: catalog.rs additions** - included in `7bb8b74` (fmt pre-commit hook merged commit)

## Files Created/Modified
- `src/model.rs` - Fact struct, updated Join, facts field on SemanticViewDefinition, test additions
- `src/catalog.rs` - catalog_upsert, catalog_delete_if_exists, ffi_catalog module with 4 FFI functions
- `src/expand.rs` - struct initializer updates (facts: vec![], from_cols: vec![])
- `tests/expand_proptest.rs` - struct initializer updates for new model fields

## Decisions Made
- Removed deny_unknown_fields from SemanticViewDefinition: old stored JSON with extra fields must load without error for backward compat
- FFI functions placed in #[cfg(feature = "extension")] mod ffi_catalog — standalone test binaries cannot use loadable-extension C API
- Join keeps both on (legacy) and from_cols (new) fields; skip_serializing_if prevents empty on from being written to new JSON

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Missing facts and from_cols fields in expand.rs struct initializers**
- **Found during:** Task 1 (Running cargo nextest after model.rs changes)
- **Issue:** expand.rs and expand_proptest.rs had struct initializers for SemanticViewDefinition and Join that didn't include the new fields, causing compile errors
- **Fix:** Added facts: vec![] to all SemanticViewDefinition initializers and from_cols: vec![] to all Join initializers in both files
- **Files modified:** src/expand.rs, tests/expand_proptest.rs
- **Verification:** cargo nextest run — 78 tests pass
- **Committed in:** 7bb8b74 (combined with task commit due to fmt hook)

---

**Total deviations:** 1 auto-fixed (blocking)
**Impact on plan:** Necessary fix — struct field additions require all initializers to be updated. No scope creep.

## Issues Encountered
- Rust escape sequence handling in Python regex script for `vec![]` — the `!` was being escaped as `\!`. Fixed with sed replacement.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- model.rs and catalog.rs foundation complete for Wave 2 execution
- Plan 11-02 (C++ parser hook) can now access catalog FFI functions declared in shim.h
- Plan 11-03 (Rust cleanup) can remove legacy DDL functions and update shim call signature

---
*Phase: 11-create-semantic-view-parser-hook*
*Completed: 2026-03-01*
