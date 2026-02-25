# Phase 4: Query Interface - Research

**Researched:** 2026-02-25
**Domain:** DuckDB Rust extension -- table function with named LIST parameters, SQL execution from within table function, WHERE composition, EXPLAIN support, DuckLake/Iceberg integration tests
**Confidence:** MEDIUM (table function named parameters verified via duckdb-rs source; SQL execution from within table function bind requires prototype validation; DuckLake integration test setup verified via official docs)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Query ergonomics:**
- Follow Snowflake's model: allow dimensions-only (returns distinct values), metrics-only (returns global aggregate), and dimensions+metrics (grouped aggregation)
- Filters via standard SQL WHERE on the result -- no `filters` parameter. WHERE clauses AND-compose with the view's row-level filters per QUERY-02
- Empty call `FROM view_name()` with no dimensions or metrics is an error with a helpful message directing users to specify at least one
- Error on empty should suggest: "Specify at least dimensions := [...] or metrics := [...]"

**Error experience:**
- Missing semantic view: fuzzy-match against registered views and suggest similar names (e.g., "Semantic view 'ordrs' not found. Did you mean 'orders'?")
- Invalid dimension/metric names: pass through expand() errors directly -- Phase 3 already produces fuzzy-matched suggestions
- SQL execution failures: show both the expanded SQL that was generated AND the DuckDB error message, so users can see what went wrong and what SQL caused it
- All errors include actionable hints pointing to relevant DDL functions (e.g., "Run FROM describe_semantic_view('orders') to see available dimensions and metrics")

**EXPLAIN output:**
- Use standard DuckDB EXPLAIN syntax: `EXPLAIN FROM view_name(dimensions := [...], metrics := [...])`
- Output includes three parts: (1) metadata header with semantic view name, requested dimensions, and requested metrics, (2) pretty-printed expanded SQL with indentation, (3) DuckDB's standard EXPLAIN plan
- Expanded SQL should be formatted multi-line with SELECT/FROM/WHERE/GROUP BY on separate lines

**Integration test setup: DuckLake + jaffle-shop:**
- Set up a local DuckLake catalog with DuckDB as the catalog backend
- Use dbt-labs/jaffle-shop dataset (download from S3)
- Python script handles the full lifecycle: download jaffle-shop data, create DuckLake Iceberg catalog, load data into Iceberg tables
- Data files must be gitignored (only the script and catalog config are committed)
- Just recipe calls the Python script for convenience

**Integration test scenarios (all five required):**
1. Basic round-trip: Define view over orders, query with dimensions + metrics, assert correct aggregates
2. WHERE composition: View has row filters + user adds WHERE, assert both apply correctly (QUERY-02)
3. Iceberg source query: Same semantic view pattern over DuckLake/Iceberg tables, prove extension works with iceberg_scan()
4. Multi-table joins: Semantic view with joins across orders+customers+products, assert join resolution works end-to-end
5. EXPLAIN plan equivalence: For key test cases, compare EXPLAIN output between semantic view query and equivalent hand-written SQL -- assert plans are identical or equivalent

### Claude's Discretion
- Table function vs replacement scan implementation approach
- SQL pretty-printing implementation (hand-rolled vs library)
- Exact error message formatting and wording
- How metadata header is formatted in EXPLAIN output

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| QUERY-01 | User can query a semantic view with named array parameters: `FROM my_view(dimensions := ['region', 'category'], metrics := ['total_revenue'])` | VTab trait with `named_parameters()` returning LIST(VARCHAR) parameters; `BindInfo::get_named_parameter()` returns `Value::List(Vec<Value>)` for extraction |
| QUERY-02 | User-supplied WHERE clauses AND-compose with the view's row-level filters | Already handled: `expand()` embeds view filters in the CTE's WHERE clause; user WHERE applies to the outer SELECT, naturally AND-composing. No additional code needed for WHERE composition. |
| QUERY-03 | `SELECT *` returns all requested dimensions and metrics; schema inferred at bind time | Output columns declared in `bind()` via `add_result_column()`; types inferred by executing expanded SQL with `LIMIT 0` at bind time, or defaulting to VARCHAR |
| QUERY-04 | `EXPLAIN` shows expanded SQL for debugging | Table function stores expanded SQL in BindData; EXPLAIN metadata row emitted as text showing the semantic view name, parameters, and formatted SQL |
| TEST-03 | Integration tests load extension, create views, run queries, assert correct results | SQLLogicTest `.test` files with `require semantic_views`; test against real tables with known data |
| TEST-04 | Integration test suite includes at least one Apache Iceberg table source | DuckLake extension provides local Iceberg-compatible tables; Python setup script loads jaffle-shop data into DuckLake catalog |
</phase_requirements>

