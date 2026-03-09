# Feature Landscape: v0.5.2 SQL DDL & PK/FK Relationships

**Domain:** DuckDB Rust extension -- proper SQL DDL grammar and Snowflake-style PK/FK relationship model
**Researched:** 2026-03-09
**Milestone:** v0.5.2 -- SQL DDL syntax + PK/FK join inference + qualified column references
**Status:** Subsequent milestone research (v0.5.1 shipped 2026-03-09)
**Overall confidence:** HIGH (Snowflake DDL grammar verified from official docs; existing parser hook architecture proven; internal model structs already contain `tables`, `join_columns`, `TableRef` fields)

---

## Scope

This document covers the feature surface for v0.5.2: replacing the function-call syntax inside `CREATE SEMANTIC VIEW` bodies with proper SQL keyword grammar (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS), adopting Snowflake-style PK/FK relationship declarations, and enabling qualified column references (`alias.column`) in dimension/metric expressions.

**What already exists (NOT in scope):**
- 7 DDL verbs via parser hooks (CREATE, CREATE OR REPLACE, IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW)
- Function-based DDL with STRUCT/LIST syntax (`create_semantic_view()`)
- Query via `semantic_view('name', dimensions := [...], metrics := [...])`
- CTE-based expansion engine with GROUP BY inference and join dependency resolution
- Error location reporting with clause hints and character positions
- Zero-copy typed output

**Focus:** New DDL body grammar, PK/FK model, join inference changes, qualified column reference support.

---

## Table Stakes

Features users expect when they see `CREATE SEMANTIC VIEW` with SQL keyword clauses. Missing = the DDL feels like a half-measure over function calls.

### T1: SQL Keyword TABLES Clause

| Aspect | Detail |
|--------|--------|
| **Feature** | `TABLES (alias AS physical_table PRIMARY KEY (col, ...), ...)` |
| **Why Expected** | Current syntax requires `:=` and struct literals (`tables := [{alias: 'o', table: 'orders'}]`), which looks alien inside a DDL statement. Users writing SQL expect SQL syntax. Snowflake uses exactly this pattern. |
| **Complexity** | **Medium** |
| **Dependencies** | Parser rewrite in `parse.rs` (new body parser); `TableRef` model struct (exists); expansion engine must resolve aliases to physical tables |
| **Notes** | PRIMARY KEY is declaration-only metadata -- not enforced at query time. It tells the relationship model which columns uniquely identify rows in this table. Snowflake also supports UNIQUE constraints; defer those. |

**Snowflake reference syntax (verified from official docs):**
```sql
TABLES (
    orders AS main.orders PRIMARY KEY (order_id),
    customers AS main.customers PRIMARY KEY (customer_id),
    line_items AS main.lineitem PRIMARY KEY (order_id, line_number)
)
```

**Current internal model (already supports this):**
```rust
pub struct TableRef { pub alias: String, pub table: String }
// PK columns need to be added to model
```

**Gap:** `TableRef` has no `primary_key` field. Need to add `pub primary_key: Vec<String>` to the model.

### T2: SQL Keyword RELATIONSHIPS Clause with FK REFERENCES

| Aspect | Detail |
|--------|--------|
| **Feature** | `RELATIONSHIPS (alias(fk_col) REFERENCES other_alias(pk_col), ...)` |
| **Why Expected** | Current relationships use either raw ON-clause strings (legacy v0.1.0) or `join_columns` struct lists. Neither reads like SQL. Snowflake's FK REFERENCES syntax is the standard pattern for declaring foreign keys in SQL. |
| **Complexity** | **Medium** |
| **Dependencies** | Parser rewrite in `parse.rs`; `Join` model struct (exists, has `join_columns: Vec<JoinColumn>`); expansion engine join resolution |
| **Notes** | The relationship declares that `orders(customer_id)` references `customers(customer_id)`. The expansion engine generates `JOIN customers ON orders.customer_id = customers.customer_id`. This replaces the ON-clause substring matching heuristic (TECH-DEBT item 6). |

**Snowflake reference syntax (verified from official docs):**
```sql
RELATIONSHIPS (
    orders_to_customers AS
        orders(customer_id) REFERENCES customers(customer_id),
    line_items_to_orders AS
        line_items(order_id) REFERENCES orders(order_id)
)
```

