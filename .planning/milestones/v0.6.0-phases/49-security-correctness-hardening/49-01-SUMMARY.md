---
phase: 49-security-correctness-hardening
plan: 01
subsystem: catalog
tags: [rwlock, mutex, lock-poisoning, bounds-check, security-hardening]

# Dependency graph
requires:
  - phase: 48-window-function-metrics
    provides: "Complete VTab bind/init/func pipeline used across all DDL and query modules"
provides:
  - "Graceful error returns on poisoned RwLock/Mutex across all 15 production modules"
  - "CatalogPoisoned ExpandError variant for semantic error messages"
  - "debug_assert! bounds checks in test helper read_typed_value"
affects: [all-vtab-modules, catalog, query-pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Lock acquisition via .map_err() instead of .unwrap()/.expect()"
    - "debug_assert! for unsafe pointer arithmetic bounds in test helpers"

key-files:
  created: []
  modified:
    - src/catalog.rs
    - src/expand/types.rs
    - src/ddl/alter.rs
    - src/ddl/drop.rs
    - src/ddl/get_ddl.rs
    - src/ddl/list.rs
    - src/ddl/show_facts.rs
    - src/ddl/show_dims.rs
    - src/ddl/show_metrics.rs
    - src/ddl/show_columns.rs
    - src/ddl/show_dims_for_metric.rs
    - src/ddl/describe.rs
    - src/ddl/define.rs
    - src/query/table_function.rs
    - src/query/explain.rs
    - src/lib.rs

key-decisions:
  - "Use .map_err() with descriptive string instead of into_inner() lock recovery"
  - "catalog_delete_if_exists silently skips on poisoned lock (best-effort semantic matches existing behavior)"
  - "debug_assert! (not assert!) for test helper bounds checks -- active in test builds, compiled out in release"

patterns-established:
  - "Lock acquisition pattern: .read()/.write()/.lock().map_err(|_| Box::<dyn std::error::Error>::from(\"catalog lock poisoned\"))?"

requirements-completed: [SEC-02, SEC-04]

# Metrics
duration: 43min
completed: 2026-04-14
---

# Phase 49 Plan 01: Lock Poisoning and Bounds Check Hardening Summary

**Replaced all panic-on-lock-poison patterns with graceful error returns across 15 modules, added debug_assert bounds checks to test FFI helper**

## Performance

- **Duration:** 43 min
- **Started:** 2026-04-14T01:37:33Z
- **Completed:** 2026-04-14T02:21:05Z
- **Tasks:** 2
- **Files modified:** 16

## Accomplishments
- Eliminated all RwLock .unwrap()/.expect() calls in production code paths across catalog.rs and 12 VTab bind() methods, replacing with .map_err() that returns descriptive error strings
- Replaced Mutex .lock().unwrap() in query streaming (table_function.rs) with error-returning pattern
- Added CatalogPoisoned variant to ExpandError for semantic-level poisoned lock messaging
- Added debug_assert! bounds checks for row_idx and col_idx in read_typed_value test helper
- Added 5 unit tests verifying poisoned lock behavior for all catalog mutation functions
- All quality gate checks pass: 570+ unit tests, 30 SQL logic tests, 6 DuckLake CI tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Replace lock .unwrap()/.expect() with error returns across all modules** - `47a1973` (feat)
2. **Task 2: Add bounds checks to test helper unsafe pointer arithmetic** - `7c3bb79` (fix)

## Files Created/Modified
- `src/catalog.rs` - Lock acquisitions use .map_err() instead of .unwrap(); 5 poisoned lock tests added
- `src/expand/types.rs` - CatalogPoisoned error variant with Display message
- `src/ddl/alter.rs` - Three .read().unwrap() replaced with .map_err() (rename + comment)
- `src/ddl/drop.rs` - .read().unwrap() replaced with .map_err()
- `src/ddl/get_ddl.rs` - .read().unwrap() replaced with .map_err()
- `src/ddl/list.rs` - Two .read().expect() replaced with .map_err() (list + terse)
- `src/ddl/show_facts.rs` - Two .read().expect() replaced with .map_err() (single + all)
- `src/ddl/show_dims.rs` - Two .read().expect() replaced with .map_err() (single + all)
- `src/ddl/show_metrics.rs` - Two .read().expect() replaced with .map_err() (single + all)
- `src/ddl/show_columns.rs` - .read().expect() replaced with .map_err()
- `src/ddl/show_dims_for_metric.rs` - .read().expect() replaced with .map_err()
- `src/ddl/describe.rs` - .read().expect() replaced with .map_err()
- `src/query/table_function.rs` - RwLock .expect() and Mutex .unwrap() replaced with .map_err()
- `src/query/explain.rs` - .read().expect() replaced with .map_err()
- `src/lib.rs` - debug_assert! bounds checks added to read_typed_value

## Decisions Made
- Used .map_err() with descriptive string instead of into_inner() lock recovery -- poisoned locks indicate a previous panic, recovery is unsafe
- catalog_delete_if_exists silently skips on poisoned lock rather than returning Result -- matches its existing "best-effort" semantics (silently no-ops when view absent)
- Used debug_assert! (not assert!) for test helper bounds checks -- these are #[cfg(test)] only functions where debug_assert is active, and assert! would add overhead to test runtime unnecessarily

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Pre-commit hook (rustfmt) reformatted chained .map_err() calls on first commit attempt -- resolved by running cargo fmt before staging

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All lock poisoning patterns eliminated from production code
- Ready for Phase 49 Plan 02 (additional security/correctness hardening)

---
*Phase: 49-security-correctness-hardening*
*Completed: 2026-04-14*