---

## Summary

Phase 4 wires the expansion engine (completed in Phase 3) to DuckDB's query pipeline via a table function registered for each semantic view name. The core technical challenge is executing the expanded SQL from within the table function and correctly inferring the output schema at bind time.

The table function approach is strongly preferred over replacement scans because: (a) it naturally supports the `FROM view_name(dimensions := [...], metrics := [...])` syntax with named LIST parameters, (b) the VTab trait in duckdb-rs has full support for named parameters via `named_parameters()`, and (c) replacement scans can only intercept bare table references without parameter support.

The critical architectural question is how to execute the expanded SQL from within the table function. The table function's `func()` method has no `Connection` object -- only `TableFunctionInfo`. The recommended approach is to pass the raw `duckdb_connection` handle through `extra_info` at registration time, then use it in `func()` via unsafe FFI calls to `duckdb_query()` or `duckdb_prepare()`/`duckdb_execute_prepared()`. This is a different context from scalar function `invoke` (which deadlocks on SQL execution) -- the table function's `func` phase runs during the scan stage, not during execution lock acquisition.

**Primary recommendation:** Implement as a table function with named LIST(VARCHAR) parameters. Store the raw `duckdb_connection` in extra_info. Execute the expanded SQL via raw FFI `duckdb_query()` in the `func()` phase. Infer output schema at bind time from the expanded SQL executed with `LIMIT 0`. This requires prototype validation early in the phase.

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `duckdb` | `=1.4.4` | VTab trait, BindInfo, named parameters, DataChunkHandle, LogicalTypeHandle | Already in Cargo.toml; the only ergonomic Rust binding for DuckDB extensions |
| `libduckdb-sys` | `=1.4.4` | Raw FFI access to `duckdb_query()`, `duckdb_prepare()`, `duckdb_fetch_chunk()`, `duckdb_connection` | Already in Cargo.toml; needed for SQL execution from within table function where duckdb-rs high-level API is not available |
| `serde` + `serde_json` | `1` | JSON deserialization of semantic view definitions | Already in Cargo.toml |
| `strsim` | `0.11` | Levenshtein distance for fuzzy view name matching | Already in Cargo.toml; used in expand.rs for dim/metric suggestions |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `duckdb` (Python) | latest | DuckLake integration test setup script | Python script creates DuckLake catalog and loads jaffle-shop data |
| `ducklake` (DuckDB ext) | latest | DuckLake extension for Iceberg-compatible local tables | Integration tests use DuckLake tables as Iceberg source |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Table function (VTab) | Replacement scan | Replacement scan cannot accept named parameters (`dimensions := [...]`); can only intercept bare table names. Table function is the correct choice. |
| Raw FFI `duckdb_query()` | DuckDB `query()` SQL function | `query()` is a DuckDB-internal table function that executes a SQL string; not callable from within another table function's bind/func phase. |
| `LIMIT 0` schema inference | Hardcode all columns as VARCHAR | LIMIT 0 gives accurate types from DuckDB's type inference; VARCHAR fallback works but loses type fidelity for numeric aggregates. |

---

## Architecture Patterns

### Recommended Module Structure
```
src/
  query/                  # New module for Phase 4
    mod.rs                # Module declarations
    table_function.rs     # VTab implementation for semantic view queries
    error.rs              # Query-specific error types (view not found, empty params)
    explain.rs            # EXPLAIN output formatting
  expand.rs               # Modified: relax metrics-required constraint for dimensions-only queries
  catalog.rs              # Unchanged
  model.rs                # Unchanged
  ddl/                    # Unchanged
  lib.rs                  # Updated: register query table function in extension_entrypoint
```

