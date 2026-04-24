# Architecture: v0.7.0 YAML Definitions & Materialization Routing

**Domain:** DuckDB semantic views extension -- YAML definition format and materialization routing engine
**Researched:** 2026-04-17
**Confidence:** HIGH (direct codebase analysis, Snowflake/Databricks docs verified, DuckDB capabilities confirmed)

## Executive Summary

v0.7.0 introduces two major features to the existing extension: (1) a YAML definition format as a second input path alongside SQL DDL, and (2) a materialization routing engine that transparently redirects queries to pre-existing aggregated tables. These features are architecturally independent -- YAML is an input format concern (parse layer), while materialization routing is an output/expansion concern (query layer). This independence means they can be built in parallel phases or sequentially without cross-dependencies.

The key architectural insight is that both features converge on the existing `SemanticViewDefinition` model as their common interface. YAML parsing produces the same `SemanticViewDefinition` struct that the SQL DDL body parser produces. Materialization routing consumes `SemanticViewDefinition` (extended with a new `materializations` field) at query expansion time. Neither feature requires changes to the catalog persistence layer -- JSON serialization via serde handles the new fields transparently.

## Recommended Architecture

### High-Level Data Flow

```
                         YAML INPUT PATH
                         ===============
SQL DDL Input            YAML Input (inline or file)
     |                        |
     v                        v
  parse.rs                 parse.rs (new DdlKind variants)
  detect_ddl_prefix()      detect FROM YAML / FROM YAML FILE
     |                        |
     v                        v
  body_parser.rs           yaml_parser.rs (NEW)
  parse_keyword_body()     parse_yaml_body()
     |                        |
     +---------+  +-----------+
               |  |
               v  v
     SemanticViewDefinition (model.rs)
               |
     +---------+---------+
     |                   |
     v                   v
  catalog.rs          render_ddl.rs / render_yaml.rs (NEW)
  (JSON persist)      (GET_DDL export)


                    MATERIALIZATION ROUTING
                    ======================
  semantic_view('view', dims := [...], metrics := [...])
               |
               v
     query/table_function.rs  (bind)
               |
               v
     materialize.rs (NEW) -- route_query()
     Check materializations for coverage
               |
        +------+------+
        |             |
  Full match     No match / partial
        |             |
        v             v
  SELECT from    expand/sql_gen.rs
  mat table      (existing expansion)
  (+ re-agg)
        |             |
        +------+------+
               |
               v
     Execute SQL on query_conn
```

### Component Boundaries

| Component | Responsibility | Status | Communicates With |
|-----------|---------------|--------|-------------------|
| `parse.rs` | DDL detection, validation, rewriting to function calls | MODIFY -- add DdlKind variants for `FROM YAML` | `body_parser.rs`, `yaml_parser.rs` (new) |
| `body_parser.rs` | SQL keyword body parsing (TABLES, DIMS, METRICS) | NO CHANGE | `parse.rs` |
| `yaml_parser.rs` (NEW) | YAML body parsing to `SemanticViewDefinition` | NEW | `parse.rs`, `model.rs` |
| `model.rs` | `SemanticViewDefinition` and sub-structs | MODIFY -- add `materializations: Vec<Materialization>` | everything |
| `catalog.rs` | In-memory cache + catalog table persistence | NO CHANGE (serde handles new fields) | `model.rs`, `ddl/define.rs` |
| `render_ddl.rs` | SQL DDL reconstruction from stored definitions | NO CHANGE | `model.rs`, `ddl/get_ddl.rs` |
| `render_yaml.rs` (NEW) | YAML export from stored definitions | NEW | `model.rs`, `ddl/get_ddl.rs` |
| `materialize.rs` (NEW) | Materialization routing engine | NEW | `model.rs`, `expand/sql_gen.rs` |
| `expand/sql_gen.rs` | Query expansion to concrete SQL | MODIFY -- call materialization router before expansion | `materialize.rs` (new), `model.rs` |
| `query/table_function.rs` | Table function bind/execute | MINOR MODIFY -- route through materialization check | `expand/sql_gen.rs`, `materialize.rs` (new) |
| `ddl/get_ddl.rs` | GET_DDL scalar function | MODIFY -- support YAML output format | `render_yaml.rs` (new) |
| `shim.cpp` | C++ parser hook registration | NO CHANGE | `parse.rs` via FFI |

