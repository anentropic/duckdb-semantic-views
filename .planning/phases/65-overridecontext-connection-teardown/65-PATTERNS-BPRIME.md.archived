# Phase 65: OverrideContext Connection Teardown — Pattern Map

**Mapped:** 2026-05-21
**Files analyzed:** 5 modify targets + 2 new files + 14 read-side call sites
**Analogs found:** 5 / 5 modify targets (all have strong existing analogs in-repo)

---

## File Classification

| New / Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---------------------|------|-----------|----------------|---------------|
| **NEW** `src/conn_guard.rs` | utility (RAII guard over raw C handle) | request-response (synchronous open/use/close) | `src/catalog.rs::PreparedStmt` + `src/catalog.rs::QueryResult` + `src/query/table_function.rs::LogicalTypeOwned` | **exact** — same shape: `struct {ptr}` + `unsafe fn open(...) -> Result<Self, String>` + `impl Drop` calling `duckdb_destroy_*` |
| **MODIFY** `src/parse.rs` (`OverrideContext`, `sv_make_override_context`, `rewrite_*`) | service (parser_override path) | event-driven (called by C++ shim per parse) | `src/parse.rs` itself (existing OverrideContext + its `Drop` impl with intentional leak — being replaced) | **self-refactor** |
| **MODIFY** `src/lib.rs::init_extension` | config / wiring (extension entry) | one-shot init | `src/lib.rs::init_extension` (current shape) | **self-refactor** |
| **MODIFY** `src/catalog.rs::CatalogReader` | model (read-side handle) | CRUD (catalog lookups) | self — methods are uniform; either swap `conn` → `db_handle` field and connect per call, OR carry both `db_handle` + take `&ConnGuard` borrow | **self-refactor** |
| **MODIFY** `src/query/table_function.rs::QueryState` + `bind` + `func` | service (query execution wiring) | streaming (result chunks) | self — `QueryState` field swap mirrors `CatalogReader` field swap; `bind`/`func` need a `ConnGuard` stored in `SemanticViewBindData` (lifecycle = query) | **self-refactor** |
| **MODIFY** ~14 read-side table-function `bind`s (`src/ddl/list.rs`, `describe.rs`, `show_*.rs`, `get_ddl.rs`, `read_yaml.rs`) | controller (table-function bind callback) | CRUD (one read per bind) | `src/ddl/list.rs::bind` (the canonical 2-liner: `bind.get_extra_info::<CatalogReader>()` → `reader.list_all()`) | **exact** — all 14 follow the same shape; fix is mechanical |
| **MODIFY** Two scalar functions (`get_ddl.rs::GetDdlScalar`, `read_yaml.rs::ReadYamlFromSemanticViewScalar`) | controller (scalar `invoke`) | CRUD (per-row lookup) | `src/ddl/get_ddl.rs::invoke` (`type State = CatalogReader;` + `state.lookup(name)`) | **exact** |
| **NEW** `test/integration/test_readonly_load.py::test_in_process_*` + `_connect_with_watchdog` helper | test (Python integration with watchdog) | request-response with timeout | `test/integration/test_concurrent_ddl.py::worker` (daemon thread + `Event` gate + `t.join(timeout=30)` + `t.is_alive()` failure path) | **role-match** (concurrent-ddl is goroutine gate; readonly is single-thread watchdog — same primitives) |

---

## Pattern Assignments

### NEW: `src/conn_guard.rs` (utility, RAII over `duckdb_connection`)

**Analog:** `src/catalog.rs::PreparedStmt` (`src/catalog.rs:176-206`) — identical shape, different C handle.

**Imports pattern** (`src/catalog.rs:85-88`):
```rust
use std::ffi::{CStr, CString};
use std::os::raw::c_void;

use libduckdb_sys as ffi;
```
Use the same `libduckdb_sys as ffi` alias.

