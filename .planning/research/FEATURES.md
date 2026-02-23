# Feature Analysis: Semantic Layer Products

**Research type:** Project Research — Features dimension
**Milestone:** Greenfield — What features do semantic layer / semantic view products have?
**Date:** 2026-02-23
**Status:** Draft

---

## Research Question

What features do semantic layer products have across Snowflake SEMANTIC_VIEW, Databricks metric views, Cube.dev, and dbt/MetricFlow? For each feature, what is table stakes vs. differentiator vs. anti-feature for a DuckDB extension specifically?

---

## Scope Note

This analysis is grounded in the four systems as of early 2026:

- **Snowflake SEMANTIC_VIEW** — SQL-native DDL (`CREATE SEMANTIC VIEW`) with a `SEMANTIC_VIEW(...)` table function query syntax; launched 2024.
- **Databricks Metric Views** — YAML-defined semantic models registered in Unity Catalog; available 2024–2025.
- **Cube.dev** — The most mature OSS semantic layer; YAML/JS data model definitions with its own HTTP/SQL API, pre-aggregation engine, and Cube Store.
- **dbt Semantic Layer / MetricFlow** — YAML metric definitions compiled by MetricFlow into SQL; query via `mf query` CLI or dbt Cloud semantic layer API.

---

## System-by-System Feature Survey

### 1. Snowflake SEMANTIC_VIEW

**Definition mechanism:** SQL DDL. `CREATE SEMANTIC VIEW name AS ...` with blocks for logical tables, dimensions, facts, metrics, and relationships.

**Key features:**

| Feature | Description |
|---|---|
| Logical tables | Named table abstractions that map to physical tables/views; alias physical columns |
| Dimensions | Named column expressions attached to a logical table; can be SQL expressions, not just column references |
| Facts | Numeric column references from a fact/event table; the raw input to metrics |
| Metrics | Named aggregation expressions (SUM, COUNT, AVG, COUNT DISTINCT, custom SQL expressions); defined over facts |
| Relationships | Declared foreign-key style joins between logical tables; engine infers JOINs from these at query time |
| Time dimensions | Timestamp/date columns tagged as time dimensions; support granularity coarsening in queries |
| Row-level filters | Predicates attached to a logical table that apply at query time (e.g., `status = 'active'`) |
| Query syntax | `SEMANTIC_VIEW(view_name DIMENSIONS ... METRICS ... WHERE ...)` table function; composable with outer SQL |
| Granularity validation | Compile-time error if requested dimensions and metrics have incompatible entity granularity |
| Schema object | Semantic view is a first-class schema object; visible in SHOW, DESCRIBE, information schema |
| SQL composability | The SEMANTIC_VIEW clause returns a derived table; can be JOINed, CTEd, PIVOTed in outer SQL |
| No pre-aggregation | No materialized rollup layer; Snowflake's query engine handles performance |
| Column-level security | Inherits from underlying tables; dimension/metric visibility controlled via row access policies |

**What it does NOT have:** YAML interface, pre-aggregation selection, cross-database portability, derived metrics (metric-on-metric), hierarchies, percentile/HLL approximate metrics.

---

### 2. Databricks Metric Views

**Definition mechanism:** YAML files registered via `CREATE METRIC VIEW` DDL backed by Unity Catalog. Definitions can also be expressed in the Databricks UI.

**Key features:**

| Feature | Description |
|---|---|
| Entities | Named entity types (customer, order) that anchor dimensions and metrics |
| Dimensions | Named column expressions (scalar SQL) attached to an entity; join-aware |
| Measures / metrics | Aggregation expressions (SUM, COUNT, AVG, COUNT DISTINCT, ratio metrics) |
| Ratio metrics | First-class support for `numerator / denominator` metrics as a type |
| Derived metrics | Metric arithmetic (e.g., profit = revenue - cost) |
| Relationships | Declared entity relationships driving JOIN inference |
| Time dimensions | Date/timestamp columns with granularity support (DAY, WEEK, MONTH, QUARTER, YEAR) |
| Filters | Row-level filter expressions on entities |
| Query syntax | `SELECT ... FROM <metric_view> METRIC ... DIM ... GRAIN ...` SQL extension |
| Unity Catalog integration | Metric views are catalog objects; subject to Unity Catalog lineage, permissions, and governance |
| Python/REST API | Query via REST or Python SDK, not just SQL |
| AI/BI Genie integration | Metric views feed into Databricks Genie (NL-to-SQL assistant) |
| Semantic search | Unity Catalog tags and descriptions enable semantic discovery |

**What it does NOT have:** Pre-aggregation selection, YAML portability (definitions are Databricks-native), SQL-only DDL interface, open source implementation.

---

### 3. Cube.dev