**Current internal model (already supports column-pair joins):**
```rust
pub struct Join {
    pub table: String,
    pub on: String,           // legacy
    pub from_cols: Vec<String>,  // Phase 11 (being replaced)
    pub join_columns: Vec<JoinColumn>,  // Phase 11.1 (current)
}
pub struct JoinColumn { pub from: String, pub to: String }
```

**Gap:** The `Join` struct models a relationship from "this table" to "base_table". The FK REFERENCES model is directional: `from_alias(from_cols) REFERENCES to_alias(to_cols)`. Need to store both alias names explicitly. The current `join.table` holds the join target -- need a `from_table` alias too.

### T3: SQL Keyword DIMENSIONS Clause

| Aspect | Detail |
|--------|--------|
| **Feature** | `DIMENSIONS (alias.dim_name AS sql_expr, ...)` |
| **Why Expected** | Current syntax: `dimensions := [{name: 'region', expr: 'region', source_table: 'customers'}]`. Snowflake uses `table_alias.dimension_name AS expression`. The qualified prefix (`alias.name`) naturally encodes the source table. |
| **Complexity** | **Low-Medium** |
| **Dependencies** | Parser rewrite; `Dimension` model struct (exists with `source_table` field) |
| **Notes** | The `alias.name` prefix replaces the separate `source_table` field. Parser extracts the alias from the qualified name. Unqualified names default to the first/base table. |

**Snowflake reference syntax (verified from official docs):**
```sql
DIMENSIONS (
    customers.customer_name AS c_name,
    orders.order_date AS o_orderdate,
    orders.order_year AS YEAR(o_orderdate)
)
```

**Mapping to current model:** `customers.customer_name AS c_name` becomes `Dimension { name: "customer_name", expr: "c_name", source_table: Some("customers") }`. Note: In Snowflake, the expression references *physical column names* from the aliased table.

### T4: SQL Keyword METRICS Clause

| Aspect | Detail |
|--------|--------|
| **Feature** | `METRICS (alias.metric_name AS agg_expr, ...)` |
| **Why Expected** | Same reasoning as dimensions. Current struct literal syntax is awkward in a DDL body. |
| **Complexity** | **Low-Medium** |
| **Dependencies** | Parser rewrite; `Metric` model struct (exists with `source_table` field) |
| **Notes** | Same pattern as dimensions -- the qualified `alias.name` prefix encodes the source table. |

**Snowflake reference syntax (verified from official docs):**
```sql
METRICS (
    customers.customer_count AS COUNT(c_custkey),
    orders.order_average_value AS AVG(o_totalprice),
    orders.total_revenue AS SUM(l_extendedprice * (1 - l_discount))
)
```

### T5: PK/FK-Based JOIN Inference (Replaces ON-Clause Heuristic)

| Aspect | Detail |
|--------|--------|
| **Feature** | Expansion engine generates JOIN ON clauses from PK/FK declarations instead of ON-clause substring matching |
| **Why Expected** | ON-clause substring matching (TECH-DEBT item 6) is a known fragility. PK/FK declarations make join semantics explicit and deterministic. Snowflake, Cube.dev, and Databricks all use explicit relationship declarations for join generation. |
| **Complexity** | **Medium** |
| **Dependencies** | TABLES clause PK declarations; RELATIONSHIPS FK declarations; `expand.rs` join generation rewrite |
| **Notes** | Given `orders(customer_id) REFERENCES customers(customer_id)`, the engine generates `LEFT JOIN customers ON orders.customer_id = customers.customer_id`. Multi-column FKs produce AND-ed ON conditions. The join type is always LEFT JOIN (same as current). |

**Current join generation (from `expand.rs`):**
- Uses `join.on` (raw SQL string) directly in the CTE
- Dependency resolution via substring matching (`table_name` appears in `on` clause)
- No PK/FK awareness

**New join generation:**
- Relationship declares `from_alias(from_cols) REFERENCES to_alias(to_cols)`
- ON clause synthesized: `from_alias.from_col = to_alias.to_col` for each column pair
- Dependency ordering from relationship graph (topological sort)
- The `on` field becomes unused (backward compat for stored JSON only)

### T6: Qualified Column References in Expressions

