---
phase: 11-create-semantic-view-parser-hook
plan: 02
subsystem: database
tags: [cpp, ffi, ddl, parser-extension]

# Dependency graph
requires:
  - phase: 11-create-semantic-view-parser-hook
    plan: 01
    provides: catalog_upsert, catalog_delete_if_exists, FFI catalog functions
  - phase: 11-create-semantic-view-parser-hook
    plan: 03
    provides: updated shim signature (3-param), persist_conn and catalog_raw wired in lib.rs
provides:
  - Full C++ parser extension: SemanticViewsParseFunction, SemanticViewsPlanFunction, ParseSemanticViewStatement tokenizer
  - SemanticViewsDDLBind and SemanticViewsDDLScan (TableFunction execution path)
  - Updated shim.h with catalog FFI declarations
  - Parser hook registered in semantic_views_register_shim (DDL-01, DDL-02, DDL-03, DDL-06)
affects: [11-04-tests]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Value::POINTER(uintptr_t) to pass raw Rust pointers through DuckDB Value system to TableFunction bind"
    - "ParserExtensionParseResult() (no args) = DISPLAY_ORIGINAL_ERROR for non-matching SQL (DDL-06)"
    - "result.modified_databases[\"main\"] = {} signals write operation to DuckDB planner"
    - "Two-phase DDL: persist_conn SQL write THEN in-memory catalog FFI update"

key-files:
  created: []
  modified:
    - src/shim/shim.h
    - src/shim/shim.cpp

key-decisions:
  - "Value::POINTER used for catalog_ptr and persist_conn through plan result parameters — GetPointer() extracts them in bind"
  - "Fast keyword check in parse_function_t (find SEMANTIC) before full parse — avoids expensive tokenizer on every non-DDL statement"
  - "Base table = table NOT appearing as REFERENCES target; circular case falls back to first declared table"
  - "IF NOT EXISTS handled in scan: catalog_insert returns -1 if view exists; scan silently returns instead of throwing"

patterns-established:
  - "Pattern: hand-written tokenizer reads upper-cased copy for keyword matching, original for expression preservation"
  - "Pattern: read_expression tracks parenthesis depth to avoid splitting COALESCE(a, b) on comma"

requirements-completed: [DDL-01, DDL-02, DDL-03, DDL-06]

# Metrics
duration: 20min
completed: 2026-03-01
---

# Phase 11 Plan 02: C++ Parser Extension Hook Summary

**Implemented full CREATE/DROP SEMANTIC VIEW parser extension in shim.cpp; updated shim.h with catalog FFI declarations; registered parser hook in semantic_views_register_shim**

## Performance

- **Duration:** 20 min
- **Started:** 2026-03-01T01:00:00Z
- **Completed:** 2026-03-01T01:20:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Updated `src/shim/shim.h`: added 4 `semantic_views_catalog_*` FFI declarations inside `extern "C"` block; updated `semantic_views_register_shim` comment to reflect Phase 11 additions
- Implemented `src/shim/shim.cpp` parser extension:
  - `SemanticViewsDDLType` enum, `SemanticViewsDDLData`, `SemanticViewsParserInfo`, `SemanticViewsDDLBindData` structs
  - Hand-written tokenizer helpers: `skip_whitespace`, `read_word`, `read_identifier`, `read_expression` (depth-aware), `read_paren_content`, `json_escape`, `split_clause_items`
  - Item parsers: `parse_table_item`, `parse_relationship_item`, `parse_field_item`
  - Main tokenizer: `ParseSemanticViewStatement` — handles CREATE [OR REPLACE] SEMANTIC VIEW [IF NOT EXISTS] with TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS clauses; also handles DROP [IF EXISTS]
  - `SemanticViewsParseFunction` (parse_function_t): fast SEMANTIC keyword check; falls through with `ParserExtensionParseResult()` for non-matching SQL (DDL-06)
  - `SemanticViewsPlanFunction` (plan_function_t): builds TableFunction plan with 8 Value parameters (including catalog_ptr and persist_conn via `Value::POINTER`)
  - `SemanticViewsDDLBind`: extracts 8 parameters into `SemanticViewsDDLBindData`
  - `SemanticViewsDDLScan`: executes CREATE/DROP via persist_conn write + Rust FFI catalog update; handles IF NOT EXISTS silent-succeed and IF EXISTS silent-drop
  - Updated `semantic_views_register_shim` to register parser hook after PRAGMA callbacks
- `cargo build --features extension` — zero errors
- All 78 unit tests pass (cargo nextest)
- All 4 catalog FFI symbols (`semantic_views_catalog_delete`, `_delete_if_exists`, `_insert`, `_upsert`) verified exported via `nm -gU`

## Task Commits

1. **Task 1+2: Implement parser extension hook in shim.h and shim.cpp** - `e19770f` (feat)

## Files Created/Modified

- `src/shim/shim.h` - Added catalog FFI declarations
- `src/shim/shim.cpp` - Full parser extension implementation (870 lines added)

## Decisions Made

- `Value::POINTER` chosen for catalog_ptr and persist_conn: `uintptr_t` cast preserves pointer identity; `GetPointer()` recovers it in bind. No ownership transfer — Arc stays alive via QueryState.
- Fast keyword check (`find("SEMANTIC")`) before full parse: avoids calling `ParseSemanticViewStatement` for every failed DuckDB parse (e.g., every `SELECT` with a syntax error)
- `ParseSemanticViewStatement` builds JSON string directly (no library): values are subsequently re-parsed by Rust `SemanticViewDefinition::from_json`, which is the authoritative validator
- `result.modified_databases["main"] = {}` required to signal DuckDB that this is a write operation

## Deviations from Plan

None — plan executed exactly as written.

## Issues Encountered

None — build succeeded on first attempt.

## User Setup Required

None.

## Next Phase Readiness

- Parser hook wired and compiling — 11-04 integration tests can now verify the full DDL pipeline end-to-end
- All Wave 2 plans (11-02 and 11-03) are complete — Wave 3 (11-04) is unblocked

---
*Phase: 11-create-semantic-view-parser-hook*
*Completed: 2026-03-01*
