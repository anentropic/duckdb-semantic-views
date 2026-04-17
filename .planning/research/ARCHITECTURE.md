# Architecture: v0.6.0 Snowflake SQL DDL Parity

**Domain:** DuckDB semantic views extension -- Snowflake DDL parity features integration
**Researched:** 2026-04-09
**Confidence:** HIGH (direct codebase analysis, Snowflake official docs verified via WebFetch)

## Executive Summary

v0.6.0 adds seven feature groups to the existing extension. This document maps each feature to its integration points, identifies new vs modified components, and proposes a build order that respects dependencies. The key architectural insight is that these features fall into three tiers of integration complexity:

**Tier 1 (Model + DDL only):** Metadata (COMMENT/SYNONYMS/PRIVATE), GET_DDL reconstruction, SHOW enhancements. These add fields to the model, extend the body parser, and add new VTabs or modify existing ones. Zero changes to the expansion pipeline.

**Tier 2 (Expansion modifications):** Wildcard selection, queryable FACTS. These add new resolution logic in the expansion pipeline but do not fundamentally change the SELECT/FROM/GROUP BY structure.

**Tier 3 (Expansion structural changes):** Semi-additive metrics, window function metrics. These require new expansion modes that produce fundamentally different SQL -- CTEs with window functions, QUALIFY clauses, or queries without GROUP BY despite having metrics. These are the highest-risk features.

The recommended build order is: Tier 1 first (unlocks model changes needed by Tier 2/3), then Tier 2 (simpler expansion changes), then Tier 3 (structural expansion changes).

## Current Architecture (Reference)

```
User SQL                  Parser Hook (C++ shim)          Rust Extension
---------                 ----------------------          --------------
CREATE SEMANTIC VIEW  --> detect_ddl_prefix()        --> validate_and_rewrite()
  name AS ...                                              |
                                                    parse_keyword_body()
                                                           |
                                                    SemanticViewDefinition (JSON)
                                                           |
                                                    catalog_insert() --> HashMap + pragma_query_t
                                                           
FROM semantic_view(   --> VTab bind()               --> expand()
  'name',                                                  |
  dimensions := [...],                              resolve dims/metrics
  metrics := [...]                                  inline facts/derived
)                                                   check fan traps
                                                    build SELECT/FROM/JOIN/GROUP BY
                                                           |
                                                    execute_sql_raw(expanded_sql)
                                                           |
                                                    duckdb_vector_reference_vector --> output
```

### Key Components

| Component | File(s) | Role |
|-----------|---------|------|
| Model | `model.rs` | `SemanticViewDefinition`, `Dimension`, `Metric`, `Fact`, `Join`, `TableRef` structs |
| Body Parser | `body_parser.rs` | State machine: parses `AS TABLES(...) RELATIONSHIPS(...) FACTS(...) DIMENSIONS(...) METRICS(...)` |
| Parse/Rewrite | `parse.rs` | DDL detection, validation, rewrite to `SELECT * FROM fn_from_json('name', 'json')` |
| Catalog | `catalog.rs` | `CatalogState = Arc<RwLock<HashMap<String, String>>>`, init/insert/upsert/delete |
| Expansion | `expand/sql_gen.rs` | `expand()` -- builds SELECT/FROM/JOIN/GROUP BY from definition + request |
| Join Resolver | `expand/join_resolver.rs` | PK/FK graph-based join resolution and ON clause synthesis |
| Fan Trap | `expand/fan_trap.rs` | LCA-based cardinality analysis |
| Facts | `expand/facts.rs` | Fact inlining, derived metric resolution via toposort |
| Role Playing | `expand/role_playing.rs` | USING context resolution for scoped aliases |
| Query VTab | `query/table_function.rs` | `SemanticViewVTab` -- bind/init/func for `semantic_view()` |
| DDL VTabs | `ddl/*.rs` | Define, Drop, Alter, Describe, List, Show* -- 11 VTab implementations |
| Persistence | `ddl/persist.rs` | Parameterized prepared statements for catalog writes |

## Feature Integration Analysis

### 1. Metadata: COMMENT, SYNONYMS, PRIVATE

**Integration tier:** Tier 1 (Model + DDL only)
**Expansion impact:** NONE

#### What Changes

**model.rs** -- Add fields to existing structs:

