# Architecture: SQL DDL Syntax & PK/FK Relationship Model Integration

**Domain:** DuckDB semantic views extension -- SQL DDL parser and join inference
**Researched:** 2026-03-09
**Confidence:** HIGH (based on codebase analysis + Snowflake reference)

## Executive Summary

This document analyzes how proper SQL DDL syntax parsing and a PK/FK relationship model integrate with the existing semantic views extension architecture. The analysis covers four specific integration points: (1) where the SQL-to-function-call translation layer fits in the rewrite pipeline, (2) how the PK/FK model changes internal structs, (3) how join inference changes from ON-clause substring matching to PK/FK graph traversal, and (4) how qualified column names change CTE expansion.

The key architectural insight: **the existing pipeline already has all the right boundaries**. The parser hook already intercepts DDL, validation already scans clause keywords, the model already has `TableRef`/`Join`/`JoinColumn` structs, and the expansion engine already handles table aliases. The gap is a **translation layer** between the keyword-based DDL body and the function-call execution target -- currently the body is passed verbatim, which forces users to write DuckDB struct/list literal syntax inside what should be SQL DDL.

## Current Architecture (v0.5.1)

### End-to-End DDL Flow

```
User SQL                          C++ shim.cpp                    Rust parse.rs
---------                         ------------                    -------------
CREATE SEMANTIC VIEW x (          sv_parse_stub()                 sv_validate_ddl_rust()
  tables := [...],                  |                               |
  dimensions := [...]               | parse hook fires              | detect_ddl_kind()
)                                   | when DuckDB parser fails      | validate_clauses()
                                    v                               | scan_clause_keywords()
                                  sv_plan_function()                |
                                    | carries raw query              v
                                    v                             sv_rewrite_ddl_rust()
                                  sv_ddl_bind()                     |
                                    | calls Rust to rewrite         | rewrite_ddl()
                                    | DDL -> function call          | parse_create_body()
                                    v                               |
                                  duckdb_query(sv_ddl_conn, sql)    v
                                    | executes on isolated conn   "SELECT * FROM
                                    v                              create_semantic_view(
                                  Results forwarded as VARCHAR       'x', tables := [...],
                                  to DuckDB output                   dimensions := [...])"

                                                                  Rust ddl/define.rs
                                                                  ----------------
                                                                  parse_define_args_from_bind()
                                                                    | reads LIST(STRUCT) params
                                                                    | from DuckDB BindInfo
                                                                    v
                                                                  SemanticViewDefinition
                                                                    | stored as JSON
                                                                    v
                                                                  catalog_insert() / catalog_upsert()
```

### End-to-End Query Flow

```
User SQL                          Rust query/table_function.rs    Rust expand.rs
---------                         --------------------------     -------------
FROM semantic_view(               bind()                         expand()
  'x',                              | loads def from catalog       |
  dimensions := [...],              | infers types (LIMIT 0)       | resolve dims/mets
  metrics := [...]                  v                              | resolve_joins()
)                                 func()                           | build _base CTE
                                    | calls expand()               | build outer SELECT
                                    | executes expanded SQL        | GROUP BY ordinals
                                    | via execute_sql_raw()         |
                                    | streams via vector_ref        v
                                    v                             Concrete SQL:
                                  Output columns (typed)          WITH "_base" AS (
                                                                    SELECT *
                                                                    FROM "orders" AS "o"
                                                                    JOIN "customers" AS "c"
                                                                      ON "o"."customer_id" = "c"."id"
                                                                  )
                                                                  SELECT region, SUM(amount)
                                                                  FROM "_base"
                                                                  GROUP BY 1
```

### The Translation Gap

The `rewrite_ddl()` function in `parse.rs` performs this rewrite for CREATE-with-body forms:

```rust
let (name, body) = parse_create_body(trimmed, plen)?;
let safe_name = name.replace('\'', "''");
Ok(format!("SELECT * FROM {fn_name}('{safe_name}', {body})"))
```

**The body is passed verbatim.** This means the DDL body must already be valid DuckDB function-call syntax (`tables := [{alias: 'o', table: 'orders'}]`). The validation layer (`scan_clause_keywords`) can detect SQL-style clause keywords (`TABLES (...)`, `DIMENSIONS (...)`) but there is no code to convert from SQL keyword syntax to function-call syntax.