**Definition mechanism:** YAML (or JS) data model files (`cubes:`, `views:`, `measures:`, `dimensions:`, `pre_aggregations:` blocks). Compiled by a Node.js schema compiler.

**Key features:**

| Feature | Description |
|---|---|
| Cubes | Named model objects mapping to tables/views; the central abstraction |
| Views | Named projections over cubes (curate member subsets for consumers) |
| Dimensions | Column or SQL expression dimensions; types: string, number, time, boolean, geo |
| Measures | Aggregation expressions: count, sum, avg, min, max, count_distinct, count_distinct_approx, number (custom SQL), running_total, cumulative_sum |
| Ratio / calculated measures | `number` type measure with custom SQL expression doing division or arithmetic |
| Derived metrics (views) | Views can compose measures from multiple cubes |
| Relationships | `one_to_many`, `many_to_one`, `one_to_one` join declarations; determines which cube owns the relationship |
| Time dimensions | `time` type dimensions; queries specify `granularity` (second, minute, hour, day, week, month, quarter, year) |
| Filters / segments | Named boolean filter predicates (`segments`); row-level security via `queryRewrite` |
| Pre-aggregations | The flagship feature: declare rollup tables (dimensions + measures + granularity) and Cube's engine routes queries to matching rollups automatically |
| Additive measure validation | Non-additive measures (count_distinct) cannot be served from pre-aggregations; falls through to raw |
| Granularity GCD | A daily rollup can serve weekly/monthly queries (re-aggregation wrapper applied) |
| Multi-stage measures | Tesseract planner supports nested aggregations (e.g., avg of daily totals), period-over-period |
| Cube Store | Custom columnar store (Parquet + Arrow + DataFusion) for storing and serving pre-aggregated data |
| HTTP API | REST + GraphQL API for querying the semantic layer; used by BI tool integrations |
| SQL API (CubeSQL) | Postgres-wire-protocol SQL interface; BI tools connect as if Cube is a Postgres database |
| Metadata API | Endpoint returning available cubes, views, measures, dimensions — used by BI tools to build UIs |
| Multi-tenancy | `queryRewrite` hooks for per-tenant filter injection; context variables |
| Caching layer | In-memory query result cache in addition to pre-aggregations |
| `egg`-based SQL rewriting | Normalizes incoming SQL from diverse BI tools into internal CubeScan plan nodes |
| Refresh scheduler | Background jobs to build and refresh pre-aggregated rollup tables |
| Deployment modes | Self-hosted (Docker), Cube Cloud (managed), embedded |

**What it does NOT have:** Native DuckDB support (DuckDB driver exists but Cube Store is separate), SQL DDL definition format, tight database integration (it is middleware not an in-database feature).

---

### 4. dbt Semantic Layer / MetricFlow

**Definition mechanism:** YAML files in a dbt project (`semantic_models:`, `metrics:` blocks). MetricFlow compiles these definitions into SQL. Queried via `mf query` CLI, dbt Cloud Semantic Layer API, or JDBC/ADBC.

**Key features:**

| Feature | Description |
|---|---|
| Semantic models | Named objects mapping to a dbt model (table/view); declare entities, dimensions, measures |
| Entities | Named keys (primary, foreign, natural, unique) that anchor join relationships |
| Dimensions | Categorical (`type: categorical`) or time (`type: time`) column/SQL-expression dimensions |
| Measures | Named aggregation expressions: sum, max, min, count, count_distinct, sum_boolean, average, percentile (experimental) |
| Metrics | Named query-able aggregations derived from measures: simple, ratio, derived, cumulative, conversion |
| Derived metrics | Metric-on-metric arithmetic (e.g., profit = revenue - cost) |
| Ratio metrics | First-class numerator/denominator metric type |
| Cumulative metrics | Running totals / window-based accumulation |
| Conversion metrics | Funnel / event-pairing metrics (experimental) |
| Time spine | A dedicated date spine table; enables period-over-period, offset windows, grain alignment |
| Time granularity | DAY, WEEK, MONTH, QUARTER, YEAR; queries specify `grain` |
| WHERE filters | Dimension-based filter predicates in queries |
| Saved queries | Pre-defined query configurations (dimensions + metrics + filters) stored in YAML |
| Exports (saved query materialization) | A saved query can be materialized as a dbt model (table/view) |
| Multi-hop joins | MetricFlow resolves multi-level entity relationships (A joins B joins C) |
| BI tool integrations | Semantic layer API consumed by Tableau, Looker Studio, Power BI, Hex, etc. |
| `mf query` CLI | Direct metric querying from the command line |
| `mf validate-configs` | Compile-time validation of semantic model YAML |
| Lineage integration | Metrics traced back to source columns via dbt's DAG |
| Column-level governance | Inherits from dbt model column-level descriptions and tags |

