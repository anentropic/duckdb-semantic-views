# Technology Stack

**Project:** DuckDB Semantic Views v0.6.0 -- Snowflake SQL DDL Parity
**Researched:** 2026-04-09
**Scope:** Stack additions/changes for semi-additive metrics, window function metrics, metadata system, GET_DDL, queryable FACTS, wildcards, SHOW enhancements

## Key Finding: No New Crates Required

This milestone requires **zero new Rust dependencies**. Every feature builds on existing capabilities:

- **Semi-additive metrics (NON ADDITIVE BY):** Pure SQL expansion change -- generates `LAST_VALUE(...) OVER (PARTITION BY ... ORDER BY ...)` which DuckDB supports natively. No new library needed; this is string-based SQL generation in `expand/sql_gen.rs`.
- **Window function metrics (PARTITION BY EXCLUDING):** Same expansion engine, different SQL output path. Generates window functions instead of GROUP BY aggregation. DuckDB's window function support is comprehensive (confirmed: LAST_VALUE, SUM OVER, PARTITION BY all available).
- **Metadata (COMMENT, SYNONYMS, PRIVATE/PUBLIC):** New fields on existing serde-derived model structs. Stored as JSON in the same catalog table. No serialization library changes.
- **GET_DDL reconstruction:** Pure Rust string building from `SemanticViewDefinition` fields. No template engine needed -- the DDL is simple enough for `format!`/`write!` macros.
- **Queryable FACTS:** New code path in the expand function that emits SELECT without GROUP BY. Reuses existing join resolution, table function infrastructure, and typed output pipeline.
- **Wildcard selection (e.g. `customer.*`):** Pattern matching on dimension/metric names at expand time. Standard library `str::ends_with` / iterator filtering.
- **SHOW enhancements (TERSE, IN scope, SHOW COLUMNS):** New VTab implementations following the established pattern in `src/ddl/`. Column schema changes only.

## Current Stack (Unchanged for v0.6.0)

### Core Dependencies
| Technology | Version | Purpose | v0.6.0 Impact |
|------------|---------|---------|---------------|
| duckdb (Rust) | =1.10500.0 | DuckDB C API bindings, VTab trait, `LogicalTypeId` | No change. VTab pattern reused for SHOW COLUMNS and TERSE variants |
| libduckdb-sys | =1.10500.0 | Raw FFI bindings (`duckdb_query`, `duckdb_vector_reference_vector`) | No change. Same query execution path for window function metrics |
| serde | 1.x | Derive `Serialize`/`Deserialize` on model types | No change. New fields added with `#[serde(default)]` for backward compat |
| serde_json | 1.x | JSON serialization of `SemanticViewDefinition` | No change. Handles new metadata fields via existing patterns |
| strsim | 0.11 | Levenshtein distance for "did you mean?" suggestions | No change |
| cc | 1.x (optional) | C++ shim compilation for extension builds | No change |

### Dev Dependencies
| Technology | Version | Purpose | v0.6.0 Impact |
|------------|---------|---------|---------------|
| proptest | 1.11 | Property-based testing for expansion, parsing, DDL round-trip | No change. New proptest strategies for semi-additive/window metric expansion |
| cargo-husky | 1.x | Pre-commit hooks | No change |

## What NOT to Add

### No Template Engine for GET_DDL
**Considered:** `tera`, `askama`, `minijinja` for DDL reconstruction.
**Decision:** Do not add. GET_DDL output is a single `CREATE SEMANTIC VIEW` statement. The DDL grammar is fixed and fully under our control. A `fn get_ddl(name: &str, def: &SemanticViewDefinition) -> String` with `write!` macros is simpler, has zero dependencies, and is easier to test. Template engines add compilation time and a DSL learning curve for ~100 lines of string building.

### No Regex Crate for Wildcard Expansion
**Considered:** `regex` crate for `customer.*` pattern matching.
**Decision:** Do not add. Wildcard syntax is limited to `<table_alias>.*` -- a single pattern that splits on `.` and checks for `*`. This is `str::split_once('.')` + exact match, not a regex problem. Adding `regex` (~300KB compile) for one string split is wasteful.