This is explicitly documented in TECH-DEBT.md item 8:

> The phase 21 validation layer (scan_clause_keywords) can parse a conventional SQL-style body (TABLES (...), DIMENSIONS (...), METRICS (...)) but there is no translation layer to convert it into executable function-call syntax.

## Recommended Architecture (v0.5.2)

### Component 1: SQL DDL Body Parser (NEW)

**Location:** New module `src/parse_sql_body.rs` (or extend `src/parse.rs`)

**Purpose:** Parse Snowflake-style SQL DDL body into a `SemanticViewDefinition` struct, bypassing the function-call rewrite entirely.

**Why a new component instead of extending the rewrite:** The current rewrite approach (`body -> function call SQL -> DuckDB parses SQL -> BindInfo -> parse_define_args_from_bind`) is a round-trip through DuckDB's SQL parser. We can go directly from text to `SemanticViewDefinition` without that round-trip.

Two architectural options were evaluated:

**OPTION A -- Direct parse (bypass DuckDB SQL parser):**
```
DDL text -> parse_sql_body() -> SemanticViewDefinition -> JSON -> catalog_insert()
```
This requires a new FFI path: instead of `sv_ddl_bind` executing rewritten SQL on `sv_ddl_conn`, it would call a Rust FFI function directly to parse the body and insert into the catalog. Changes required in `shim.cpp` (new FFI call path), new Rust FFI entry point, and the parser itself.

**OPTION B -- Translator (convert SQL syntax to function-call syntax):**
```
DDL text -> translate_sql_body() -> function-call syntax -> existing pipeline unchanged
```
The translator converts SQL keyword syntax into DuckDB struct/list literal syntax. The rest of the pipeline (`sv_ddl_bind` -> `duckdb_query` -> `create_semantic_view` VTab -> `parse_define_args_from_bind`) is unchanged.

**Recommendation: OPTION B (translator approach).** It has the smallest blast radius -- no changes to `shim.cpp`, no new FFI boundary, and the existing pipeline is already tested. The translator is a pure function (`&str -> Result<String, String>`) that can be unit-tested independently.

**Target SQL syntax** (Snowflake-aligned):

```sql
CREATE SEMANTIC VIEW order_analytics (
  TABLES (
    o AS orders PRIMARY KEY (order_id),
    c AS customers PRIMARY KEY (customer_id),
    li AS line_items PRIMARY KEY (line_item_id)
  ),
  RELATIONSHIPS (
    o (customer_id) REFERENCES c,
    li (order_id) REFERENCES o
  ),
  DIMENSIONS (
    o.region AS region,
    o.order_date AS o_orderdate,
    c.name AS customer_name
  ),
  METRICS (
    o.revenue AS SUM(li.amount),
    o.order_count AS COUNT(o.order_id)
  )
)
```

**Grammar sketch** (recursive descent, not a formal grammar):

```
body        ::= clause ( ',' clause )*
clause      ::= TABLES '(' table_list ')'
              | RELATIONSHIPS '(' rel_list ')'
              | DIMENSIONS '(' dim_list ')'
              | METRICS '(' metric_list ')'
table_list  ::= table_def ( ',' table_def )*
table_def   ::= IDENT 'AS' table_ref [ 'PRIMARY' 'KEY' '(' col_list ')' ]
rel_list    ::= rel_def ( ',' rel_def )*
rel_def     ::= IDENT '(' col_list ')' 'REFERENCES' IDENT
dim_list    ::= dim_def ( ',' dim_def )*
dim_def     ::= table_alias '.' IDENT 'AS' sql_expr
metric_list ::= metric_def ( ',' metric_def )*
metric_def  ::= table_alias '.' IDENT 'AS' sql_expr
sql_expr    ::= <balanced parentheses, no commas at depth 0>
col_list    ::= IDENT ( ',' IDENT )*
table_ref   ::= IDENT [ '.' IDENT [ '.' IDENT ] ]
```

**Translation output for the example above:**

