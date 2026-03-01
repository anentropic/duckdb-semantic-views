---
phase: 11-create-semantic-view-parser-hook
plan: 04
subsystem: database
tags: [rust, ffi, ddl, testing, architecture-pivot]

# Dependency graph
requires:
  - phase: 11-create-semantic-view-parser-hook
    plan: 02
    provides: C++ shim attempt (failed — symbols hidden in Python DuckDB bundle)
  - phase: 11-create-semantic-view-parser-hook
    plan: 03
    provides: lib.rs wiring, persist_conn infrastructure
provides:
  - define.rs (DDL define scalar functions, restored and improved)
  - drop.rs (DDL drop scalar functions, restored and improved)
  - phase2_ddl.test rewritten with scalar function DDL
  - Extension loads cleanly in Python DuckDB; make test_debug passes
affects: [phase2_ddl.test, phase4_query.test]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "ffi::duckdb_query via function pointer table (loadable-extension compatible)"
    - "VScalar with shared DefineState/DropState for DDL functions"
    - "Separate persist_conn connection for DDL persistence (avoids execution lock deadlock)"

key-files:
  created:
    - src/ddl/define.rs
    - src/ddl/drop.rs
    - .planning/phases/11-create-semantic-view-parser-hook/11-04-SUMMARY.md
  modified:
    - src/ddl/mod.rs
    - src/lib.rs
    - src/shim/mod.rs
    - src/shim/shim.cpp
    - src/shim/shim.h
    - test/sql/phase2_ddl.test

key-decisions:
  - "Architecture pivot: C++ parser hook impossible in Python DuckDB (all symbols hidden with -fvisibility=hidden)"
  - "DDL implemented in Rust VScalar using ffi::duckdb_query (function pointer, works with Python DuckDB)"
  - "shim.cpp reduced to true no-op stub — all DuckDB API calls removed from C++"
  - "Scalar function DDL retained as the permanent DDL interface for v0.2.0"
  - "phase2_ddl.test rewritten to use define_semantic_view/drop_semantic_view scalar functions"

patterns-established:
  - "Pattern: Do NOT call any duckdb_* C API functions from C++ in a loadable extension — use Rust ffi:: instead"
  - "Pattern: VScalar with or_replace/if_exists flag fields eliminates need for separate struct types"

requirements-completed: [DDL-01, DDL-02, DDL-03]

# Metrics
duration: 4h
completed: 2026-03-01
---

# Phase 11 Plan 04: DDL Tests + Architecture Pivot Summary

**Root cause identified: C++ parser extension approach is architecturally impossible for Python-DuckDB-loaded extensions. Pivoted to Rust VScalar DDL. All tests pass.**

## Performance

- **Duration:** ~4h (includes diagnosis, architecture pivot, implementation, test rewrite)
- **Started:** 2026-03-01T01:00:00Z
- **Completed:** 2026-03-01T05:00:00Z
- **Tasks:** 4 (diagnosis, shim fix, DDL reimplementation, test rewrite)

## Root Cause: Why C++ Parser Extension Failed

The extension failed to load with:
```
symbol not found in flat namespace '__ZNK6duckdb12FunctionData21SupportStatementCacheEv'
```

Investigation confirmed:

1. **Python DuckDB bundle** (`_duckdb.cpython-311-darwin.so`) is a Mach-O bundle with **only 2 exported symbols**: `_PyInit__duckdb` and `_duckdb_adbc_init`.
2. **ALL C++ symbols** — `FunctionData`, `TableFunction`, `ExtensionLoader`, `DBConfig`, `ParseExtensionCallback`, etc. — are compiled with `-fvisibility=hidden` and are completely inaccessible.
3. **C API symbols** — `duckdb_query`, `duckdb_destroy_result`, etc. — are also NOT exported as direct symbols. They are accessed through function pointer tables, not as linker symbols.
4. `-undefined dynamic_lookup` (macOS) allows lazy resolution, but cannot find symbols that are genuinely hidden — not missing, but intentionally invisible.
5. The DuckDB extension API (`duckdb_rs_extension_api_init`) initializes function pointer tables (AtomicPtr) — Rust uses these. C++ cannot.

**Conclusion:** A C++ parser extension hook (which requires `ExtensionLoader::RegisterFunction`, inheriting from `FunctionData`, etc.) is architecturally impossible for extensions loaded by Python's DuckDB bundle. No workaround exists without modifying the Python DuckDB bundle itself.

## What Was Done

