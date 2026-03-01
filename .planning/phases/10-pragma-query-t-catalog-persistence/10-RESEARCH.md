# Phase 10: pragma_query_t Catalog Persistence - Research

**Researched:** 2026-03-01
**Domain:** DuckDB C++ extension internals — `pragma_query_t`, `ExtensionLoader`, `InternalAppender`, cross-language FFI boundary
**Confidence:** MEDIUM — C++ API internals verified against vendored headers (HIGH); deadlock resolution for scalar invoke path is MEDIUM (open design question)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Replace sidecar file with DuckDB-native table persistence via `pragma_query_t` callback registered in the C++ shim.
- `init_catalog` performs one-time migration: read sidecar → insert into table → delete sidecar file. Runs on first load; skips silently if no sidecar exists.
- Write failure in `define_semantic_view()` returns an error; no silent degradation to in-memory-only state.
- In-memory databases (`:memory:`) skip the pragma write path; HashMap is the sole source of truth for the session.
- ALL sidecar code is deleted: `sidecar_path()`, `read_sidecar()`, `write_sidecar()`, all sidecar tests.
- `DefineState` and `DropState` lose the `db_path` field.
- The C++ shim registers `pragma_query_t` at load time. Rust calls `semantic_views_pragma_define(name_ptr, json_ptr)` and `semantic_views_pragma_drop(name_ptr)` FFI functions from invoke.
- The shim executes `INSERT OR REPLACE INTO semantic_layer._definitions` or `DELETE FROM semantic_layer._definitions` via the `pragma_query_t` mechanism.
- Transaction semantics (PERSIST-02): use the approach that keeps HashMap consistent with committed table state. If pragma write fails or transaction is rolled back, HashMap should reflect the pre-define state.

### Claude's Discretion
- Exact C++ `pragma_query_t` API call signatures
- Whether `INSERT OR REPLACE` or `DELETE + INSERT` for idempotency in migration
- Whether `semantic_views_pragma_define` / `_drop` FFI functions are synchronous or return an error code (prefer error code + propagate to Rust `Result`)
- Naming of the internal shim PRAGMA (not user-visible)
- HashMap update ordering (Option B from CONTEXT.md: only update HashMap after pragma write succeeds)