| Aspect | Detail |
|--------|--------|
| **Feature** | Dimension/metric expressions can use `alias.column` syntax (e.g., `SUM(orders.amount)`) |
| **Why Expected** | TECH-DEBT item 7 documents that "unqualified column names required in expressions" because the CTE flattens everything into `_base`. Qualified names are the natural SQL pattern and Snowflake requires them. |
| **Complexity** | **Medium-High** |
| **Dependencies** | Table alias registry; expansion engine CTE rewrite |
| **Notes** | This is the highest-complexity item because it requires changing the CTE expansion strategy. Currently: all tables are `SELECT *` into a single `_base` CTE, and expressions reference unqualified names. With qualified references, the expansion must either: (a) alias-prefix all columns in the CTE (e.g., `o.amount AS "orders.amount"`), or (b) use table aliases directly in the FROM clause instead of flattening. Option (b) is cleaner -- use `FROM base_table AS alias LEFT JOIN ... AS alias` and let expressions reference `alias.column` naturally. |

**Current CTE pattern (from `expand.rs`):**
```sql
WITH _base AS (
    SELECT * FROM orders
    LEFT JOIN customers ON orders.customer_id = customers.customer_id
)
SELECT region, SUM(amount) AS revenue FROM _base GROUP BY 1
```

**New pattern (table aliases in FROM, no flattening):**
```sql
SELECT c.region, SUM(o.amount) AS revenue
FROM orders AS o
LEFT JOIN customers AS c ON o.customer_id = c.customer_id
GROUP BY 1
```

This is simpler SQL (no CTE wrapper needed when there is no ambiguity), supports qualified names naturally, and produces cleaner `EXPLAIN` output.

---

## Differentiators

Features that improve DX beyond Snowflake parity. Not expected, but valued.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| **Transitive relationship resolution** | If A references B and B references C, requesting dimensions from A and C automatically joins through B. Snowflake and Cube.dev both support this. | **Medium** | Relationship graph; topological sort | Already partially implemented (`collect_transitive_dependencies` in expand.rs). Needs to work with new PK/FK model. |
| **Relationship naming** | `orders_to_customers AS orders(fk) REFERENCES customers(pk)` -- the name is informational/documentary. Snowflake supports optional names. | **Low** | Parser; stored but not used at query time | Low effort, good self-documentation. |
| **Composite primary keys** | `PRIMARY KEY (order_id, line_number)` for junction tables. Snowflake supports this. | **Low** | Model: `Vec<String>` for PK columns | Already natural from the Vec representation. |
| **Multi-column foreign keys** | `line_items(order_id, line_number) REFERENCES order_details(order_id, line_number)` | **Low** | Already supported by `join_columns: Vec<JoinColumn>` | The model already handles this. Parser needs to support the list. |
| **Backward-compatible function syntax** | Keep `create_semantic_view()` with STRUCT/LIST args working alongside new SQL DDL | **Low** | No changes to existing path | The statement rewrite produces function calls. Old JSON still deserializes. |
| **FACTS clause** | `FACTS (alias.fact_name AS raw_expr)` -- pre-aggregation facts that other expressions can reference. Snowflake has this. | **Medium** | `Fact` model struct (exists); parser; expansion | Already in the model. Snowflake uses FACTS as intermediate named expressions referenced by metrics. Useful for complex metrics like `SUM(line_items.price * (1 - line_items.discount))` -- define a fact `line_items.net_price AS price * (1 - discount)`, then `SUM(line_items.net_price)`. |

---

## Anti-Features

