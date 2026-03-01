# Feature Landscape: v0.2.0 New Features

**Domain:** DuckDB Rust extension — semantic layer preprocessor
**Researched:** 2026-02-28
**Milestone:** v0.2.0 — Native DDL + Time Dimensions
**Status:** Subsequent milestone research (v0.1.0 already shipped)

---

## Scope

This document covers only the four NEW features targeted for v0.2.0. All v0.1.0 features
(define_semantic_view DDL functions, semantic_query table function, SQL expansion engine,
sidecar persistence, fuzzy suggestions, identifier quoting) are already built and are not
re-researched here.

**Four new features under research:**
1. Native `CREATE SEMANTIC VIEW` DDL — parser hook integration
2. Time dimensions with granularity coarsening (day → week → month → year)
3. Native `EXPLAIN FROM semantic_query(...)` — shows expanded SQL
4. `pragma_query_t` catalog persistence — replaces sidecar file

---

## Table Stakes

Features that are expected by users. Missing = product feels incomplete.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| `CREATE SEMANTIC VIEW` DDL | SQL-native definition is the stated v0.2.0 goal; function-based DDL is a documented workaround. DuckDB users expect schema objects defined with DDL, not function calls. | High | Requires C++ shim for DuckDB parser extension hooks. The `duckdb-rs` C API does not expose `ParserExtension`. |
| Time dimension declaration in DDL | Time dimensions are universal across all reference systems (Snowflake, Databricks, Cube.dev, dbt). Without time support, analytical queries require manual `date_trunc` and GROUP BY. | Medium | Requires a type flag and granularity keyword in the DDL and query syntax. |
| Granularity coarsening at query time | Users expect `order_date AT MONTH` to produce `date_trunc('month', order_date)` and group by that truncated value. All four reference systems support this as standard. | Low-Medium | SQL generation is simple: map granularity keyword to `date_trunc` call. DuckDB handles the rest. |
| `DROP SEMANTIC VIEW` DDL | Complement to `CREATE`. Required for the DDL to be a complete schema object lifecycle. | Low | Already implemented as a function in v0.1.0; native DDL version is straightforward to add alongside `CREATE`. |
| `EXPLAIN FROM semantic_query(...)` | Power users want to verify what SQL the extension generates before running it. dbt MetricFlow exposes `--explain`; this is an analogue. The `explain_semantic_view()` workaround exists in v0.1.0 but native EXPLAIN integration is the expected behavior. | High | Requires C++ EXPLAIN hook to intercept EXPLAIN before DuckDB generates the physical plan. |
| Catalog persistence via DuckDB tables | `pragma_query_t` enables writing catalog entries directly to DuckDB tables (instead of a sidecar file), which is standard DuckDB extension practice. The sidecar file is a documented workaround that should be eliminated. | High | Requires C++ shim; `pragma_query_t` returns a SQL string DuckDB executes after the callback returns — during parsing before execution locks are held. |

### Confidence: HIGH
All four features are explicitly listed in the v0.1.0 TECH-DEBT.md as deferred requirements,
with root-cause analysis complete. The behavior requirements are well-understood.

---

## Differentiators

Features that distinguish this extension from comparable tools once v0.2.0 ships.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Typed output columns | `semantic_query()` currently returns all VARCHAR. Restoring proper types (BIGINT, DOUBLE, DATE) would improve composability — users don't need to CAST numeric metrics before arithmetic. | Medium | Requires type-specific vector writes in the FFI layer. Not blocked on C++ shim but benefits from it. |
| Granularity in define DDL (type annotation) | Declaring a column as `time` type in `CREATE SEMANTIC VIEW` DDL makes the interface self-documenting. Users see time dimension declarations in `DESCRIBE SEMANTIC VIEW`. | Low | Requires DDL grammar extension in the C++ shim. |
| First-class `WEEK` and `QUARTER` granularity | MetricFlow supports 8 granularities (second through year). DuckDB's `date_trunc` supports all standard intervals. Exposing WEEK and QUARTER in query syntax makes common reporting patterns first-class. | Low | Implementation: extend the granularity enum; map `WEEK` to `date_trunc('week', ...)` and `QUARTER` to `date_trunc('quarter', ...)`. |
| EXPLAIN shows expansion + DuckDB plan | If the C++ EXPLAIN hook can pass the expanded SQL to DuckDB's planner, users could see both the semantic expansion AND the physical plan in one EXPLAIN output. | High | May require two phases in the EXPLAIN hook: first show expanded SQL, then optionally show DuckDB physical plan. |

