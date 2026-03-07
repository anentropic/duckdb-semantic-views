# Architecture: Parser Extension Integration

**Project:** DuckDB Semantic Views Extension (v0.5.0)
**Researched:** 2026-03-07
**Scope:** How DuckDB parser extension hooks integrate with the existing pure-Rust extension architecture
**Confidence:** HIGH for entry point mechanics and data flow; MEDIUM for C++ shim build details (prql is the closest reference, but no Rust+C++ mixed extension ships with parser hooks yet)

---

## Current Architecture (Baseline)

```
                    DuckDB Extension Loader
                            |
                    reads footer: ABI_TYPE = C_STRUCT
                            |
                    calls semantic_views_init_c_api()
                            |
                    lib.rs (manual FFI entrypoint)
                            |
                    init_extension(Connection, db_handle)
                            |
          +-----------------+-----------------+
          |                 |                 |
    DDL Functions     Query Functions    Catalog Init
    (define, drop,    (semantic_view,    (schema + table
     list, describe)   explain)          + HashMap sync)
          |                 |                 |
    parse_args.rs     table_function.rs  catalog.rs
    model.rs          expand.rs          (CatalogState)
```

**Key facts about the current entry path:**
- Footer ABI type: `C_STRUCT` (stamped by extension-ci-tools Python script)
- Entry symbol: `semantic_views_init_c_api(info, access)` -- manual FFI (not macro-generated)
- Receives: `duckdb_extension_info` + `duckdb_extension_access*` (function pointer table)
- Extracts: `duckdb_database` handle, creates `Connection::open_from_raw()`
- Creates: separate `persist_conn` (DDL writes) and `query_conn` (semantic_view execution)
- Registers: 8 table functions via `con.register_table_function_with_extra_info()`
- Symbol visibility: `build.rs` exports only `_semantic_views_init_c_api`

---

## Target Architecture (v0.5.0)

```
                    DuckDB Extension Loader
                            |
                    reads footer: ABI_TYPE = CPP
                            |
                    calls semantic_views_init() [C++ symbol]
                            |
                    shim.cpp (DUCKDB_CPP_EXTENSION_ENTRY macro)
                            |
          +-----------------+-----------------+
          |                 |                 |
    Register Parser    Call sv_init_rust()   (C++ scope ends)
    Extension          via extern "C" FFI
    (C++ only)              |
          |           Rust init_extension()
          |           (same as today, but
          |            receives db_handle
          |            differently)
          |                 |
    parse_function     DDL + Query Functions
    plan_function      (unchanged)
    (C++ trampolines
     -> Rust FFI)
```

---

## Question 1: Entry Point Coexistence (CPP vs C_STRUCT)

### How DuckDB Chooses the Entry Point

DuckDB's extension loader reads the binary footer appended to every `.duckdb_extension` file. The footer contains an `ABI_TYPE` field with three possible values:

| ABI Type | Entry symbol pattern | How it works |
|----------|---------------------|--------------|
| `CPP` | `{name}_init(DatabaseInstance&)` or `DUCKDB_CPP_EXTENSION_ENTRY` | Full C++ link against amalgamation. DuckDB resolves the symbol from the loaded shared library. |
| `C_STRUCT` | `{name}_init_c_api(info, access)` | C API function pointer table. No C++ symbol resolution needed. |
| `C_STRUCT_UNSTABLE` | Same as `C_STRUCT` | Uses unstable function pointers; tied to exact DuckDB version. |

**DuckDB calls exactly one entry point based on the footer ABI type.** If the footer says `CPP`, only `semantic_views_init` is called. If it says `C_STRUCT`, only `semantic_views_init_c_api` is called. Both symbols can exist in the binary -- only one is invoked.

### Can Both Coexist in the Binary?

Yes. The symbols are independent. The footer determines dispatch, not symbol presence. However:

