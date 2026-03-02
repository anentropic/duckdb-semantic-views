# Design Doc: Semantic Views Extension for DuckDB

## Overview

This document captures the design rationale for a DuckDB extension that implements **semantic views** — a declarative layer for defining measures, dimensions, relationships, and pre-aggregated materializations — inspired by Snowflake's `SEMANTIC_VIEW` and Cube.dev's pre-aggregation system.

The core insight driving the architecture: semantic views are **syntax sugar for parameterised analytic queries**, and pre-aggregation selection is **another layer of that sugar** before handing the prepared query to DuckDB itself.

---

## Prior Art

### Cube.dev

Cube is an open-source semantic layer that sits between BI tools and data sources. Its pre-aggregation system is the most mature implementation of aggregate-aware query routing in the open-source ecosystem.

**Architecture:**

- **Schema compiler** (`@cubejs-backend/schema-compiler`, JS/TS): Compiles YAML/JS data model definitions into an internal representation. Contains the original pre-aggregation matching logic in `PreAggregations.js` — a sequential scan that tests each candidate pre-aggregation against the incoming query via set-containment checks.
- **CubeSQL** (`rust/cubesql/`, Rust): SQL API layer that accepts Postgres wire protocol queries from BI tools. Uses Apache DataFusion as its query engine and the `egg` e-graph library for query plan rewriting.
- **Tesseract** (`rust/cubesqlplanner/`, Rust): Next-generation SQL planner for multi-stage calculations (period-over-period, nested aggregations, YTD). Still in preview, enabled via `CUBEJS_TESSERACT_SQL_PLANNER`.
- **Cube Store** (`rust/cubestore/`, Rust): Custom columnar store built on Parquet, Arrow, and DataFusion. Stores and serves pre-aggregated rollup tables.
- **Query Orchestrator** (`packages/cubejs-query-orchestrator/`, JS/TS): Manages the three-way routing decision — serve from pre-aggregation, query upstream warehouse, or push down to target database.

**Key finding:** None of Cube's Rust crates are published to crates.io. They are internal workspace crates with path dependencies, deep coupling to Cube's data model, a JS↔Rust Neon bridge, and forked DataFusion/Arrow dependencies. Direct reuse is not feasible.

**Where `egg` is used in Cube:** The `egg` e-graph rewriting library operates in CubeSQL's query planning phase — *not* in pre-aggregation selection. Its purpose is to recognise incoming SQL from BI tools (Tableau, Superset, Metabase — each generating wildly different SQL for the same semantic query) and rewrite it into Cube's internal `CubeScan` plan nodes. It also handles the three-way classification of queries into regular queries, queries with post-processing, and queries with pushdown. This is necessary because Cube acts as its own query engine, not a preprocessor for another database.

**Pre-aggregation matching algorithm (from Cube docs and source):**

1. Extract members (measures, dimensions, time dimensions) from the query
2. If query references views, resolve to underlying cube members
3. Scan pre-aggregations in definition order (rollups before `original_sql`)
4. For each candidate, check:
   - All leaf measures are present and **additive** (sum, count, min, max compose; countDistinct does not)
   - All dimensions and filter dimensions are present
   - Time granularity is ≤ query granularity (use GCD — a `day` rollup serves `week`/`month` queries)
   - Time zone matches
   - Join tree is compatible
5. First match wins; fall back to raw source if none

### Snowflake Semantic Views

Snowflake's semantic views are SQL-native schema objects (`CREATE SEMANTIC VIEW`) that define logical tables, dimensions, facts, metrics, and relationships. They are queried via a `SEMANTIC_VIEW(...)` clause in `FROM`.

**Key design decisions relevant to this project:**

- **Composable with regular SQL.** The `SEMANTIC_VIEW(...)` clause produces a derived table that can participate in JOINs, CTEs, PIVOT, UNPIVOT, and GROUP BY like any other relation.
- **Metrics are auto-aggregated.** Requesting `METRICS orders.total_revenue DIMENSIONS customer.region` produces a grouped result. The engine handles the GROUP BY.
- **Granularity validation.** Snowflake validates that dimension entities have equal or lower granularity than metric entities in a query. Invalid combinations produce compile-time errors.
- **No pre-aggregation layer.** Snowflake semantic views generate SQL against physical tables directly. There is no materialized view selection — Snowflake relies on its own engine optimisation.

