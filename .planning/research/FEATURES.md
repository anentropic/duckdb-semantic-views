# Feature Landscape: v0.5.3 Advanced Semantic Features

**Domain:** DuckDB Rust extension -- advanced semantic modeling capabilities
**Researched:** 2026-03-14
**Milestone:** v0.5.3 -- FACTS clause, derived metrics, hierarchies, fan trap detection, role-playing dimensions, semi-additive metrics, multiple join paths
**Status:** Subsequent milestone research (v0.5.2 shipped 2026-03-13)
**Overall confidence:** HIGH (Snowflake DDL grammar verified from official docs; Cube.dev patterns verified from official docs; dbt/MetricFlow patterns verified from official docs; existing codebase analyzed directly)

---

## Scope

This document covers the feature surface for v0.5.3: adding advanced semantic modeling capabilities to the existing DuckDB semantic views extension. Each feature is analyzed across Snowflake, Cube.dev, and dbt/MetricFlow to identify standard behavior, expected semantics, and edge cases.

**What already exists (NOT in scope):**
- 7 DDL verbs via parser hooks (CREATE, CREATE OR REPLACE, IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW)
- SQL keyword body syntax: TABLES, RELATIONSHIPS, DIMENSIONS, METRICS clauses
- PK/FK relationship model with define-time graph validation (cycles, diamonds, orphans rejected)
- Topological sort ordering and transitive join inclusion
- Qualified column references (`alias.column`) in dimension/metric expressions
- Query via `semantic_view('name', dimensions := [...], metrics := [...])`
- `Fact` struct exists in model.rs (unused -- no parser or expansion support)

**Focus:** Seven new features, their semantics across platforms, implementation complexity, and dependencies.

---

## Table Stakes

Features users expect from any semantic layer claiming "advanced modeling." Missing = the extension is a toy for single-fact-table aggregations.

### T1: FACTS Clause (Named Row-Level Sub-Expressions)

| Aspect | Detail |
|--------|--------|
| **Feature** | `FACTS (alias.fact_name AS sql_expr, ...)` -- named, unaggregated row-level expressions that metrics can reference |
| **Why Expected** | Complex metrics like `SUM(price * (1 - discount))` are verbose and error-prone when repeated. Facts let you name the sub-expression once (`net_price AS price * (1 - discount)`) and reference it in metrics (`SUM(net_price)`). Snowflake, Cube.dev, and dbt all support this concept. |
| **Complexity** | **Low-Medium** |
| **Dependencies** | Body parser (add FACTS clause). Expansion engine (inline fact expressions into metric SQL). Model (`Fact` struct already exists in `model.rs`). |
| **Existing work** | `Fact` struct with `name`, `expr`, `source_table` fields already in `model.rs`. Never parsed or expanded -- purely structural scaffolding from Phase 11. |

**How it works across platforms:**

| Platform | Name | Semantics | Key Detail |
|----------|------|-----------|------------|
| Snowflake | FACTS | Row-level expressions that dimensions and metrics can reference | FACTS clause must appear before DIMENSIONS. Facts can reference other facts and dimensions. |
| Cube.dev | `sql` on measures | Inline SQL expression | No separate "fact" concept -- measures contain their own expressions directly. |
| dbt/MetricFlow | Measures | `expr` field on a measure definition | Measures are the equivalent -- a named expression with an aggregation type. |

**Snowflake FACTS semantics (verified from official docs):**

```sql
FACTS (
  line_items.net_price AS l_extendedprice * (1 - l_discount),
  line_items.line_item_id AS CONCAT(l_orderkey, '-', l_linenumber),
  orders.count_line_items AS COUNT(line_items.line_item_id)
)
```

**Expression reference rules (Snowflake):**
- Facts CAN reference: physical columns from their source table, other facts, dimensions
- Facts CANNOT reference: metrics (aggregate-level expressions)
- Dimensions CAN reference: facts and physical columns
- Metrics CAN reference: facts, dimensions, physical columns (wrapped in aggregation functions)
- Derived metrics CAN reference: other metrics (see T2)

**Implementation approach for this extension:**
1. Add FACTS as a recognized clause keyword in `body_parser.rs` (between RELATIONSHIPS and DIMENSIONS in clause ordering)
2. Parse fact definitions with same `alias.name AS expr` syntax as dimensions
3. At expansion time, inline fact expressions into metric expressions via text substitution: if metric expr contains `fact_name`, replace with `(fact_expr)`
4. Facts are row-level -- they appear in the expanded SQL as sub-expressions inside aggregation calls, NOT as separate SELECT columns

**Edge cases:**
- **Fact referencing fact:** `net_price AS price * (1 - discount)`, then `net_value AS net_price * quantity`. Requires topological resolution of fact references -- expand inner facts first.
- **Fact and dimension with same name:** Ambiguous. Snowflake resolves by scoping: `table.fact_name` vs `table.dim_name`. Our extension should reject name collisions within the same source table scope.
- **Fact without source_table:** Same defaulting as dimensions -- falls back to base table.
- **COUNT in a fact:** Snowflake allows aggregate-level facts like `COUNT(line_items.line_item_id)`. This blurs the row-level boundary. For simplicity, initially restrict facts to row-level expressions and defer aggregate facts.