## Feature 1: YAML Definitions

### 1.1 DDL Syntax Design

Two forms following the Snowflake pattern:

```sql
-- Inline YAML with dollar-quoting
CREATE SEMANTIC VIEW my_view FROM YAML $$
tables:
  - alias: o
    table: orders
    primary_key: [id]
dimensions:
  - name: region
    expr: o.region
metrics:
  - name: revenue
    expr: SUM(o.amount)
$$;

-- File-based YAML
CREATE SEMANTIC VIEW my_view FROM YAML FILE 'path/to/definition.yaml';

-- Also supports OR REPLACE and IF NOT EXISTS
CREATE OR REPLACE SEMANTIC VIEW my_view FROM YAML $$ ... $$;
CREATE SEMANTIC VIEW IF NOT EXISTS my_view FROM YAML FILE '...';
```

**Rationale for dollar-quoting:** DuckDB natively supports `$$`-delimited string constants (confirmed via DuckDB docs). Dollar-quoting avoids the need to escape single quotes inside YAML content, which would be painful since YAML uses colons, brackets, and can contain SQL expressions with single quotes. This mirrors Snowflake's `SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML` which also uses dollar-quoting for inline YAML.

### 1.2 Parser Hook Integration (parse.rs)

The `FROM YAML` and `FROM YAML FILE` syntax is NOT a new DdlKind variant -- it is a **body format specifier** within the existing CREATE forms. The detection flow is:

1. `detect_ddl_prefix()` detects `CREATE SEMANTIC VIEW` (existing logic, unchanged)
2. `validate_create_body()` extracts view name, optional COMMENT (existing logic)
3. **NEW branch:** After extracting the view name + optional COMMENT, check if the remaining text starts with `FROM YAML` instead of `AS`
4. If `FROM YAML $$`, extract the dollar-quoted content and route to `yaml_parser.rs`
5. If `FROM YAML FILE '...'`, extract the file path (will be loaded at bind time, not parse time)
6. If `AS`, route to existing `body_parser.rs` (unchanged)

**Why not a new DdlKind?** The DdlKind enum distinguishes statement-level forms (CREATE vs DROP vs ALTER). YAML vs SQL body is a sub-dispatch within CREATE -- the same DdlKind::Create/CreateOrReplace/CreateIfNotExists apply. Adding `CreateFromYaml` etc. would triple the CREATE variants for no benefit. Instead, the body format detection happens inside `validate_create_body()`.

**Modification to `validate_create_body()`:** After the existing `is_as_body` check, add:

```rust
let is_yaml_body = after_name_trimmed
    .get(..9)
    .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
    && (after_name_trimmed.len() == 9
        || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());

if is_yaml_body {
    return rewrite_ddl_yaml_body(kind, name, after_name_trimmed, body_offset, view_comment);
}
```

### 1.3 Dollar-Quoted String Extraction

DuckDB's parser already handles dollar-quoting at the SQL level. However, the parser extension hook fires BEFORE DuckDB's own parser succeeds (it is a fallback parser). This means `CREATE SEMANTIC VIEW` statements are intercepted as raw text. The `$$` extraction must happen in Rust.

**Algorithm for `extract_dollar_quoted()`:**

```
1. Find opening $$ (or $tag$) after "FROM YAML" + whitespace
2. Find matching closing $$ (or $tag$) -- same tag
3. Return content between delimiters
4. Error if no closing delimiter found
```