**What it does NOT have:** In-database DDL syntax, pre-aggregation/materialization selection (only static exports), real-time serving layer (requires dbt Cloud for the API).

---

## Cross-System Feature Matrix

| Feature | Snowflake | Databricks | Cube.dev | dbt/MetricFlow | Notes |
|---|---|---|---|---|---|
| Dimension definitions | Yes | Yes | Yes | Yes | Universal |
| Measure/metric definitions | Yes | Yes | Yes | Yes | Universal |
| Simple aggregations (SUM, COUNT, AVG, MIN, MAX) | Yes | Yes | Yes | Yes | Universal |
| COUNT DISTINCT | Yes | Yes | Yes | Yes | Universal |
| Relationship/join declarations | Yes | Yes | Yes | Yes | Universal |
| JOIN inference from relationships | Yes | Yes | Yes | Yes | Universal |
| Time dimensions | Yes | Yes | Yes | Yes | Universal |
| Granularity coarsening (day → month) | Yes | Yes | Yes | Yes | Universal |
| Row-level filters / segments | Yes | Yes | Yes | Yes | Universal |
| Compile-time / expansion-time validation | Yes | Yes | Yes | Yes | Universal |
| SQL DDL definition format | Yes | Partial | No | No | Snowflake-native advantage for DuckDB |
| YAML definition format | No | Yes | Yes | Yes | Common outside Snowflake |
| Ratio metrics (numerator/denominator) | No | Yes | Yes | Yes | Databricks/dbt/Cube have it |
| Derived metrics (metric-on-metric) | No | Yes | Yes | Yes | Absent from Snowflake |
| Hierarchies (drill-down paths) | No | No | No | No | Rare; mostly Looker |
| Pre-aggregation / rollup selection | No | No | Yes | No (exports only) | Cube.dev distinguishing feature |
| Granularity GCD re-aggregation | No | No | Yes | No | Tied to pre-aggregation |
| Multi-stage / nested aggregations | No | No | Yes (Tesseract) | Cumulative only | Advanced; Cube Tesseract |
| Cumulative / running total metrics | No | No | Yes | Yes | dbt + Cube |
| Conversion / funnel metrics | No | No | No | Yes (experimental) | dbt experimental |
| Multi-hop join resolution | No | No | Yes | Yes | dbt + Cube; complex |
| SQL composability (outer queries) | Yes | Partial | No (API-based) | No | Snowflake advantage |
| Schema object (SHOW, DESCRIBE) | Yes | Yes | No | No | DB-native systems |
| BI tool metadata API | No | No | Yes | Yes | Middleware systems |
| Query API (HTTP/REST) | No | Yes | Yes | Yes | Not relevant for DuckDB ext |
| In-process / embedded usage | No | No | No | No | DuckDB's key advantage |
| Column-level security | Yes | Yes | No | No | DB-native systems |
| Saved / named queries | No | No | No | Yes | dbt exports |

---

## Feature Classification for DuckDB Extension v0.1

### Methodology

Features are classified relative to a DuckDB extension specifically:

- **Table stakes** — users expect this; absence makes the product non-functional or confusing
- **Differentiator** — competitive advantage for DuckDB's use case (local, embedded, developer-focused)
- **Anti-feature** — deliberately out of scope in v0.1; adds complexity without proportionate v0.1 value

---

### TABLE STAKES — Must Have

These features are present in all four reference systems and represent the minimum viable semantic layer. Absence makes the extension not worth using.

---

#### TS-1: Dimension definitions with SQL expressions

**Description:** Named dimensions attached to a base table or joined table. A dimension maps a name to a SQL expression (column reference or derived expression like `UPPER(first_name) || ' ' || last_name`).

**Why table stakes:** Every semantic layer has this. Without it, the "semantic" in semantic views means nothing — there's no abstraction over physical columns.

**Present in:** Snowflake, Databricks, Cube.dev, dbt/MetricFlow

**Complexity:** Low. Storing name → SQL expression pairs; emitting them in the SELECT list during expansion.

**Dependencies:** None. This is the primitive on which everything else builds.

---

#### TS-2: Metric/measure definitions with aggregation type

**Description:** Named metrics defined by an aggregation function and a source column or expression. At minimum: SUM, COUNT, AVG, MIN, MAX. COUNT DISTINCT is also present in all four systems.

**Why table stakes:** Core purpose of a semantic layer. Without metrics, it is just a view aliasing system.

**Present in:** Snowflake, Databricks, Cube.dev, dbt/MetricFlow