**Core RAII pattern** (`src/catalog.rs:176-206` — copy almost verbatim, substituting `duckdb_connect`/`duckdb_disconnect`):
```rust
/// RAII guard for `duckdb_prepared_statement`. Drops the statement on
/// scope exit even when the body short-circuits via `?`. Pre-v0.8.0
/// every error path repeated `duckdb_destroy_prepare` by hand.
struct PreparedStmt {
    ptr: ffi::duckdb_prepared_statement,
}

impl PreparedStmt {
    unsafe fn prepare(conn: ffi::duckdb_connection, sql: &CStr) -> Result<Self, String> {
        let mut ptr: ffi::duckdb_prepared_statement = std::ptr::null_mut();
        let rc = ffi::duckdb_prepare(conn, sql.as_ptr(), &mut ptr);
        if rc != ffi::DuckDBSuccess {
            let err = ffi::duckdb_prepare_error(ptr);
            let msg = if err.is_null() {
                "unknown prepare error".to_string()
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            ffi::duckdb_destroy_prepare(&mut ptr);
            return Err(msg);
        }
        Ok(Self { ptr })
    }

    fn raw(&self) -> ffi::duckdb_prepared_statement {
        self.ptr
    }
}

impl Drop for PreparedStmt {
    fn drop(&mut self) {
        unsafe { ffi::duckdb_destroy_prepare(&mut self.ptr) };
    }
}
```

**Adapt to `ConnGuard`:**
```rust
pub(crate) struct ConnGuard {
    conn: ffi::duckdb_connection,
}

impl ConnGuard {
    /// SAFETY: `db` must be a valid `duckdb_database` handle that remains
    /// live for the duration of this guard.
    pub(crate) unsafe fn open(db: ffi::duckdb_database) -> Result<Self, String> {
        let mut conn: ffi::duckdb_connection = std::ptr::null_mut();
        let rc = ffi::duckdb_connect(db, &mut conn);
        if rc != ffi::DuckDBSuccess {
            return Err("duckdb_connect failed".to_string());
        }
        Ok(Self { conn })
    }

    pub(crate) fn raw(&self) -> ffi::duckdb_connection { self.conn }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        if !self.conn.is_null() {
            unsafe { ffi::duckdb_disconnect(&mut self.conn) };
        }
    }
}
```

**Send/Sync pattern** (copy from `src/catalog.rs:108-112`):
```rust
// SAFETY: `duckdb_connection` is an opaque pointer managed by DuckDB.
// The connection itself owns its synchronisation; reads from multiple
// threads via the same handle are serialised by DuckDB internally.
unsafe impl Send for CatalogReader {}
unsafe impl Sync for CatalogReader {}
```
For `ConnGuard`, `Send` is acceptable but `Sync` is not appropriate — a guard is meant to be owned by one scope. Stay with `Send` only (or omit both — let the caller wrap in `Mutex` if needed; `QueryState` precedent at `src/query/table_function.rs:42-43` only needs `Send` for the `extra_info` path).

**Module-feature gate** (copy from `src/catalog.rs:83-88`):
```rust
#[cfg(feature = "extension")]
mod inner {
    use libduckdb_sys as ffi;
    // ...
}

#[cfg(feature = "extension")]
pub use inner::ConnGuard;
```

**Unit test pattern** (RESEARCH §7.2 B14 — Drop closes exactly once). Analog in same file: `src/catalog.rs::tests::two_statement_guard_then_dml_smoke` (`src/catalog.rs:361-405`). Same `#[cfg(not(feature = "extension"))]` gate for tests that touch a real connection vs. `#[cfg(feature = "extension")]` for tests that only verify Drop signatures via mocked function pointers (cannot use C-API in the bundled-feature unit tests). Proptest harness lives outside this file (use existing proptest patterns in `src/util.rs` or `src/expand/`).

---

### MODIFY: `src/parse.rs` — `OverrideContext` field swap