For v0.7.0, support only `$$` (untagged). Tagged dollar-quoting (`$yaml$ ... $yaml$`) is a nice-to-have but low priority.

### 1.4 YAML Parser (yaml_parser.rs -- NEW MODULE)

A new module `src/yaml_parser.rs` that:
1. Takes a YAML string
2. Parses it into a `SemanticViewDefinition`
3. Returns the same struct as `parse_keyword_body()` but from YAML input

**Implementation approach:** Use `serde_yml` crate (the maintained fork of deprecated `serde_yaml`). Since `SemanticViewDefinition` already derives `Serialize` and `Deserialize`, the YAML parsing is nearly free -- but the YAML schema should use user-friendly field names, not the internal JSON representation.

**YAML schema design (mapping to internal model):**

```yaml
tables:
  - alias: o
    table: orders
    primary_key: [id]
    unique: [[email], [first_name, last_name]]
    comment: "Main orders table"
    synonyms: [order_facts]

relationships:
  - name: order_to_customer
    from: o
    columns: [customer_id]
    references: c

facts:
  - name: unit_price
    table: o
    expr: price / qty
    comment: "Price per unit"

dimensions:
  - name: region
    table: o
    expr: o.region
    type: VARCHAR
    comment: "Geographic region"
    synonyms: [area, territory]

metrics:
  - name: revenue
    table: o
    expr: SUM(o.amount)
    type: DOUBLE
    using: [order_to_customer]
    non_additive_by:
      - dimension: date_dim
        order: DESC
        nulls: FIRST
    comment: "Total revenue"
    synonyms: [total_revenue]
    access: private

materializations:
  - table: orders_daily_agg
    dimensions: [date_dim, region]
    metrics: [revenue, order_count]
```

**Key design decision:** The YAML schema uses **human-readable field names** (`table` instead of `source_table`, `primary_key` instead of `pk_columns`, `from` instead of `from_alias`) and maps to the internal model structs via custom deserialization. This is different from just serializing/deserializing `SemanticViewDefinition` directly as YAML, which would expose internal field names that are confusing to users.

**Implementation strategy:** Define intermediate `YamlDef` structs with `serde(rename)` attributes that map to the user-facing YAML field names, then convert `YamlDef` -> `SemanticViewDefinition` with validation. This keeps the internal model stable while providing a clean YAML API.

### 1.5 FILE Loading Path

For `FROM YAML FILE 'path/to/definition.yaml'`:

**Option A (recommended): Use DuckDB's `read_text()` at bind time.**

The rewritten SQL would be:
```sql
SELECT * FROM create_semantic_view_from_yaml(
  'view_name',
  (SELECT content FROM read_text('path/to/definition.yaml'))
)
```

This leverages DuckDB's built-in file access layer which already handles:
- Local filesystem paths
- S3/GCS/Azure blob storage (via httpfs extension)
- Glob patterns (not useful here but free)

**Why not read files from Rust?** The extension runs in DuckDB's process. Rust's `std::fs::read_to_string()` would work for local files but would bypass DuckDB's FileSystem abstraction -- no cloud storage support, no access control integration. Using `read_text()` as a subquery in the rewritten SQL keeps file I/O in DuckDB's domain.

**Alternative considered and rejected:** Reading the file in the parse/validate phase. The parse hook runs in the parser extension context, which does not have access to the execution engine or file system. File reads must happen at bind time (when the rewritten SQL executes).

**Implementation:** In `rewrite_ddl_yaml_body()`, when `FILE` is detected:
1. Extract the file path from the single-quoted string
2. Generate rewritten SQL that wraps `read_text()` as a subquery
3. The YAML content arrives as a string parameter to the `create_semantic_view_from_yaml` table function
4. The table function calls `yaml_parser::parse_yaml()` to produce `SemanticViewDefinition`

### 1.6 YAML-Aware Table Functions

Two new table functions needed for YAML-based creation:

