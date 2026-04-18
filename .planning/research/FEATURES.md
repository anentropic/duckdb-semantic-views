# Feature Landscape: v0.7.0 YAML Definitions & Materialization Routing

**Domain:** DuckDB Rust extension -- YAML definition format and materialization routing engine
**Researched:** 2026-04-17
**Milestone:** v0.7.0 -- YAML as second definition format + materialization routing for pre-aggregated tables
**Status:** Subsequent milestone research (v0.6.0 shipped 2026-04-14)
**Overall confidence:** HIGH (Snowflake YAML spec verified from official docs; Cube.dev pre-aggregation matching algorithm verified from docs + source references; Databricks metric view materialization verified from official docs)

---

## Scope

This document covers two feature areas for v0.7.0:

**Area A: YAML Definition Format**
- Inline YAML: `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$`
- File-based YAML: `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'`
- YAML round-trip export: GET_DDL variant producing YAML output

**Area B: Materialization Routing**
- MATERIALIZATIONS clause in CREATE SEMANTIC VIEW DDL
- Query-time routing to pre-existing aggregated tables
- Re-aggregation for subset dimension matches
- Fallback to raw table expansion (current behavior)

**What already exists (NOT in scope for research):**
- Complete SQL keyword DDL (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS)
- All metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC)
- Semi-additive metrics, window function metrics, derived metrics
- Fan trap detection, role-playing dimensions, USING RELATIONSHIPS
- GET_DDL for SQL DDL round-trip
- Full query expansion engine producing concrete SQL
- SemanticViewDefinition model with serde Serialize/Deserialize

---

## Comparable System Analysis

### Snowflake: YAML Semantic Views

**How it works:** Snowflake provides two system procedures for YAML-based semantic views:

1. `SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML(schema, yaml_string, verify_only)` -- creates a semantic view from YAML
2. `SYSTEM$READ_YAML_FROM_SEMANTIC_VIEW(view_name)` -- exports a semantic view back to YAML

The YAML format is table-centric (dimensions, facts, metrics are nested under tables), while the SQL DDL is flat (dimensions, facts, metrics are top-level clauses with qualified names). Both produce identical semantic views -- the storage format is the same regardless of input method.

**Key YAML schema structure:**
```yaml
name: view_name
description: view comment
tables:
  - name: alias
    base_table: { database: db, schema: sch, table: tbl }
    primary_key: { columns: [col1] }
    dimensions: [{ name, expr, data_type, synonyms, description, unique, is_enum, sample_values }]
    time_dimensions: [{ name, expr, data_type, synonyms, description, unique }]
    facts: [{ name, expr, data_type, synonyms, description, access_modifier }]
    metrics: [{ name, expr, synonyms, description, access_modifier, non_additive_dimensions, using_relationships }]
    filters: [{ name, expr, synonyms, description }]
relationships:
  - { name, left_table, right_table, relationship_columns: [{ left_column, right_column }], relationship_type }
metrics:  # view-level derived metrics
  - { name, expr, synonyms, description, access_modifier }
verified_queries:  # Cortex Analyst hints
  - { name, question, sql, verified_by, verified_at, use_as_onboarding_question }
```

**Key differences from SQL DDL:**
- YAML nests dims/facts/metrics under tables (table-centric); SQL DDL uses flat qualified references (`table.name`)
- YAML has `time_dimensions` as a separate category; SQL DDL has no such distinction
- YAML has `filters` (reusable WHERE conditions); SQL DDL has no `FILTERS` clause
- YAML has `verified_queries` (Cortex AI hints); SQL DDL has no equivalent
- YAML uses `access_modifier: private_access`; SQL DDL uses `PRIVATE` keyword
- YAML has `is_enum` and `sample_values` on dimensions; SQL DDL does not
- YAML `relationship_type` is `many_to_one` (inferred); SQL DDL infers from PK/UNIQUE

**Round-trip fidelity:** Snowflake guarantees that `CREATE FROM YAML -> READ YAML` produces valid YAML that can recreate the same view. The exported YAML may reformat or reorder fields but is semantically equivalent.

**Confidence:** HIGH (verified from Snowflake official docs: YAML spec page, CREATE_FROM_YAML procedure, READ_YAML function)

### Databricks: Metric View Materialization

**How it works:** Databricks embeds materialization declarations directly in the metric view YAML definition via a `materialization` top-level field:

```yaml
materialization:
  schedule: every 6 hours
  mode: relaxed
  materialized_views:
    - name: baseline
      type: unaggregated
    - name: revenue_breakdown
      type: aggregated
      dimensions: [category, color]
      measures: [total_revenue]
```

**Two materialization types:**
1. **Aggregated:** Pre-computes specific dimension+measure combinations. Query routing requires exact dimension match and measure subset match.
2. **Unaggregated:** Materializes the entire unaggregated data model (source tables + joins + filters applied). Broader coverage, less performance lift.