```rust
pub struct Dimension {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    // Dimensions are always PUBLIC in Snowflake -- no visibility field needed
}

pub struct Metric {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    #[serde(default, skip_serializing_if = "Visibility::is_default")]
    pub visibility: Visibility,
    // NON ADDITIVE BY dims stored here too (see section 2)
}

pub struct Fact {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    #[serde(default, skip_serializing_if = "Visibility::is_default")]
    pub visibility: Visibility,
}

pub struct TableRef {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
}

pub struct SemanticViewDefinition {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
}
```

All new fields use `#[serde(default)]` for backward-compatible deserialization of existing stored JSON.

**body_parser.rs** -- Extend entry parsing within each clause:

The body parser currently uses `parse_qualified_entries` for dims/metrics/facts. Each entry is `alias.name AS expr`. The new syntax adds optional prefixes and suffixes:

```
METRICS (
    [PRIVATE] alias.name [USING (...)] [NON ADDITIVE BY (...)] AS expr
        [WITH SYNONYMS = ('syn1', 'syn2')]
        [COMMENT = 'text']
)
```

The parser needs to:
1. Check for `PRIVATE`/`PUBLIC` keyword before `alias.name`
2. After `AS expr`, scan for `WITH SYNONYMS = (...)` and `COMMENT = '...'`
3. Same pattern for FACTS and DIMENSIONS (minus PRIVATE for dims, minus NON ADDITIVE BY for dims/facts)
4. For TABLES entries: `WITH SYNONYMS` and `COMMENT` after the table declaration

**parse.rs** -- No structural changes needed. The rewrite path already serializes `SemanticViewDefinition` to JSON.

**ddl/describe.rs** -- Extend `collect_*_rows()` to emit COMMENT, SYNONYMS, VISIBILITY properties per object.

**ddl/show_dims.rs, show_metrics.rs, show_facts.rs** -- Add `synonyms` and `comment` columns to output schemas (Snowflake includes these in SHOW output).

**ddl/list.rs** -- Add `comment` column for SHOW SEMANTIC VIEWS (Snowflake full mode includes comment).

#### New Components

- `model::Visibility` enum (new type, 6 lines)

#### Modified Components

- `model.rs` -- 5 struct changes (add fields)
- `body_parser.rs` -- entry parsing logic extended
- `ddl/describe.rs` -- new property rows
- `ddl/show_dims.rs`, `show_metrics.rs`, `show_facts.rs` -- new output columns
- `ddl/list.rs` -- new output column

### 2. Semi-Additive Metrics (NON ADDITIVE BY)

**Integration tier:** Tier 3 (Expansion structural change)
**Expansion impact:** MAJOR -- new SQL generation path

#### Snowflake Semantics

`NON ADDITIVE BY (year_dim, month_dim, day_dim)` means: when the query groups by dimensions that overlap with the NON ADDITIVE list, use "last snapshot" aggregation instead of SUM across those dimensions. The rows are sorted by the non-additive dimensions, and LAST_VALUE is used to select the latest snapshot before aggregating.

#### What Changes

**model.rs** -- Add to Metric:

```rust
pub struct Metric {
    // ... existing fields ...
    /// Dimensions that this metric is non-additive by.
    /// When non-empty, expansion uses snapshot aggregation for these dims.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_additive_by: Vec<NonAdditiveDim>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NonAdditiveDim {
    pub dimension: String,
    #[serde(default)]
    pub order: SortOrder,
    #[serde(default)]
    pub nulls: NullsOrder,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum SortOrder { #[default] Asc, Desc }

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum NullsOrder { #[default] Last, First }
```

**body_parser.rs** -- Parse `NON ADDITIVE BY (dim [ASC|DESC] [NULLS FIRST|LAST], ...)` after USING clause but before AS in metric entries.

**expand/sql_gen.rs** -- This is the critical change. The `expand()` function currently produces:

```sql
SELECT dims, agg_metrics FROM base JOIN... GROUP BY 1,2,...
```

For a semi-additive metric, the expansion must produce a CTE-based query:

```sql
WITH __sv_snapshot AS (
    SELECT dims, measure_expr,
           ROW_NUMBER() OVER (
               PARTITION BY non_na_dims
               ORDER BY na_dim1 DESC, na_dim2 DESC, na_dim3 DESC
           ) AS __sv_rn
    FROM base JOIN...
)
SELECT non_na_dims,
       agg_fn(__sv_snapshot.measure_expr) AS metric_name
FROM __sv_snapshot
WHERE __sv_rn = 1
GROUP BY 1, 2, ...
```