### Deferred Ideas (OUT OF SCOPE)
- None — discussion stayed within phase scope.
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PERSIST-01 | Semantic view definitions persist via DuckDB native tables (`pragma_query_t`) — no sidecar `.semantic_views` file | `pragma_query_t` returning INSERT SQL writes to `semantic_layer._definitions` table; table already persists across restarts via DuckDB WAL |
| PERSIST-02 | A `ROLLBACK` reverts a definition change in both persistent storage and in-memory catalog | `pragma_query_t` returned SQL runs inside the calling transaction; if user calls `BEGIN; PRAGMA define_semantic_view_internal(...); ROLLBACK;` the INSERT is rolled back. HashMap must also be rolled back — use Option B (don't update HashMap until after write succeeds). |
| PERSIST-03 | Sidecar file mechanism removed from codebase | Delete `sidecar_path()`, `read_sidecar()`, `write_sidecar()` from `catalog.rs`; remove all sidecar imports and tests |
</phase_requirements>

---

## Summary

Phase 10 replaces the sidecar file persistence mechanism with DuckDB-native table writes using `pragma_query_t`. The sidecar existed because scalar function `invoke` cannot execute DuckDB SQL (deadlock). Phase 10 resolves this by registering a C++ PRAGMA callback that returns an INSERT SQL string — DuckDB executes that string as the "substitute query" for the PRAGMA statement, which participates in the current transaction.

The `pragma_query_t` type is confirmed in vendored headers: `string (*)(ClientContext&, const FunctionParameters&)`. Registration uses `ExtensionLoader::RegisterFunction(PragmaFunction)`, constructing `ExtensionLoader` from `DatabaseInstance&` extracted from the `duckdb_database` C handle passed to the shim. All relevant C++ types are available in the `duckdb_capi/` vendored headers.

The critical open question (flagged in STATE.md blockers) is how `define_semantic_view()` scalar invoke calls `semantic_views_pragma_define` without deadlock. Two viable paths exist and are documented below with tradeoffs. The recommended path (Option B: separate connection for scalar invoke, PRAGMA path for user-visible ROLLBACK testing) aligns with success criterion 2 of the phase.

**Primary recommendation:** Register `pragma_query_t` for user-visible PRAGMA path (PERSIST-02). For scalar function invoke path, use a pre-stored `duckdb_connection` (the existing `query_conn` from lib.rs init) to execute the INSERT — this avoids deadlock because it's a separate connection from the one currently executing the scalar function.

---

## Standard Stack

### Core

| Component | Version | Purpose | Confirmation |
|-----------|---------|---------|--------------|
| `duckdb/function/pragma_function.hpp` | vendored (matches duckdb-rs =1.4.4) | `pragma_query_t` type, `PragmaFunction` class | Verified in `duckdb_capi/duckdb/function/pragma_function.hpp` |
| `duckdb/main/extension/extension_loader.hpp` | vendored | `ExtensionLoader::RegisterFunction(PragmaFunction)` | Verified in `duckdb_capi/duckdb/main/extension/extension_loader.hpp` |
| `duckdb/main/capi/capi_internal.hpp` | vendored | `DatabaseWrapper` struct for casting `duckdb_database` → `DatabaseInstance&` | Verified in `duckdb_capi/duckdb/main/capi/capi_internal.hpp` |
| `duckdb/main/appender.hpp` | vendored | `InternalAppender` — alternative write path for pragma_function_t | Verified in `duckdb_capi/duckdb/main/appender.hpp` |
| `libduckdb-sys` via Rust | `=1.4.4` | `ffi::duckdb_query`, `ffi::duckdb_connection` for scalar invoke write path | Confirmed in `Cargo.toml` and `src/query/table_function.rs` |

### Supporting

| Component | Version | Purpose | When to Use |
|-----------|---------|---------|-------------|
| `PragmaFunction::PragmaStatement` | same | No-arg pragma registration | If the internal pragma takes no parameters (not applicable here — name + json needed) |
| `PragmaFunction::PragmaCall` with `pragma_function_t` | same | Direct execution (not SQL substitution) | Alternative to `pragma_query_t` for in-process writes via `InternalAppender` |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `pragma_query_t` returning INSERT SQL | `pragma_function_t` with `InternalAppender` | Both are transaction-aware. `pragma_query_t` returns a SQL string executed by DuckDB (simple, composable). `pragma_function_t` with InternalAppender writes rows directly via C++ catalog API (faster, no SQL parse overhead). Either works for PERSIST-02. |
| Separate `query_conn` for scalar invoke write | `InternalAppender` with `ClientContext` | Separate connection means different transaction — doesn't participate in user `BEGIN/ROLLBACK`. `InternalAppender` with context IS in the transaction but requires getting `ClientContext` from invoke, which is not exposed by `VScalar::invoke`. |

---

## Architecture Patterns

### Type Definitions (from vendored headers — HIGH confidence)

```cpp
// duckdb_capi/duckdb/function/pragma_function.hpp
typedef string (*pragma_query_t)(ClientContext &context, const FunctionParameters &parameters);
typedef void (*pragma_function_t)(ClientContext &context, const FunctionParameters &parameters);
```

### Pattern 1: pragma_query_t Registration in C++ Shim

`semantic_views_register_shim` gains real logic in Phase 10. It extracts `DatabaseInstance&` from the C handle and calls `ExtensionLoader::RegisterFunction`.

```cpp
// Source: vendored headers + TPCH extension pattern
#include "duckdb.hpp"
#include "duckdb/main/extension/extension_loader.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "duckdb/main/capi/capi_internal.hpp"
#include "shim.h"

using namespace duckdb;

// Internal PRAGMA callback — returns INSERT SQL to execute in current transaction
static string PragmaDefineSemanticView(ClientContext &context, const FunctionParameters &params) {
    // params.values[0] = view name (VARCHAR)
    // params.values[1] = definition JSON (VARCHAR)
    auto name = params.values[0].GetValue<string>();
    auto json = params.values[1].GetValue<string>();
    // Return SQL that DuckDB will execute in the current transaction
    return "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES ('" +
           name + "', '" + json + "')";
    // NOTE: Use proper escaping in real implementation — see Pitfalls section
}

static string PragmaDropSemanticView(ClientContext &context, const FunctionParameters &params) {
    auto name = params.values[0].GetValue<string>();
    return "DELETE FROM semantic_layer._definitions WHERE name = '" + name + "'";
    // NOTE: Use proper escaping in real implementation
}

extern "C" {

void semantic_views_register_shim(void* db_instance_ptr) {
    // Cast chain: void* → duckdb_database → DatabaseWrapper → DuckDB → DatabaseInstance
    auto* db_c = reinterpret_cast<duckdb_database>(db_instance_ptr);
    auto* wrapper = reinterpret_cast<DatabaseWrapper*>(db_c->internal_ptr);
    DatabaseInstance& db_instance = *wrapper->database->instance;

    ExtensionLoader loader(db_instance, "semantic_views");

    // Register PRAGMA define_semantic_view_internal(name VARCHAR, json VARCHAR)
    auto define_pragma = PragmaFunction::PragmaCall(
        "define_semantic_view_internal",
        PragmaDefineSemanticView,
        {LogicalType::VARCHAR, LogicalType::VARCHAR}
    );
    loader.RegisterFunction(define_pragma);

    // Register PRAGMA drop_semantic_view_internal(name VARCHAR)
    auto drop_pragma = PragmaFunction::PragmaCall(
        "drop_semantic_view_internal",
        PragmaDropSemanticView,
        {LogicalType::VARCHAR}
    );
    loader.RegisterFunction(drop_pragma);
}

} // extern "C"
```

### Pattern 2: Scalar Invoke Write Path (RECOMMENDED — separate connection)

The scalar function invoke cannot execute SQL on the main connection (deadlock). The pre-existing `query_conn` (a separate `duckdb_connection` created at init time) is passed to the shim and stored for use from invoke.

```rust
// src/shim/mod.rs — new FFI declarations for Phase 10
unsafe extern "C" {
    /// Write a semantic view definition to the persistent table via a stored
    /// separate connection. Returns 0 on success, -1 on error.
    /// error_buf receives a null-terminated error string on failure (up to 256 bytes).
    fn semantic_views_pragma_define(
        conn: ffi::duckdb_connection,
        name_ptr: *const std::ffi::c_char,
        json_ptr: *const std::ffi::c_char,
        error_buf: *mut std::ffi::c_char,
        error_buf_len: usize,
    ) -> i32;

    fn semantic_views_pragma_drop(
        conn: ffi::duckdb_connection,
        name_ptr: *const std::ffi::c_char,
        error_buf: *mut std::ffi::c_char,
        error_buf_len: usize,
    ) -> i32;
}
```

```cpp
// In shim.cpp — implements direct SQL via stored connection
// This avoids deadlock because query_conn is a SEPARATE connection
// Limitation: NOT in the user's transaction (separate connection = separate txn)
extern "C" {

int32_t semantic_views_pragma_define(
    duckdb_connection conn,
    const char* name,
    const char* json,
    char* error_buf,
    size_t error_buf_len
) {
    // Build INSERT OR REPLACE SQL with parameter escaping
    // Execute on the stored separate connection
    // Return 0 on success, -1 on error with message in error_buf
}

} // extern "C"
```

**Limitation:** The separate connection write is NOT in the user's calling transaction. However, the success criterion for PERSIST-02 (`BEGIN; PRAGMA define_semantic_view_internal(...); ROLLBACK;`) uses the PRAGMA path directly — not the scalar function path. The scalar `define_semantic_view()` is typically called standalone (not wrapped in user-managed transactions).

### Pattern 3: PRAGMA path for direct user use (PERSIST-02)

When the user calls `PRAGMA define_semantic_view_internal('name', '{"json":...}')`, the `pragma_query_t` callback returns INSERT SQL that DuckDB executes in the current transaction. A ROLLBACK correctly undoes it.

```sql
-- This works transactionally:
BEGIN;
PRAGMA define_semantic_view_internal('orders', '{"base_table":"orders",...}');
ROLLBACK;
-- Result: _definitions table unchanged (PERSIST-02 satisfied)
```

### Pattern 4: HashMap Update Ordering (Option B)

Per CONTEXT.md, prefer updating HashMap ONLY after the write succeeds:

```rust
// ddl/define.rs invoke — Phase 10 change
// OLD: catalog_insert first, then write_sidecar
// NEW: pragma_define first (returns error if it fails), then catalog_insert

// 1. Persist to DB table first (via separate connection or PRAGMA)
if !db_path_is_memory {
    call_pragma_define_ffi(name, json)?; // propagates error if write fails
}
// 2. Only update HashMap after successful persist
catalog_insert(&state.catalog, name, json)?;
```

This ensures HashMap and persistent table are consistent. If the FFI call fails, HashMap is unchanged.

### Pattern 5: One-time Migration in init_catalog

```rust
// catalog.rs init_catalog — Phase 10 additions
pub fn init_catalog(con: &Connection, db_path: &str) -> Result<CatalogState> {
    // ... (existing: CREATE TABLE IF NOT EXISTS) ...
    // ... (existing: load rows from table into map) ...

    // One-time migration: if sidecar exists, import and delete it
    if db_path != ":memory:" {
        let sidecar = sidecar_path(db_path); // last use of sidecar_path
        if sidecar.exists() {
            let migration_data = read_sidecar(db_path); // last use of read_sidecar
            if !migration_data.is_empty() {
                // Use INSERT OR REPLACE to merge (sidecar definitions win on conflict)
                for (name, def) in &migration_data {
                    con.execute(
                        "INSERT OR REPLACE INTO semantic_layer._definitions VALUES (?, ?)",
                        duckdb::params![name, def],
                    )?;
                }
                // Reload map from table (now includes migrated data)
                map = load_map_from_table(con)?;
            }
            // Delete sidecar file regardless of whether it had data
            let _ = std::fs::remove_file(&sidecar);
        }
    }
    // Remove ALL sidecar_path/read_sidecar/write_sidecar functions after migration block
    Ok(Arc::new(RwLock::new(map)))
}
```

### Anti-Patterns to Avoid

- **SQL string concatenation without escaping:** `'name'` in INSERT SQL is vulnerable to injection. Use DuckDB's `Value::CreateValue(str).ToString()` or proper quoting in the C++ callback. The name and JSON values come from trusted Rust code, but double-quotes or apostrophes in view names/JSON strings will break the SQL.
- **Calling `duckdb_query` on main connection from invoke:** Still deadlocks in Phase 10. Only the separate `query_conn` or C++ InternalAppender (in pragma callback) avoids this.
- **Assuming `ExtensionLoader` throws on duplicate pragma:** `RegisterFunction(PragmaFunction)` throws if the pragma already exists. The shim must guard against double registration (e.g., check if already loaded, or use `TryGetFunction`).
- **Including `capi_internal.hpp` in `shim.h`:** The public header `shim.h` must be includable from C. Keep the internal cast in `shim.cpp` only.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Transaction-aware table writes from C++ | Custom transaction tracking | `pragma_query_t` returning INSERT SQL | DuckDB handles transaction tracking; returned SQL runs in caller's transaction automatically |
| SQL parameter escaping in pragma callback | Manual string escaping | `Value::GetValue<string>()` + use DuckDB prepared statement or `StringUtil::Replace` | Raw string concat is brittle; apostrophes in JSON values break the SQL |
| Extension function registration | Direct catalog manipulation | `ExtensionLoader::RegisterFunction()` | ExtensionLoader handles the full registration flow including versioning |

---

## Common Pitfalls

### Pitfall 1: SQL Injection in pragma_query_t Callback

**What goes wrong:** The pragma callback builds INSERT SQL by concatenating `name` and `json` strings. If `name` contains a single quote (e.g., `O'Brien`) or the JSON contains SQL metacharacters, the resulting SQL is malformed or executable.

**Why it happens:** `pragma_query_t` returns a raw SQL string. DuckDB re-parses it as a new statement.

**How to avoid:** Use `StringUtil::Replace(name, "'", "''")` (double-quote escape for SQL) for the view name. The JSON definition is also a string and needs the same treatment. Or use `Value` objects to build the query in a structured way.

**Warning signs:** Test with a view name containing `'` → SQL parse error in pragma return.

### Pitfall 2: Double Registration of PRAGMA

**What goes wrong:** `ExtensionLoader::RegisterFunction(PragmaFunction)` throws if the pragma name already exists. If the extension is somehow loaded twice or the shim function is called twice, it throws.

**Why it happens:** Pragma registration is non-idempotent (unlike scalar function registration which merges overloads).

**How to avoid:** Guard with `loader.TryGetFunction("define_semantic_view_internal")` before registering, or use the `ExtensionIsLoaded` check that already exists in DuckDB's extension loading infrastructure.

### Pitfall 3: duckdb_database Cast Chain

**What goes wrong:** The comment in `shim.cpp` says `db_instance_ptr will be cast to DatabaseInstance*`. This is WRONG. What's passed is a `duckdb_database` pointer (i.e., pointer to `_duckdb_database { void *internal_ptr }`). A direct cast to `DatabaseInstance*` produces garbage.

**Correct cast chain (HIGH confidence — verified against vendored headers):**
```cpp
// db_instance_ptr is actually: *mut _duckdb_database (cast to void*)
auto* db_c = reinterpret_cast<duckdb_database>(db_instance_ptr);
// db_c->internal_ptr points to DatabaseWrapper { shared_ptr<DuckDB> database }
auto* wrapper = reinterpret_cast<duckdb::DatabaseWrapper*>(db_c->internal_ptr);
// wrapper->database is shared_ptr<DuckDB>
// DuckDB::instance is shared_ptr<DatabaseInstance>
duckdb::DatabaseInstance& db_instance = *wrapper->database->instance;
```

Requires `#include "duckdb/main/capi/capi_internal.hpp"` in `shim.cpp`.

**Warning signs:** Segfault or wrong function registrations at load time.

### Pitfall 4: Separate Connection is NOT in User Transaction

**What goes wrong:** If `define_semantic_view()` scalar function writes via a stored `query_conn` (separate connection), that write is auto-committed to its own transaction. If the user has an explicit `BEGIN` on the main connection, the table write happens outside that transaction.

**Why it happens:** DuckDB connections have independent transaction contexts.

**How to avoid:** Document this clearly. The scalar `define_semantic_view()` is not designed to be called inside user transactions. The transactional path is `PRAGMA define_semantic_view_internal(...)` for users who need rollback semantics (PERSIST-02 test case).

**Warning signs:** `BEGIN; SELECT define_semantic_view(...); ROLLBACK;` — the table write persists even after ROLLBACK (but HashMap is rolled back on next init if DB is restarted).

### Pitfall 5: build.rs Symbol Visibility with New Exports

**What goes wrong:** The macOS exported symbols list (`semantic_views.exp`) and Linux version script currently only export `_semantic_views_init_c_api`. Adding `semantic_views_pragma_define` and `semantic_views_pragma_drop` as exported C symbols requires they be listed (or the approach is adjusted).

**Why it happens:** Symbol visibility restrictions strip all non-listed symbols.

**How to avoid:** These functions are called from Rust (`extern "C"` in `src/shim/mod.rs`), so they'll be statically linked into the cdylib — NOT exported. They don't need to be in the symbols list. The linker resolves them at build time, not runtime. Only `semantic_views_init_c_api` needs to be externally visible.

### Pitfall 6: Sidecar Test Data Interfering

**What goes wrong:** `test/data/test_catalog.duckdb.semantic_views` and `test/sql/restart_test.db.semantic_views` are actual sidecar files in the repo. After Phase 10, these files must not be read by `init_catalog`. If they're left in place, the migration logic runs and tries to import them.

**How to avoid:** Sidecar migration in `init_catalog` should run ONCE and then delete the sidecar. The test files will be deleted on first test run after Phase 10 ships. But the test SQLLogicTest files themselves reference sidecar behavior — update them to not expect sidecar files. Also, PERSIST-03 verification (`grep -r "semantic_views"` on file paths) would fail if these test data files exist.

**Warning signs:** PERSIST-03 grep catches `test/sql/restart_test.db.semantic_views`.

---

## Code Examples

### Complete include set for shim.cpp Phase 10

```cpp
// Source: vendored duckdb_capi/ headers, confirmed present
#include "duckdb.hpp"
#include "duckdb/main/extension/extension_loader.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "duckdb/main/capi/capi_internal.hpp"  // for DatabaseWrapper
#include "duckdb/common/string_util.hpp"        // for StringUtil::Replace (escaping)
#include "shim.h"

using namespace duckdb;
```

### FunctionParameters access in pragma_query_t callback

```cpp
// Source: pragmafunction.hpp + TPCH extension pattern
static string PragmaDefineSemanticView(ClientContext &context,
                                       const FunctionParameters &params) {
    // params.values is vector<Value>
    auto name = params.values[0].GetValue<string>();
    auto json = params.values[1].GetValue<string>();

    // Escape single quotes for SQL safety
    auto safe_name = StringUtil::Replace(name, "'", "''");
    auto safe_json = StringUtil::Replace(json, "'", "''");

    return "INSERT OR REPLACE INTO semantic_layer._definitions "
           "(name, definition) VALUES ('" + safe_name + "', '" + safe_json + "')";
}
```

### ExtensionLoader construction from duckdb_database handle

```cpp
// Source: capi_internal.hpp (DatabaseWrapper) + database.hpp (DuckDB::instance)
void semantic_views_register_shim(void* db_instance_ptr) {
    auto* db_c = reinterpret_cast<duckdb_database>(db_instance_ptr);
    auto* wrapper = reinterpret_cast<DatabaseWrapper*>(db_c->internal_ptr);
    DatabaseInstance& db_instance = *wrapper->database->instance;
    ExtensionLoader loader(db_instance, "semantic_views");
    // ... RegisterFunction calls ...
}
```

### Rust FFI declaration for pragma write from invoke

```rust
// Source: src/shim/mod.rs (Phase 10)
// These are implemented in shim.cpp and statically linked into the cdylib.
// They execute INSERT/DELETE on a separate stored connection — no deadlock.
unsafe extern "C" {
    fn semantic_views_pragma_define(
        conn: libduckdb_sys::duckdb_connection,
        name: *const std::ffi::c_char,
        json: *const std::ffi::c_char,
    ) -> i32; // 0 = success, -1 = error

    fn semantic_views_pragma_drop(
        conn: libduckdb_sys::duckdb_connection,
        name: *const std::ffi::c_char,
    ) -> i32;
}
```

### DefineState Phase 10 change

```rust
// src/ddl/define.rs — Phase 10
// REMOVE: db_path field (sidecar gone)
// ADD: persist_conn for table writes from invoke
#[derive(Clone)]
pub struct DefineState {
    pub catalog: CatalogState,
    pub persist_conn: libduckdb_sys::duckdb_connection, // separate connection for writes
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Sidecar file (plain filesystem I/O from invoke) | `pragma_query_t` table write (transaction-aware) | Phase 10 | Writes participate in DuckDB's WAL; no external file; ROLLBACK works via PRAGMA path |
| `ExtensionUtil::RegisterFunction` (older DuckDB API) | `ExtensionLoader::RegisterFunction` (DuckDB 1.2+) | ~DuckDB 1.2 | `ExtensionLoader` is the current pattern; `ExtensionUtil` still exists but `ExtensionLoader` is preferred |
| `CreatePragmaFunctionInfo` + `catalog.CreatePragmaFunction` (very old) | `ExtensionLoader::RegisterFunction(PragmaFunction)` | ~DuckDB 1.0 | The loader wraps catalog access; simpler API |

**Deprecated/outdated:**
- `ExtensionUtil::RegisterFunction` with `DatabaseInstance&` directly: Still works but `ExtensionLoader` is the canonical modern pattern (as shown in `extension_loader.hpp` being the primary header for extension development).
- Sidecar file approach: Gone entirely after Phase 10.

---

## Open Questions

1. **How does `semantic_views_pragma_define` avoid deadlock from within scalar invoke?**
   - What we know: DuckDB holds execution locks during scalar invoke; `duckdb_query` on the main connection deadlocks; sidecar file I/O was the only deadlock-free option in v0.1.0.
   - What's unclear: Whether a pre-stored separate connection (`query_conn`) can safely execute `INSERT OR REPLACE` from within invoke without contending on the main connection's lock.
   - Recommendation: **Use the pre-existing `query_conn` from lib.rs init.** It's a fully independent connection. DuckDB allows concurrent connections to a file-backed database (WAL mode). The write goes to a separate transaction — acceptable because `define_semantic_view()` is typically called standalone. The PRAGMA path (separate statement from user) is the transactional path for PERSIST-02. Verify empirically in Wave 1.

2. **SQL injection in pragma_query_t return string — is `StringUtil::Replace` sufficient?**
   - What we know: View names and JSON defs come from user input. Single quotes in names/JSON break the returned SQL string.
   - What's unclear: Whether DuckDB 1.4.4's `StringUtil::Replace` is accessible from extension shim code, or whether a simpler manual replace is needed.
   - Recommendation: Use a simple manual replace loop in the shim (no dependency). OR use `$1, $2` parameter syntax in the INSERT — but `pragma_query_t` returns a raw SQL string with no parameter binding API, so values must be embedded literally.

3. **Does the PRAGMA test for PERSIST-02 call the internal PRAGMA directly, or through define_semantic_view()?**
   - What we know: CONTEXT.md specifics say "the test goes through the raw PRAGMA callback directly, not via the scalar function wrapper."
   - What's unclear: Whether the PRAGMA also updates the in-memory HashMap (it shouldn't — it only writes to the table; HashMap is for the current session and is populated from the table on next `init_catalog`).
   - Recommendation: PRAGMA path writes to table only. HashMap is NOT updated by PRAGMA. When DB is reopened, `init_catalog` loads from table into HashMap. This is consistent with the design.

4. **Should `DefineState` store `duckdb_connection` (C handle) or pass it through the FFI call?**
   - What we know: `DefineState` is `Clone` and shared across threads. Raw `duckdb_connection` is a raw pointer — not `Send + Sync` in Rust's type system.
   - Recommendation: Store as `*mut c_void` wrapped in a newtype struct that implements `Send + Sync` (justified because DuckDB connections are thread-safe for serial access). OR pass the conn as a parameter to the FFI call rather than storing in DefineState (cleaner ownership).

---

## Sources

### Primary (HIGH confidence)
- Vendored headers at `duckdb_capi/duckdb/function/pragma_function.hpp` — confirmed `pragma_query_t` type definition, `PragmaFunction::PragmaCall` signatures
- Vendored headers at `duckdb_capi/duckdb/main/extension/extension_loader.hpp` — confirmed `ExtensionLoader::RegisterFunction(PragmaFunction)` signature
- Vendored headers at `duckdb_capi/duckdb/main/capi/capi_internal.hpp` — confirmed `DatabaseWrapper` struct for cast chain
- Vendored headers at `duckdb_capi/duckdb/main/database.hpp` — confirmed `DuckDB::instance` field type
- Vendored headers at `duckdb_capi/duckdb/main/appender.hpp` — confirmed `InternalAppender(ClientContext&, TableCatalogEntry&)` constructor
- `src/catalog.rs`, `src/ddl/define.rs`, `src/ddl/drop.rs`, `src/lib.rs`, `src/shim/shim.cpp` — existing codebase reviewed for integration points
- TPCH extension pattern (WebFetch from DuckDB GitHub main): `PragmaFunction::PragmaCall("tpch", PragmaTpchQuery, {LogicalType::BIGINT})` + `loader.RegisterFunction(tpch_func)` — confirms registration API
- `duckdb_capi/duckdb.h` line 488-490 — confirmed `duckdb_database` typedef: `struct _duckdb_database { void *internal_ptr; } * duckdb_database`

### Secondary (MEDIUM confidence)
- WebFetch: `pragma_function.hpp` via GitHub (raw) — corroborates vendored header content; confirms `pragma_query_t` returning SQL string is re-executed by DuckDB in current transaction context (pattern confirmed from TPCH extension behavior)
- WebFetch: `extension_util.hpp` via GitHub — confirmed `RegisterFunction(DatabaseInstance&, PragmaFunction)` signature (older API; ExtensionLoader is newer)

### Tertiary (LOW confidence)
- WebSearch for `pragma_query_t` transaction behavior — no authoritative source found; inferred from TPCH extension usage (returns SELECT query that runs in current session)
- Cast chain from `duckdb_database` → `DatabaseInstance&` — derived from header inspection; not confirmed against DuckDB C API source (implementation files not accessible). The cast is a derived inference from struct definitions. **Needs empirical verification during Wave 1.**

---

## Metadata

**Confidence breakdown:**
- Standard stack (types, signatures): HIGH — vendored headers directly consulted
- Architecture (cast chain, integration points): MEDIUM — headers confirmed but implementation .cpp files not accessible; derived logically
- Pitfalls (SQL injection, symbol visibility, cast error): HIGH — specific, verifiable from source inspection
- Deadlock resolution for scalar invoke: MEDIUM — logically sound but needs empirical confirmation

**Research date:** 2026-03-01
**Valid until:** 2026-04-01 (stable API; no fast-moving dependencies)

---

## Key Planning Guidance for Planner

The planner MUST address these in task sequencing:

1. **Wave 0 (C++ shim):** Update `semantic_views_register_shim` in `shim.cpp` + `shim.h`. Add `semantic_views_pragma_define` / `semantic_views_pragma_drop` C functions. Add FFI declarations in `src/shim/mod.rs`.

2. **Wave 1 (Rust invoke update):** Update `DefineState`/`DropState` to remove `db_path`, add persist connection. Update `ddl/define.rs` and `ddl/drop.rs` invoke to call pragma FFI instead of `write_sidecar`. Use Option B: write first, then update HashMap.

3. **Wave 2 (catalog.rs migration + sidecar deletion):** Add one-time migration block to `init_catalog`. Delete all sidecar functions (`sidecar_path`, `read_sidecar`, `write_sidecar`). Delete all sidecar tests.

4. **Wave 3 (test files + test updates):** Update SQLLogicTest files that reference sidecar behavior. Delete or update test data files. Add PRAGMA rollback test for PERSIST-02.

5. **PERSIST-03 verification:** Run `grep -r "semantic_views" --include="*.rs" --include="*.cpp" --include="*.h"` on file paths (file extensions, not content) — must return no results for `.semantic_views` file extension references.

The `STATE.md` blocker says "Confirm `pragma_query_t` non-PRAGMA DDL integration path against DuckDB 1.4.4 source before writing Phase 10 plan." This research resolves that blocker: use separate connection for scalar invoke path, pragma_query_t for user-visible PRAGMA path. These are two distinct write paths with different transaction semantics — document clearly in code.