```sql
SELECT * FROM create_semantic_view('order_analytics',
  tables := [
    {alias: 'o', table: 'orders'},
    {alias: 'c', table: 'customers'},
    {alias: 'li', table: 'line_items'}
  ],
  relationships := [
    {from_table: 'o', to_table: 'c', join_columns: [{from: 'customer_id', to: 'customer_id'}]},
    {from_table: 'li', to_table: 'o', join_columns: [{from: 'order_id', to: 'order_id'}]}
  ],
  dimensions := [
    {name: 'region', expr: 'o.region', source_table: 'o'},
    {name: 'o_orderdate', expr: 'o.order_date', source_table: 'o'},
    {name: 'customer_name', expr: 'c.name', source_table: 'c'}
  ],
  metrics := [
    {name: 'revenue', expr: 'SUM(li.amount)', source_table: 'o'},
    {name: 'order_count', expr: 'COUNT(o.order_id)', source_table: 'o'}
  ]
)
```

**Key parsing decisions:**

- **Dimension/metric scoping:** `o.region AS region` -- the `o.` prefix before the dot is the `source_table`, `region` after AS is the output `name`, and the text between AS and the next comma/end is the `expr`. For metrics the expr is expected to be an aggregate.
- **Relationship PK resolution:** `o (customer_id) REFERENCES c` -- the FK column is `customer_id` on table `o`, and the PK column on `c` must be looked up from `c`'s PRIMARY KEY declaration. If `c` has `PRIMARY KEY (customer_id)`, the join column pair is `{from: 'customer_id', to: 'customer_id'}`. If the FK name differs from the PK name, the relationship syntax would need `REFERENCES c (id)` to specify the target column.
- **Expression boundaries:** Expressions after `AS` extend until the next comma at depth 0 or the closing `)` of the clause. Parentheses, string literals, and nested function calls are tracked to avoid splitting mid-expression.

### Component 2: PK/FK Table Registry (MODIFIED)

**Location:** `src/model.rs` -- modify existing `TableRef` and `Join` structs

**Current state:**

```rust
pub struct TableRef {
    pub alias: String,
    pub table: String,
}

pub struct Join {
    pub table: String,
    pub on: String,              // legacy
    pub from_cols: Vec<String>,  // legacy
    pub join_columns: Vec<JoinColumn>,  // Phase 11.1
}

pub struct JoinColumn {
    pub from: String,
    pub to: String,
}
```

**Proposed changes:**

```rust
pub struct TableRef {
    pub alias: String,
    pub table: String,
    #[serde(default)]
    pub primary_key: Vec<String>,  // NEW: PK column names for this table
}

pub struct Join {
    pub table: String,           // target table (PK side, physical name)
    #[serde(default)]
    pub from_table: String,      // NEW: source table alias (FK side)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,              // legacy
    #[serde(default)]
    pub from_cols: Vec<String>,  // legacy
    #[serde(default)]
    pub join_columns: Vec<JoinColumn>,
}
```

**Why `from_table` is needed on `Join`:** In the current model, `Join` only stores the target table. The FK source table is implicitly the base table. But with the PK/FK graph, a FK can be on any table, not just the base. The relationship `li (order_id) REFERENCES o` creates a `Join` where `table` is `orders` (the PK side) and `from_table` is `li` (the FK side). Without `from_table`, the graph traversal cannot determine which table holds the FK.

**Evidence:** In `parse_args.rs` line 118, the `from_alias` is already read from the relationship struct but discarded:

```rust
let _from_alias = extract_struct_child_varchar(rel_child, 0); // from_table
```

Preserving this value is a one-line change.

**Backward compatibility:** `#[serde(default)]` on both new fields ensures old stored JSON deserializes correctly:
- Old `TableRef` without `primary_key` -> empty Vec
- Old `Join` without `from_table` -> empty String (legacy path assumed base table)

### Component 3: PK/FK Join Graph Traversal (MODIFIED)

**Location:** `src/expand.rs` -- modify `resolve_joins()`

**Current join resolution** (ON-clause substring matching):

```rust
// Fixed-point loop: if a needed join's ON clause references another
// declared join's table name, add that table to the needed set too.
loop {
    let on_lower = join.on.to_ascii_lowercase();
    // ...
    if on_lower.contains(&other_lower) {  // <-- substring match!
        needed.insert(other_lower);
    }
}
```

This is Tech Debt item 6. It works for simple cases but is fragile -- a table named `ord` would match inside `orders`.

**PK/FK model makes transitive dependencies explicit.** Given:

```
RELATIONSHIPS (
    o (customer_id) REFERENCES c,
    li (order_id) REFERENCES o
)
```

This creates a directed graph:

```
li --[order_id]--> o --[customer_id]--> c
```

When a metric has `source_table: "li"`, the resolver walks the graph:
1. `li` is needed -> find Join where `from_table == "li"` -> join target is `o` -> need `o`
2. `o` is needed -> find Join where `from_table == "o"` -> join target is `c` -> need `c`
3. `c` is needed -> no Join where `from_table == "c"` -> stop (or `c` is joined to base)

**Implementation approach:**

```rust
fn resolve_joins_pkfk<'a>(
    joins: &'a [Join],
    needed_aliases: &HashSet<String>,
    def: &SemanticViewDefinition,
) -> Vec<&'a Join> {
    let mut resolved: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = needed_aliases.iter().cloned().collect();

    // Base table alias is always "resolved" (no join needed)
    if let Some(base) = def.tables.first() {
        resolved.insert(base.alias.to_ascii_lowercase());
    }

    let mut result_joins: Vec<&Join> = Vec::new();

    while let Some(alias) = queue.pop() {
        if resolved.contains(&alias) {
            continue;
        }
        // Find the join that brings this alias into scope.
        // The join.table is the physical table; look up alias from def.tables.
        if let Some(join) = joins.iter().find(|j| {
            def.tables.iter().any(|t|
                t.table.eq_ignore_ascii_case(&j.table)
                && t.alias.eq_ignore_ascii_case(&alias)
            )
        }) {
            result_joins.push(join);
            resolved.insert(alias.clone());
            // The from_table of this join is a transitive dependency
            if !join.from_table.is_empty() {
                let from_lower = join.from_table.to_ascii_lowercase();
                if !resolved.contains(&from_lower) {
                    queue.push(from_lower);
                }
            }
        } else {
            resolved.insert(alias); // table not found in joins -- likely base table
        }
    }

    // Topological sort: joins ordered so dependencies appear first
    // (ensures SQL JOIN order is valid)
    topological_sort(&result_joins, def)
}
```

**Topological sort rationale:** While SQL does not strictly require joins in dependency order (all previously joined tables are available in ON clauses), producing deterministic, logically ordered SQL makes debugging easier and aligns with Snowflake's behavior.

**Dispatch logic:** The entry point `resolve_joins()` checks which model the definition uses:

```rust
fn resolve_joins<'a>(
    joins: &'a [Join],
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    def: &SemanticViewDefinition,
) -> Vec<&'a Join> {
    // Collect needed aliases
    let mut needed: HashSet<String> = /* same as current */;

    if has_pkfk_joins(joins) {
        resolve_joins_pkfk(joins, &needed, def)
    } else {
        resolve_joins_legacy(joins, &needed, def)  // current ON-clause substring code
    }
}

fn has_pkfk_joins(joins: &[Join]) -> bool {
    joins.iter().any(|j| !j.from_table.is_empty())
}
```

### Component 4: Direct Query Expansion (MODIFIED)

**Location:** `src/expand.rs` -- new expansion function alongside existing `expand()`

**The CTE problem with qualified names:**

The current expansion produces:

```sql
WITH "_base" AS (
    SELECT *
    FROM "orders" AS "o"
    JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
)
SELECT
    o.region AS "region"    -- ERROR: "o" is not a table in the outer query
FROM "_base"
GROUP BY 1
```

After `SELECT *`, table aliases are flattened into the CTE. The outer query only sees `_base`, not `o` or `c`. Qualified references like `o.region` fail.

**Three options were evaluated:**

| Option | Approach | Pros | Cons |
|--------|----------|------|------|
| A: No CTE | `SELECT ... FROM orders AS o JOIN ...` directly | Simplest. Aliases work naturally. | Different expansion path from legacy. |
| B: CTE with projection | CTE projects specific columns with unique names | Keeps CTE pattern. | Must enumerate all needed columns. Complex. |
| C: CTE with subquery aliases | Outer query re-aliases CTE | Unnatural SQL. | Hacky. |

**Recommendation: Option A (no CTE) for PK/FK definitions.** The CTE was originally needed because `SELECT *` flattened all tables. With the PK/FK model, expressions are qualified and we know exactly which tables are involved. A direct FROM+JOIN query is simpler and correct.

**Dual expansion dispatch:**

