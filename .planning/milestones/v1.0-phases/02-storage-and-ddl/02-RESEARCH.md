# Phase 2: Storage and DDL - Research

**Researched:** 2026-02-24
**Domain:** DuckDB Rust extension — scalar functions, table functions, catalog persistence
**Confidence:** HIGH (core APIs verified via Context7/docs.rs; patterns verified against duckdb-rs source)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Definition JSON schema:**
- Flat object with typed arrays (not nested entity structure)
- Required top-level fields: `base_table` (string), `dimensions` (array), `metrics` (array)
- Optional top-level fields: `filters` (array, defaults to []), `joins` (array, defaults to [])
- Dimension item shape: `{ "name": "region", "expr": "region" }` — name + expr only
- Metric item shape: `{ "name": "revenue", "expr": "sum(amount)" }` — name + expr only
- Join item shape: `{ "table": "customers", "on": "orders.customer_id = customers.id" }`
- Validation at define time: error immediately on invalid JSON or missing required fields; nothing written to catalog on failure

**Error contracts:**
- `define_semantic_view` with a name that already exists: **error** — user must call `drop_semantic_view` first
- `drop_semantic_view` with a name that doesn't exist: **error** — "semantic view 'X' does not exist"
- `describe_semantic_view` with a name that doesn't exist: **error** — "semantic view 'X' does not exist"
- Success confirmation: single VARCHAR column with human-readable message — standard scalar function pattern

**`describe_semantic_view` output shape:**
- Returns one row with typed columns: `(name VARCHAR, base_table VARCHAR, dimensions VARCHAR/JSON, metrics VARCHAR/JSON, filters VARCHAR/JSON, joins VARCHAR/JSON)`
- JSON columns: use DuckDB JSON type if duckdb-rs makes it straightforward; fall back to VARCHAR — Claude's discretion
- `list_semantic_views()` returns `(name VARCHAR, base_table VARCHAR)` only

**Catalog table placement and sync:**
- Catalog lives at `semantic_layer._definitions`
- Extension creates `semantic_layer` schema and `_definitions` table on every extension load (CREATE SCHEMA/TABLE IF NOT EXISTS — idempotent)
- Table schema: `(name VARCHAR PRIMARY KEY, definition JSON)`
- In-memory HashMap loaded from catalog at extension load time (SELECT all rows after table creation)
- Write order: write to catalog first, update HashMap only on success
- Catalog is authoritative; HashMap is a load-time cache

### Claude's Discretion

- JSON columns in `describe_semantic_view`: use DuckDB JSON type if straightforward; fall back to VARCHAR

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DDL-01 | User can register a semantic view via `SELECT define_semantic_view('name', '{json}')` | VScalar trait + `register_scalar_function` covers scalar function returning VARCHAR; serde_json for validation |
| DDL-02 | User can remove a semantic view via `SELECT drop_semantic_view('name')` | Same pattern as DDL-01 — scalar function with HashMap + catalog mutation |
| DDL-03 | User can list all registered semantic views via `FROM list_semantic_views()` | VTab trait + `register_table_function` covers zero-parameter table function |
| DDL-04 | User can inspect a semantic view definition via `FROM describe_semantic_view('name')` | VTab trait with one VARCHAR parameter; multi-column output via flat_vector per column |
| DDL-05 | Definitions persist across DuckDB restarts, stored in catalog table | `execute_batch` in entrypoint creates schema+table; `query_row`/`prepare` loads rows into HashMap |
</phase_requirements>

---

## Summary

Phase 2 implements the full DDL surface for semantic views: two scalar functions (`define_semantic_view`, `drop_semantic_view`) and two table functions (`list_semantic_views`, `describe_semantic_view`), backed by a catalog DuckDB table that persists definitions across restarts and an in-memory HashMap that serves as a load-time cache.