### Pattern 1: Table Function with Named LIST Parameters

**What:** Register a table function for each semantic view that accepts `dimensions` and `metrics` as named `LIST(VARCHAR)` parameters.

**Approach A: Single generic table function** (recommended)
Register one table function per semantic view name at extension load time. Each has the view name baked in and the catalog state + connection handle in extra_info.

**Approach B: Dynamic registration via replacement scan**
Not viable -- replacement scans cannot pass named parameters.

**How to declare LIST(VARCHAR) named parameters:**
```rust
fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
    let list_varchar = LogicalTypeHandle::list(
        &LogicalTypeHandle::from(LogicalTypeId::Varchar)
    );
    Some(vec![
        ("dimensions".to_string(), list_varchar.clone()),
        ("metrics".to_string(), list_varchar),
    ])
}
```
Source: `LogicalTypeHandle::list()` documented at docs.rs/duckdb; `named_parameters()` trait method verified in duckdb-rs vtab/mod.rs

**How to extract list values in bind:**
```rust
fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn Error>> {
    let dims = match bind.get_named_parameter("dimensions") {
        Some(Value::List(values)) => values
            .into_iter()
            .map(|v| match v {
                Value::Text(s) => Ok(s),
                _ => Err("dimension names must be strings".into()),
            })
            .collect::<Result<Vec<String>, _>>()?,
        Some(_) => return Err("dimensions must be a list".into()),
        None => vec![],
    };
    // Same for metrics
    // ...
}
```
Source: `Value::List(Vec<Value>)` variant verified in duckdb-rs types/value.rs; `BindInfo::get_named_parameter()` returns `Option<Value>` per docs.rs

### Pattern 2: SQL Execution from Table Function via Raw FFI

**What:** Execute the expanded SQL against DuckDB from within the table function, since duckdb-rs does not expose a `Connection` object to the `func()` method.

**Critical constraint:** The table function `func()` phase runs during the scan stage of query execution. Unlike scalar function `invoke()` (which runs under execution locks and deadlocks on any SQL execution), the table function scan phase can execute SQL on the same connection via the C API.

**Approach: Store `duckdb_connection` in extra_info, execute via raw FFI**

At extension registration time:
```rust
// In extension_entrypoint, extract the raw duckdb_connection handle
let raw_conn: ffi::duckdb_connection = /* extract from Connection */;
let query_state = QueryState {
    catalog: catalog_state.clone(),
    conn: raw_conn,  // raw pointer -- must remain valid for extension lifetime
};
con.register_table_function_with_extra_info::<SemanticViewVTab, _>(
    view_name, &query_state
)?;
```

In func():
```rust
unsafe {
    let state = &*func.get_extra_info::<QueryState>();
    let sql_cstr = CString::new(bind_data.expanded_sql.as_str())?;
    let mut result: ffi::duckdb_result = std::mem::zeroed();
    let rc = ffi::duckdb_query(state.conn, sql_cstr.as_ptr(), &mut result);
    if rc != ffi::DuckDBSuccess {
        let err_msg = /* extract error from result */;
        ffi::duckdb_destroy_result(&mut result);
        return Err(format!("SQL execution failed: {err_msg}\nExpanded SQL: {}", bind_data.expanded_sql).into());
    }
    // Fetch chunks from result and copy to output DataChunkHandle
    // ...
    ffi::duckdb_destroy_result(&mut result);
}
```

**CRITICAL RISK**: This approach requires prototype validation. Re-entrant query execution from a table function scan is NOT guaranteed to work. If it fails, the fallback is to materialize the full result set in the `bind()` phase (where `duckdb_table_function_get_client_context` is available) and emit rows from BindData in `func()`.

Source: `duckdb_query(connection, query, out_result)` is in libduckdb-sys 1.4.4 FFI bindings (verified in `bindgen_bundled_version.rs` line 867)

### Pattern 3: Schema Inference at Bind Time

**What:** Determine output column names and types at bind time so `SELECT *` works correctly (QUERY-03).

