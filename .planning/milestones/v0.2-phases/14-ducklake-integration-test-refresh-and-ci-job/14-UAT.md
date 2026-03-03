---
status: passed
phase: 14-ducklake-integration-test-refresh-and-ci-job
source: 14-01-SUMMARY.md, 14-02-SUMMARY.md, 14-03-SUMMARY.md
started: 2026-03-02T00:00:00Z
updated: 2026-03-02T00:00:00Z
---

## Current Test
<!-- OVERWRITE each test - shows where we are -->

number: 5
name: DuckDBVersionMonitor.yml has DuckLake compatibility step
expected: |
  Opening `.github/workflows/DuckDBVersionMonitor.yml` shows a `ducklake_test` step
  with `continue-on-error: true`, and both PR body templates reference
  `steps.ducklake_test.outcome`.
awaiting: complete

## Tests

### 1. CI integration test runs locally
expected: Run `just test-ducklake-ci` — all 6 test cases pass (define view, query with dimension, global aggregate, explain, typed BIGINT, time dimension with date assertions). No errors.
result: pass

### 2. Local DuckLake test uses v0.2.0 API
expected: Run `uv run test/integration/test_ducklake.py` — passes using the new `create_semantic_view` and `semantic_view` API (no legacy `define_semantic_view` or `semantic_query` calls). 6 test cases pass.
result: pass

### 3. test-all includes DuckLake CI target
expected: Running `just test-all` (or inspecting the Justfile) shows `test-ducklake-ci` is included in the full test suite.
result: pass
notes: Justfile:89 `test-all: test-rust test-sql test-iceberg test-ducklake-ci`. Confirmed via `just test-all` — all tests passed (including after HUGEINT bug fix).

### 4. PullRequestCI.yml has ducklake-ci-check job
expected: Opening `.github/workflows/PullRequestCI.yml` shows a `ducklake-ci-check` parallel job (no `needs:` dependency) that builds the extension and runs `test_ducklake_ci.py`.
result: pass
notes: Job at line 26, no `needs:` key (runs in parallel). Builds extension then runs `uv run test/integration/test_ducklake_ci.py`.

### 5. DuckDBVersionMonitor.yml has DuckLake compatibility step
expected: Opening `.github/workflows/DuckDBVersionMonitor.yml` shows a `ducklake_test` step with `continue-on-error: true`, and both PR body templates reference `steps.ducklake_test.outcome`.
result: pass
notes: Step at line 64 with `continue-on-error: true` (line 69). Both PR templates reference outcome at lines 82 and 101.

## Summary

total: 5
passed: 5
issues: 0
pending: 0
skipped: 0

## Gaps

[none]