**Analog (self):** current `OverrideContext` at `src/parse.rs:46-71`. Swap `catalog: crate::catalog::CatalogReader` (which carries the leaked `conn`) for `db_handle: ffi::duckdb_database` + `is_file_backed: bool`.

**Current shape** (`src/parse.rs:46-71` — being replaced):
```rust
#[cfg(feature = "extension")]
pub struct OverrideContext {
    pub catalog: crate::catalog::CatalogReader,
    pub is_file_backed: bool,
}

#[cfg(feature = "extension")]
impl Drop for OverrideContext {
    fn drop(&mut self) {
        // Phase 62 Q2 — INTENTIONAL LEAK of self.catalog.conn (the duckdb_connection).
        //
        // ~SemanticViewsParserInfo (and therefore Drop for OverrideContext) fires
        // during ~DBConfig, AFTER ~DatabaseInstance has already reset
        // connection_manager (duckdb.cpp:276819). Calling duckdb_disconnect here
        // would invoke ~Connection() → ConnectionManager::RemoveConnection() on
        // the destroyed manager — use-after-free.
        // ...
    }
}
```

**Target shape:**
```rust
#[cfg(feature = "extension")]
pub struct OverrideContext {
    /// Database handle (opaque pointer). Does NOT increment refcount on the
    /// shared_ptr<DatabaseInstance> — only Connection objects do. Safe to
    /// store at DBConfig scope. See Phase 65 RESEARCH §3.3.
    pub db_handle: libduckdb_sys::duckdb_database,
    pub is_file_backed: bool,
}
// No custom Drop needed — no owned resources beyond the Box itself.
```

**FFI entrypoint update** (`src/parse.rs:2461-2484` — `sv_make_override_context`): change the C signature to accept `duckdb_database` instead of `duckdb_connection`. The C++ shim (`cpp/src/shim.cpp:354-407`) `sv_register_parser_hooks` already receives `db_handle` as its first arg (`src/lib.rs:416`), so the plumbing change is minimal — pass `db_handle` through instead of `catalog_conn`.

**Catalog read sites in parse.rs** (`src/parse.rs:1715-1768` rewrite_to_native_sql, `:1770-1815` rewrite_drop_or_alter, `:1820-1935` rewrite_create, `:1989-2035` enrichment using `ctx.catalog.raw()`): each call site that today does `ctx.catalog.exists(...)` / `ctx.catalog.raw()` must instead:

```rust
// Open a per-call connection via the guard. Drop closes it before return.
let guard = unsafe { ConnGuard::open(ctx.db_handle) }
    .map_err(|e| ParseError { ... })?;
let catalog = CatalogReader::new(guard.raw(), /*catalog_table_present=*/ true);
let exists = catalog.exists(&name).map_err(...)?;
// ... use guard.raw() for further raw-API reads ...
// guard dropped at end of scope → duckdb_disconnect fires
```

**Use site analog for raw-API reads** (`src/parse.rs:1989-2035`):
```rust
unsafe { crate::query::table_function::execute_sql_raw(ctx.catalog.raw(), &read_sql) }
```
Becomes:
```rust
unsafe { crate::query::table_function::execute_sql_raw(guard.raw(), &read_sql) }
```
Same call shape; just substitute the source of the `duckdb_connection`.

---

### MODIFY: `src/lib.rs::init_extension` — drop H1 + H2 ownership

**Current shape** (`src/lib.rs:378-408`, lines 493-508 for H2):
```rust
// H1 — catalog_conn
let mut catalog_conn: ffi::duckdb_connection = ptr::null_mut();
let rc = unsafe { ffi::duckdb_connect(db_handle, &mut catalog_conn) };
if rc != ffi::DuckDBSuccess {
    return Err("Failed to create catalog connection".into());
}
let catalog_reader =
    crate::catalog::CatalogReader::new(catalog_conn, catalog_table_present);
// ...
if !unsafe { sv_register_parser_hooks(db_handle, catalog_conn, is_file_backed) } { ... }
// register_table_function_with_extra_info(..., &catalog_reader) for 14 sites

// H2 — query_conn
let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
// ...
let query_state = QueryState { catalog: catalog_reader, conn: query_conn };
con.register_table_function_with_extra_info::<SemanticViewVTab, _>("semantic_view", &query_state)?;
```

