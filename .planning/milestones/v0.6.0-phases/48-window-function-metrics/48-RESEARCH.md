# Phase 48: Window Function Metrics - Research

**Researched:** 2026-04-12
**Domain:** DDL parsing, SQL expansion, metric classification, window function SQL generation
**Confidence:** HIGH

## Summary

Window function metrics are a new metric classification that wraps an aggregate metric in a SQL window function (e.g., `AVG(total_sales) OVER (PARTITION BY ... ORDER BY ...)`). Unlike regular aggregate metrics which produce GROUP BY queries, window function metrics produce aggregated results with window computations applied on top -- conceptually a two-step process: (1) aggregate by all queried dimensions, (2) apply the window function over the partition. The Snowflake-specific `PARTITION BY EXCLUDING` syntax means "partition by all queried dimensions EXCEPT the listed ones," requiring the extension to compute the actual PARTITION BY columns at query expansion time.

This phase builds on the semi-additive metrics infrastructure (Phase 47) as a design precedent but has fundamentally different SQL generation semantics: semi-additive uses a CTE with ROW_NUMBER for snapshot selection before aggregation, while window metrics apply a window function AFTER aggregation. The key design challenge is that window metrics cannot be mixed with aggregate metrics in the same query (different output semantics -- aggregate metrics collapse rows via GROUP BY, window metrics produce one row per dimension group with a computed window value). Window metrics reference other metrics by name (derived metric composition), meaning the metric dependency resolution system (inline_derived_metrics) must handle window metric expressions as a special case.

**Primary recommendation:** Model window metrics as a new variant in the Metric struct (via a `window_spec` field), parse the OVER clause from the DDL expression, and add a third expansion path in `sql_gen.rs::expand()` (alongside regular and semi-additive). At expansion time, generate a CTE subquery that performs the base aggregation, then wrap it with the window function SELECT. Add a blocking error for window + aggregate metric mixing, and skip window metrics in fan trap detection.

## Project Constraints (from CLAUDE.md)

- **Snowflake reference:** If in doubt about SQL syntax or behavior, refer to Snowflake semantic views
- **Quality gate:** All phases must pass `just test-all` (Rust unit + proptest + sqllogictest + DuckLake CI)
- **Testing:** `just test-sql` requires fresh `just build` first
- **Test completeness:** Phase verification that only runs `cargo test` is incomplete

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| WIN-01 | User can declare a window function metric with PARTITION BY EXCLUDING in DDL | Body parser extension to detect OVER clause in metric expr; new model field `window_spec` with excluded_dims, order_by, frame_clause |
| WIN-02 | Window function metrics produce correct non-aggregated results at query time | New expansion path: CTE for base aggregation + outer SELECT with window function; no GROUP BY on the outer query |
| WIN-03 | Queries cannot mix window function metrics with aggregate metrics (blocking error) | New `ExpandError::WindowAggregateMixing` variant; check in `expand()` before SQL generation |
| WIN-04 | Window function metrics are excluded from fan trap detection | Add `is_window_metric()` check in `check_fan_traps()`, analogous to `!met.non_additive_by.is_empty()` skip |
| WIN-05 | SHOW SEMANTIC DIMENSIONS FOR METRIC shows required=TRUE for dimensions in window metric partition spec | Modify `ShowDimensionsForMetricVTab::bind()` to identify excluded/order-by dimensions and mark them required |
</phase_requirements>

## Architecture Patterns

### How Window Metrics Work (Snowflake Semantics)

Window metrics are conceptually a two-phase computation: [CITED: docs.snowflake.com/en/user-guide/views-semantic/querying]

1. **Phase 1 (Aggregation):** Compute the referenced aggregate metric grouped by ALL queried dimensions
2. **Phase 2 (Window):** Apply the window function over the aggregated results, partitioned by the specified columns

Example: Given `AVG(total_sales_quantity) OVER (PARTITION BY EXCLUDING date.date, date.year ORDER BY date.date RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)`:

- Phase 1: Compute `total_sales_quantity` (which is `SUM(ss_quantity)`) grouped by all queried dims including `date.date` and `date.year`
- Phase 2: Apply `AVG(...) OVER (PARTITION BY <all queried dims except date.date and date.year> ORDER BY date.date RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)`

The output has one row per unique dimension combination (same cardinality as a regular aggregated query).

### PARTITION BY EXCLUDING Resolution

`PARTITION BY EXCLUDING dim1, dim2` means: "At query time, partition by all dimensions the user requested EXCEPT dim1 and dim2." [CITED: docs.snowflake.com/en/sql-reference/sql/create-semantic-view]

This is a dynamic computation -- the partition columns depend on what dimensions appear in the query request. The excluded dimensions are still required in the query (they must be specified as queried dimensions) because they appear in the ORDER BY or are needed for the window frame to be meaningful.

### Required Dimensions

When querying a window metric, the user MUST include all dimensions that appear in: [CITED: docs.snowflake.com/en/sql-reference/constructs/semantic_view]
- `PARTITION BY EXCLUDING <dimensions>` -- the excluded dimensions must be queried
- `ORDER BY <dimensions>` -- the order dimensions must be queried

If required dimensions are omitted, Snowflake returns: `"Dimension 'DATE.DATE' used in a window function metric must be requested in the query."` [CITED: docs.snowflake.com/en/user-guide/views-semantic/querying]

### Recommended Implementation Structure

```
Phase 48 touches:
src/model.rs             -- Add WindowSpec to Metric
src/body_parser.rs       -- Parse OVER clause from metric expression
src/expand/sql_gen.rs    -- Window metric expansion path
src/expand/types.rs      -- New error variants
src/expand/fan_trap.rs   -- Skip window metrics
src/expand/mod.rs        -- Export new items if needed
src/render_ddl.rs        -- GET_DDL for window metrics (should already work if expr stored verbatim)
src/ddl/show_dims_for_metric.rs -- required=TRUE for window dims
test/sql/phase48_window_metrics.test -- sqllogictest integration
```

### Pattern 1: Model Extension

**What:** Add a `WindowSpec` struct to the Metric model to store parsed window function metadata.

**Why not just store the raw expression?** The extension needs to REWRITE the OVER clause at expansion time -- `PARTITION BY EXCLUDING date, year` must be transformed to `PARTITION BY <computed columns>`. Storing the parsed components enables this rewriting without re-parsing at query time.

```rust
// Source: Snowflake windowFunctionMetricExpression syntax
/// Parsed window function specification for window metrics.
/// Stored alongside the raw expression for expansion-time rewriting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowSpec {
    /// The window function name (e.g., "AVG", "LAG", "SUM")
    pub window_function: String,
    /// The metric name referenced inside the window function
    pub inner_metric: String,
    /// Additional arguments after the inner metric (e.g., "30" in LAG(metric, 30))
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    /// Dimensions to EXCLUDE from partitioning (PARTITION BY EXCLUDING semantics)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluding_dims: Vec<String>,
    /// ORDER BY clause entries (dimension/expression + direction + nulls)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<WindowOrderBy>,
    /// Raw frame clause (e.g., "RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_clause: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowOrderBy {
    pub expr: String,
    #[serde(default, skip_serializing_if = "SortOrder::is_default")]
    pub order: SortOrder,
    #[serde(default, skip_serializing_if = "NullsOrder::is_default")]
    pub nulls: NullsOrder,
}
```

The Metric struct gets a new field:
```rust
pub struct Metric {
    // ... existing fields ...
    /// Window function specification for window metrics.
    /// When non-empty, this metric uses a window function wrapping another metric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_spec: Option<WindowSpec>,
}
```

### Pattern 2: DDL Parsing Strategy

**What:** The OVER clause is part of the metric expression in DDL, not a separate annotation.

**Parsing approach:** After extracting the expression from `AS <expr>`, check if the expression contains an OVER clause. Parse the OVER clause to extract the WindowSpec, and store both the raw expression (for GET_DDL rendering) and the parsed WindowSpec (for expansion-time rewriting).