```rust
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // ... validation (unchanged) ...

    if uses_direct_expansion(def) {
        expand_direct(view_name, def, req, &resolved_dims, &resolved_mets)
    } else {
        expand_cte(view_name, def, req, &resolved_dims, &resolved_mets)
    }
}

fn uses_direct_expansion(def: &SemanticViewDefinition) -> bool {
    // Use direct expansion when we have table aliases with PKs
    !def.tables.is_empty()
        && def.joins.iter().any(|j| !j.from_table.is_empty())
}
```

**Direct expansion output:**

```sql
SELECT
    "o"."region" AS "region",
    SUM("li"."amount") AS "revenue"
FROM "orders" AS "o"
JOIN "customers" AS "c" ON "o"."customer_id" = "c"."customer_id"
JOIN "line_items" AS "li" ON "li"."order_id" = "o"."order_id"
WHERE (status = 'active')
GROUP BY 1
```

**Legacy CTE expansion** (existing code, unchanged) continues to work for definitions without PK/FK model.

## Integration Point Map

### Files to MODIFY

| File | What Changes | Why |
|------|-------------|-----|
| `src/model.rs` | Add `primary_key: Vec<String>` to `TableRef`, add `from_table: String` to `Join` | PK/FK model needs PK storage and FK source tracking |
| `src/parse.rs` | `rewrite_ddl()` detects SQL-style body and calls translator; `validate_clauses()` updated for new syntax | Translation layer between SQL DDL and function-call syntax |
| `src/expand.rs` | New `expand_direct()` function (no CTE, qualified names); `resolve_joins()` dispatches to PK/FK graph traversal for new-style definitions | New expansion path for PK/FK definitions |
| `src/ddl/parse_args.rs` | Preserve `from_alias` (currently discarded with `let _from_alias`) in `Join.from_table` | FK source table needed for graph traversal |
| `src/lib.rs` | Add `pub mod parse_sql_body;` if new module created | Module declaration |

### Files to CREATE

| File | Purpose |
|------|---------|
| `src/parse_sql_body.rs` | SQL DDL body translator: parses `TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)` keyword syntax and emits DuckDB struct/list literal syntax for the existing rewrite pipeline |

### Files UNCHANGED

| File | Why Unchanged |
|------|--------------|
| `cpp/src/shim.cpp` | Parser hook, plan function, DDL bind/execute all unchanged. The translator approach keeps the same rewrite-then-execute pipeline. |
| `src/catalog.rs` | Catalog storage is JSON strings -- serde handles new fields automatically |
| `src/query/table_function.rs` | Query execution pipeline unchanged -- it calls `expand()` which dispatches internally |
| `src/ddl/define.rs` | VTab bind/invoke unchanged -- receives translated function-call syntax |
| `src/ddl/describe.rs` | Reads from catalog JSON -- will reflect new fields via serde |
| `src/ddl/list.rs` | Lists view names only -- unaffected |
| `src/ddl/drop.rs` | Removes by name -- unaffected |

## Data Flow: Proposed vs Current

### Current DDL Flow (v0.5.1)

```
"CREATE SEMANTIC VIEW x (tables := [...], dimensions := [...])"
    |
    v
sv_parse_stub() -> PARSE_SUCCESSFUL
    |
    v
sv_ddl_bind() -> sv_rewrite_ddl_rust():
    rewrite_ddl() -> "SELECT * FROM create_semantic_view('x', tables := [...], ...)"
                     body passed VERBATIM (must be function-call syntax)
    |
    v
duckdb_query(sv_ddl_conn, rewritten_sql) -> create_semantic_view VTab
    parse_define_args_from_bind() -> SemanticViewDefinition -> catalog
```

### Proposed DDL Flow (v0.5.2) -- Translator Approach

```
"CREATE SEMANTIC VIEW x (
    TABLES (o AS orders PRIMARY KEY (order_id)),
    DIMENSIONS (o.region AS region),
    METRICS (o.revenue AS SUM(amount))
)"
    |
    v
sv_parse_stub() -> PARSE_SUCCESSFUL  [unchanged]
    |
    v
sv_ddl_bind() -> sv_rewrite_ddl_rust():
    rewrite_ddl():
        parse_create_body() -> body="TABLES (o AS orders ...) ..."
        body_uses_sql_syntax(body)? -> YES
        translate_sql_body(body) -> function-call syntax  [NEW]
            "tables := [{alias: 'o', table: 'orders'}], ..."
        -> "SELECT * FROM create_semantic_view('x', tables := [...], ...)"
    |
    v
duckdb_query(sv_ddl_conn, rewritten_sql) -> create_semantic_view VTab  [unchanged]
    parse_define_args_from_bind() -> SemanticViewDefinition -> catalog  [unchanged]
```