**Confidence:** HIGH (Snowflake docs verified, model struct exists)

---

### T2: Derived Metrics (Metric Referencing Other Metrics)

| Aspect | Detail |
|--------|--------|
| **Feature** | Metrics that combine other metrics without being scoped to a specific table. E.g., `profit AS revenue - cost` where `revenue` and `cost` are already-defined metrics. |
| **Why Expected** | Derived metrics are universal across semantic layers. Without them, users must manually compute composite metrics in their query results, defeating the purpose of a semantic layer. |
| **Complexity** | **Medium-High** |
| **Dependencies** | Facts (T1) should be implemented first -- facts handle row-level composition, derived metrics handle aggregate-level composition. Expansion engine needs metric dependency resolution. |

**How it works across platforms:**

| Platform | Syntax | Semantics | Key Constraint |
|----------|--------|-----------|----------------|
| Snowflake | `metric_name AS metric_a + metric_b` (no table prefix) | Unscoped metric combining table-scoped metrics | Cannot use USING clause. Cannot be referenced by regular metrics. Only another derived metric can reference a derived metric. |
| Cube.dev | Calculated measures | `sql` references other measures by name | Must resolve dependency DAG at compile time |
| dbt/MetricFlow | `derived` metric type | `expr` + `input_metrics` list | Explicit input declaration; `expr` uses metric names as variables |

**Snowflake derived metrics (verified from official docs):**

```sql
METRICS (
  orders.total_revenue AS SUM(o.amount),
  orders.total_cost AS SUM(o.cost),
  profit AS orders.total_revenue - orders.total_cost,
  margin AS profit / orders.total_revenue
)
```

**Key semantics:**
1. **No table prefix** on derived metrics -- they are scoped to the semantic view, not a logical table
2. **Cannot contain aggregation functions** -- they operate on already-aggregated values
3. **Can stack:** `margin` references `profit` which references `total_revenue` and `total_cost`
4. **Cannot be referenced by regular metrics, dimensions, or facts** -- only other derived metrics
5. **No USING clause** on derived metrics -- relationship disambiguation is not supported

**Implementation approach:**
1. **Detection:** A metric without a `source_table` (or an explicit `derived: true` flag) is treated as derived
2. **Dependency resolution:** Build a directed graph of metric references. Topological sort. Reject cycles.
3. **Expansion:** Derived metrics expand to post-aggregation expressions in the SELECT clause. The SQL looks like:

```sql
SELECT
  "c"."region" AS "region",
  SUM("o"."amount") AS "total_revenue",
  SUM("o"."cost") AS "total_cost",
  (SUM("o"."amount") - SUM("o"."cost")) AS "profit",
  ((SUM("o"."amount") - SUM("o"."cost")) / SUM("o"."amount")) AS "margin"
FROM ...
GROUP BY 1
```

Each derived metric's expression is expanded by substituting referenced metric names with their full aggregate expressions. This avoids a subquery/CTE layer.

**Edge cases:**
- **Derived metric references non-existent metric:** Define-time validation error with "did you mean" suggestion
- **Circular derived metrics:** `a AS b + 1`, `b AS a - 1` -- detect via topological sort, reject at define time
- **Division by zero in derived:** `margin AS profit / revenue` when `revenue = 0` -- user's responsibility; DuckDB produces NULL or Inf
- **Derived metric with mixed source tables:** `profit AS orders.revenue - returns.refund_total` -- requires both tables joined. The expansion must include joins for all metrics referenced transitively.
- **Query requesting only derived metric without its dependencies:** Must still compute the underlying metrics. The expansion engine resolves transitive metric dependencies, not just table joins.

**Confidence:** HIGH (Snowflake docs verified, dbt/MetricFlow pattern verified)

---

### T3: Multiple Join Paths / USING RELATIONSHIPS (Relaxes Diamond Rejection)

| Aspect | Detail |
|--------|--------|
| **Feature** | Allow multiple relationships between the same pair of tables. Metrics specify which relationship to use via `USING (relationship_name)`. Replaces the current diamond rejection. |
| **Why Expected** | The "flights with departure_airport and arrival_airport" pattern is fundamental to real-world data modeling. The current extension rejects this as a diamond. Without USING, users cannot model role-playing dimensions at all. |
| **Complexity** | **High** |
| **Dependencies** | Relationship names (already parsed and stored as `Join.name`). Graph validation changes (relax `check_no_diamonds` when USING is specified). Expansion engine changes (select specific join path). Query syntax changes (USING in query request). |

**How it works across platforms:**

