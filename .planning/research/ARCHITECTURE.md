# Architecture: Advanced Semantic Features Integration

**Domain:** DuckDB semantic views extension -- FACTS, derived metrics, hierarchies, fan traps, role-playing dimensions, semi-additive metrics, multiple join paths
**Researched:** 2026-03-14
**Confidence:** HIGH (codebase analysis + Snowflake CREATE SEMANTIC VIEW reference + Cube.dev/MetricFlow patterns)

## Recommended Architecture

The seven features integrate as modifications to three existing subsystems (body_parser, graph, expand) plus two new subsystems (metric_resolver, fan_detection). No changes needed to the FFI layer, catalog, DDL pipeline, or query table function.

### Architecture Principle: Expansion-Only

All seven features remain within the "expansion-only" preprocessor model. The extension generates SQL; DuckDB executes it. No new connections, no new table functions, no changes to the query pipeline. Every feature translates to different SQL output from `expand()`.

### Component Map (New + Modified)

```
DDL Input
  |
  v
body_parser.rs  [MODIFY] -- add FACTS/HIERARCHIES clause parsing
  |
  v
model.rs        [MODIFY] -- add Hierarchy, NON_ADDITIVE_BY, USING relationships, derived flag
  |
  v
graph.rs        [MODIFY] -- relax diamond rejection, support named relationships, role-playing
  |
  v
metric_resolver [NEW]    -- topological sort of metric dependencies, derived expansion
  |
  v
fan_detection   [NEW]    -- detect fan-out risk from graph + metric source analysis
  |
  v
expand.rs       [MODIFY] -- facts substitution, derived metric expansion, semi-additive SQL,
                            USING relationship join path selection, hierarchy metadata
  |
  v
define.rs       [MODIFY] -- wire new validation (fan warning, metric DAG cycles)
```

### Component Boundaries

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| `body_parser.rs` | Parse FACTS/HIERARCHIES clauses, NON ADDITIVE BY, USING, derived metric syntax | model.rs (produces structs) |
| `model.rs` | Data structures: Hierarchy, metric additivity, relationship names, derived flag | All consumers |
| `graph.rs` | Named relationships, multi-path support (relaxed diamond check), role-playing table aliases | expand.rs, define.rs, fan_detection |
| `metric_resolver.rs` (NEW) | Resolve derived metric dependency DAG, topological order, cycle detection | expand.rs |
| `fan_detection.rs` (NEW) | Detect fan-out risk from graph topology + requested metrics, emit warnings | define.rs, expand.rs |
| `expand.rs` | Generate SQL with facts substitution, derived subqueries, semi-additive window functions, USING-based join path selection | query pipeline (unchanged) |
| `define.rs` | Wire fan trap warnings and metric DAG validation at CREATE time | graph.rs, metric_resolver, fan_detection |

### Data Flow

```
CREATE SEMANTIC VIEW sales_analysis AS
  TABLES (...)
  RELATIONSHIPS (
    sale_date AS o(order_date) REFERENCES d(date_key),
    ship_date AS o(ship_date) REFERENCES d(date_key)   -- role-playing
  )
  FACTS (
    o.line_total AS o.quantity * o.unit_price
  )
  HIERARCHIES (
    geo AS c.country, c.region, c.city
  )
  DIMENSIONS (...)
  METRICS (
    o.revenue AS SUM(o.line_total),                     -- references fact
    o.cost AS SUM(o.unit_cost * o.quantity),
    profit AS o.revenue - o.cost,                       -- derived metric
    o.latest_balance NON ADDITIVE BY (date_dim) AS SUM(o.balance)  -- semi-additive
  )

                            body_parser.rs
                                 |
                    parses into model structs
                                 |
                         define.rs (bind)
                        /        |        \
              graph.rs    metric_resolver  fan_detection
             (validates    (validates       (warns if
              named         metric DAG,     fan-out risk
              rels,         no cycles)      detected)
              multi-path)
                                 |
                           stored as JSON
                                 |
                     semantic_view('sales_analysis',
                       dimensions := ['region'],
                       metrics := ['profit'])
                                 |
                            expand.rs
                        /       |        \
              resolve_joins  resolve_derived  build_semi_additive
              (uses USING    (topo-sort       (window function
               named path)   metric deps)     for NON ADDITIVE)
                                 |
                        Final SQL string
                                 |
                       DuckDB executes (unchanged)
```

