---
phase: 15-entry-point-poc
plan: 01
subsystem: infra
tags: [cc-crate, cpp, duckdb-amalgamation, build-pipeline, ci]

# Dependency graph
requires: []
provides:
  - "cpp/ directory structure with vendored duckdb.hpp and compiling shim"
  - "cc crate as optional build-dependency gated on extension feature"
  - "CI header re-fetch step in DuckDB Version Monitor workflow"
affects: [15-02-entry-point-wiring]

# Tech tracking
tech-stack:
  added: [cc crate (build-dependency)]
  patterns: [feature-gated C++ compilation via cc crate, vendored amalgamation header]

key-files:
  created:
    - cpp/src/shim.cpp
  modified:
    - Cargo.toml
    - Cargo.lock
    - build.rs
    - .gitignore
    - .github/workflows/DuckDBVersionMonitor.yml

key-decisions:
  - "Symbol visibility stays on semantic_views_init_c_api until Plan 02 wires CPP entry (macOS requires symbols to exist at link time)"
  - "duckdb.hpp vendored from GitHub release zip, gitignored, re-fetched by CI and local just command"

patterns-established:
  - "Feature-gated C++ compilation: #[cfg(feature = extension)] guards cc::Build in build.rs"
  - "Amalgamation header sourcing: download libduckdb-src.zip, extract duckdb.hpp to cpp/include/"

requirements-completed: [BUILD-01, BUILD-02]

# Metrics
duration: 9min
completed: 2026-03-07
---

# Phase 15 Plan 01: C++ Build Infrastructure Summary

**C++ shim compiles against vendored DuckDB v1.4.4 amalgamation header via cc crate, feature-gated to preserve pure-Rust cargo test workflow**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-07T14:47:27Z
- **Completed:** 2026-03-07T14:57:21Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments
- Vendored DuckDB v1.4.4 amalgamation header (1.8MB) with gitignore and CI re-fetch
- Established cc crate C++ compilation pipeline, feature-gated behind `extension`
- CI workflow updated to auto-fetch duckdb.hpp when DuckDB version is bumped
- Full test suite passes: cargo test (83 unit), sqllogictest (3), DuckLake CI (6)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create feature branch, vendor duckdb.hpp, and update Cargo.toml** - `8e36961` (chore)
2. **Task 2: Update build.rs for cc crate compilation and CPP symbol visibility** - `602d386` (feat)
3. **Task 3: Add header re-fetch step to DuckDB Version Monitor CI** - `38f0321` (chore)

## Files Created/Modified
- `cpp/include/duckdb.hpp` - Vendored DuckDB v1.4.4 amalgamation header (gitignored, 1.8MB)
- `cpp/src/shim.cpp` - Minimal compilation test validating ParserExtension type availability
- `Cargo.toml` - Added cc as optional build-dependency, gated on extension feature
- `Cargo.lock` - Updated lockfile with cc crate dependency
- `build.rs` - Added feature-gated cc crate C++ compilation of shim.cpp
- `.gitignore` - Added cpp/include/duckdb.hpp entry with re-fetch instructions
- `.github/workflows/DuckDBVersionMonitor.yml` - Added duckdb.hpp re-fetch step on version bump

## Decisions Made
- **Symbol visibility transitional state:** macOS `-exported_symbols_list` requires all listed symbols to exist at link time. Since `semantic_views_duckdb_cpp_init` does not exist yet (added in Plan 02), the visibility list retains the existing `semantic_views_init_c_api` symbol. Plan 02 will switch to the CPP symbol after wiring the entry point.
- **Amalgamation header source:** Downloaded from DuckDB GitHub release (libduckdb-src.zip) rather than extracting from cargo build cache, since the cargo cache only contains the source tree header (384 bytes), not the amalgamation (1.8MB).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Symbol visibility cannot list undefined symbols on macOS**
- **Found during:** Task 2 (build.rs update)
- **Issue:** Plan specified updating symbol visibility to export `semantic_views_duckdb_cpp_init` instead of `semantic_views_init_c_api`. However, the CPP symbol does not exist until Plan 02 adds the C++ entry point. macOS linker fails with "Undefined symbols" when the exported symbols list references non-existent symbols.
- **Fix:** Kept the existing `semantic_views_init_c_api` in the symbol visibility lists with clear comments marking the Plan 02 transition point. The `semantic_views_duckdb_cpp_init` name is referenced 4 times in build.rs comments for Plan 02 to find.
- **Files modified:** build.rs
- **Verification:** `cargo build --no-default-features --features extension` succeeds; `grep -c semantic_views_duckdb_cpp_init build.rs` returns 4
- **Committed in:** `602d386` (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary adjustment for macOS linker requirements. Symbol visibility will be finalized in Plan 02 when the CPP entry point symbol is defined. No scope creep.

## Issues Encountered
- **Amalgamation header not in cargo cache:** The `libduckdb-sys` cargo build cache contains the DuckDB source tree header (384 bytes with `#include` directives), not the amalgamation header (1.8MB self-contained). Downloaded from GitHub release instead.
- **Empty archive warning:** The shim.cpp only contains a `static_assert`, which generates no object code. The cc crate produces an empty static library, triggering a ranlib warning. This is harmless for Plan 01 and will resolve in Plan 02 when the shim has real code.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- C++ build pipeline is operational: `cargo build --features extension` compiles shim.cpp via cc crate
- Ready for Plan 02 to wire the actual C++ entry point (`DUCKDB_CPP_EXTENSION_ENTRY`)
- Plan 02 must: add real entry point code to shim.cpp, switch symbol visibility to `semantic_views_duckdb_cpp_init`, change Makefile ABI type to CPP

## Self-Check: PASSED

- [x] cpp/include/duckdb.hpp exists (1.8MB amalgamation)
- [x] cpp/src/shim.cpp exists (minimal compilation test)
- [x] 15-01-SUMMARY.md created
- [x] Commit 8e36961 found (Task 1)
- [x] Commit 602d386 found (Task 2)
- [x] Commit 38f0321 found (Task 3)
- [x] cargo test passes (83 tests)
- [x] cargo build --features extension succeeds
- [x] just test-all passes (cargo + sqllogictest + DuckLake CI)

---
*Phase: 15-entry-point-poc*
*Completed: 2026-03-07*