| Platform | Resolution Mechanism | Query-Time Syntax | Key Detail |
|----------|---------------------|-------------------|------------|
| Snowflake | `USING (relationship_name)` on metric definitions | Metrics carry their own relationship binding | Each relationship must originate from the metric's logical table. Cannot specify multi-hop paths. |
| Cube.dev | Dijkstra shortest path + join hints / join paths | `joinHints` in REST API, `CROSS JOIN` in SQL API | Automatic disambiguation; explicit hints override |
| dbt/MetricFlow | Explicit entity relationships with join paths | Join path specified in semantic model | Multiple paths handled via entity resolution |

**Snowflake USING semantics (verified from official docs):**

```sql
RELATIONSHIPS (
  flight_departure AS flights(departure_airport) REFERENCES airports,
  flight_arrival AS flights(arrival_airport) REFERENCES airports
)

METRICS (
  flights.departure_count USING (flight_departure) AS COUNT(flight_id),
  flights.arrival_count USING (flight_arrival) AS COUNT(flight_id)
)
```

**Key semantics:**
1. **USING is on the metric definition, not the query.** The relationship binding is declared at CREATE time.
2. **Multiple named relationships to the same target table are allowed** when each has a unique name
3. **When querying without USING disambiguation, Snowflake errors:** "Multi-path relationship between... is not supported"
4. **USING specifies direct relationships only** -- cannot specify multi-hop paths like `A -> B -> C`
5. **Cannot use USING on derived metrics**

**Implementation approach:**
1. **Relax diamond rejection:** `check_no_diamonds()` should NOT reject when the diamond involves named relationships. Instead, store the multi-path information.
2. **Add `using_relationships: Option<Vec<String>>` to `Metric` model struct** (serde default = None)
3. **Body parser:** Parse `USING (rel_name, ...)` between metric name and `AS` keyword
4. **At expansion time:** When a metric has `using_relationships`, use those specific named relationships for join resolution instead of the default graph walk
5. **The same physical table gets joined multiple times with different aliases.** In the airports example: `LEFT JOIN airports AS airports_dep ON flights.departure_airport = airports_dep.airport_code LEFT JOIN airports AS airports_arr ON flights.arrival_airport = airports_arr.airport_code`

**Edge cases:**
- **USING references non-existent relationship:** Define-time validation error
- **USING references relationship from wrong source table:** Error -- each USING relationship must originate from the metric's source table
- **Dimension on the multi-path target table without USING:** Ambiguous. Snowflake errors. We should too: "dimension 'airports.city_name' is ambiguous; it is reachable via relationships 'flight_departure' and 'flight_arrival'"
- **Multiple USING on same metric:** `USING (rel_a, rel_b)` -- joins through both paths simultaneously. Each provides a different join for the metric expression.
- **Two metrics with different USING but same requested dimensions:** Both join paths must be included in the FROM clause with distinct aliases

**This is the highest-complexity feature.** It touches: model, body parser, graph validation, expansion engine (join alias management), and potentially the query request format.

**Confidence:** HIGH (Snowflake docs verified, flight/airports example is canonical)

---

## Differentiators

Features that improve DX beyond basic semantic layer parity. Not universally expected, but valued.

### D1: Role-Playing Dimensions (Same Table Joined Via Different Relationships)

| Aspect | Detail |
|--------|--------|
| **Feature** | The same physical table (e.g., `airports`) can participate in multiple relationships with different semantic roles (e.g., departure airport vs arrival airport). Dimensions from that table are disambiguated by the relationship used to reach them. |
| **Value Proposition** | Role-playing dimensions are the most common multi-path pattern. Supporting them makes the extension usable for real-world star schemas with date dimensions (order_date, ship_date, delivery_date all referencing the same `dates` table). |
| **Complexity** | **High** (tightly coupled to T3: USING RELATIONSHIPS) |
| **Dependencies** | T3 (multiple join paths) must be implemented first. Role-playing dimensions are a consequence of USING, not a separate mechanism. |

**How it works across platforms:**

| Platform | Approach | Key Detail |
|----------|----------|------------|
| Snowflake | Same table joined via multiple named relationships; metrics use USING to select path; dimensions inherit relationship context from their usage alongside metrics | Role-playing is modeled at the relationship + metric level, not the dimension level |
| Power BI / SSAS | Explicit role-playing dimension concept; only one relationship "active" at a time; DAX uses `USERELATIONSHIP()` to switch | Different paradigm -- not directly applicable to SQL-based expansion |
| Cube.dev | Not a first-class concept; handled via separate cubes wrapping the same table with different join definitions | Each "role" is a distinct cube with its own dimensions/measures |
| Traditional data warehousing | Dimension table aliased multiple times in FROM clause | The standard SQL pattern: `JOIN dates AS order_date ON ..., JOIN dates AS ship_date ON ...` |

**Implementation:**
Role-playing dimensions are NOT a separate DDL construct. They emerge from the combination of:
1. Multiple named relationships to the same table (T3)
2. Metrics with USING clauses that select a specific relationship
3. When a dimension from the multi-path target table is queried alongside a metric with USING, the dimension joins through the same relationship as the metric