**Option A: Execute with LIMIT 0** (preferred if re-entrant SQL works in bind)
```rust
fn bind(bind: &BindInfo) -> Result<Self::BindData, ...> {
    let expanded_sql = expand(view_name, &def, &req)?;
    let schema_sql = format!("{expanded_sql} LIMIT 0");
    // Execute schema_sql via FFI, read column names and types from result
    // Use types to call bind.add_result_column() for each column
}
```

**Option B: Infer from definition metadata** (fallback if re-entrant SQL blocked in bind)
- Dimension columns: always VARCHAR (safe default; expressions are opaque strings)
- Metric columns: always VARCHAR or DOUBLE (aggregation expressions are opaque)
- This loses type fidelity (e.g., `count(*)` returns BIGINT, not VARCHAR)

**Option C: All columns VARCHAR** (simplest fallback)
- Works but forces users to cast numeric results
- Acceptable as initial implementation; type inference can be improved later

**Recommendation:** Start with Option A. If prototype shows re-entrant SQL does not work in bind, fall back to Option B with a note that type inference is best-effort.

### Pattern 4: Dynamic Table Function Registration

**What:** Register a table function for each semantic view name so `FROM orders_view(...)` works.

**Challenge:** Views are defined dynamically via `define_semantic_view()`. Table functions must be registered at extension load time from `init_catalog()` rows, AND re-registered when `define_semantic_view()` is called.

**Approach:** At extension load, iterate the catalog and register one table function per view name. When `define_semantic_view()` is called, also register a new table function for the new view name. When `drop_semantic_view()` is called, the registered function remains but will error at bind time with "view not found."

**Alternative approach -- single function with positional param:**
Instead of one function per view, register a single `semantic_query(name, dimensions := [...], metrics := [...])` function. This avoids the dynamic registration problem but changes the syntax from `FROM orders_view(...)` to `FROM semantic_query('orders_view', dimensions := [...])`.

**CRITICAL DECISION NEEDED:** The user's requirement (QUERY-01) specifies `FROM my_view(dimensions := [...], metrics := [...])` -- one function per view name. This requires dynamic registration. But there's a subtlety: table functions registered after the extension entrypoint returns are catalog entries. The `define_semantic_view` scalar function could potentially call `duckdb_register_table_function` via raw FFI, but this is uncharted territory from within `invoke`. This needs prototype validation.

**Pragmatic recommendation:** If dynamic registration from `invoke` doesn't work, implement a single `semantic_query('view_name', dimensions := [...], metrics := [...])` function as the v0.1 approach, with a note that per-view functions require the C++ shim (v0.2).

### Pattern 5: WHERE Composition (QUERY-02)

**What:** User WHERE clauses AND-compose with view's row-level filters.

**How it already works:** The `expand()` function embeds view filters inside the CTE:
```sql
WITH "_base" AS (
    SELECT * FROM "orders"
    WHERE (status = 'completed') AND (amount > 100)  -- view filters
)
SELECT region AS "region", sum(amount) AS "total_revenue"
FROM "_base"
GROUP BY region
```

When the user writes:
```sql
SELECT * FROM orders_view(dimensions := ['region'], metrics := ['total_revenue'])
WHERE region = 'EMEA'
```

DuckDB applies the user's `WHERE region = 'EMEA'` to the result set of the table function, which already has view filters embedded in the CTE. The two filter sets are naturally AND-composed: the CTE filters restrict the base data, and the user's WHERE restricts the aggregated output.

**No additional code needed.** This is a consequence of the CTE architecture.

### Pattern 6: EXPLAIN Output (QUERY-04)

**What:** `EXPLAIN FROM view_name(...)` shows expanded SQL and DuckDB's plan.

**Approach:** The table function itself cannot intercept `EXPLAIN`. When DuckDB runs `EXPLAIN` on a table function, it shows the physical plan (TableFunctionScan). To show the expanded SQL, the table function should:

1. Store the expanded SQL string in BindData (already needed for execution)
2. When `EXPLAIN` is detected (DuckDB may not provide a direct signal), include the SQL in the bind error or as a special output mode

**Better approach:** Add a companion function `explain_semantic_view('view_name', dimensions := [...], metrics := [...])` that returns the expanded SQL as a VARCHAR result, along with metadata. This is more reliable than intercepting EXPLAIN.