The approach:
1. Identify which requested dimensions are in `non_additive_by` lists (NA dims) vs not (regular dims)
2. The inner CTE selects all dims + the raw measure expression (not aggregated)
3. ROW_NUMBER partitions by non-NA dims, orders by NA dims (user-specified order)
4. The outer query filters to `__sv_rn = 1` and aggregates the measure

**Key complexity:** When a query mixes semi-additive and regular metrics, the expansion must handle both in a single query. Two strategies:
- **Strategy A (recommended):** Separate CTE per semi-additive metric, join results on regular dims. Clean but produces N+1 CTEs for N semi-additive metrics.
- **Strategy B:** Single-pass with conditional aggregation. More complex, harder to debug.

Recommend Strategy A for correctness-first approach.

#### New Components

- `expand/semi_additive.rs` -- New submodule for CTE generation logic
- `model::NonAdditiveDim`, `SortOrder`, `NullsOrder` types

#### Modified Components

- `expand/sql_gen.rs` -- `expand()` branches on presence of non-additive metrics
- `expand/mod.rs` -- new submodule declaration
- `body_parser.rs` -- NON ADDITIVE BY parsing
- `model.rs` -- Metric struct extension

### 3. Window Function Metrics (PARTITION BY EXCLUDING)

**Integration tier:** Tier 3 (Expansion structural change)
**Expansion impact:** MAJOR -- expansion without GROUP BY

#### Snowflake Semantics

Window function metrics use `OVER(PARTITION BY ... ORDER BY ...)` instead of aggregate functions. The query produces one row per input row (no aggregation). `PARTITION BY EXCLUDING dims` means "partition by all queried dimensions except the excluded ones."

#### What Changes

**model.rs** -- Add window function metadata to Metric:

```rust
pub struct Metric {
    // ... existing fields ...
    /// When Some, this metric is a window function metric.
    /// The expansion omits GROUP BY and uses OVER() instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_spec: Option<WindowSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowSpec {
    /// Dimensions to partition by, or EXCLUDING list.
    pub partition: PartitionSpec,
    /// ORDER BY within the window.
    pub order_by: Vec<WindowOrderBy>,
    /// Optional frame clause (ROWS/RANGE BETWEEN...).
    pub frame: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PartitionSpec {
    Include(Vec<String>),    // explicit dimension list
    Excluding(Vec<String>),  // EXCLUDING dims
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowOrderBy {
    pub expr: String,
    pub order: SortOrder,
    pub nulls: NullsOrder,
}
```

**body_parser.rs** -- Parse window function metric syntax. The expr contains the full window function call including OVER clause. The parser needs to recognize the `OVER (PARTITION BY EXCLUDING ...)` pattern.

**expand/sql_gen.rs** -- When any requested metric has `window_spec`, the expansion mode changes:

```sql
-- Window function metric: no GROUP BY
SELECT
    dim1 AS "dim1",
    dim2 AS "dim2",
    window_fn(measure) OVER (
        PARTITION BY dim1
        ORDER BY dim2 DESC
    ) AS "metric_name"
FROM base JOIN...
```

**Key question:** Can window function metrics and regular aggregate metrics coexist in a single query? In Snowflake, yes -- the query would use a subquery for the aggregation, then apply the window function on top. However, for v0.6.0, recommend requiring that window function metrics are queried separately (not mixed with aggregate metrics). This avoids complex query planning and is consistent with Snowflake's approach where window metrics reference other metrics.

#### New Components

- `expand/window.rs` -- New submodule for window function SQL generation
- `model::WindowSpec`, `PartitionSpec`, `WindowOrderBy` types

#### Modified Components

- `expand/sql_gen.rs` -- branches on window metric presence
- `body_parser.rs` -- window function metric parsing
- `model.rs` -- Metric struct extension

### 4. GET_DDL Reconstruction

**Integration tier:** Tier 1 (DDL only)
**Expansion impact:** NONE

#### What Changes

GET_DDL reconstructs the `CREATE SEMANTIC VIEW` DDL from the stored JSON definition. This is a pure rendering operation -- read JSON from catalog, format as DDL text.

**New file: `ddl/get_ddl.rs`** -- Implements a VTab that:
1. Takes a view name parameter
2. Reads the JSON definition from CatalogState
3. Deserializes to `SemanticViewDefinition`
4. Renders back to DDL syntax

The rendering function walks each section:

```rust
fn render_ddl(name: &str, def: &SemanticViewDefinition) -> String {
    let mut out = format!("CREATE SEMANTIC VIEW {name} AS\n");
    render_tables(&mut out, &def.tables);
    if !def.joins.is_empty() {
        render_relationships(&mut out, &def.joins, &def.tables);
    }
    if !def.facts.is_empty() {
        render_facts(&mut out, &def.facts);
    }
    if !def.dimensions.is_empty() {
        render_dimensions(&mut out, &def.dimensions);
    }
    render_metrics(&mut out, &def.metrics);
    out
}
```

Each `render_*` function reconstructs the clause from struct fields:
- Tables: `alias AS physical_table [PRIMARY KEY (cols)] [UNIQUE (cols)]`
- Relationships: `name AS from_alias(fk_cols) REFERENCES to_alias[(ref_cols)]`
- Facts/Dims/Metrics: `[PRIVATE] alias.name AS expr [USING (...)] [NON ADDITIVE BY (...)] [WITH SYNONYMS = (...)] [COMMENT = '...']`

**parse.rs** -- Add `DdlKind::GetDdl` variant for `GET_DDL SEMANTIC VIEW name` detection. Rewrite to `SELECT * FROM get_ddl_semantic_view('name')`.

**lib.rs** -- Register the new VTab.

#### New Components

- `ddl/get_ddl.rs` -- VTab implementation + render functions (~200-300 lines)
- `DdlKind::GetDdl` variant in `parse.rs`

#### Modified Components

- `ddl/mod.rs` -- add `pub mod get_ddl;`
- `parse.rs` -- add detection/rewrite for GET_DDL
- `lib.rs` -- register VTab

### 5. Queryable FACTS

**Integration tier:** Tier 2 (Expansion modification)
**Expansion impact:** MODERATE -- new expansion mode

#### Snowflake Semantics

In Snowflake, FACTS can appear in the DIMENSIONS clause or in a separate FACTS clause of the `SEMANTIC_VIEW()` query. Unlike metrics, facts are NOT aggregated. The query returns row-level data with optional GROUP BY for any co-queried dimensions.

#### What Changes

**expand/types.rs** -- Extend `QueryRequest`:

```rust
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
    pub facts: Vec<String>,  // NEW
}
```

**expand/sql_gen.rs** -- When facts are requested:

```sql
-- Facts only (no metrics): row-level, no GROUP BY
SELECT
    dim1 AS "dim1",
    fact_expr AS "fact_name"
FROM base JOIN...

-- Facts + dimensions (no metrics): row-level grouped
-- Snowflake: "the query does not group the facts"
-- This means SELECT DISTINCT dims, fact_exprs (no aggregation)
SELECT DISTINCT
    dim1 AS "dim1",
    fact_expr AS "fact_name"
FROM base JOIN...

-- Facts + metrics: facts appear alongside aggregated metrics
-- Facts must be either in GROUP BY or aggregated
-- Snowflake resolves this by treating facts in a separate subquery
```

The key insight from Snowflake docs: "Unlike dimensions specified in the DIMENSIONS clause, the query does not group the facts specified in the FACTS clause." This means facts produce row-level output. When combined with metrics, the expansion becomes more complex.

**Recommendation:** For v0.6.0, support facts-only and facts+dimensions (row-level queries). Defer facts+metrics to a future milestone -- the semantic complexity of mixing aggregated and non-aggregated columns in a single query is significant.

**query/table_function.rs** -- Add `facts` named parameter to `SemanticViewVTab`:

```rust
let facts = match bind.get_named_parameter("facts") {
    Some(ref val) => unsafe { extract_list_strings(val) },
    None => vec![],
};
```

#### New Components

- None (modifications to existing)

#### Modified Components

- `expand/types.rs` -- `QueryRequest` gains `facts` field
- `expand/sql_gen.rs` -- new branch for fact queries
- `expand/resolution.rs` -- add `find_fact()` resolution (parallel to `find_dimension`)
- `query/table_function.rs` -- parse `facts` parameter

### 6. Wildcard Selection

**Integration tier:** Tier 2 (Expansion modification)
**Expansion impact:** MODERATE -- resolution-time expansion

#### Snowflake Semantics

`customer.*` in DIMENSIONS or METRICS expands to all dimensions/metrics scoped to the `customer` table alias. An unqualified `*` is not allowed.

#### What Changes

**expand/sql_gen.rs** -- Before resolving individual dimension/metric names, expand wildcards:

```rust
fn expand_wildcards(
    names: &[String],
    items: &[impl HasSourceTable + HasName],
    kind: &str,  // "dimension" or "metric"
) -> Result<Vec<String>, ExpandError> {
    let mut result = Vec::new();
    for name in names {
        if name.ends_with(".*") {
            let alias = &name[..name.len() - 2];
            let matches: Vec<_> = items.iter()
                .filter(|item| item.source_table()
                    .map_or(false, |st| st.eq_ignore_ascii_case(alias)))
                .map(|item| item.name().to_string())
                .collect();
            if matches.is_empty() {
                return Err(/* no items for alias */);
            }
            result.extend(matches);
        } else {
            result.push(name.clone());
        }
    }
    Ok(result)
}
```

This runs BEFORE the existing resolution loop, replacing `customer.*` with `[customer_name, customer_region, ...]`.

**query/table_function.rs** -- No change needed; wildcards are just strings passed via the `dimensions`/`metrics` list parameters. Expansion happens in `expand()`.

**Visibility filter:** When expanding wildcards, PRIVATE metrics and facts must be excluded. The wildcard resolver needs access to the `Visibility` field.

#### New Components

- None (modifications to existing)

#### Modified Components

- `expand/sql_gen.rs` or new `expand/wildcards.rs` -- wildcard expansion logic
- `expand/types.rs` -- possible new error variant `NoItemsForAlias`

### 7. SHOW Enhancements

**Integration tier:** Tier 1 (DDL only)
**Expansion impact:** NONE

#### 7a. IN SCHEMA/DATABASE Scope Filtering

**parse.rs** -- The existing `parse_show_filter_clauses` already handles `IN view_name`. Extend to handle `IN SCHEMA schema_name` and `IN DATABASE db_name`:

```rust
struct ShowClauses<'a> {
    // ... existing ...
    in_schema: Option<&'a str>,   // NEW
    in_database: Option<&'a str>, // NEW
}
```

**ddl/show_*.rs and ddl/list.rs** -- The `_all` VTab variants currently return all views across the catalog. With scope filtering, the rewritten SQL adds WHERE clauses:

```sql
SELECT * FROM list_semantic_views()
WHERE database_name = 'my_db' AND schema_name = 'my_schema'
```

This works because the VTab output already includes `database_name` and `schema_name` columns. The filter can be injected at the SQL rewrite level in `parse.rs` (same pattern as LIKE/STARTS WITH).

#### 7b. TERSE Mode

**parse.rs** -- Detect `TERSE` keyword after `SHOW`:

```
SHOW TERSE SEMANTIC VIEWS [LIKE ...] [IN ...] [STARTS WITH ...] [LIMIT ...]
```

Add new `DdlKind` variants: `ShowTerse`, or better, add a `terse: bool` field to the rewrite output.

The simplest approach: TERSE mode is handled at the SQL rewrite level by selecting a subset of columns:

```sql
-- Full mode (current)
SELECT * FROM list_semantic_views()

-- TERSE mode
SELECT created_on, name, kind, database_name, schema_name
FROM list_semantic_views()
```

This avoids creating new VTabs. The column subset is fixed per Snowflake spec:
- SHOW SEMANTIC VIEWS TERSE: `created_on, name, kind, database_name, schema_name`
- SHOW SEMANTIC DIMENSIONS TERSE: not specified by Snowflake (no TERSE mode documented)

**Recommendation:** Implement TERSE only for SHOW SEMANTIC VIEWS (where Snowflake specifies it). The SHOW SEMANTIC DIMENSIONS/METRICS/FACTS commands do not have a TERSE variant in Snowflake docs.

#### 7c. SHOW COLUMNS

**New file: `ddl/show_columns.rs`** -- A new VTab that returns all components (dimensions, metrics, facts) of a semantic view with their types:

Output schema (Snowflake-aligned):
```
column_name | kind | data_type | comment
```

Where `kind` is `DIMENSION`, `METRIC`, or `FACT`.

**parse.rs** -- Detect `SHOW COLUMNS IN SEMANTIC VIEW name`:

```rust
DdlKind::ShowColumns => {
    // Rewrite to: SELECT * FROM show_semantic_columns('name')
}
```

#### New Components

- `ddl/show_columns.rs` -- new VTab (~150 lines)
- `DdlKind::ShowColumns` or `DdlKind::ShowTerse` in `parse.rs`

#### Modified Components