| Function | Purpose | Parameters |
|----------|---------|------------|
| `create_semantic_view_from_yaml` | Create from YAML string | `(name, yaml_text)` |
| `create_or_replace_semantic_view_from_yaml` | Create/replace from YAML | `(name, yaml_text)` |
| `create_semantic_view_if_not_exists_from_yaml` | Create if absent from YAML | `(name, yaml_text)` |

These mirror the existing `_from_json` table functions. They could share implementation by having a common `create_from_definition()` that takes a `SemanticViewDefinition`, with the `_from_yaml` variants calling `yaml_parser::parse_yaml()` first and the `_from_json` variants calling `SemanticViewDefinition::from_json()`.

**Alternatively (simpler):** The YAML path could convert YAML -> JSON at parse time, then call the existing `_from_json` functions. This avoids new table functions entirely:

```rust
// In rewrite_ddl_yaml_body():
let yaml_def = yaml_parser::parse_yaml(yaml_text)?;
let json = serde_json::to_string(&yaml_def)?;
// Rewrite to existing JSON path
Ok(format!("SELECT * FROM {fn_name}('{safe_name}', '{safe_json}')"))
```

**Recommendation:** Use the YAML-to-JSON-at-parse-time approach. This has zero new table function registrations, reuses the existing `_from_json` path entirely, and keeps the DDL pipeline simple. The YAML parsing happens in Rust (parse.rs), and by the time DuckDB executes the rewritten SQL, it is identical to the SQL DDL path.

### 1.7 GET_DDL YAML Export (render_yaml.rs -- NEW MODULE)

A new module `src/render_yaml.rs` that renders `SemanticViewDefinition` as YAML text. Parallel to `render_ddl.rs` which renders SQL DDL.

**Invocation:** Extend `GET_DDL` to accept a third optional parameter:
```sql
SELECT GET_DDL('SEMANTIC_VIEW', 'my_view');           -- SQL (default)
SELECT GET_DDL('SEMANTIC_VIEW', 'my_view', 'YAML');   -- YAML
```

**Modification to `ddl/get_ddl.rs`:** Check for the third parameter. If `'YAML'`, call `render_yaml::render_yaml()` instead of `render_ddl::render_create_ddl()`. Requires adding a third optional parameter to the VScalar signature.

**YAML rendering approach:** Use `serde_yml::to_string()` on the intermediate `YamlDef` structs (same ones used for parsing). This ensures round-trip fidelity: YAML in -> internal model -> YAML out produces equivalent YAML.

## Feature 2: Materialization Routing

### 2.1 Concept

A materialization declares that a pre-existing table contains pre-aggregated data for a known set of dimensions and metrics. At query time, if the requested dimensions and metrics are a subset of a materialization's coverage, the query can be routed to the materialization table instead of expanding from raw tables.

**Key constraint:** This is NOT pre-aggregation (the extension does not create or refresh materialized tables). It is a routing engine that assumes the user has created and maintains the aggregated tables externally. The extension simply redirects queries when possible.

### 2.2 DDL Syntax

A new optional `MATERIALIZATIONS` clause in the SQL DDL body:

```sql
CREATE SEMANTIC VIEW sales AS
TABLES (
    o AS orders PRIMARY KEY (id),
    c AS customers PRIMARY KEY (id)
)
RELATIONSHIPS (
    order_to_customer AS o(customer_id) REFERENCES c
)
DIMENSIONS (
    o.region AS o.region,
    o.date_dim AS DATE_TRUNC('day', o.order_date)
)
METRICS (
    o.revenue AS SUM(o.amount),
    o.order_count AS COUNT(*)
)
MATERIALIZATIONS (
    orders_daily_agg DIMENSIONS (date_dim, region) METRICS (revenue, order_count),
    orders_region_agg DIMENSIONS (region) METRICS (revenue)
)
```

In YAML:
```yaml
materializations:
  - table: orders_daily_agg
    dimensions: [date_dim, region]
    metrics: [revenue, order_count]
  - table: orders_region_agg
    dimensions: [region]
    metrics: [revenue]
```