The expansion engine must:
- Assign unique SQL aliases when the same physical table is joined multiple times: `airports AS "airports__flight_departure"` and `airports AS "airports__flight_arrival"`
- Qualify dimension column references with the correct alias based on relationship context

**Edge case:**
- If the user queries a dimension from airports without any metric that has USING, the query is ambiguous and should error.
- If the user queries two metrics with different USING, both pointing to the same target table, the target table gets two separate JOINs and two separate columns for the same dimension.

**Confidence:** HIGH (standard pattern, Snowflake docs verified)

---

### D2: Fan Trap Detection and Warning

| Aspect | Detail |
|--------|--------|
| **Feature** | Detect when a query crosses a one-to-many boundary in a way that could cause measure inflation (double-counting). Warn the user, but do not block the query. |
| **Value Proposition** | Fan traps are the most common source of incorrect analytics results. Even a warning helps users avoid silent data corruption. Full auto-deduplication (like Cube.dev) is complex, but detection alone is valuable. |
| **Complexity** | **Medium** |
| **Dependencies** | Relationship cardinality declarations (new). Graph traversal (existing). |

**How it works across platforms:**

| Platform | Approach | Detail |
|----------|----------|--------|
| Cube.dev | **Auto-deduplication** -- detects fan/chasm traps at query time and generates dedup subqueries using PK-based `SELECT DISTINCT` before joining | Requires `primary_key: true` on a dimension. Fully automatic. |
| Snowflake | **Granularity validation** -- validates that dimension entities have equal or lower granularity than metric entities | Prevents invalid grain combinations at query time |
| dbt/MetricFlow | **Entity-based join graph** -- join paths enforce grain through entity relationships | Implicit fan trap prevention via entity resolution |

**Fan trap definition:**
A fan trap occurs when aggregating a metric from table A while joining to table B through a one-to-many relationship. The join inflates the rows from A, causing SUM/COUNT metrics to be overcounted.

Example: `orders` (fact) JOIN `line_items` (one-to-many). If you SUM(orders.amount) while joining to line_items, each order row is duplicated per line item, inflating the sum.

**Implementation approach (detection-only, not auto-dedup):**
1. **Add relationship type metadata:** Extend `Join` model with `relationship_type: Option<String>` accepting `one_to_one`, `one_to_many`, `many_to_one`
2. **At expansion time:** When a metric's source table is on the "one" side of a one-to-many join, and the query also includes dimensions from the "many" side, emit a warning: "metric 'total_order_amount' on table 'orders' may be inflated by the one-to-many relationship to 'line_items'"
3. **Do NOT block the query.** The user may know what they are doing (e.g., they want the Cartesian product).

**Why detection-only (not auto-dedup):**
- Cube.dev's auto-dedup generates nested subqueries (`SELECT DISTINCT pk FROM fact JOIN dim ...`) that significantly change the query structure
- This requires PK awareness at query time (we have PKs from the TABLES clause, so technically feasible)
- But auto-dedup changes the semantics of the result -- it is not always what the user wants
- Detection + warning is the 80/20: catches the mistake without surprising the user

**Edge cases:**
- **No relationship type declared:** Cannot detect fan traps. Skip silently.
- **Chain of one-to-many:** `A -> B (1:M) -> C (1:M)` -- warn on any metric from A when dimensions from B or C are requested
- **Metric on the "many" side:** `SUM(line_items.price)` -- no fan trap risk when joining to `orders` (many-to-one). Only warn when aggregating on the "one" side while traversing a "one-to-many" edge.

**Confidence:** MEDIUM (Cube.dev auto-dedup pattern verified; detection-only is a simplified variant)

---

### D3: Hierarchies / Drill-Down Paths

| Aspect | Detail |
|--------|--------|
| **Feature** | Named groupings of dimensions that define drill-down paths. E.g., `location: [country, region, city]`. Metadata-only -- does not change query expansion. |
| **Value Proposition** | BI tools (Superset, Metabase, custom dashboards) can use hierarchy metadata to offer drill-down navigation. Without hierarchies, tools must guess the drill path. |
| **Complexity** | **Low** |
| **Dependencies** | None (additive metadata). |

**How it works across platforms:**

| Platform | Approach | Detail |
|----------|----------|--------|
| Cube.dev | `hierarchies` block with `levels` array | First-class concept. Cross-cube hierarchies supported (dimensions from joined cubes). |
| Snowflake | No native hierarchy concept | Dimensions are flat. Hierarchies are implicit via naming conventions. |
| dbt/MetricFlow | No native hierarchy concept | Flat dimension list. Hierarchy is a BI tool concern. |
| SSAS / Power BI | First-class hierarchy object | Dimension attributes organized into levels |

**Cube.dev hierarchies (verified from official docs):**

```javascript
hierarchies: {
  location: {
    title: 'User Location',
    levels: [state, city]
  }
}
```