---

## Anti-Features

Features to deliberately NOT build in v0.2.0.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| YAML definition format | Two definition paths = two parsers, two validation paths, cognitive overhead. v0.1.0 established SQL DDL as the interface. YAML adds no value until the SQL DDL is stable. | SQL DDL only. If users want YAML, they can generate DDL SQL from YAML in a pre-processing step. |
| Custom time spine table | MetricFlow requires a dedicated date spine table to resolve period-over-period metrics. This adds DDL complexity and a required setup step. DuckDB's `generate_series` and `date_trunc` are sufficient for v0.2.0. | Use `date_trunc` directly in expansion. No time spine required for granularity coarsening. |
| Fiscal calendar / custom week start | Cube.dev supports custom granularities (fiscal quarter, fiscal year). This requires parameterized truncation logic and locale configuration. | Standard ISO calendar only (Sunday week start from `date_trunc('week')`). Fiscal calendar deferred to v0.3.0+. |
| Period-over-period metrics | Cumulative / running total metrics (dbt MetricFlow) require window functions or multi-level CTEs. Complex and orthogonal to the expansion engine. | Raw SQL in outer query. Users can WINDOW over semantic_query results. |
| Pre-aggregation DDL | Materialization selection is v0.3.0+. Adding it to v0.2.0 would double scope. | Continue expansion-only approach. DuckDB handles performance. |
| Community extension registry publication | Publishing requires upstream PR to `duckdb/community-extensions`. Deferred to v0.3.0 when native DDL is stable and TPC-H demo exists. | Internal use and GitHub release only. |
| Derived metrics (metric-on-metric) | Requires two sub-aggregations composed in SQL. Out of scope since v0.1.0. | Users compose metrics in outer SQL. |
| Multi-hop join resolution | Graph traversal with cycle detection. Deferred since v0.1.0. | Users define a SQL view pre-joining tables and reference it as the base table. |

---

## Feature Dependency Graph

```
[C++ shim]
  ├── DDL-1: CREATE SEMANTIC VIEW syntax
  │     └── DDL-2: DROP SEMANTIC VIEW syntax
  ├── DDL-3: pragma_query_t catalog persistence
  │     └── (replaces sidecar file; DDL-1 depends on this to store definitions)
  └── DDL-4: EXPLAIN hook for semantic_query

DDL-1 (CREATE DDL)
  └── TIME-1: time dimension type declaration in DDL
        └── TIME-2: granularity keyword in semantic_query call
              └── TIME-3: date_trunc expansion in SQL generator
                    └── TIME-4: WEEK / QUARTER granularity variants

TIME-3 → (existing expansion engine in v0.1.0) → correct GROUP BY

DDL-4 (EXPLAIN hook)
  └── (existing expansion engine in v0.1.0) → expanded SQL string to show
```

**Critical path:** C++ shim → `pragma_query_t` → `CREATE SEMANTIC VIEW` parser hook →
time dimension DDL syntax → granularity expansion → EXPLAIN hook.

The C++ shim is a prerequisite for all four v0.2.0 features. It is the foundational
work that unblocks everything else.

---

## Expected User-Facing Behavior (Concrete Examples)

### Feature 1: Native CREATE SEMANTIC VIEW DDL

**What users write today (v0.1.0 workaround):**
```sql
SELECT define_semantic_view('sales_analysis', '{
  "base_table": "orders",
  "dimensions": [...],
  "metrics": [...]
}');
```