**Alternative:** Register the expanded SQL as metadata on the table function that DuckDB's EXPLAIN can display. In practice, EXPLAIN on a table function shows the table function name and parameters, which is already informative. The expanded SQL can be exposed via `explain_semantic_view()`.

**Recommended approach:** Implement EXPLAIN support as a separate table function `explain_semantic_view()` that returns formatted output. Also store expanded SQL in BindData so it appears in error messages if SQL execution fails.

### Anti-Patterns to Avoid

- **Do NOT use replacement scans:** They cannot accept named parameters like `dimensions := [...]`. A replacement scan can only intercept a bare table name reference and delegate to a table function with positional parameters set by the scan itself. The named parameter syntax is fundamental to the user experience.

- **Do NOT attempt SQL execution from scalar function invoke:** This was thoroughly proven to deadlock in Phase 2 research. The table function `func()` phase is different and *may* allow re-entrant SQL, but this must be validated.

- **Do NOT hardcode column types without justification:** If type inference is possible (via LIMIT 0 execution or result metadata), use it. Defaulting everything to VARCHAR breaks numeric operations on metrics.

- **Do NOT register table functions lazily:** All known views must be registered at extension load time. Lazy registration would mean the first query fails because the function doesn't exist yet.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| LIST(VARCHAR) type construction | Manual FFI list type creation | `LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar))` | The duckdb-rs API provides a safe factory method |
| Fuzzy view name matching | Custom string comparison | `strsim::levenshtein()` with threshold <= 3 | Already used in expand.rs; consistent UX across errors |
| SQL identifier quoting | String concatenation with ad-hoc escaping | `quote_ident()` from expand.rs | Already built and tested; handles embedded double quotes |
| JSON parsing of definitions | Manual string parsing | `SemanticViewDefinition::from_json()` | Already built with serde; validates schema |
| DuckLake catalog setup | Manual Iceberg metadata | DuckLake extension (`INSTALL ducklake; ATTACH 'ducklake:...'`) | DuckLake provides local Iceberg-compatible tables without external infrastructure |

**Key insight:** The expansion engine (Phase 3) already does the heavy lifting. Phase 4 is primarily plumbing -- connecting the expansion output to DuckDB's query pipeline and wiring up error handling.

---

## Common Pitfalls

### Pitfall 1: Re-entrant SQL Execution Deadlock
**What goes wrong:** Executing SQL from within the table function `func()` phase may deadlock if DuckDB holds internal locks during scan execution that conflict with starting a new query.
**Why it happens:** DuckDB uses internal mutexes for transaction management and catalog access. The scalar function `invoke` context proved this locks are held (Phase 2 research). The table function context is different but not guaranteed to be lock-free.
**How to avoid:** Prototype this early. Test by registering a minimal table function that calls `duckdb_query()` in its `func()` phase. If it deadlocks, fall back to materializing results in `bind()`.
**Warning signs:** Hanging test, timeout in CI, or "lock acquisition timeout" error.

### Pitfall 2: `duckdb_connection` Lifetime in Extra Info
**What goes wrong:** The raw `duckdb_connection` pointer stored in extra_info becomes dangling if the connection is closed before the table function executes.
**Why it happens:** `duckdb_connection` is a raw pointer with no lifetime tracking. If stored by value in a Rust struct that's cloned into extra_info, the pointer persists but the underlying connection may be freed.
**How to avoid:** The connection from `extension_entrypoint` is the host DuckDB connection -- it lives for the lifetime of the database session. As long as we don't create a temporary connection, the pointer remains valid. Document this invariant clearly.
**Warning signs:** Segfault or invalid memory access during query execution.

### Pitfall 3: Expand() Requires Metrics -- Dimensions-Only Queries Fail
**What goes wrong:** The Phase 4 CONTEXT.md specifies "allow dimensions-only (returns distinct values)" but the current `expand()` function returns `ExpandError::EmptyMetrics` when no metrics are provided.
**Why it happens:** Phase 3 designed expand() with the invariant that at least one metric is required.
**How to avoid:** Modify `expand()` to allow empty metrics when dimensions are present. When metrics are empty and dimensions are present, generate a `SELECT DISTINCT` query instead of a `GROUP BY` query. When both are empty, the table function bind should error before calling expand().
**Warning signs:** "at least one metric is required" error when user calls `FROM view(dimensions := ['region'])`.

