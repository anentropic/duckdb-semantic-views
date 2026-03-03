---
phase: quick-8
plan: 01
subsystem: infra
tags: [github-actions, ci, fuzzing, path-filter]

# Dependency graph
requires: []
provides:
  - "Path-filtered fuzz CI workflow (skip on non-code changes)"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: ["GitHub Actions paths filter for expensive CI jobs"]

key-files:
  created: []
  modified: [".github/workflows/Fuzz.yml"]

key-decisions:
  - "Allowlist (paths:) over denylist (paths-ignore:) for safety -- new file types default to excluded"

patterns-established:
  - "CI path filtering: gate expensive workflows on paths that can affect their outcomes"

requirements-completed: [QUICK-8]

# Metrics
duration: 47s
completed: 2026-03-03
---

# Quick Task 8: Gate Fuzz CI on Relevant File Changes Summary

**Added paths filter to Fuzz.yml so 30-min fuzz job only triggers on src/fuzz/Cargo changes, not docs or tooling**

## Performance

- **Duration:** 47s
- **Started:** 2026-03-03T13:18:31Z
- **Completed:** 2026-03-03T13:19:18Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Added `paths:` filter under push trigger with 5 entries: `src/**`, `fuzz/**`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/Fuzz.yml`
- `workflow_dispatch` remains unconditional for manual runs
- Saves ~30 min CI time on documentation, planning, and tooling-only pushes

## Task Commits

Each task was committed atomically:

1. **Task 1: Add paths filter to Fuzz.yml push trigger** - `261473c` (chore)

## Files Created/Modified
- `.github/workflows/Fuzz.yml` - Added `paths:` allowlist under push trigger

## Decisions Made
- Used allowlist (`paths:`) instead of denylist (`paths-ignore:`) so new file types are excluded by default, requiring explicit opt-in to trigger fuzz CI

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Fuzz CI will now only run when relevant files change on push to main
- Manual dispatch still available for on-demand fuzzing

## Self-Check: PASSED

- FOUND: `.github/workflows/Fuzz.yml`
- FOUND: commit `261473c`
- FOUND: `8-SUMMARY.md`

---
*Quick Task: 8-gate-fuzz-ci-on-relevant-file-changes*
*Completed: 2026-03-03*
