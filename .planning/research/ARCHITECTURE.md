# Architecture Research: DuckDB Semantic Views Extension

**Research date:** 2026-02-23
**Confidence:** Medium-High. DuckDB's extension API is well-documented at the C++ level; Rust bindings via `duckdb-rs` are less documented and require careful bridging. Findings on custom DDL and parser hooks are based on deep study of DuckDB internals and existing extension examples.

---

## Summary

DuckDB extensions integrate through a stable C ABI and a rich C++ SDK exposing hooks into the parser, catalog, planner, and function registry. Semantic view expansion fits cleanly as a **parser/statement-level hook**: the extension intercepts `CREATE SEMANTIC VIEW` DDL statements and `FROM my_view(...)` table function calls before the planner runs, rewrites them to concrete SQL, and lets DuckDB handle all subsequent execution. The `duckdb-rs` crate wraps the C API sufficiently for table functions and scalar functions but has limited (and partially unsafe) support for custom DDL and catalog integration; bridging via raw FFI to the C++ extension SDK is the realistic path for custom DDL.

---

## Component Architecture

### DuckDB Extension Integration Points

DuckDB extensions interact with the engine through several well-defined hooks. All are exposed via `DatabaseInstance` and `Connection`-level APIs in the C++ SDK, which is what extensions compiled against DuckDB's header-only `duckdb.hpp` or `duckdb_extension.h` interface use.

#### 1. Extension Lifecycle

Every DuckDB extension exports two C symbols:

```c
void <name>_init(duckdb::DatabaseInstance &db);
std::string <name>_version();
```

`_init` receives the live `DatabaseInstance` and is where the extension registers all of its capabilities. For Rust extensions compiled as `cdylib`, these are `#[no_mangle] pub extern "C"` functions that call into the `duckdb-rs` registration helpers or raw DuckDB C FFI.

#### 2. Function Registration

The most stable and well-supported extension API. DuckDB provides:

- **Table functions** (`duckdb::TableFunction`): Return a relation from a function call. This is the mechanism for `FROM my_semantic_view(DIMENSIONS ..., METRICS ...)`.
- **Scalar functions** (`duckdb::ScalarFunction`): Return a single value per row.
- **Aggregate functions** (`duckdb::AggregateFunction`): Custom aggregates.
- **Table macro functions** (`CreateMacroFunction`): SQL-level macros that expand to a subquery. Less flexible than table functions but simpler.

Table functions are the **primary mechanism for the query interface** of this extension. A table function receives named parameters and has a `bind` phase (determines output schema from parameters) and an `execute` phase (produces tuples). For semantic view expansion, the bind phase is where expansion logic runs — the table function inspects the semantic view definition, resolves dimensions/metrics, and either:
  - Returns a schema that maps to a virtual relation (pull model), or
  - Emits an internal SQL string that DuckDB re-executes (push model via `duckdb::TableFunction` with `replacement_scan`).

#### 3. Parser Hooks / Statement-Level Interception

DuckDB provides two hook points for intercepting SQL before planning:

**a. `AddParserExtension` (custom statement types)**

Allows an extension to register a custom parser that handles unrecognised SQL statements. When DuckDB's built-in parser fails to parse a statement, it tries each registered parser extension in order. If the custom parser succeeds, it returns an `ExtensionParseData` object (a `unique_ptr<ParserExtensionParseData>`) that DuckDB carries through the pipeline. The extension must also register a `PlannerExtension` callback to convert this custom parse data into a `LogicalOperator` subtree.

This mechanism supports `CREATE SEMANTIC VIEW` DDL: DuckDB's Postgres-dialect parser will not recognise the statement, so the extension's parser hook fires, parses the definition, stores it, and returns a custom parse node. The planner extension callback then stores the definition in the catalog.

**b. `AddStatementMatcher` / query rewriting** (less stable path)

Some extensions use DuckDB's `ClientContext` hooks to intercept queries post-parse. The `config.replacement_scans` hook (a list of `ReplacementScanCallback`) fires when the binder encounters an unresolved table reference. This is the mechanism used by `httpfs` (to intercept `FROM 'file.parquet'`) and the JSON extension. For semantic views, a replacement scan hook can fire whenever the binder encounters `FROM my_semantic_view(...)` and the name doesn't resolve to a regular table — the callback can then return a `TableFunctionRef` pointing at the extension's table function.