**Implementation approach:**
1. **Model:** Add `Hierarchy` struct: `{ name: String, levels: Vec<String> }`. Add `hierarchies: Vec<Hierarchy>` to `SemanticViewDefinition`.
2. **DDL syntax:**
```sql
HIERARCHIES (
  location AS (country, region, city),
  time AS (year, quarter, month)
)
```
3. **Validation:** All level names must reference declared dimensions. Reject unknown dimension references.
4. **Query expansion:** Hierarchies do NOT change expansion. They are metadata stored in the definition JSON and exposed via `DESCRIBE SEMANTIC VIEW`.
5. **DESCRIBE output:** Add `hierarchies` column showing the hierarchy definitions.

**Why this is a differentiator (not table stakes):**
- Neither Snowflake nor dbt supports hierarchies natively
- Cube.dev does, but it's a metadata concept, not a query concept
- Most semantic layer users don't need hierarchies until they integrate with BI tools
- Low complexity, pure metadata -- good candidate for "free" value add

**Edge cases:**
- **Dimension in multiple hierarchies:** Allowed. `month` can appear in both `fiscal_calendar` and `calendar` hierarchies.
- **Empty hierarchy:** Reject -- a hierarchy with zero levels is meaningless.
- **Hierarchy referencing non-existent dimension:** Define-time validation error.

**Confidence:** HIGH (Cube.dev docs verified, straightforward metadata concept)

---

### D4: Semi-Additive Metrics (NON ADDITIVE BY)

| Aspect | Detail |
|--------|--------|
| **Feature** | Mark a metric as non-additive across specific dimensions. E.g., `account_balance NON ADDITIVE BY (year, month, day) AS SUM(balance)` -- the metric should use the latest snapshot value for time dimensions rather than summing across all dates. |
| **Value Proposition** | Account balances, inventory levels, and headcount are common metrics that are additive across some dimensions (e.g., customer) but not others (e.g., time). Without NON ADDITIVE BY, users get silently incorrect results. |
| **Complexity** | **High** |
| **Dependencies** | Body parser changes. Metric model changes. Expansion engine changes (window function SQL generation). |

**How it works across platforms:**

| Platform | Syntax | Semantics | SQL Generation |
|----------|--------|-----------|----------------|
| Snowflake | `NON ADDITIVE BY (dim1 DESC, dim2 DESC)` on metric | Rows sorted by non-additive dimensions; values from last rows aggregated | Not documented publicly; likely uses `QUALIFY ROW_NUMBER()` or `LAST_VALUE()` |
| dbt/MetricFlow | `non_additive_dimension: { name: dim, window_choice: max }` on measure | Filter to `MAX(dim)` per group, then aggregate | Generates subquery with window function |
| Cube.dev | `rollingWindow` or custom SQL | No first-class semi-additive concept | Manual SQL |

**Snowflake NON ADDITIVE BY (verified from official docs):**

```sql
METRICS (
  bank_accounts.m_account_balance
    NON ADDITIVE BY (year_dim DESC NULLS FIRST, month_dim DESC NULLS FIRST, day_dim DESC NULLS FIRST)
    AS SUM(balance)
)
```

**Semantics:** For customer `cust-001` in 2024, instead of summing all daily balances (incorrect: 910), return the latest day's balance (correct: 210). The "latest" is determined by sorting DESC on the non-additive dimensions and taking the last row per group.

**Implementation approach:**
1. **Model:** Add `non_additive_by: Option<Vec<NonAdditiveDim>>` to `Metric` where `NonAdditiveDim { dimension: String, descending: bool, nulls_first: Option<bool> }`
2. **Body parser:** Parse `NON ADDITIVE BY (dim1 [ASC|DESC] [NULLS FIRST|LAST], ...)` between metric name and `AS`
3. **Expansion:** When a metric has `non_additive_by`, the expanded SQL wraps the metric in a subquery with a window function:

```sql
-- Instead of:
SELECT customer_id, SUM(balance) AS account_balance FROM ...
-- Generate:
SELECT customer_id, SUM(balance) AS account_balance
FROM (
  SELECT *, ROW_NUMBER() OVER (
    PARTITION BY customer_id
    ORDER BY year DESC, month DESC, day DESC
  ) AS _rn
  FROM bank_accounts
) WHERE _rn = 1
GROUP BY customer_id
```

This filters to the latest snapshot per group before aggregating. The PARTITION BY includes all queried dimensions EXCEPT the non-additive ones.

**Edge cases:**
- **Non-additive dimension not in query:** If the user doesn't request `year_dim` in dimensions, the non-additive filter still applies but the window function partitions differently
- **Multiple non-additive metrics with different dimensions:** Each needs its own subquery/CTE
- **Non-additive dimension is from a different table:** The window function must be applied after the join but before aggregation -- requires a CTE or subquery layer
- **Combined with derived metrics:** A derived metric referencing a semi-additive metric should work -- the semi-additive metric is already resolved before derivation