### No Additional Parsing Library for NON ADDITIVE BY / PARTITION BY EXCLUDING
**Considered:** `nom`, `pest`, `lalrpop` for parsing new metric modifier clauses.
**Decision:** Do not add. The existing `body_parser.rs` state machine handles all current clause parsing. The new modifiers (`NON ADDITIVE BY (dim1 DESC, dim2 ASC)` and `PARTITION BY EXCLUDING (dim1)`) follow the same parenthesized-list pattern already parsed by `split_at_depth0_commas`. Extend the existing metric parser to recognize these keywords after the `AS <expr>` portion.

### No chrono/time Crate
**Considered:** `chrono` or `time` for timestamp handling in metadata.
**Decision:** Do not add. Timestamps are already stored as VARCHAR strings via DuckDB's `now()` function (established in v0.5.5). The pattern is: capture via SQL, store as string, output as string. No Rust-side date manipulation needed.

## Integration Points for New Features

### Model Layer (`src/model.rs`)

New fields on existing structs (all with `#[serde(default)]` for backward compat):

```rust
// On SemanticViewDefinition:
pub comment: Option<String>,           // View-level comment

// On Dimension:
pub comment: Option<String>,           // COMMENT = '...'
pub synonyms: Vec<String>,             // WITH SYNONYMS = ('...', '...')

// On Metric:
pub comment: Option<String>,
pub synonyms: Vec<String>,
pub is_private: bool,                  // PRIVATE modifier (default: false = PUBLIC)
pub non_additive_dims: Vec<NonAdditiveDim>,  // NON ADDITIVE BY (...)
pub partition_by_excluding: Vec<String>,     // PARTITION BY EXCLUDING (...)

// On Fact:
pub comment: Option<String>,
pub synonyms: Vec<String>,
pub is_private: bool,

// On TableRef:
pub comment: Option<String>,
pub synonyms: Vec<String>,

// New struct:
pub struct NonAdditiveDim {
    pub name: String,                  // dimension name reference
    pub descending: bool,              // DESC (default: false = ASC)
    pub nulls_first: bool,             // NULLS FIRST (default: false = NULLS LAST)
}
```

**Confidence:** HIGH -- follows established `#[serde(default, skip_serializing_if)]` pattern used for every model extension since v0.2.0.

### Body Parser (`src/body_parser.rs`)

Extensions needed in `parse_single_metric_entry` and `parse_metrics_clause`:

1. **NON ADDITIVE BY:** After parsing `AS <expr>`, check for `NON ADDITIVE BY (` keyword sequence. Parse comma-separated dimension references with optional `ASC`/`DESC` and `NULLS FIRST`/`NULLS LAST` modifiers. Reuse `split_at_depth0_commas` for the parenthesized list.

2. **PARTITION BY EXCLUDING:** After parsing `AS <expr>`, check for `PARTITION BY EXCLUDING (` keyword sequence. Parse comma-separated dimension references (no sort modifiers).

3. **COMMENT/SYNONYMS/PRIVATE/PUBLIC on table entries:** Extend `parse_single_table_entry` to recognize `COMMENT = '...'` and `WITH SYNONYMS = ('...', '...')` after the PRIMARY KEY / UNIQUE clauses.

4. **COMMENT/SYNONYMS/PRIVATE/PUBLIC on dim/metric/fact entries:** Extend `parse_single_qualified_entry` and `parse_single_metric_entry` to recognize these modifiers after the expression.

**Confidence:** HIGH -- the parser is a hand-written state machine with established extension patterns. Each new modifier is a keyword check + parenthesized list parse, identical in structure to existing USING RELATIONSHIPS parsing.

### Expansion Engine (`src/expand/sql_gen.rs`)

Three new expansion modes need to coexist with the current grouped-aggregation path:

1. **Semi-additive metrics (NON ADDITIVE BY):**
   - For each semi-additive metric, generate a subquery or CTE that uses `LAST_VALUE(expr IGNORE NULLS) OVER (PARTITION BY <group_dims> ORDER BY <non_additive_dims>)` followed by `DISTINCT` to collapse window results.
   - The tricky part: mixing semi-additive and regular metrics in one query. Snowflake handles this by computing semi-additive values first (via window), then aggregating. Our expansion should do the same: wrap the base query with window functions, then outer-aggregate.
   - **Approach:** Two-pass expansion. Inner query computes window functions for semi-additive metrics alongside raw values. Outer query does GROUP BY with regular aggregates on the window-computed values.

