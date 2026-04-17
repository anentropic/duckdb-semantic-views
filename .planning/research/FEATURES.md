# Feature Landscape: v0.6.0 Snowflake SQL DDL Parity

**Domain:** DuckDB Rust extension -- Snowflake SQL DDL parity features for semantic views
**Researched:** 2026-04-09
**Milestone:** v0.6.0 -- Close all remaining feature gaps against Snowflake's SQL DDL semantic views
**Status:** Subsequent milestone research (v0.5.5 shipped 2026-04-05)
**Overall confidence:** HIGH (Snowflake DDL syntax, SHOW/DESCRIBE schemas, and query semantics verified from official docs; dbt MetricFlow semi-additive pattern cross-referenced)

---

## Scope

This document covers the 9 target feature areas for v0.6.0. Each is assessed against Snowflake's implementation, comparable systems (dbt MetricFlow, Cube.dev), and the existing codebase.

**What already exists (NOT in scope for research):**
- CREATE/DROP/ALTER RENAME/CREATE OR REPLACE SEMANTIC VIEW
- TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS clauses
- PK/FK/UNIQUE relationships with cardinality inference
- Standard aggregate metrics (SUM, COUNT, AVG, MIN, MAX, etc.)
- Derived metrics (metric-on-metric composition)
- Fan trap detection, role-playing dimensions, USING RELATIONSHIPS
- SHOW SEMANTIC VIEWS/DIMENSIONS/METRICS/FACTS with LIKE/STARTS WITH/LIMIT/IN
- DESCRIBE SEMANTIC VIEW (property-per-row, 5 columns, 6 object kinds)
- Metadata storage (created_on, database_name, schema_name)
- 487 tests, 16,342 LOC

**Focus:** New DDL features, query capabilities, and introspection enhancements.

---

## Table Stakes

Features that Snowflake's SQL DDL semantic views support and that users working toward parity will expect. Missing = incomplete Snowflake alignment claim.

### T1: Semi-Additive Metrics (NON ADDITIVE BY)

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `NON ADDITIVE BY (dim1, dim2, ...)` on metric definitions. At query time, instead of aggregating across the named dimensions, sort by them and take the last (most recent) value before aggregating. |
| **Why Expected** | Snowflake shipped NON ADDITIVE BY on 2026-03-05. dbt MetricFlow has `non_additive_dimension` with `window_choice: max/min`. Semi-additive measures are a fundamental data warehouse concept (account balances, inventory levels, MRR). Without this, users cannot correctly model snapshot data. |
| **Complexity** | **High** -- requires structural expansion pipeline changes |
| **Dependencies** | None on other v0.6.0 features; self-contained expansion path change |

**How Snowflake does it:**

DDL syntax:
```sql
METRICS (
  bank.m_account_balance
    NON ADDITIVE BY (year_dim, month_dim, day_dim)
    AS SUM(balance)
)
```

The `NON ADDITIVE BY` dimensions have optional sort order (`ASC|DESC`) and `NULLS FIRST|LAST`:
```
NON ADDITIVE BY (
  <dimension> [ { ASC | DESC } ] [ NULLS { FIRST | LAST } ]
  [ , ... ]
)
```

At query time: rows are sorted by the non-additive dimensions (descending by default for "latest snapshot" semantics), and the values from the last rows are aggregated. This means for a SUM(balance) metric that is NON ADDITIVE BY (date_dim), the system takes the last date's balance per group and sums those.

**How dbt MetricFlow does it:**

```yaml
measures:
  - name: account_balance
    agg: sum
    expr: balance
    non_additive_dimension:
      name: date
      window_choice: max  # or min
      window_groupings:
        - customer_id
```

MetricFlow generates a CTE with `ROW_NUMBER() OVER (PARTITION BY [groupings] ORDER BY [dim] DESC)` and filters to `rn = 1` before aggregating. This is the standard SQL pattern for semi-additive measures.

**Expansion SQL pattern (what this extension should generate):**