## Feature-by-Feature Integration

### 1. FACTS Clause

**What:** Named row-level sub-expressions scoped to a table alias. Referenced in metric expressions instead of raw column math.

**Model change:** `Fact` struct already exists in `model.rs` (scaffolded in Phase 11). No model change needed.

**Parser change:** Add `"facts"` to `CLAUSE_KEYWORDS` and `CLAUSE_ORDER` in `body_parser.rs`. Parse entries as `alias.name AS sql_expr` with optional PRIVATE modifier. Insert between RELATIONSHIPS and DIMENSIONS in clause order.

**Expansion change:** Before metric expansion, scan metric expressions for fact references (`fact_name` or `alias.fact_name`). Replace each reference with the fact's `expr` (parenthesized). This is pure text substitution on the expression string before it enters the SELECT clause.

**Graph change:** None. Facts are row-level expressions, not join-affecting.

**Validation:** At define time, verify fact `source_table` aliases exist in the TABLES clause (reuse `check_source_tables_reachable` pattern). Verify no circular references between facts.

**SQL output example:**
```sql
-- FACT: o.line_total AS o.quantity * o.unit_price
-- METRIC: o.revenue AS SUM(o.line_total)
-- Expanded:
SELECT SUM(("o"."quantity" * "o"."unit_price")) AS "revenue"
```

**Complexity:** Low. Model exists. Parser is a new clause variant following existing patterns. Expansion is string substitution.

### 2. Derived Metrics

**What:** Metrics that reference other metrics by name. E.g., `profit AS revenue - cost`. Snowflake syntax: derived metrics omit the `table_alias.` prefix.

**Model change:** Add `is_derived: bool` flag to `Metric` struct (or infer from absence of `source_table`). Derived metrics have no `source_table` -- they are computed post-aggregation.

**New component: `metric_resolver.rs`**
- Build a directed dependency graph: derived metric -> referenced metrics
- Topological sort to determine evaluation order
- Detect cycles (error at define time)
- At expand time, resolve leaf metrics first, then compose derived metrics from their results

**Expansion change:** This is the most architecturally significant feature. Derived metrics cannot appear in the same GROUP BY query as their constituent metrics -- they must be computed AFTER aggregation. Two strategies:

**Strategy A (Recommended): Expression inlining**
If derived metric `profit = revenue - cost`, and `revenue = SUM(o.amount)`, `cost = SUM(o.unit_cost * o.quantity)`, inline to produce:
```sql
SELECT
    SUM("o"."amount") AS "revenue",
    SUM("o"."unit_cost" * "o"."quantity") AS "cost",
    SUM("o"."amount") - SUM("o"."unit_cost" * "o"."quantity") AS "profit"
```
This works when derived metrics are simple arithmetic over aggregates from the same GROUP BY grain.

**Strategy B: Subquery wrapping**
When derived metrics reference metrics from different join paths (cross-path derived metrics), use a CTE or subquery:
```sql
WITH _base AS (
  SELECT region, SUM(amount) AS revenue, SUM(cost) AS cost
  FROM ...
  GROUP BY region
)
SELECT region, revenue, cost, revenue - cost AS profit FROM _base
```

Strategy A is simpler, covers 90% of cases, and avoids changing the single-query expansion model. Strategy B is needed only for cross-path derived metrics (which interact with USING RELATIONSHIPS).

**Recommendation:** Implement Strategy A first. Add Strategy B only if cross-path derived metrics are scoped in.

**Complexity:** Medium. Requires new module, DAG construction, cycle detection. But expansion change is modest for Strategy A.

### 3. Hierarchies / Drill-Down Paths

**What:** Named ordered lists of dimensions representing drill-down paths (e.g., `geo AS country, region, city`). Metadata-only -- does not change SQL generation. Used by BI tools and DESCRIBE output.