**What users write in v0.2.0:**
```sql
CREATE SEMANTIC VIEW sales_analysis AS
  BASE TABLE orders
  JOIN customers ON orders.customer_id = customers.id
  DIMENSIONS (
    orders.region,
    customers.segment,
    orders.order_date TIME
  )
  METRICS (
    SUM(orders.revenue) AS total_revenue,
    COUNT(orders.id) AS order_count
  )
  FILTERS (
    orders.status = 'complete'
  );
```

**Design decisions reflected in this syntax:**
- `TIME` keyword after a dimension declares it as a time dimension (distinct from regular dimensions)
- `BASE TABLE` and `JOIN` follow existing v0.1.0 mental model
- `FILTERS` replaces `row_filter` JSON field
- `AS aggregation_func(column) AS alias` follows SQL convention
- `CREATE OR REPLACE` and `IF NOT EXISTS` modifiers expected (standard DuckDB DDL pattern)
- `DROP SEMANTIC VIEW name` is the complement

**How Snowflake does it (reference):**
Snowflake uses `TABLES(alias AS table_name PRIMARY KEY(...))`, `RELATIONSHIPS(...)`,
`DIMENSIONS(table.name AS expression)`, `METRICS(table.name AS expression)`. There is
no special syntax to declare a column as a time dimension — time columns are just used
with scalar functions (YEAR, DATE_TRUNC) in dimension expressions. DuckDB's extension
can be more explicit with a `TIME` type marker.

**Confidence:** MEDIUM. The exact grammar is not yet designed. The behavioral contract
is clear (native DDL, same semantics as function-based DDL). Grammar choices require
design work during the milestone.

---

### Feature 2: Time Dimensions with Granularity Coarsening

**What users write (query):**
```sql
-- Day granularity (default for a time dimension)
SELECT * FROM semantic_query('sales_analysis',
  dimensions := ['order_date'],
  metrics := ['total_revenue']
);

-- Month granularity (coarsening)
SELECT * FROM semantic_query('sales_analysis',
  dimensions := ['order_date:month'],
  metrics := ['total_revenue']
);

-- Week granularity
SELECT * FROM semantic_query('sales_analysis',
  dimensions := ['order_date:week'],
  metrics := ['total_revenue']
);
```

**What the expansion engine generates:**

For `order_date:month`:
```sql
WITH _base AS (SELECT orders.*, customers.* FROM orders JOIN customers ...)
SELECT
  date_trunc('month', order_date) AS order_date,
  SUM(revenue) AS total_revenue
FROM _base
WHERE status = 'complete'
GROUP BY date_trunc('month', order_date)
ORDER BY date_trunc('month', order_date)
```

**Supported granularities (v0.2.0):**
| Keyword | DuckDB SQL | Notes |
|---------|------------|-------|
| `day` | `date_trunc('day', col)` | Default if no granularity specified |
| `week` | `date_trunc('week', col)` | ISO week (Monday start in DuckDB) |
| `month` | `date_trunc('month', col)` | First day of month |
| `quarter` | `date_trunc('quarter', col)` | First day of quarter |
| `year` | `date_trunc('year', col)` | January 1 |

**What "coarsening" means:** A time dimension declared in a semantic view definition has
a natural granularity (the column is a DATE or TIMESTAMP). Requesting it at a coarser
granularity (e.g., `month` for a daily order_date) truncates the value and groups by the
truncated value. Requesting a finer granularity than the column's type supports is an
error (e.g., requesting `HOUR` on a DATE column). The expansion engine should validate
this at query time.

**Coarsening is NOT re-aggregation from a materialized table** (that is pre-aggregation,
v0.3.0+). In v0.2.0, `date_trunc` is applied to the raw column and DuckDB groups by it.

**How all reference systems do it:**
- Snowflake: no explicit granularity keyword; users write `date_trunc()` or `YEAR()` in dimension expressions
- dbt MetricFlow: `time_dimension_name__granularity` naming convention; maps to `DATE_TRUNC('month', col)` in generated SQL
- Cube.dev: `granularity` parameter in query JSON; maps to `date_trunc` for DuckDB driver
- Databricks: `GRAIN` keyword in custom SQL extension syntax

The `:granularity` suffix on dimension names (e.g., `order_date:month`) is a DuckDB-native design that fits the existing `dimensions := [...]` array syntax without requiring a separate GRAIN parameter.