**Complexity:** Low for simple aggregations. COUNT DISTINCT is non-additive (impacts pre-aggregation matching, but that's v0.2). For v0.1, just emit `COUNT(DISTINCT col)` — DuckDB handles it.

**Dependencies:** TS-1 (metrics reference fact columns, which are in the same dimension/table namespace).

---

#### TS-3: Relationship / join declarations

**Description:** Declared relationships between logical tables (foreign-key semantics). At expansion time, the extension infers which JOINs to emit based on which dimensions and metrics are requested.

**Why table stakes:** Multi-table semantic models are the primary use case. A single-table semantic view is just a named view. The join inference is what the extension actually does for users — they should never write GROUP BY or JOIN by hand once a semantic view is defined.

**Present in:** Snowflake, Databricks, Cube.dev, dbt/MetricFlow

**Complexity:** Medium. Need to decide which relationships to activate (only those connecting requested dimensions to the fact table), handle join ordering, avoid Cartesian products. For v0.1, limit to single-level (A joins B), not multi-hop (A joins B joins C).

**Dependencies:** TS-1, TS-2.

---

#### TS-4: Automatic GROUP BY / aggregation in query expansion

**Description:** When a user requests dimensions and metrics from a semantic view, the expansion emits a concrete SQL query with the correct GROUP BY clause. The user never writes GROUP BY.

**Why table stakes:** This is the core value proposition stated in the project brief. If the extension doesn't handle GROUP BY inference, there is no reason to use it over a regular view.

**Present in:** Snowflake, Databricks, Cube.dev, dbt/MetricFlow

**Complexity:** Low once TS-1, TS-2, TS-3 are implemented. GROUP BY = all requested dimensions. SELECT = requested dimensions + requested metrics.

**Dependencies:** TS-1, TS-2, TS-3.

---

#### TS-5: SQL DDL definition syntax (`CREATE SEMANTIC VIEW`)

**Description:** The semantic view definition is expressed in SQL DDL, not YAML. The extension registers a `CREATE SEMANTIC VIEW` statement that DuckDB parses and stores.

**Why table stakes for DuckDB specifically:** DuckDB users work in SQL. YAML is a foreign interface that requires a separate toolchain. Every DuckDB extension uses SQL DDL for schema objects. Violating this would make the extension feel alien. Additionally, Snowflake's DDL approach is the closest prior art and aligns with the project's stated design decision.

**Present in:** Snowflake (SQL DDL); Databricks (partial — YAML backed by DDL); dbt, Cube (YAML only).

**Complexity:** Medium-high. Requires hooking into DuckDB's parser extension mechanism. Must parse and store the semantic view definition. But this is fundamental to the extension's identity.

**Dependencies:** None from the semantic model side; depends on DuckDB's parser extension API.

---

#### TS-6: Table function query syntax (`FROM view_name(DIMENSIONS ... METRICS ...)`)

**Description:** Users query a semantic view with an explicit dimension and metric selection syntax. The extension intercepts this, expands it to SQL, and returns results.

**Why table stakes:** The query interface is what users interact with daily. It must be natural, predictable, and unambiguous about what is being requested.

**Present in:** Snowflake uses `SEMANTIC_VIEW(view_name DIMENSIONS ... METRICS ...)`. The project has flexibility on exact syntax; the key property is explicit dimension + metric selection rather than `SELECT *`.

**Complexity:** Medium. Requires DuckDB parser extension hooks to intercept the custom syntax, or use DuckDB's table function mechanism with structured arguments.

**Dependencies:** TS-5 (needs stored definitions to look up).

---

#### TS-7: Time dimensions with granularity

**Description:** Columns declared as time dimensions support granularity coarsening at query time. Requesting `order_date` at `MONTH` granularity emits `date_trunc('month', order_date)` and groups by it.

**Why table stakes:** Analytical queries are almost always time-sliced. All four reference systems support this. Absence forces users to write `date_trunc` manually, defeating the purpose.

**Present in:** Snowflake, Databricks, Cube.dev, dbt/MetricFlow

**Complexity:** Low-medium. Storing the time dimension type flag, mapping granularity keywords to `date_trunc` calls, including truncated column in GROUP BY.

**Dependencies:** TS-1 (time dimensions are a subtype of dimension), TS-4 (granularity affects GROUP BY).

---

#### TS-8: Expansion-time validation with clear errors

**Description:** The extension validates dimension-metric combinations at query time (before SQL is emitted) and produces actionable error messages for invalid combinations. Examples: requesting a dimension not defined in the view; requesting a metric from a disconnected table with no join path; incompatible entity granularity.

**Why table stakes:** Without validation, invalid queries either return incorrect results silently or produce cryptic SQL errors from DuckDB. Snowflake explicitly validates granularity compatibility at compile time. Good DX requires this.

**Present in:** Snowflake, Databricks (partially), Cube.dev (partially), dbt MetricFlow (compile-time YAML validation).

**Complexity:** Medium. Need to implement the validation logic: dimension membership check, join path reachability, granularity compatibility.

**Dependencies:** TS-1, TS-2, TS-3, TS-5, TS-6.

---

#### TS-9: Row-level filter predicates on tables

**Description:** A semantic view definition can include filter predicates that always apply when that table is scanned. Equivalent to Cube's `segments` or Snowflake's table-level filters. Example: `WHERE deleted_at IS NULL` always applied to the users table.

**Why table stakes:** Almost all production datasets have soft-delete flags, status columns, or tenant partitions. Without this, users must re-specify the filter in every query, defeating semantic encapsulation. All four systems support it.

**Present in:** Snowflake, Databricks, Cube.dev, dbt/MetricFlow

**Complexity:** Low. Store filter expressions per table; append to WHERE clause in expansion.

**Dependencies:** TS-1, TS-4.

---

#### TS-10: Persistence of semantic view definitions

**Description:** `CREATE SEMANTIC VIEW` stores the definition in DuckDB's catalog (or the extension's catalog) such that it survives across sessions. `DROP SEMANTIC VIEW` removes it.

