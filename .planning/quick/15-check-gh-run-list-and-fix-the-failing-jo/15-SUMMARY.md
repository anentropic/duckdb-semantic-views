---
phase: quick-15
plan: 01
subsystem: infra
tags: [ci, makefile, github-actions, amalgamation, duckdb]

# Dependency graph
requires:
  - phase: v0.5.0
    provides: C++ shim compilation via build.rs (cpp/include/duckdb.cpp)
provides:
  - Makefile ensure_amalgamation target for auto-download of gitignored DuckDB amalgamation
  - PullRequestCI ducklake-ci-check amalgamation download step
  - Fixed justfile update-headers version source
affects: [ci, build, release]

# Tech tracking
tech-stack:
  added: []
  patterns: [file-based make target for conditional download]

key-files:
  created: []
  modified: [Makefile, justfile, .github/workflows/PullRequestCI.yml]

key-decisions:
  - "File-based Make target (cpp/include/duckdb.cpp:) for idempotent download -- Make's own dependency resolution handles 'only if missing'"
  - "Used /tmp/libduckdb-src.zip for temp file -- valid on Linux, macOS, and Windows CI (Git Bash/MSYS2)"

patterns-established:
  - "Amalgamation auto-download: ensure_amalgamation as prerequisite on build targets"

requirements-completed: []

# Metrics
duration: 8min
completed: 2026-03-09
---

# Quick Task 15: Fix CI Amalgamation Auto-Download Summary

**Makefile ensure_amalgamation target with file-based dependency auto-downloads DuckDB amalgamation in CI; PullRequestCI ducklake-ci-check fixed; justfile version source corrected**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-09T21:20:29Z
- **Completed:** 2026-03-09T21:29:01Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Added `ensure_amalgamation` Makefile target that downloads DuckDB amalgamation from GitHub releases when `cpp/include/duckdb.cpp` is missing, no-op when present
- Both `build_extension_library_debug` and `build_extension_library_release` depend on `ensure_amalgamation` -- CI builds via `make release` auto-download
- PullRequestCI `ducklake-ci-check` job (which bypasses Make) now has explicit amalgamation download step
- Fixed justfile `update-headers` recipe to read version from `.duckdb-version` directly instead of parsing Makefile

## Task Commits

All tasks implemented in a single atomic commit (tasks 1-2 were edits, task 3 was verification):

1. **Tasks 1-3: Add amalgamation auto-download, fix PullRequestCI, fix justfile, verify locally** - `3859d68` (fix)

## Files Created/Modified
- `Makefile` - Added `ensure_amalgamation` target with file-based dependency, AMALGAMATION_URL variable, and prerequisite on both build targets
- `justfile` - Fixed `update-headers` to read version from `.duckdb-version` instead of grepping Makefile
- `.github/workflows/PullRequestCI.yml` - Added "Download DuckDB amalgamation" step before cargo build in ducklake-ci-check job

## Decisions Made
- File-based Make target (`cpp/include/duckdb.cpp:`) instead of phony target with conditional -- lets Make's own dependency resolution handle the "only if missing" check naturally
- Used `/tmp/libduckdb-src.zip` for temp download path -- valid across Linux, macOS, and Windows CI (Git Bash/MSYS2 shells)
- Updated stale justfile comment to reference `.duckdb-version` instead of Makefile (Rule 1 - auto-fix, stale documentation)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed stale justfile comment**
- **Found during:** Task 1
- **Issue:** Comment on line 113 still said "Version is read from TARGET_DUCKDB_VERSION in Makefile" after changing the code to read from `.duckdb-version`
- **Fix:** Updated comment to "Version is read from .duckdb-version"
- **Files modified:** justfile
- **Verification:** Comment matches actual behavior
- **Committed in:** 3859d68

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Trivial documentation fix, no scope creep.

## Issues Encountered
- Network sandbox blocked curl to GitHub releases during local verification -- required sandbox override to test the download target. This does not affect CI which runs without sandbox restrictions.

## User Setup Required
None - no external service configuration required.

## Next Steps
- Push to main to trigger Main Extension Distribution Pipeline on all 5 platforms
- Monitor CI run to confirm all platforms pass

---
*Quick Task: 15-fix-ci-amalgamation*
*Completed: 2026-03-09*