The Snowflake DDL looks like:
```sql
METRICS (
    store_sales.total_sales_quantity AS SUM(ss_quantity),
    store_sales.avg_7_days AS AVG(total_sales_quantity)
        OVER (PARTITION BY EXCLUDING date.date, date.year 
              ORDER BY date.date
              RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)
)
```

Key parsing considerations:
- The OVER keyword appears in the expression text after `AS`
- Must handle nested parentheses (window frame clauses contain parens)
- EXCLUDING is a Snowflake-specific keyword only valid inside window metric OVER clauses
- The inner function argument references another metric by name (not a column)

### Pattern 3: SQL Expansion (CTE Approach)

**What:** Generate a CTE that performs the base aggregation, then an outer SELECT that applies the window function.

**Example input:**
```
dimensions: [region, date]
metrics: [avg_7_days]  -- where avg_7_days = AVG(total_sales) OVER (PARTITION BY EXCLUDING date ORDER BY date)
```

**Generated SQL:**
```sql
WITH __sv_agg AS (
    SELECT
        region AS "region",
        date AS "date",
        SUM(amount) AS "total_sales"
    FROM "sales" AS "s"
    GROUP BY 1, 2
)
SELECT
    "region",
    "date",
    AVG("total_sales") OVER (PARTITION BY "region" ORDER BY "date") AS "avg_7_days"
FROM __sv_agg
```

Key aspects:
- **CTE:** Aggregates the inner metric by ALL queried dimensions (including excluded ones)
- **Outer SELECT:** References CTE columns, applies window function with computed PARTITION BY
- **No outer GROUP BY:** The window function operates over the aggregated rows
- **PARTITION BY computation:** All queried dims MINUS excluded dims
- **ORDER BY:** Passed through from the WindowSpec, referencing CTE column aliases

### Pattern 4: Blocking Error for Mixed Queries

**What:** When a query requests both window function metrics and regular aggregate metrics, return a clear error.

**Why:** Window metrics produce output without GROUP BY (one row per dimension group, with window values), while aggregate metrics produce GROUP BY output. These are incompatible result structures. Snowflake's documentation does not show examples of mixing them, and the generated SQL structures are fundamentally different. [ASSUMED]

```rust
ExpandError::WindowAggregateMixing {
    view_name: String,
    window_metrics: Vec<String>,
    aggregate_metrics: Vec<String>,
}
```

**Note:** Multiple window metrics in the same query SHOULD be allowed -- they can all share the same CTE and each applies its own OVER clause in the outer SELECT.

### Pattern 5: Fan Trap Exclusion

**What:** Skip window metrics in fan trap detection, analogous to how semi-additive metrics are skipped.

**Why:** Window metrics operate on pre-aggregated results (from the CTE), so the fan-out concern is addressed by the inner aggregation step. The base metric referenced by the window function will have its own fan trap check when it's used standalone, but the window wrapping doesn't introduce new fan-out risk.

### Anti-Patterns to Avoid

- **Re-parsing the expression at query time:** Parse the OVER clause once at DDL time, store the WindowSpec. Do not try to parse SQL at expansion time.
- **Trying to generate window functions inline without CTE:** The expression references another metric name, which must first be resolved to its aggregate expression and computed. A CTE cleanly separates aggregation from windowing.
- **Allowing window + aggregate mixing and hoping the SQL works:** The SQL structures are incompatible. Better to block with a clear error than produce wrong results.
- **Storing only the parsed WindowSpec without the raw expression:** The raw expression is needed for GET_DDL round-tripping and for display in SHOW commands.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SQL OVER clause parsing | Full SQL parser | Targeted OVER clause extraction from expression | Full SQL parsing is massive scope; we only need to find and decompose the OVER clause |
| Window frame validation | Frame clause validator | Store raw frame clause, let DuckDB validate at query time | Frame clause syntax is complex (ROWS/RANGE/GROUPS, BETWEEN, INTERVAL); DuckDB will validate it |
| Dimension requirement checking | Manual dimension tracking | Derive required dims from WindowSpec.excluding_dims + order_by at query time | The excluded dims and order-by dims are already parsed and stored |

