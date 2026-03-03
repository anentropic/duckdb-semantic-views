---
phase: 14-ducklake-integration-test-refresh-and-ci-job
plan: "02"
subsystem: integration-tests
tags: [ducklake, integration-test, python, v0.2.0-api]
provides:
  - Updated test_ducklake.py with v0.2.0 API and 6 test cases
affects:
  - test/integration/test_ducklake.py
tech-stack:
  added: []
  patterns:
    - v0.2.0 create_semantic_view 6-arg STRUCT/LIST DDL
    - semantic_view table function (renamed from semantic_query)
    - Shared helpers import pattern
key-files:
  created: []
  modified:
    - test/integration/test_ducklake.py
key-decisions:
  - "Rewrote define_semantic_view JSON call to create_semantic_view STRUCT/LIST call with time_dimension arg"
  - "Renamed semantic_query → semantic_view throughout"
  - "Added time_dimension to initial view definition (Test 1) so Tests 5 and 6 can use it"
  - "Test 6 (time dimension) uses isinstance check for datetime.date but no known-value assertions (real jaffle-shop data, dates not predetermined)"
requirements-completed: [DUCKLAKE-LOCAL]
duration: "5 min"
completed: "2026-03-02T18:18:00Z"
---

# Phase 14 Plan 02: Update Local DuckLake Test Summary

Updated test_ducklake.py from v0.1.0 API to v0.2.0 API and added 2 new test cases.

**Duration:** 5 min | **Tasks:** 1 | **Files modified:** 1

## What Was Built

Updated `test/integration/test_ducklake.py`:
- Replaced `define_semantic_view('name', json_string)` with `create_semantic_view(name, tables, rels, dims, time_dims, metrics)` (v0.2.0 6-arg STRUCT/LIST API)
- Replaced `semantic_query(...)` with `semantic_view(...)` throughout
- Added import from `test_ducklake_helpers` for shared boilerplate
- Added Test 5: typed BIGINT assertion (isinstance int)
- Added Test 6: time dimension returns datetime.date values
- View definition now includes `ordered_at` as a time_dimension with day granularity

## Task Commits

- Task 1: `80b360e` — feat(14-02): rewrite test_ducklake.py

## Deviations from Plan

None - plan executed exactly as written. The original 4 tests preserved and updated; 2 new tests added.

## Next

Ready for 14-03 (CI job wiring).

## Self-Check: PASSED
- test/integration/test_ducklake.py: exists ✓
- No 'define_semantic_view' or 'semantic_query' in file ✓
- git log: feat(14-02) commit present ✓