**Model change:** Add `Hierarchy` struct to `model.rs`:
```rust
pub struct Hierarchy {
    pub name: String,
    pub levels: Vec<String>,  // dimension names in drill-down order
}
```
Add `pub hierarchies: Vec<Hierarchy>` to `SemanticViewDefinition`.

**Parser change:** Add `"hierarchies"` to `CLAUSE_KEYWORDS` and `CLAUSE_ORDER`. Parse entries as `name AS dim1, dim2, dim3`. Place after FACTS, before DIMENSIONS.

**Expansion change:** None. Hierarchies are metadata. They do not affect SQL generation.

**Validation:** At define time, verify all dimension names referenced in hierarchy levels exist in the DIMENSIONS clause.

**DESCRIBE change:** Include hierarchies in `describe_semantic_view` output (new rows or new section).

**Complexity:** Low. Metadata-only feature. No expansion changes.

### 4. Fan Trap Detection

**What:** Detect when a query might produce inflated aggregation results due to one-to-many fan-out across join paths. Emit a warning (not an error) at define time or query time.

**New component: `fan_detection.rs`**

**Detection algorithm:**
1. From the `RelationshipGraph`, identify relationship cardinality. Currently the graph has edges but no cardinality annotation. Since PK/FK relationships are inherently many-to-one (FK side has many rows pointing to PK side), the graph edges encode directionality: `from_alias` (FK side, many) -> `to_alias` (PK side, one).
2. A fan trap occurs when two one-to-many paths diverge from a common ancestor, and metrics from BOTH branches are aggregated together. In graph terms: node A has edges to B and C (A is parent of both), and metrics reference columns from both B and C.
3. At define time: analyze the graph for "fan patterns" -- nodes with 2+ children where metrics span multiple children.
4. At query time (in `expand`): check if the requested metrics come from tables that fan out from a common ancestor.

**Warning, not error:** Fan traps are sometimes intentional (e.g., COUNT DISTINCT avoids double-counting). Emit a diagnostic warning, not a hard error.

**Integration points:**
- `define.rs`: call `detect_fan_traps(&def, &graph)` after graph validation, store warnings in definition metadata
- `expand.rs`: optionally re-check at query time for the specific requested dimension/metric combination

**Model change:** Add optional `warnings: Vec<String>` to definition (or return from define as side-channel).

**Complexity:** Medium. The graph analysis is straightforward (BFS from root, track descendants per branch). The challenge is accurately determining when fan-out WILL produce incorrect results vs. when it is safe (COUNT DISTINCT, MAX, MIN are fan-safe; SUM, COUNT, AVG are not).

### 5. Role-Playing Dimensions

**What:** Same physical table joined via different relationships to serve different roles. E.g., a `dates` table joined as both `order_date` and `ship_date`.

**Snowflake syntax:**
```sql
RELATIONSHIPS (
  sale_date AS orders(order_date_key) REFERENCES dates(date_key),
  ship_date AS orders(ship_date_key) REFERENCES dates(date_key)
)
```

**Model change:** The `Join` struct already has a `name: Option<String>` field (Phase 24). This is the relationship name. For role-playing, multiple `Join` entries reference the same physical table (`dates`) but with different `from_alias` FK columns and different relationship names.

The key change: each role-playing instance needs its OWN table alias. The `dates` table appears twice in the join with different aliases (e.g., `sale_date` and `ship_date` are both aliases for `dates`).

**Graph change:** The current `RelationshipGraph` uses `table.to_ascii_lowercase()` (the join target alias) as the node identifier. For role-playing, TWO joins target the same physical table but with different relationship names. The graph must:
1. Allow multiple edges to the same physical table (currently rejected as "diamond")
2. Use the RELATIONSHIP NAME as the disambiguating key, not just the table alias
3. Relax `check_no_diamonds()` to allow diamonds where relationships are explicitly named