### 2.3 Model Extension (model.rs)

Add a new struct and field:

```rust
/// A pre-existing aggregated table that covers specific dimensions and metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Materialization {
    /// The physical table name (may be schema-qualified).
    pub table: String,
    /// Dimension names covered by this materialization.
    pub dimensions: Vec<String>,
    /// Metric names covered by this materialization.
    pub metrics: Vec<String>,
}
```

Add to `SemanticViewDefinition`:

```rust
/// Pre-existing materialization tables with known dim/metric coverage.
/// Old stored JSON without this field deserializes with empty Vec.
/// Not serialized when empty to preserve backward-compatible JSON.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub materializations: Vec<Materialization>,
```

**Backward compatibility:** `#[serde(default)]` ensures old stored JSON (without `materializations`) loads with `vec![]`. `skip_serializing_if = "Vec::is_empty"` ensures existing views without materializations produce identical JSON. This is the same pattern used for every field added since v0.5.2.

### 2.4 Body Parser Extension (body_parser.rs)

Add `MATERIALIZATIONS` as a new optional clause keyword, ordered after METRICS:

```rust
const CLAUSE_KEYWORDS: &[&str] = &[
    "tables", "relationships", "facts", "dimensions", "metrics", "materializations"
];
const CLAUSE_ORDER: &[&str] = &[
    "tables", "relationships", "facts", "dimensions", "metrics", "materializations"
];
```

The MATERIALIZATIONS clause parser needs to handle:
```
mat_table_name DIMENSIONS (dim1, dim2) METRICS (metric1, metric2)
```

This is a simpler syntax than the existing clauses (no expressions, just name lists).

### 2.5 Materialization Router (materialize.rs -- NEW MODULE)

The core routing logic:

```rust
pub enum RouteResult {
    /// Query can be fully served from a materialization table.
    /// The SQL selects from the mat table with re-aggregation if needed.
    Materialized(String),
    /// No suitable materialization found; fall back to normal expansion.
    Fallback,
}

pub fn route_query(
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> RouteResult {
    // 1. Skip if no materializations defined
    // 2. For each materialization, check if requested dims are a subset
    //    of mat dims AND requested metrics are a subset of mat metrics
    // 3. Pick the "best" match (smallest superset = fewest extra dims/metrics)
    // 4. If exact match: SELECT dims, metrics FROM mat_table GROUP BY dims
    //    (GROUP BY needed because mat may have more granularity than requested)
    // 5. If no match: return Fallback
}
```

**Routing algorithm:**

```
For each materialization M:
  if requested_dims is subset of M.dimensions
     AND requested_metrics is subset of M.metrics:
    score = |M.dimensions| + |M.metrics|  // prefer smaller materializations
    candidates.push((M, score))

Sort candidates by score ascending (prefer tightest match)
Return first candidate or Fallback
```

**Re-aggregation for subset matches:** When the requested dimensions are a proper subset of the materialization's dimensions, the query needs re-aggregation:

```sql
-- Mat table has: date_dim, region, revenue, order_count
-- Request: dimensions=[region], metrics=[revenue]
-- Generated SQL:
SELECT "region", SUM("revenue") AS "revenue"
FROM orders_daily_agg
GROUP BY "region"
```

The re-aggregation assumes metrics are SUM-compatible (additive). For non-additive metrics (like `MAX`, `AVG`), re-aggregation produces incorrect results. The routing engine should skip materializations for metrics that use non-SUM aggregations unless the dimensions match exactly.

**Handling non-additive metrics:** Two approaches:

1. **Conservative (recommended for v0.7.0):** Only route to materializations when requested dimensions exactly match the materialization's dimensions. No re-aggregation. This is correct for all aggregation types.