- **The Rust-generated `semantic_views_init_c_api` must still be compiled** because `duckdb-rs` with `loadable-extension` feature generates it (or the manual FFI code in `lib.rs` defines it). It will simply never be called when the footer says `CPP`.
- **It should NOT be exported** from the final binary. The symbol visibility list in `build.rs` must change from `_semantic_views_init_c_api` to `_semantic_views_init` (plus any other symbols the C++ shim needs).
- **For `cargo test`** (bundled feature, no extension feature), nothing changes -- tests don't use either entry point.

### Recommendation

Switch the footer ABI type to `CPP`. Keep `semantic_views_init_c_api` in the source for compilation purposes but hide it from the symbol table. The C++ `DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader)` macro generates `semantic_views_init` with the correct C++ mangling and `extern "C"` decoration.

**Confidence:** HIGH -- verified from DuckDB PR #12682 (C API extensions) and extension-template-c documentation.

---

## Question 2: Initializing Rust's duckdb-rs C API Stubs from C++ Path

### The Problem

When entering via `semantic_views_init_c_api`, the Rust code calls `duckdb_rs_extension_api_init(info, access, version)` to populate the `duckdb/loadable-extension` function pointer stubs. These stubs are what make `ffi::duckdb_query()`, `ffi::duckdb_connect()`, etc. work from Rust code in a loadable extension (they redirect through the function pointer table instead of trying to resolve C symbols from the host).

When entering via the C++ path (`DUCKDB_CPP_EXTENSION_ENTRY`), the shim receives an `ExtensionLoader&` -- NOT `duckdb_extension_info` + `duckdb_extension_access*`. The Rust stubs cannot be initialized without the `access` pointer.

### Solution: Extract the C API Access Struct from ExtensionLoader

The `ExtensionLoader` wraps the same `duckdb_extension_info` and `duckdb_extension_access` that the C API path uses. The C++ shim can extract them:

```cpp
// In shim.cpp
DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader) {
    // 1. Get the raw C API handles from the loader
    auto info = loader.GetExtensionInfo();      // duckdb_extension_info
    auto access = loader.GetExtensionAccess();  // duckdb_extension_access*

    // 2. Pass to Rust for C API stub init
    sv_init_rust(info, access);

    // 3. Register parser hooks (C++ only)
    auto& db = loader.GetDatabaseInstance();
    auto& cfg = DBConfig::GetConfig(db);
    // ... parser extension registration ...
}
```

The Rust side then calls `duckdb_rs_extension_api_init(info, access, version)` exactly as it does today, followed by `Connection::open_from_raw()` and the rest of `init_extension()`.

**Key insight:** `ExtensionLoader` is a wrapper, not a replacement. The underlying C API handles are accessible from it.

### Alternative: Skip duckdb-rs Stubs Entirely

If `ExtensionLoader` does NOT expose the raw C API handles (needs verification against DuckDB source), an alternative is:

1. The C++ shim creates a `Connection(db)` directly (C++ API -- always works)
2. Pass the `duckdb_connection` handle to Rust via `sv_init_rust(duckdb_connection)`
3. Rust uses this connection handle with raw `ffi::duckdb_query()` calls
4. Problem: `ffi::duckdb_query` is a stub that requires initialization

This fallback is less clean. The preferred path is extracting `info`/`access` from `ExtensionLoader`.

### Verification Needed

Whether `ExtensionLoader` exposes raw C API handles needs verification against DuckDB v1.4.4 source. If not, the shim must construct equivalent structures or use a different initialization path.

**Confidence:** MEDIUM -- the extraction approach is architecturally sound but the specific `ExtensionLoader` API surface needs source verification.

---

## Question 3: The PRQL Extension Pattern (C++ Entry -> Rust Logic)

### How PRQL Bridges C++ and Rust

PRQL is a **pure C++ extension** that compiles PRQL syntax to SQL. It does NOT bridge to Rust. However, its architecture provides the canonical pattern for parser extension registration:

#### Registration (LoadInternal)

```cpp
void LoadInternal(ExtensionLoader &loader) {
    auto& db = loader.GetDatabaseInstance();
    auto& config = DBConfig::GetConfig(db);

    // Parser extension: fallback hook
    PrqlParserExtension parser_ext;
    parser_ext.parse_function = prql_parse;
    parser_ext.plan_function = prql_plan;
    config.parser_extensions.push_back(parser_ext);

    // Operator extension: bind override
    config.operator_extensions.push_back(make_uniq<PrqlOperatorExtension>());
}
```