**Implementation approach:**
- The TABLES clause declares each role-playing alias as a separate logical table:
  ```sql
  TABLES (
    o AS orders PRIMARY KEY (id),
    sale_date AS dates PRIMARY KEY (date_key),
    ship_date AS dates PRIMARY KEY (date_key)
  )
  ```
- Each gets its own alias in the graph (already supported -- `sale_date` and `ship_date` are different aliases)
- Relationships connect `o` -> `sale_date` and `o` -> `ship_date`
- Dimensions scoped to `sale_date.year` vs `ship_date.year` resolve to different join paths

**Graph change (specific):** Actually, if role-playing aliases are declared as separate TABLES entries, the current graph already handles this correctly -- `sale_date` and `ship_date` are different nodes, different aliases, no diamond. The diamond check only fires when TWO relationships point to the SAME alias, which role-playing avoids by using distinct aliases.

**Expansion change:** Minimal. The expansion already generates `LEFT JOIN "dates" AS "sale_date"` and `LEFT JOIN "dates" AS "ship_date"` from separate graph nodes. Each has its own ON clause via PK/FK synthesis.

**Complexity:** Low. The existing architecture handles this naturally via alias-per-role in TABLES. No new code needed in graph.rs -- just documentation and examples showing the pattern. The body_parser already supports multiple TABLES entries pointing to the same physical table.

### 6. Semi-Additive Metrics (NON ADDITIVE BY)

**What:** Metrics that should not be summed across specific dimensions. Instead of SUM across the non-additive dimension, take the LAST value (latest snapshot). Snowflake takes the last row when sorted by the non-additive dimensions.

**Model change:** Add to `Metric`:
```rust
pub struct NonAdditiveSpec {
    pub dimensions: Vec<String>,
    pub sort_order: Vec<SortDirection>,  // ASC/DESC per dimension
}

pub enum SortDirection {
    Asc,
    Desc,  // default: take latest
}
```
Add `pub non_additive_by: Option<NonAdditiveSpec>` to `Metric`.

**Parser change:** Parse `NON ADDITIVE BY (dim1 [ASC|DESC], dim2 [ASC|DESC])` after the metric name, before `AS`. Follows Snowflake syntax exactly.

**Expansion change:** This is the most complex SQL generation change. When a semi-additive metric is requested:

1. If the query includes ALL non-additive dimensions in its GROUP BY, the metric behaves normally (additive aggregation is safe because the non-additive dimensions are already grouped).

2. If the query OMITS some non-additive dimensions, the expansion must:
   a. Use a window function to identify the "latest" row per group
   b. Then aggregate only those latest rows

**SQL generation pattern:**
```sql
-- Semi-additive: latest_balance NON ADDITIVE BY (date_dim DESC)
-- Query: dimensions=[region], metrics=[latest_balance]

-- Step 1: Rank rows within each group by the non-additive dimension
-- Step 2: Filter to rank=1 (latest)
-- Step 3: Aggregate the filtered rows

SELECT
    "region",
    SUM("balance") AS "latest_balance"
FROM (
    SELECT
        "o"."region",
        "o"."balance",
        ROW_NUMBER() OVER (
            PARTITION BY "o"."region"
            ORDER BY "o"."date_dim" DESC
        ) AS "_rn"
    FROM "orders" AS "o"
) AS "_semi"
WHERE "_rn" = 1
GROUP BY 1
```

When MULTIPLE semi-additive metrics with different `NON ADDITIVE BY` specs are requested in the same query, each needs its own subquery. This significantly complicates the single-query expansion model.

**Recommendation:** Start with single semi-additive metric per query. If multiple are needed, use CTE-based approach:
```sql
WITH _semi_balance AS (
    SELECT region, balance,
        ROW_NUMBER() OVER (PARTITION BY region ORDER BY date_dim DESC) AS _rn
    FROM orders
),
_semi_inventory AS (
    SELECT region, inventory,
        ROW_NUMBER() OVER (PARTITION BY region ORDER BY snapshot_date DESC) AS _rn
    FROM warehouses
)
SELECT
    b.region,
    SUM(b.balance) AS latest_balance,
    SUM(i.inventory) AS latest_inventory
FROM _semi_balance b
JOIN _semi_inventory i ON b.region = i.region
WHERE b._rn = 1 AND i._rn = 1
GROUP BY 1
```

