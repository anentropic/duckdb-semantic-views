---
phase: 34-duckdb-1-5-upgrade-lts-branch
plan: 02
subsystem: infra
tags: [ci, github-actions, lts, dual-track, version-monitor, duckdb]

# Dependency graph
requires:
  - phase: 34-duckdb-1-5-upgrade-lts-branch
    plan: 01
    provides: "Extension compiled and tested against DuckDB 1.5.0"
provides:
  - "CI workflows updated for DuckDB 1.5.0 with duckdb/* branch triggers"
  - "duckdb/1.4.x LTS branch with correct version pins"
  - "Dual-track DuckDB Version Monitor (latest + LTS)"
  - "extension-ci-tools submodule on v1.5.0 branch"
affects: [34-03, registry, release]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Branch-based CI: duckdb/* pattern in workflow triggers routes LTS builds"
    - "Dual-track version monitor: check-latest (main) and check-lts (duckdb/1.4.x) as parallel jobs"
    - "Inline version bumping: sed commands in workflow replace missing just bump-duckdb recipe"
    - "duckdb-rs crate version formula: DuckDB 1.X.Y -> 1.1XY00.0"

key-files:
  created: []
  modified:
    - .github/workflows/Build.yml
    - .github/workflows/PullRequestCI.yml
    - .github/workflows/CodeQuality.yml
    - .github/workflows/Fuzz.yml
    - .github/workflows/DuckDBVersionMonitor.yml

key-decisions:
  - "LTS branch created from 8f0b3fa (last commit before DuckDB 1.5.0 upgrade) to preserve v1.4.4 state"
  - "Inline version bumping in Version Monitor replaces nonexistent just bump-duckdb recipe"
  - "Cargo.toml version 0.5.4+duckdb1.4 on LTS branch uses semver build metadata for disambiguation"
  - "extension-ci-tools submodule remains at main branch commit on LTS (workflow uses @v1.4.4 tag reference)"

patterns-established:
  - "LTS branch naming: duckdb/{major}.{minor}.x (e.g., duckdb/1.4.x)"
  - "Version suffix: +duckdb{major}.{minor} in Cargo.toml for LTS branches"
  - "Dual-track monitoring: one job per supported DuckDB version line"

requirements-completed: [DKDB-02, DKDB-03, DKDB-04, DKDB-05, DKDB-06]

# Metrics
duration: ~5min
completed: 2026-03-16
---

# Phase 34 Plan 02: CI Workflows, LTS Branch, and Dual-Track Version Monitor Summary

**CI updated for DuckDB 1.5.0 with extension-ci-tools@v1.5.0, duckdb/1.4.x LTS branch created with v1.4.4 pins, Version Monitor rewritten for dual-track latest+LTS monitoring**

## Performance

- **Duration:** ~5 min
- **Started:** 2026-03-16T01:19:55Z
- **Completed:** 2026-03-16T01:24:45Z
- **Tasks:** 3 (2 auto + 1 human-verify auto-approved)
- **Files modified:** 6 (5 workflows + extension-ci-tools submodule)

## Accomplishments
- Updated all four CI workflows (Build, PullRequestCI, CodeQuality, Fuzz) with `duckdb/*` branch triggers and DuckDB 1.5.0 version references
- Created duckdb/1.4.x LTS branch from pre-upgrade commit with Cargo.toml version 0.5.4+duckdb1.4, duckdb pins at =1.4.4
- Rewrote DuckDB Version Monitor with two parallel jobs: check-latest (main, releases/latest) and check-lts (duckdb/1.4.x, v1.4.* releases)
- Updated extension-ci-tools submodule to v1.5.0 branch

## Task Commits

Each task was committed atomically:

1. **Task 1: Update CI workflows for DuckDB 1.5.0 and add duckdb/* branch triggers** - `31bd965` (chore)
2. **Task 2: Create duckdb/1.4.x LTS branch and rewrite Version Monitor for dual-track** - `73a3d8d` (feat)
3. **Task 3: Verify LTS branch and CI configuration** - Auto-approved (checkpoint)

## Files Created/Modified
- `.github/workflows/Build.yml` - extension-ci-tools@v1.5.0, duckdb_version v1.5.0, duckdb/* trigger
- `.github/workflows/PullRequestCI.yml` - extension-ci-tools@v1.5.0, duckdb_version v1.5.0, duckdb/* trigger
- `.github/workflows/CodeQuality.yml` - duckdb/* trigger added
- `.github/workflows/Fuzz.yml` - duckdb/* trigger added (multi-line format)
- `.github/workflows/DuckDBVersionMonitor.yml` - Complete rewrite: dual-track check-latest + check-lts
- `extension-ci-tools` (submodule) - Updated to v1.5.0 branch (5a7e84e)

## Decisions Made

1. **LTS branch point**: Created duckdb/1.4.x from commit `8f0b3fa` (docs(34): create phase plan), which is the last commit before the DuckDB 1.5.0 version pin update in Plan 01.

2. **Inline version bumping**: The old Version Monitor referenced `just bump-duckdb` which never existed. The new monitor does version bumping inline with sed commands, computing the duckdb-rs crate version from the DuckDB version using the formula `1.X.Y -> 1.1XY00.0`.

3. **Cargo.toml version suffix**: Used `0.5.4+duckdb1.4` with semver build metadata (`+duckdb1.4`) to distinguish LTS builds from main builds while keeping the same base version.

4. **Submodule on LTS branch**: Left extension-ci-tools submodule at its original `main` branch commit on the LTS branch. The CI workflow uses `@v1.4.4` tag reference, so the submodule pin is irrelevant for builds.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

1. **Cargo.lock dirty after LTS branch commit**: The pre-commit hook ran `cargo build` which updated Cargo.lock to reflect the new version `0.5.4+duckdb1.4`. Had to amend the LTS commit to include Cargo.lock. Minor workflow issue, no impact on correctness.

2. **extension-ci-tools submodule dirty after branch switch**: Switching between branches left the submodule at the wrong commit. Required explicit submodule checkout to restore correct state for each branch.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- CI workflows ready for both main (DuckDB 1.5.0) and duckdb/1.4.x (DuckDB 1.4.4) branches
- duckdb/1.4.x branch ready to push to remote (not pushed yet -- user may want to verify first)
- Version Monitor ready for dual-track monitoring once branches are pushed
- Ready for 34-03 (registry publishing / milestone completion)

## Self-Check: PASSED

- All 5 workflow files verified present
- Both task commits verified (31bd965, 73a3d8d)
- duckdb/1.4.x branch exists
- Build.yml contains @v1.5.0
- DuckDBVersionMonitor.yml contains dual-track (check-latest + check-lts)

---
*Phase: 34-duckdb-1-5-upgrade-lts-branch*
*Completed: 2026-03-16*