#### Parse Function

```cpp
ParserExtensionParseResult prql_parse(ParserExtensionInfo*, const string& query) {
    // 1. Check if this is PRQL (prefix/delimiter check)
    // 2. If not, return ParserExtensionResultType::DISPLAY_ORIGINAL_ERROR
    // 3. If yes, compile PRQL to SQL
    // 4. Parse the resulting SQL with DuckDB's own parser
    // 5. Return ParserExtensionParseResult with PrqlParseData
}
```

#### Plan Function (Stash Pattern)

```cpp
ParserExtensionPlanResult prql_plan(ParserExtensionInfo*, ClientContext& context,
                                     unique_ptr<ParserExtensionParseData> parse_data) {
    // Stash parsed state in context.registered_state
    auto state = make_shared_ptr<PrqlState>(move(parse_data));
    context.registered_state->Remove("prql");
    context.registered_state->Insert("prql", state);

    // Throw to defer to operator extension bind
    throw BinderException("Use prql_bind instead");
}
```

#### Operator Extension Bind

```cpp
BoundStatement prql_bind(ClientContext& context, Binder& binder,
                         OperatorExtensionInfo*, SQLStatement& statement) {
    // Retrieve stashed state
    auto& prql_state = context.registered_state->Get<PrqlState>("prql");
    // Create child binder, bind the parsed SQL statement
    auto child_binder = binder.CreateBinder();
    return child_binder->Bind(prql_state.statement);
}
```

### What This Means for Semantic Views

The PRQL pattern is designed for **query rewriting** (PRQL -> SQL -> execute). Semantic views need a different pattern because `CREATE SEMANTIC VIEW` is **DDL** -- it modifies catalog state, not query execution.

For DDL, the `plan_function` should return a `ParserExtensionPlanResult` containing a `TableFunction` that performs the catalog mutation. This is how the RBAC extension pattern works:

```cpp
ParserExtensionPlanResult sv_plan(...) {
    ParserExtensionPlanResult result;
    result.function = TableFunction("sv_create_internal", {}, sv_create_execute, sv_create_bind);
    result.parameters.push_back(Value(parsed_json));  // Pass parsed DDL as parameter
    return result;
}
```

The `plan_function` returns a `TableFunction` + parameters. DuckDB binds and executes this table function, which performs the actual catalog insert/update.

**Confidence:** HIGH for the registration pattern (verified from prql source). MEDIUM for the DDL table function return pattern (inferred from `ParserExtensionPlanResult.function` field and RBAC RFC, not verified from a shipping DDL extension).

---

## Question 4: Connection Lifetime (C++ -> Rust)

### The Problem

The C++ shim creates objects with C++ lifetime semantics. Rust needs a `duckdb_connection` handle that survives beyond the `LoadInternal` call scope.

### Solution

The C++ entry point has access to `DatabaseInstance&` which lives for the entire database session. Creating connections from it is safe:

```cpp
DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader) {
    auto& db = loader.GetDatabaseInstance();

    // Option A: Pass db handle to Rust, let Rust create connections
    // Rust calls ffi::duckdb_connect(db_handle, &mut conn)
    sv_init_rust(db_as_c_handle);

    // Option B: Create connection in C++, pass to Rust
    // The Connection must be heap-allocated to survive this scope
    auto* conn = new Connection(db);  // leaked intentionally -- lives for DB lifetime
    sv_init_rust(conn->GetConnection());
}
```

**Option A is preferred** because:
- The current Rust code already creates connections via `ffi::duckdb_connect(db_handle, &mut conn)` in `init_extension`
- It avoids C++ heap allocation management
- The `db_handle` (a `duckdb_database`) is the same type Rust already works with
- Connection lifetime is managed by Rust (RAII via `Drop` or explicit `duckdb_disconnect`)

