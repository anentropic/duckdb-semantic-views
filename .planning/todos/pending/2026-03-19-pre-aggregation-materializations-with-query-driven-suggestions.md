---
created: 2026-03-19T23:28:00.000Z
title: Pre-aggregation materializations with query-driven suggestions
area: general
files: []
---

## Problem

Currently duckdb-semantic-views is expansion-only — every query hits the base tables. Real semantic layers (Databricks Lakeview, dbt, Cube.dev) support pre-aggregated materializations that dramatically improve query performance for common access patterns.

Three interconnected features:

### 1. Materialization DDL

Define pre-aggregated tables as part of the semantic view definition. Prior art:

- **Cube.dev**: `preAggregations` block defines rollups with dimensions, measures, time granularity, refresh schedule
- **dbt**: `materialized` config on metrics/models (table, incremental, ephemeral)
- **Databricks**: Materialized views with automatic refresh

Syntax idea (TBD):
```sql
CREATE SEMANTIC VIEW sales AS
  ...
  MATERIALIZATIONS (
    daily_by_region AS (region, date_trunc('day', order_date)) METRICS (revenue, order_count) REFRESH INTERVAL '1 hour'
  )
```

### 2. Query rewrite engine

Like Cube.dev's query planner — when a query comes in via `semantic_view()`, check if a materialization covers the requested dimensions + metrics at sufficient granularity. If yes, rewrite the query to hit the pre-aggregated table instead of the base tables. Transparent to the caller.

Key challenge: partial coverage — what if a materialization covers some but not all requested dimensions? Cube.dev handles this with rollup selection logic.

### 3. Query statistics and suggested materializations

Track query patterns (which dimension/metric combinations are requested, how often, latency) in an auto-maintained stats table. Use this to suggest materializations:

```sql
SELECT * FROM semantic_view_stats('sales');
-- Shows: dimension_combo, metric_combo, query_count, avg_duration, suggested_materialization
```

This closes the feedback loop: use → measure → suggest → materialize → faster queries.

## Solution

TBD — needs research into:
- Cube.dev pre-aggregation architecture (rollup selection algorithm, refresh mechanics)
- How materialization refresh works in DuckDB context (CRON? On-query staleness check?)
- Query stats collection without significant overhead (sampling? lightweight counters?)
- DDL syntax design for MATERIALIZATIONS clause
- Whether this is a single milestone or needs to be split (likely: stats first, then materializations, then rewrite engine)
- Note: "no pre-aggregation" is listed in TECH-DEBT.md as an accepted deferral — this would be the future milestone that picks it up