2. **Smart re-aggregation (future):** Analyze the metric expression to determine if re-aggregation is safe (SUM, COUNT are additive; MAX, MIN are semi-additive; AVG is not re-aggregatable). This requires expression analysis that is complex and error-prone.

**Recommendation:** Start with exact-dimension matching only. Re-aggregation for subset matches is a v0.8.0 feature. This simplifies the routing logic and avoids correctness pitfalls.

### 2.6 Integration with Expansion Pipeline

The materialization check happens BEFORE `expand()` in the query path:

```rust
// In query/table_function.rs bind(), after resolving the definition:
let route = materialize::route_query(&def, &req);
let expanded_sql = match route {
    RouteResult::Materialized(sql) => sql,
    RouteResult::Fallback => expand(view_name, &def, &req)?,
};
```

This is a clean interception point because:
1. The `QueryRequest` (dims + metrics) is already parsed
2. The `SemanticViewDefinition` (with materializations) is already loaded
3. The result (a SQL string) feeds into the same execution path

**No changes to `expand/sql_gen.rs` internals.** The materialization router is a pre-check that short-circuits the expansion. If no materialization matches, the existing expansion runs unchanged.

### 2.7 Materialization Validation at Define Time

When a view with materializations is created, validate:

1. **Dimension names exist:** Each dimension name in a materialization must match a declared dimension in the view.
2. **Metric names exist:** Each metric name in a materialization must match a declared metric in the view.
3. **Table is accessible:** Optionally verify the materialization table exists (via catalog query). Could be deferred to query time.

This validation happens in `parse.rs::rewrite_ddl_keyword_body()` after constructing the `SemanticViewDefinition`, or in `ddl/define.rs` during the create flow. The latter is preferred because it can use the catalog_conn for table existence checks.

### 2.8 SHOW/DESCRIBE Integration

- `DESCRIBE SEMANTIC VIEW` should show materialization entries (new object kind)
- `SHOW COLUMNS IN SEMANTIC VIEW` may optionally include materialization info
- A new `SHOW SEMANTIC MATERIALIZATIONS IN view_name` command could list materialization tables and their coverage

For v0.7.0, at minimum add materialization info to `DESCRIBE`.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Dual Model Structs
**What:** Creating separate model types for YAML vs SQL DDL definitions.
**Why bad:** Doubles the maintenance surface, risks drift between the two formats, complicates catalog persistence.
**Instead:** Single `SemanticViewDefinition` model. Both parsers produce the same struct. Use intermediate structs only for serde mapping (YamlDef -> SemanticViewDefinition conversion).

### Anti-Pattern 2: YAML Parsing at Bind Time for Inline YAML
**What:** Passing raw YAML through the rewritten SQL to be parsed at table function bind time.
**Why bad:** Escaping YAML content inside SQL strings is fragile. YAML can contain single quotes, backticks, dollar signs. The existing JSON path already has this problem (mitigated by SQL-escaping the JSON), but YAML is worse because it is multi-line and whitespace-sensitive.
**Instead:** Parse YAML at parse-hook time (in `validate_create_body`), convert to JSON, and pass JSON through the existing `_from_json` path. The YAML content never touches SQL string escaping.

### Anti-Pattern 3: Re-Aggregation Without Expression Analysis
**What:** Always re-aggregating metrics when routing to materializations with extra dimensions.
**Why bad:** `AVG(amount)` in a daily materialization cannot be correctly re-aggregated to monthly by doing `AVG(AVG(amount))`. Similarly, `COUNT(DISTINCT ...)` cannot be re-aggregated.
**Instead:** Start with exact-dimension-match routing only. Add re-aggregation later with explicit expression analysis.

### Anti-Pattern 4: File I/O in the Parser Hook
**What:** Reading YAML files during the parse phase (sv_parse_stub / sv_validate_ddl_rust).
**Why bad:** The parser hook runs in the DuckDB parser context which does not have access to the execution engine, connection state, or file system. File reads would need to go through Rust's `std::fs` directly, bypassing DuckDB's FileSystem abstraction.
**Instead:** For `FROM YAML FILE`, extract the path at parse time but defer file reading to bind time via `read_text()` in the rewritten SQL.