**Why table stakes:** A semantic view that disappears when the DuckDB connection closes is useless in practice. Schema objects must persist.

**Present in:** Snowflake, Databricks (Unity Catalog), Cube.dev (data model files on disk), dbt (YAML files in project).

**Complexity:** Medium. DuckDB's extension API must be used to persist definitions, either in DuckDB's catalog mechanism or in a DuckDB table in the extension's own schema. The former is preferable but depends on what the extension API exposes.

**Dependencies:** TS-5.

---

### DIFFERENTIATORS — Competitive Advantage for DuckDB

These features are not universally present or are implemented differently, and have particular value in DuckDB's use case (local, embedded, analytics, developer-focused).

---

#### D-1: In-process / embedded operation (no server required)

**Description:** The semantic layer runs inside the DuckDB process. No HTTP API, no separate service, no Docker container. Works in DuckDB CLI, Python, R, Node.js, anywhere DuckDB runs.

**Why differentiating:** Cube.dev requires a Node.js server. dbt Semantic Layer requires dbt Cloud or a local dbt-core process. Snowflake and Databricks are cloud services. DuckDB is the only major analytics system that runs fully in-process. A semantic view extension inherits this property automatically — and it is a significant advantage for local development, testing, and embedded analytics.

**Present in:** None of the reference systems (they all require separate processes or services).

**Complexity:** Free — it is the natural consequence of being a DuckDB extension.

**Dependencies:** None.

---

#### D-2: SQL-native definition (no YAML toolchain)

**Description:** The entire semantic view lifecycle — define, query, drop — happens in SQL. No YAML files, no separate compiler, no CLI commands to sync definitions.

**Why differentiating:** dbt and Cube require YAML + a compilation step. Databricks requires YAML backed by a catalog registration step. Only Snowflake shares the SQL-native approach. For DuckDB users who want to define semantic models in a single `.sql` file or a Jupyter notebook cell, SQL DDL is far more accessible.

**Present in:** Snowflake (SQL DDL).

**Complexity:** Already captured in TS-5. The differentiating value comes from the combination: SQL DDL + embedded + DuckDB.

**Dependencies:** TS-5.

---

#### D-3: Composability with arbitrary SQL (outer query wrapping)

**Description:** The semantic view clause returns a derived table / subquery. Users can JOIN it to other tables, use it in CTEs, PIVOT it, apply HAVING, WINDOW functions, etc. in the outer query.

**Why differentiating:** Cube.dev's API-based model does not support this natively — users query via REST or the SQL API which has a constrained query model. dbt's semantic layer is also primarily API-based. Snowflake supports this explicitly, calling it out as a design goal. For DuckDB users who want to integrate semantic results into larger analytical pipelines, full SQL composability is essential and natural.

**Present in:** Snowflake explicitly; partially possible in Databricks SQL.

**Complexity:** Low implementation cost — it is a consequence of expansion to subquery. The validation (TS-8) must ensure expansion produces a well-formed subquery that can act as a derived table.

**Dependencies:** TS-4, TS-6, TS-8.

---

#### D-4: Works with local files (Parquet, CSV, DuckDB tables)

**Description:** The semantic view definition can reference physical tables that are DuckDB tables, CSV files, Parquet files, or any other DuckDB data source.

**Why differentiating:** DuckDB's superpower is querying local files directly. A semantic layer that only works with database tables misses this. If a user has `sales.parquet` and `customers.parquet`, they should be able to define a semantic view over them without first loading them into a database.

**Present in:** None of the cloud reference systems (they require registered catalog objects). Cube.dev with DuckDB driver is the closest but still requires Cube infra.

**Complexity:** Low — DuckDB already handles this. The extension just needs to reference the table name/path in the semantic view definition and DuckDB resolves it.