For `SUM(balance) NON ADDITIVE BY (date_dim)` with requested dimensions `[customer_id, date_dim]`:
```sql
-- Standard aggregation: GROUP BY customer_id, date_dim
-- Semi-additive: no change when ALL non-additive dims are in the query

-- But when date_dim is NOT in the requested dimensions:
WITH _dedup AS (
  SELECT *, ROW_NUMBER() OVER (
    PARTITION BY customer_id  -- remaining dimensions
    ORDER BY date_dim DESC    -- non-additive dims, descending
  ) AS _rn
  FROM base_table
)
SELECT customer_id, SUM(balance)
FROM _dedup WHERE _rn = 1
GROUP BY customer_id
```

**Key design decisions needed:**
1. **Default sort order:** Snowflake uses ASC by default for NON ADDITIVE BY dimensions, but "last row" semantics typically means DESC. Need to verify the exact Snowflake behavior vs implement the sensible default (DESC = latest).
2. **Interaction with fan traps:** Semi-additive metrics with JOINs need the dedup CTE before the JOIN, or the fan trap detection needs to account for the dedup.
3. **Interaction with derived metrics:** A derived metric referencing a semi-additive metric should inherit the semi-additive behavior (the base metric's dedup applies first).

**Model changes required:**
```rust
pub struct Metric {
    // ... existing fields ...
    /// Dimensions that this metric is non-additive across.
    /// Empty = fully additive (default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_additive_dims: Vec<NonAdditiveDim>,
}

pub struct NonAdditiveDim {
    pub dimension: String,
    pub descending: bool,       // default true for "latest"
    pub nulls_last: bool,       // default true
}
```

**Parser changes:** Body parser must recognize `NON ADDITIVE BY (...)` between metric name and `AS` expression.

**Expansion changes:** This is the hard part. The expansion pipeline currently generates a single `SELECT ... FROM ... GROUP BY ...`. Semi-additive metrics require a CTE-based pre-filter when non-additive dimensions are excluded from the query. The expansion engine must:
1. Detect which requested dimensions overlap with non-additive dimensions
2. If ALL non-additive dims are in the query: standard expansion (no change)
3. If SOME/NONE non-additive dims are in the query: wrap with ROW_NUMBER CTE

**Confidence:** HIGH for Snowflake syntax; MEDIUM for expansion approach (standard SQL pattern, but integration with existing JOIN/fan-trap/USING machinery needs design)

---

### T2: COMMENT on Views and Objects

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `COMMENT = '...'` on the semantic view itself, on tables, dimensions, metrics, and facts in the DDL. Surface comments in DESCRIBE and SHOW output. Support `ALTER SEMANTIC VIEW SET COMMENT = '...'` and `UNSET COMMENT`. |
| **Why Expected** | Snowflake supports COMMENT at every level of the semantic view. Comments are the primary documentation mechanism for semantic models. Without comments, the semantic layer cannot serve as a self-documenting data catalog. |
| **Complexity** | **Medium** -- pervasive but shallow; touches parser, model, catalog, SHOW, DESCRIBE |
| **Dependencies** | None |

**Snowflake's exact DDL syntax:**

View-level:
```sql
CREATE SEMANTIC VIEW my_view
  COMMENT = 'Revenue analysis view'
  TABLES (...)
  ...
```

Object-level (tables, dimensions, facts, metrics):
```sql
TABLES (
  orders AS my_schema.orders
    PRIMARY KEY (order_id)
    COMMENT = 'All customer orders'
)
DIMENSIONS (
  orders.order_date AS orders.created_at
    COMMENT = 'Date the order was placed'
)
METRICS (
  orders.total_revenue AS SUM(orders.amount)
    COMMENT = 'Total revenue across all orders'
)
FACTS (
  orders.amount_fact AS orders.amount
    COMMENT = 'Raw order amount'
)
```

ALTER syntax:
```sql
ALTER SEMANTIC VIEW my_view SET COMMENT = 'Updated description'
ALTER SEMANTIC VIEW my_view UNSET COMMENT
```

**DESCRIBE output:** Comments appear as a COMMENT property row for each object_kind. The view-level comment appears with `object_kind = NULL, property = 'COMMENT'`.

**SHOW output:** Snowflake's SHOW SEMANTIC VIEWS has a `comment` column (position 6 of 8). SHOW SEMANTIC DIMENSIONS/METRICS/FACTS have a `comment` column (position 8 of 8).

**Model changes required:**

```rust
pub struct SemanticViewDefinition {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,  // view-level comment
}

pub struct TableRef {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

pub struct Dimension {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

// Same for Metric and Fact
```

**Parser changes:** Recognize `COMMENT = '...'` after object definitions. The body parser state machine needs a new trailing-clause recognition for each entry type.

**ALTER changes:** Extend `DdlKind` to handle `ALTER SEMANTIC VIEW <name> SET COMMENT = '...'` and `UNSET COMMENT`. The ALTER currently only supports RENAME TO. Need to add SET/UNSET variants.

**SHOW changes:** Add `comment` column to all SHOW commands (SHOW VIEWS position 6; SHOW DIMS/METRICS/FACTS position 8). This is additive -- new column at end.

**DESCRIBE changes:** Emit COMMENT property row for each object that has one. View-level comment emitted with `object_kind = NULL`.

**Confidence:** HIGH (straightforward string metadata; Snowflake syntax well-documented)

---

### T3: SYNONYMS on Objects

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `WITH SYNONYMS = ('alias1', 'alias2')` on tables, dimensions, metrics, and facts in the DDL. |
| **Why Expected** | Snowflake supports synonyms on all object types. Synonyms serve as alternative names for Cortex Analyst / AI-powered querying. They appear in DESCRIBE and SHOW output. |
| **Complexity** | **Low-Medium** -- same pattern as COMMENT but with list values |
| **Dependencies** | None |

**Snowflake's exact DDL syntax:**

```sql
TABLES (
  orders AS my_schema.orders
    PRIMARY KEY (order_id)
    WITH SYNONYMS = ('sales_orders', 'purchase_records')
)
DIMENSIONS (
  orders.cust_name AS orders.customer_name
    WITH SYNONYMS = ('customer_name', 'buyer_name')
)
```

**Important Snowflake note:** "Synonyms are used for informational purposes only. You cannot use a synonym to refer to a dimension, fact, or metric in another dimension, fact, or metric."

This means synonyms are pure metadata -- they do not affect query resolution or expansion. They exist for documentation and AI-powered natural language query interfaces.

**Model changes required:**

```rust
pub struct Dimension {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
}
// Same for TableRef, Metric, Fact
```

**SHOW output:** Snowflake's SHOW SEMANTIC DIMENSIONS/METRICS/FACTS have a `synonyms` column (position 7 of 8), rendered as a JSON array string like `["cust_name", "buyer_name"]`.

**DESCRIBE output:** Emit SYNONYMS property row for each object that has synonyms.

**Confidence:** HIGH (pure metadata; no query-time behavior)

---

### T4: PRIVATE/PUBLIC Access Modifiers

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `PRIVATE` / `PUBLIC` keywords on facts and metrics. Private objects cannot be queried directly. |
| **Why Expected** | Snowflake supports PRIVATE/PUBLIC on facts and metrics. This enables hiding intermediate calculations (helper facts, internal metrics) from end users while keeping them available for derived metric composition. |
| **Complexity** | **Low-Medium** -- model + parser + query-time validation |
| **Dependencies** | None |

**Snowflake's rules:**
1. Dimensions are always PUBLIC (cannot be marked PRIVATE)
2. Facts and metrics default to PUBLIC if neither keyword is specified
3. Private facts/metrics cannot be queried or used in WHERE conditions
4. Private facts/metrics CAN be referenced by other metrics in the same view (derived metric composition)
5. Private objects appear in GET_DDL output
6. Private objects appear in SHOW output only if the role has REFERENCES or OWNERSHIP privilege (DuckDB has no RBAC, so always visible)

**Model changes:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AccessModifier {
    #[default]
    Public,
    Private,
}

pub struct Metric {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "AccessModifier::is_default")]
    pub access: AccessModifier,
}

