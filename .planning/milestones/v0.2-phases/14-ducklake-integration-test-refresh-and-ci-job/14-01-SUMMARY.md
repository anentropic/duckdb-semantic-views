---
phase: 14-ducklake-integration-test-refresh-and-ci-job
plan: "01"
subsystem: integration-tests
tags: [ducklake, integration-test, ci, python]
provides:
  - test_ducklake_helpers.py module with shared extension-load and DuckLake-attach helpers
  - test_ducklake_ci.py CI integration test with 6 test cases and inline synthetic data
affects:
  - test/integration/
tech-stack:
  added: []
  patterns:
    - PEP 723 uv script with inline dependencies
    - tempfile.mkdtemp() for isolated DuckLake catalog creation
    - SEMANTIC_VIEWS_EXTENSION_PATH env var for CI extension path override
key-files:
  created:
    - test/integration/test_ducklake_helpers.py
    - test/integration/test_ducklake_ci.py
  modified: []
key-decisions:
  - "Used SEMANTIC_VIEWS_EXTENSION_PATH env var with fallback to build/debug/ — CI sets the env var, local uses CMake build path"
  - "setup_synthetic_ducklake() creates 5 rows with 3 distinct dates — deterministic for time dimension assertions"
  - "shutil.rmtree in finally block ensures temp dir cleanup even on test failure"
requirements-completed: [DUCKLAKE-CI]
duration: "8 min"
completed: "2026-03-02T18:18:00Z"
---

# Phase 14 Plan 01: Create Helpers and CI Test Summary

Created the shared helpers module and CI integration test with inline synthetic data.

**Duration:** 8 min | **Tasks:** 2 | **Files created:** 2

## What Was Built

- `test/integration/test_ducklake_helpers.py` — shared module with `get_project_root()`, `get_ext_dir()`, `get_extension_path()`, `load_extension()`, and `attach_ducklake()` functions. Handles SEMANTIC_VIEWS_EXTENSION_PATH env var for CI path override.

- `test/integration/test_ducklake_ci.py` — self-contained CI integration test that creates a DuckLake catalog in a temp directory with 5 synthetic rows (3 distinct dates, 2 distinct store IDs). Runs 6 test cases: define view, query with dimension, global aggregate, explain, typed BIGINT output, time dimension with known date assertions.

## Task Commits

- Task 1: `6b02d81` — feat(14-01): create test_ducklake_helpers.py
- Task 2: `4297c26` — feat(14-01): create test_ducklake_ci.py

## Deviations from Plan

None - plan executed exactly as written.

## Next

Ready for 14-02 (update local test) and 14-03 (CI wiring).

## Self-Check: PASSED
- test/integration/test_ducklake_helpers.py: exists ✓
- test/integration/test_ducklake_ci.py: exists ✓
- git log: feat(14-01) commits present ✓
