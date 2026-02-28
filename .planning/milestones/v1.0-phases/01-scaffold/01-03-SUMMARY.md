---
phase: 01-scaffold
plan: "03"
subsystem: infra
tags: [github-actions, duckdb, version-monitor, automation, ci]

# Dependency graph
requires: []
provides:
  - Weekly DuckDB version polling via GitHub API
  - Automated version-bump PR creation when build passes after upgrade
  - Automated breakage PR with @copilot mention when build fails after upgrade
  - Manual trigger via workflow_dispatch for testing
affects:
  - 02-storage
  - 03-expansion
  - 04-query
  - 05-hardening

# Tech tracking
tech-stack:
  added:
    - peter-evans/create-pull-request@v7 (conditional PR creation from workflows)
    - dtolnay/rust-toolchain@stable (Rust toolchain setup in monitor workflow)
    - gh CLI (GitHub API polling for latest release)
  patterns:
    - "continue-on-error: true on build step to allow failure-path PR creation"
    - "steps.build.outcome (not conclusion) for correct result after continue-on-error"
    - "GITHUB_OUTPUT for sharing values between steps"

key-files:
  created:
    - .github/workflows/DuckDBVersionMonitor.yml
  modified: []

key-decisions:
  - "Use steps.build.outcome (not steps.build.conclusion) because conclusion reads success even on failure when continue-on-error: true"
  - "Breakage PR includes @copilot mention to request automated fix attempt; version-bump PR does not"
  - "Workflow updates both Makefile TARGET_DUCKDB_VERSION and both CI workflow duckdb_version fields in a single sed pass"
  - "delete-branch: true on both PR actions prevents stale branches accumulating"

patterns-established:
  - "Pattern: All automated version-bump PRs use branch prefix chore/duckdb-bump-{version}"
  - "Pattern: All breakage PRs use branch prefix fix/duckdb-breakage-{version}"

requirements-completed:
  - INFRA-03

# Metrics
duration: 1min
completed: 2026-02-24
---

# Phase 1 Plan 03: DuckDB Version Monitor Summary

**GitHub Actions workflow that polls for new DuckDB releases weekly and opens a version-bump PR on build success or a @copilot-tagged breakage PR on build failure**

## Performance

- **Duration:** 1 min
- **Started:** 2026-02-24T00:03:05Z
- **Completed:** 2026-02-24T00:04:22Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- DuckDB version monitor workflow created with weekly cron (Monday 09:00 UTC) and manual `workflow_dispatch` trigger
- Correct `continue-on-error: true` / `steps.build.outcome` pattern ensures failure-path PR step always runs regardless of build result
- Dual PR paths: clean version-bump PR on success, @copilot-tagged breakage PR on failure with link to Actions run log
- Workflow updates three files atomically (Makefile, MainDistributionPipeline.yml, PullRequestCI.yml) before attempting build

## Task Commits

Each task was committed atomically:

1. **Task 1: Create DuckDB version monitor workflow** - `5ad1d8f` (feat)

**Plan metadata:** _(see final docs commit below)_

## Files Created/Modified
- `.github/workflows/DuckDBVersionMonitor.yml` - Weekly DuckDB version polling and conditional PR creation workflow

## Decisions Made
- `steps.build.outcome` used instead of `steps.build.conclusion` — when `continue-on-error: true` is set, `conclusion` is always `success` regardless of whether the step actually failed; `outcome` correctly reflects the real result
- `@copilot` mention placed only in breakage PR body, not in version-bump PR body — this matches the design doc intent (request automated fix only when human attention is needed)
- `delete-branch: true` on both PR creation actions — prevents stale `chore/duckdb-bump-*` and `fix/duckdb-breakage-*` branches from accumulating over time

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

YAML validation via `python3 -c "import yaml; ..."` was not possible (pyyaml not available in active virtualenv context). All structural checks were performed via Node.js pattern matching instead, verifying all required elements: cron schedule, workflow_dispatch, permissions block, continue-on-error, @copilot mention, outcome-based conditionals. All checks passed.

## User Setup Required

None - no external service configuration required. The workflow uses `secrets.GITHUB_TOKEN` which is automatically provided by GitHub Actions.

## Next Phase Readiness
- DuckDB version monitor is ready; it will start firing once plans 01 and 02 create the Makefile and CI workflows it references
- Plans 01-01 and 01-02 must be executed to create the Makefile with `TARGET_DUCKDB_VERSION` variable and the `MainDistributionPipeline.yml` / `PullRequestCI.yml` workflows that this monitor updates

---
*Phase: 01-scaffold*
*Completed: 2026-02-24*