## Common Pitfalls

### Pitfall 1: OVER Clause Parsing Ambiguity

**What goes wrong:** The expression `AVG(total_sales_quantity) OVER (...)` contains nested parentheses. A naive split on `OVER` could break if the metric expression or frame clause contains the word "OVER" in a string literal or nested context.
**Why it happens:** SQL expressions can be arbitrarily complex.
**How to avoid:** Find the `OVER` keyword at word boundaries (not inside parens or string literals), then extract the balanced parenthesized content after it. Use the existing `split_at_depth0_commas` pattern for paren-depth tracking.
**Warning signs:** Tests with complex frame clauses (e.g., `RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW`) failing to parse.

### Pitfall 2: EXCLUDING Dimension Name Resolution

**What goes wrong:** EXCLUDING references dimension names, but dimension names may differ from the column names used in the expression. E.g., dimension `date` might have expr `d.d_date`.
**Why it happens:** Dimensions have both a `name` (user-facing) and an `expr` (SQL expression).
**How to avoid:** At expansion time, resolve excluded dimension names against the dimension definitions. The PARTITION BY in the generated SQL should use the CTE column aliases (which match dimension names), not the raw expressions.
**Warning signs:** Generated SQL references dimension expressions instead of aliases in the PARTITION BY clause.

### Pitfall 3: Inner Metric Must Be Resolved Before Window Application

**What goes wrong:** The window function references a metric by name (e.g., `AVG(total_sales_quantity)`). If the inner metric is a derived metric itself, it must be fully resolved before being used in the CTE.
**Why it happens:** Window metrics compose with the existing metric dependency graph.
**How to avoid:** Use `inline_derived_metrics` to resolve the inner metric's expression first, then use that resolved expression in the CTE SELECT.
**Warning signs:** Window metric referencing a derived metric produces wrong SQL or unknown column errors.

### Pitfall 4: serde Backward Compatibility

**What goes wrong:** Adding `window_spec: Option<WindowSpec>` to Metric without `#[serde(default)]` breaks deserialization of pre-Phase 48 stored JSON.
**Why it happens:** Missing `#[serde(default)]` annotation on new optional fields.
**How to avoid:** Always use `#[serde(default, skip_serializing_if = "Option::is_none")]` for new optional fields. This is an established project pattern (Phase 43 META-07).
**Warning signs:** Existing views fail to load after code update.

### Pitfall 5: Window Metrics Should Not Have source_table Set

**What goes wrong:** Window metrics reference other metrics, not table columns. If they have a `source_table`, the expansion engine treats them as base metrics.
**Why it happens:** Confusion about window metric classification -- they are similar to derived metrics (no source_table, reference other metrics by name) but with a window function wrapper.
**How to avoid:** Window metrics should have `source_table: None` and be identified by `window_spec: Some(...)`. The body parser should handle qualified syntax (`alias.name AS ...`) but NOT set `source_table` when a window spec is detected -- or alternatively, set source_table for join resolution but classify via window_spec presence.

**Design decision needed:** Whether window metrics store `source_table` or not. Snowflake syntax is `store_sales.avg_7_days AS AVG(total_sales_quantity) OVER (...)` -- the `store_sales` prefix is the table alias. The question is whether this alias should be stored in `source_table` (for join resolution) or ignored (since window metrics operate on pre-aggregated results).

**Recommendation:** Store `source_table` from the qualified syntax (for consistency with how the parser works for all qualified metrics), but use `window_spec.is_some()` as the primary classification signal. The expansion path for window metrics does NOT use `source_table` for SQL generation -- it uses the inner metric's resolution instead.

### Pitfall 6: Multiple Window Metrics in Same Query