Features to explicitly NOT build in v0.5.2.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| **UNIQUE constraints on tables** | Snowflake supports `UNIQUE(col)` alongside `PRIMARY KEY`. Adds complexity for marginal value -- PK suffices for join inference. | Defer. PK is the join key. |
| **ASOF / temporal relationships** | Snowflake (as of Feb 2026 preview) supports `ASOF` in REFERENCES and `BETWEEN start AND end EXCLUSIVE` for range joins. Complex temporal join semantics. | Defer to future milestone. Standard equi-joins cover 95% of use cases. |
| **DISTINCT RANGE constraints** | Snowflake supports range-based constraints for slowly-changing dimension tables. Niche use case. | Defer. |
| **NON ADDITIVE BY clause on metrics** | Snowflake allows `NON ADDITIVE BY (dimension)` to mark metrics that cannot be freely aggregated across certain dimensions. Requires query-time validation. | Defer. All metrics treated as additive for now. Document limitation. |
| **Window function metrics** | Snowflake supports `metric AS window_function(metric) OVER (...)`. Requires special expansion that does not GROUP BY. | Defer to future milestone. |
| **WITH SYNONYMS** | Snowflake supports `WITH SYNONYMS = ('alias1', 'alias2')` for AI/natural-language discovery. Not relevant for SQL-only DuckDB. | Not applicable. No AI query interface. |
| **COMMENT on expressions** | Snowflake supports per-dimension/metric comments. Nice for documentation but no runtime effect. | Defer. Comments can be added later without breaking changes. |
| **AI_SQL_GENERATION / AI_QUESTION_CATEGORIZATION** | Snowflake-specific AI integration directives. | Not applicable. |
| **COPY GRANTS** | Snowflake permission model. DuckDB has no grant system for extension objects. | Not applicable. |
| **PUBLIC/PRIVATE visibility** | Snowflake supports marking dimensions/metrics as PUBLIC or PRIVATE. No access control in DuckDB extensions. | Not applicable. |
| **Automatic relationship type inference** | Snowflake's Autopilot infers one-to-many vs one-to-one from data cardinality. Adds query-time analysis overhead. | All joins are LEFT JOIN. The user declares the direction via FK REFERENCES. |
| **Circular relationship detection** | Snowflake prohibits circular relationships and validates at CREATE time. | Validate at CREATE time -- but keep it simple: check the relationship graph is a DAG. Do NOT build complex cycle-detection beyond basic topological sort failure. |
| **Multiple join paths between same tables** | Snowflake requires separate logical table entries for each path. Complex disambiguation. | Error if two relationships connect the same pair. Defer multi-path to future. |
| **Cube.dev-style fan/chasm trap detection** | Cube detects fan traps (one-to-many causing double-counting) and generates deduplication subqueries. Requires primary key dedup logic. | Defer. Users are responsible for correct join topology. Document that metrics from many-side tables need care. |
| **Databricks-style YAML definitions** | Databricks metric views use YAML, not SQL DDL. | SQL DDL first. YAML is out of scope per PROJECT.md. |
| **Derived metrics (metric referencing metric)** | Snowflake supports metrics that reference other metrics. Requires expression dependency resolution. | Defer to future milestone (already noted in PROJECT.md Out of Scope). |
| **Hierarchies / drill-down paths** | Dimensional hierarchies (country -> region -> city). | Defer to future milestone (already noted in PROJECT.md Out of Scope). |
| **Qualified names in SEMANTIC_VIEW query syntax** | Snowflake allows `SEMANTIC_VIEW(... METRICS customer.order_count DIMENSIONS customer.name)` with qualified names in the query function too. | Defer. Keep `semantic_view('name', dimensions := ['region'], metrics := ['revenue'])` for now. Qualified query names are a separate concern. |

---

## Detailed Design: DDL Body Grammar

### Current State (v0.5.1)

The DDL body uses DuckDB function-call syntax because `rewrite_ddl` passes it verbatim to the underlying `create_semantic_view()` function:

```sql
CREATE SEMANTIC VIEW tpch_analysis (
    tables := [{alias: 'o', table: 'orders'}, {alias: 'c', table: 'customers'}],
    relationships := [{from_table: 'o', to_table: 'c', join_columns: [{from: 'customer_id', to: 'id'}]}],
    dimensions := [{name: 'region', expr: 'region', source_table: 'c'}],
    metrics := [{name: 'revenue', expr: 'sum(amount)', source_table: 'o'}]
)
```

### Target State (v0.5.2)

Proper SQL keyword syntax inside the DDL body:

```sql
CREATE SEMANTIC VIEW tpch_analysis (
    TABLES (
        o AS orders PRIMARY KEY (order_id),
        c AS customers PRIMARY KEY (customer_id)
    )
    RELATIONSHIPS (
        o(customer_id) REFERENCES c(customer_id)
    )
    DIMENSIONS (
        c.region AS c_region,
        o.order_date AS o_orderdate
    )
    METRICS (
        o.revenue AS SUM(o_totalprice),
        o.item_count AS COUNT(*)
    )
)
```

### Parser Strategy

The `rewrite_ddl` function currently passes the body verbatim to the function call. For v0.5.2, it must:

1. **Parse** the SQL keyword body into structured data
2. **Translate** into the existing `SemanticViewDefinition` model (or directly into the function-call syntax that `create_semantic_view()` expects)
3. **Rewrite** to `SELECT * FROM create_semantic_view('name', tables := [...], ...)` with STRUCT/LIST literals

This keeps the function-based DDL as the internal execution target while presenting SQL syntax to users. The parser is a translator, not a new execution path.

**Alternative:** Parse directly into `SemanticViewDefinition` JSON and call a new internal function that accepts JSON. This avoids generating STRUCT/LIST literals. Simpler code generation, but adds a new code path.

**Recommendation:** Translate to STRUCT/LIST function-call syntax. Reuses the proven `parse_args.rs` pipeline. No new execution path. The generated syntax is ugly but never user-facing.

### Grammar Specification

```
body ::= clause { clause }
clause ::= TABLES_clause | RELATIONSHIPS_clause | DIMENSIONS_clause | METRICS_clause

TABLES_clause ::= 'TABLES' '(' table_def { ',' table_def } ')'
table_def ::= alias 'AS' physical_table [ 'PRIMARY' 'KEY' '(' column { ',' column } ')' ]

RELATIONSHIPS_clause ::= 'RELATIONSHIPS' '(' rel_def { ',' rel_def } ')'
rel_def ::= [ rel_name 'AS' ] from_alias '(' column { ',' column } ')' 'REFERENCES' to_alias '(' column { ',' column } ')'

DIMENSIONS_clause ::= 'DIMENSIONS' '(' dim_def { ',' dim_def } ')'
dim_def ::= [ alias '.' ] dim_name 'AS' sql_expr

METRICS_clause ::= 'METRICS' '(' metric_def { ',' metric_def } ')'
metric_def ::= [ alias '.' ] metric_name 'AS' sql_expr
```

**Parsing complexity:** Medium. The grammar is LL(1) for clause detection. The tricky parts are: (a) sql_expr can contain parentheses, commas inside function calls, and string literals; (b) separating items within a clause requires tracking nesting depth.

### Expression Parsing Challenge

The `AS sql_expr` portion of dimensions and metrics is free-form SQL. The parser must find where one definition ends and the next begins. Strategy:

- Items are comma-separated at depth 0 (outside any parentheses)
- The parser tracks paren depth and string literal state
- A comma at depth 0 separates items
- The closing `)` of the clause ends the last item

This is the same approach already used in `scan_clause_keywords` for bracket tracking.

---

## Detailed Design: Expansion Engine Changes

### Current CTE Flattening

```rust
// expand.rs: build_base_cte()
// SELECT * FROM base_table LEFT JOIN t2 ON ... LEFT JOIN t3 ON ...
```

All tables merged into `_base` CTE. Expressions must use unqualified column names.

### New Alias-Based FROM Clause

With table aliases and PK/FK relationships, the expansion shifts from CTE flattening to alias-based JOINs:

```sql
SELECT c.c_region AS region, SUM(o.o_totalprice) AS revenue
FROM orders AS o
LEFT JOIN customers AS c ON o.customer_id = c.customer_id
WHERE 1=1
GROUP BY 1
```

**Benefits:**
- Qualified column names work naturally (`o.amount`, `c.region`)
- Simpler generated SQL (no CTE wrapper)
- Cleaner EXPLAIN output
- Column name ambiguity resolved by alias prefix

**Backward compatibility:** Definitions created with v0.5.1 (using unqualified names and ON-clause strings) must still work. Detection: if `def.joins[i].on` is non-empty (legacy format), use the old CTE-based expansion. If `def.joins[i].join_columns` is populated and `def.tables` is non-empty, use the new alias-based expansion.

### Join Dependency Resolution

**Current:** Substring matching -- check if a table name appears in ON clauses of other joins.

**New:** Relationship graph. Each relationship is a directed edge from FK table to PK table. Topological sort determines join order. When a query requests dimensions/metrics from tables A and C, and A->B->C in the relationship graph, all intermediate tables (B) are included in the JOIN chain.

This is the transitive dependency resolution already sketched in `collect_transitive_dependencies`, but formalized with explicit graph edges from FK declarations.

---

## Comparison: Snowflake vs Cube.dev vs Databricks