## Patterns to Follow

### Pattern 1: Serde Default + Skip Serialization (model.rs)
**What:** All new optional fields use `#[serde(default, skip_serializing_if)]`
**When:** Every time a field is added to a persisted struct
**Why:** Ensures backward-compatible deserialization of old stored JSON and forward-compatible serialization (no unnecessary keys in output)

### Pattern 2: Parse-Time Validation, Bind-Time Execution
**What:** Validate DDL syntax in the parser hook (fast, synchronous, positioned error reporting). Execute side effects at bind time (catalog writes, file I/O).
**When:** Any new DDL form
**Why:** Matches the existing architecture. The parser hook returns PARSE_SUCCESSFUL or an error with position. The plan function routes to a table function whose bind() does the actual work.

### Pattern 3: Rewrite to Existing Table Functions
**What:** New DDL forms should rewrite to existing table function calls rather than registering new table functions where possible.
**When:** The new form produces the same internal representation as an existing form.
**Why:** Reduces the extension's registration surface area and avoids code duplication. The YAML path rewriting to `_from_json` after YAML->JSON conversion is a prime example.

## New Components Summary

| Component | Type | LOC Estimate | Dependencies |
|-----------|------|-------------|--------------|
| `yaml_parser.rs` | New module | ~200-300 | `serde_yml`, `model.rs` |
| `render_yaml.rs` | New module | ~150-200 | `serde_yml`, `model.rs` |
| `materialize.rs` | New module | ~200-300 | `model.rs`, `expand/types.rs` |
| `Materialization` struct | New in `model.rs` | ~30 | serde |

## Modified Components Summary

| Component | Change | Risk |
|-----------|--------|------|
| `parse.rs` :: `validate_create_body()` | Add `FROM YAML` / `FROM YAML FILE` branch after `AS` check | LOW -- additive branch, no existing logic changes |
| `parse.rs` :: new `rewrite_ddl_yaml_body()` | New function parallel to `rewrite_ddl_keyword_body()` | LOW -- new function, no existing function modified |
| `parse.rs` :: new `extract_dollar_quoted()` | Dollar-quote extraction utility | LOW -- self-contained |
| `model.rs` :: `SemanticViewDefinition` | Add `materializations: Vec<Materialization>` | LOW -- serde default handles backward compat |
| `body_parser.rs` :: `CLAUSE_KEYWORDS` | Add `"materializations"` | LOW -- additive |
| `body_parser.rs` :: clause parsing | Add materialization clause parser | LOW -- new branch in existing dispatch |
| `query/table_function.rs` :: bind | Insert materialization routing check before `expand()` | MEDIUM -- touches hot path, needs careful testing |
| `ddl/get_ddl.rs` :: `GetDdlScalar` | Add optional third parameter for format | LOW -- parameter addition |
| `lib.rs` | Add `pub mod yaml_parser; pub mod render_yaml; pub mod materialize;` | TRIVIAL |
| `Cargo.toml` | Add `serde_yml` dependency | LOW |

## Suggested Build Order

The build order respects dependencies and provides testable increments:

### Phase 1: YAML Parser Core
**Build:** `yaml_parser.rs`, YAML schema structs, `YamlDef` -> `SemanticViewDefinition` conversion
**Test:** Unit tests with YAML strings -> verify correct `SemanticViewDefinition` output
**Dependencies:** `model.rs` (existing), `serde_yml` (new dep)
**Rationale:** Foundation for all YAML features. Self-contained, testable without extension loading.

