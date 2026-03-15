---
phase: quick-16
plan: 01
subsystem: examples
tags: [documentation, examples, v0.5.3]
key-files:
  created:
    - examples/advanced_features.py
decisions: []
metrics:
  duration: 4min
  completed: "2026-03-15T08:10:00Z"
---

# Quick Task 16: Advanced Features Example Summary

Self-contained Python example demonstrating all v0.5.3 semantic features with realistic e-commerce and flight data, runnable via `uv run examples/advanced_features.py`.

## What Was Done

### Task 1: Create advanced_features.py example script

Created `examples/advanced_features.py` (360 lines) following the exact style of `basic_ddl_and_query.py`:
- PEP 723 inline metadata header with `duckdb==1.4.4` dependency
- Extension loaded from `build/debug/semantic_views.duckdb_extension`
- 8 labeled sections with printed output

**Features demonstrated:**

1. **FACTS** -- Reusable row-level expressions with chaining (net_price -> tax_amount)
2. **HIERARCHIES** -- Drill-down path metadata shown via DESCRIBE
3. **Derived metrics** -- Metric-on-metric composition (profit = revenue - cost, margin = profit / revenue * 100)
4. **Fan trap detection** -- MANY TO ONE cardinality annotations; error shown when querying across fan-out direction
5. **Role-playing dimensions** -- USING RELATIONSHIPS for departure/arrival airports; ambiguous query error demonstrated
6. **EXPLAIN** -- Shows generated SQL with scoped aliases (`a__dep_airport`)
7. **DESCRIBE** -- Full metadata view with USING relationships

**Verified output values:**
- East: total_net=250, total_tax=17.30, profit=120, margin=48.0%
- West: total_net=150, total_tax=15.00, profit=90, margin=60.0%
- Grand total profit: 210
- Fan trap error correctly blocks inflated count
- Ambiguous role-playing error correctly blocks city+total_flights

**Commit:** c9f8e16

## Deviations from Plan

None -- plan executed exactly as written.

## Verification

Script runs end-to-end with `uv run examples/advanced_features.py` after `just build`. All 8 sections print correct output, error cases are caught and displayed.

## Self-Check: PASSED

- [x] examples/advanced_features.py exists (360 lines, exceeds 150-line minimum)
- [x] Commit c9f8e16 exists
- [x] Script runs end-to-end without errors
- [x] All 6 v0.5.3 features demonstrated