**Dependencies:** TS-5.

---

#### D-5: Zero-dependency installation (DuckDB community extension)

**Description:** Users install the extension with `INSTALL semantic_views FROM community; LOAD semantic_views;` — one SQL command. No npm, no pip, no YAML toolchain, no sidecar process.

**Why differentiating:** Every other semantic layer has a non-trivial installation story. Cube.dev is a Node.js server. dbt requires pip + project setup. Snowflake and Databricks are cloud-only. The DuckDB community extension registry makes this achievable and it is a genuine competitive moat.

**Present in:** None of the reference systems.

**Complexity:** Medium (building the extension in a way that passes community extension requirements and CI), but the developer experience payoff is very high.

**Dependencies:** All other features must be implemented before this can be packaged.

---

#### D-6: Deterministic, inspectable SQL expansion

**Description:** The expanded SQL that the extension generates is inspectable. An `EXPLAIN SEMANTIC VIEW` or similar mechanism shows users the concrete SQL that will be executed for a given query.

**Why differentiating:** Cube.dev, dbt MetricFlow, and Databricks all generate SQL behind the scenes. For debugging, optimization, and trust-building, showing the exact SQL that the semantic layer generates is extremely valuable — especially for DuckDB power users who want to understand what's happening and potentially copy/paste the SQL for further manipulation.

**Present in:** None of the reference systems expose this prominently. dbt MetricFlow has a `--explain` flag on `mf query`.

**Complexity:** Low. Expansion already produces a SQL string. An `EXPLAIN SEMANTIC VIEW ...` or query option that returns the expanded SQL (rather than executing it) is a simple addition.

**Dependencies:** TS-4, TS-6, TS-8.

---

### ANTI-FEATURES — Deliberately Out of Scope for v0.1

These features exist in reference systems but are explicitly excluded from v0.1. Each has a clear rationale.

---

#### AF-1: Pre-aggregation / materialization selection