The critical requirement: **the `duckdb_database` handle must be extractable from `ExtensionLoader`**. This is the same handle that `(*access).get_database.unwrap()(info)` returns in the C API path. The C++ path gets it via `loader.GetDatabaseInstance()`, which can be cast back to a C handle if needed.

### Current Code Impact

The existing `init_extension` signature is:
```rust
fn init_extension(con: &Connection, db_handle: ffi::duckdb_database) -> Result<...>
```

This stays the same. The only change is **how** `con` and `db_handle` are obtained:
- Today: extracted from `duckdb_extension_access` function pointers in Rust
- v0.5.0: passed from C++ shim via FFI

**Confidence:** HIGH -- the `duckdb_database` handle extraction from `DatabaseInstance` is standard DuckDB C++ API usage, and Rust already handles connection creation from this handle.

---

## Question 5: Data Flow for CREATE SEMANTIC VIEW

### Complete Flow: Statement Text to Catalog Mutation

```
User: CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...], metrics := [...]);
  |
  v
1. DuckDB statement splitter
   Splits input into individual statements. Our hook receives one statement string.
  |
  v
2. DuckDB native parser attempts to parse
   Fails at "SEMANTIC" keyword (not in grammar). Falls through to extension hooks.
  |
  v
3. parse_function (C++ trampoline -> Rust FFI)
   Receives: const string& query (the full statement text)
   Rust does:
     a. Strip leading whitespace/comments, trailing semicolon
     b. Prefix match: "CREATE [OR REPLACE] SEMANTIC VIEW" / "DROP SEMANTIC VIEW"
     c. If no match: return DISPLAY_ORIGINAL_ERROR (pass through)
     d. If match: parse the statement body into SemanticViewDefinition
     e. Serialize to JSON (or custom ParseData struct)
     f. Return ParserExtensionParseResult { type: PARSE_SUCCESSFUL, parse_data }
  |
  v
4. plan_function (C++ trampoline -> Rust FFI)
   Receives: ClientContext&, unique_ptr<ParserExtensionParseData>
   Returns: ParserExtensionPlanResult containing:
     - A TableFunction (internal, not user-visible)
     - Parameters: [view_name, serialized_definition_json]
   This TableFunction is what DuckDB will bind and execute.
  |
  v
5. DuckDB binds the TableFunction
   Calls the bind callback with the parameters.
   The bind callback:
     a. Deserializes the JSON into SemanticViewDefinition
     b. Performs type inference (LIMIT 0 query) if file-backed
     c. Persists to semantic_layer._definitions via persist_conn
     d. Inserts into in-memory catalog (CatalogState HashMap)
     e. Returns bind data (view name for output)
  |
  v
6. DuckDB executes the TableFunction
   Calls the func callback, which outputs a single row: the view name.
   (Same pattern as existing DefineSemanticViewVTab::func)
```

### Parse Function Detail

The parse function in Rust needs a **new parser** for the SQL DDL syntax. This is NOT the same as `parse_args.rs` (which parses DuckDB STRUCT/LIST values from bind parameters). The new parser handles raw SQL text:

```sql
CREATE SEMANTIC VIEW sales (
  TABLES (
    {alias: 'o', table: 'orders'},
    {alias: 'c', table: 'customers'}
  ),
  DIMENSIONS (
    {name: 'region', expr: 'region', source_table: 'c'},
    {name: 'order_date', expr: 'date_trunc(''month'', order_date)'}
  ),
  METRICS (
    {name: 'revenue', expr: 'sum(amount)'}
  )
);
```

Options for the parse surface:
1. **Minimal: Reuse DuckDB's own struct/list literal parsing** -- extract the body after `CREATE SEMANTIC VIEW <name>` and parse it as the same struct literals the scalar function accepts. DuckDB can parse `{alias: 'o', table: 'orders'}` natively.
2. **Custom: Parse a SQL-like DDL syntax** -- define a grammar closer to Snowflake's `CREATE SEMANTIC VIEW` with `TABLES (...), DIMENSIONS (...), METRICS (...)` clauses.
3. **Passthrough: Convert to scalar function call** -- the parse function rewrites `CREATE SEMANTIC VIEW sales (...)` into `FROM create_semantic_view('sales', tables := [...], ...)` and re-parses with DuckDB's native parser. This reuses 100% of existing code but may look awkward.