### Proposed Query Expansion Flow (v0.5.2)

```
FROM semantic_view('x', dimensions := ['region'], metrics := ['revenue'])
    |
    v
expand("x", def, req):
    uses_direct_expansion(def)?
      YES -> expand_direct():
        resolve_joins_pkfk() -- graph walk, not substring
        Build direct SQL (no CTE):
          SELECT "o"."region" AS "region", SUM(amount) AS "revenue"
          FROM "orders" AS "o"
          WHERE ...
          GROUP BY 1
      NO -> expand_cte():  [existing code, unchanged]
        resolve_joins_legacy() -- ON-clause substring
        Build CTE SQL (SELECT * FROM base JOIN ...)
```

## Patterns to Follow

### Pattern 1: Serde Default for Backward Compatibility

**What:** All new fields on serialized structs use `#[serde(default)]` so that old stored JSON deserializes without error.

**When:** Every time a field is added to model structs stored in the catalog.

**Example:**

```rust
pub struct TableRef {
    pub alias: String,
    pub table: String,
    #[serde(default)]
    pub primary_key: Vec<String>,  // old JSON without this field -> empty Vec
}
```

### Pattern 2: Dual Expansion Paths with Feature Detection

**What:** The `expand()` function detects whether the definition uses the PK/FK model or the legacy model and dispatches accordingly.

**When:** The expansion strategy differs fundamentally between definition formats.

**Example:**

```rust
fn uses_direct_expansion(def: &SemanticViewDefinition) -> bool {
    !def.tables.is_empty()
        && def.joins.iter().any(|j| !j.from_table.is_empty())
}
```

### Pattern 3: Body Syntax Detection for Translator Dispatch

**What:** `rewrite_ddl()` detects whether the DDL body uses SQL keyword syntax or function-call syntax, then either translates or passes through.

**When:** The body is about to be embedded in a function call.

**Example:**

```rust
fn body_uses_sql_syntax(body: &str) -> bool {
    let trimmed = body.trim();
    // SQL syntax uses TABLES keyword without := assignment
    // Function-call syntax uses tables := [...] with list literals
    let has_keyword_pattern = trimmed.to_ascii_uppercase().starts_with("TABLES");
    let has_assign_pattern = trimmed.contains(":=");
    has_keyword_pattern && !has_assign_pattern
}
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Parsing SQL Expressions

**What:** Attempting to fully parse SQL expressions inside dimension/metric definitions (e.g., `SUM(li.amount * li.quantity)`).

**Why bad:** SQL expression syntax is vast -- parentheses, function calls, CASE, subqueries, operators. Building a full SQL parser is out of scope.

**Instead:** Treat SQL expressions as opaque text between known delimiters. The translator only needs to find expression boundaries (comma at depth 0, clause-ending parenthesis), not understand expression internals. Use depth-tracking for parentheses and string literal awareness.

### Anti-Pattern 2: Removing Legacy Paths

**What:** Removing the CTE expansion path or the function-call DDL syntax.

**Why bad:** Existing stored definitions use the old format. Users may have scripts using function-call syntax. Breaking backward compatibility causes data loss.

**Instead:** Both paths coexist. Detection is automatic based on definition content. Old definitions continue to work unchanged.

### Anti-Pattern 3: Storing Derived Data in the Catalog

**What:** Pre-computing ON clauses from PK/FK declarations and storing them in JSON.

**Why bad:** Introduces normalization violation -- ON clauses are derived from `join_columns` and `tables` data. If one side changes, the other becomes stale.

**Instead:** Compute ON clauses at expansion time from PK/FK model data. `append_join_on_clause()` already does this.

### Anti-Pattern 4: Making shim.cpp Changes

**What:** Modifying the C++ shim to handle the new SQL syntax directly.

**Why bad:** C++ changes are expensive (full amalgamation recompile), harder to test, and the current Rust-side translation approach avoids any C++ changes entirely.

**Instead:** Keep all new parsing logic in Rust. The C++ shim remains a thin bridge between DuckDB's parser hooks and Rust FFI functions.

## Scalability Considerations

| Concern | Current (v0.5.1) | Proposed (v0.5.2) | Notes |
|---------|------------------|-------------------|-------|
| Join graph complexity | O(J^2) per expansion (fixed-point substring scan) | O(J) per expansion (directed graph walk) | J = number of joins. PK/FK graph is directed acyclic. |
| Column name collisions | Undefined behavior (CTE `SELECT *` may error) | Handled (qualified names, no `SELECT *`) | Major correctness improvement. |
| DDL body size | 4096-byte buffer in shim.cpp | Same 4096-byte buffer | Consider increasing if large definitions hit the limit. |
| Catalog migration | Automatic (serde defaults) | Automatic (serde defaults) | No migration step needed. |
| Translation overhead | None (body passed verbatim) | One-time string transform at DDL time | Negligible -- DDL is not a hot path. |

## Build Order (Dependency-Aware)

### Phase 1: Model Changes (no dependencies, prerequisite for all others)

1. Add `primary_key: Vec<String>` to `TableRef` with `#[serde(default)]`
2. Add `from_table: String` to `Join` with `#[serde(default)]`
3. Serde backward-compat tests (old JSON without new fields deserializes correctly)
4. Update `parse_define_args_from_bind()` to preserve `from_alias` in `Join.from_table`