**Target shape:** remove both `duckdb_connect` calls. Instead:

```rust
// No long-lived connections. Build per-call handles instead.
let catalog_handle = CatalogHandle {
    db: db_handle,
    catalog_table_present,  // captured at init for the read-only fast path
};

// sv_register_parser_hooks takes db_handle (not catalog_conn). FFI signature
// changes in cpp/src/shim.cpp and src/parse.rs::sv_make_override_context.
if !unsafe { sv_register_parser_hooks(db_handle, is_file_backed) } { ... }

// Each register_table_function_with_extra_info passes &catalog_handle
// instead of &catalog_reader. The bind callbacks open/close their own
// connection via ConnGuard.
con.register_table_function_with_extra_info::<ListSemanticViewsVTab, _>(
    "list_semantic_views",
    &catalog_handle,
)?;
// ... 13 more identical sites ...

let query_state = QueryState { db_handle, catalog_table_present };
con.register_table_function_with_extra_info::<SemanticViewVTab, _>("semantic_view", &query_state)?;
```

**Note on `CatalogHandle` vs reusing `CatalogReader`:** the planner has two viable shapes (RESEARCH §7.5):
- (a) introduce a new lightweight `CatalogHandle { db, catalog_table_present }` type carrying only `duckdb_database`; the 14 read-side bind callbacks each open a `ConnGuard` and construct a transient `CatalogReader::new(guard.raw(), catalog_table_present)`.
- (b) refactor `CatalogReader` itself to store `db` instead of `conn`, and have its `lookup`/`list_all`/`list_names` methods internally open+close a `ConnGuard` per call.

Both work. (a) keeps `CatalogReader`'s current Send/Sync/Copy semantics and is closer to the existing pattern; (b) is more transparent to call sites. Planner chooses; analog code below applies either way.

**FFI signature change to `sv_register_parser_hooks`** (`cpp/src/shim.cpp:354-407` not read in this pattern pass — planner-side excerpt): drop the `catalog_conn` parameter, keep `db_handle` + `is_file_backed`. Then `sv_make_override_context` takes `(db_handle, is_file_backed)` instead of `(conn, is_file_backed)`. This is a binary-ABI break on the internal C++ shim — but the shim is in-tree, so just update both sides in lockstep.

---

### MODIFY: `src/catalog.rs::CatalogReader` — field swap (option b only)

If the planner chooses shape (b) above, the field swap is:

**Current** (`src/catalog.rs:97-124`):
```rust
#[derive(Clone, Copy)]
pub struct CatalogReader {
    conn: ffi::duckdb_connection,
    catalog_table_present: bool,
}

impl CatalogReader {
    pub fn new(conn: ffi::duckdb_connection, catalog_table_present: bool) -> Self {
        Self { conn, catalog_table_present }
    }
    pub fn raw(&self) -> ffi::duckdb_connection { self.conn }

    pub fn lookup(&self, name: &str) -> Result<Option<String>, String> {
        if !self.catalog_table_present { return Ok(None); }
        unsafe { prepared_lookup(self.conn, name) }
    }
    // list_all, list_names similar
}
```

**Target** (shape b):
```rust
#[derive(Clone, Copy)]
pub struct CatalogReader {
    db: ffi::duckdb_database,
    catalog_table_present: bool,
}

impl CatalogReader {
    pub fn new(db: ffi::duckdb_database, catalog_table_present: bool) -> Self {
        Self { db, catalog_table_present }
    }

    pub fn lookup(&self, name: &str) -> Result<Option<String>, String> {
        if !self.catalog_table_present { return Ok(None); }
        let guard = unsafe { crate::conn_guard::ConnGuard::open(self.db)? };
        unsafe { prepared_lookup(guard.raw(), name) }
        // guard dropped → duckdb_disconnect
    }
    // list_all, list_names: same per-call open
}
```