**Recommendation: Option 3 (passthrough rewrite) for the spike.** It validates the parser hook mechanism end-to-end without writing a new parser. The parse function extracts the view name and body, wraps them into the existing scalar function call syntax, and lets DuckDB parse the rest. A custom grammar can follow in a later milestone.

### Plan Function Detail

For a DDL passthrough (option 3), the plan function receives the rewritten SQL statement and returns a `ParserExtensionPlanResult` with a `TableFunction`. The table function is the existing `create_semantic_view` (or a thin internal wrapper).

For a DDL with custom parsing (options 1/2), the plan function receives the parsed `SemanticViewDefinition` and returns a table function that performs the catalog mutation directly.

**Confidence:** HIGH for the overall flow. MEDIUM for the exact `ParserExtensionPlanResult` -> `TableFunction` binding mechanics (verified from header definition but no DDL extension source code fully traced).

---

## Question 6: Scalar Function DDL Alongside Native DDL

### Can Both Interfaces Coexist?

Yes. They operate at different levels:

| Interface | Trigger | Parser involvement | Execution path |
|-----------|---------|-------------------|----------------|
| `FROM create_semantic_view(...)` | DuckDB recognizes `create_semantic_view` as a registered table function | None (standard DuckDB parser handles it) | VTab bind -> parse_args.rs -> catalog |
| `CREATE SEMANTIC VIEW ...` | DuckDB parser fails, falls through to parse_function hook | parse_function + plan_function | plan_function returns TableFunction -> bind -> catalog |

The parser hook only fires when DuckDB's native parser fails. Since `FROM create_semantic_view(...)` parses successfully as a standard table function call, the parser hook is never invoked for it.

**Both interfaces write to the same catalog** (`CatalogState` HashMap + `semantic_layer._definitions` table). Views created via either interface are visible and queryable through both.

**Recommendation:** Keep both interfaces permanently. The scalar function DDL is valuable for:
- Programmatic use (Python/Node.js clients constructing definitions)
- Backward compatibility
- Environments where the C++ shim is not available (if a pure-Rust build is ever needed)

**Confidence:** HIGH -- parser hooks are fallback-only and do not interfere with successfully-parsed statements.

---

## New Components Required

### 1. `shim.cpp` (NEW -- ~50 lines)

**Purpose:** C++ entry point + parser extension registration + FFI trampolines.

**Responsibilities:**
- Exports `semantic_views_init` via `DUCKDB_CPP_EXTENSION_ENTRY` macro
- Extracts `duckdb_database` handle and passes to Rust init
- Registers `ParserExtension` with `parse_function` and `plan_function`
- Provides C++ trampoline functions that call Rust via `extern "C"` FFI

**Depends on:** DuckDB amalgamation header (`duckdb.hpp`)

```
shim.cpp
  |-- #include "duckdb.hpp"  (amalgamation, ~500K lines)
  |-- extern "C" sv_init_rust(...)  -> lib.rs
  |-- extern "C" sv_parse(...)      -> parser.rs (new)
  |-- extern "C" sv_plan(...)       -> parser.rs (new)
```

### 2. `src/parser.rs` (NEW -- ~100-200 lines)

**Purpose:** Rust-side parser hook implementation.

**Responsibilities:**
- `sv_parse`: Prefix detection + statement rewriting (or custom parse)
- `sv_plan`: Returns parsed data for plan function trampoline
- FFI types for `ParserExtensionParseResult` / `ParserExtensionPlanResult`

**Depends on:** `model.rs` (SemanticViewDefinition), `catalog.rs` (CatalogState)

### 3. `build.rs` changes (MODIFY)

**Changes needed:**
- Compile `shim.cpp` via `cc` crate against DuckDB amalgamation header
- Update exported symbol from `_semantic_views_init_c_api` to `_semantic_views_init`
- Add include path for `duckdb.hpp`
- Link C++ standard library (`-lstdc++` on Linux, `-lc++` on macOS)