**What goes wrong:** If two window metrics have different EXCLUDING sets, the PARTITION BY clauses differ. The CTE must include all dimensions needed by any of the window metrics.
**Why it happens:** Each window metric may exclude different dimensions.
**How to avoid:** The CTE aggregates by ALL queried dimensions (the union of what all window metrics need). Each window metric in the outer SELECT uses its own computed PARTITION BY.
**Warning signs:** Query with two window metrics produces wrong partitioning for one of them.

## Code Examples

### Example 1: DDL Declaration (Snowflake Syntax)

```sql
-- Source: docs.snowflake.com/en/sql-reference/sql/create-semantic-view
CREATE SEMANTIC VIEW sales_analysis AS
TABLES (
    s AS sales PRIMARY KEY (id),
    d AS dates PRIMARY KEY (id)
)
RELATIONSHIPS (
    sale_date AS s(date_id) REFERENCES d
)
DIMENSIONS (
    d.date AS d.d_date,
    d.year AS d.d_year,
    s.store AS s.store_name
)
METRICS (
    s.total_qty AS SUM(s.quantity),
    s.avg_7_days AS AVG(total_qty)
        OVER (PARTITION BY EXCLUDING d.date, d.year
              ORDER BY d.date
              RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)
)
```

### Example 2: Expected SQL Expansion

```sql
-- Query: dimensions := ['store', 'date', 'year'], metrics := ['avg_7_days']
-- Expansion:
WITH __sv_agg AS (
    SELECT
        s.store_name AS "store",
        d.d_date AS "date",
        d.d_year AS "year",
        SUM(s.quantity) AS "total_qty"
    FROM "sales" AS "s"
    LEFT JOIN "dates" AS "d" ON "s"."date_id" = "d"."id"
    GROUP BY 1, 2, 3
)
SELECT
    "store",
    "date",
    "year",
    AVG("total_qty") OVER (
        PARTITION BY "store"
        ORDER BY "date"
        RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW
    ) AS "avg_7_days"
FROM __sv_agg
```

### Example 3: Missing Required Dimension Error

```sql
-- Query: dimensions := ['store'], metrics := ['avg_7_days']
-- Error: date and year are required (they appear in PARTITION BY EXCLUDING and ORDER BY)
-- Message: "semantic view 'sales_analysis': window function metric 'avg_7_days' requires
--           dimension 'date' to be included in the query (used in PARTITION BY EXCLUDING)"
```

### Example 4: Mixed Window + Aggregate Error

