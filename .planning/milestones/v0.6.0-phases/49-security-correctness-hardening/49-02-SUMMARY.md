---
phase: 49-security-correctness-hardening
plan: 02
subsystem: ffi-safety
tags: [catch-unwind, panic-safety, cycle-detection, depth-limit, ffi-boundary]

# Dependency graph
requires:
  - phase: 49-security-correctness-hardening
    plan: 01
    provides: "Graceful lock poisoning error returns across all VTab modules"
provides:
  - "catch_unwind wrapping on all 18 VTab bind(), SemanticViewVTab func(), GetDdlScalar invoke()"
  - "catch_unwind wrapping on extension init and 4 FFI catalog functions"
  - "CycleDetected and MaxDepthExceeded ExpandError variants for query-time cycle/depth detection"
  - "toposort_derived returns Err on cycles instead of silent truncation"
  - "MAX_DERIVATION_DEPTH=64 enforced in inline_derived_metrics"
  - "catch_unwind_to_result helper in src/util.rs"
affects: [all-vtab-modules, catalog, query-pipeline, expand-facts]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "catch_unwind_to_result helper at FFI boundaries converting panics to Box<dyn Error>"
    - "std::panic::AssertUnwindSafe wrapping VTab/VScalar closures"
    - "Result-returning toposort_derived and inline_derived_metrics instead of silent truncation"

key-files:
  created: []
  modified:
    - src/util.rs
    - src/expand/types.rs
    - src/expand/facts.rs
    - src/expand/sql_gen.rs
    - src/catalog.rs
    - src/lib.rs
    - src/ddl/alter.rs
    - src/ddl/define.rs
    - src/ddl/describe.rs
    - src/ddl/drop.rs
    - src/ddl/get_ddl.rs
    - src/ddl/list.rs
    - src/ddl/show_columns.rs
    - src/ddl/show_dims.rs
    - src/ddl/show_dims_for_metric.rs
    - src/ddl/show_facts.rs
    - src/ddl/show_metrics.rs
    - src/query/table_function.rs
    - src/query/explain.rs

key-decisions:
  - "AssertUnwindSafe is justified because we catch panics at the FFI boundary and never observe partially-mutated state"
  - "catch_unwind_to_result helper centralizes panic-to-error conversion for consistent error messages"
  - "MAX_DERIVATION_DEPTH set to 64 -- prevents stack overflow from linear chains that pass cycle detection"
  - "toposort_derived returns Err on cycle instead of returning partial results silently"
  - "Box<dyn Error> explicit type annotations needed in closures for QueryError coercion under extension feature"

patterns-established:
  - "FFI boundary pattern: crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| { ... }))"
  - "Catalog FFI pattern: std::panic::catch_unwind(AssertUnwindSafe(|| { ... })).unwrap_or(-1)"
  - "Extension init pattern: catch_unwind around init_internal with explicit panic message via CString"

requirements-completed: [SEC-01, SEC-03]

# Metrics
duration: 79min
completed: 2026-04-14
---

# Phase 49 Plan 02: FFI Panic Safety and Cycle Detection Summary

**catch_unwind wrapping on all 25 FFI entry points plus cycle detection and depth limits in derived metric/fact resolution**

## Performance

- **Duration:** 79 min
- **Started:** 2026-04-14T02:24:00Z
- **Completed:** 2026-04-14T03:43:40Z
- **Tasks:** 2
- **Files modified:** 19

## Accomplishments
- Wrapped all 18 VTab bind() methods, SemanticViewVTab func(), GetDdlScalar invoke(), extension init, and 4 FFI catalog functions in catch_unwind -- 25 FFI entry points total
- Added catch_unwind_to_result helper to src/util.rs for consistent panic-to-error conversion at FFI boundaries
- Changed toposort_derived from silently returning partial results on cycle to returning Err with descriptive cycle description
- Changed inline_derived_metrics to return Result and enforce MAX_DERIVATION_DEPTH=64
- Replaced 2 toposort_facts .unwrap_or_default() calls with proper ExpandError::CycleDetected propagation
- Added CycleDetected and MaxDepthExceeded variants to ExpandError enum
- Added 6 unit tests for cycle detection and depth limit enforcement
- Full quality gate passes: 665 unit tests, 30 SQL logic tests, 6 DuckLake CI tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Add cycle detection and depth limits to derived metric/fact resolution** - `834612d` (feat)
2. **Task 2: Wrap all FFI entry points in catch_unwind** - `e998f7a` (feat)