The `prepared_lookup`/`execute_list_all`/`execute_list_names` helpers (`src/catalog.rs:255-332`) remain unchanged — they already take `conn: ffi::duckdb_connection` as a parameter, perfectly compatible with `guard.raw()`.

**Removed:** `CatalogReader::raw()` accessor. Any caller that previously held the raw connection (e.g., `src/parse.rs:1929` `ctx.catalog.raw()`, `src/parse.rs:2035` same) must now own its own `ConnGuard` and pass `guard.raw()`.

---

### MODIFY: `src/query/table_function.rs::QueryState` + `bind` + `func`

**Current shape** (`src/query/table_function.rs:32-43`):
```rust
#[derive(Clone)]
pub struct QueryState {
    pub catalog: CatalogReader,
    pub conn: ffi::duckdb_connection,
}
unsafe impl Send for QueryState {}
unsafe impl Sync for QueryState {}
```

**Target shape:**
```rust
#[derive(Clone, Copy)]
pub struct QueryState {
    pub db_handle: ffi::duckdb_database,
    pub catalog_table_present: bool,
}
unsafe impl Send for QueryState {}
unsafe impl Sync for QueryState {}
```

**`SemanticViewBindData` must own a `ConnGuard`** for the query-lifetime connection that `func()` will use to call `execute_sql_raw`. The guard lives in bind data and drops when DuckDB destroys the bind. Reference site (`src/query/table_function.rs:516-525`):
```rust
let state_ptr = bind.get_extra_info::<QueryState>();
let state = unsafe { &*state_ptr };
let json_str = match state
    .catalog
    .lookup(&view_name)
    .map_err(Box::<dyn std::error::Error>::from)?
{ ... };
```

Target shape inside bind:
```rust
let state = unsafe { &*bind.get_extra_info::<QueryState>() };
let guard = unsafe { ConnGuard::open(state.db_handle) }
    .map_err(...)?;
let catalog = CatalogReader::new(guard.raw(), state.catalog_table_present);
let json_str = catalog.lookup(&view_name).map_err(...)?;
// ... build SemanticViewBindData and STORE `guard` inside it for func() ...
```