```sql
-- Query: dimensions := ['store', 'date'], metrics := ['total_qty', 'avg_7_days']
-- Error: cannot mix window and aggregate metrics
-- Message: "semantic view 'sales_analysis': cannot mix window function metrics ['avg_7_days']
--           with aggregate metrics ['total_qty'] in the same query"
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| N/A (new feature) | Window function metrics in semantic views | Snowflake 2024-2025 | Enables running averages, LAG/LEAD, cumulative computations as first-class metrics |

**Key Snowflake feature:** `PARTITION BY EXCLUDING` is a Snowflake-specific extension to SQL window functions, not standard SQL. It exists only in semantic view metric definitions. [CITED: docs.snowflake.com/en/sql-reference/sql/create-semantic-view]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Window metrics and aggregate metrics cannot be mixed in same query | Pattern 4 | If Snowflake allows mixing, the blocking error is over-restrictive. Could be relaxed later. |
| A2 | Window metrics should have source_table set from qualified DDL syntax | Pitfall 5 | If source_table should be None for window metrics, parser needs different logic for qualified window metric entries |
| A3 | Multiple window metrics in the same query share one CTE | Pattern 3/Pitfall 6 | If different window metrics need different base aggregations (different inner metrics), multiple CTEs may be needed |

## Open Questions (RESOLVED)

1. **Can window metrics reference any metric, or only base metrics?**
   - What we know: Snowflake syntax shows `AVG(total_sales_quantity)` where `total_sales_quantity` is a base metric. The docs say "a metric or any valid metric expression." [CITED: docs.snowflake.com/en/sql-reference/sql/create-semantic-view]
   - What's unclear: Can a window metric reference another derived metric (which itself references base metrics)?
   - RESOLVED: Allow it. The inline_derived_metrics system already resolves derived expressions. The CTE just needs the fully-resolved expression.

2. **Should window-only queries include queried dimensions in the output?**
   - What we know: Snowflake includes all queried dimensions in the output (both partition and excluded dims).
   - What's unclear: Whether the implementation should follow the same fact-query pattern (unaggregated) or use a different approach.
   - RESOLVED: Include all queried dimensions in the outer SELECT (same as regular queries). The CTE handles aggregation; the outer SELECT includes dims + window metrics.

3. **LAG/LEAD with offset arguments**
   - What we know: Snowflake syntax shows `LAG(total_sales_quantity, 30)` with extra arguments
   - What's unclear: How to parse and store the extra arguments (the "30" part)
   - RESOLVED: Store extra_args as strings in WindowSpec. At expansion time, reconstruct the window function call with all arguments.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in + sqllogictest |
| Config file | test/sql/*.test (sqllogictest), Cargo.toml (Rust tests) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| WIN-01 | Window metric DDL parsing + storage | unit | `cargo test body_parser::tests::parse_metrics` | Extend existing |
| WIN-01 | Window metric DDL integration | integration | `just test-sql` (phase48 test file) | Wave 0 |
| WIN-02 | Window metric SQL expansion (CTE + window) | unit | `cargo test expand::semi_additive::tests` or new module | Wave 0 |
| WIN-02 | Window metric query results | integration | `just test-sql` (phase48 test file) | Wave 0 |
| WIN-03 | Window + aggregate mixing error | unit | `cargo test expand::sql_gen::tests` | Wave 0 |
| WIN-03 | Mixed query error integration | integration | `just test-sql` (phase48 test file) | Wave 0 |
| WIN-04 | Fan trap skips window metrics | unit | `cargo test expand::fan_trap` or expand tests | Wave 0 |
| WIN-05 | SHOW DIMS required=TRUE for window dims | integration | `just test-sql` (phase48 test file) | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase48_window_metrics.test` -- integration tests for WIN-01 through WIN-05
- [ ] Expand module: window metric expansion tests (unit)
- [ ] Body parser: window metric parsing tests (unit, extend existing)
- [ ] Model: WindowSpec serde roundtrip tests

## Security Domain

Security enforcement is not applicable to this phase. This phase adds a new metric type within the existing semantic view system. No new authentication, access control, input validation (beyond DDL parsing), cryptography, or session management is introduced. The existing PRIVATE/PUBLIC access modifier system applies to window metrics through the same code path as other metrics.

## Sources

### Primary (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW docs](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- windowFunctionMetricExpression syntax, PARTITION BY EXCLUDING semantics
- [Snowflake querying semantic views docs](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- query-time behavior, required dimensions, error messages
- [Snowflake SEMANTIC_VIEW clause docs](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- dimension requirements for window metrics
- [Snowflake SHOW SEMANTIC DIMENSIONS FOR METRIC docs](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric) -- required=TRUE column semantics
- Codebase analysis: src/model.rs, src/body_parser.rs, src/expand/*.rs, src/ddl/show_dims_for_metric.rs

### Secondary (MEDIUM confidence)
- [Medium article on Snowflake semantic view testing](https://medium.com/@masato.takada/comprehensive-testing-of-snowflakes-new-sql-syntax-for-semantic-views-db4485d90556) -- practical testing insights, window metric behavior confirmation

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- this is all Rust in the existing codebase, no new dependencies
- Architecture: HIGH -- clear precedent from semi-additive (Phase 47), well-understood CTE pattern
- Pitfalls: HIGH -- most pitfalls identified from actual codebase patterns (serde compat, parser structure, expansion paths)
- Snowflake semantics: MEDIUM -- PARTITION BY EXCLUDING is well-documented but mixing behavior with aggregate metrics is [ASSUMED] to be blocked

**Research date:** 2026-04-12
**Valid until:** 2026-05-12 (stable domain -- internal codebase + Snowflake docs)