**Why this is a differentiator (not table stakes):**
- Snowflake only added this in March 2026 (preview)
- Most semantic layers struggle with semi-additive metrics
- The SQL generation is non-trivial (window function + subquery)
- But the value for financial/inventory use cases is enormous

**Confidence:** HIGH (Snowflake syntax verified, dbt pattern verified, SQL generation approach is standard)

---

## Anti-Features

Features to explicitly NOT build in v0.5.3.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| **Fan trap auto-deduplication** | Cube.dev's approach generates nested subqueries that change query semantics. Complex, surprising behavior. | Detection + warning only (D2). Users decide how to handle. Document the pattern. |
| **Window function metrics** | Snowflake supports `metric AS window_function(metric) OVER (...)`. Requires fundamentally different expansion that does not GROUP BY. | Defer. Window functions are orthogonal to the aggregation model. |
| **ASOF / temporal relationships** | Range-based joins for slowly-changing dimensions. Complex temporal join semantics. | Defer. Standard equi-joins cover 95% of use cases. |
| **Cube.dev-style Dijkstra join path selection** | Automatic join path finding adds unpredictability. | Explicit USING RELATIONSHIPS is more deterministic and Snowflake-aligned. |
| **Qualified names in query syntax** | `semantic_view('v', dimensions := ['customer.name'])` with dot-qualified dimension names in queries. | Defer. Keep flat dimension names in queries. Qualified names are a DDL concern. |
| **Aggregate facts** | Snowflake allows `COUNT(line_items.id)` in FACTS -- blurs the row-level boundary. | Facts are row-level only. Aggregation belongs in METRICS. |
| **COMMENT on expressions** | Snowflake supports per-dimension/metric/fact comments. Nice but no runtime effect. | Defer. Can be added later without breaking changes. |
| **PUBLIC/PRIVATE visibility** | Snowflake marks expressions as PUBLIC or PRIVATE. No access control in DuckDB extensions. | Not applicable. |
| **WITH SYNONYMS** | Snowflake supports synonyms for AI/natural-language discovery. | Not applicable for SQL-only DuckDB. |
| **Pre-aggregation / materialization** | Deferred per PROJECT.md. | Out of scope. DuckDB handles execution. |
| **Relationship type inference from data** | Snowflake Autopilot infers one-to-many vs one-to-one from cardinality. | Users declare relationship type explicitly (if D2 is implemented). |
| **Cross-cube/cross-view hierarchies** | Cube.dev supports hierarchies referencing dimensions from joined cubes. | Keep hierarchies within a single semantic view's dimension space. |
| **Fiscal calendar support** | ISO 8601 only per PROJECT.md constraints. | Document limitation. Users handle fiscal calendars in dimension expressions. |

---

## Feature Dependencies

```
T1: FACTS clause
  |
  +-> T2: Derived Metrics (facts provide row-level composition;
  |                        derived metrics provide aggregate-level composition)
  |
  +-> D4: Semi-additive metrics (NON ADDITIVE BY references dimensions,
                                  operates on metrics -- orthogonal to facts)

T3: Multiple Join Paths (USING RELATIONSHIPS)
  |
  +-> D1: Role-Playing Dimensions (consequence of USING;
  |                                same table joined via different relationships)
  |
  +-> D2: Fan Trap Detection (requires relationship cardinality metadata;
                               can warn about one-to-many traversals)

D3: Hierarchies (independent -- pure metadata, no dependencies)
```

**Dependency ordering implications for phases:**
1. **FACTS (T1)** has no dependencies and unblocks derived metrics
2. **USING RELATIONSHIPS (T3)** has no feature dependencies but requires graph validation changes
3. **Hierarchies (D3)** is fully independent
4. **Derived Metrics (T2)** depends on T1 being designed (so metric expressions can reference facts)
5. **Role-Playing Dimensions (D1)** depends on T3
6. **Fan Trap Detection (D2)** depends on relationship type metadata (can be concurrent with T3)
7. **Semi-Additive (D4)** is independent but complex -- save for last

---

## Detailed Design: Expression Reference Hierarchy

Understanding how expressions reference each other is critical for correct expansion ordering.

### Reference Rules (Snowflake-aligned)

```
Physical Columns (base)
    |
    v
Facts (row-level named expressions)
    |  - CAN reference: physical columns, other facts, dimensions
    |  - CANNOT reference: metrics, derived metrics
    |
    v
Dimensions (row-level attribute expressions)
    |  - CAN reference: physical columns, facts
    |  - CANNOT reference: metrics, derived metrics
    |
    v
Metrics (aggregate expressions)
    |  - CAN reference: physical columns, facts, dimensions (inside aggregation)
    |  - CANNOT reference: other metrics, derived metrics
    |
    v
Derived Metrics (post-aggregation expressions)
       - CAN reference: other metrics, other derived metrics
       - CANNOT reference: physical columns, facts, dimensions directly
       - CANNOT use USING clause
       - CANNOT contain aggregation functions
```