### Pitfall 4: Dynamic Table Function Registration from Scalar Invoke
**What goes wrong:** `define_semantic_view()` needs to register a new table function for the newly defined view, but it runs inside scalar function `invoke()` where SQL execution (and possibly function registration) deadlocks.
**Why it happens:** Same lock issue as Phase 2 -- the scalar invoke context holds execution locks.
**How to avoid:** Two options: (a) accept that newly defined views only become queryable after extension reload (user must `LOAD` or restart), or (b) use a single `semantic_query('view_name', ...)` function that looks up the view name at bind time instead of registering per-view functions.
**Warning signs:** Newly defined view cannot be queried without restart.

### Pitfall 5: Column Type Mismatch Between Schema and Data
**What goes wrong:** Schema declared at bind time (e.g., all VARCHAR) doesn't match actual data types returned by the executed SQL (e.g., BIGINT for count, DOUBLE for sum).
**Why it happens:** If type inference fails or is skipped, bind may declare wrong types.
**How to avoid:** Use LIMIT 0 execution to discover actual types. If that's not possible, use VARCHAR as universal fallback -- DuckDB will cast automatically.
**Warning signs:** "Type mismatch" errors at query time, or incorrect numeric results due to string-to-number conversion.

### Pitfall 6: DuckLake Extension Not Available in SQLLogicTest Runner
**What goes wrong:** Integration tests that require DuckLake/Iceberg fail because the test runner doesn't have the DuckLake extension installed.
**Why it happens:** The SQLLogicTest runner uses a DuckDB binary that may not have DuckLake pre-installed. DuckLake is a community extension that needs explicit INSTALL.
**How to avoid:** Integration tests that use DuckLake should include `INSTALL ducklake; LOAD ducklake;` in the test setup. Alternatively, use Python-based integration tests that install extensions programmatically.
**Warning signs:** "Extension 'ducklake' not found" error in test output.

---

## Code Examples

### Example 1: VTab with Named LIST Parameters (Verified Pattern)

```rust
use duckdb::{
    core::{DataChunkHandle, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

struct SemanticViewVTab;

impl VTab for SemanticViewVTab {
    type BindData = SemanticViewBindData;
    type InitData = SemanticViewInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Extract named list parameters
        let dimensions: Vec<String> = match bind.get_named_parameter("dimensions") {
            Some(duckdb::types::Value::List(values)) => {
                values.into_iter().map(|v| v.to_string()).collect()
            }
            _ => vec![],
        };
        let metrics: Vec<String> = match bind.get_named_parameter("metrics") {
            Some(duckdb::types::Value::List(values)) => {
                values.into_iter().map(|v| v.to_string()).collect()
            }
            _ => vec![],
        };

        // Look up definition from catalog (via extra_info)
        let state_ptr = bind.get_extra_info::<QueryState>();
        let catalog = unsafe { &(*state_ptr).catalog };
        // ... expand, declare columns, etc.

        Ok(SemanticViewBindData { /* ... */ })
    }

    fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
        let list_varchar = LogicalTypeHandle::list(
            &LogicalTypeHandle::from(LogicalTypeId::Varchar)
        );
        Some(vec![
            ("dimensions".to_string(), list_varchar),
            ("metrics".to_string(), LogicalTypeHandle::list(
                &LogicalTypeHandle::from(LogicalTypeId::Varchar)
            )),
        ])
    }

    // ... init and func
}
```
Source: VTab trait from duckdb-rs vtab/mod.rs; named_parameters pattern from HelloWithNamedVTab test; LogicalTypeHandle::list from docs.rs/duckdb

### Example 2: DuckLake Integration Test Setup

```sql
-- Install and load DuckLake extension
INSTALL ducklake;
LOAD ducklake;

-- Create a local DuckLake catalog
ATTACH 'ducklake:test_catalog.ducklake' AS test_lake (DATA_PATH 'test_data/');

-- Create tables and load jaffle-shop data
CREATE TABLE test_lake.orders AS SELECT * FROM read_csv('jaffle-data/raw_orders.csv');
CREATE TABLE test_lake.customers AS SELECT * FROM read_csv('jaffle-data/raw_customers.csv');
CREATE TABLE test_lake.items AS SELECT * FROM read_csv('jaffle-data/raw_items.csv');

-- Now these are Iceberg-compatible tables accessible via DuckLake
SELECT * FROM test_lake.orders LIMIT 5;
```
Source: DuckLake official docs at github.com/duckdb/ducklake; ATTACH syntax verified

