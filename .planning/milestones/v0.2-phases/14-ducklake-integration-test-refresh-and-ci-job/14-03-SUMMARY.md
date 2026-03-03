---
phase: 14-ducklake-integration-test-refresh-and-ci-job
plan: "03"
subsystem: ci
tags: [ci, github-actions, ducklake, justfile]
provides:
  - ducklake-ci-check parallel CI job in PullRequestCI.yml
  - DuckLake compatibility step in DuckDBVersionMonitor.yml
  - test-ducklake-ci Justfile target
affects:
  - .github/workflows/PullRequestCI.yml
  - .github/workflows/DuckDBVersionMonitor.yml
  - Justfile
tech-stack:
  added: []
  patterns:
    - Parallel CI jobs (no needs: dependency)
    - continue-on-error for non-blocking DuckLake check
    - SEMANTIC_VIEWS_EXTENSION_PATH env var for CI extension path
key-files:
  created: []
  modified:
    - .github/workflows/PullRequestCI.yml
    - .github/workflows/DuckDBVersionMonitor.yml
    - Justfile
key-decisions:
  - "ducklake-ci-check uses cargo build --features extension (not CMake/make) — direct Rust build outputs to target/debug/"
  - "Version monitor DuckLake test uses build/release/ path (CMake make test_release builds there)"
  - "test-ducklake-ci Justfile target has no 'build' prerequisite — CI triggers it after explicit build step"
requirements-completed: [DUCKLAKE-CI, DUCKLAKE-MONITOR]
duration: "5 min"
completed: "2026-03-02T18:22:00Z"
---

# Phase 14 Plan 03: CI Wiring Summary

Wired the DuckLake CI test into the CI system.

**Duration:** 5 min | **Tasks:** 3 | **Files modified:** 3

## What Was Built

- **PullRequestCI.yml** — Added `ducklake-ci-check` parallel job (no `needs:`) running on `ubuntu-latest`. Installs Rust stable, builds extension with `cargo build --features extension`, installs uv via pip, runs `uv run test/integration/test_ducklake_ci.py` with `SEMANTIC_VIEWS_EXTENSION_PATH=target/debug/semantic_views.duckdb_extension`.

- **DuckDBVersionMonitor.yml** — Added `ducklake_test` step after `build` step with `continue-on-error: true`. Step runs `pip install uv` then the CI test with `build/release/` extension path. Both PR body templates (version bump and breakage) now include DuckLake compatibility status line using `steps.ducklake_test.outcome`.

- **Justfile** — Added `test-ducklake-ci` target running `uv run test/integration/test_ducklake_ci.py` (no `build` prerequisite). Updated `test-all` to include `test-ducklake-ci`. Updated `test-iceberg` comment to say `semantic_view` (renamed from `semantic_query`).

## Task Commits

- Task 1: `bc7f0cf` — feat(14-03): add ducklake-ci-check parallel job to PullRequestCI.yml
- Task 2: `bce77bf` — feat(14-03): add DuckLake test step to DuckDBVersionMonitor.yml
- Task 3: `572abeb` — feat(14-03): add test-ducklake-ci target to Justfile

## Deviations from Plan

None - plan executed exactly as written.

## Self-Check: PASSED
- .github/workflows/PullRequestCI.yml: ducklake-ci-check job present, no needs: field ✓
- .github/workflows/DuckDBVersionMonitor.yml: ducklake_test step with continue-on-error, DuckLake compatibility lines in PR bodies ✓
- Justfile: test-ducklake-ci target and test-all update ✓
- git log: feat(14-03) commits present ✓