2. **Window function metrics (PARTITION BY EXCLUDING):**
   - These metrics must NOT be grouped. They produce row-level output with a window function applied.
   - When the query includes PARTITION BY EXCLUDING metrics, skip GROUP BY entirely. All dimensions appear as bare columns; window metrics get `OVER (PARTITION BY <all_queried_dims EXCEPT excluded>)`.
   - **Constraint:** Cannot mix regular aggregate metrics and window metrics in the same query (Snowflake enforces this too). Detect and reject at expand time.

3. **Queryable FACTS:**
   - New query mode: `FROM semantic_view('v', facts := ['f1', 'f2'])` or `FROM semantic_view('v', facts := ['f1'], dimensions := ['d1'])`.
   - No GROUP BY. Facts are row-level expressions. Dimensions in fact-query mode are bare columns (no aggregation).
   - **Constraint:** Cannot mix facts and metrics in the same query (matches Snowflake: "You cannot specify both FACTS and METRICS in the same clause"). Detect and reject.

**Confidence:** HIGH for facts and window metrics (straightforward expansion paths). MEDIUM for semi-additive metrics (two-pass expansion is more complex; needs careful testing of mixed metric queries).

### DDL Detection (`src/parse.rs`)

New `DdlKind` variants needed:

```rust
pub enum DdlKind {
    // ... existing variants ...
    ShowColumns,           // SHOW COLUMNS IN VIEW <name>
    GetDdl,                // GET_DDL('SEMANTIC_VIEW', '<name>')
}
```

**Note on GET_DDL:** Snowflake's `GET_DDL` is a built-in function, not DDL syntax. For DuckDB, implement as a scalar function or table function: `SELECT get_semantic_view_ddl('view_name')`. This avoids parser hook complexity. The function reads from the catalog and reconstructs the DDL string.

**Confidence:** HIGH -- follows established `DdlKind` dispatch pattern.

### Query Interface (`src/query/table_function.rs`)

The `QueryRequest` struct needs extension:

```rust
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
    pub facts: Vec<String>,          // NEW: fact names for row-level query mode
}
```

The `semantic_view` table function bind needs to accept a `facts` named parameter and dispatch to the appropriate expansion mode.

**Confidence:** HIGH -- mirrors existing `dimensions`/`metrics` parameter handling.

### SHOW Enhancements (`src/ddl/`)

| Feature | Implementation | New Files |
|---------|---------------|-----------|
| SHOW ... IN SCHEMA/DATABASE | Filter by `database_name`/`schema_name` in VTab bind | No -- extend existing VTabs |
| TERSE mode | Conditional column declaration in VTab bind (skip comment/synonyms columns) | No -- parameter check in existing VTabs |
| SHOW COLUMNS | New VTab listing all dims/facts/metrics as "columns" | `src/ddl/show_columns.rs` |
| synonyms/comment in SHOW output | Add 2 VARCHAR columns to existing ShowDimRow/ShowMetricRow/ShowFactRow | No -- extend existing structs |

**Column additions for Snowflake alignment:**

Current SHOW SEMANTIC DIMENSIONS has 6 columns. Snowflake has 8 (adds `synonyms`, `comment`). Add these two columns to all three SHOW object commands (dims, metrics, facts).

Current SHOW SEMANTIC VIEWS has 5 columns. Snowflake has 8 (adds `comment`, `owner`, `owner_role_type`). Add `comment` column. Skip `owner`/`owner_role_type` (DuckDB has no role system -- emit empty strings for compatibility).

**Confidence:** HIGH -- pure additive column changes following established VTab patterns.

### DDL Result Pipeline (`shim.cpp`)

The C++ result forwarding pipeline currently handles all `DdlKind` variants. New variants (ShowColumns, GetDdl) route through the same `sv_ddl_execute` -> `duckdb_value_varchar` -> output path. No C++ changes needed unless we add GET_DDL as a separate function outside the parser hook pipeline.

**Decision:** Route GET_DDL through the DDL pipeline for consistency. It's detected as a parser hook form (`GET_DDL('SEMANTIC_VIEW', 'name')`) and rewritten to a table function call. The C++ pipeline returns the single-row VARCHAR result.