### Expansion Order

1. Resolve fact expressions (inline sub-facts first, topological order)
2. Resolve dimension expressions (inline facts if referenced)
3. Resolve metric expressions (inline facts if referenced)
4. Resolve derived metric expressions (substitute metric expressions)
5. Generate SQL: dimensions + metrics in SELECT, derived metrics as post-aggregation expressions

---

## Detailed Design: Expansion SQL Patterns

### Pattern A: Facts + Metrics (T1)

**Definition:**
```sql
FACTS (li.net_price AS li.price * (1 - li.discount))
METRICS (o.revenue AS SUM(li.net_price))
```

**Expanded SQL:**
```sql
SELECT SUM(("li"."price" * (1 - "li"."discount"))) AS "revenue"
FROM "orders" AS "o"
LEFT JOIN "line_items" AS "li" ON "li"."order_id" = "o"."id"
```

The fact `net_price` is inlined into the metric expression. Facts never appear as separate columns.

### Pattern B: Derived Metrics (T2)

**Definition:**
```sql
METRICS (
  o.revenue AS SUM(o.amount),
  o.cost AS SUM(o.expense),
  profit AS o.revenue - o.cost
)
```

**Expanded SQL:**
```sql
SELECT
  SUM("o"."amount") AS "revenue",
  SUM("o"."expense") AS "cost",
  (SUM("o"."amount") - SUM("o"."expense")) AS "profit"
FROM "orders" AS "o"
GROUP BY 1
```

Derived metrics inline the referenced metrics' aggregate expressions.

### Pattern C: USING RELATIONSHIPS / Role-Playing Dimensions (T3 + D1)

**Definition:**
```sql
TABLES (
  f AS flights PRIMARY KEY (flight_id),
  a AS airports PRIMARY KEY (airport_code)
)
RELATIONSHIPS (
  dep AS f(departure_airport) REFERENCES a,
  arr AS f(arrival_airport) REFERENCES a
)
DIMENSIONS (
  a.city AS a.city_name
)
METRICS (
  f.departures USING (dep) AS COUNT(f.flight_id),
  f.arrivals USING (arr) AS COUNT(f.flight_id)
)
```

**Expanded SQL (querying departures + city):**
```sql
SELECT
  "a__dep"."city_name" AS "city",
  COUNT("f"."flight_id") AS "departures"
FROM "flights" AS "f"
LEFT JOIN "airports" AS "a__dep" ON "f"."departure_airport" = "a__dep"."airport_code"
GROUP BY 1
```

The airports table is joined with alias `a__dep` (combining table alias with relationship name) to disambiguate.

### Pattern D: Semi-Additive Metrics (D4)

**Definition:**
```sql
METRICS (
  b.balance NON ADDITIVE BY (date_dim DESC) AS SUM(b.balance)
)
```

**Expanded SQL (querying balance by customer):**
```sql
SELECT
  "b"."customer_id" AS "customer_id",
  SUM("b"."balance") AS "balance"
FROM (
  SELECT *, ROW_NUMBER() OVER (
    PARTITION BY "b"."customer_id"
    ORDER BY "b"."date" DESC
  ) AS "_rn"
  FROM "bank_accounts" AS "b"
) AS "b"
WHERE "_rn" = 1
GROUP BY 1
```

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC | Risk | Phase Order |
|---------|------------|----------|------|-------------|
| FACTS clause (T1) | Low-Medium | ~150 | Low -- model exists, parser pattern established | 1st |
| Hierarchies (D3) | Low | ~100 | None -- pure metadata | 1st (parallel) |
| Derived Metrics (T2) | Medium-High | ~250 | Medium -- dependency resolution, expression substitution | 2nd |
| Fan Trap Detection (D2) | Medium | ~150 | Low -- detection only, no query changes | 2nd (parallel) |
| USING RELATIONSHIPS (T3) | High | ~400 | **High** -- graph validation changes, multi-alias join expansion, relationship-scoped resolution | 3rd |
| Role-Playing Dimensions (D1) | High (coupled to T3) | ~200 | **High** -- same-table multi-join alias management | 3rd (with T3) |
| Semi-Additive NON ADDITIVE BY (D4) | High | ~300 | **High** -- window function subquery injection, changes expansion pipeline structure | 4th |
| **Total** | **High** | **~1550 lines** | **Medium-High** | |

---

## MVP Recommendation

### Wave 1: Low-Risk Foundations (estimated ~250 LOC)

Build the features that have no dependencies and low risk:

1. **FACTS clause (T1):** Parser + expansion inline. The model already exists. Re-use the DIMENSIONS parser pattern for `FACTS (alias.fact AS expr)`. Inline fact expressions at expansion time via text substitution.
2. **Hierarchies (D3):** Model + parser + DESCRIBE. Pure metadata. New `HIERARCHIES` clause keyword. Store in definition JSON. Expose via DESCRIBE output.