**Complexity:** High. Requires subquery wrapping, which changes the fundamental expansion pattern from single flat SELECT to potentially nested CTEs. The interaction with other joins adds further complexity.

### 7. Multiple Join Paths (USING RELATIONSHIPS)

**What:** Allow diamonds in the relationship graph when the user explicitly names relationships and specifies which path to use per metric via `USING (relationship_name)`.

**Model change:** Add `pub using_relationships: Option<Vec<String>>` to `Metric`. When present, specifies which named relationships to traverse for this metric's join resolution.

**Parser change:** Parse `USING (rel1, rel2)` after metric name, before `AS`. Follows Snowflake syntax.

**Graph change:** This is the main architectural change to `graph.rs`:
1. `check_no_diamonds()` must be relaxed: diamonds are allowed when ALL relationships involved are named (have `name: Some(...)`)
2. New validation: when diamonds exist, every metric that touches tables reachable via multiple paths MUST have a `USING` clause to disambiguate
3. `RelationshipGraph` needs to support path-specific traversal: "find the path from root to table X using only relationships R1, R2, R3"

**Expansion change:** `resolve_joins_pkfk` currently walks reverse edges unconditionally. With USING:
1. Filter the graph edges to only those whose relationship name is in the metric's `USING` list
2. Resolve joins using only the filtered subgraph
3. Different metrics in the same query might use different join paths -- this means the FROM clause could need multiple join instances of the same table

**Interaction with role-playing:** Role-playing dimensions are a special case of multiple join paths. If the user declares `sale_date` and `ship_date` as separate aliases, no diamond exists and no USING is needed. USING is needed when the same alias is reachable via multiple named relationships.

**Complexity:** High. Changes the fundamental assumption in graph.rs (tree structure). Requires per-metric join resolution instead of per-query. The expansion must handle different metrics needing different join paths in the same query -- potentially requiring subquery-per-metric with a final join.

## Patterns to Follow

### Pattern 1: Clause Extension in body_parser.rs

**What:** Adding new clauses (FACTS, HIERARCHIES) to the keyword body parser.
**When:** Any new DDL clause.
**Example:**
```rust
// 1. Add to CLAUSE_KEYWORDS
const CLAUSE_KEYWORDS: &[&str] = &[
    "tables", "relationships", "facts", "hierarchies", "dimensions", "metrics"
];

// 2. Add to CLAUSE_ORDER (defines valid ordering)
const CLAUSE_ORDER: &[&str] = &[
    "tables", "relationships", "facts", "hierarchies", "dimensions", "metrics"
];

// 3. Add parse function following existing pattern (parse_tables_clause, parse_relationships_clause)
fn parse_facts_clause(content: &str, offset: usize) -> Result<Vec<Fact>, ParseError> {
    // Split at depth-0 commas, parse each entry
}

// 4. Add to KeywordBody struct
pub struct KeywordBody {
    pub tables: Vec<TableRef>,
    pub relationships: Vec<Join>,
    pub facts: Vec<Fact>,          // NEW
    pub hierarchies: Vec<Hierarchy>, // NEW
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
}
```

### Pattern 2: Define-Time Validation Chain

**What:** New validations run at CREATE time in define.rs bind().
**When:** Any feature that can detect errors before query time.
**Example:**
```rust
// In define.rs bind():
// 1. Existing: validate_graph(&def)?
// 2. NEW: validate metric DAG
metric_resolver::validate_metric_dag(&def)?;
// 3. NEW: detect fan traps (warning, not error)
let warnings = fan_detection::detect_fan_traps(&def, &graph);
// 4. NEW: validate hierarchy dimension references
validate_hierarchies(&def)?;
```

### Pattern 3: Expression Substitution in expand.rs