pub struct Fact {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "AccessModifier::is_default")]
    pub access: AccessModifier,
}
```

**Query-time enforcement:** When a user requests a private metric or fact in `semantic_view('view', metrics := ['private_metric'])`, the extension should return an error: "Metric 'private_metric' is private and cannot be queried directly."

**DESCRIBE output:** Emit ACCESS_MODIFIER property row for facts and metrics.

**Parser changes:** Recognize optional `PRIVATE` / `PUBLIC` keyword before `<table_alias>.<name>` in FACTS and METRICS clauses.

**Confidence:** HIGH (straightforward boolean-like modifier with query-time check)

---

### T5: GET_DDL Reconstruction

| Aspect | Detail |
|--------|--------|
| **Feature** | Reconstruct a valid `CREATE SEMANTIC VIEW` DDL statement from the stored JSON definition. Callable as `SELECT get_semantic_view_ddl('view_name')` or exposed via a DDL command. |
| **Why Expected** | Snowflake supports `GET_DDL('SEMANTIC VIEW', 'view_name')` which returns the complete DDL. This is essential for version control, migration scripts, and backup workflows. Round-trip fidelity (create -> get_ddl -> create) is critical. |
| **Complexity** | **Medium** -- must handle all DDL features including new ones (comments, synonyms, access modifiers, NON ADDITIVE BY) |
| **Dependencies** | Should be implemented AFTER T1-T4 so all features are representable |

**Snowflake's behavior:**
- Returns a `CREATE OR REPLACE SEMANTIC VIEW` statement
- Includes PRIVATE facts/metrics in the output
- May include default property values
- Must be a valid, re-executable DDL statement

**Implementation approach:**

A scalar function `get_semantic_view_ddl(name VARCHAR) -> VARCHAR` that:
1. Loads the `SemanticViewDefinition` from catalog
2. Reconstructs each clause (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS) with proper formatting
3. Includes all metadata (COMMENT, SYNONYMS, PRIVATE/PUBLIC, NON ADDITIVE BY)
4. Returns a formatted, re-parseable DDL string

**Key concerns:**
- **Round-trip fidelity:** `CREATE` -> store -> `GET_DDL` -> `CREATE OR REPLACE` must produce an identical definition. This means the model must preserve all DDL-specified information, including ordering of entries.
- **Formatting:** Snowflake returns nicely indented DDL. The extension should produce readable output.
- **Lossy fields:** `column_type_names` and `column_types_inferred` are inferred at define time and should NOT appear in GET_DDL output (they are not part of the DDL surface).

**Alternative interface:** Instead of a scalar function, could be a DDL command: `GET DDL FOR SEMANTIC VIEW 'name'`. The scalar function is simpler to implement and more flexible (can be used in SELECT, COPY TO, etc.).

**Confidence:** HIGH (string reconstruction from known model; no query-time behavior)

---

### T6: Queryable FACTS (Row-Level Query Mode)

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `FACTS` clause in the `semantic_view()` table function, returning row-level fact values without GROUP BY. Cannot be combined with METRICS in the same query. |
| **Why Expected** | Snowflake supports `FACTS` in the `SEMANTIC_VIEW()` query clause. Facts return row-level data without aggregation. This is useful for detail-level reporting, debugging, and data quality checks. |
| **Complexity** | **Medium** -- requires a new expansion path without GROUP BY |
| **Dependencies** | None on other v0.6.0 features |

**Snowflake's rules:**
1. `FACTS` and `METRICS` cannot be specified in the same query
2. When using FACTS with DIMENSIONS, all facts and dimensions must be from the same logical table
3. FACTS queries produce row-level output (no GROUP BY)
4. Facts can be used in WHERE clauses

**Current state:** Facts exist in the model and DDL but are only used for inlining into metric expressions. The `semantic_view()` table function only accepts `dimensions` and `metrics` parameters.

**Query syntax change:**

```sql
-- Current: only metrics mode
FROM semantic_view('view', dimensions := ['d1'], metrics := ['m1'])