### Example 3: Raw FFI SQL Execution

```rust
use libduckdb_sys as ffi;
use std::ffi::CString;

unsafe fn execute_sql(
    conn: ffi::duckdb_connection,
    sql: &str,
) -> Result<ffi::duckdb_result, String> {
    let sql_cstr = CString::new(sql).map_err(|e| e.to_string())?;
    let mut result: ffi::duckdb_result = std::mem::zeroed();
    let rc = ffi::duckdb_query(conn, sql_cstr.as_ptr(), &mut result);
    if rc != ffi::DuckDBSuccess {
        let err_ptr = ffi::duckdb_result_error(&mut result);
        let err_msg = if err_ptr.is_null() {
            "unknown error".to_string()
        } else {
            std::ffi::CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
        };
        ffi::duckdb_destroy_result(&mut result);
        return Err(err_msg);
    }
    Ok(result)
}
```
Source: `duckdb_query` signature from libduckdb-sys 1.4.4 bindgen_bundled_version.rs line 867

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Replacement scan for table-like queries | Table function with named parameters | DuckDB 0.9+ (C API `duckdb_table_function_add_named_parameter`) | Named parameters enable `dimensions := [...]` syntax |
| `duckdb::vtab::arrow::ArrowVTab` for data return | Direct `DataChunkHandle` manipulation | duckdb-rs 1.x | Simpler, no Arrow dependency needed for non-Arrow data |
| DuckDB Iceberg extension (read-only) | DuckLake extension (read-write, Iceberg-compatible) | DuckLake 0.1 (2025) | Simpler local setup; no need for external Iceberg catalog |
| Manual Iceberg REST catalog + MinIO | DuckLake with local DuckDB catalog backend | 2025 | Dramatically simpler test setup; just `INSTALL ducklake; ATTACH` |

**Deprecated/outdated:**
- PyIceberg + Spark setup for local Iceberg tables -- DuckLake replaces this entirely for testing
- `duckdb::vtab::arrow` -- not needed for this use case; direct DataChunkHandle manipulation is simpler

---

## Critical Design Decisions

### Decision 1: Per-View Table Function vs Single Generic Function

**Per-view registration:** `FROM orders_view(dimensions := [...])` -- requires registering one function per catalog entry at load time, plus dynamic re-registration when views are defined.

**Single generic function:** `FROM semantic_query('orders_view', dimensions := [...])` -- one function registered once; view name is a positional parameter.

**Tradeoff:** Per-view is better UX (matches QUERY-01 spec and Snowflake convention). Single generic is simpler to implement and avoids the dynamic registration problem.

**Recommendation:** Start with the single generic `semantic_query()` approach for robustness, then attempt per-view registration if the prototype shows dynamic function registration works from scalar invoke. If per-view registration works, register both: per-view functions for ergonomics AND `semantic_query()` as a fallback.

### Decision 2: Schema Inference Strategy

The output schema must be known at bind time for `SELECT *` to work (QUERY-03).

**Option A: LIMIT 0 execution** -- Execute expanded SQL with LIMIT 0 at bind time, read column types from result metadata. Most accurate.

**Option B: Definition metadata** -- Dimensions are always VARCHAR, metrics are always DOUBLE. Simple but loses type fidelity.

**Option C: All VARCHAR** -- Everything is text. Simplest but worst UX.

**Recommendation:** Try Option A first. If re-entrant SQL in bind fails, use Option B with a documented limitation.

### Decision 3: expand() Modification for Dimensions-Only Queries

The CONTEXT.md requires dimensions-only queries to return distinct values. The current `expand()` requires at least one metric.

**Required change:** When `req.metrics` is empty and `req.dimensions` is non-empty, generate `SELECT DISTINCT` instead of `GROUP BY`. Remove the `EmptyMetrics` error case (or change it to `EmptyRequest` when both are empty).