**What:** Replace symbolic names in metric expressions with their underlying SQL.
**When:** Facts (name -> expr), derived metrics (metric name -> aggregate expr).
**Example:**
```rust
fn substitute_facts(expr: &str, facts: &[Fact], tables: &[TableRef]) -> String {
    let mut result = expr.to_string();
    for fact in facts {
        // Replace fact references with their expressions
        // Handle both qualified (alias.fact_name) and unqualified (fact_name)
        let qualified = format!("{}.{}", fact.source_table.as_deref().unwrap_or(""), &fact.name);
        result = result.replace(&qualified, &format!("({})", fact.expr));
        result = result.replace(&fact.name, &format!("({})", fact.expr));
    }
    result
}
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Per-Metric Subqueries When Unnecessary

**What:** Generating a separate subquery for each metric and joining them.
**Why bad:** Extremely verbose SQL, poor DuckDB query plan, unnecessary complexity.
**Instead:** Use expression inlining for derived metrics when all constituent metrics share the same GROUP BY grain. Only use subqueries when semi-additive or cross-path metrics force it.

### Anti-Pattern 2: Modifying the Query Table Function

**What:** Adding new parameters to `semantic_view()` for USING, hierarchies, etc.
**Why bad:** The table function signature is the public API. Adding parameters creates breaking changes and complexity.
**Instead:** Encode all behavior in the definition (CREATE time). The query function should remain `semantic_view('name', dimensions := [...], metrics := [...])`. USING is declared per-metric in the definition, not per-query.

### Anti-Pattern 3: Eager Diamond Rejection Without Named Relationships

**What:** Keeping the current `check_no_diamonds()` unconditionally.
**Why bad:** Blocks role-playing dimensions and legitimate multi-path schemas.
**Instead:** Relax to: diamonds allowed when all paths are named and all affected metrics have USING clauses.

### Anti-Pattern 4: Runtime Graph Construction

**What:** Building the relationship graph at query time.
**Why bad:** The graph is static per definition. Building it every query wastes CPU.
**Instead:** Build and validate at CREATE time. At query time, deserialize and traverse the pre-validated graph.

## Suggested Build Order

Build order is driven by dependencies between features and increasing complexity.

### Phase 1: FACTS Clause + Hierarchies (Low Complexity, No Graph Changes)

**FACTS:**
1. Add `"facts"` to `CLAUSE_KEYWORDS` and `CLAUSE_ORDER` in body_parser.rs
2. Add `parse_facts_clause()` following existing clause patterns
3. Wire into `KeywordBody` and `parse_keyword_body()`
4. In expand.rs, add `substitute_facts()` called before metric expression expansion
5. In define.rs, validate fact source_table aliases exist

**Hierarchies:**
1. Add `Hierarchy` struct to model.rs
2. Add `"hierarchies"` to clause keywords/order in body_parser.rs
3. Add `parse_hierarchies_clause()` parser
4. Validate hierarchy level references exist in dimensions
5. Include in DESCRIBE output

**Why first:** Both are low-risk. FACTS is needed by derived metrics. Hierarchies are metadata-only. Neither touches graph.rs or changes SQL structure.

### Phase 2: Derived Metrics (Medium Complexity, New Module)

1. Create `src/metric_resolver.rs`
2. Build metric dependency DAG from metric expressions
3. Topological sort + cycle detection
4. At expand time, determine if derived metric can be inlined (Strategy A) or needs subquery (Strategy B)
5. Implement Strategy A (expression inlining) for same-grain derived metrics
6. Wire metric DAG validation into define.rs

**Why second:** Depends on FACTS (derived metrics may reference facts). Does not depend on graph changes. Medium complexity but high value.

### Phase 3: Role-Playing Dimensions (Low Complexity, Documentation)

1. Document the pattern: separate TABLES aliases for each role
2. Add integration tests showing role-playing with current architecture
3. Verify graph.rs handles separate aliases correctly (it should already)
4. Add examples to README

**Why third:** Likely already works with existing architecture. Needs verification and documentation, not new code.

### Phase 4: Fan Trap Detection (Medium Complexity, New Module)

1. Create `src/fan_detection.rs`
2. Implement graph analysis: find nodes with 2+ child branches
3. Cross-reference with metric source_table assignments
4. Classify aggregate types (SUM/COUNT = fan-unsafe, COUNT DISTINCT/MAX/MIN = fan-safe)
5. Emit warnings at define time
6. Optionally re-check at query time for specific metric combination

**Why fourth:** Does not block other features. Can be added independently. Warning-only means no breaking changes.

### Phase 5: Semi-Additive Metrics (High Complexity, Expansion Changes)

1. Add `NonAdditiveSpec` to model.rs Metric struct
2. Parse `NON ADDITIVE BY (dim [ASC|DESC])` in body_parser.rs
3. In expand.rs, detect semi-additive metrics in the request
4. Generate subquery with ROW_NUMBER() window function
5. Wrap the main query to filter on _rn = 1 before final aggregation
6. Handle interaction with regular metrics in the same query

**Why fifth:** Highest complexity expansion change. Changes SQL output structure from flat SELECT to nested subquery. Should be built after simpler features are stable.

### Phase 6: Multiple Join Paths / USING RELATIONSHIPS (High Complexity, Graph Changes)

1. Parse `USING (rel_name)` on metrics in body_parser.rs
2. Add `using_relationships: Option<Vec<String>>` to Metric model
3. Relax `check_no_diamonds()` in graph.rs: allow diamonds when relationships are named
4. Add validation: metrics touching diamond paths must have USING
5. Modify `resolve_joins_pkfk()` in expand.rs to filter by relationship name
6. Handle per-metric join path differences in SQL generation

**Why last:** Most complex graph change. Relaxes a fundamental invariant (tree structure). Should be built last when all other features are stable.

## Scalability Considerations

| Concern | At current scale (5-10 tables) | At 50 tables | At 100+ tables |
|---------|-------------------------------|--------------|----------------|
| Graph validation | Instant | Instant (Kahn's is O(V+E)) | Instant |
| Metric DAG resolution | Instant | <1ms for 100 metrics | May need caching |
| Fan trap detection | Instant | O(V*E) acceptable | Consider memoization |
| Semi-additive subquery nesting | Single subquery | Multiple CTEs | May need query splitting |
| USING path resolution | Single traversal | Multiple per metric | Consider pre-computed paths |

All features operate at definition time or expansion time, not execution time. DuckDB handles execution optimization. The extension's overhead is string manipulation and graph traversal, which is negligible compared to query execution.

## Sources

- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- PRIMARY reference for DDL syntax, FACTS, NON ADDITIVE BY, USING, role-playing (HIGH confidence)
- [Snowflake Semantic View SQL Examples](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- Role-playing dimensions, derived metrics examples (HIGH confidence)
- [Snowflake Semantic View Validation Rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- Graph constraints, circular relationship prevention, multi-path rules (HIGH confidence)
- [Snowflake Semi-Additive Metrics Release (March 2026)](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY is a recent Snowflake addition (HIGH confidence)
- [MetricFlow / DeepWiki](https://deepwiki.com/dbt-labs/metricflow) -- Derived metric architecture, DAG-based resolution, subquery generation (MEDIUM confidence)
- [Cube.dev Measures Reference](https://cube.dev/docs/reference/data-model/measures) -- Measure composition patterns, rolling windows, multi-stage (MEDIUM confidence)
- [Cube.dev Non-Additivity Guide](https://cube.dev/docs/guides/recipes/query-acceleration/non-additivity) -- Non-additive measure strategies (MEDIUM confidence)
- [Fan Trap - Datacadamia](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) -- Fan trap definition and resolution (MEDIUM confidence)
- [Sisense Chasm and Fan Traps](https://docs.sisense.com/main/SisenseLinux/chasm-and-fan-traps.htm) -- Fan trap detection via alias/context (MEDIUM confidence)
- [Kimball Semi-Additive Facts](https://www.kimballgroup.com/data-warehouse-business-intelligence-resources/kimball-techniques/dimensional-modeling-techniques/additive-semi-additive-non-additive-fact/) -- Canonical definition of additivity (HIGH confidence)