### 4. `Cargo.toml` changes (MODIFY)

**Changes needed:**
- Add `cc` build dependency: `[build-dependencies] cc = "1"`
- May need to vendor or download `duckdb.hpp` amalgamation

### 5. Footer stamping changes (MODIFY)

**Changes needed:**
- Extension-ci-tools footer script must stamp `ABI_TYPE = CPP` instead of `C_STRUCT`

---

## Modified Components

### `src/lib.rs` (MODIFY)

**Changes:**
- Add `#[no_mangle] pub extern "C" fn sv_init_rust(...)` -- the Rust entry called from C++
- The existing `semantic_views_init_c_api` stays for compilation but is no longer the active entry point
- `init_extension()` internal logic is unchanged
- The `db_handle` extraction changes: instead of `(*access).get_database.unwrap()(info)`, it receives the handle as a parameter from the C++ shim

### `src/ddl/parse_args.rs` (UNCHANGED)

The existing struct/list parsing logic is NOT affected. It continues to serve the scalar function DDL path. The parser hook path uses a different entry (raw SQL text -> `parser.rs`).

### `src/catalog.rs` (UNCHANGED)

Both DDL paths converge on `catalog_insert` / `catalog_upsert`. No changes needed.

### `src/expand.rs` (UNCHANGED)

Query expansion is orthogonal to DDL syntax.

### `src/query/` (UNCHANGED)

Query execution (semantic_view table function) is orthogonal to DDL syntax.

---

## Component Boundaries

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| `shim.cpp` | C++ entry, parser hook registration, FFI trampolines | DuckDB (loader API), `sv_init_rust` (Rust), `sv_parse`/`sv_plan` (Rust) |
| `src/parser.rs` | Parse `CREATE SEMANTIC VIEW` text, prepare plan result | `model.rs` (types), `catalog.rs` (state), `shim.cpp` (via FFI) |
| `src/lib.rs` | Rust init, function registration (unchanged core) | `catalog.rs`, `ddl/*`, `query/*`, `shim.cpp` (receives call) |
| `src/ddl/define.rs` | Scalar function DDL (unchanged) | `parse_args.rs`, `catalog.rs` |
| `src/catalog.rs` | In-memory + persistent catalog (unchanged) | `ddl/*`, `parser.rs`, `query/*` |
| `build.rs` | Compile C++ shim, symbol visibility | `cc` crate, DuckDB amalgamation |

---

## Patterns to Follow

### Pattern 1: FFI Trampoline (C++ -> Rust)

The C++ shim calls Rust functions via `extern "C"`. Each trampoline converts C++ types to C types, calls Rust, and converts back.

```cpp
// shim.cpp
ParserExtensionParseResult sv_parse_trampoline(
    ParserExtensionInfo* info, const string& query) {
    // Call Rust
    auto result = sv_parse(query.c_str(), query.size());
    // Convert Rust result to C++ result
    if (result.success) {
        return ParserExtensionParseResult(
            make_uniq<SvParseData>(result.json, result.json_len));
    }
    return ParserExtensionParseResult();  // DISPLAY_ORIGINAL_ERROR
}
```

```rust
// parser.rs
#[no_mangle]
pub extern "C" fn sv_parse(query: *const c_char, len: usize) -> SvParseResult {
    let query_str = unsafe { std::str::from_utf8_unchecked(
        std::slice::from_raw_parts(query as *const u8, len)
    )};
    // Fast prefix check
    if !is_semantic_view_statement(query_str) {
        return SvParseResult { success: false, .. };
    }
    // Parse and return
    // ...
}
```

### Pattern 2: Statement Rewrite (Spike Approach)

For the v0.5.0 spike, rewrite `CREATE SEMANTIC VIEW` into the existing table function call:

```
Input:  CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...], metrics := [...])
Output: FROM create_semantic_view('sales', tables := [...], dimensions := [...], metrics := [...])
```

The parse function extracts the view name, wraps the arguments into the scalar function syntax, and the plan function returns this as a SQL statement for DuckDB to re-parse and execute normally.