**c. `AddOptimizerExtension`**

Registers a callback that fires after the logical plan is built but before physical planning. The callback receives the `LogicalOperator` tree and can modify it. This is a planner-level rewrite hook. Useful if semantic view expansion needs to happen after binding rather than during parsing (e.g., to leverage DuckDB's existing name resolution).

#### 4. Catalog APIs

DuckDB's internal catalog is organized as: `Catalog → Schema → CatalogEntry`. Entry types include `TableCatalogEntry`, `ViewCatalogEntry`, `FunctionCatalogEntry`, and the extensible `CatalogEntry` base.

Extensions can add custom catalog entry types by subclassing `CatalogEntry` and registering them via `Catalog::CreateEntry`. For semantic view definitions, the options are:

**Option A: Custom CatalogEntry subclass** — Define `SemanticViewCatalogEntry` extending `CatalogEntry`. Register via `catalog.CreateEntry(context, CatalogType::TABLE_ENTRY, ...)` with a custom `CreateInfo`. This integrates with DuckDB's transaction model and `SHOW TABLES`-style queries. However, DuckDB's `Catalog::CreateEntry` API accepts only known `CatalogType` values by default; adding a truly custom type requires modifying enum values (not extensible from outside), or aliasing to an existing type.

**Option B: Store as DuckDB views or tables** — The simplest approach: serialize the semantic view definition (as JSON or SQL text) and store it in a regular DuckDB table within a dedicated schema (e.g., `semantic_layer._definitions`). On extension load, read this table to reconstruct the in-memory representation. This avoids all catalog API complexity and works with DuckDB's existing `ATTACH`/persistence model.

**Option C: Macro-based storage** — DuckDB's `CREATE MACRO` system stores macro definitions in the catalog and persists them. Some extensions encode metadata as macros. Not clean for structured definitions.

**Recommended for v0.1:** Option B (plain DuckDB table in a `semantic_layer` schema). It's the most portable, avoids deep catalog integration, and survives database restart because DuckDB persists user-defined tables. The extension, on load, creates the schema and table if they don't exist, and registers all defined semantic views from that table into its in-memory registry.

#### 5. Replacement Scan API

`config.replacement_scans` is a vector of `ReplacementScan` callbacks in `DBConfig`. Each callback receives a `ClientContext`, a table name, and (optionally) schema/catalog names. If the callback recognizes the name as a semantic view, it returns a `ReplacementScanData` that substitutes a `TableFunctionRef` in place of the unresolved table reference.

This is how the query interface works without requiring users to write `FROM SEMANTIC_VIEW(...)` — they can write `FROM orders_by_region(DIMENSIONS region, METRICS revenue)` and the extension's replacement scan resolves `orders_by_region` to the semantic view table function.

---

### Catalog & Persistence

#### How DuckDB Persists Extension State

DuckDB uses a WAL (write-ahead log) and block-based storage in `.duckdb` files. All user-created catalog objects (tables, views, macros, sequences) are stored in this file and reloaded on open.

Extensions that need persistent state have two patterns:

**Pattern 1: Piggyback on DuckDB's catalog (via internal tables)**
Create a schema at extension load (`CREATE SCHEMA IF NOT EXISTS semantic_layer`). Store definitions in a regular table:

```sql
CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
    name      VARCHAR PRIMARY KEY,
    schema    VARCHAR,
    definition JSON,
    created_at TIMESTAMP DEFAULT now()
);
```

The extension reads this table in `_init` to rebuild its in-memory `HashMap<String, SemanticViewDef>`. When `CREATE SEMANTIC VIEW` runs, the extension inserts a row here and registers the view in its in-memory map. This survives database restart automatically because DuckDB persists the table.

**Pattern 2: External file**
Store definitions as a JSON/TOML file alongside the database. Less robust; not recommended because it breaks with `ATTACH` and relative path assumptions.

**Pattern 3: DuckDB Macro system**
DuckDB persists macros. An extension can encode semantic view metadata as macro body text. This is a hack and limits definition size to SQL string length constraints.

**Recommendation:** Pattern 1 is correct for this extension. The `semantic_layer` schema acts as the extension's private catalog namespace. JSON column stores the full definition (dimensions, measures, join paths, filter expressions). A secondary in-memory registry (populated from this table at load time) provides fast lookup during query expansion.

#### Schema Design for Semantic View Definitions

```sql
-- Main definitions table
CREATE TABLE semantic_layer._definitions (
    name       VARCHAR PRIMARY KEY,
    catalog    VARCHAR DEFAULT current_catalog(),
    schema     VARCHAR DEFAULT current_schema(),
    definition JSONB,         -- full definition including dims, measures, joins
    created_at TIMESTAMPTZ DEFAULT now(),
    updated_at TIMESTAMPTZ DEFAULT now()
);

-- Optional: separate table for relationship graph
CREATE TABLE semantic_layer._relationships (
    from_view  VARCHAR,
    to_view    VARCHAR,
    join_key   VARCHAR,
    join_expr  VARCHAR,       -- SQL expression for the join condition
    cardinality VARCHAR       -- 'many_to_one', 'one_to_many', etc.
);
```

#### JSON Schema for a Semantic View Definition

```json
{
  "name": "sales_analysis",
  "base_table": "orders",
  "dimensions": [
    {
      "name": "region",
      "expr": "customers.region",
      "requires_join": "customers"
    },
    {
      "name": "order_date",
      "expr": "orders.order_date",
      "is_time": true
    }
  ],
  "measures": [
    {
      "name": "total_revenue",
      "expr": "SUM(orders.amount)",
      "is_additive": true
    },
    {
      "name": "unique_customers",
      "expr": "COUNT(DISTINCT orders.customer_id)",
      "is_additive": false
    }
  ],
  "joins": [
    {
      "alias": "customers",
      "table": "customers",
      "condition": "orders.customer_id = customers.id",
      "type": "LEFT"
    }
  ],
  "row_filter": null
}
```

---

### Query Expansion Pipeline

#### Where Expansion Fits in DuckDB's Pipeline

DuckDB's query execution pipeline:

```
SQL Text
  → Parser (produces ParsedStatement tree)
  → Binder (resolves names, produces BoundStatement)
  → LogicalPlanner (produces LogicalOperator tree)
  → Optimizer (rewrite rules → optimized LogicalOperator)
  → Physical Planner (produces PhysicalOperator tree)
  → Executor (produces result tuples)
```

Semantic view expansion must happen **before** the Binder, because the expanded SQL references physical tables that the Binder needs to resolve. Two viable injection points:

**Injection Point A: Table Function (bind phase) — Recommended for v0.1**

Register a table function `semantic_view_scan` (or name it after each semantic view). During the bind phase of this table function, perform the expansion:
1. Look up the semantic view definition by name.
2. Parse the `DIMENSIONS` and `METRICS` parameters.
3. Construct the concrete SQL SELECT/FROM/JOIN/GROUP BY.
4. Execute this SQL internally using `context.Query(expanded_sql)` or by returning the result as a `TableFunctionInput`.

The challenge: DuckDB does not natively support a table function that "re-executes" a SQL string as its output. The cleanest approach is for the table function to act as a **view expander** — it produces the concrete SQL and DuckDB then evaluates it. This can be achieved via:
- `QueryResult`-based approach: in the bind phase, execute the expanded SQL and return its schema; in the scan phase, return rows from the cached result.
- Or use `replacement_scan` to return an `Expression` (subquery) that DuckDB evaluates as a derived table.

**Injection Point B: Parser Extension Hook — For custom DDL**

`CREATE SEMANTIC VIEW` cannot be a table function. It is a DDL statement. The parser extension hook intercepts it before DuckDB's Postgres parser rejects it. The flow:

```
"CREATE SEMANTIC VIEW orders_kpis (...)"
  → DuckDB parser fails to parse
  → Parser extension callback fires
  → Extension parses definition, validates, stores in _definitions table
  → Returns success / error to user
```

**Injection Point C: Replacement Scan — For FROM my_view(...) syntax**

When the user writes `FROM orders_kpis(DIMENSIONS region, METRICS revenue)`, the binder looks up `orders_kpis` in the catalog. If it doesn't find a regular table, the replacement scan callbacks fire. The extension's callback:
1. Checks if `orders_kpis` matches a registered semantic view.
2. Builds the expanded SQL as a subquery expression.
3. Returns a `TableFunctionRef` or a subquery `SelectStatement` that replaces the unresolved reference.

This allows the syntax `FROM my_semantic_view(DIMENSIONS ..., METRICS ...)` where parameters are passed as function arguments.

#### Concrete Expansion Algorithm

```
Input:  semantic view name, requested dimensions [d1, d2, ...],
        requested measures [m1, m2, ...], optional WHERE filter

1. Load SemanticViewDef from in-memory registry (keyed by name)
2. Resolve requested dimensions:
   - For each dim name, find the dimension definition
   - Collect required joins (which tables must be joined)
3. Resolve requested measures:
   - For each measure name, find the measure definition
   - Collect additional required joins
4. Compute minimal join set (union of joins required by dims + measures)
5. Build SQL:
   SELECT {dim.expr AS dim.name, ...}, {measure.expr AS measure.name, ...}
   FROM {base_table}
   {LEFT JOIN alias ON condition for each join in minimal_join_set}
   WHERE {row_filter if defined} AND {user_filter if provided}
   GROUP BY {dim.expr, ... for all requested dimensions}
6. Return expanded SQL string
```

Step 5 produces a concrete, executable SQL string. This string is what DuckDB sees and plans against physical tables.

---

### Rust Binding Layer

#### duckdb-rs Crate Overview

The `duckdb` crate on crates.io (v0.10.x as of early 2025) wraps DuckDB's C API (`libduckdb`). It provides:

- `Connection`, `Statement`, `Row`, `Rows` — standard query execution
- `ToSql`, `FromSql` traits — Rust type mapping
- `vtab` module — virtual table (table-valued function) support
- `arrow` feature — Arrow RecordBatch integration

The crate does **not** expose:
- Parser extension hooks
- Replacement scan registration
- Custom catalog entry types
- Optimizer extension hooks

These require calling DuckDB's C++ SDK directly via raw FFI or using the `libduckdb-sys` crate (which exposes the C API) as a foundation for manual bindings.

#### Extension Registration in duckdb-rs

For building a DuckDB extension in Rust, the typical approach is to compile a `cdylib` that exports the two required C symbols:

```rust
#[no_mangle]
pub extern "C" fn semantic_views_init(db: *mut duckdb_sys::duckdb_database) {
    // register table functions, replacement scans, etc.
}

#[no_mangle]
pub extern "C" fn semantic_views_version() -> *const std::os::raw::c_char {
    // return DuckDB version string this was compiled against
}
```

The `duckdb_sys` crate provides raw FFI bindings to DuckDB's C API. The `duckdb` crate builds on top of `duckdb_sys`.

#### Table Function Registration (duckdb-rs vtab module)

The `vtab` module in `duckdb-rs` allows implementing virtual tables (table-valued functions). A virtual table implements:

```rust
pub trait VTab: Sized {
    type InitData: Sized;
    type BindData: Sized;
    fn bind(bind: &BindInfo, data: *mut Self::BindData) -> Result<(), Box<dyn std::error::Error>>;
    fn init(init: &InitInfo, data: *mut Self::InitData) -> Result<(), Box<dyn std::error::Error>>;
    fn func(func: &FunctionInfo, output: &mut DataChunk) -> Result<(), Box<dyn std::error::Error>>;
    fn parameters() -> Option<Vec<LogicalType>>;
}
```

This is adequate for table functions that produce rows. The bind phase receives parameters and registers output column names/types. The func phase produces rows in Arrow-style `DataChunk`s.

#### Named Parameters for Table Functions

For the `FROM my_view(DIMENSIONS region, METRICS revenue)` syntax, parameters need to be parsed. DuckDB table functions support named parameters via `duckdb_bind_get_named_parameter`. In duckdb-rs this is exposed through `BindInfo`. The extension would use named parameters:

```sql
FROM my_view(dimensions=['region', 'product'], metrics=['revenue', 'orders'])
```

or positional parameters if a fixed calling convention is chosen.

#### Raw FFI for Parser/Replacement Scan Hooks

For parser extension hooks and replacement scan registration, the extension must use `libduckdb-sys` directly. The relevant C API functions:

```c
// Add a replacement scan (fires when table name is unresolved)
void duckdb_add_replacement_scan(
    duckdb_database db,
    duckdb_replacement_callback_t replacement,
    void *extra_data,
    duckdb_delete_callback_t delete_callback
);
```

In Rust:
```rust
extern "C" fn replacement_scan(
    info: duckdb_sys::duckdb_replacement_scan_info,
    table_name: *const c_char,
    data: *mut c_void,
) {
    let name = unsafe { CStr::from_ptr(table_name).to_str().unwrap() };
    if is_semantic_view(name) {
        // set the replacement function
        unsafe {
            duckdb_sys::duckdb_replacement_scan_set_function_name(info, c"semantic_view_scan".as_ptr());
            // add parameters...
        }
    }
}
```

Parser extension hooks are exposed via the C++ SDK but **not** via the C API (`duckdb.h`). This is the most significant friction point: `CREATE SEMANTIC VIEW` DDL requires either:
1. Calling C++ API via FFI (brittle, ABI-sensitive), or
2. Intercepting at the SQL level using a workaround (see below).

#### Workaround for Custom DDL Without Parser Hooks

Since the C API does not expose parser extension hooks, the cleanest workaround for `CREATE SEMANTIC VIEW` is to use a **scalar function or table function that acts as DDL**:

```sql
-- Instead of:
CREATE SEMANTIC VIEW my_view (...);

-- Use:
SELECT define_semantic_view('my_view', '{...json definition...}');
-- or:
FROM create_semantic_view('my_view', dimensions => [...], measures => [...]);
```

This is less ergonomic but implementable entirely within duckdb-rs's table function API.

**Alternative:** Implement the extension in C++ for the DDL registration parts and link a Rust library for the core logic. The DuckDB extension template supports mixed C++/Rust builds.

**Alternative 2:** Use the `duckdb-extension` crate (if it has matured — see Research Gaps below) which may provide higher-level wrappers.

---

## Component Build Order

The components have clear dependencies. Build in this sequence:

### Phase 0: Project Skeleton
- Cargo workspace with `duckdb-sys` and `duckdb` dependencies
- cdylib crate structure with `_init` and `_version` exports
- Minimal "hello world" extension that loads in DuckDB without crashing
- Test harness: DuckDB process that loads the `.so`/`.dylib` and runs SQL

**Unblocks:** Everything.

### Phase 1: Storage Layer (In-Memory Registry + Persistence)
- `SemanticViewDef` struct with full definition schema (Rust)
- JSON serialization/deserialization (serde_json)
- `Registry` — in-memory `HashMap<String, SemanticViewDef>`
- DuckDB table `semantic_layer._definitions` creation at init time
- Load definitions from table into registry at init time
- Write definition to table when a view is created

**Depends on:** Phase 0
**Unblocks:** Phase 2 (needs something to expand), Phase 3 (needs something to query)

### Phase 2: DDL Interface (`CREATE SEMANTIC VIEW`)
- Parser for the DDL syntax (either custom SQL text or function-based)
- Validation of definition (join references, expression syntax, etc.)
- Integration with Phase 1 storage

Two sub-paths:
- **2A (function-based, faster):** Implement `define_semantic_view(name, json)` as a scalar/table function. User writes `SELECT define_semantic_view(...)`. Entirely within duckdb-rs table function API.
- **2B (native DDL, better UX):** Custom parser hook via C++ SDK FFI. Implements `CREATE SEMANTIC VIEW` natively. More complex but aligns with the project's stated goal.

Start with 2A to validate the registry and expansion before tackling 2B.

**Depends on:** Phase 0, Phase 1
**Unblocks:** Phase 3 (need views before querying them)

### Phase 3: Expansion Engine
- `expand(view_name, dimensions, measures, filter) -> String` pure function
- Join dependency resolution (which joins are needed for requested dims/measures)
- SQL builder (produces SELECT/FROM/JOIN/GROUP BY string)
- Validation (requested dims/measures exist, join graph is connected, etc.)
- Error messages for invalid combinations

**Depends on:** Phase 1 (registry lookup)
**Unblocks:** Phase 4

### Phase 4: Query Interface (Table Function / Replacement Scan)
- Table function `semantic_view_scan` registered with DuckDB
- Bind phase: look up view, validate parameters, return output schema
- Scan/execute phase: call expansion engine, execute expanded SQL, return rows
- Registration: either table function users call explicitly, or replacement scan for transparent name lookup

**Depends on:** Phase 3
**Unblocks:** Phase 5

### Phase 5: Integration & Testing
- End-to-end tests: `CREATE SEMANTIC VIEW` → `FROM my_view(...)` → correct SQL output
- TPC-H or jaffle-shop demo
- Error path tests (invalid dim name, missing join, etc.)
- Performance baseline (expansion latency)

**Depends on:** Phases 1–4

### Phase 6: Native DDL (if pursuing 2B)
- C++ FFI wrapper for parser extension hooks
- `CREATE SEMANTIC VIEW` parsed natively
- `DROP SEMANTIC VIEW` / `SHOW SEMANTIC VIEWS`

**Depends on:** Phase 2A working (validates the rest of the stack first)

---

## Reference Implementations

### Extensions with Custom Table Functions

**`httpfs` extension (C++)**
- Registers replacement scans for `FROM 'file.parquet'`, `FROM 'http://...'`
- Uses `config.replacement_scans` to intercept unresolved table names
- Source: `duckdb/extension/httpfs/` in DuckDB monorepo
- Relevant: shows the replacement scan pattern for transparent syntax

**`json` extension (C++)**
- Adds scalar functions (`json_extract`, `json_object`, etc.)
- Registers table function `read_json` / `read_json_auto`
- Source: `duckdb/extension/json/` in DuckDB monorepo
- Relevant: shows table function registration and named parameter handling

**`spatial` extension (C++)**
- Adds custom types, scalar functions, table functions
- Source: `duckdb/extension/spatial/` (community extension)
- Relevant: most complex community extension; shows how a large extension is structured

### Extensions with Rust (duckdb-rs)

**`duckdb-extension-template-rs`**
- Official Rust template for DuckDB extensions
- GitHub: `duckdb/duckdb-extension-template-rust` (or similar; check DuckDB GitHub org)
- Provides the `cdylib` skeleton, build scripts, and CI pipeline
- Uses `duckdb-rs` vtab module for table functions
- The canonical starting point for Rust extensions

**Community Rust extension examples:**
- `duckdb-excel` — reads Excel files, table function
- `duckdb-rs` itself has examples in `examples/` showing vtab usage

### Extensions with Custom DDL

**`delta` extension (C++)**
- Handles `FROM delta_scan('path')` — table function approach
- No custom DDL; uses function-based interface
- Shows that even complex extensions avoid custom DDL

**`iceberg` extension (C++)**
- `FROM iceberg_scan(...)` — same pattern
- No custom `CREATE ICEBERG ...` DDL

**Key observation:** No widely-used DuckDB extension implements custom `CREATE ...` DDL via the parser extension hook in a production-shipped extension. The pattern exists in the C++ SDK but is rarely used. The community practice is to use function-based interfaces for extension state management. This strongly suggests starting with the function-based DDL approach (Phase 2A).

### Closest Analog: DuckDB's Own `CREATE VIEW`

DuckDB's built-in `CREATE VIEW` is handled by the core parser and stores view definitions in the catalog as `ViewCatalogEntry`. Reading its implementation shows:
- View definitions are stored as SQL strings
- At query time, the binder substitutes the view's query in place of the view reference
- This is functionally identical to what semantic view expansion needs to do

The semantic view extension is essentially extending this pattern: store a parameterized view definition, and at query time, instantiate it with the user's requested dimensions and metrics.

---

## Open Questions

### Q1: Table Function vs. Replacement Scan for Query Syntax

Should the user write:
- `FROM my_view(dimensions=['region'], metrics=['revenue'])` (explicit table function call), or
- `FROM SEMANTIC_VIEW(my_view, DIMENSIONS region, METRICS revenue)` (single table function for all views)

The first requires one registration per view. The second uses a single global table function. The second is simpler to implement (no dynamic registration), but is slightly less ergonomic. Decision needed before Phase 4.

### Q2: Custom DDL vs. Function-Based Interface

`CREATE SEMANTIC VIEW` as native SQL DDL requires C++ FFI for parser hooks (not available in the C API). The function-based alternative (`SELECT define_semantic_view(...)`) is implementable entirely in Rust.

Open question: is native DDL essential for the v0.1 user experience, or is the function-based interface acceptable for an initial release?

**Lean:** Start with function-based (Phase 2A) and add native DDL in Phase 6. Validate the rest of the stack first.

### Q3: In-Process SQL Execution in Table Function Bind Phase

When the table function expands a semantic view and needs to execute the resulting SQL to determine the output schema (column types), it needs to call back into DuckDB from within a bind-phase callback. DuckDB may or may not permit re-entrant query execution during bind. This is a known limitation: the bind phase should be side-effect free and not execute queries.

**Mitigations:**
- Infer output schema from the semantic view definition (compute types from the dimension/measure definitions without executing SQL)
- Pre-declare all types in the definition (require users to annotate types)
- Use DuckDB's `DESCRIBE` or schema inference on the expanded SQL in a separate connection

This needs a prototype to determine the correct approach.

### Q4: Persistence Across `ATTACH`

When a DuckDB database file is opened with `ATTACH`, do tables in the `semantic_layer` schema appear correctly? Does the extension need to be loaded before or after `ATTACH`? The init hook needs to handle the case where the extension loads into a connection that already has an attached database containing semantic view definitions.

### Q5: duckdb-rs Extension API Completeness

The `duckdb-rs` crate's `vtab` module and extension registration APIs may not expose everything needed (replacement scan registration, named parameters for table functions, custom parameter types). The exact gap needs to be determined by reading the current source of `duckdb-rs` (v0.10.x or v0.11.x).

**Risk:** Medium-High. If the vtab API doesn't support all needed features, the extension will need to use `libduckdb-sys` raw FFI, significantly increasing complexity.

### Q6: DuckDB Version Stability

DuckDB's extension ABI requires that the extension is compiled against the exact same DuckDB version as the host. The `_version()` export enforces this. Community extensions are distributed as pre-compiled binaries per DuckDB version.

**Question:** What DuckDB version to target for v0.1? DuckDB is releasing frequently (0.9, 0.10, 1.0 in 2024; 1.1+ in 2025). Targeting a stable release (1.0+) is advisable.

### Q7: Syntax for Dimension/Metric Parameters

How should dimensions and metrics be passed to the table function?

```sql
-- Option A: Array literals
FROM my_view(dimensions => ['region', 'date'], metrics => ['revenue'])

-- Option B: Variadic strings (not standard)
FROM my_view(DIMENSIONS region, date, METRICS revenue)

-- Option C: Single struct parameter
FROM my_view({dimensions: ['region'], metrics: ['revenue']})
```

Option A is most compatible with DuckDB's table function parameter system. Options B and C require custom parsing. Decision needed before Phase 4.

---

## Confidence Levels

| Finding | Confidence | Notes |
|---------|------------|-------|
| Extensions use `_init` / `_version` C exports | High | Documented, stable, used by all extensions |
| Table functions are the primary query interface mechanism | High | Well-documented, multiple reference implementations |
| Replacement scan API exists and works for name interception | High | Used by httpfs, documented in DuckDB internals |
| Parser extension hooks exist for custom DDL | Medium | Exists in C++ SDK; not exposed in C API; rarely used in production |
| duckdb-rs vtab module supports table functions | High | Documented in crate; examples exist |
| duckdb-rs does NOT expose parser hooks or replacement scan | Medium-High | Based on reading crate API; would need verification against current version |
| Storing definitions in a DuckDB table (semantic_layer schema) persists across restarts | High | DuckDB persists all user-created tables in .duckdb file |
| In-process SQL re-execution during table function bind phase is problematic | Medium | Based on DuckDB's bind-phase semantics; needs prototype to confirm |
| No widely-used extension implements custom DDL via parser hooks | Medium | Based on surveying community extensions; may have missed examples |
| duckdb-extension-template-rust exists | Medium | Referenced in project documentation; current state/maintenance level unknown |
| DuckDB 1.0+ is a stable target | Medium | DuckDB reached 1.0 in 2024; API stability improved but extension ABI still version-locked |
| Optimizer extension hook is available for post-bind rewriting | Medium | Exists in C++ SDK; C API exposure status unknown |