**Confidence:** HIGH. `date_trunc` mapping is standard SQL. The v0.1.0 expansion engine
already emits the GROUP BY — adding `date_trunc` wrapping is a localized change.

---

### Feature 3: Native EXPLAIN from semantic_query

**What users write:**
```sql
EXPLAIN SELECT * FROM semantic_query('sales_analysis',
  dimensions := ['region', 'order_date:month'],
  metrics := ['total_revenue']
);
```

**What they see today (v0.1.0):**
DuckDB's physical query plan for the `semantic_query` table function internals — not
the expanded SQL. The workaround is `explain_semantic_view()`.

**What they should see in v0.2.0 (two design options):**

*Option A: Show expanded SQL only (simpler)*
```
Expanded SQL for semantic_query('sales_analysis'):
────────────────────────────────────────────────
WITH _base AS (
  SELECT orders.*, customers.*
  FROM orders
  JOIN customers ON orders.customer_id = customers.id
  WHERE orders.status = 'complete'
)
SELECT
  region AS region,
  date_trunc('month', order_date) AS order_date,
  SUM(revenue) AS total_revenue
FROM _base
GROUP BY region, date_trunc('month', order_date)
ORDER BY region, date_trunc('month', order_date)
```

*Option B: Show expanded SQL + then DuckDB physical plan for that SQL (richer)*
Show the expanded SQL, then run EXPLAIN on it and show DuckDB's physical plan.

**How the EXPLAIN hook works technically:**
DuckDB's C++ API exposes an EXPLAIN hook that extensions can register. When the user runs
`EXPLAIN`, the hook fires before DuckDB generates the physical plan. The extension can
intercept the `semantic_query()` table function reference, run the expansion, and return
the expanded SQL string as the EXPLAIN output.

The EXPLAIN hook is exposed via the C++ API but NOT through the Rust `duckdb-rs` C API
wrapper. This is why it requires a C++ shim — the same shim being built for native DDL.

**Preferred design:** Option A for v0.2.0. Option B adds complexity (requires running
EXPLAIN on the expanded SQL and merging outputs). Option A unblocks the primary use case:
"show me what SQL the extension will run" for debugging and trust-building.

**Confidence:** MEDIUM. The EXPLAIN hook mechanism exists in DuckDB's C++ API. Exact
function signatures and integration patterns need to be verified against the DuckDB
source at the time of implementation. LOW confidence on Option B feasibility in v0.2.0.

---

### Feature 4: pragma_query_t Catalog Persistence

**What it replaces:**
The v0.1.0 sidecar file approach: catalog writes go to `<db>.semantic_views` via plain
file I/O, then sync into `semantic_layer._definitions` on next extension load.

