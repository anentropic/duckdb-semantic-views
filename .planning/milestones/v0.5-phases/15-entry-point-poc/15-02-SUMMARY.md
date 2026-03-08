---
phase: 15-entry-point-poc
plan: 02
subsystem: entry-point
tags: [parser-hooks, cpp-shim, ffi, amalgamation, go-no-go]

# Dependency graph
requires: ["15-01"]
provides:
  - "Parser hook stub (sv_parse_stub + sv_plan_stub) registered via C++ helper"
  - "Full hook chain verified: CREATE SEMANTIC VIEW -> parse -> plan -> execute"
  - "GO decision for v0.5.0 parser extension milestone"
  - "duckdb.cpp amalgamation compilation for symbol independence"
affects: [16-parser-hook-registration]

# Tech tracking
tech-stack:
  added: [duckdb.cpp amalgamation compilation]
  patterns: [C_STRUCT entry + C++ helper (Option A), amalgamation source compilation]

key-files:
  created:
    - _notes/entry-point-decision.md
  modified:
    - cpp/src/shim.cpp
    - src/lib.rs
    - build.rs
    - Makefile
    - .gitignore
    - .github/workflows/DuckDBVersionMonitor.yml
    - justfile

key-decisions:
  - "Option B (CPP entry point) failed — unresolved C++ symbols under Python DuckDB -fvisibility=hidden"
  - "Option A (C_STRUCT + C++ helper) chosen — Rust owns entry, calls sv_register_parser_hooks() for parser hooks"
  - "Amalgamation compilation (duckdb.cpp) eliminates all manual symbol stubs — robust and future-proof"
  - "GO decision: parser extension spike proceeds with Option A + amalgamation"

patterns-established:
  - "C++ helper pattern: Rust entry point delegates parser hook registration to C++ via extern C FFI"
  - "Amalgamation compilation: duckdb.cpp compiled alongside shim.cpp provides all DuckDB C++ symbols internally"
  - "DatabaseWrapper extraction: duckdb_database -> internal_ptr -> DatabaseWrapper -> shared_ptr<DuckDB> -> DatabaseInstance"

requirements-completed: [ENTRY-01, ENTRY-02, ENTRY-03]

# Metrics
completed: 2026-03-07
---

# Phase 15 Plan 02: CPP Entry Point + Parser Hook Stubs Summary

**GO decision: Option A (C_STRUCT + C++ helper) with duckdb.cpp amalgamation compilation proves parser hook chain works end-to-end under Python DuckDB**

## Accomplishments
- Attempted Option B (CPP entry via DUCKDB_CPP_EXTENSION_ENTRY) — failed due to unresolved C++ symbols
- Pivoted to Option A: Rust C_STRUCT entry + C++ helper for parser hook registration
- Resolved symbol stubs via duckdb.cpp amalgamation compilation (eliminates all manual stubs)
- Parser hook chain verified: CREATE SEMANTIC VIEW -> sv_parse_stub -> sv_plan_stub -> stub result
- All tests pass: 130 Rust + 3 sqllogictest + 6 DuckLake CI
- GO decision recorded in _notes/entry-point-decision.md

## Task Commits

1. **Task 1: Rewrite shim.cpp + lib.rs for CPP entry** — `c56f5c8` (feat, initial Option B attempt)
2. **Task 2: Pivot to Option A + amalgamation, verify, record decision** — `e81799d` (feat)

## Deviations from Plan

### Major Deviation: Option B -> Option A

- **Plan expected:** Option B (CPP entry point) as primary, Option A as fallback
- **What happened:** Option B failed immediately — DUCKDB_CPP_EXTENSION_ENTRY references non-inlined C++ symbols not available under Python DuckDB's -fvisibility=hidden
- **Pivot:** Switched to Option A (C_STRUCT + C++ helper). Initial header-only approach hit "whack-a-mole" with symbol stubs (Function hierarchy, RTTI). Resolved by compiling duckdb.cpp amalgamation source.
- **Impact:** Same end result (parser hooks work), different mechanism. C_STRUCT ABI is actually better — C API stubs initialized automatically, no dual-entry-point risk.

## Next Phase Readiness
- Phase 16 can use any DuckDB C++ type freely in the shim (amalgamation provides all symbols)
- Entry point stays Rust-owned (C_STRUCT ABI) — no changes needed
- sv_parse_stub and sv_plan_stub are stub implementations ready to be extended in Phase 16

## Self-Check: PASSED

- [x] Extension loads under Python DuckDB (sqllogictest passes)
- [x] CREATE SEMANTIC VIEW triggers full hook chain (verified via uv run)
- [x] All existing tests pass (130 + 3 + 6 = 139)
- [x] GO decision recorded in _notes/entry-point-decision.md
- [x] STATE.md updated with Phase 15 decisions
- [x] Committed on feat/cpp-entry-point

---
*Phase: 15-entry-point-poc*
*Completed: 2026-03-07*