**`func()` site** (`src/query/table_function.rs:763-773`):
```rust
if guard.is_none() {
    let state = unsafe { &*func.get_extra_info::<QueryState>() };
    let mut result = unsafe {
        execute_sql_raw(state.conn, &bind_data.execution_sql).map_err(...)?
    };
```
Becomes:
```rust
if guard.is_none() {
    let mut result = unsafe {
        execute_sql_raw(bind_data.conn_guard.raw(), &bind_data.execution_sql)
            .map_err(...)?
    };
```
(`bind_data` already lives across `bind`→`func`→destroy; adding a `conn_guard: ConnGuard` field to `SemanticViewBindData` keeps the connection alive for exactly the query's lifetime and closes it deterministically when DuckDB drops bind data.)

The `StreamingState`/`Mutex<Option<>>` pattern (`src/query/table_function.rs:71-97`) is already correctly query-scoped (H9 in RESEARCH §5 audit — fine). No change needed there.

**`explain_semantic_view`** (`src/query/explain.rs`) follows the same shape — single `bind`/`func` pair using `QueryState`; same refactor.

---

### MODIFY: 14 read-side table-function `bind`s — mechanical extra-info swap

**Analog (canonical):** `src/ddl/list.rs:73-77`:
```rust
let state_ptr = bind.get_extra_info::<CatalogReader>();
let reader = unsafe { *state_ptr };
let entries = reader
    .list_all()
    .map_err(Box::<dyn std::error::Error>::from)?;
```

If planner picks shape (a) — `CatalogHandle` extra_info:
```rust
let handle = unsafe { *bind.get_extra_info::<CatalogHandle>() };
let guard = unsafe { ConnGuard::open(handle.db) }
    .map_err(Box::<dyn std::error::Error>::from)?;
let reader = CatalogReader::new(guard.raw(), handle.catalog_table_present);
let entries = reader.list_all().map_err(...)?;
// guard dropped at bind-callback return
```

If planner picks shape (b) — `CatalogReader` refactored to carry `db`:
```rust
// Unchanged from today's code! CatalogReader::list_all internally
// opens+closes a ConnGuard.
let reader = unsafe { *bind.get_extra_info::<CatalogReader>() };
let entries = reader.list_all().map_err(...)?;
```

**Shape (b) is mechanically simpler at the 14 call sites** but harder inside `CatalogReader` (each method opens+closes; if the bind needs multiple reads it pays repeated overhead). Shape (a) is slightly more code at each site but each bind opens exactly one connection no matter how many reads it does. RESEARCH §6.5 cost analysis says per-call overhead is ~µs either way — both are fine. Pick whichever is more readable.

**14 affected sites** (each is a single `bind.get_extra_info::<CatalogReader>()` call):
| File | Lines |
|------|-------|
| `src/ddl/list.rs` | 73, 205 |
| `src/ddl/describe.rs` | 524 |
| `src/ddl/show_columns.rs` | 146 |
| `src/ddl/show_dims.rs` | 154, 203 |
| `src/ddl/show_dims_for_metric.rs` | 183 |
| `src/ddl/show_metrics.rs` | 156, 205 |
| `src/ddl/show_facts.rs` | 154, 203 |
| `src/ddl/show_materializations.rs` | 141, 190 |

---

### MODIFY: 2 scalar functions — `VScalar::State`

**Analog:** `src/ddl/get_ddl.rs:17-50`:
```rust
impl VScalar for GetDdlScalar {
    type State = CatalogReader;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            // ...
            let json = state
                .lookup(&name)
                .map_err(Box::<dyn std::error::Error>::from)?
                .ok_or_else(|| format!("semantic view '{}' does not exist", name))?;
            // ...
        }))
    }
}
```

Shape (a): `type State = CatalogHandle;` + open ConnGuard inside `invoke`. Shape (b): `type State = CatalogReader;` unchanged at this layer — `lookup` opens internally.

Same shape applies to `src/ddl/read_yaml.rs::ReadYamlFromSemanticViewScalar` (line 28: `type State = CatalogReader;`).

---

### NEW: `test/integration/test_readonly_load.py` — watchdog tests

**Analog:** `test/integration/test_concurrent_ddl.py:79-120` — daemon thread + `threading.Event` gate + `join(timeout=N)` + post-join `is_alive()` failure check.

**Watchdog skeleton** (synthesized from RESEARCH §7.3 + concurrent_ddl pattern):
```python
import threading, time, gc, tempfile
from pathlib import Path
import duckdb


def _connect_with_watchdog(path, watchdog_seconds=5, **kwargs):
    """connect-with-watchdog: fails fast instead of busy-spinning forever.

    On the v0.9.0 baseline, duckdb.connect(path, read_only=True) busy-spins
    in DBInstanceCache::GetInstanceInternal forever (see Phase 65 RESEARCH §2).
    Python cannot interrupt a busy-spinning C++ thread, so the daemon thread
    leaks for the process lifetime when the test fails. Acceptable for a
    fail-once regression test; run these tests LAST in the file under
    pytest-timeout to keep CI hygiene.
    """
    result = {"conn": None, "exc": None}

    def _do():
        try:
            result["conn"] = duckdb.connect(path, **kwargs)
        except BaseException as e:
            result["exc"] = e

    t = threading.Thread(target=_do, daemon=True)
    start = time.monotonic()
    t.start()
    t.join(timeout=watchdog_seconds)
    elapsed = time.monotonic() - start
    if t.is_alive():
        raise TimeoutError(
            f"duckdb.connect({path!r}, **{kwargs!r}) did not return within "
            f"{watchdog_seconds}s — likely the in-process RW→RO busy-spin "
            f"(Phase 65 regression)"
        )
    if result["exc"]:
        raise result["exc"]
    return result["conn"], elapsed
```

**Per-test pattern** (B1, B2, B3, B4):
```python
def test_in_process_bootstrap_then_readonly_fresh():
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "fresh.duckdb")
        w = open_writable(db)  # existing helper, src line 59
        w.execute("CREATE TABLE t (i INT)")
        w.execute(
            "CREATE SEMANTIC VIEW v AS "
            "  TABLES (t1 AS t PRIMARY KEY (i)) "
            "  DIMENSIONS (t1.i AS t1.i) "
            "  METRICS (t1.c AS COUNT(*))"
        )
        w.close()
        del w
        gc.collect()
        ro, elapsed = _connect_with_watchdog(db, watchdog_seconds=5, read_only=True,
                                              config={...same as open_readonly...})
        try:
            assert elapsed < 5.0
            ro.execute("LOAD semantic_views")
            names = [r[0] for r in ro.execute("SELECT name FROM list_semantic_views()").fetchall()]
            assert names == ["v"]
        finally:
            ro.close()
```

**B11 (repeated LOAD+close loop)** — analog: existing `test_multi_db_isolation.py` style. Loop opens 50 file-backed temp DBs sequentially, asserts each completes within 5s and process RSS stays bounded (use `psutil` if available, else just rely on no-busy-spin completion).

**Error / pass / fail reporting convention** (`test/integration/test_readonly_load.py:136-150` — existing `run_test` shim): all new tests must integrate with the existing `run_test(name, fn)` runner so the file remains executable as a script with exit code 0/1. Don't introduce pytest framework — the file is a script per its `# /// script` PEP 723 header (line 2-5).

**Existing subprocess tests stay** (LIFE-03 + B5): the new in-process tests are **added alongside** `bootstrap_in_subprocess` — do not delete it. Subprocess tests still validate the deployment-style smoke (real users have separate bootstrap+RO processes).

---

## Shared Patterns

### RAII guard convention (apply to all C-API handle ownership)

**Source:** `src/catalog.rs::PreparedStmt` (lines 176-206), `src/catalog.rs::QueryResult` (lines 208-230), `src/query/table_function.rs::LogicalTypeOwned` (lines 180-191), `src/query/table_function.rs::StreamingState::Drop` (lines 83-87).

**Apply to:** the new `src/conn_guard.rs::ConnGuard`. Same shape exactly:
1. Tuple/named struct holding a raw `duckdb_*` handle.
2. `unsafe fn open/prepare/...(args) -> Result<Self, String>` constructor that checks `rc != DuckDBSuccess` and surfaces the error string (using `duckdb_*_error` when available).
3. `fn raw(&self) -> ffi::duckdb_*` accessor returning the handle by value (not by reference) — DuckDB handles are pointer-sized, cheap to copy.
4. `impl Drop` calling the matching `duckdb_destroy_*` / `duckdb_disconnect` function.
5. SAFETY comment explaining the handle is opaque and DuckDB owns synchronisation.

**Excerpt** (`src/catalog.rs:202-206`):
```rust
impl Drop for PreparedStmt {
    fn drop(&mut self) {
        unsafe { ffi::duckdb_destroy_prepare(&mut self.ptr) };
    }
}
```

### FFI signature pattern for shim helpers

**Source:** `src/parse.rs::sv_make_override_context` (lines 2461-2484).

**Apply to:** modified `sv_make_override_context` + new `sv_register_parser_hooks` signature.

```rust
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_make_override_context(
    /* updated args */
    db: libduckdb_sys::duckdb_database,
    is_file_backed: bool,
) -> *mut std::ffi::c_void {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = Box::new(OverrideContext { db_handle: db, is_file_backed });
        Box::into_raw(ctx) as *mut std::ffi::c_void
    }));
    result.unwrap_or(std::ptr::null_mut())
}
```

Note the `std::panic::catch_unwind(AssertUnwindSafe(...))` wrapper — mandatory at every Rust↔C boundary. Same pattern at `sv_drop_override_context` (line 2500). Apply to any new FFI surface.

### `#[cfg(feature = "extension")]` gating

**Source:** every C-API consumer in `src/` — e.g. `src/catalog.rs:83-88, 335-336`, `src/parse.rs:46-71`, `src/lib.rs:340-517`.

**Apply to:** `src/conn_guard.rs` (entire module behind `#[cfg(feature = "extension")]`, re-exported at module root) and all new FFI entrypoints. `cargo test` (default features) must continue to compile without the extension feature.

### Send + Sync impls for `extra_info` types

**Source:** `src/query/table_function.rs:42-43`, `src/catalog.rs:108-112`.

**Apply to:** any new `CatalogHandle` / refactored `QueryState`. Required because DuckDB-rs's `register_table_function_with_extra_info` requires `Send + Sync`. Comment must document why the raw `duckdb_database` pointer is safe to share across threads (DuckDB synchronizes internally).

### Test integration with existing script runner

**Source:** `test/integration/test_readonly_load.py:136-150` — `run_test(name, fn) -> bool` shim wrapping pass/fail/error reporting; called from a `main()` that sums booleans and returns 0/1.

**Apply to:** all new B1-B4 + B11 tests. Do not introduce pytest, unittest discovery, or new frameworks — the file's `# /// script` PEP 723 header at lines 2-5 means it runs under `uv run` as a standalone script.

---

## No Analog Found

None. Every modify target and new file has a strong in-repo analog. The closest stretch is the test-side watchdog (RESEARCH §7.3 specifies a daemon-thread pattern; concurrent_ddl uses daemon threads + `join(timeout=...)` for a similar fail-fast contract). All FFI/RAII work mirrors the Phase 61 `PreparedStmt`/`QueryResult` introduction patterns.

---

## Metadata

**Analog search scope:**
- `src/` — every `.rs` file touching `duckdb_connect`, `duckdb_disconnect`, `duckdb_database`, `duckdb_connection`, `OverrideContext`, `CatalogReader`, `QueryState`, `register_table_function_with_extra_info`, `VScalar::State`.
- `src/ddl/` — all 14 read-side table-function bind callbacks.
- `test/integration/` — existing watchdog / timeout / daemon-thread patterns.
- `cpp/src/shim.cpp` — referenced via grep through `src/parse.rs` only (not opened — shim changes accompany Rust FFI signature changes in lockstep, planner manages).

**Files scanned:**
- `src/lib.rs` (init_extension + C_STRUCT entrypoint)
- `src/parse.rs` (OverrideContext, sv_make/drop_override_context, rewrite_create/drop_or_alter/to_native_sql, enrichment paths)
- `src/catalog.rs` (CatalogReader + PreparedStmt + QueryResult RAII analogs)
- `src/query/table_function.rs` (QueryState, SemanticViewBindData, bind/func, execute_sql_raw, LogicalTypeOwned, StreamingState)
- `src/query/explain.rs` (QueryState consumer — confirmed same shape)
- `src/ddl/list.rs` (canonical 14-site analog) + grep across `src/ddl/*.rs` for `get_extra_info::<CatalogReader>` (all 14 sites enumerated above)
- `src/ddl/get_ddl.rs`, `src/ddl/read_yaml.rs` (2 scalar functions)
- `test/integration/test_concurrent_ddl.py` (daemon-thread / watchdog analog)
- `test/integration/test_readonly_load.py` (target file — existing subprocess helpers, `run_test` shim, `open_writable` / `open_readonly`)

**Pattern extraction date:** 2026-05-21