**Query routing algorithm (3-step):**
1. **Exact match:** grouping expressions precisely match materialization dimensions; aggregation expressions are a subset of materialization measures
2. **Unaggregated match:** if an unaggregated materialization exists
3. **Fallback:** route to source tables

**Relaxed mode:** Only checks that materialized views contain necessary dimensions and measures. Skips freshness verification, SQL setting compatibility, and result determinism checks.

**Refresh:** Managed by Lakeflow Spark Declarative Pipelines. Schedule-based or manual `REFRESH MATERIALIZED VIEW <name>`.

**Confidence:** HIGH (verified from Databricks official docs)

### Cube.dev: Pre-Aggregation Matching

**How it works:** Cube defines pre-aggregations inline in cube definitions:

```javascript
cube(`orders`, {
  measures: { count: { type: `count` } },
  dimensions: { status: { sql: `status`, type: `string` } },
  pre_aggregations: {
    orders_by_status: {
      measures: [count],
      dimensions: [status],
      time_dimension: created_at,
      granularity: `day`,
    }
  }
});
```

**Matching algorithm (sequential, first-match-wins):**
1. Extract members (measures, dimensions, time dimensions) from query
2. If query references views, resolve to underlying cube members
3. Scan pre-aggregations in definition order (rollup before original_sql)
4. For each candidate, check ALL of:
   - All leaf measures are present AND **additive** (SUM, COUNT, MIN, MAX, COUNT_DISTINCT_APPROX compose; COUNT_DISTINCT, AVG do not)
   - All dimensions and filter dimensions are present
   - Time granularity is <= query granularity (use GCD: a `day` rollup serves `week`/`month` queries)
   - Timezone matches
   - Join tree is compatible
5. First match wins; fall back to raw source if none

**Re-aggregation:** When a query uses a subset of dimensions from a matched rollup, Cube Store "aggregates over missing dimensions." This only works for additive measures. Cube uses "aggregating indexes" -- essentially a rollup of a rollup table -- to optimize this pattern at build time rather than query time.

**Non-additive measures:** Three strategies:
1. Replace `count_distinct` with `count_distinct_approx` (additive, approximate)
2. Decompose: store SUM + COUNT separately, compute AVG at query time
3. Create exact-match-only pre-aggregations (no re-aggregation possible)

**Confidence:** HIGH (verified from Cube.dev official docs + design doc analysis)

### dbt MetricFlow: Saved Queries / Exports

**How it works:** dbt does NOT have inline materialization declarations. Instead:
1. Define metrics in YAML
2. Create "saved queries" that pin specific metric+dimension combinations
3. "Exports" materialize saved queries into tables/views in the data platform

```yaml
saved_queries:
  - name: revenue_by_region
    query_params:
      metrics: [total_revenue]
      group_by: [region, date]
    exports:
      - name: revenue_by_region_table
        config:
          export_as: table
          schema: analytics
```

dbt MetricFlow does NOT do automatic query routing to materialized tables. Exports are static materialized outputs. Users query the exported table directly or use the Semantic Layer API with optional caching.