**What it enables:**
The `pragma_query_t` pattern (used by DuckDB's FTS extension) allows an extension to
return a SQL string from a PRAGMA callback. DuckDB executes that SQL string AFTER the
callback returns, during the parsing phase before execution locks are held. This means:

1. `CREATE SEMANTIC VIEW` DDL calls a PRAGMA-backed handler
2. The handler returns a SQL `INSERT INTO semantic_layer._definitions VALUES (...)` string
3. DuckDB executes that SQL with its normal query execution (not inside the extension callback)
4. No execution locks conflict with the extension callback
5. Catalog writes participate in DuckDB's normal transaction semantics

**What users observe:**
No behavioral change — catalog persistence is invisible to users. The improvement is:
- Definitions written to `semantic_layer._definitions` in the same DuckDB transaction
  as the `CREATE SEMANTIC VIEW` statement (not on next extension load)
- No sidecar file on disk (simpler installation, no separate file to manage)
- Multi-connection access works correctly (two connections to same DuckDB file see
  the same catalog because it's in DuckDB's own catalog tables)
- Rollback of a `CREATE SEMANTIC VIEW` (e.g., in an aborted transaction) correctly
  removes the definition

**How the FTS extension does this (confirmed):**
When `PRAGMA create_fts_index(...)` is called, `FTSIndexing::CreateFTSIndexQuery()`
generates a complete SQL script creating the FTS schema and tables. DuckDB executes that
returned SQL through its normal execution path. The extension never executes SQL directly
inside the callback — it only generates SQL strings.

This pattern is the correct solution to the DuckDB extension execution lock problem that
the v0.1.0 sidecar file was working around.

**v0.2.0 behavior for `CREATE SEMANTIC VIEW`:**
```
1. C++ shim intercepts CREATE SEMANTIC VIEW ... statement via ParserExtension
2. Parses the DDL into a SemanticViewDefinition struct (C++ or Rust)
3. Serializes the definition to JSON
4. Returns SQL string: INSERT INTO semantic_layer._definitions (name, definition)
                       VALUES ('view_name', '<json>') ON CONFLICT REPLACE
5. DuckDB executes the INSERT
6. Definition is now in the catalog table, persisted via DuckDB's WAL
```

**Confidence:** MEDIUM. The `pragma_query_t` pattern is confirmed in the FTS extension.
Applying it to a custom DDL statement (rather than a PRAGMA) requires the ParserExtension
hook to also support returning SQL for DuckDB to execute. This exact flow needs
verification in DuckDB's extension source code during implementation.

---

## MVP Recommendation for v0.2.0

**Build in this order:**

1. **C++ shim infrastructure** — This is the prerequisite for everything. Set up the CMake
   build that compiles a thin C++ shim alongside the Rust extension. The shim registers
   a `ParserExtension` with DuckDB's C++ API and exposes a C ABI for the Rust code to call.

2. **`pragma_query_t` catalog persistence** — Implement first because all subsequent DDL
   features depend on a working catalog write mechanism. Verify the pattern eliminates the
   sidecar file.

3. **`CREATE SEMANTIC VIEW` parser hook** — Wire the C++ `parse_function` to parse the
   new DDL syntax and the `plan_function` to generate the catalog INSERT via `pragma_query_t`.
   Start with the existing JSON definition format internally.

4. **Time dimension DDL syntax** — Add the `TIME` keyword to the DDL grammar. Extend
   the internal definition model to track which dimensions are time dimensions.

5. **Granularity coarsening in expansion** — Extend the v0.1.0 expansion engine to
   recognize `dimension:granularity` syntax in `dimensions := [...]` arrays and emit
   `date_trunc(granularity, column)` in the SELECT and GROUP BY.

6. **EXPLAIN hook** — Add last; depends on working expansion. The C++ shim registers an
   EXPLAIN callback that intercepts `EXPLAIN FROM semantic_query(...)` and returns the
   expanded SQL string.

**Defer to v0.3.0:**
- Typed output columns (useful but not C++ shim dependent; can be separate Rust work)
- QUARTER granularity (add alongside WEEK; low-risk addition)
- Community extension registry publication

---

## Sources

- Snowflake CREATE SEMANTIC VIEW documentation: https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view
- Snowflake SEMANTIC_VIEW query syntax: https://docs.snowflake.com/en/sql-reference/constructs/semantic_view
- Snowflake semantic view DDL examples: https://docs.snowflake.com/en/user-guide/views-semantic/sql
- dbt MetricFlow time dimensions: https://deepwiki.com/dbt-labs/metricflow/3.2-time-dimensions-and-granularities
- DuckDB FTS extension pragma/SQL-generation pattern: https://deepwiki.com/duckdb/duckdb-fts/2.1-creating-fts-indexes
- DuckDB runtime-extensible parsers (2024 research): https://duckdb.org/2024/11/22/runtime-extensible-parsers
- DuckDB extension deadlock analysis: https://dev.to/nk_maker/fixing-duckdb-extension-deadlock-from-query-strings-to-list-types-10c2
- v0.1.0 TECH-DEBT.md: accepted decisions 1 (sidecar), 5 (EXPLAIN deferred), and deferred requirements table
- v0.1.0 _notes/semantic-views-duckdb-design-doc.md: prior art analysis and design principles
- v0.1.0 FEATURES.md (greenfield research, 2026-02-23): cross-system feature matrix and classification