- `parse.rs` -- IN SCHEMA/DATABASE parsing, TERSE detection, SHOW COLUMNS detection
- `ddl/mod.rs` -- add `pub mod show_columns;`
- `lib.rs` -- register new VTab

### 8. ALTER SET/UNSET COMMENT

**Integration tier:** Tier 1 (DDL only)
**Expansion impact:** NONE

#### What Changes

**parse.rs** -- New DDL forms:

```sql
ALTER SEMANTIC VIEW name SET COMMENT = 'text'
ALTER SEMANTIC VIEW name UNSET COMMENT
```

These rewrite to new VTab calls:

```sql
SELECT * FROM alter_semantic_view_set_comment('name', 'text')
SELECT * FROM alter_semantic_view_unset_comment('name')
```

**New file: `ddl/alter_comment.rs`** -- VTab that:
1. Reads existing JSON from catalog
2. Deserializes to `SemanticViewDefinition`
3. Sets/clears the `comment` field
4. Re-serializes and updates catalog (both HashMap and persistent storage)

**parse.rs** -- Extend ALTER detection to handle SET COMMENT / UNSET COMMENT in addition to RENAME TO.

#### New Components

- `ddl/alter_comment.rs` -- new VTab (~100 lines)
- New `DdlKind::AlterSetComment`, `DdlKind::AlterUnsetComment` variants

#### Modified Components

- `parse.rs` -- ALTER subcommand parsing
- `ddl/mod.rs` -- add module
- `lib.rs` -- register VTab

## Component Boundaries

```
                                  +-----------+
                                  |  model.rs |
                                  | (structs) |
                                  +-----+-----+
                                        |
              +-------------------------+-------------------------+
              |                         |                         |
     +--------+--------+      +--------+--------+       +--------+--------+
     | body_parser.rs   |      |   expand/        |       |   ddl/           |
     | (parse DDL body) |      | (SQL generation) |       | (VTab handlers)  |
     +--------+--------+      +--------+--------+       +--------+--------+
              |                         |                         |
              v                         v                         v
     +--------+--------+      +--------+--------+       +--------+--------+
     |   parse.rs       |      | query/           |       | catalog.rs       |
     | (detect/rewrite) |      | (table function) |       | (state/persist)  |
     +--------+--------+      +--------+--------+       +--------+--------+
              |                         |                         |
              +-------------------------+-------------------------+
                                        |
                                  +-----+-----+
                                  | lib.rs     |
                                  | (init/reg) |
                                  +-----------+
```

### New vs Modified Summary

| Feature | New Files | Modified Files |
|---------|-----------|----------------|
| Metadata (COMMENT/SYNONYMS/PRIVATE) | None | model.rs, body_parser.rs, ddl/describe.rs, ddl/show_*.rs, ddl/list.rs |
| Semi-additive metrics | expand/semi_additive.rs | model.rs, body_parser.rs, expand/sql_gen.rs, expand/mod.rs |
| Window function metrics | expand/window.rs | model.rs, body_parser.rs, expand/sql_gen.rs, expand/mod.rs |
| GET_DDL | ddl/get_ddl.rs | parse.rs, ddl/mod.rs, lib.rs |
| Queryable FACTS | None | expand/types.rs, expand/sql_gen.rs, expand/resolution.rs, query/table_function.rs |
| Wildcard selection | expand/wildcards.rs (optional) | expand/sql_gen.rs |
| SHOW enhancements | ddl/show_columns.rs | parse.rs, ddl/mod.rs, lib.rs |
| ALTER SET/UNSET COMMENT | ddl/alter_comment.rs | parse.rs, ddl/mod.rs, lib.rs |

## Data Flow Changes

### Current Data Flow (Query Path)

```
semantic_view('v', dims=['a'], metrics=['m'])
    --> bind: catalog lookup --> expand(def, req) --> expanded SQL
    --> bind: type inference (LIMIT 0 or stored)
    --> bind: build_execution_sql (type cast wrapper)
    --> func: execute_sql_raw(execution_sql) --> stream chunks
```

### New Data Flows

**Wildcard Resolution (in bind):**
```
dims=['customer.*'] --> expand_wildcards(dims, def.dimensions)
    --> dims=['customer_name', 'customer_region', ...] --> existing flow
```

**Queryable FACTS (in bind):**
```
facts=['order_count'] --> expand(def, req_with_facts)
    --> SELECT DISTINCT dims, fact_exprs FROM... (no GROUP BY, no aggregation)
```