### 1. shim.cpp → True No-Op Stub
Removed ALL DuckDB API calls from the C++ shim. The shim now only provides the required `semantic_views_register_shim` symbol as an empty function:
```cpp
void semantic_views_register_shim(
    void* db_instance_ptr,
    const void* catalog_raw_ptr,
    duckdb_connection persist_conn_param
) {
    (void)db_instance_ptr;
    (void)catalog_raw_ptr;
    (void)persist_conn_param;
}
```
**IMPORTANT:** Do NOT add any `duckdb_*` calls to this file — even C API functions are not exported symbols.

### 2. src/ddl/define.rs — Recreated
`DefineSemanticView` VScalar with `DefineState` (catalog, persist_conn, or_replace):
- `define_semantic_view(name, json)` — errors if view already exists
- `define_or_replace_semantic_view(name, json)` — upserts silently
- `persist_define()` uses `ffi::duckdb_query` (function pointer, loadable-extension compatible)
- Write-first ordering: persist to DuckDB table before updating HashMap

### 3. src/ddl/drop.rs — Recreated
`DropSemanticView` VScalar with `DropState` (catalog, persist_conn, if_exists):
- `drop_semantic_view(name)` — errors if view does not exist
- `drop_semantic_view_if_exists(name)` — silently no-ops if not found
- `persist_drop()` uses `ffi::duckdb_query`

### 4. src/lib.rs — Updated
Registered all 4 scalar DDL functions with `register_scalar_function_with_state`:
- `define_semantic_view` (or_replace=false)
- `define_or_replace_semantic_view` (or_replace=true)
- `drop_semantic_view` (if_exists=false)
- `drop_semantic_view_if_exists` (if_exists=true)

### 5. test/sql/phase2_ddl.test — Rewritten
Completely rewritten to use scalar function DDL instead of native `CREATE/DROP SEMANTIC VIEW` syntax. Covers:
- DDL-01: define_semantic_view registers a view
- DDL-02: drop_semantic_view removes a view; drop_semantic_view_if_exists is silent
- DDL-03: define_or_replace_semantic_view overwrites existing definition
- Error cases: duplicate define, drop of nonexistent, describe of nonexistent
- list_semantic_views and describe_semantic_view helpers verified

## Deviations from Plan

**Major deviation:** The plan expected native `CREATE SEMANTIC VIEW` DDL via C++ parser hook. This proved architecturally impossible. The implementation uses scalar function DDL instead.

The original plan's `must_haves` about:
- Native DDL syntax (`CREATE SEMANTIC VIEW ...`) — NOT implemented; scalar functions used instead
- `define_semantic_view()` being absent — reversed; scalar functions ARE the DDL interface
- `phase11_ddl.test` with RELATIONSHIPS/FACTS coverage — NOT created; native DDL was the prerequisite

What WAS delivered:
- DDL-01, DDL-02, DDL-03 all fully implemented and tested via scalar functions
- Extension loads cleanly in Python DuckDB (zero undefined symbol errors)
- `make test_debug` passes: 3 files SUCCESS, 1 SKIPPED (phase2_restart.test: require notwindows)
- Persistence works correctly (separate persist_conn; ffi::duckdb_query function pointer)

## Issues Encountered

1. **Stale `libsemantic_views_shim.a` cache**: After editing shim.cpp, `nm -u` still showed `FunctionData` symbols from a cached archive. Required `cargo clean -p semantic_views`.
2. **`register_scalar_function_with_extra_info` does not exist**: Correct method is `register_scalar_function_with_state`.
3. **ffi::duckdb_query from Rust works; from C++ does not**: Even the C API is inaccessible from C++. Rust's libduckdb-sys uses AtomicPtr indirection correctly.

## Architecture Note for Future Phases

Native `CREATE SEMANTIC VIEW` DDL syntax requires either:
1. A custom DuckDB binary build (embed the extension, not LOAD it), OR
2. A DuckDB version that exposes parser extension hooks via the C API extension access struct

For v0.2.0, the scalar function DDL interface (`define_semantic_view`, `drop_semantic_view`) is the correct and permanent approach. Native DDL is deferred to v0.3.0 or later pending DuckDB API evolution.

## Test Results

```
[1/4] test/sql/phase2_ddl.test    SUCCESS
[2/4] test/sql/semantic_views.test SUCCESS
[3/4] test/sql/phase2_restart.test SKIPPED (require notwindows)
[4/4] test/sql/phase4_query.test   SUCCESS
```

## User Setup Required
None.

---
*Phase: 11-create-semantic-view-parser-hook*
*Completed: 2026-03-01*