**Confidence:** MEDIUM (verified from dbt official docs; dbt's approach is architecturally different -- no transparent routing)

---

## Table Stakes

Features users expect. Missing = incomplete or broken-feeling implementation.

### T1: YAML Definition Parsing (FROM YAML $$...$$)

| Aspect | Detail |
|--------|--------|
| **Feature** | Parse inline YAML in `CREATE SEMANTIC VIEW name FROM YAML $$ yaml_content $$` and produce the same `SemanticViewDefinition` as SQL DDL. Dollar-quoting avoids escaping issues. |
| **Why Expected** | Snowflake provides this via `SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML`. YAML is the lingua franca for semantic model definitions across Cube.dev (JS/YAML), dbt (YAML), and Databricks (YAML). Many users maintain semantic models in YAML files for version control. The existing JSON storage model (`SemanticViewDefinition` with serde) makes YAML a near-zero-cost addition. |
| **Complexity** | **Medium** -- YAML parsing is straightforward via serde; the complexity is in mapping Snowflake's table-centric YAML schema to the existing flat model, handling field name differences, and validating required fields. |
| **Dependencies** | New crate dependency: a YAML serde library (see STACK.md for recommendation). Parser hook must detect `FROM YAML` prefix. |

**Design decisions needed:**

1. **YAML schema design -- Snowflake-aligned vs native:**
   - Snowflake nests dims/facts/metrics under tables (table-centric). The existing SQL DDL uses flat qualified references (`table.name AS expr`).
   - **Recommendation:** Use the Snowflake YAML schema as the canonical format. The extension already stores a flat `SemanticViewDefinition` JSON internally -- the YAML parser must transform table-centric YAML into the flat model. This is a straightforward denormalization step: iterate tables, prefix each dim/fact/metric name with `table_alias.name`.

2. **Dollar-quoting syntax:**
   - `FROM YAML $$ ... $$` follows PostgreSQL dollar-quoting convention. The parser detects `FROM YAML` after the view name, then scans for matching `$$` delimiters.
   - Alternative: `FROM YAML '...'` with single-quote escaping -- worse UX for multi-line YAML.
   - **Recommendation:** Dollar-quoting (`$$`). Simple to implement in the parser (scan for `$$` start, read until next `$$`).

3. **Fields not in existing model:**
   - `time_dimensions` -- Snowflake distinguishes these from regular dimensions. The existing model has no such distinction (removed in v0.4.0). **Decision:** Accept `time_dimensions` in YAML, store as regular dimensions. Emit as `dimensions` in round-trip. No special query-time behavior.
   - `filters` -- Snowflake has named reusable filter conditions. Not in current model. **Decision:** Defer to future milestone. Reject with clear error if present in YAML.
   - `verified_queries` -- Cortex AI feature. **Decision:** Ignore/strip silently. Not applicable.
   - `is_enum`, `sample_values` -- Dimension metadata for AI. **Decision:** Ignore/strip silently. Pure metadata with no query-time behavior.
   - `cortex_search_service` -- Snowflake-specific. **Decision:** Ignore/strip silently.
   - `data_type` -- Snowflake requires this on dimensions/facts in YAML. The existing model has `output_type` (optional). **Decision:** Map `data_type` to `output_type`.

**Confidence:** HIGH (serde makes YAML parsing trivial; the mapping logic is the real work)

---

### T2: YAML File Loading (FROM YAML FILE '/path')

| Aspect | Detail |
|--------|--------|
| **Feature** | Load YAML from a file path: `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/definition.yaml'`. Read file, parse as YAML, produce same result as inline YAML. |
| **Why Expected** | Users maintain semantic models in version-controlled YAML files. Snowflake's `SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML` accepts YAML as a string (requiring users to `$$`-quote file contents). File-based loading is a DX improvement that Snowflake does NOT offer natively -- it requires wrapping in Python/SQL scripting. This is a differentiator for the DuckDB extension. |
| **Complexity** | **Low** -- File I/O + reuse T1 parsing. The parser detects `FROM YAML FILE`, reads the path, loads the file, delegates to the YAML parser. |
| **Dependencies** | T1 (YAML parsing logic). File access permissions (DuckDB extension sandbox considerations). |

**Design decisions needed:**

1. **Path resolution:** Relative paths should resolve relative to the DuckDB database file directory (consistent with DuckDB's `read_csv_auto` behavior). Absolute paths work as-is.
2. **File not found:** Clear error: `"YAML file not found: '/path/to/file.yaml'"`.
3. **Security:** DuckDB extensions can read files within DuckDB's allowed paths. No special sandboxing needed beyond what DuckDB already enforces.

**Confidence:** HIGH (trivial file I/O; all complexity is in T1)

---

### T3: YAML Round-Trip Export (GET_DDL variant)

| Aspect | Detail |
|--------|--------|
| **Feature** | Export a semantic view definition as YAML. Either a new function `GET_YAML('SEMANTIC_VIEW', 'name')` or a format parameter `GET_DDL('SEMANTIC_VIEW', 'name', format := 'yaml')`. |
| **Why Expected** | Snowflake provides `SYSTEM$READ_YAML_FROM_SEMANTIC_VIEW(name)` for this exact purpose. The YAML export enables version control, migration between environments, and creating new views from modified exports. Round-trip fidelity (YAML -> create -> export YAML -> create) is critical. |
| **Complexity** | **Medium** -- requires serializing `SemanticViewDefinition` back to the Snowflake-aligned YAML schema (table-centric nesting). This is the inverse of T1's denormalization. |
| **Dependencies** | T1 (same YAML schema understanding). Existing GET_DDL infrastructure in `render_ddl.rs`. |

**Design decisions needed:**

1. **Interface:** Two options:
   - `GET_DDL('SEMANTIC_VIEW', 'name', 'YAML')` -- third parameter selects format. Natural extension of existing GET_DDL.
   - `GET_YAML('SEMANTIC_VIEW', 'name')` -- separate function. Cleaner but another DDL/function to register.
   - **Recommendation:** `GET_DDL` with format parameter. Snowflake uses a separate function but our GET_DDL is already a scalar function; adding a format parameter is trivial.

2. **Re-nesting logic:** The flat `SemanticViewDefinition` stores dimensions with `source_table: Some("orders")`. The YAML export must group dimensions by source_table and nest them under the appropriate table entry. Dimensions/facts/metrics without `source_table` go under the first (base) table.

3. **Fields to emit:** Emit all fields that are present in the model. Omit Snowflake-only fields that we don't store (`time_dimensions` as separate category, `filters`, `verified_queries`, `is_enum`, `sample_values`). Use Snowflake field names for compatibility (`access_modifier: private_access` not `access: Private`).

4. **Round-trip fidelity guarantee:** `CREATE FROM YAML -> GET_DDL YAML` must produce YAML that can recreate the same view. Field ordering may differ but semantic content must be identical. This means:
   - Order of tables, dimensions, metrics in YAML output should follow definition order
   - Default values should be omitted (e.g., `access_modifier: public_access` is the default, omit it)

**Confidence:** HIGH (inverse of T1; serde YAML serialization handles the heavy lifting)

---

### T4: Materialization Declaration (MATERIALIZATIONS clause)

| Aspect | Detail |
|--------|--------|
| **Feature** | New `MATERIALIZATIONS` clause in CREATE SEMANTIC VIEW DDL declaring pre-existing aggregated tables with their covered dimensions and metrics. No automatic table creation or refresh -- the user manages the materialized tables. |
| **Why Expected** | This is the core value of the materialization routing feature. Cube.dev, Databricks, and the project's own design doc all identify materialization routing as the second phase of semantic view architecture. The existing expansion engine handles Phase 1 (semantic expansion); this adds Phase 2 (pre-aggregation selection). |
| **Complexity** | **Medium** -- new DDL clause, model struct, parser changes. No query-time behavior yet (that's T5). |
| **Dependencies** | None on YAML features. |

**DDL syntax design:**

```sql
CREATE SEMANTIC VIEW sales_analysis
TABLES (...)
RELATIONSHIPS (...)
DIMENSIONS (...)
METRICS (...)
MATERIALIZATIONS (
    daily_revenue AS 'analytics.daily_revenue_agg'
        DIMENSIONS (date_dim, region)
        METRICS (total_revenue, order_count),
    monthly_summary AS 'analytics.monthly_summary'
        DIMENSIONS (month_dim, region, category)
        METRICS (total_revenue, order_count, avg_order_value)
)
```

**Key design decisions:**

1. **Materialization = pointer to existing table:** Unlike Cube.dev (which manages materialization lifecycle) or Databricks (which creates and refreshes materialized views), this extension only POINTS to pre-existing tables. The user creates/refreshes these tables themselves (via DuckDB `CREATE TABLE AS`, dbt, or external ETL). This aligns with the "DuckDB is the engine, extension is the preprocessor" design principle.

2. **Metadata stored per materialization:**
   ```rust
   pub struct Materialization {
       pub name: String,           // logical name for the materialization
       pub table: String,          // physical table name (qualified)
       pub dimensions: Vec<String>,// dimension names covered (must match dim names in view)
       pub metrics: Vec<String>,   // metric names covered (must match metric names in view)
   }
   ```

3. **Define-time validation:**
   - All dimension names must exist in the semantic view's DIMENSIONS
   - All metric names must exist in the semantic view's METRICS
   - The materialized table must exist (verify via `LIMIT 0` query, same pattern as type inference)
   - Warn (not error) if a metric is non-additive and appears in a materialization (re-aggregation may produce incorrect results)

4. **Ordering:** Materializations are checked in definition order (first match wins), following Cube.dev's convention.

**What this does NOT include:**
- No `REFRESH` mechanism (user manages tables)
- No `SCHEDULE` (no automation)
- No freshness tracking (no staleness checks)
- No `type: unaggregated` (Databricks-specific; all materializations are aggregated)

**Confidence:** HIGH (straightforward DDL + model addition; query routing is T5)

---

### T5: Query-Time Materialization Routing

| Aspect | Detail |
|--------|--------|
| **Feature** | At query time, when `semantic_view('view', dimensions := [...], metrics := [...])` is called, check if any declared materialization covers the requested dimensions and metrics. If yes, route to the materialized table instead of expanding raw tables. |
| **Why Expected** | This is the core value proposition. Without routing, materializations are just metadata. The design doc (Phase 2) explicitly calls this out as a substitution, not a rewrite. |
| **Complexity** | **High** -- requires a matching algorithm, SQL generation for materialization queries, and integration with the existing expansion pipeline. |
| **Dependencies** | T4 (materialization declarations must exist). |

**Matching algorithm (adapted from Cube.dev + Databricks):**

```
fn select_materialization(
    requested_dims: &[DimensionName],
    requested_metrics: &[MetricName],
    materializations: &[Materialization],
) -> Option<&Materialization> {
    for mat in materializations {
        // 1. ALL requested metrics must be present in materialization
        if !requested_metrics.iter().all(|m| mat.metrics.contains(m)) {
            continue;
        }
        // 2. ALL requested dimensions must be present in materialization
        //    (superset is OK -- we re-aggregate over extra dims)
        if !requested_dims.iter().all(|d| mat.dimensions.contains(d)) {
            continue;
        }
        // 3. ALL requested metrics must be additive for re-aggregation
        //    (if mat has MORE dimensions than requested)
        if mat.dimensions.len() > requested_dims.len() {
            if !requested_metrics.iter().all(|m| is_additive(m)) {
                continue;
            }
        }
        return Some(mat);
    }
    None
}
```

**Additivity classification:**

| Aggregate | Additive | Re-aggregation function |
|-----------|----------|------------------------|
| SUM | Yes | SUM |
| COUNT | Yes | SUM (count of counts) |
| MIN | Yes | MIN |
| MAX | Yes | MAX |
| AVG | No* | N/A (decompose into SUM/COUNT) |
| COUNT(DISTINCT ...) | No | N/A |
| Derived metrics | Depends | Check leaf measures |
| Semi-additive (NON ADDITIVE BY) | No | N/A (snapshot semantics break) |
| Window metrics | No | N/A (window semantics break) |

*AVG can be decomposed but requires both SUM and COUNT to be in the materialization. This is a future optimization, not table stakes.

**SQL generation for materialization hit:**

Exact match (same dimensions):
```sql
SELECT dim1, dim2, metric1, metric2
FROM analytics.daily_revenue_agg
```

Subset match (fewer dimensions requested than materialized):
```sql
SELECT dim1, SUM(metric1) AS metric1, SUM(metric2) AS metric2
FROM analytics.daily_revenue_agg
GROUP BY dim1
```

Note: COUNT metrics become `SUM(metric_name)` in re-aggregation because the materialized table stores counts that must be summed, not re-counted.

**Key design decisions:**

1. **Matching order:** Definition order (first match wins). Simple, predictable, same as Cube.dev.
2. **Exact match preference:** If a materialization has exactly the requested dimensions, prefer it over one requiring re-aggregation. This means we should check exact matches first, then superset matches.
3. **Filter passthrough:** WHERE clauses from the user's query must be applied to the materialization table query. The materialization table must have the filtered columns available.
4. **Non-additive rejection:** If ANY requested metric is non-additive and the match requires re-aggregation (superset dimensions), skip that materialization. Exact dimension matches are fine for non-additive metrics.
5. **Fallback:** If no materialization matches, fall back to current behavior (raw table expansion). This is the default and must always work.

**Integration with expansion pipeline:**

The routing check happens BEFORE expansion. In the `build_execution_sql` flow:
1. Parse requested dimensions and metrics
2. Check materializations (new step)
3. If match found: generate simple SELECT from materialized table (possibly with GROUP BY for re-aggregation)
4. If no match: proceed with existing expansion pipeline

This is a clean insertion point -- the materialization router produces a complete SQL string that replaces the expansion output.

**Confidence:** HIGH for the matching algorithm (well-established pattern across Cube.dev and Databricks); MEDIUM for integration with existing pipeline (needs careful handling of WHERE clauses, column naming, and type inference)

---

### T6: Re-Aggregation for Subset Matches

| Aspect | Detail |
|--------|--------|
| **Feature** | When a materialization has MORE dimensions than requested, wrap the materialization query in a GROUP BY over the requested dimensions, re-aggregating the metrics. |
| **Why Expected** | Cube.dev calls this "aggregating over missing dimensions." Databricks exact-match-only approach is more conservative. Re-aggregation is what makes a single wide materialization serve multiple queries. Without it, you'd need one materialization per dimension combination. |
| **Complexity** | **Medium** -- the SQL generation is straightforward but requires knowing the re-aggregation function for each metric. |
| **Dependencies** | T4 (materializations), T5 (routing). |

**Re-aggregation function mapping:**

The expansion engine already knows each metric's aggregate expression (e.g., `SUM(orders.amount)`). For re-aggregation, we need to map from the original aggregate to the re-aggregation aggregate:

| Original Aggregate | Re-Aggregation Aggregate | Notes |
|--------------------|----|-------|
| `SUM(expr)` | `SUM(metric_name)` | Additive |
| `COUNT(*)` | `SUM(metric_name)` | Count-of-counts = SUM |
| `COUNT(expr)` | `SUM(metric_name)` | Same as COUNT(*) |
| `MIN(expr)` | `MIN(metric_name)` | Min-of-mins = MIN |
| `MAX(expr)` | `MAX(metric_name)` | Max-of-maxes = MAX |
| `AVG(expr)` | Not supported | Would need SUM+COUNT decomposition |
| `COUNT(DISTINCT expr)` | Not supported | Cannot re-aggregate distinct counts |

**Implementation approach:**

1. Parse the metric's `expr` to extract the aggregate function name (SUM/COUNT/MIN/MAX/AVG/etc.)
2. Classify as additive or non-additive
3. For additive metrics, generate re-aggregation SQL using the mapped function
4. For non-additive metrics, reject the materialization for subset matches

**Aggregate detection:** The existing model stores `expr` as a raw SQL string (e.g., `"SUM(orders.amount)"`). We need to extract the outermost aggregate function. A simple regex or prefix scan for `SUM(`, `COUNT(`, `MIN(`, `MAX(` is sufficient -- derived metrics that reference other metrics are already inlined before this point.

**Edge case -- derived metrics:** A derived metric like `revenue_per_order AS total_revenue / order_count` references two additive metrics. For re-aggregation from a materialization, the derived metric's stored value in the materialized table is already the derived value -- it CANNOT be re-aggregated. Derived metrics should either:
- Be excluded from re-aggregation (treated as non-additive)
- Or be re-computed from their component metrics in the re-aggregation query

**Recommendation:** Exclude derived metrics from re-aggregation. If a query requests a derived metric and the materialization match requires re-aggregation, skip that materialization. This is conservative but correct.

**Confidence:** MEDIUM (aggregate detection from raw SQL strings is heuristic; need robust parsing or model-level metadata)

---

## Differentiators

Features that set the product apart or exceed comparable system capabilities.

### D1: Additivity Metadata on Metrics

| Aspect | Detail |
|--------|--------|
| **Feature** | Store explicit additivity classification on each metric in the model, derived at define time from the metric expression. Expose in DESCRIBE output. |
| **Value Proposition** | Cube.dev derives additivity from measure type declarations. Databricks infers from aggregate functions. This extension currently stores raw SQL expressions. Adding additivity metadata removes the need for heuristic aggregate detection at query time (T6 re-aggregation) and makes the routing decision more robust. |
| **Complexity** | **Medium** -- aggregate function detection at parse time + model field |
| **Dependencies** | Useful for T5/T6 but not strictly required (can detect at query time) |

**Implementation:**
```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Additivity {
    #[default]
    Unknown,       // raw expr, couldn't detect
    Additive,      // SUM, COUNT, MIN, MAX
    NonAdditive,   // AVG, COUNT DISTINCT, etc.
    Derived,       // references other metrics
    SemiAdditive,  // NON ADDITIVE BY present
    Window,        // window function metric
}
```

Parse-time detection: scan `expr` for outermost aggregate function. If it starts with `SUM(`, `COUNT(`, `MIN(`, `MAX(` -> Additive. If `AVG(` -> NonAdditive. If it references another metric name -> Derived. If `non_additive_by` is non-empty -> SemiAdditive. If `window_spec` is Some -> Window.

This metadata would also power SHOW SEMANTIC METRICS with an `additivity` column.

**Confidence:** HIGH (straightforward classification from existing model data)

---

### D2: Materialization Validation Report

| Aspect | Detail |
|--------|--------|
| **Feature** | A diagnostic command `EXPLAIN MATERIALIZATION FOR SEMANTIC VIEW 'name' dimensions := [...] metrics := [...]` that shows which materialization (if any) would be selected and why. |
| **Value Proposition** | Databricks uses `EXPLAIN EXTENDED` to verify materialization routing. Cube.dev has `Playground` diagnostics. Without a way to verify routing decisions, users cannot debug why a query hits raw tables instead of a materialization. |
| **Complexity** | **Low-Medium** -- reuse matching algorithm, format diagnostic output |
| **Dependencies** | T4, T5 |

**Output format:**
```
Selected: daily_revenue (analytics.daily_revenue_agg)
Match type: subset (re-aggregation required)
Dimensions matched: 2/3 (date_dim, region)
Extra dimensions: category (will be aggregated away)
Metrics: total_revenue (SUM -> SUM), order_count (COUNT -> SUM)
Skipped: monthly_summary (missing metric: avg_order_value)
```

**Confidence:** HIGH (diagnostic wrapper around T5 matching logic)

---

### D3: YAML with MATERIALIZATIONS

| Aspect | Detail |
|--------|--------|
| **Feature** | Support MATERIALIZATIONS in the YAML schema, enabling full definition of a semantic view (including materialization routing) in a single YAML file. |
| **Value Proposition** | Databricks includes materialization in their YAML metric view definitions. This would make the YAML format fully self-contained. |
| **Complexity** | **Low** -- extend YAML schema with materializations section |
| **Dependencies** | T1 (YAML parsing), T4 (materialization model) |

**YAML syntax:**
```yaml
materializations:
  - name: daily_revenue
    table: analytics.daily_revenue_agg
    dimensions: [date_dim, region]
    metrics: [total_revenue, order_count]
```

**Confidence:** HIGH (simple YAML section mapped to existing Materialization model)

---

## Anti-Features

Features to explicitly NOT build in v0.7.0.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Automatic materialization creation/refresh | DuckDB extension is a preprocessor, not a scheduler. Cube.dev and Databricks manage materialization lifecycle because they control the runtime. DuckDB does not. Users create tables themselves. | Declare pointers to existing tables. Document `CREATE TABLE AS` patterns. |
| Freshness tracking / staleness detection | Requires metadata storage for refresh timestamps, which adds complexity for marginal value in a local DuckDB context. | User is responsible for table freshness. Consider a SHOW MATERIALIZATIONS command showing declared (not live) metadata. |
| `type: unaggregated` materializations | Databricks-specific. An unaggregated materialization is just a regular table with joins pre-applied -- the user can create this themselves. The routing logic would need a different code path (no re-aggregation, just scan replacement). | Only support aggregated materializations. For "pre-joined" tables, users can create a view and reference it as a base table. |
| AVG decomposition (SUM+COUNT auto-split) | Cube.dev supports this but it requires the materialization table to have separate sum and count columns. The extension cannot verify this without schema inspection of the external table. Too magic. | Reject AVG metrics in re-aggregation scenarios. Users should store SUM and COUNT separately if they want re-aggregation. |
| Cross-view materialization sharing | The design doc explicitly lists this as a non-goal. Each semantic view's materializations are independent. | No cross-view optimization. |
| Snowflake `time_dimensions` as first-class category | Removed in v0.4.0. Time dimensions are regular dimensions with `date_trunc()` in expr. Adding a separate category would be a regression. | Accept `time_dimensions` in YAML, store as regular dimensions. |
| Snowflake `filters` clause | Not in the existing model. Would require a new DDL clause and query-time filter application mechanism. Orthogonal to v0.7.0 goals. | Reject with clear error if present in YAML. Defer to future milestone. |
| Snowflake `verified_queries` / Cortex AI features | DuckDB extension has no AI integration. These are Snowflake-specific. | Silently ignore in YAML parsing. |
| Granularity-based matching (day serves month) | Cube.dev does GCD granularity matching for time dimensions. This extension has no granularity concept (removed in v0.4.0). Time truncation is in dimension expressions. | Dimension matching is name-based only. A `date_dim` materialization does not auto-serve `month_dim` queries. Users must declare separate materializations or use the same dimension names. |
| Materialization priority/scoring | Cube.dev uses first-match-wins. Adding scoring (prefer exact match, then smallest superset) adds complexity. | First-match-wins for v0.7.0. Recommend users order materializations from most-specific to least-specific. Document this clearly. Consider exact-match-first as a v0.7.1 enhancement. |

---

## Feature Dependencies

```
T1: YAML Parsing (FROM YAML $$...$$)
  |
  +---> T2: YAML File Loading (FROM YAML FILE) -- reuses T1 parser
  |
  +---> T3: YAML Export (GET_DDL YAML format) -- inverse of T1 mapping
  |
  +---> D3: YAML with MATERIALIZATIONS -- extends T1 + T4

T4: Materialization Declaration (MATERIALIZATIONS clause)
  |
  +---> T5: Query-Time Routing -- uses T4 model
  |       |
  |       +---> T6: Re-Aggregation -- extends T5 with GROUP BY wrapper
  |
  +---> D2: Materialization Validation Report -- diagnostic for T5
  |
  +---> D3: YAML with MATERIALIZATIONS -- extends T4 model

D1: Additivity Metadata -- independent but improves T5/T6

No dependency between YAML features (T1-T3) and Materialization features (T4-T6).
They can be implemented in parallel tracks.
```

**Critical ordering insight:** YAML and Materialization are independent feature tracks. They can be phased separately:

- **Track A (YAML):** T1 -> T2 -> T3
- **Track B (Materialization):** T4 -> T5 -> T6 -> D2

T3 (YAML export) should come after T4 if D3 (YAML materializations) is in scope, so the export includes materializations.

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC Delta | Risk | Category |
|---------|------------|----------------|------|----------|
| T1: YAML parsing (FROM YAML) | Medium | ~500 (YAML schema structs + mapping + parser detection + tests) | Medium (schema mapping fidelity) | Table Stakes |
| T2: YAML file loading | Low | ~80 (file I/O + path resolution) | Low | Table Stakes |
| T3: YAML export (GET_DDL YAML) | Medium | ~400 (reverse mapping + serialization + tests) | Low-Medium (round-trip fidelity) | Table Stakes |
| T4: Materialization declaration | Medium | ~350 (model + parser + define-time validation + tests) | Low | Table Stakes |
| T5: Query-time routing | High | ~500 (matching algorithm + SQL generation + pipeline integration + tests) | Medium (WHERE passthrough, type inference) | Table Stakes |
| T6: Re-aggregation | Medium | ~300 (aggregate detection + re-agg SQL generation + tests) | Medium (heuristic aggregate detection) | Table Stakes |
| D1: Additivity metadata | Medium | ~200 (model field + parse-time detection + SHOW column) | Low | Differentiator |
| D2: Validation report | Low-Medium | ~150 (diagnostic formatting + new command) | Low | Differentiator |
| D3: YAML materializations | Low | ~100 (YAML schema extension) | Low | Differentiator |
| **Table Stakes Total** | | **~2,130 LOC** | | |
| **All Features Total** | | **~2,580 LOC** | | |

---

## MVP Recommendation

Prioritize:

1. **T1: YAML parsing** -- Core YAML capability. New crate dependency. Define the YAML schema structs. This is the foundation for T2 and T3.

2. **T2: YAML file loading** -- Trivial addition once T1 exists. High user value for version-controlled definitions.

3. **T4: Materialization declaration** -- Core materialization model. Independent of YAML. Define the MATERIALIZATIONS clause, model struct, parser, and define-time validation.

4. **T5: Query-time routing** -- The core value of materializations. Without this, T4 is just metadata. Implement the matching algorithm and SQL generation.

5. **T6: Re-aggregation** -- What makes materializations flexible. A single wide materialization can serve many queries. Implement aggregate detection and re-aggregation SQL.

6. **T3: YAML export** -- Completes the YAML round-trip. Should come after T4 is settled so YAML export can include materializations (D3).

7. **D1: Additivity metadata** -- Makes T5/T6 more robust. Worth including if time permits.

Defer:
- **D2: Validation report** -- Useful diagnostic but not required for correct functionality. Can be added in a follow-up.
- **D3: YAML materializations** -- Low complexity, but only needed after both T1 and T4 are done. Bundle with T3.

**Recommended phase ordering:**
1. **YAML Core (T1 + T2)** -- Add YAML crate dependency, define YAML schema structs, implement FROM YAML parsing and file loading. These are tightly coupled.
2. **Materialization Model (T4)** -- Add MATERIALIZATIONS clause, model, parser, validation. Independent of YAML.
3. **Materialization Routing (T5 + T6)** -- Matching algorithm + re-aggregation. The hard part. Independent of YAML.
4. **YAML Export + Materializations in YAML (T3 + D3)** -- Complete the round-trip with all features represented. Last because it must serialize everything including materializations.
5. **Polish (D1, D2)** -- Additivity metadata and diagnostics if time permits.

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [YAML specification for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- Complete YAML schema with all fields, types, constraints
- [SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML](https://docs.snowflake.com/en/sql-reference/stored-procedures/system_create_semantic_view_from_yaml) -- Procedure syntax, parameters, verify_only mode
- [SYSTEM$READ_YAML_FROM_SEMANTIC_VIEW](https://docs.snowflake.com/en/sql-reference/functions/system_read_yaml_from_semantic_view) -- YAML export function, round-trip workflow
- [CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- SQL DDL syntax (no materialization support)

### Databricks Official Documentation (HIGH confidence)

- [Materialization for metric views](https://docs.databricks.com/aws/en/metric-views/materialization) -- Materialization YAML syntax, routing algorithm, relaxed mode, refresh mechanisms
- [Metric view YAML syntax reference](https://docs.databricks.com/gcp/en/business-semantics/metric-views/yaml-reference) -- Complete YAML schema

### Cube.dev Official Documentation (HIGH confidence)

- [Matching queries with pre-aggregations](https://cube.dev/docs/product/caching/matching-pre-aggregations) -- Sequential matching algorithm, additivity checks, time dimension rules
- [Pre-aggregations reference](https://cube.dev/docs/product/data-modeling/reference/pre-aggregations) -- Definition syntax, types (rollup, original_sql, rollup_join, rollup_lambda)
- [Using pre-aggregations](https://cube.dev/docs/product/caching/using-pre-aggregations) -- Aggregating indexes, re-aggregation over missing dimensions
- [Accelerating non-additive measures](https://cube.dev/docs/product/caching/recipes/non-additivity) -- Decomposition strategies, count_distinct_approx

### dbt / MetricFlow (MEDIUM confidence)

- [Saved queries](https://docs.getdbt.com/docs/build/saved-queries) -- YAML saved query syntax, cache configuration
- [Exports](https://docs.getdbt.com/docs/use-dbt-semantic-layer/exports) -- Materialization of saved queries, schema/alias config

### Rust YAML Ecosystem (MEDIUM confidence)

- [serde_yaml deprecation discussion](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868) -- serde_yaml deprecated; alternatives: serde_yml (unsound advisory RUSTSEC-2025-0068), serde-yaml-ng, serde_yaml_bw, serde-saphyr
- [RUSTSEC-2025-0068](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) -- serde_yml advisory

### Project Internal References

- `_notes/semantic-views-duckdb-design-doc.md` -- Two-phase architecture (expansion + pre-aggregation selection), Cube.dev pre-aggregation algorithm analysis, why `egg` is not needed
- `src/model.rs` -- Current SemanticViewDefinition, Metric, Dimension, Fact structs with serde derive
- `src/render_ddl.rs` -- Existing GET_DDL SQL reconstruction