**Alternative (preferred):** Register GET_DDL as a standalone scalar function that returns VARCHAR. This is simpler: no parser hook detection needed, no C++ pipeline involvement. Just `SELECT get_semantic_view_ddl('orders')`. Register it alongside the existing table functions at extension init.

**Recommendation:** Use the scalar function approach. It's cleaner, avoids parser hook complexity, and matches how DuckDB users expect functions to work.

**Confidence:** MEDIUM -- the scalar function registration path via duckdb-rs `create_scalar_function` may need investigation. The VTab path (table function returning one row) is proven.

## DuckDB Capabilities Verification

Features the new expansion relies on, verified against DuckDB documentation:

| SQL Feature | DuckDB Support | Used By | Confidence |
|-------------|---------------|---------|------------|
| `LAST_VALUE(expr IGNORE NULLS) OVER (PARTITION BY ... ORDER BY ...)` | YES -- documented, full window function support | Semi-additive metrics | HIGH |
| `ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW` | YES -- default frame with ORDER BY | Semi-additive metrics | HIGH |
| `SUM(expr) OVER (PARTITION BY ...)` | YES -- all aggregates usable as window functions | Window function metrics | HIGH |
| Window functions without GROUP BY | YES -- standard SQL behavior | PARTITION BY EXCLUDING | HIGH |
| `SELECT DISTINCT` on window function output | YES | Semi-additive dedup | HIGH |

## Semi-Additive Expansion Strategy

The most complex new feature. Snowflake's approach: for a semi-additive metric like `SUM(balance) NON ADDITIVE BY (date_dim DESC NULLS FIRST)`, when querying with dimensions [customer, date_dim]:

1. Partition by non-excluded dimensions (customer)
2. Order by the non-additive dimensions (date_dim DESC NULLS FIRST)
3. Take the LAST_VALUE per partition (latest snapshot)
4. Aggregate (SUM) across partitions

**DuckDB expansion pattern:**

```sql
-- Inner: compute last-value per snapshot group
WITH _snapshot AS (
  SELECT
    customer AS "customer",
    date_dim AS "date_dim",
    LAST_VALUE(balance IGNORE NULLS) OVER (
      PARTITION BY customer
      ORDER BY date_dim DESC NULLS FIRST
      ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
    ) AS "_semi_balance"
  FROM ...
)
-- Outer: aggregate the snapshot values
SELECT
  "customer",
  SUM("_semi_balance") AS "total_balance"
FROM _snapshot
GROUP BY 1
```

This two-pass CTE approach keeps the expansion engine's output as a single SQL string, compatible with the existing `execute_sql_raw` pipeline. No changes to the query execution layer.

**Confidence:** MEDIUM -- the CTE wrapping approach is sound in principle but needs validation against edge cases: multiple semi-additive metrics with different NON ADDITIVE BY dimensions, mixing semi-additive and regular metrics, semi-additive metrics with no non-additive dimensions in the query.

## Wildcard Expansion Strategy

For `dimensions := ['customer.*']`:

1. At expand time, parse `customer.*` as (table_alias=`customer`, pattern=`*`)
2. Find all dimensions where `source_table == "customer"` (or alias matches)
3. Expand the wildcard into the concrete dimension names
4. Proceed with normal expansion

This happens before validation, so the expanded names go through the existing duplicate/unknown checks.

**Confidence:** HIGH -- pure string manipulation at the request-parsing layer.

## Sources

- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- NON ADDITIVE BY, COMMENT, SYNONYMS, PRIVATE/PUBLIC, PARTITION BY EXCLUDING syntax
- [Snowflake semi-additive metrics release note (March 5, 2026)](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY behavior specification
- [Snowflake SEMANTIC_VIEW query construct](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- FACTS query mode, wildcard `table.*` syntax, METRICS/FACTS mutual exclusion
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- TERSE, IN scope, column schema
- [Snowflake SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- synonyms/comment columns, no TERSE
- [Snowflake SHOW COLUMNS](https://docs.snowflake.com/en/sql-reference/sql/show-columns) -- semantic view support, column schema
- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- SYNONYMS/COMMENT property rows
- [Snowflake GET_DDL](https://docs.snowflake.com/en/sql-reference/functions/get_ddl) -- function-based DDL reconstruction
- [DuckDB Window Functions](https://duckdb.org/docs/current/sql/functions/window_functions) -- LAST_VALUE, IGNORE NULLS, frame specs
