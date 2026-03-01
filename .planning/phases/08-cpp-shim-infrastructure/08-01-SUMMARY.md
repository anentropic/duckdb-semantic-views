---
phase: 08-cpp-shim-infrastructure
plan: "01"
subsystem: infra
tags: [cpp, duckdb, ffi, cc-crate, headers, build]

requires:
  - phase: none
    provides: "n/a — first plan of Phase 8"
provides:
  - "duckdb_capi/ — full DuckDB C++ header tree vendored from cargo build cache"
  - "src/shim/shim.h — extern C boundary header declaring semantic_views_register_shim"
  - "src/shim/shim.cpp — C++ skeleton with no-op stub, includes all Phase 10/11 headers"
  - "src/shim/mod.rs — empty Rust module (exports added in Phases 10/11)"
  - "Cargo.toml — cc = \"1.2\" declared in [build-dependencies]"
  - "Justfile — update-headers recipe for refreshing vendored headers after DuckDB bumps"
affects: [08-02-build-rs, phase-10-pragma, phase-11-parser]

tech-stack:
  added: [cc = "1.2" (build-dependency)]
  patterns:
    - "Vendor full DuckDB C++ header tree from cargo build cache at duckdb_capi/"
    - "Empty Rust module stub pattern for shim module in Phases 10/11"

key-files:
  created:
    - duckdb_capi/duckdb.hpp
    - duckdb_capi/duckdb/ (subdirectory header tree)
    - src/shim/shim.h
    - src/shim/shim.cpp
    - src/shim/mod.rs
  modified:
    - Cargo.toml
    - Justfile

key-decisions:
  - "Copy full duckdb/src/include/ tree (not just duckdb.hpp) — duckdb.hpp includes subdirectory headers so the full tree is required"
  - "Source headers from target/debug/build/libduckdb-sys-*/out/duckdb/src/include/ (already unpacked by prior cargo test runs)"

patterns-established:
  - "Pattern: Full header tree vendoring — copy entire include/ directory, not just amalgam entry point"
  - "Pattern: update-headers Just recipe — single command to refresh vendored headers after version bump"

requirements-completed:
  - INFRA-01

duration: 1 min
completed: 2026-03-01
---

# Phase 8 Plan 01: Vendor duckdb.hpp and Create C++ Shim Skeleton Summary

**Full DuckDB C++ header tree vendored at duckdb_capi/, C++ shim skeleton with extern "C" no-op stub created at src/shim/, and cc build-dependency declared — ready for build.rs wiring in Plan 02.**

## Performance

- **Duration:** 1 min
- **Started:** 2026-03-01T02:10:47Z
- **Completed:** 2026-03-01T02:12:09Z
- **Tasks:** 2
- **Files modified:** 7 (duckdb_capi/ tree + 3 shim files + Cargo.toml + Justfile)

## Accomplishments

- Vendored full DuckDB 1.4.4 C++ header tree (1285 files) from existing cargo build cache into duckdb_capi/
- Created `src/shim/shim.h` with extern C boundary declaring `semantic_views_register_shim`
- Created `src/shim/shim.cpp` skeleton including all headers that Phases 10/11 will use (`duckdb.hpp`, `duckdb/main/config.hpp`, `duckdb/parser/parser_extension.hpp`, `duckdb/function/pragma_function.hpp`)
- Added `cc = "1.2"` to `[build-dependencies]` in Cargo.toml
- Added `update-headers` Just recipe for post-DuckDB-bump header refresh
- `cargo test` passes unchanged — no regressions (no build.rs yet, no C++ compilation in test path)

## Task Commits

1. **Task 1: Vendor duckdb.hpp and update Cargo.toml + Justfile** — `27fa7dd` (feat)
2. **Task 2: Create C++ shim skeleton and Rust module stub** — `c6381f3` (feat)

## Files Created/Modified

- `duckdb_capi/duckdb.hpp` — DuckDB C++ single-header entry point (v1.4.4)
- `duckdb_capi/duckdb/` — Full C++ header subdirectory tree (config.hpp, parser_extension.hpp, pragma_function.hpp, etc.)
- `duckdb_capi/duckdb.h` — DuckDB C API header (already existed at the include path)
- `duckdb_capi/duckdb_extension.h` — DuckDB extension C API header
- `src/shim/shim.h` — extern "C" boundary header
- `src/shim/shim.cpp` — C++ skeleton with no-op stub
- `src/shim/mod.rs` — Empty Rust module file
- `Cargo.toml` — Added `[build-dependencies]` section with `cc = "1.2"`
- `Justfile` — Added `update-headers` recipe

## Decisions Made

- **Full header tree vs duckdb.hpp only:** The `duckdb.hpp` from the build cache is NOT self-contained — it `#include`s from `duckdb/` subdirectories. The full `duckdb/src/include/` tree was copied. This matches Pitfall 5 from the research document.
- **Source from build cache:** Headers were copied from `target/debug/build/libduckdb-sys-0dce711b60676540/out/duckdb/src/include/` (already present from v0.1.0 `cargo test` runs) rather than downloading from GitHub, which is faster and guaranteed to match the pinned version.

## Deviations from Plan

None — plan executed exactly as written. The research document had already anticipated the full-tree vendoring requirement (Pitfall 5 guidance).

## Issues Encountered

None.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- All C++ source artifacts in place: `duckdb_capi/duckdb.hpp`, `src/shim/shim.cpp`, `src/shim/shim.h`, `src/shim/mod.rs`
- `cc = "1.2"` declared in `[build-dependencies]` — ready for `cc::Build::new()` in build.rs
- `cargo test` still passes (no build.rs yet, no C++ compilation triggered)
- Plan 02 can now create build.rs and wire lib.rs

---
*Phase: 08-cpp-shim-infrastructure*
*Completed: 2026-03-01*

## Self-Check: PASSED

- [x] `duckdb_capi/duckdb.hpp` exists on disk
- [x] `src/shim/shim.cpp`, `src/shim/shim.h`, `src/shim/mod.rs` exist on disk
- [x] `git log --oneline --all --grep="08-01"` returns 2 commits (27fa7dd, c6381f3)
- [x] No `## Self-Check: FAILED` marker