This approach:
- Validates the parser hook mechanism end-to-end
- Reuses 100% of existing DDL code
- Avoids building a custom SQL parser
- Can be replaced with a proper grammar later

### Pattern 3: Cargo Feature Gating for C++ Shim

```toml
# Cargo.toml
[features]
extension = ["duckdb/loadable-extension", "duckdb/vscalar"]
parser = ["extension"]  # parser implies extension
```

```rust
// build.rs
if std::env::var("CARGO_FEATURE_PARSER").is_ok() {
    cc::Build::new()
        .cpp(true)
        .file("shim/shim.cpp")
        .include("shim/include")  // duckdb.hpp location
        .flag_if_supported("-std=c++17")
        .compile("semantic_views_shim");
}
```

The `parser` feature gates the C++ compilation. Without it, the extension builds as pure Rust (C_STRUCT ABI). This preserves the ability to ship a pure-Rust extension if needed.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Dynamic C++ Symbol Resolution from Host

**What:** Trying to call `DBConfig::GetConfig()` or `DatabaseInstance::GetConfig()` by expecting the host DuckDB binary to export these symbols.

**Why bad:** Python DuckDB (and potentially other embeddings) compile with `-fvisibility=hidden`. These symbols are not in the dynamic symbol table. The extension fails to load.

**Instead:** Compile against the amalgamation header (`duckdb.hpp`). All needed DuckDB code is compiled INTO the extension binary. No runtime symbol resolution required.

### Anti-Pattern 2: Passing C++ Objects Across FFI Boundary

**What:** Trying to pass `std::string`, `unique_ptr`, or `Connection` objects from C++ to Rust.

**Why bad:** C++ objects have non-trivial destructors, move semantics, and ABI-dependent layouts that Rust cannot safely handle.

**Instead:** Convert to C types at the boundary: `const char*` + `size_t` for strings, raw `void*` or `duckdb_connection` for handles, C structs for structured data.

### Anti-Pattern 3: Two Entry Points Doing Different Things

**What:** Having `semantic_views_init` (C++) register parser hooks AND `semantic_views_init_c_api` (Rust) register table functions, with the caller needing to invoke both.

**Why bad:** DuckDB calls exactly one entry point based on footer ABI type. The other is never called.

**Instead:** One entry point delegates to the other. C++ entry calls Rust init for all shared logic.

---

## Build Order (Suggested Implementation Sequence)

### Phase 1: C++ Shim Scaffold (No Parser Hooks Yet)

1. Add `shim/shim.cpp` with `DUCKDB_CPP_EXTENSION_ENTRY` that calls `sv_init_rust()`
2. Add `sv_init_rust()` to `lib.rs` as `#[no_mangle] pub extern "C"` -- delegates to `init_extension()`
3. Update `build.rs` to compile `shim.cpp` via `cc` crate
4. Update `build.rs` symbol visibility to export `_semantic_views_init`
5. Update footer stamping to `ABI_TYPE = CPP`
6. **Validation:** `just build && just test-sql` -- all existing tests pass with the new entry point

This phase proves the C++ entry point works and Rust init is correctly invoked.

### Phase 2: Parser Hook Registration (Stub)

1. Register a `ParserExtension` in `shim.cpp` with stub `parse_function`
2. Stub returns `DISPLAY_ORIGINAL_ERROR` for all queries (pass-through)
3. **Validation:** All tests still pass (stub has no effect)

This phase proves parser hook registration works without breaking anything.

### Phase 3: Parse Function (Prefix Detection)

1. Add `src/parser.rs` with `sv_parse()` FFI function
2. Implement prefix detection: `CREATE [OR REPLACE] SEMANTIC VIEW`
3. If matched, extract view name and body, rewrite to `FROM create_semantic_view(...)`
4. Return `PARSE_SUCCESSFUL` with the rewritten SQL
5. **Validation:** `CREATE SEMANTIC VIEW` statements are recognized and rewritten

### Phase 4: Plan Function (Statement Execution)