The duckdb-rs `vscalar` feature provides the `VScalar` trait for scalar functions and `register_scalar_function` for registration. The `vtab` feature (already supported by the project's `loadable-extension` feature flag) provides the `VTab` trait for table functions and `register_table_function`. Both are callable from the extension entrypoint `Connection`. The key implementation challenge is threading state (the HashMap) through to both scalar and table functions — the cleanest approach is `register_scalar_function_with_state` and `register_table_function_with_extra_info`, which pass a cloned or Arc-wrapped state reference into each function at registration time.

Catalog persistence is straightforward: call `execute_batch` in the entrypoint to create the schema and table (idempotent), then load existing rows into the HashMap with `prepare` + `query_map`. JSON parsing at define time uses `serde_json`; the definition is stored as a raw JSON string in the VARCHAR/JSON catalog column. The write-catalog-first pattern maps naturally to Rust `Result` propagation — write to catalog, return error on failure, update HashMap only on `Ok`.

**Primary recommendation:** Use `VScalar` for `define_semantic_view`/`drop_semantic_view`, `VTab` for `list_semantic_views`/`describe_semantic_view`, `Arc<RwLock<HashMap<String, String>>>` for shared mutable state, and `execute_batch` + `prepare`/`query_map` for catalog init. Add `serde_json = "1"` as a dependency for JSON validation.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `duckdb` (already in Cargo.toml) | `=1.4.4` | VScalar + VTab traits, `register_scalar_function`, `register_table_function`, `execute_batch`, `prepare` | The duckdb-rs crate is the only ergonomic Rust binding; version-pinned to match DuckDB ABI |
| `serde_json` | `"1"` | Parse and validate incoming definition JSON at define time; serialize Definition struct back to JSON string | Standard Rust JSON library; duckdb crate uses it internally as optional dep |

### Feature Flags Required

The existing Cargo.toml has `features = ["loadable-extension"]`. Phase 2 requires adding:

| Feature | Adds |
|---------|------|
| `vscalar` | `VScalar` trait, `register_scalar_function`, `ScalarFunctionSignature` |

The `vtab` feature is implicitly enabled by `loadable-extension` (verify at build time; if not, add explicitly).

**Updated Cargo.toml dependency:**
```toml
duckdb = { version = "=1.4.4", features = ["loadable-extension", "vscalar"] }
serde_json = "1"
```

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `std::sync::{Arc, RwLock}` | std | Share HashMap between registered functions | Required when state is read by multiple functions; prefer `RwLock` over `Mutex` for read-heavy workloads |
| `std::collections::HashMap` | std | In-memory cache: name → definition JSON string | Always |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `serde_json` | `simd-json`, `sonic-rs` | serde_json is standard and sufficient; no perf pressure on DDL path |
| `Arc<RwLock<HashMap>>` | `OnceLock<Mutex<HashMap>>` | OnceLock requires init-once semantics which conflicts with entrypoint receive-and-load; Arc gives flexible clone-into-closure sharing |
| `register_scalar_function_with_state` | raw `libduckdb-sys` FFI | duckdb-rs high-level API is sufficient and avoids unsafe code proliferation |

---

## Architecture Patterns

### Recommended Project Structure

```
src/
├── lib.rs           # Extension entrypoint: catalog init, function registration
├── catalog.rs       # CatalogState type (Arc<RwLock<HashMap>>), load_catalog(), write/delete helpers
├── ddl/
│   ├── mod.rs       # pub use of DDL functions
│   ├── define.rs    # define_semantic_view VScalar impl
│   ├── drop.rs      # drop_semantic_view VScalar impl
│   ├── list.rs      # list_semantic_views VTab impl
│   └── describe.rs  # describe_semantic_view VTab impl
└── model.rs         # SemanticViewDefinition struct + serde_json validation
```

### Pattern 1: Scalar Function with Shared State (VScalar)

**What:** Implement `VScalar` for `define_semantic_view` and `drop_semantic_view`. Pass the shared catalog state via `register_scalar_function_with_state`.

**When to use:** Functions that return a single value per call (not a table) and need mutable access to shared state.

```rust
// Source: https://docs.rs/duckdb/latest/duckdb/vscalar/index.html
// Source: duckdb-rs src/vscalar/mod.rs (EchoScalar test pattern)
use duckdb::{Connection, Result};
use duckdb::vscalar::{VScalar, ScalarFunctionSignature};
use duckdb::core::{LogicalTypeId, DataChunkHandle, WritableVector};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

pub type CatalogState = Arc<RwLock<HashMap<String, String>>>;

struct DefineSemanticView;

impl VScalar for DefineSemanticView {
    type State = CatalogState;

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeId::Varchar.into(), // name
                LogicalTypeId::Varchar.into(), // definition JSON
            ],
            LogicalTypeId::Varchar.into(),     // confirmation message
        )]
    }

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. Read name param from input.flat_vector(0)
        // 2. Read json param from input.flat_vector(1)
        // 3. Validate JSON with serde_json::from_str
        // 4. Check HashMap — error if name exists
        // 5. Write to catalog table (separate connection or use execute on entrypoint conn)
        // 6. On catalog success: state.write().unwrap().insert(name, json)
        // 7. output.flat_vector().insert(0, "Semantic view 'X' registered successfully")
        Ok(())
    }
}

// In entrypoint:
pub fn register_ddl_functions(con: &Connection, state: CatalogState) -> Result<()> {
    con.register_scalar_function_with_state::<DefineSemanticView>("define_semantic_view", &state)?;
    // ... register others
    Ok(())
}
```

### Pattern 2: Table Function with Shared State (VTab + extra_info)

**What:** Implement `VTab` for `list_semantic_views` and `describe_semantic_view`. Use `register_table_function_with_extra_info` to inject the shared catalog state.

**When to use:** Functions invoked with `FROM fn()` syntax that return multiple rows/columns.

```rust
// Source: https://docs.rs/duckdb/latest/duckdb/vtab/trait.VTab.html
// Source: duckdb-rs src/vtab/mod.rs (HelloVTab test pattern)
use duckdb::vtab::{VTab, BindInfo, InitInfo, TableFunctionInfo, DataChunkHandle};
use std::sync::atomic::{AtomicBool, Ordering};

struct ListSemanticViewsVTab;

struct ListBindData {
    // snapshot of views taken at bind time
    rows: Vec<(String, String)>, // (name, base_table)
}

struct ListInitData {
    done: AtomicBool,
}

impl VTab for ListSemanticViewsVTab {
    type BindData = ListBindData;
    type InitData = ListInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Add output columns
        bind.add_result_column("name", LogicalTypeId::Varchar.into());
        bind.add_result_column("base_table", LogicalTypeId::Varchar.into());

        // Read catalog state from extra_info
        let state = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state).read().unwrap() };
        let rows = guard.iter().map(|(name, json)| {
            let def: serde_json::Value = serde_json::from_str(json).unwrap();
            (name.clone(), def["base_table"].as_str().unwrap_or("").to_string())
        }).collect();

        Ok(ListBindData { rows })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ListInitData { done: AtomicBool::new(false) })
    }

    fn func(func: &TableFunctionInfo<Self>, output: &mut DataChunkHandle)
        -> Result<(), Box<dyn std::error::Error>>
    {
        let init = func.get_init_data();
        if init.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }
        let bind = func.get_bind_data();
        let n = bind.rows.len();
        let name_vec = output.flat_vector(0);
        let base_table_vec = output.flat_vector(1);
        for (i, (name, base_table)) in bind.rows.iter().enumerate() {
            name_vec.insert(i, name.as_str());
            base_table_vec.insert(i, base_table.as_str());
        }
        output.set_len(n);
        Ok(())
    }
}
```

### Pattern 3: Catalog Init in Entrypoint

**What:** On every extension load, create the catalog schema+table idempotently, then read all rows into the HashMap.

```rust
// Source: docs.rs/duckdb Connection::execute_batch, Connection::prepare
fn init_catalog(con: &Connection) -> Result<CatalogState> {
    con.execute_batch(
        "CREATE SCHEMA IF NOT EXISTS semantic_layer;
         CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
             name VARCHAR PRIMARY KEY,
             definition JSON
         );"
    )?;

    let mut map = HashMap::new();
    let mut stmt = con.prepare("SELECT name, definition FROM semantic_layer._definitions")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name, def) = row?;
        map.insert(name, def);
    }

    Ok(Arc::new(RwLock::new(map)))
}
```

### Pattern 4: Write-Catalog-First Mutation

**What:** For DDL mutations, write to catalog first (using `execute`), update HashMap only on `Ok`. Rust's `?` operator naturally propagates the error if the write fails, preventing HashMap from being updated.

```rust
fn catalog_insert(con: &Connection, state: &CatalogState, name: &str, json: &str)
    -> Result<(), Box<dyn std::error::Error>>
{
    // Write to catalog first — error propagates via ?
    con.execute(
        "INSERT INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
        duckdb::params![name, json],
    )?;
    // Only update HashMap if catalog write succeeded
    state.write().unwrap().insert(name.to_string(), json.to_string());
    Ok(())
}
```

### Anti-Patterns to Avoid

- **Updating HashMap before catalog write:** If the catalog write fails, the HashMap has stale state that survives until the next extension load. Always write catalog first.
- **Using `Arc<Mutex<_>>` and calling `.lock()` during DuckDB's bind/func phases without checking for deadlock:** The `RwLock` read path is safe for concurrent reads (multiple `FROM list_semantic_views()` calls); avoid holding the write lock across catalog I/O.
- **Storing a `Connection` inside `VScalar::State` or `VTab::BindData`:** DuckDB connections are not `Send + Sync`. Instead, pass a separate `Connection` for catalog writes or use `execute` on the entrypoint connection by keeping catalog ops in the entrypoint-level init function.
- **Parsing the definition JSON at query time (in VTab::func) instead of bind time:** Bind time is the right place; func is called repeatedly per chunk.
- **Using `register_table_function` (not `_with_extra_info`) for stateful VTab:** Without extra_info, the VTab has no way to access the shared HashMap.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON schema validation | Custom parser | `serde_json::from_str` + struct with `#[serde(deny_unknown_fields)]` | Edge cases in JSON parsing (unicode escapes, null bytes, trailing commas) are numerous |
| VARCHAR reading from DuckDB chunks | Raw pointer arithmetic | `flat_vector(idx).as_slice_with_len::<duckdb_string_t>` + `DuckString` | duckdb-rs wraps the inline/pointer string duality; getting this wrong causes UB |
| Catalog schema management | Custom binary format | DuckDB table via `execute_batch` | DuckDB already persists the file; piggybacking on it is free and survives compaction, WAL replay, etc. |
| Concurrent state protection | Custom lock-free structure | `Arc<RwLock<HashMap>>` | Standard Rust pattern; `RwLock` allows multiple concurrent readers, correct for read-heavy DDL cache |

**Key insight:** The function registration and invocation plumbing in duckdb-rs is non-trivial to replicate; always use the `VScalar`/`VTab` traits and their associated registration methods rather than raw `libduckdb-sys` FFI calls.

---

## Common Pitfalls

### Pitfall 1: `vscalar` Feature Not Enabled

**What goes wrong:** `register_scalar_function` and `VScalar` are not available; compiler gives "method not found" or "trait not found".
**Why it happens:** `vscalar` is an opt-in feature in duckdb-rs, not included with `loadable-extension`.
**How to avoid:** Add `"vscalar"` to the features list in Cargo.toml for the duckdb dependency.
**Warning signs:** Build failure mentioning `VScalar` or `register_scalar_function` not in scope.

### Pitfall 2: Connection is Not Send — Cannot Store in State

**What goes wrong:** Attempting to store a `duckdb::Connection` inside `VScalar::State` or `VTab::BindData` fails to compile with "the trait bound `Connection: Send` is not satisfied".
**Why it happens:** DuckDB connections hold raw pointers to C structures; duckdb-rs deliberately does not implement `Send` for `Connection`.
**How to avoid:** Do not store `Connection` in function state. Catalog reads happen at init time; catalog writes during scalar function invocation require either a fresh `Connection::open` to the same database file or a channel to the entrypoint connection. The simplest approach: open a fresh connection to the db file path for writes inside the scalar function's `invoke`.
**Warning signs:** Compile error referencing `Send` bound on `Connection`.

### Pitfall 3: `get_extra_info` Returns Raw Pointer — Must Use Correctly

**What goes wrong:** Calling `bind.get_extra_info::<CatalogState>()` and dereferencing incorrectly causes UB or a crash.
**Why it happens:** `get_extra_info` returns `*const T`; if `T` is `CatalogState` (i.e., `Arc<RwLock<...>>`), you must dereference the pointer to clone the `Arc` (not take ownership).
**How to avoid:** Use `unsafe { (*state).clone() }` to get an owned `Arc` clone inside bind.
**Warning signs:** Segfault or double-free in bind phase; sanitizer reports.

### Pitfall 4: Silent HashMap/Catalog Drift

**What goes wrong:** HashMap says a view exists; catalog says it doesn't (or vice versa).
**Why it happens:** Any code path that updates one without the other, including error handling branches.
**How to avoid:** The write-catalog-first pattern (Pattern 4) makes this structurally impossible for writes. For reads: HashMap is only populated from the catalog at load time; it is never independently mutated without a preceding catalog write.
**Warning signs:** `describe_semantic_view` returns a view that `SELECT * FROM semantic_layer._definitions` does not contain.

### Pitfall 5: `execute_batch` in Entrypoint Fails Silently if Not Propagated

**What goes wrong:** `execute_batch("CREATE SCHEMA IF NOT EXISTS ...")` fails (e.g., permission issue or corrupt DB) but the error is swallowed, causing all subsequent catalog reads to fail with confusing errors.
**Why it happens:** If the entrypoint uses `let _ = con.execute_batch(...)` or doesn't propagate via `?`.
**How to avoid:** Always use `?` on `execute_batch` inside the entrypoint. The entrypoint returns `Result<(), Box<dyn Error>>` — errors are surfaced to the user as a `LOAD` failure.
**Warning signs:** Extension loads without error but `SELECT define_semantic_view(...)` panics or returns `table not found`.

### Pitfall 6: JSON Type Column vs VARCHAR

**What goes wrong:** Storing the definition as `JSON` column type requires the DuckDB JSON extension to be loaded; otherwise the `definition JSON` column DDL may fail or behave unexpectedly in some DuckDB builds.
**Why it happens:** `JSON` is implemented as a loadable extension in DuckDB, not a built-in type in all contexts.
**How to avoid:** Use `VARCHAR` for the `definition` column if JSON type availability cannot be guaranteed. Functionally equivalent — DuckDB stores JSON as VARCHAR internally anyway. The `LogicalTypeId` enum does not expose a `Json` variant in the verified duckdb-rs docs; use `Varchar` for output columns containing JSON text in `describe_semantic_view`.
**Warning signs:** `Catalog Error` or `Type not found: JSON` on extension load.

---

## Code Examples

Verified patterns from official sources:

### Scalar Function Registration (Two-Parameter VARCHAR → VARCHAR)

```rust
// Source: https://docs.rs/duckdb/latest/duckdb/vscalar/struct.ScalarFunctionSignature.html
// Source: duckdb-rs src/vscalar/mod.rs

impl VScalar for DefineSemanticView {
    type State = CatalogState;

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeId::Varchar.into(), // view name
                LogicalTypeId::Varchar.into(), // definition JSON string
            ],
            LogicalTypeId::Varchar.into(),     // confirmation message
        )]
    }

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Read VARCHAR from column 0
        let names = input.flat_vector(0);
        let names = names.as_slice_with_len::<duckdb_string_t>(input.len());
        // Read VARCHAR from column 1
        let jsons = input.flat_vector(1);
        let jsons = jsons.as_slice_with_len::<duckdb_string_t>(input.len());

        let out = output.flat_vector();
        for i in 0..input.len() {
            let name = DuckString::new(&mut { names[i] }).as_str().to_string();
            let json_str = DuckString::new(&mut { jsons[i] }).as_str().to_string();
            // validate JSON
            serde_json::from_str::<serde_json::Value>(&json_str)?;
            // ... catalog write + HashMap update ...
            let msg = format!("Semantic view '{}' registered successfully", name);
            out.insert(i, msg.as_str());
        }
        Ok(())
    }
}

// Registration in entrypoint:
con.register_scalar_function_with_state::<DefineSemanticView>(
    "define_semantic_view",
    &catalog_state,
)?;
```

### One-Parameter Table Function (describe_semantic_view)

```rust
// Source: https://docs.rs/duckdb/latest/duckdb/vtab/trait.VTab.html
impl VTab for DescribeSemanticViewVTab {
    // ...
    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Declare output columns
        bind.add_result_column("name",       LogicalTypeId::Varchar.into());
        bind.add_result_column("base_table", LogicalTypeId::Varchar.into());
        bind.add_result_column("dimensions", LogicalTypeId::Varchar.into()); // JSON text
        bind.add_result_column("metrics",    LogicalTypeId::Varchar.into());
        bind.add_result_column("filters",    LogicalTypeId::Varchar.into());
        bind.add_result_column("joins",      LogicalTypeId::Varchar.into());

        // Read the name parameter
        let name_param = bind.get_parameter(0).to_string();
        // Lookup in catalog state (via extra_info)
        let state = bind.get_extra_info::<CatalogState>();
        let guard = unsafe { (*state).read().unwrap() };
        let json_str = guard.get(&name_param).ok_or_else(|| {
            format!("semantic view '{}' does not exist", name_param)
        })?.clone();
        drop(guard);

        let def: serde_json::Value = serde_json::from_str(&json_str)?;
        Ok(DescribeBindData { name: name_param, def })
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeId::Varchar.into()])
    }
}
```

### Catalog Initialization in Entrypoint

```rust
// Source: https://docs.rs/duckdb/latest/duckdb/struct.Connection.html
fn init_catalog(con: &Connection) -> Result<CatalogState> {
    // Idempotent — IF NOT EXISTS guards make this safe on every load
    con.execute_batch(
        "CREATE SCHEMA IF NOT EXISTS semantic_layer;
         CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
             name    VARCHAR PRIMARY KEY,
             definition VARCHAR   -- JSON text; using VARCHAR for portability
         );"
    )?;

    let mut map = HashMap::new();
    let mut stmt = con.prepare(
        "SELECT name, definition FROM semantic_layer._definitions"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name, def) = row?;
        map.insert(name, def);
    }
    Ok(Arc::new(RwLock::new(map)))
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `duckdb_entrypoint` macro (duckdb_loadable_macros crate) | `duckdb_entrypoint_c_api` macro re-exported from `duckdb` crate directly | duckdb-rs 1.x | No separate `duckdb-loadable-macros` dep needed — already used in this project's lib.rs |
| Raw `libduckdb-sys` FFI for scalar functions | `VScalar` trait + `register_scalar_function` in `vscalar` feature | Added in duckdb-rs mid-2024 (PR #524 added docs) | High-level, safe-ish API; dramatically reduces unsafe code |
| No register_scalar_function in loadable extensions | `register_scalar_function_with_state` on `Connection` | Issue #356 resolved | Scalar functions with shared state are now first-class |

**Deprecated/outdated:**
- `duckdb_loadable_macros` crate as a separate dep: superseded by `duckdb::duckdb_entrypoint_c_api` re-export (already handled in this project)
- Using `JSON` column type in catalog DDL: safe to use if JSON extension is loaded, but `VARCHAR` is more portable for the catalog table definition

---

## Open Questions

1. **Can a VScalar `invoke` method open a second `Connection` to the same file for catalog writes?**
   - What we know: `Connection::open(path)` works for on-disk databases; duckdb supports multiple connections to the same file
   - What's unclear: Whether opening a second connection inside `invoke` (which may be called in a DuckDB execution thread) is safe or causes deadlocks in WAL mode
   - Recommendation: Prefer passing catalog write operations through a channel to the entrypoint connection, OR store the database path in State and open a fresh connection per invocation (LOW confidence on safety). The safest v0.1 approach is to run DDL functions single-threaded and verify with integration tests. If this proves problematic, consider using `libduckdb-sys` `duckdb_appender` directly.

2. **Does `register_table_function_with_extra_info` exist in duckdb-rs 1.4.4?**
   - What we know: The signature was found in Context7 source fetch: `pub fn register_table_function_with_extra_info<T: VTab, E>(&self, name: &str, extra_info: &E) -> Result<()> where E: Clone + Send + Sync + 'static`
   - What's unclear: Whether this was present in 1.4.4 specifically (Context7 source was from 1.3.0 docs.rs mirror)
   - Recommendation: Verify at compile time. If absent, fall back to storing state in a `static OnceLock<CatalogState>` that is set during the entrypoint before function registration, and access it from `get_extra_info` set via a raw pointer to the static.

3. **`LogicalTypeId::Json` availability in duckdb-rs 1.4.4**
   - What we know: DuckDB's JSON type is physically VARCHAR; the official docs.rs LogicalTypeId page did not list a `Json` variant
   - What's unclear: Whether a `Json` variant exists in 1.4.4 bindings (a GitHub discussion mentioned missing variants were added)
   - Recommendation: Use `LogicalTypeId::Varchar` for all JSON-containing columns in `describe_semantic_view` output. This is the locked fallback from CONTEXT.md Claude's Discretion.

---

## Validation Architecture

> `workflow.nyquist_validation` is not set in `.planning/config.json` — section omitted per instructions.

---

## Sources

### Primary (HIGH confidence)

- `/websites/rs_duckdb_duckdb` (Context7) — VScalar trait, VTab trait, BindInfo, InitInfo, DataChunkHandle, ScalarFunctionSignature, LogicalTypeId, Connection methods (execute_batch, register_scalar_function, register_table_function, register_table_function_with_extra_info, prepare, query_map)
- `/duckdb/duckdb-rs` (Context7) — Feature flags (vscalar, vtab, loadable-extension), serde_json integration, Config API
- https://docs.rs/duckdb/latest/duckdb/vscalar/struct.ScalarFunctionSignature.html — `exact()` and `variadic()` constructors
- https://docs.rs/duckdb/latest/duckdb/core/enum.LogicalTypeId.html — confirmed `Varchar` variant, no `Json` variant found
- https://docs.rs/duckdb/latest/duckdb/struct.Connection.html — `register_scalar_function`, `register_scalar_function_with_state`, `execute_batch` signatures
- https://raw.githubusercontent.com/duckdb/duckdb-rs/main/crates/duckdb/src/vscalar/mod.rs — EchoScalar implementation, VScalar trait definition, VARCHAR input/output pattern

### Secondary (MEDIUM confidence)

- https://github.com/duckdb/duckdb-rs/issues/356 — confirmed `register_scalar_function` is in `vscalar` feature; docs were added in PR #524
- https://duckdb.org/docs/stable/data/json/json_type — confirmed JSON type is physically VARCHAR in DuckDB

### Tertiary (LOW confidence)

- WebSearch results on global state patterns (OnceLock, Arc, Mutex) — standard Rust patterns, not duckdb-rs specific
- Second Connection for catalog writes inside VScalar invoke — pattern inferred, not verified against duckdb concurrency model

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — core APIs verified via Context7 (docs.rs) and official duckdb-rs source
- Architecture: HIGH — VScalar + VTab patterns directly from duckdb-rs source/tests; catalog init from Connection API docs
- Pitfalls: MEDIUM — JSON type pitfall and Connection Send bound are verified; second-connection concurrency risk is inferred (LOW)

**Research date:** 2026-02-24
**Valid until:** 2026-03-24 (stable library — duckdb-rs 1.4.4 is version-pinned; no API churn expected within 30 days)