**Test:** `cargo test` -- all existing tests pass (serde defaults handle old JSON).

### Phase 2: SQL Body Translator (depends on Phase 1 for model knowledge)

1. Create `src/parse_sql_body.rs` with `translate_sql_body()` function
2. Implement clause splitting (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS)
3. Implement per-clause parsing (table defs, relationship defs, dim/metric defs)
4. Implement function-call syntax emission
5. Integrate into `rewrite_ddl()` with `body_uses_sql_syntax()` detection
6. Update `validate_clauses()` to accept the new syntax patterns
7. Unit tests for each clause type and for full translation

**Test:** `cargo test` -- new translation tests. Existing rewrite tests still pass (function-call syntax detected and passed through).

### Phase 3: PK/FK Join Resolution (depends on Phase 1)

1. Implement `resolve_joins_pkfk()` with directed graph traversal
2. Implement topological sort for join ordering
3. Modify `resolve_joins()` to dispatch based on `has_pkfk_joins()`
4. Unit tests: single-hop, multi-hop, diamond pattern, mixed legacy+new

**Test:** `cargo test` -- new PK/FK resolution tests. Existing substring-based tests still pass for legacy definitions.

### Phase 4: Direct Query Expansion (depends on Phases 1 + 3)

1. Implement `expand_direct()` for FROM+JOIN without CTE
2. Handle qualified column names in SELECT expressions
3. Modify `expand()` to dispatch based on `uses_direct_expansion()`
4. Unit tests for direct expansion with various dim/metric combinations

**Test:** `cargo test` -- new expansion tests. Legacy CTE expansion tests still pass.

### Phase 5: Integration (depends on Phases 2 + 3 + 4)

1. End-to-end: SQL DDL syntax -> translate -> define -> query -> correct results
2. SQL logic tests (`test/sql/`) with Snowflake-style DDL syntax
3. Verify legacy definitions still work end-to-end
4. Verify mixed usage (SQL DDL define, function-call query)

**Test:** `just test-all` -- full test suite including sqllogictest and DuckLake CI.

## Sources

- Codebase analysis: `src/parse.rs`, `src/expand.rs`, `src/model.rs`, `src/ddl/parse_args.rs`, `src/ddl/define.rs`, `cpp/src/shim.cpp`, `src/lib.rs`, `src/catalog.rs`
- [Snowflake CREATE SEMANTIC VIEW DDL](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- reference syntax for SQL DDL
- [Snowflake semantic view SQL example](https://docs.snowflake.com/en/user-guide/views-semantic/example) -- TPC-H worked example showing TABLES with PRIMARY KEY, RELATIONSHIPS with REFERENCES, qualified dimension/metric names
- `TECH-DEBT.md` items 6 (ON-clause substring matching), 7 (unqualified column names), 8 (statement rewrite gap)
- `_notes/semantic-views-duckdb-design-doc.md` -- original design rationale and Snowflake/Cube.dev prior art