**Description:** Automatically routing semantic view queries to pre-computed rollup tables when available (Cube's core feature).

**Why anti-feature in v0.1:** This is a completely separate problem from semantic expansion. It requires: a DDL for defining materializations, a refresh mechanism, a catalog of available rollups, and a matching algorithm (set-containment check). All of this is correct and valuable, but it is orthogonal to proving the semantic expansion layer works. The PROJECT.md explicitly defers this to v0.2+. Cube spent years building this; it is not a weekend feature.

**Implementation complexity if included:** High. Effectively doubles the scope. Adds DDL complexity, refresh scheduling, catalog management, and matching logic.

**Alternative:** Users can manually create DuckDB materialized tables and reference them in semantic view definitions. That is "pre-aggregation" without automation.

---

#### AF-2: YAML definition format

**Description:** Defining semantic views via YAML files (dbt/MetricFlow and Cube.dev style).

**Why anti-feature in v0.1:** Two definition formats means two parsers, two validation paths, and cognitive overhead for users who must learn both. The PROJECT.md explicitly defers this. SQL DDL is sufficient and is more natural for DuckDB users. If YAML is added later, it can compile to the same internal representation.

**Implementation complexity if included:** Medium. Parser + schema validation + file-watching + sync mechanism. Not enormous, but adds surface area without differentiating value in v0.1.

---

#### AF-3: Derived metrics (metric-on-metric)

**Description:** Metrics defined as arithmetic over other metrics (e.g., `profit = revenue - cost`, `conversion_rate = signups / visits`).

**Why anti-feature in v0.1:** Derived metrics require either (a) computing both base metrics in a single subquery — complex when dimensions differ — or (b) wrapping one subquery over another — adds query plan complexity. dbt MetricFlow handles this with multi-level SQL generation; Cube uses its Tesseract planner. This is a non-trivial semantic extension. v0.1 establishes the base: simple aggregations, single-level metrics. Derived metrics come after that foundation is solid.

**Ratio metrics** (numerator/denominator) are a lighter version of this but still require two independent aggregations composed. Also deferred.

**Implementation complexity if included:** High. Requires representing metric dependencies, generating sub-queries or CTEs per metric component, and handling edge cases where base metrics have incompatible dimensions.

---

#### AF-4: Multi-hop join resolution

**Description:** Automatically resolving join paths that span more than one relationship (A → B → C where the user requests dimensions from A and C, and B is intermediate).

**Why anti-feature in v0.1:** Multi-hop join resolution requires graph traversal of the relationship graph, cycle detection, and disambiguation when multiple paths exist. dbt MetricFlow and Cube.dev both support this and it is a source of significant complexity and subtle bugs. v0.1 limits to direct relationships: if A has a declared relationship to B, that join is available. Joining A to C via B is not automatically inferred.

**Implementation complexity if included:** High. Graph search + cycle detection + path disambiguation + ambiguous join path error handling.

**Alternative:** Users can define a SQL view that pre-joins A, B, C and use that as the base table in the semantic view definition.

---

#### AF-5: Hierarchies (drill-down paths)

**Description:** Declared dimension hierarchies (e.g., Country → Region → City) enabling consistent drill-down and roll-up behavior, with automatic level-of-detail management.

**Why anti-feature in v0.1:** Hierarchies are complex to define, query, and expand. They are prominent in OLAP cube systems (MDX, Analysis Services) but largely absent from the modern semantic layer systems surveyed — none of the four reference systems implement hierarchies natively. This is a post-v1 concern even in the reference systems.

**Implementation complexity if included:** Very high. Hierarchy definition, drill-path validation, level-of-detail expressions per level, breadcrumb semantics in query results.

---

#### AF-6: Multi-stage / nested aggregations

**Description:** Metrics that aggregate over other aggregations (e.g., "average of daily totals", "sum of last-7-day counts"). Requires a multi-level query plan with intermediate aggregation steps.

**Why anti-feature in v0.1:** This is Cube's Tesseract feature, still in preview in Cube. It requires a dedicated multi-stage SQL planner (or CTE chain generation) and is substantially more complex than standard aggregation. v0.1's group-by-and-aggregate model cannot express this. Cumulative/running-total metrics (dbt) are also deferred for the same reason.

**Implementation complexity if included:** Very high. Requires multi-level query plan representation, intermediate CTE generation, window function integration.

---

#### AF-7: BI tool metadata API / query API (HTTP/REST)

**Description:** An HTTP endpoint that BI tools can connect to, returning metadata (available measures, dimensions) and accepting semantic queries.

**Why anti-feature in v0.1:** This is the middleware model (Cube.dev, dbt Cloud). A DuckDB extension operates in-process and surfaces its interface through SQL. BI tools connect to DuckDB directly and use the SQL query syntax. A separate HTTP API introduces a server process, networking, authentication, and session management — none of which belong in a DuckDB extension. If integration with BI tools is needed, it goes through DuckDB's existing connectivity (JDBC, ADBC, ODBC) and the SQL interface.

**Implementation complexity if included:** Very high. Requires a separate server process, authentication, schema introspection API design, and maintenance of a client-facing contract.

---

#### AF-8: Column-level security / row access policies

**Description:** Hiding specific dimensions or metrics from users based on role, or applying row-level filter policies that restrict data access.

**Why anti-feature in v0.1:** DuckDB is primarily a local/single-user analytics engine. Multi-user security is not a DuckDB strength and adding it to the extension adds complexity without a target user. If security is needed, it belongs at the source table level (DuckDB's built-in privileges) or in the platform deploying DuckDB. The semantic view extension should be transparent to DuckDB's existing permission model.

**Implementation complexity if included:** High. Requires integration with DuckDB's user/role system, dynamic filter injection per query, secure view resolution.

---

#### AF-9: Saved / named queries with materialization

**Description:** Named query configurations (specific dimension + metric + filter combinations) that can be stored and materialized as tables or views on demand (dbt MetricFlow "saved queries" + "exports").

**Why anti-feature in v0.1:** While useful, this is a convenience layer on top of the core expansion feature. Users in v0.1 can achieve the equivalent by defining a DuckDB view over a semantic view query. The semantic layer should stabilize before adding query management features.

**Implementation complexity if included:** Medium. Requires DDL for named queries, storage, and a refresh/export mechanism.

---

## Feature Dependency Graph

```
TS-5 (DDL syntax)
  └─> TS-10 (persistence)
  └─> TS-6 (query syntax)
        └─> TS-8 (validation)
              └─> D-6 (inspectable expansion)

TS-1 (dimensions)
  └─> TS-2 (metrics)
        └─> TS-3 (relationships)
              └─> TS-4 (GROUP BY inference)
                    └─> TS-7 (time dimensions)
                    └─> TS-9 (row filters)

TS-4 + TS-6 + TS-8
  └─> D-3 (SQL composability)

[D-1, D-2, D-4, D-5 have no semantic dependencies — they are properties of the DuckDB extension architecture]
```

**Critical path for v0.1:** TS-1 → TS-2 → TS-3 → TS-4 → TS-5 → TS-6 → TS-8 → TS-10

---

## Summary Table

| ID | Feature | Category | Complexity | v0.1? |
|---|---|---|---|---|
| TS-1 | Dimension definitions with SQL expressions | Table stakes | Low | Yes |
| TS-2 | Metric definitions with aggregation type | Table stakes | Low | Yes |
| TS-3 | Relationship / join declarations | Table stakes | Medium | Yes (single-hop only) |
| TS-4 | Automatic GROUP BY inference | Table stakes | Low | Yes |
| TS-5 | SQL DDL definition syntax | Table stakes | Medium-High | Yes |
| TS-6 | Table function query syntax | Table stakes | Medium | Yes |
| TS-7 | Time dimensions with granularity | Table stakes | Low-Medium | Yes |
| TS-8 | Expansion-time validation with clear errors | Table stakes | Medium | Yes |
| TS-9 | Row-level filter predicates | Table stakes | Low | Yes |
| TS-10 | Persistence of definitions | Table stakes | Medium | Yes |
| D-1 | In-process / embedded operation | Differentiator | Free | Yes |
| D-2 | SQL-native definition (no YAML) | Differentiator | Free (via TS-5) | Yes |
| D-3 | SQL composability (outer queries) | Differentiator | Low | Yes |
| D-4 | Works with local files (Parquet, CSV) | Differentiator | Free | Yes |
| D-5 | Zero-dependency extension install | Differentiator | Medium | Yes (at release) |
| D-6 | Inspectable SQL expansion (EXPLAIN) | Differentiator | Low | Yes |
| AF-1 | Pre-aggregation / materialization selection | Anti-feature | High | No (v0.2+) |
| AF-2 | YAML definition format | Anti-feature | Medium | No |
| AF-3 | Derived metrics (metric-on-metric) | Anti-feature | High | No |
| AF-4 | Multi-hop join resolution | Anti-feature | High | No |
| AF-5 | Hierarchies (drill-down paths) | Anti-feature | Very High | No |
| AF-6 | Multi-stage / nested aggregations | Anti-feature | Very High | No |
| AF-7 | BI tool metadata / HTTP API | Anti-feature | Very High | No |
| AF-8 | Column-level security / row access | Anti-feature | High | No |
| AF-9 | Saved queries with materialization | Anti-feature | Medium | No |

---

## Key Observations for v0.1 Scoping

**The table stakes list is achievable.** All 10 table-stakes features are implementable in a single milestone. The semantic model is: dimensions, metrics, relationships, time dimensions, and row filters. The query interface is: expand to SQL with GROUP BY and JOIN inference. The two medium-high complexity items (TS-5 DDL syntax, TS-10 persistence) are the core DuckDB extension integration work that must be done regardless.

**The differentiators are mostly free.** D-1 (in-process), D-2 (SQL-native), and D-4 (local files) are architectural properties of being a DuckDB extension — no additional implementation cost. D-3 (SQL composability) and D-6 (inspectable expansion) are low-complexity additions. D-5 (community extension packaging) is a one-time investment with high payoff.

**The anti-feature list matches what's already in PROJECT.md's Out of Scope section.** This research confirms that deferring pre-aggregation (AF-1), YAML (AF-2), derived metrics (AF-3), and multi-hop joins (AF-4) is consistent with what the reference systems themselves treat as optional/advanced. None of them are table stakes: the simplest useful semantic layer is expansion + validation.

**The critical gap is COUNT DISTINCT in v0.1.** All four reference systems support it as a metric type. It should be included in v0.1 as an aggregation type (TS-2). The non-additivity concern (it cannot be re-aggregated from pre-aggregations) is only relevant when pre-aggregation (AF-1) is implemented. For v0.1 expansion-only, COUNT DISTINCT is just `COUNT(DISTINCT col)` emitted directly.

**Ratio metrics are the first v0.2 semantic addition.** After derived metrics (AF-3) is out, the next-simplest semantic feature is ratio metrics (numerator/denominator). Three of the four reference systems have it. It requires two sub-aggregations and a division — more complex than simple aggregations but less complex than full derived metric arithmetic.

---

## Sources

Feature knowledge drawn from:
- Snowflake Semantic Views documentation (https://docs.snowflake.com/en/user-guide/views-semantic/overview)
- Snowflake `SEMANTIC_VIEW` query syntax reference (https://docs.snowflake.com/en/sql-reference/constructs/semantic_view)
- Databricks Metric Views documentation (https://docs.databricks.com/en/data-governance/metric-views/index.html)
- Cube.dev data model documentation (https://cube.dev/docs/product/data-modeling/overview)
- Cube.dev pre-aggregation documentation (https://cube.dev/docs/product/caching/matching-pre-aggregations)
- dbt MetricFlow documentation (https://docs.getdbt.com/docs/build/about-metricflow)
- dbt Semantic Layer overview (https://docs.getdbt.com/docs/use-dbt-semantic-layer/dbt-sl)
- `_notes/semantic-views-duckdb-design-doc.md` (prior art research in this repo)
- `.planning/PROJECT.md` (project scope and constraints)

*Note: Web access was not available during this research session. All feature knowledge is based on training data through August 2025 and the design doc in this repository. Key claims should be verified against current documentation before finalizing requirements.*