This is a modification to Phase 3 code that must be made carefully to avoid breaking existing tests.

---

## Open Questions

1. **Can `duckdb_query()` be called from within a table function's `func()` phase?**
   - What we know: Scalar function `invoke` deadlocks on SQL execution (confirmed in Phase 2). Table function `func()` runs during scan phase, which may be a different lock context.
   - What's unclear: Whether the scan phase holds locks that conflict with `duckdb_query()` on the same connection.
   - Recommendation: **Prototype this first.** Write a minimal table function that calls `duckdb_query("SELECT 42")` in `func()`. If it works, proceed. If not, try materializing in `bind()` instead.

2. **Can `duckdb_register_table_function()` be called from within scalar function `invoke()`?**
   - What we know: SQL execution from invoke deadlocks. Function registration is a catalog mutation, not SQL execution.
   - What's unclear: Whether the DuckDB catalog API is thread-safe for function registration during scalar invoke.
   - Recommendation: If per-view functions are desired, prototype this. If it fails, use the single `semantic_query()` approach.

3. **How to extract the raw `duckdb_connection` from `duckdb-rs` `Connection` in the extension entrypoint?**
   - What we know: `InnerConnection.con` is the raw `ffi::duckdb_connection`, but it's behind `RefCell<InnerConnection>` which is private.
   - What's unclear: Whether there's a public API to access the raw handle, or if we need unsafe access.
   - Recommendation: Check if `Connection` exposes the raw handle via any method. If not, the `duckdb_entrypoint_c_api` macro may provide the raw handle before wrapping it in `Connection`. Look at the macro expansion.

4. **Does DuckLake work in the SQLLogicTest runner?**
   - What we know: DuckLake is a community extension installable via `INSTALL ducklake`.
   - What's unclear: Whether the test runner's DuckDB binary supports dynamic extension installation.
   - Recommendation: Test with `INSTALL ducklake; LOAD ducklake;` in a SQLLogicTest file. If not supported, use Python-based integration tests instead.

---

## Sources

### Primary (HIGH confidence)
- duckdb-rs source code (VTab trait, BindInfo, named_parameters) -- verified in `/Users/paul/.cargo/registry/src/.../duckdb-1.4.4/src/vtab/mod.rs`
- libduckdb-sys 1.4.4 FFI bindings -- verified `duckdb_query()`, `duckdb_table_function_get_client_context()`, `duckdb_add_replacement_scan()` in `bindgen_bundled_version.rs`
- DuckDB C API table functions docs -- https://duckdb.org/docs/stable/clients/c/table_functions
- DuckDB C API complete reference -- https://duckdb.org/docs/stable/clients/c/api
- DuckLake official docs -- https://github.com/duckdb/ducklake
- docs.rs/duckdb LogicalTypeHandle -- `LogicalTypeHandle::list()` factory method for LIST types

### Secondary (MEDIUM confidence)
- DuckDB query/query_table functions -- https://duckdb.org/docs/stable/guides/sql_features/query_and_query_table_functions
- DuckDB replacement scans -- https://duckdb.org/docs/stable/clients/c/replacement_scans
- DuckDB Iceberg extension table functions -- https://deepwiki.com/duckdb/duckdb-iceberg/3-table-functions
- duckdb-rs VTab documentation -- https://deepwiki.com/duckdb/duckdb-rs/5.1-virtual-tables-(vtab)

### Tertiary (LOW confidence)
- Re-entrant SQL execution safety from table function scan phase -- no authoritative source found; requires prototype validation
- Dynamic function registration from scalar invoke -- no authoritative source found; requires prototype validation

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in Cargo.toml; VTab and named parameter APIs verified in source
- Architecture: MEDIUM -- table function pattern is well-understood; SQL execution from func() is the key unknown requiring prototype
- Pitfalls: HIGH -- re-entrant query risk well-documented from Phase 2 experience; mitigations identified
- Integration tests: MEDIUM -- DuckLake setup verified via official docs; availability in test runner needs validation

**Research date:** 2026-02-25
**Valid until:** 2026-03-25 (stable domain; duckdb-rs version pinned)