| Aspect | Snowflake | Cube.dev | Databricks | This Extension (v0.5.2) |
|--------|-----------|---------|------------|------------------------|
| **Definition format** | SQL DDL | YAML/JS | YAML | SQL DDL |
| **Table declarations** | `alias AS table PK(col)` | Implicit from cube name | `source: table` | `alias AS table PRIMARY KEY(col)` |
| **Relationships** | `FK REFERENCES PK` | `joins: { sql, relationship }` | `ON` / `USING` | `FK REFERENCES PK` |
| **Join inference** | Auto from PK/FK + cardinality | Dijkstra shortest path | Explicit ON/USING | From PK/FK declarations |
| **Join type** | Inferred (one-to-one, many-to-one) | Always LEFT JOIN | Explicit (LEFT/INNER) | Always LEFT JOIN |
| **Fan trap handling** | Granularity validation | Auto-dedup via PK | Not documented | Not handled (v0.5.2) |
| **Qualified refs** | `alias.column` required | `CubeName.dimension` | `source.column` / `join.column` | `alias.column` supported |
| **Metrics syntax** | `alias.name AS AGG(expr)` | `measures: { sql, type }` | `measures: [{ name, agg, expr }]` | `alias.name AS AGG(expr)` |
| **Query syntax** | `SEMANTIC_VIEW(... METRICS ... DIMENSIONS ...)` | REST API / SQL API | `SELECT ... FROM metric_view(...)` | `semantic_view('name', dimensions := [...], metrics := [...])` |

**Key takeaway:** Snowflake is the closest design reference. Cube.dev's Dijkstra join path selection is more sophisticated but requires a full graph model. Databricks uses YAML, which is out of scope. Adopt Snowflake's DDL grammar with simplifications (no UNIQUE, no ASOF, no visibility modifiers).

---

## Feature Dependencies

```
SQL Body Parser (new)
  |
  +-> TABLES clause parser --> TableRef + PK fields in model
  |     |
  |     +-> RELATIONSHIPS clause parser --> Join with FK REFERENCES in model
  |           |
  |           +-> PK/FK join inference in expand.rs (replaces ON-clause heuristic)
  |
  +-> DIMENSIONS clause parser --> Dimension with alias.name -> source_table
  |     |
  |     +-> Qualified column references in expand.rs
  |           (requires alias-based FROM instead of CTE flattening)
  |
  +-> METRICS clause parser --> Metric with alias.name -> source_table
        |
        +-> Same qualified column support as dimensions

Expansion engine rewrite
  |
  +-> Alias-based FROM clause (replaces CTE _base flattening)
  +-> PK/FK-based ON clause generation
  +-> Topological sort for join ordering
  +-> Backward compat: legacy ON-clause definitions still work
```

**Critical path:** SQL body parser -> model changes -> expansion engine rewrite. These are sequential dependencies.

**Parallel work:** TABLES and RELATIONSHIPS parsers can be built together. DIMENSIONS and METRICS parsers are nearly identical and can share code.

---

## MVP Recommendation

### Wave 1: Model & Parser (Foundation)

1. **Extend model:** Add `primary_key: Vec<String>` to `TableRef`. Add `from_table: String` to `Join` (or refactor to a `Relationship` struct with `from_alias`, `from_cols`, `to_alias`, `to_cols`).
2. **SQL body parser:** Parse TABLES, RELATIONSHIPS, DIMENSIONS, METRICS clauses from the DDL body into the model structs.
3. **Translate to function-call syntax:** Generate the STRUCT/LIST literal form for `create_semantic_view()`.
4. **Detect syntax variant:** If body starts with a clause keyword (`TABLES`, `DIMENSIONS`, etc.), use new parser. If body starts with `:=` argument syntax, use old verbatim passthrough. This provides backward compatibility.

### Wave 2: Expansion Engine (Core Value)

5. **Alias-based FROM generation:** Replace CTE flattening with `FROM base AS alias LEFT JOIN t2 AS alias2 ON ...` when table aliases exist.
6. **PK/FK ON clause synthesis:** Generate `ON alias1.col = alias2.col` from relationship declarations.
7. **Topological join ordering:** Order JOINs based on relationship graph.
8. **Backward compat path:** Keep old CTE-based expansion for definitions without table aliases.

### Wave 3: Testing & Polish