### Wave 2: Metric Composition (estimated ~400 LOC)

Build features that compose with Wave 1:

3. **Derived Metrics (T2):** Detect derived metrics (no source_table prefix). Build dependency DAG. Topological sort. Expand by inlining aggregate expressions. Define-time validation for cycles and unknown references.
4. **Fan Trap Detection (D2):** Add optional `relationship_type` to relationship declarations. At expansion time, check for metrics aggregating across one-to-many boundaries. Emit warnings (not errors).

### Wave 3: Multi-Path Joins (estimated ~600 LOC)

The highest-complexity features:

5. **USING RELATIONSHIPS + Role-Playing Dimensions (T3 + D1):** Relax diamond rejection for named relationships. Parse `USING (rel_name)` on metrics. Generate multi-alias JOINs. Relationship-scoped dimension resolution. This is the riskiest wave.

### Wave 4: Semi-Additive (estimated ~300 LOC)

6. **NON ADDITIVE BY (D4):** Parse `NON ADDITIVE BY (dim DESC, ...)` on metrics. Generate window function subquery at expansion time. Test with known snapshot data.

### Deferral Rationale

- **T3 + D1 before D4:** USING RELATIONSHIPS enables more use cases (role-playing dimensions are very common). Semi-additive is important but niche (financial/inventory only).
- **T1 before T2:** Facts provide the row-level composition that derived metrics build upon. Derived metrics cannot be meaningfully tested without facts.
- **D2 concurrent with T2:** Fan trap detection is independent of metric composition. Both can be developed in parallel.
- **D3 anytime:** Hierarchies are pure metadata with zero risk. Can be built in any wave.

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- full DDL grammar including FACTS, NON ADDITIVE BY, USING clauses
- [Using SQL commands for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- worked examples of FACTS, derived metrics, USING RELATIONSHIPS, role-playing dimensions, NON ADDITIVE BY
- [Semi-additive metrics release note (2026-03-05)](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY syntax and semantics
- [Derived metrics release note (2025-09-30)](https://docs.snowflake.com/en/release-notes/2025/other/2025-09-30-semantic-view-derived-metrics) -- derived metric support announcement
- [SEMANTIC_VIEW query syntax](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- query-time semantics
- [Validation rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- expression reference constraints, circular reference prohibition
- [YAML specification](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- NON ADDITIVE BY YAML syntax, using_relationships YAML syntax
- [Overview of semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/overview) -- expression hierarchy (facts vs dimensions vs metrics)

### Cube.dev Documentation (MEDIUM confidence)

- [Joins between cubes](https://cube.dev/docs/product/data-modeling/concepts/working-with-joins) -- Dijkstra join path resolution, fan/chasm trap detection, diamond subgraph handling
- [Joins reference](https://cube.dev/docs/reference/data-model/joins) -- relationship types (one_to_one, one_to_many, many_to_one), auto-dedup via PK, directed join graph
- [Hierarchies reference](https://cube.dev/docs/product/data-modeling/reference/hierarchies) -- hierarchy syntax with levels array, cross-cube hierarchies, title/public options
- [Cube.dev symmetric aggregation issue #7512](https://github.com/cube-js/cube/issues/7512) -- community discussion on fan trap handling

### dbt / MetricFlow Documentation (MEDIUM confidence)

- [About MetricFlow](https://docs.getdbt.com/docs/build/about-metricflow) -- semantic layer architecture, semi-additive measures
- [Build your metrics](https://docs.getdbt.com/docs/build/build-metrics-intro) -- metric types including derived
- [Creating metrics](https://docs.getdbt.com/docs/build/metrics-overview) -- derived metric syntax (expr + input_metrics)
- [Measures](https://docs.getdbt.com/docs/build/measures) -- non_additive_dimension configuration with window_choice

### General Semantic Layer References (LOW confidence)

- [Fan trap / chasm trap patterns](https://datacadamia.com/data/type/cube/semantic/fan_trap) -- general definition and detection approaches
- [Role-playing dimensions pattern](https://www.starschema.co.uk/post/role-playing-dimensions) -- traditional data warehousing pattern
- [Semantic Layer 2025 comparison](https://www.typedef.ai/resources/semantic-layer-metricflow-vs-snowflake-vs-databricks) -- MetricFlow vs Snowflake vs Databricks comparison

### Project Source Code (HIGH confidence -- direct analysis)

- `src/model.rs` -- `Fact` struct (exists, unused), `Metric`/`Dimension`/`Join`/`TableRef` structs
- `src/body_parser.rs` -- SQL keyword body parser (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS)
- `src/expand.rs` -- expansion engine (no fact support, no derived metric support)
- `src/graph.rs` -- `RelationshipGraph` with `check_no_diamonds()` (to be relaxed)
- `test/sql/phase28_e2e.test` -- current end-to-end test pattern (reference for new tests)
- `TECH-DEBT.md` -- resolved items 6-8 (ON-clause heuristic, unqualified names, statement rewrite)
