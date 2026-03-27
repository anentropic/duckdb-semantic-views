---
phase: 35-documentation-site
plan: 01
subsystem: infra
tags: [sphinx, shibuya, github-pages, github-actions, ci-cd, documentation]

# Dependency graph
requires:
  - phase: 33-unique-constraints-cardinality-inference
    provides: stable DDL syntax for documentation content
provides:
  - Sphinx docs deployed to GitHub Pages via Docs.yml workflow
  - PR docs build validation via docs-check job in PullRequestCI.yml
  - sphinx.ext.githubpages extension for .nojekyll generation
  - README documentation badge and link
affects: [36-registry-publishing]

# Tech tracking
tech-stack:
  added: [sphinx.ext.githubpages, actions/deploy-pages@v4, actions/upload-pages-artifact@v3, actions/configure-pages@v5]
  patterns: [uv-based docs build in CI, two-job build+deploy pattern for GitHub Pages]

key-files:
  created: [.github/workflows/Docs.yml]
  modified: [docs/conf.py, .github/workflows/PullRequestCI.yml, README.md, .planning/REQUIREMENTS.md, .planning/ROADMAP.md]

key-decisions:
  - "Removed intersphinx extension: DuckDB docs have no objects.inv, mapping 404s break -W builds"
  - "Docs.yml deploys only from main branch (not milestone branches) to avoid overwriting published docs"
  - "cancel-in-progress: false on Pages deployment to avoid incomplete deployments"

patterns-established:
  - "GitHub Pages deployment: build job (sphinx-build + upload artifact) then deploy job (OIDC token)"
  - "PR validation: sphinx-build -W in docs-check job catches broken refs before merge"

requirements-completed: [DOCS-01, DOCS-02, DOCS-03, DOCS-04]

# Metrics
duration: 5min
completed: 2026-03-27
---

# Phase 35 Plan 01: Documentation Site Summary

**GitHub Pages deployment workflow with Sphinx -W validation, PR build checks, and README documentation link**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-27T08:33:46Z
- **Completed:** 2026-03-27T08:39:00Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments
- Fixed Sphinx config: added githubpages extension, removed broken intersphinx mapping
- Created Docs.yml workflow deploying to GitHub Pages on push to main with OIDC-based deployment
- Added docs-check job to PullRequestCI.yml validating sphinx-build -W on every PR
- Added documentation badge and link to README, updated version to v0.5.4
- Updated planning metadata (REQUIREMENTS.md, ROADMAP.md) to reflect Sphinx + Shibuya framework

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix Sphinx config and update planning metadata** - `e20f9f2` (chore)
2. **Task 2: Create Docs.yml deployment workflow and add docs-check to PullRequestCI.yml** - `67b957c` (feat)
3. **Task 3: Add documentation badge and link to README** - `9c9692c` (docs)

## Files Created/Modified
- `.github/workflows/Docs.yml` - GitHub Pages deployment workflow (build + deploy jobs)
- `.github/workflows/PullRequestCI.yml` - Added docs-check job for PR validation
- `docs/conf.py` - Added githubpages extension, removed intersphinx
- `README.md` - Added docs badge, Documentation section, updated version to v0.5.4
- `.planning/REQUIREMENTS.md` - Updated DOCS-01 to reflect Sphinx + Shibuya
- `.planning/ROADMAP.md` - Updated Phase 35 framework and success criterion

## Decisions Made
- Removed `sphinx.ext.intersphinx` and `intersphinx_mapping` entirely because DuckDB docs do not publish an `objects.inv` file, causing 404 failures under `-W` (warnings-as-errors) builds
- Docs.yml triggers only on `push: branches: [main]` (not milestone branches) to avoid overwriting published docs with in-progress content
- Used `cancel-in-progress: false` for the Pages concurrency group to prevent canceling in-progress deployments (unlike Build.yml which uses `true`)
- Used uv + `astral-sh/setup-uv@v7` in CI workflows matching the Justfile `docs-build` recipe

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required

**Manual step required after merge to main:**
- Enable GitHub Pages in repo Settings > Pages > Source = "GitHub Actions"
- First push to main after this merge will trigger the Docs.yml workflow

## Next Phase Readiness
- Documentation site infrastructure is complete
- Ready for Phase 36 (Registry Publishing & Maintainer Docs) which depends on docs site for CE page link

## Self-Check: PASSED

All files exist. All commits verified (e20f9f2, 67b957c, 9c9692c).

---
*Phase: 35-documentation-site*
*Completed: 2026-03-27*