9. **SQL logic tests:** New `.slt` files exercising SQL DDL syntax, multi-table joins, qualified column references.
10. **Property-based tests:** Roundtrip parser tests (parse SQL -> model -> function-call syntax -> parse back).
11. **Error messages:** Clause-aware errors for the new SQL grammar (line/column in DDL body).

**Defer from v0.5.2:** FACTS clause (model exists, parser can be added later), relationship naming (parse and store but ignore), ASOF/temporal joins, fan trap detection, derived metrics.

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC | Risk |
|---------|------------|----------|------|
| Model changes (PK on TableRef, refactored Join) | Low | ~40 | Low -- additive fields with serde defaults |
| SQL body parser (TABLES clause) | Medium | ~120 | Medium -- free-form table names, PK syntax |
| SQL body parser (RELATIONSHIPS clause) | Medium | ~100 | Medium -- FK REFERENCES grammar |
| SQL body parser (DIMENSIONS/METRICS clauses) | Medium | ~150 | Medium -- AS sql_expr boundary detection |
| Syntax variant detection | Low | ~20 | Low -- check first token |
| Translate to function-call syntax | Low-Medium | ~80 | Low -- string generation |
| Alias-based FROM expansion | Medium-High | ~200 | **Medium-High** -- replaces core expansion, must maintain backward compat |
| PK/FK ON clause synthesis | Low-Medium | ~60 | Low -- direct column-pair mapping |
| Topological join ordering | Medium | ~80 | Low -- standard graph algorithm |
| Backward compat for legacy definitions | Medium | ~60 | Medium -- dual-path expansion |
| SQL logic tests (new syntax) | Low | ~150 | None |
| Property-based tests (parser roundtrip) | Medium | ~100 | None |
| **Total** | **Medium-High** | **~1160 lines** | **Medium** -- expansion engine rewrite is the riskiest piece |

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- full DDL grammar with TABLES, RELATIONSHIPS, DIMENSIONS, METRICS, FACTS clauses
- [SEMANTIC_VIEW query syntax](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- SEMANTIC_VIEW() query clause with qualified dimension/metric references
- [Overview of semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/overview) -- logical table / entity / relationship model
- [Validation rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- circular reference prohibition, expression scoping rules, PK/FK validation, granularity checks
- [Using SQL commands for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- complete worked examples with TPC-H data
- [YAML specification](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- alternative definition format
- [Querying semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- qualified/unqualified name resolution, wildcard metrics

### Cube.dev Documentation (MEDIUM confidence)

- [Joins between cubes](https://cube.dev/docs/product/data-modeling/concepts/working-with-joins) -- Dijkstra join path resolution, fan/chasm trap detection
- [Joins reference](https://cube.dev/docs/product/data-modeling/reference/joins) -- `one_to_one`, `one_to_many`, `many_to_one` relationship types, LEFT JOIN default
- [Cube reference](https://cube.dev/docs/product/data-modeling/reference/cube) -- `primary_key` requirement in dimensions for deduplication
- [GitHub issue #1179](https://github.com/cube-js/cube/issues/1179) -- auto-adding joins via PK/FK (community request, not implemented)

### Databricks Documentation (MEDIUM confidence)

- [Unity Catalog metric views](https://docs.databricks.com/aws/en/metric-views/) -- YAML-based semantic layer
- [Model metric view data](https://docs.databricks.com/aws/en/metric-views/data-modeling/) -- dimensions, measures, joins, filters
- [Use joins in metric views](https://docs.databricks.com/aws/en/metric-views/data-modeling/joins) -- ON/USING join syntax, star/snowflake schema support

### Project Source Code (HIGH confidence -- direct analysis)

- `src/model.rs` -- `TableRef`, `Join`, `JoinColumn`, `Dimension`, `Metric`, `Fact`, `SemanticViewDefinition`
- `src/parse.rs` -- `scan_clause_keywords`, `validate_clauses`, `rewrite_ddl`, `DdlKind`
- `src/expand.rs` -- CTE-based expansion, `collect_transitive_dependencies`, `suggest_closest`
- `src/ddl/parse_args.rs` -- STRUCT/LIST argument extraction from DuckDB BindInfo
- `TECH-DEBT.md` -- items 6 (ON-clause substring matching), 7 (unqualified column names), 8 (statement rewrite approach)
