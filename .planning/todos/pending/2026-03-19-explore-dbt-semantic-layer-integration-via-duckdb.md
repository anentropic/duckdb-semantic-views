---
created: 2026-03-19T23:25:00.000Z
title: Explore dbt semantic layer integration via DuckDB
area: general
files: []
---

## Problem

Could duckdb-semantic-views serve as a local query frontend for dbt semantic layer definitions? The idea: import or translate dbt MetricFlow YAML (models, metrics, dimensions, entities) into CREATE SEMANTIC VIEW DDL, then query locally via DuckDB instead of going through the dbt Cloud Semantic Layer API.

Key questions to research:

1. **Source table access** — dbt models typically materialize in a warehouse (Snowflake, BigQuery, Redshift, Databricks). For DuckDB to query them:
   - **Iceberg**: DuckDB has native Iceberg support — if warehouse tables are exposed as Iceberg (Snowflake Iceberg tables, Databricks Unity Catalog), DuckDB can read them directly
   - **Parquet/S3**: If dbt materializes to object storage (e.g., dbt-duckdb adapter exports)
   - **Postgres**: DuckDB postgres_scanner for Postgres-backed dbt projects
   - **Direct warehouse connectors**: DuckDB doesn't have native Snowflake/BigQuery connectors — would need export or Iceberg bridge
   - Constraint: this only works if the underlying tables are accessible to DuckDB somehow

2. **Schema translation** — How close is MetricFlow YAML to our DDL?
   - MetricFlow: entities (join keys), dimensions, measures, metrics (derived from measures)
   - Ours: TABLES, RELATIONSHIPS (PK/FK), DIMENSIONS, METRICS, FACTS
   - Entities → RELATIONSHIPS mapping seems feasible
   - MetricFlow measures vs our METRICS have different semantics (measures are pre-aggregation building blocks, metrics compose them)

3. **Value proposition** — Why would someone use this over dbt Cloud SL API?
   - Local/offline querying
   - No dbt Cloud dependency (works with dbt Core)
   - DuckDB performance on local/cached data
   - Exploratory analysis without hitting the warehouse

## Solution

TBD — needs research into:
- MetricFlow YAML schema and how it maps to CREATE SEMANTIC VIEW DDL
- Which dbt materializations produce DuckDB-readable outputs (Iceberg, Parquet, dbt-duckdb)
- Whether a `dbt-to-semantic-view` translator tool makes sense as a companion project
- dbt Core vs dbt Cloud constraints on accessing the semantic layer definitions