## Files Created/Modified
- `src/util.rs` - Added catch_unwind_to_result helper for FFI panic safety
- `src/expand/types.rs` - Added CycleDetected and MaxDepthExceeded ExpandError variants with Display
- `src/expand/facts.rs` - toposort_derived returns Result, inline_derived_metrics returns Result, MAX_DERIVATION_DEPTH constant, 6 unit tests
- `src/expand/sql_gen.rs` - Replaced .unwrap_or_default() with ExpandError::CycleDetected propagation, updated test call sites for Result
- `src/catalog.rs` - 4 FFI catalog functions wrapped in catch_unwind
- `src/lib.rs` - Extension init wrapped in catch_unwind with panic-specific error message
- `src/ddl/alter.rs` - 3 VTab bind() methods wrapped in catch_unwind_to_result
- `src/ddl/define.rs` - DefineFromJsonVTab bind() wrapped in catch_unwind_to_result
- `src/ddl/describe.rs` - DescribeSemanticViewVTab bind() wrapped
- `src/ddl/drop.rs` - DropSemanticViewVTab bind() wrapped
- `src/ddl/get_ddl.rs` - GetDdlScalar invoke() wrapped in catch_unwind_to_result
- `src/ddl/list.rs` - 2 VTab bind() methods wrapped
- `src/ddl/show_columns.rs` - ShowColumnsInSemanticViewVTab bind() wrapped
- `src/ddl/show_dims.rs` - 2 VTab bind() methods wrapped
- `src/ddl/show_dims_for_metric.rs` - ShowDimensionsForMetricVTab bind() wrapped
- `src/ddl/show_facts.rs` - 2 VTab bind() methods wrapped
- `src/ddl/show_metrics.rs` - 2 VTab bind() methods wrapped
- `src/query/table_function.rs` - SemanticViewVTab bind() and func() wrapped
- `src/query/explain.rs` - ExplainSemanticViewVTab bind() wrapped

## Decisions Made
- AssertUnwindSafe is justified because we catch panics at the FFI boundary and never observe partially-mutated state -- the panic is converted to an error string returned to DuckDB
- catch_unwind_to_result helper centralizes the panic payload inspection (checks for &str and String payloads) for consistent error messages across all entry points
- MAX_DERIVATION_DEPTH set to 64 -- prevents stack overflow from long linear chains (a->b->c->...->m64) that pass cycle detection
- Box<dyn Error> explicit type annotations needed in catch_unwind closures because Rust cannot coerce Box<QueryError> to Box<dyn Error> across closure boundaries when the return type is inferred

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed Box<QueryError> to Box<dyn Error> coercion in extension build**
- **Found during:** Task 2 (catch_unwind wrapping)
- **Issue:** `Box::new(QueryError::...)` returns `Box<QueryError>`, but `catch_unwind_to_result` closure requires `Result<T, Box<dyn Error>>`. The coercion works without catch_unwind but fails inside the closure because the compiler cannot infer the return type.
- **Fix:** Added explicit `let err: Box<dyn std::error::Error> = Box::new(QueryError::...)` type annotations at 4 return sites in table_function.rs and explain.rs. Also changed `map_err` in func() to explicitly produce `Box<dyn std::error::Error>`.
- **Files modified:** src/query/table_function.rs, src/query/explain.rs
- **Verification:** `just build` succeeds, `just test-all` passes
- **Committed in:** e998f7a (part of Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Type coercion fix is a Rust-specific technicality. No scope creep.

## Issues Encountered
- Extension build (`just build`) compiles with `--features extension` which enables VTab/VScalar code not compiled by `cargo test`. The QueryError type coercion issue was only visible under the extension feature. Resolved by explicit type annotations.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All FFI entry points now have panic safety guards
- All lock poisoning patterns eliminated (plan 01) + all panics caught (plan 02) = complete security hardening
- Phase 49 (security-correctness-hardening) fully complete

---
*Phase: 49-security-correctness-hardening*
*Completed: 2026-04-14*