### Phase 2: Dollar-Quoting and Parser Integration
**Build:** `extract_dollar_quoted()` in `parse.rs`, `FROM YAML` detection in `validate_create_body()`, `rewrite_ddl_yaml_body()` function
**Test:** Unit tests for dollar-quote extraction, `validate_and_rewrite()` tests for YAML DDL forms
**Dependencies:** Phase 1 (yaml_parser)
**Rationale:** Connects YAML parsing to the DDL pipeline. Still testable via `cargo test` without extension loading.

### Phase 3: YAML FILE Loading
**Build:** `FROM YAML FILE` path extraction, rewrite to `read_text()` subquery
**Test:** SQLLogicTest with local YAML files
**Dependencies:** Phase 2
**Rationale:** Requires `just build` + sqllogictest because file I/O goes through DuckDB's `read_text()`.

### Phase 4: YAML Export (GET_DDL)
**Build:** `render_yaml.rs`, modification to `ddl/get_ddl.rs` for YAML format parameter
**Test:** Unit tests for render, sqllogictest for round-trip (create from YAML -> GET_DDL YAML -> verify)
**Dependencies:** Phase 1 (yaml_parser for round-trip verification)
**Rationale:** Completes the YAML feature set. Can be built in parallel with Phase 3.

### Phase 5: Materialization Model
**Build:** `Materialization` struct in `model.rs`, `MATERIALIZATIONS` clause in `body_parser.rs`, YAML schema extension, define-time validation
**Test:** Unit tests for parsing, define-time name validation
**Dependencies:** None (model and parser are independent of YAML)
**Rationale:** Foundation for routing. Self-contained model + parser work.

### Phase 6: Materialization Router
**Build:** `materialize.rs` with `route_query()`, integration into `query/table_function.rs` bind path
**Test:** Unit tests for routing algorithm, sqllogictest for end-to-end routing
**Dependencies:** Phase 5 (model), existing expand/ module
**Rationale:** The core feature. Requires materialization tables to exist for integration tests.

### Phase 7: DESCRIBE/SHOW Integration
**Build:** Materialization entries in DESCRIBE output, optional SHOW SEMANTIC MATERIALIZATIONS
**Test:** SQLLogicTest
**Dependencies:** Phase 5 (model)
**Rationale:** Introspection for materializations. Lower priority than routing.

**Note on parallelism:** Phases 1-4 (YAML) and Phases 5-7 (materialization) are independent tracks. They can be built in any interleaving, or one track completed before the other. The only shared touch point is `model.rs`, which receives additive changes from both tracks.

## Scalability Considerations

| Concern | Impact | Approach |
|---------|--------|----------|
| YAML parsing performance | Negligible -- YAML files are small (< 100KB typically) | No optimization needed |
| Materialization routing with many materializations | Linear scan over materializations per query | Fine for < 100 materializations per view. If needed, precompute a dimension-set index |
| Dollar-quote extraction | O(n) scan for closing delimiter | Fine -- DDL strings are small |
| File reading for FROM YAML FILE | DuckDB's `read_text()` handles efficiently | No custom buffering needed |

## Sources

- DuckDB dollar-quoted string support: [Literal Types -- DuckDB](https://duckdb.org/docs/current/sql/data_types/literal_types)
- DuckDB `read_text()` function: [Directly Reading Files -- DuckDB](https://duckdb.org/docs/current/guides/file_formats/read_file)
- Snowflake YAML spec: [YAML specification for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec)
- Snowflake SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML: [Snowflake Documentation](https://docs.snowflake.com/en/sql-reference/stored-procedures/system_create_semantic_view_from_yaml)
- Databricks materialization: [Materialization for metric views](https://docs.databricks.com/aws/en/metric-views/materialization)
- serde_yml (maintained fork): [GitHub - sebastienrousseau/serde_yml](https://github.com/sebastienrousseau/serde_yml)
- Codebase analysis: `src/parse.rs`, `src/body_parser.rs`, `src/model.rs`, `src/catalog.rs`, `src/render_ddl.rs`, `src/expand/sql_gen.rs`, `src/query/table_function.rs`, `cpp/src/shim.cpp`
