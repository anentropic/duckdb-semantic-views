---
phase: 01-scaffold
plan: "02"
subsystem: infra
tags: [github-actions, ci, duckdb, extension-ci-tools, makefile, sqllogictest, abi, rust]

# Dependency graph
requires: []
provides:
  - Pull Request CI workflow (Linux x86_64 fast check only)
  - Main Extension Distribution Pipeline (full 5-platform matrix on main/release)
  - Code Quality CI workflow (rustfmt, clippy, cargo-deny, 80% coverage)
  - SQLLogicTest LOAD smoke test catching DuckDB ABI mismatches
  - extension-ci-tools git submodule wired into Makefile
  - Verified arch names from distribution_matrix.json
affects:
  - 02-storage
  - 03-expansion
  - 04-query
  - 05-hardening

# Tech tracking
tech-stack:
  added:
    - duckdb/extension-ci-tools (git submodule)
    - duckdb/extension-ci-tools/_extension_distribution.yml (reusable workflow)
    - EmbarkStudios/cargo-deny-action@v2
    - taiki-e/install-action (cargo-llvm-cov, nextest)
    - dtolnay/rust-toolchain@stable
  patterns:
    - Reusable workflow pattern: extension delegates to extension-ci-tools for platform matrix
    - Reduced CI pattern: PRs get Linux x86_64 only; main/release get full 5-platform matrix
    - LOAD smoke test pattern: require directive in SQLLogicTest catches ABI mismatches that cargo test misses

key-files:
  created:
    - .github/workflows/PullRequestCI.yml
    - .github/workflows/MainDistributionPipeline.yml
    - .github/workflows/CodeQuality.yml
    - test/sql/semantic_views.test
    - Makefile
    - .gitmodules
  modified: []

key-decisions:
  - "PullRequestCI uses exclude_archs with all non-linux_amd64 targets excluded — verified exact names from extension-ci-tools/config/distribution_matrix.json"
  - "MainDistributionPipeline excludes linux_amd64_musl, linux_arm64_musl, windows_arm64, windows_amd64_mingw, and all WASM targets — keeping 5 target platforms"
  - "CodeQuality.yml uses cargo llvm-cov nextest --fail-under-lines 80 with a comment noting --lib fallback for cdylib coverage"
  - "make configure completes successfully, downloading duckdb-1.4.4 Python package and sqllogictest runner"

patterns-established:
  - "PR CI is fast (Linux x86_64 only); push to main/release is thorough (5-platform)"
  - "All CI workflows reference extension-ci-tools reusable workflow — do not hand-roll platform matrix"
  - "LOAD smoke test in test/sql/semantic_views.test is the canonical ABI check; cargo test is not sufficient"

requirements-completed:
  - INFRA-02
  - INFRA-04

# Metrics
duration: 4min
completed: 2026-02-24
---

# Phase 1 Plan 02: CI Workflows and LOAD Smoke Test Summary

**Three GitHub Actions workflows (PR fast check, 5-platform distribution, code quality) plus SQLLogicTest LOAD smoke test wired into extension-ci-tools Makefile infrastructure**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-24T00:02:49Z
- **Completed:** 2026-02-24T00:06:43Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- GitHub Actions CI infrastructure with correct PR vs main/release branch split
- Exact architecture names verified from `extension-ci-tools/config/distribution_matrix.json` — no guessing
- SQLLogicTest `require semantic_views` smoke test that catches DuckDB ABI mismatches cargo test cannot detect
- `make configure` verified working — downloads duckdb-1.4.4 Python package and SQLLogicTest runner
- extension-ci-tools submodule initialized and populated at commit time

## Task Commits

Each task was committed atomically:

1. **Task 1: Initialize extension-template-rs Makefile and submodule structure** - `108d0c7` (feat: already committed as part of 01-03 execution)
2. **Task 2: Create GitHub Actions workflows and LOAD smoke test** - `9578ebb` (feat)

## Files Created/Modified

- `.github/workflows/PullRequestCI.yml` - PR CI: Linux x86_64 only for fast feedback
- `.github/workflows/MainDistributionPipeline.yml` - Full 5-platform matrix on main/release pushes
- `.github/workflows/CodeQuality.yml` - rustfmt, clippy --D warnings, cargo-deny, 80% coverage with cargo-llvm-cov
- `test/sql/semantic_views.test` - SQLLogicTest smoke test with `require semantic_views` LOAD directive
- `Makefile` - Thin wrapper including extension-ci-tools makefiles (committed in prior 01-03 run)
- `.gitmodules` - extension-ci-tools submodule entry (committed in prior 01-03 run)

## Decisions Made

- **Arch names from distribution_matrix.json:** The plan's `exclude_archs` example in RESEARCH.md was incomplete. After initializing the submodule, we read `extension-ci-tools/config/distribution_matrix.json` and found two additional targets: `linux_arm64_musl` and `windows_arm64`, `windows_amd64_mingw`. These were added to the appropriate exclude lists.
- **PullRequestCI excludes:** All non-`linux_amd64` targets. The plan proposed excluding `linux_amd64_musl;linux_arm64;osx_amd64;osx_arm64;windows_amd64` but missed `linux_arm64_musl;windows_arm64;windows_amd64_mingw;wasm_*`. Corrected by reading the actual matrix.
- **Makefile already committed:** The Makefile and submodule were already committed by the 01-03 plan execution (which ran before 01-02). The identical Makefile was written, resulting in no diff.

## Deviations from Plan

None - plan executed exactly as specified, with one clarification: the `exclude_archs` values in the plan's examples were incomplete. We read the actual `distribution_matrix.json` from the initialized submodule and used the correct, complete list of architecture identifiers. This is expected — the RESEARCH.md Open Questions section explicitly noted this.

## Issues Encountered

- Plans 01-01 and 01-03 were already executed before 01-02. The Makefile and submodule (Task 1 of this plan) were already committed as part of 01-03's work. We wrote an identical Makefile (no diff), ran `make configure` to verify, and proceeded to Task 2.

## User Setup Required

None - no external service configuration required. CI workflows will activate automatically when pushed to GitHub.

## Next Phase Readiness

- CI infrastructure is in place for Phase 2 (Storage and DDL) work
- SQLLogicTest runner is installed locally via `make configure`
- All three workflow files ready to execute once code is pushed to GitHub
- Code quality gates (rustfmt, clippy, cargo-deny, coverage) will enforce quality from first PR

---
*Phase: 01-scaffold*
*Completed: 2026-02-24*