-- New: facts mode
FROM semantic_view('view', dimensions := ['d1'], facts := ['f1', 'f2'])
```

**Expansion for FACTS mode:**
```sql
-- No GROUP BY, no aggregation
SELECT d1_expr AS d1, f1_expr AS f1, f2_expr AS f2
FROM base_table AS alias
-- JOINs if needed (but same-table constraint may eliminate this)
WHERE <filters>
```

**Validation at query time:**
- If both `facts` and `metrics` are specified: error
- If facts reference different tables than dimensions: error (same-table constraint)
- If a private fact is requested: error

**Model changes:** The table function bind needs a new `facts` parameter.

**Confidence:** HIGH for semantics; MEDIUM for implementation (same-table constraint enforcement, new expansion path)

---

### T7: Wildcard Dimension/Metric Selection

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `table_alias.*` syntax in the `dimensions` and `metrics` parameters to select all dimensions or metrics from a specific logical table. |
| **Why Expected** | Snowflake supports `customer.*` in both DIMENSIONS and METRICS clauses. This is a significant convenience for tables with many dimensions/metrics. |
| **Complexity** | **Low** -- expand wildcards to concrete names at query time |
| **Dependencies** | None |

**Snowflake's rules:**
1. Must be qualified with a table alias: `customer.*` is valid, bare `*` is not
2. Applies to DIMENSIONS, METRICS, and FACTS clauses separately
3. Expands to all objects scoped to that logical table

**Implementation approach:**

In the table function bind:
1. Parse each requested dimension/metric/fact name
2. If it matches `<alias>.*` pattern, expand to all objects with that `source_table`
3. Proceed with normal validation on the expanded list

**Key edge case:** What if `customer.*` in dimensions yields a dimension that causes a fan trap with a requested metric? The fan trap detection should run AFTER wildcard expansion, so the user gets a clear error about which specific dimension is problematic.

**Parser changes:** None in the DDL parser. This is a query-time parameter expansion.

**Model changes:** None. The expansion is purely in the table function bind logic.

**Confidence:** HIGH (simple string expansion; no semantic complexity)

---

## Differentiators

Features that set the product apart or align with Snowflake but go beyond minimum viability.

### D1: Window Function Metrics (PARTITION BY EXCLUDING)

| Aspect | Detail |
|--------|--------|
| **Feature** | Support window function metrics with `PARTITION BY EXCLUDING` in the DDL, producing non-aggregated output alongside regular metrics. |
| **Value Proposition** | Snowflake supports this for rolling averages, cumulative sums, and other window calculations. This is a significant analytical capability. |
| **Complexity** | **Very High** -- requires a fundamentally different expansion path that does NOT use GROUP BY |
| **Dependencies** | T1 (semi-additive) shares expansion path concerns |

**How Snowflake does it:**

```sql
METRICS (
  store_sales.avg_7_days_sales_quantity
    AS AVG(total_sales_quantity) OVER (
      PARTITION BY EXCLUDING date.date, date.year
      ORDER BY date.date
      RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW
    )
)
```

`PARTITION BY EXCLUDING` means: partition by ALL dimensions in the query EXCEPT the named ones. This is a Snowflake-specific SQL extension -- `EXCLUDING` is not valid outside semantic view metric definitions.

**Query rules:** When querying a window function metric, you MUST also include the dimensions named in PARTITION BY EXCLUDING and ORDER BY. The `required` column in SHOW SEMANTIC DIMENSIONS FOR METRIC indicates these mandatory dimensions.

**Why this is a differentiator, not table stakes:** Window function metrics require an expansion path that produces output WITHOUT GROUP BY, which is architecturally orthogonal to the current aggregation model. The existing expansion pipeline assumes every query produces `SELECT ... GROUP BY dims`. Window metrics need row-level output with window expressions layered on top.

**Recommendation:** Implement the DDL parsing and model storage now so definitions are complete, but defer query-time expansion to a later milestone. This allows GET_DDL round-trip fidelity and correct DESCRIBE/SHOW output while avoiding the expansion complexity.

If implemented in v0.6.0, the `required` column in SHOW SEMANTIC DIMENSIONS FOR METRIC gains real meaning (currently constant FALSE).

**Confidence:** HIGH for syntax; LOW for expansion implementation (architectural change needed)

---

### D2: SHOW ... IN SCHEMA/DATABASE Scope Filtering

| Aspect | Detail |
|--------|--------|
| **Feature** | Extend SHOW commands to support `IN DATABASE <name>` and `IN SCHEMA <name>` scope filtering, beyond the current `IN <semantic_view_name>`. |
| **Value Proposition** | Snowflake supports ACCOUNT/DATABASE/SCHEMA scoping. For DuckDB with attached databases, DATABASE-level scoping has real value. |
| **Complexity** | **Low-Medium** -- parser and WHERE clause injection |
| **Dependencies** | T5 metadata (database_name, schema_name stored at define time) |

**Current state:** SHOW SEMANTIC VIEWS supports no IN clause. SHOW SEMANTIC DIMENSIONS/METRICS/FACTS support `IN <view_name>` to scope to a single view. Cross-view forms (`_all` suffix) return everything.

**Snowflake's IN clause:**
```sql
SHOW SEMANTIC VIEWS IN DATABASE my_db
SHOW SEMANTIC VIEWS IN SCHEMA my_db.my_schema
SHOW SEMANTIC DIMENSIONS IN SCHEMA my_db.my_schema
```

**Implementation:** Add `IN DATABASE <name>` and `IN SCHEMA <name>` variants to the SHOW parser. Generate WHERE clauses filtering on stored `database_name` and `schema_name`.

For SHOW SEMANTIC VIEWS: `IN DATABASE` and `IN SCHEMA` are the meaningful scopes.
For SHOW SEMANTIC DIMENSIONS/METRICS/FACTS: `IN <view_name>` already works; add `IN DATABASE` and `IN SCHEMA` as alternatives.

**Confidence:** HIGH (mechanical WHERE clause injection)

---

### D3: TERSE Mode for SHOW Commands

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `SHOW TERSE SEMANTIC VIEWS` returning a reduced column set. |
| **Value Proposition** | Snowflake supports TERSE for SHOW SEMANTIC VIEWS (not for DIMS/METRICS/FACTS). Returns: created_on, name, kind, database_name, schema_name (the same 5 columns -- so in practice TERSE is identical to regular for our extension since we already omit comment/owner). |
| **Complexity** | **Low** -- parser change to detect TERSE keyword |
| **Dependencies** | None |

**Key insight:** In Snowflake, TERSE removes the `comment`, `owner`, and `owner_role_type` columns. Since this extension already omits those columns, TERSE mode would produce identical output to regular mode. The value is syntactic compatibility -- scripts written for Snowflake that use `SHOW TERSE SEMANTIC VIEWS` should not error.

**Recommendation:** Implement as a no-op parser recognition (accept the TERSE keyword, produce standard output). This is cheap and ensures Snowflake DDL script portability.

**Confidence:** HIGH (trivial)

---

### D4: SHOW COLUMNS on Semantic View

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `SHOW COLUMNS IN VIEW <semantic_view_name>` returning dimensions, facts, and metrics with a `kind` column. |
| **Value Proposition** | Snowflake's SHOW COLUMNS is a unified interface that works across tables, views, and semantic views. It returns a `kind` column with values DIMENSION, FACT, or METRIC. |
| **Complexity** | **Medium** -- new command + parser prefix detection |
| **Dependencies** | None |

**Snowflake's SHOW COLUMNS output for semantic views:**

| Column | Description |
|--------|-------------|
| table_name | Semantic view name |
| schema_name | Schema |
| column_name | Dimension/fact/metric name |
| data_type | Data type |
| null? | Nullability |
| default | Default value |
| kind | DIMENSION, FACT, or METRIC |
| expression | The defining expression |
| comment | Comment text |
| database_name | Database |
| autoincrement | N/A for semantic views |

**Alternative:** The existing SHOW SEMANTIC DIMENSIONS + SHOW SEMANTIC METRICS + SHOW SEMANTIC FACTS already provides this information (split across three commands). SHOW COLUMNS provides a single unified view.

**Recommendation:** Implement if time allows. Lower priority than the core DDL features (T1-T7).

**Confidence:** HIGH for semantics; MEDIUM for implementation (new DDL prefix detection)

---

## Anti-Features

Features to explicitly NOT build in v0.6.0.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| `owner` / `owner_role_type` columns in SHOW VIEWS | DuckDB has no RBAC model. These are Snowflake-specific privilege columns. | Omit. Not applicable to DuckDB extensions. |
| CUSTOM_INSTRUCTIONS object_kind in DESCRIBE | Snowflake Cortex AI integration for natural language query hints. | Omit entirely. Snowflake-specific AI feature. |
| Cortex Search Service dimension properties | Snowflake-specific vector search integration. | Omit entirely. |
| CONSTRAINT object_kind (DISTINCT_RANGE) | Snowflake uses constraints for time-range boundaries (START_COLUMN/END_COLUMN). Niche temporal feature. | Omit. PK/UNIQUE expressed as TABLE properties. |
| Synonym-based query resolution | Snowflake explicitly states synonyms are "informational only." Building synonym resolution would be non-standard. | Store synonyms as metadata only. |
| `IN ACCOUNT` scoping | DuckDB extensions operate within a single database. Account-level scoping is meaningless. | Omit. `IN DATABASE` and `IN SCHEMA` cover the meaningful scopes. |
| Window function metric query expansion (if parser-only approach taken) | Architectural change to support non-GROUP BY expansion paths. Very high complexity. | Parse and store in model; defer expansion to future milestone. |
| `USING RELATIONSHIPS` for facts queries | Snowflake's same-table constraint for facts eliminates multi-table join paths. | Enforce same-table constraint; no join path selection needed for facts. |

---

## Feature Dependencies

```
T2: COMMENT ----+
T3: SYNONYMS ---+--> T5: GET_DDL (needs all metadata to reconstruct)
T4: PRIVATE ----+
T1: NON ADDITIVE --+

