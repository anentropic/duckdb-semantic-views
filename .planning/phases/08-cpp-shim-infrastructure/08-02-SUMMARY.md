---
phase: 08-cpp-shim-infrastructure
plan: "02"
subsystem: infra
tags: [cpp, build-rs, cc-crate, ffi, symbol-visibility, linker, duckdb-extension]

requires:
  - phase: 08-01
    provides: "duckdb_capi/duckdb.hpp + src/shim/ skeleton + cc build-dependency declared"
provides:
  - "build.rs: feature-gated C++ shim compilation via cc::Build, symbol visibility linker flags"
  - "src/lib.rs: extern C declaration + call to semantic_views_register_shim in init_extension"
  - "Working end-to-end: cargo test, cargo build --features extension, make debug, just test all pass"
  - "Symbol visibility: nm shows exactly 1 exported symbol (_semantic_views_init_c_api)"
affects: [phase-10-pragma, phase-11-parser, phase-12-explain]

tech-stack:
  added: []
  patterns:
    - "Pattern: CARGO_FEATURE_EXTENSION guard in build.rs — cargo test skips C++ compilation entirely"
    - "Pattern: cc::Build for Rust+C++ mixed builds — no CMake, no shell scripts"
    - "Pattern: macOS exported symbols list in OUT_DIR — restricts cdylib exports to entry point only"
    - "Pattern: ELF version script for Linux — same restriction via version-script linker flag"
    - "Pattern: unsafe extern C declaration inside #[cfg(feature=extension)] mod — no top-level leakage"

key-files:
  created:
    - build.rs
  modified:
    - src/lib.rs

key-decisions:
  - "Exclude semantic_views_version from exported symbols list — it is appended by CI post-build script (extension-ci-tools), not compiled into binary; listing it causes linker failure"
  - "Use db_handle.cast() instead of as *mut c_void — avoids pedantic clippy ptr_as_ptr lint"
  - "unsafe extern C block inside mod extension (not top-level) — inherits the extension feature gate"

patterns-established:
  - "Pattern: Only export semantic_views_init_c_api from the compiled binary; CI post-build adds semantic_views_version separately"
  - "Pattern: All C++ shim wiring lives inside #[cfg(feature=extension)] — zero impact on cargo test"

requirements-completed:
  - INFRA-01

duration: 2 min
completed: 2026-03-01
---

# Phase 8 Plan 02: Wire Build System — build.rs + lib.rs Integration Summary

**build.rs compiles C++ shim via cc::Build with feature gate and symbol visibility; lib.rs calls semantic_views_register_shim — all 5 verification checks pass (cargo test, extension build, symbol count, make debug, just test).**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-01T02:13:38Z
- **Completed:** 2026-03-01T02:16:06Z
- **Tasks:** 2
- **Files modified:** 2 (build.rs created, src/lib.rs modified)

## Accomplishments

- Created `build.rs` with CARGO_FEATURE_EXTENSION guard, `cc::Build` compiling `src/shim/shim.cpp`, and symbol visibility linker flags for macOS (exported symbols list) and Linux (ELF version script)
- Added `unsafe extern "C" { fn semantic_views_register_shim }` declaration inside `mod extension` in `src/lib.rs`
- Called `semantic_views_register_shim(db_handle.cast())` as last step in `init_extension` before `Ok(())`
- All 5 verification checks pass:
  1. `cargo test` — 7 tests pass, no regressions, no C++ compilation in test path
  2. `cargo build --no-default-features --features extension` — C++ shim compiles and links cleanly
  3. `nm -gU target/debug/libsemantic_views.dylib | grep ' T '` — exactly 1 line: `_semantic_views_init_c_api`
  4. `make debug` — full Makefile build succeeds, CI metadata script appends extension header
  5. `just test` — all 4 SQLLogicTest files pass (phase2_restart skipped on non-Windows as expected)

## Task Commits

1. **Task 1: Create build.rs** — `f1d4d09` (feat)
2. **Task 2: Wire lib.rs + fix symbol visibility** — `f772213` (feat)

## Files Created/Modified

- `build.rs` — Feature-gated cc::Build, symbol visibility on macOS (exported symbols list) and Linux (version script)
- `src/lib.rs` — extern "C" declaration + call to semantic_views_register_shim in init_extension

## Decisions Made

- **Exclude `semantic_views_version` from exported symbols:** The plan included it in both the ELF version script and the macOS exported symbols list. This caused a linker failure (`ld: symbol(s) not found: _semantic_views_version`). The `semantic_views_version` symbol is appended by `extension-ci-tools/scripts/append_extension_metadata.py` as binary metadata AFTER compilation — it does not exist in the compiled binary. Only `semantic_views_init_c_api` (which IS compiled) belongs in the symbol list.
- **`db_handle.cast()`** over `as *mut c_void` — avoids pedantic clippy `ptr_as_ptr` lint, cleaner type conversion.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed semantic_views_version from exported symbols list**
- **Found during:** Task 2, Check 2 (`cargo build --no-default-features --features extension`)
- **Issue:** Plan specified `_semantic_views_init_c_api` AND `_semantic_views_version` in exported symbols list. `_semantic_views_version` does not exist in the compiled binary — it is appended by the CI post-build Python script. Linker error: `ld: symbol(s) not found for architecture arm64: _semantic_views_version`
- **Fix:** Removed `_semantic_views_version` from macOS `.exp` file and Linux `.map` file in build.rs. Added explanatory comments documenting why CI adds it separately.
- **Files modified:** `build.rs`
- **Verification:** `cargo build --no-default-features --features extension` succeeds, `nm -gU` shows exactly 1 exported symbol
- **Committed in:** `f772213` (combined with lib.rs changes in Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary correctness fix. The plan's research document mentioned `semantic_views_version` as a DuckDB-expected symbol without distinguishing between compile-time and CI-appended symbols. The fix correctly restricts compile-time exports to only symbols that exist in the binary.

## Issues Encountered

None beyond the auto-fixed symbol visibility bug above.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

Phase 8 is complete:
- `cargo test` passes (pure Rust, no C++ in test path)
- `cargo build --no-default-features --features extension` succeeds (C++ shim compiled and linked)
- `make debug` succeeds (CI-mirror build produces `.duckdb_extension` binary)
- `just test` passes (all v0.1.0 SQLLogicTests unaffected)
- Symbol count confirmed: exactly 1 exported symbol (`_semantic_views_init_c_api`)
- The C++ boundary is established; Phases 10 and 11 can add logic to `semantic_views_register_shim`

---
*Phase: 08-cpp-shim-infrastructure*
*Completed: 2026-03-01*

## Self-Check: PASSED

- [x] `build.rs` exists and contains `CARGO_FEATURE_EXTENSION` guard, `shim.cpp` reference, and `exported_symbols_list`/`version-script` flags
- [x] `grep "semantic_views_register_shim" src/lib.rs` matches both the extern C declaration and the call
- [x] `git log --oneline --all --grep="08-02"` returns 2 commits (f1d4d09, f772213)
- [x] No `## Self-Check: FAILED` marker