1. Implement `sv_plan()` that returns a `ParserExtensionPlanResult`
2. The result contains the rewritten SQL statement for DuckDB to execute
3. **Validation:** `CREATE SEMANTIC VIEW` creates a view visible via `list_semantic_views()`

### Phase 5: DROP + OR REPLACE + IF NOT EXISTS

1. Extend parser to handle `DROP SEMANTIC VIEW`, `CREATE OR REPLACE`, `IF NOT EXISTS`
2. **Validation:** Full DDL surface works through native SQL syntax

### Phase 6: Integration Tests

1. Add sqllogictest tests for native DDL syntax
2. Verify both interfaces (scalar function + native DDL) create interoperable views
3. **Validation:** `just test-all` passes

---

## Scalability Considerations

| Concern | Impact | Mitigation |
|---------|--------|------------|
| Amalgamation compile time | `duckdb.hpp` is ~500K lines; first compile takes 30-60s | Only recompiles when `shim.cpp` or DuckDB version changes. `cc` crate caches object files. |
| Binary size | Amalgamation adds ~10-20MB to extension binary | Release builds use LTO + strip (already configured). Acceptable for a loadable extension. |
| DuckDB version coupling | CPP ABI type ties extension to exact DuckDB version | Already true for C_STRUCT_UNSTABLE. Pin DuckDB version in CI; monitor with existing version checker. |
| `parser` feature complexity | Two build modes (pure Rust vs Rust+C++) | Feature gating keeps the boundary clean. CI tests both modes. |

---

## Open Questions

1. **Does `ExtensionLoader` expose `GetExtensionInfo()` / `GetExtensionAccess()`?** -- Needs verification against DuckDB v1.4.4 source. If not, the Rust C API stub initialization path needs an alternative (e.g., extracting `duckdb_database` from `DatabaseInstance` and using it directly without stubs, or constructing a minimal `duckdb_extension_access` from C++ side).

2. **Can `plan_function` return a rewritten SQL statement directly?** -- The `ParserExtensionPlanResult` has a `function` field (TableFunction) but the prql pattern uses the operator extension bind path instead. For DDL, returning a TableFunction is the cleaner approach, but the exact mechanics need verification.

3. **DuckDB amalgamation sourcing:** -- The amalgamation header must match the pinned DuckDB version (1.4.4). Options: vendor in repo (~15MB), download in build.rs, or reference from duckdb-sys crate's bundled copy.

4. **Windows build:** -- The `cc` crate handles MSVC compilation, but `duckdb.hpp` may need specific MSVC flags. Linux and macOS are the primary targets; Windows support can follow.

---

## Sources

- [DuckDB C API Extensions PR #12682](https://github.com/duckdb/duckdb/pull/12682) -- ABI types, entry point dispatch
- [DuckDB Extension C API Stable PR #14992](https://github.com/duckdb/duckdb/pull/14992) -- C_STRUCT vs C_STRUCT_UNSTABLE
- [prql DuckDB extension](https://github.com/ywelsch/duckdb-prql) -- parse/plan/bind pattern, DUCKDB_CPP_EXTENSION_ENTRY usage
- [DuckDB parser_extension.hpp](https://github.com/duckdb/duckdb/blob/main/src/include/duckdb/parser/parser_extension.hpp) -- ParserExtension struct, function typedefs, result types
- [DuckDB Runtime-Extensible Parsers (CIDR 2025)](https://duckdb.org/pdf/CIDR2025-muehleisen-raasveldt-extensible-parsers.pdf) -- parse_function vs parser_override semantics
- [DuckDB stable C++ API](https://github.com/duckdb/duckdb-cpp-api) -- confirmed NO parser extension hooks in stable API
- [duckpgq extension](https://github.com/cwida/duckpgq-extension) -- CREATE PROPERTY GRAPH as DDL parser extension precedent
- [RBAC Extension RFC](https://gist.github.com/dufferzafar/f12081d4f32e640966d984b33e7077e6) -- plan_function returning TableFunction for DDL
- Project investigation: `_notes/parser-extension-investigation.md` -- prior art analysis, Phase 11 post-mortem