**Semi-Additive Metrics (in expand):**
```
metrics=['balance'] where balance.non_additive_by=['year','month','day']
    --> detect semi-additive --> generate CTE with ROW_NUMBER
    --> WITH __sv_snapshot AS (
            SELECT ..., ROW_NUMBER() OVER(PARTITION BY non_na_dims ORDER BY na_dims DESC) AS __sv_rn
            FROM ...
        )
        SELECT non_na_dims, AGG(measure) FROM __sv_snapshot WHERE __sv_rn = 1 GROUP BY ...
```

**GET_DDL (new DDL path):**
```
GET_DDL SEMANTIC VIEW 'name'
    --> parse.rs: detect, rewrite to SELECT * FROM get_ddl_semantic_view('name')
    --> VTab bind: catalog lookup, deserialize, render_ddl()
    --> func: emit single-row VARCHAR result
```

## Patterns to Follow

### Pattern 1: Backward-Compatible Serde Fields

**What:** Every new field on model structs uses `#[serde(default, skip_serializing_if = "...")]`
**When:** Always when adding fields to serialized structs
**Why:** Existing stored JSON must deserialize without error. New JSON should omit default values to minimize storage.

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub comment: Option<String>,

#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub synonyms: Vec<String>,

#[serde(default, skip_serializing_if = "Visibility::is_default")]
pub visibility: Visibility,
```

### Pattern 2: SQL Rewrite at Parse Level (for new DDL forms)

**What:** New DDL commands are detected in `parse.rs` and rewritten to `SELECT * FROM fn(args)`
**When:** Adding GET_DDL, ALTER SET COMMENT, SHOW COLUMNS, TERSE mode
**Why:** Consistent with existing architecture -- parser hooks intercept DDL, Rust rewrites to function calls

### Pattern 3: CTE Wrapping for Complex Expansion

**What:** Use CTEs (`WITH __sv_* AS (...)`) when the expansion needs intermediate steps
**When:** Semi-additive metrics (snapshot selection), potentially window function metrics
**Why:** CTEs keep the SQL readable and debuggable; DuckDB optimizes them away

### Pattern 4: VTab-per-DDL-Verb Pattern

**What:** Each DDL operation gets its own VTab implementation
**When:** GET_DDL, ALTER SET COMMENT, SHOW COLUMNS
**Why:** Consistent with existing architecture (11 VTabs already follow this pattern)

## Anti-Patterns to Avoid

### Anti-Pattern 1: Mutating Expansion Based on Metadata

**What:** Using COMMENT/SYNONYMS/PRIVATE fields to change SQL expansion behavior
**Why bad:** Metadata is informational; mixing it with query logic creates coupling
**Instead:** Metadata flows through DDL/SHOW/DESCRIBE paths only. The only metadata that affects expansion is `visibility: Private` (which blocks querying of private facts/metrics) and `non_additive_by` (which is semantic, not metadata).

### Anti-Pattern 2: Mixed Aggregation Modes in Single Query

**What:** Allowing window metrics + aggregate metrics + facts in a single `semantic_view()` call
**Why bad:** Produces SQL that is extremely complex, hard to debug, and may have ambiguous semantics
**Instead:** Validate at bind time that the request is one of: (a) regular dims+metrics, (b) facts-only or facts+dims, (c) window metrics+dims. Return a clear error if modes are mixed.

### Anti-Pattern 3: String Template for GET_DDL Rendering

**What:** Using format strings with interpolation to build DDL output
**Why bad:** SQL injection risk if stored names contain single quotes, incorrect escaping
**Instead:** Use the same identifier quoting (`quote_ident`) already used in expansion

## Suggested Build Order

### Phase 1: Metadata Foundation (Model + Parser)

Add all model struct fields (COMMENT, SYNONYMS, PRIVATE/Visibility), extend body_parser.rs to parse the new syntax elements. This phase produces no new user-visible behavior but lays the foundation for everything else.

**Rationale:** Every subsequent feature needs these model fields. Building them first avoids repeated model modifications.

**Scope:**
- model.rs: Add Visibility enum, comment/synonyms/visibility fields to all structs, NonAdditiveDim/SortOrder/NullsOrder types
- body_parser.rs: Parse COMMENT =, WITH SYNONYMS =, PRIVATE/PUBLIC, NON ADDITIVE BY
- Tests: roundtrip serialization, parser tests for new syntax

### Phase 2: SHOW/DESCRIBE Metadata Columns + SHOW Enhancements

Surface metadata in SHOW/DESCRIBE output. Add TERSE mode, IN SCHEMA/DATABASE, SHOW COLUMNS.

**Rationale:** Once model fields exist, surfacing them in introspection is straightforward. Completing SHOW changes here avoids revisiting these VTabs later.

**Scope:**
- ddl/describe.rs: Emit COMMENT, SYNONYMS, VISIBILITY properties
- ddl/show_dims.rs, show_metrics.rs, show_facts.rs: Add synonyms, comment columns
- ddl/list.rs: Add comment column
- ddl/show_columns.rs: New VTab
- parse.rs: TERSE detection, IN SCHEMA/DATABASE, SHOW COLUMNS detection

### Phase 3: ALTER SET/UNSET COMMENT + GET_DDL

**Rationale:** These are self-contained DDL features that depend on the model fields from Phase 1 but not on expansion changes. GET_DDL tests serve as roundtrip validation for the model.

**Scope:**
- ddl/alter_comment.rs: New VTab
- ddl/get_ddl.rs: New VTab with render functions
- parse.rs: New DdlKind variants and rewrite logic

### Phase 4: Wildcard Selection + Queryable FACTS

**Rationale:** These are moderate expansion changes that share a dependency: both need to resolve items from the definition that were not previously queryable (wildcards expand names, facts expand expressions). Building them together exercises the expansion pipeline modification pattern.

**Scope:**
- expand/sql_gen.rs: Wildcard expansion before resolution
- expand/types.rs: QueryRequest gains `facts` field
- expand/resolution.rs: `find_fact()` function
- query/table_function.rs: Parse `facts` parameter
- PRIVATE visibility enforcement in wildcard expansion

### Phase 5: Semi-Additive Metrics

**Rationale:** This is the highest-complexity expansion change. All model fields are in place from Phase 1. Building this after simpler expansion changes (Phase 4) means the developer is familiar with the expansion pipeline.

**Scope:**
- expand/semi_additive.rs: CTE-based expansion for NON ADDITIVE BY
- expand/sql_gen.rs: Detection and branching for semi-additive metrics
- Extensive testing: snapshot correctness, mixed regular+semi-additive queries

### Phase 6: Window Function Metrics (if included)

**Rationale:** Most complex feature, orthogonal to semi-additive. Can be deferred to a future milestone if v0.6.0 scope is too large. Currently listed in Out of Scope in PROJECT.md.

**Note:** The milestone context mentions window function metrics, but PROJECT.md lists them as Out of Scope. If included, this should be the last phase due to its structural impact on the expansion pipeline.

**Scope:**
- expand/window.rs: Window function SQL generation
- expand/sql_gen.rs: Window metric detection and no-GROUP-BY path
- Validation that window metrics cannot be mixed with aggregate metrics

## Scalability Considerations

| Concern | Current (v0.5.5) | After v0.6.0 |
|---------|------------------|--------------|
| Model struct size | 5 optional fields | +8 optional fields per struct. Serde skip_serializing_if keeps JSON compact |
| Parser complexity | 5 clause keywords | Same 5 keywords, but each entry has more optional suffixes. State machine approach scales linearly |
| DdlKind variants | 12 | +3-4 (GET_DDL, ShowColumns, AlterSetComment, AlterUnsetComment) |
| VTab count | 18 registered | +3-4 (get_ddl, show_columns, alter_comment variants) |
| Expansion paths | 3 modes (dims-only, metrics-only, both) | +3 modes (facts, semi-additive, window). Each is a separate code path in expand() |
| JSON storage size | ~500 bytes typical | +10-20% with metadata fields. Negligible for in-memory HashMap |

## Sources

- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- full DDL syntax including COMMENT, SYNONYMS, PRIVATE, NON ADDITIVE BY, window functions
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- TERSE mode, IN clause, output columns
- [Snowflake SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- output columns including synonyms and comment
- [Snowflake SEMANTIC_VIEW query construct](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- wildcard selection, queryable facts
- [Snowflake Querying semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- facts query examples, wildcard examples
- [Snowflake GET_DDL](https://docs.snowflake.com/en/sql-reference/functions/get_ddl) -- semantic view reconstruction
- [Snowflake Using SQL commands](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- ALTER SET COMMENT, GET_DDL examples, SHOW COLUMNS
- [Snowflake Semi-additive metrics release](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY feature details
- Direct codebase analysis of `src/` (16,342 LOC Rust)
