---
phase: 48-window-function-metrics
verified: 2026-04-12T20:30:00Z
status: passed
score: 5/5 must-haves verified
re_verification: false
---

# Phase 48: Window Function Metrics Verification Report

**Phase Goal:** Users can declare and query window function metrics that produce non-aggregated results with partition-aware computation
**Verified:** 2026-04-12T20:30:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

All five roadmap success criteria verified against actual codebase and integration tests.

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | User can declare a window function metric with PARTITION BY EXCLUDING in DDL and the view stores successfully | VERIFIED | `parse_window_over_clause` in body_parser.rs (34 references), MetricEntry 9-tuple with `Option<WindowSpec>`, backward-compat `serde(default, skip_serializing_if = "Option::is_none")`, sqllogictest Test 1 passes |
| 2 | Querying a window function metric produces correct non-aggregated results (no GROUP BY in generated SQL) | VERIFIED | `expand_window_metrics` in window.rs generates `WITH __sv_agg AS (... GROUP BY ...)` CTE then outer SELECT FROM `__sv_agg` with no outer GROUP BY; sqllogictest Tests 2 and 3 verify correct AVG and LAG results |
| 3 | Attempting to mix window function metrics with aggregate metrics in the same query produces a clear blocking error | VERIFIED | `WindowAggregateMixing` error variant in types.rs (2 occurrences), dispatch in sql_gen.rs (`has_window` check), sqllogictest Test 4b verifies "cannot mix window function metrics" error |
| 4 | Window function metrics are excluded from fan trap detection (no false errors) | VERIFIED | `if met.is_window() { continue; }` in fan_trap.rs, sqllogictest Test 9 queries window metric across many-to-one boundary without fan trap error |
| 5 | SHOW SEMANTIC DIMENSIONS FOR METRIC shows required=TRUE for dimensions in the window metric's partition specification | VERIFIED | `required: bool` field in ShowDimForMetricRow, `window_spec` lookup for EXCLUDING/ORDER BY dims in show_dims_for_metric.rs, sqllogictest Test 6 verifies `date` required=TRUE, `store`/`year` required=FALSE |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/model.rs` | WindowSpec, WindowOrderBy structs with serde | VERIFIED | `pub struct WindowSpec` (1), `pub struct WindowOrderBy` (1), `pub window_spec: Option<WindowSpec>` on Metric (1), `fn is_window()` (1), all with `serde(default, skip_serializing_if)` for backward compat |
| `src/body_parser.rs` | OVER clause parsing in metric expressions | VERIFIED | `parse_window_over_clause` function (line 1372), MetricEntry extended to 9-tuple (Option<WindowSpec> as 9th element), mutual exclusion errors for OVER+derived and OVER+NON_ADDITIVE_BY |
| `src/render_ddl.rs` | OVER clause emission in GET_DDL | VERIFIED | 7 references to `window_spec`, reconstructs OVER clause from parsed WindowSpec with explicit NULLS placement |
| `src/expand/window.rs` | Window metric CTE expansion | VERIFIED | 655 lines, `pub(super) fn expand_window_metrics`, `WITH __sv_agg AS (...)` CTE pattern, `PARTITION BY` set-difference computation, frame clause passthrough |
| `src/expand/sql_gen.rs` | Window metric dispatch in expand() | VERIFIED | `has_window` dispatch check (2 occurrences), calls `super::window::expand_window_metrics`, `WindowAggregateMixing` guard |
| `src/expand/fan_trap.rs` | Window metric skip in fan trap check | VERIFIED | `if met.is_window() { continue; }` (1 occurrence) |
| `src/ddl/show_dims_for_metric.rs` | required=TRUE for window metric dims | VERIFIED | `required: bool` in struct (1), `window_spec` lookup (1), `row.required` read in func() (1) |
| `test/sql/phase48_window_metrics.test` | End-to-end sqllogictest integration | VERIFIED | 266 lines, 18 `statement ok`, 3 `statement error`, 7 `PARTITION BY EXCLUDING` occurrences, covers DDL, AVG query, LAG query, mixing error, missing dim error, SHOW DIMS, GET_DDL, DESCRIBE, fan trap |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/expand/sql_gen.rs` | `src/expand/window.rs` | `expand_window_metrics()` dispatch | WIRED | `return super::window::expand_window_metrics(...)` at line 357 |
| `src/expand/window.rs` | `src/model.rs` | WindowSpec field access for SQL generation | WIRED | `met.window_spec`, `ws.excluding_dims`, `ws.order_by`, `ws.frame_clause` throughout window.rs |
| `src/expand/fan_trap.rs` | `src/model.rs` | `is_window()` check for skip | WIRED | `met.is_window()` at line 72 |
| `src/ddl/show_dims_for_metric.rs` | `src/model.rs` | WindowSpec excluding_dims + order_by for required flag | WIRED | `if let Some(ref ws) = met.window_spec` at line 222 |
| `src/body_parser.rs` | `src/model.rs` | WindowSpec struct construction | WIRED | `WindowSpec { ... }` construction (line 1439), imported at line 9 |
| `src/render_ddl.rs` | `src/model.rs` | WindowSpec field access for DDL emission | WIRED | 7 references to `window_spec` in render_ddl.rs |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|--------------|--------|-------------------|--------|
| `src/expand/window.rs` | `resolved_exprs` (inner metric expressions) | `resolved_mets` / `ws.inner_metric` lookup | Yes — resolves from actual SemanticViewDefinition metric entries | FLOWING |
| `src/ddl/show_dims_for_metric.rs` | `required_dim_names` | `met.window_spec.excluding_dims` + `order_by[].expr` from stored definition | Yes — populated from actual parsed WindowSpec, not hardcoded | FLOWING |
| `test/sql/phase48_window_metrics.test` | Query results | DuckDB execution of generated CTE SQL | Yes — verified against known data (LA/NYC sales rows) with exact expected values | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| cargo test suite (646 tests) | `cargo test` | 646 passed, 0 failed | PASS |
| sqllogictest (30 files incl. phase48) | `just test-sql` | 30 tests run, 0 failed | PASS |
| DuckLake CI integration tests | `just test-ducklake-ci` | 6 passed, 0 failed, ALL PASSED | PASS |
| WindowSpec struct exists | `grep -c "pub struct WindowSpec" src/model.rs` | 1 | PASS |
| MetricEntry is 9-tuple | `grep -A 12 "type MetricEntry" src/body_parser.rs` | 9 type elements confirmed | PASS |
| expand_window_metrics dispatched | `grep -n "expand_window_metrics" src/expand/sql_gen.rs` | line 357: `return super::window::expand_window_metrics(...)` | PASS |
| Fan trap skip wired | `grep -n "is_window" src/expand/fan_trap.rs` | line 72: `met.is_window()` | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| WIN-01 | 48-01 | User can declare a window function metric with PARTITION BY EXCLUDING in DDL | SATISFIED | `parse_window_over_clause` in body_parser.rs, WindowSpec model in model.rs, serde backward compat, sqllogictest Test 1. NOTE: REQUIREMENTS.md traceability table still shows "Pending" — this is a documentation artifact only; the code fully implements WIN-01 |
| WIN-02 | 48-02 | Window function metrics produce correct non-aggregated results at query time | SATISFIED | CTE + outer window SELECT in window.rs; sqllogictest Tests 2 (AVG) and 3 (LAG) verify correct results |
| WIN-03 | 48-02 | Queries cannot mix window function metrics with aggregate metrics (blocking error) | SATISFIED | `WindowAggregateMixing` error in types.rs + dispatch guard in sql_gen.rs; sqllogictest Test 4b |
| WIN-04 | 48-02 | Window function metrics are excluded from fan trap detection | SATISFIED | `if met.is_window() { continue; }` in fan_trap.rs; sqllogictest Test 9 |
| WIN-05 | 48-02 | SHOW SEMANTIC DIMENSIONS FOR METRIC shows required=TRUE for window metric partition dimensions | SATISFIED | `required: bool` in ShowDimForMetricRow, window_spec lookup; sqllogictest Test 6 |

### Anti-Patterns Found

No anti-patterns found. Scanned `src/expand/window.rs`, `src/model.rs`, `src/body_parser.rs`, `src/render_ddl.rs`, `src/ddl/show_dims_for_metric.rs`, `src/expand/fan_trap.rs` for TODO/FIXME/placeholder/empty-return patterns. None detected.

### Human Verification Required

None. All success criteria are verifiable programmatically. Quality gate (`just test-all`) passed with:
- 646 Rust tests (cargo test): 0 failures
- 30 sqllogictest files (just test-sql): 0 failures
- DuckLake CI (just test-ducklake-ci): 6/6 passed

## Gaps Summary

No gaps. All 5 roadmap success criteria are satisfied by substantive, wired implementations with flowing data and passing integration tests.

**One documentation note (not a gap):** The REQUIREMENTS.md traceability table shows WIN-01 as "Pending" and the checkbox is unchecked, but the implementation is complete. This is a documentation artifact — the executor completed WIN-01 implementation in Plan 01 (commit 9bb8471, 46f4c8a) but did not update REQUIREMENTS.md. The code, tests, and plan summaries all confirm WIN-01 is complete.

---

_Verified: 2026-04-12T20:30:00Z_
_Verifier: Claude (gsd-verifier)_