---

## Design Principles

1. **DuckDB does the heavy lifting.** We are a preprocessing layer, not a query engine. All execution is pushed down to DuckDB.
2. **Semantic views are syntax sugar.** They encapsulate complex analytic query logic. The primary use case is standalone queries, not composition with other views or tables.
3. **Composition should work, but needn't be optimal.** SQL allows joining semantic views to other tables. This must produce correct results but is not the performance-critical path. Each semantic view expands independently.
4. **Pre-aggregation selection is a substitution, not a rewrite.** Given a semantic query and a set of materialised tables, pick the best match and substitute the table reference. This is a pure function with no global query analysis.

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  User SQL                        │
│  SELECT * FROM SEMANTIC_VIEW(                    │
│    my_view DIMENSIONS region METRICS revenue     │
│  ) JOIN other_table ON ...                       │
└───────────────────┬─────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────┐
│           1. Semantic View Expansion              │
│                                                   │
│  Resolve dimensions → column expressions          │
│  Resolve metrics → aggregation expressions        │
│  Resolve relationships → JOIN clauses             │
│  Emit concrete subquery                           │
└───────────────────┬─────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────┐
│        2. Pre-Aggregation Selection               │
│                                                   │
│  For each semantic view reference:                │
│    Match (requested dims, metrics, granularity)   │
│    against available materialised tables          │
│    Substitute table scan if match found           │
│    Possibly add re-aggregation layer              │
└───────────────────┬─────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────┐
│              3. DuckDB Execution                  │
│                                                   │
│  Concrete SQL with physical table references      │
│  DuckDB handles: join reordering, predicate       │
│  pushdown, filter elimination, execution          │
└─────────────────────────────────────────────────┘
```

### Step 1: Semantic View Expansion

**Input:** Semantic view reference with requested dimensions and metrics.

**Process:** Deterministic template expansion. The semantic view definition fully determines:
- Which base tables to scan
- Which JOINs to apply (from relationship definitions)
- Which expressions to compute (dimension and metric SQL expressions)
- Which GROUP BY to apply (all requested dimensions)

**Output:** A concrete SQL subquery.

This step is analogous to how Snowflake generates SQL from semantic view definitions — there is no search space, no ambiguity, and no need for term rewriting.

### Step 2: Pre-Aggregation Selection

**Input:** The concrete subquery from Step 1, plus a catalogue of available materialised tables.

**Process:** Set-containment matching, adapted from Cube's algorithm:

```
fn select_pre_aggregation(
    requested_measures: &[Measure],
    requested_dimensions: &[Dimension],
    requested_granularity: Option<Granularity>,
    available_materialisations: &[Materialisation],
) -> Option<&Materialisation> {
    for mat in available_materialisations {
        if !requested_measures.iter().all(|m| mat.contains_measure(m)) {
            continue;
        }
        if !requested_measures.iter().all(|m| m.is_additive()) {
            continue;  // countDistinct etc. can't be re-aggregated
        }
        if !requested_dimensions.iter().all(|d| mat.contains_dimension(d)) {
            continue;
        }
        if let Some(gran) = requested_granularity {
            if !mat.granularity_is_finer_or_equal(gran) {
                continue;
            }
        }
        return Some(mat);
    }
    None
}
```

If a match is found, the subquery is rewritten to scan the materialised table instead of the raw tables. If the materialised table has finer granularity than requested, a re-aggregation wrapper is added (e.g., the mat table is daily but the query asks for monthly — wrap with `GROUP BY month`).

**Output:** A potentially rewritten subquery pointing at a materialised table, or the original subquery unchanged.

### Step 3: DuckDB Execution

The rewritten SQL is handed to DuckDB as a standard query. DuckDB handles all downstream optimisation — predicate pushdown through joins, join reordering, parallel execution, etc.

If the user has composed the semantic view with other tables (JOINs, CTEs, subqueries), DuckDB optimises the full query plan. Our extension does not need to reason about this composition.

---

## Why `egg` Is Not Needed

We evaluated the `egg` e-graph equality saturation library (used by Cube's CubeSQL) for this project. Conclusion: it solves a different problem.

| Concern | Cube's situation | Our situation |
|---|---|---|
| **Inbound query format** | Arbitrary SQL from diverse BI tools (Tableau, Superset, Metabase generate different SQL for the same intent) | Structured `SEMANTIC_VIEW(...)` clause with explicit dimensions and metrics |
| **Execution engine** | Cube *is* the engine (DataFusion + Cube Store). Must decide: regular query, post-processing, or pushdown. | DuckDB is the engine. We only preprocess. |
| **Rule interaction** | Many rewrite rules that interact (SQL normalisation, CubeScan recognition, pushdown transpilation). Order matters. | Two deterministic phases (expand, substitute). No rule ordering problem. |
| **Plan search space** | Large — multiple equivalent plans for the same semantic query, varying by execution strategy. | Minimal — expansion is deterministic, substitution is a single match decision. |

`egg` becomes relevant if:
- Multiple materialised views need to be **composed** to answer a single query (partial coverage)
- The interaction between filter pushdown, join elimination, and mat view substitution creates combinatorial complexity
- You need to explore equivalent execution plans because *you are the query engine*

None of these apply. Our pre-aggregation selector is a pure function. DuckDB handles the rest.

---

## Composition Semantics

### Primary use case

Semantic views encapsulate complex analytic query logic. The expected usage is standalone:

```sql
SELECT *
FROM SEMANTIC_VIEW(
    sales_analysis
    DIMENSIONS region, product_category
    METRICS total_revenue, avg_order_value
    WHERE order_date >= '2025-01-01'
)
ORDER BY total_revenue DESC;
```

### Composition (supported but not optimised)

Users may join semantic views to other tables. This must produce correct results:

```sql
SELECT sv.region, sv.total_revenue, t.budget
FROM SEMANTIC_VIEW(
    sales_analysis
    DIMENSIONS region
    METRICS total_revenue
) AS sv
JOIN regional_budgets t ON sv.region = t.region;
```

**Implementation:** Each `SEMANTIC_VIEW(...)` reference is expanded independently into a subquery. Pre-aggregation selection happens per-reference. DuckDB composes and optimises the outer query.

**Non-goal:** Cross-view optimisation. If two semantic view references could share a materialised table, we don't detect this. The result is correct but may involve two scans instead of one.

### Granularity validation

Following Snowflake's approach, the extension should validate at expansion time that requested dimensions and metrics are compatible. Invalid combinations (e.g., a metric that aggregates at the order level paired with a dimension from a higher-granularity table without a valid relationship) should produce a clear compile-time error rather than silently incorrect results.

---

## Materialisation Management

Out of scope for this design doc, but the extension will need:

- **DDL for defining materialisations** — which semantic view, which dimensions/metrics/granularity to pre-aggregate.
- **Refresh mechanism** — on-demand or scheduled rebuild of materialised tables.
- **Catalogue** — metadata tracking available materialisations, their schemas, freshness, and the semantic views they accelerate.

DuckDB's existing table and view infrastructure, plus Parquet file support, provides the storage layer. No need to build a Cube Store equivalent.

---

## Open Questions

1. **Syntax design.** Should we use Snowflake's `SEMANTIC_VIEW(...)` table function syntax, or integrate more deeply into DuckDB's parser with a custom `CREATE SEMANTIC VIEW` DDL?
2. **Metric additivity.** How to handle non-additive metrics (countDistinct, percentiles) in the pre-aggregation matcher — reject, approximate (HLL), or fall through to raw tables?
3. **Time dimension handling.** Should time granularity coarsening (day → month) be handled by the extension's re-aggregation wrapper, or by emitting appropriate `date_trunc` and letting DuckDB handle it?
4. **Multi-stage measures.** Cube's Tesseract supports nested aggregations (e.g., "average of daily totals"). Do we need this, and if so, can it be expressed as CTE expansion without a dedicated planner?

---

## References

- [Cube.dev pre-aggregation matching docs](https://cube.dev/docs/product/caching/matching-pre-aggregations)
- [Cube.dev source — `cube-js/cube` on GitHub](https://github.com/cube-js/cube) (Apache 2.0)
- [Snowflake semantic views overview](https://docs.snowflake.com/en/user-guide/views-semantic/overview)
- [Snowflake `SEMANTIC_VIEW` query syntax](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view)
- [egg: Fast and Extensible Equality Saturation](https://egraphs-good.github.io/) (Willsey et al., 2021)
- [Apache DataFusion](https://datafusion.apache.org/)