T6: Queryable FACTS (independent)
T7: Wildcard selection (independent)

D1: Window metrics (independent, but shares expansion concerns with T1)
D2: IN SCHEMA/DATABASE (uses stored database_name/schema_name from v0.5.5)
D3: TERSE mode (parser-only, independent)
D4: SHOW COLUMNS (independent)

T2 COMMENT --> ALTER SET/UNSET COMMENT (extends existing ALTER infrastructure)

SHOW output changes (add comment/synonyms columns) depend on T2/T3 model changes.
DESCRIBE output changes (new property rows) depend on T2/T3/T4 model changes.
```

**Critical ordering insight:** T5 (GET_DDL) should be the LAST table-stakes feature implemented because it must reconstruct ALL other features faithfully. Implementing it first would require updating it with every subsequent feature addition.

**Recommended phase ordering:**
1. **Metadata features (T2 + T3 + T4)** -- Add comment, synonyms, access modifiers to model/parser/SHOW/DESCRIBE. These are shallow, pervasive changes that touch the same files. Bundle them to minimize churn.
2. **Semi-additive metrics (T1)** -- Deep expansion pipeline change. Independent of metadata features. Implement after metadata is stable.
3. **Queryable FACTS (T6)** -- New expansion path (no GROUP BY). Independent.
4. **Wildcard selection (T7)** -- Simple query-time expansion. Can go anywhere.
5. **GET_DDL (T5)** -- Last, after all DDL features are finalized.
6. **Introspection (D2 + D3 + D4)** -- Polish features that can be added at any point.

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC Delta | Risk | Category |
|---------|------------|----------------|------|----------|
| T1: Semi-additive metrics (NON ADDITIVE BY) | High | ~400 (parser + model + expansion + tests) | Medium (expansion pipeline structural change) | Table Stakes |
| T2: COMMENT on views and objects | Medium | ~250 (parser + model + SHOW + DESCRIBE + ALTER) | Low | Table Stakes |
| T3: SYNONYMS on objects | Low-Medium | ~200 (same pattern as COMMENT) | Low | Table Stakes |
| T4: PRIVATE/PUBLIC access modifiers | Low-Medium | ~180 (model + parser + query validation + DESCRIBE) | Low | Table Stakes |
| T5: GET_DDL reconstruction | Medium | ~300 (DDL string builder + tests) | Low-Medium (round-trip fidelity) | Table Stakes |
| T6: Queryable FACTS | Medium | ~250 (new expansion path + table function param + tests) | Medium (new code path) | Table Stakes |
| T7: Wildcard selection | Low | ~80 (query-time name expansion) | Low | Table Stakes |
| D1: Window function metrics | Very High | ~600+ (parser + model + expansion architecture) | High (orthogonal expansion path) | Differentiator |
| D2: SHOW IN SCHEMA/DATABASE | Low-Medium | ~100 (parser + WHERE injection) | Low | Differentiator |
| D3: TERSE mode | Low | ~30 (parser recognition) | None | Differentiator |
| D4: SHOW COLUMNS | Medium | ~200 (new command + parser) | Low | Differentiator |
| **Table Stakes Total** | | **~1,660 LOC** | | |
| **All Features Total** | | **~2,590 LOC** | | |

---

## MVP Recommendation

Prioritize:

1. **T2 + T3 + T4: Metadata features bundle** -- COMMENT, SYNONYMS, PRIVATE/PUBLIC. These three touch the same model structs, parser paths, and SHOW/DESCRIBE outputs. Bundling reduces file churn. Low individual complexity, medium combined.

2. **T1: Semi-additive metrics** -- The most impactful semantic feature. Required for correct snapshot data modeling. High complexity but self-contained within the expansion pipeline.

3. **T6: Queryable FACTS** -- Enables row-level querying, a distinct query mode. Medium complexity.

4. **T7: Wildcard selection** -- Low-hanging fruit. Simple convenience feature.

5. **T5: GET_DDL reconstruction** -- Must be last table-stakes feature so it can represent everything.

6. **D3: TERSE mode** -- Trivial parser change for Snowflake compatibility.

7. **D2: IN SCHEMA/DATABASE** -- Useful introspection enhancement.

Defer:
- **D1: Window function metrics** -- Very high complexity. Parse and store the DDL now but defer query-time expansion. This gives GET_DDL round-trip fidelity without the expansion architecture changes.
- **D4: SHOW COLUMNS** -- Nice-to-have unified view, but redundant with existing SHOW SEMANTIC DIMENSIONS/METRICS/FACTS.

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- Complete DDL syntax including NON ADDITIVE BY, PARTITION BY EXCLUDING, COMMENT, SYNONYMS, PRIVATE/PUBLIC
- [SEMANTIC_VIEW query syntax](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- FACTS vs METRICS mutual exclusivity, wildcard `table.*` syntax, window metric required dimensions
- [Querying semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- FACTS rules, same-table constraint, WHERE clause support
- [ALTER SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/alter-semantic-view) -- SET/UNSET COMMENT syntax (only comment alterable; other changes require replace)
- [DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- 5-column output, ACCESS_MODIFIER/COMMENT/SYNONYMS properties per object_kind
- [SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- 8-column output with comment, TERSE mode, IN scope, LIKE/STARTS WITH/LIMIT
- [SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- 8-column output including synonyms and comment columns
- [SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics) -- Same 8-column schema
- [SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts) -- Same 8-column schema
- [SHOW COLUMNS](https://docs.snowflake.com/en/sql-reference/sql/show-columns) -- `kind` column for DIMENSION/FACT/METRIC, works with semantic views via VIEW keyword
- [GET_DDL](https://docs.snowflake.com/en/sql-reference/functions/get_ddl) -- Supports 'SEMANTIC VIEW' object type
- [Semi-additive metrics release note](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY feature announcement (March 5, 2026)

### dbt / MetricFlow (MEDIUM confidence -- cross-reference)

- [dbt Measures documentation](https://docs.getdbt.com/docs/build/measures) -- `non_additive_dimension` parameter with `window_choice` and `window_groupings`
- [dbt Semantic Layer Spec Proposal #7456](https://github.com/dbt-labs/dbt-core/discussions/7456) -- Semi-additive measures design discussion

### Cube.dev (MEDIUM confidence -- cross-reference)

- [Cube.dev Measures documentation](https://cube.dev/docs/product/data-modeling/reference/measures) -- Additive vs non-additive measure types
- [Cube.dev Non-Additivity guide](https://cube.dev/docs/guides/recipes/query-acceleration/non-additivity) -- Pre-aggregation strategies for non-additive measures

### General Data Warehouse Patterns (MEDIUM confidence)

- [Semi-Additive Measures in DAX (SQLBI)](https://www.sqlbi.com/articles/semi-additive-measures-in-dax/) -- Conceptual reference for LAST_VALUE over time patterns
