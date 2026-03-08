# Phase 17: DDL Execution - Research

**Researched:** 2026-03-07
**Domain:** DuckDB parser extension plan_function, statement rewriting, C++/Rust FFI for DDL execution
**Confidence:** HIGH

## Summary

Phase 17 wires the parser hook chain (established in Phases 15-16) to the existing catalog code so that `CREATE SEMANTIC VIEW name (tables := [...], dimensions := [...], metrics := [...])` creates a view that is immediately queryable via `semantic_view()`. The critical technical challenge is that the `plan_function` in DuckDB's parser extension API returns a `ParserExtensionPlanResult` containing a `TableFunction` + `vector<Value>` positional parameters, which DuckDB binds via `BindTableFunction()`. This overload does NOT support named parameters (`named_parameter_map_t` is empty). Since the existing `create_semantic_view` table function requires named parameters (`tables :=`, `dimensions :=`, `metrics :=`) for its `LIST(STRUCT)` arguments, the plan function cannot simply return the existing registered table function.

The recommended approach for this spike is **statement rewriting via a dedicated DDL TableFunction in C++**. The plan function constructs a new internal `TableFunction` (not user-visible) whose bind callback receives the raw DDL body text as a single VARCHAR parameter. The bind callback then calls Rust via FFI to parse the DDL body, extract the view name and argument block, and perform the catalog insert using the existing `CatalogState` machinery. This avoids the named parameter limitation entirely and keeps all parsing logic in Rust.

The parse function (already in Rust) needs minimal extension: instead of just detecting the `CREATE SEMANTIC VIEW` prefix, it also needs to extract and forward the full query text to the plan function via `SemanticViewParseData`. This is already done -- Phase 16 carries the raw query text in `SemanticViewParseData.query`.

**Primary recommendation:** Replace `sv_plan_stub` with a real plan function that returns an internal `TableFunction`. The TableFunction's bind callback passes the DDL text to Rust via FFI. Rust parses the text, extracts view name + body, and calls the same catalog insert logic used by `create_semantic_view()`. BUILD-03 is verified by running the extension under both DuckDB CLI and Python.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DDL-01 | `CREATE SEMANTIC VIEW name (tables := [...], dimensions := [...], metrics := [...])` creates a semantic view via parser hook -> plan function -> existing catalog code | Plan function returns internal TableFunction; bind callback delegates to Rust for DDL body parsing and catalog insert. Full flow: parse_function detects prefix -> plan_function returns TableFunction -> bind callback -> Rust FFI -> catalog_insert |
| DDL-02 | View created via native DDL is queryable via `FROM semantic_view('name', dimensions := [...], metrics := [...])` | Both DDL paths write to the same CatalogState HashMap. Views created via native DDL are indistinguishable from those created via function-based DDL. |
| DDL-03 | Existing function-based DDL (`FROM create_semantic_view(...)`) continues to work alongside native DDL | Parser hook is a fallback -- only fires when DuckDB's parser fails. `FROM create_semantic_view(...)` parses successfully as a standard table function call, so the hook never fires for it. No interference. |
| BUILD-03 | Extension binary loads successfully via `LOAD` in DuckDB CLI and Python client | Verified by running `just test-sql` (DuckDB CLI via sqllogictest runner) and `just test-ducklake-ci` (Python DuckDB). Both already work for Phase 16; Phase 17 must not break them. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| DuckDB amalgamation (duckdb.hpp + duckdb.cpp) | v1.4.4 | C++ types for `ParserExtensionPlanResult`, `TableFunction`, `Value` | Already compiled in Phase 15; provides `TableFunction` class needed for plan function |
| cc (Rust crate) | 1.x | Compiles shim.cpp | Already in use |
| serde_json | 1.x | JSON serialization for `SemanticViewDefinition` | Already a dependency; used by existing `catalog_insert` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| libduckdb-sys | =1.4.4 | FFI types (`duckdb_connection`, `duckdb_database`) | Already used; needed for Rust-side DDL execution via persist_conn |
| duckdb (Rust crate) | =1.4.4 | High-level Rust API | Already used; `Connection` for catalog init |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Internal TableFunction in C++ | ClientContext::Query() in plan_function | ClientContext holds execution lock during binding -- calling Query() would deadlock |
| Positional VARCHAR param (raw DDL text) | Named parameters via custom BindTableFunction call | BindTableFunction overload from extension binder does NOT support named params |
| Rust-side DDL parsing from raw SQL text | Re-using DuckDB's own expression parser | DuckDB's parser can't parse `tables := [...]` as a standalone expression -- it's table function syntax |

## Architecture Patterns

### The Plan Function Cannot Use Named Parameters

**Critical finding (HIGH confidence):** DuckDB's `Binder::Bind(ExtensionStatement&)` calls `BindTableFunction(parse_result.function, std::move(parse_result.parameters))`. This overload creates an **empty** `named_parameter_map_t`:

```cpp
// Source: DuckDB v1.4.4 src/planner/binder/tableref/bind_table_function.cpp
unique_ptr<LogicalOperator> Binder::BindTableFunction(TableFunction &function,
                                                      vector<Value> parameters) {
    named_parameter_map_t named_parameters;  // EMPTY -- no named params
    // ...
    return BindTableFunctionInternal(function, ref, std::move(parameters),
                                     std::move(named_parameters), ...);
}
```

This means the plan function's `result.parameters` vector provides only positional parameters. The existing `create_semantic_view` table function requires named parameters (`tables :=`, `dimensions :=`, `metrics :=`) because DuckDB cannot infer STRUCT types from empty list literals. Therefore, the plan function CANNOT simply return the existing registered table function.

### Recommended Architecture: Internal DDL TableFunction

```
User types: CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...], metrics := [...])
     |
     v
[DuckDB parser fails at "SEMANTIC"]
     |
     v
[sv_parse_stub] -- C++ trampoline
     | calls sv_parse_rust() -- detects prefix, returns PARSE_DETECTED
     | wraps raw query text in SemanticViewParseData
     v
[sv_plan_function] -- C++ plan function (NEW in Phase 17)
     | receives SemanticViewParseData with raw query text
     | constructs ParserExtensionPlanResult with internal TableFunction
     | passes raw query text as VARCHAR positional parameter
     v
[DuckDB binder calls BindTableFunction]
     | binds the internal TableFunction with the VARCHAR parameter
     v
[sv_ddl_bind] -- C++ bind callback (NEW in Phase 17)
     | receives query text from input.inputs[0]
     | calls Rust via FFI: sv_execute_ddl_rust(query_ptr, query_len, ...)
     v
[Rust sv_execute_ddl_rust] -- (NEW in Phase 17)
     | parses DDL text: extracts view name + body
     | rewrites to: SELECT * FROM create_semantic_view('name', body...)
     | executes rewritten SQL via persist_conn (separate connection)
     | inserts into in-memory catalog
     v
[sv_ddl_execute] -- C++ execute callback
     | outputs single row with view name (confirms success)
```

### Pattern 1: DDL Text Parsing in Rust

**What:** Rust receives the raw DDL text (e.g., `CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...], metrics := [...])`), extracts the view name and the parenthesized body, then rewrites it into the function-based DDL form for execution via a separate connection.

**When to use:** This spike phase. A future phase could parse the body directly into `SemanticViewDefinition` without the rewrite.

**Why rewrite instead of direct parsing:** The DDL body uses the same syntax as the function-based DDL (`tables := [{...}], dimensions := [{...}]`). By rewriting to `SELECT * FROM create_semantic_view('name', tables := [...], ...)` and executing via a separate connection, we reuse 100% of the existing argument parsing and catalog logic without duplicating it. The rewrite approach is explicitly recommended by the project architecture research (Option 3).

**Example rewrite:**
```
Input:  CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...], metrics := [...])
Output: SELECT * FROM create_semantic_view('sales', tables := [...], dimensions := [...], metrics := [...])
```

The rewrite is straightforward string manipulation:
1. Strip `CREATE SEMANTIC VIEW ` prefix
2. Extract view name (next token)
3. Extract body (everything from first `(` to last `)`)
4. Construct: `SELECT * FROM create_semantic_view('{name}', {body})`

### Pattern 2: Executing Rewritten SQL via Separate Connection

**What:** The Rust DDL handler executes the rewritten SQL via the existing `persist_conn` (or a new DDL-specific connection), NOT via `ClientContext`.

**Why NOT ClientContext:** During binding, DuckDB holds the `context_lock` on the current `ClientContext`. Calling `ClientContext::Query()` from within the bind callback would attempt to re-acquire the same lock, causing a deadlock. This is the same pattern that required `persist_conn` in the original DDL implementation (Phase 10).

**Implementation:** The Rust DDL handler receives a `duckdb_connection` handle (the same persist_conn or a new connection) and uses `ffi::duckdb_query()` to execute the rewritten SQL. This is the same pattern used by `persist_define()` in `src/ddl/define.rs`.

### Pattern 3: C++ Plan Function Construction

**What:** The C++ `sv_plan_function` replaces `sv_plan_stub`. It extracts the query text from `SemanticViewParseData`, constructs a `ParserExtensionPlanResult` with an internal `TableFunction`, and passes the query text as a positional VARCHAR parameter.

**Example:**
```cpp
// Source: Pattern derived from DuckDB loadable_extension_demo.cpp QuackPlanFunction
static ParserExtensionPlanResult sv_plan_function(
    ParserExtensionInfo *, ClientContext &,
    unique_ptr<ParserExtensionParseData> parse_data) {
    auto &sv_data = static_cast<SemanticViewParseData &>(*parse_data);

    ParserExtensionPlanResult result;
    result.function = TableFunction("sv_ddl_internal", {LogicalType::VARCHAR},
                                    sv_ddl_execute, sv_ddl_bind,
                                    sv_ddl_init_global);
    result.parameters.push_back(Value(sv_data.query));
    result.requires_valid_transaction = true;
    result.return_type = StatementReturnType::QUERY_RESULT;
    return result;
}
```

### Pattern 4: C++ Bind Callback Delegates to Rust via FFI

**What:** The internal TableFunction's bind callback receives the raw DDL text and calls a Rust FFI function to parse and execute the DDL.

**Example:**
```cpp
extern "C" {
    // Returns: 0 = success, non-zero = error
    // On success, name_out is filled with the view name (null-terminated)
    uint8_t sv_execute_ddl_rust(
        const char *query_ptr, size_t query_len,
        duckdb_connection exec_conn,  // connection for executing rewritten SQL
        char *name_out, size_t name_out_len,
        char *error_out, size_t error_out_len);
}

static unique_ptr<FunctionData> sv_ddl_bind(
    ClientContext &, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("view_name");

    auto query = StringValue::Get(input.inputs[0]);
    // ... call Rust FFI to parse and execute DDL ...
    return make_uniq<SvDdlBindData>(view_name);
}
```

### Anti-Patterns to Avoid

- **Calling ClientContext::Query() from bind/plan:** Deadlocks due to context_lock. Use a separate connection.
- **Trying to pass named parameters via ParserExtensionPlanResult:** The BindTableFunction overload from the extension binder does not support named parameters. Use positional parameters only.
- **Parsing the DDL body in C++:** All parsing logic should be in Rust for testability under `cargo test`. C++ is just a trampoline.
- **Creating a new connection per DDL statement:** Use the existing persist_conn pattern -- one connection created at init time, reused for all DDL writes.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DDL body argument parsing | Custom parser for `tables := [...]` | Rewrite to `create_semantic_view()` call and execute via DuckDB | DuckDB's own parser handles the complex STRUCT/LIST literal syntax perfectly |
| Catalog insert logic | Direct HashMap + persistence code | `create_semantic_view()` table function (via rewritten SQL) | Already handles type inference, persistence, duplicate detection, or_replace, if_not_exists |
| Named parameter binding from plan_function | Custom BindTableFunction call with named params | Positional VARCHAR parameter carrying raw DDL text | BindTableFunction from extension binder only supports positional params |
| Connection management for DDL execution | New connection per statement | Existing persist_conn pattern from init_extension() | Connection reuse avoids overhead; pattern already proven in Phase 10 |

**Key insight:** The statement rewrite approach means Phase 17 adds NO new parsing logic for DDL arguments. All argument parsing is handled by the existing `parse_define_args_from_bind` function in `src/ddl/parse_args.rs`, invoked by `DefineSemanticViewVTab::bind`. The rewrite simply transforms the syntax to match what the existing function expects.

## Common Pitfalls

### Pitfall 1: Deadlock from ClientContext::Query() in Bind
**What goes wrong:** The bind callback tries to execute SQL via `ClientContext::Query()`. DuckDB holds `context_lock` during binding. The Query call tries to acquire the same lock. Deadlock.
**Why it happens:** Natural instinct to use the `ClientContext` that's available in the bind signature.
**How to avoid:** Use a separate `duckdb_connection` (persist_conn) and `ffi::duckdb_query()` for executing the rewritten SQL. This is the same pattern used by `persist_define()` in `src/ddl/define.rs`.
**Warning signs:** Extension hangs indefinitely when `CREATE SEMANTIC VIEW` is executed.

### Pitfall 2: Passing persist_conn to the Plan Function
**What goes wrong:** The plan function needs to pass a `duckdb_connection` handle to the TableFunction's bind callback, but there's no obvious way to thread state through the plan function -> BindTableFunction -> bind callback chain.
**Why it happens:** The plan function returns a `ParserExtensionPlanResult` containing a `TableFunction` and `parameters`. The bind callback receives `TableFunctionBindInput` but not arbitrary user data.
**How to avoid:** Use one of these approaches:
  1. **Global/static state:** Store the connection handle in a static variable (initialized at extension load time). The bind callback reads it. Thread-safe because DuckDB serializes extension binding.
  2. **ParserExtensionInfo:** The `ParserExtension` struct has a `parser_info` field (`shared_ptr<ParserExtensionInfo>`) that flows through parse -> plan. However, accessing it from the bind callback is not straightforward.
  3. **Encode in parameters:** Pass the connection handle as an opaque uint64 in the parameters vector (cast pointer to integer). The bind callback casts it back. Ugly but effective for a spike.
  4. **C++ global capturing:** Define the bind callback as a closure or use a file-scope static that the entry point initializes.
**Recommendation:** Option 1 (file-scope static) or option 4 (C++ global set at init time). The connection handle is stable for the extension's lifetime.

### Pitfall 3: DDL Body Extraction Off-by-One
**What goes wrong:** The Rust DDL parser incorrectly extracts the view name or body from the raw text, leading to malformed SQL in the rewrite.
**Why it happens:** Edge cases in the DDL syntax: view names with special characters, nested parentheses in STRUCT literals, quoted strings containing parentheses.
**How to avoid:** The rewrite does NOT need to parse the body -- it only needs to:
  1. Strip the `CREATE SEMANTIC VIEW ` prefix (already done by detect function)
  2. Find the view name (next whitespace-delimited token)
  3. Find the opening `(` and last `)`
  4. Everything between them is the body -- pass through verbatim
**Warning signs:** `create_semantic_view` errors about malformed arguments.

### Pitfall 4: In-Memory vs File-Backed Catalog Divergence
**What goes wrong:** The rewritten SQL executes on a separate connection (persist_conn). For in-memory databases, persist_conn is `None`. The rewrite must still work for in-memory DBs.
**Why it happens:** The existing DDL function handles both paths (in-memory HashMap only vs file-backed HashMap + table persistence). The rewrite needs the same logic.
**How to avoid:** Execute the rewritten SQL on the SAME connection that DuckDB uses for the current statement. Actually, this is incorrect -- we need a separate connection to avoid deadlocks. For in-memory databases, the DDL handler should fall back to direct catalog manipulation (parsing the body in Rust and calling `catalog_insert` directly). OR: always create a DDL execution connection at init time, even for in-memory databases.
**Recommendation:** Create a dedicated DDL execution connection at init time (for both file-backed and in-memory). This simplifies the code path -- the rewritten SQL always executes on this connection.

### Pitfall 5: Existing Tests Regressing
**What goes wrong:** Modifying `sv_plan_stub` breaks the existing Phase 16 sqllogictest.
**Why it happens:** The Phase 16 test (`phase16_parser.test`) expects `sv_plan_stub` to return "CREATE SEMANTIC VIEW stub fired". Replacing it with a real plan function changes the behavior.
**How to avoid:** Update `phase16_parser.test` to exercise the new behavior (real DDL execution) instead of the stub. The test should create a semantic view via native DDL and verify it's queryable.
**Warning signs:** `just test-sql` fails on `phase16_parser.test` after the change.

### Pitfall 6: Symbol Visibility for New FFI Functions
**What goes wrong:** New Rust FFI functions (e.g., `sv_execute_ddl_rust`) are not visible to the C++ shim at link time.
**Why it happens:** The build.rs symbol visibility list only exports `semantic_views_init_c_api`. Internal FFI functions between C++ and Rust within the same cdylib don't need to be in the export list -- they resolve at link time within the binary.
**How to avoid:** Rust `extern "C"` functions with `#[no_mangle]` are visible to C++ within the same binary without export list changes. The export list controls what's visible to the DuckDB host process.
**Warning signs:** Linker errors about undefined symbols.

## Code Examples

### DDL Text Parsing in Rust (Pure Function)

```rust
// Source: New function for Phase 17 -- pattern derived from existing
// detect_create_semantic_view in src/parse.rs

/// Parse a `CREATE SEMANTIC VIEW` statement into its components.
///
/// Input:  "CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])"
/// Output: Ok(("sales", "tables := [...], dimensions := [...]"))
///
/// Returns Err if the statement is malformed.
pub fn parse_ddl_text(query: &str) -> Result<(&str, &str), String> {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();

    // Strip "CREATE SEMANTIC VIEW " prefix (case-insensitive)
    let prefix = "create semantic view";
    if trimmed.len() < prefix.len()
        || !trimmed.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        return Err("Not a CREATE SEMANTIC VIEW statement".into());
    }
    let after_prefix = trimmed[prefix.len()..].trim_start();

    // Extract view name: everything up to first '(' or whitespace
    let name_end = after_prefix
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_prefix.len());
    let name = &after_prefix[..name_end];
    if name.is_empty() {
        return Err("Missing view name".into());
    }

    // Extract body: everything from first '(' to last ')'
    let rest = after_prefix[name_end..].trim_start();
    if !rest.starts_with('(') {
        return Err("Expected '(' after view name".into());
    }
    let body_start = 1; // skip the '('
    let body_end = rest.rfind(')').ok_or("Missing closing ')'")?;
    let body = &rest[body_start..body_end];

    Ok((name, body.trim()))
}

/// Rewrite a CREATE SEMANTIC VIEW statement to the function-based DDL form.
///
/// Input:  "CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])"
/// Output: "SELECT * FROM create_semantic_view('sales', tables := [...], dimensions := [...])"
pub fn rewrite_ddl_to_function_call(query: &str) -> Result<String, String> {
    let (name, body) = parse_ddl_text(query)?;
    // Escape single quotes in view name
    let safe_name = name.replace('\'', "''");
    Ok(format!(
        "SELECT * FROM create_semantic_view('{safe_name}', {body})"
    ))
}
```

### C++ Plan Function (Replaces sv_plan_stub)

```cpp
// Source: Pattern from DuckDB loadable_extension_demo.cpp QuackPlanFunction
// + existing sv_plan_stub in cpp/src/shim.cpp

// Forward-declare Rust DDL execution function
extern "C" {
    uint8_t sv_execute_ddl_rust(
        const char *query_ptr, size_t query_len,
        duckdb_connection exec_conn,
        char *name_out, size_t name_out_len,
        char *error_out, size_t error_out_len);
}

// File-scope static: set during sv_register_parser_hooks
static duckdb_connection sv_ddl_conn = nullptr;

// Bind data: holds the view name returned by DDL execution
struct SvDdlBindData : public FunctionData {
    string view_name;
    explicit SvDdlBindData(string name) : view_name(std::move(name)) {}
    unique_ptr<FunctionData> Copy() const override {
        return make_uniq<SvDdlBindData>(view_name);
    }
    bool Equals(const FunctionData &other) const override {
        return view_name == other.Cast<SvDdlBindData>().view_name;
    }
};

static unique_ptr<FunctionData> sv_ddl_bind(
    ClientContext &, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("view_name");

    auto query = StringValue::Get(input.inputs[0]);

    char name_buf[256] = {0};
    char error_buf[1024] = {0};
    uint8_t rc = sv_execute_ddl_rust(
        query.c_str(), query.size(),
        sv_ddl_conn,
        name_buf, sizeof(name_buf),
        error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException("CREATE SEMANTIC VIEW failed: %s", error_buf);
    }
    return make_uniq<SvDdlBindData>(string(name_buf));
}

// Global state + execute: same pattern as sv_stub
struct SvDdlGlobalState : public GlobalTableFunctionState {
    bool done = false;
};

static unique_ptr<GlobalTableFunctionState> sv_ddl_init_global(
    ClientContext &, TableFunctionInitInput &) {
    return make_uniq<SvDdlGlobalState>();
}

static void sv_ddl_execute(ClientContext &, TableFunctionInput &input,
                           DataChunk &output) {
    auto &state = input.global_state->Cast<SvDdlGlobalState>();
    if (state.done) {
        output.SetCardinality(0);
        return;
    }
    state.done = true;
    auto &bind_data = input.bind_data->Cast<SvDdlBindData>();
    output.SetCardinality(1);
    output.SetValue(0, 0, Value(bind_data.view_name));
}

// Plan function: replaces sv_plan_stub
static ParserExtensionPlanResult sv_plan_function(
    ParserExtensionInfo *, ClientContext &,
    unique_ptr<ParserExtensionParseData> parse_data) {
    auto &sv_data = static_cast<SemanticViewParseData &>(*parse_data);

    ParserExtensionPlanResult result;
    result.function = TableFunction("sv_ddl_internal",
                                    {LogicalType::VARCHAR},
                                    sv_ddl_execute, sv_ddl_bind,
                                    sv_ddl_init_global);
    result.parameters.push_back(Value(sv_data.query));
    result.requires_valid_transaction = true;
    result.return_type = StatementReturnType::QUERY_RESULT;
    return result;
}
```

### Rust FFI DDL Execution Function

```rust
// Source: New function for Phase 17 -- pattern from existing persist_define
// in src/ddl/define.rs and sv_parse_rust in src/parse.rs

/// Execute a CREATE SEMANTIC VIEW DDL statement by rewriting it to the
/// function-based DDL form and executing via a separate connection.
///
/// # Safety
///
/// `query_ptr` must point to valid UTF-8 of `query_len` bytes.
/// `exec_conn` must be a valid duckdb_connection.
/// `name_out` and `error_out` must point to writable buffers.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_execute_ddl_rust(
    query_ptr: *const u8,
    query_len: usize,
    exec_conn: ffi::duckdb_connection,
    name_out: *mut u8,
    name_out_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
) -> u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // ... parse DDL text, rewrite, execute via exec_conn ...
    }))
    .unwrap_or(1) // Return error code on panic
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `sv_plan_stub` returns "stub fired" string | `sv_plan_function` returns DDL-executing TableFunction | Phase 17 (this phase) | Enables end-to-end DDL execution via parser hook |
| Detection-only parse function | Parse function carries raw query text through to plan | Phase 16 (already done) | SemanticViewParseData.query carries the text |
| No DDL connection for parser hook path | DDL connection passed through static/global to plan function's TableFunction | Phase 17 (this phase) | Avoids ClientContext deadlock |

**Deprecated/outdated:**
- `sv_plan_stub` with its stub bind/execute callbacks will be fully replaced
- The stub return message "CREATE SEMANTIC VIEW stub fired" will no longer appear
- `phase16_parser.test` must be updated to test real DDL behavior

## Open Questions

1. **How to pass duckdb_connection to the DDL bind callback**
   - What we know: The plan function returns a `ParserExtensionPlanResult` with a `TableFunction` + `vector<Value>` parameters. The bind callback receives `TableFunctionBindInput` with only the parameter values.
   - What's unclear: Whether passing a pointer-as-integer through `result.parameters` is safe, or whether a file-scope static is more appropriate.
   - Recommendation: File-scope static `duckdb_connection` set during `sv_register_parser_hooks`. This is safe because the connection is stable for the extension's lifetime and DuckDB serializes extension binding. **Confidence: HIGH** -- this is the simplest approach and avoids pointer-as-integer hackery.

2. **In-memory database DDL execution**
   - What we know: For in-memory databases, `persist_conn` is currently `None`. The rewrite approach executes SQL on a separate connection.
   - What's unclear: Whether executing `create_semantic_view()` on a separate connection for an in-memory database correctly populates the same CatalogState HashMap.
   - Recommendation: Create a dedicated DDL execution connection at init time (even for in-memory). The `create_semantic_view` table function writes to the CatalogState which is shared across connections (it's an Arc<Mutex<HashMap>>). The separate connection only needs to see the registered `create_semantic_view` table function, which is registered at extension init time.
   - **Critical verification needed:** Confirm that a table function registered via `con.register_table_function_with_extra_info()` on one connection is visible to queries executed on a different connection to the same database. **Confidence: MEDIUM** -- DuckDB table function registration is database-scoped, not connection-scoped, so this should work. Needs empirical verification.

3. **Error propagation from Rust to C++ bind callback**
   - What we know: The Rust DDL function executes SQL and may encounter errors (duplicate view name, malformed arguments, etc.).
   - What's unclear: The exact error message format needed.
   - Recommendation: Write error message to the `error_out` buffer and return non-zero. C++ bind throws `BinderException` with the error text. DuckDB displays it as a standard error message.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (Python runner) + cargo test (Rust) + DuckLake CI (Python) |
| Config file | `test/sql/TEST_LIST` (sqllogictest), `Cargo.toml` (Rust tests) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DDL-01 | CREATE SEMANTIC VIEW creates a view | integration (sqllogictest) | `just test-sql` | Wave 0 -- update phase16_parser.test |
| DDL-02 | View created via native DDL is queryable | integration (sqllogictest) | `just test-sql` | Wave 0 -- new test case in phase16_parser.test |
| DDL-03 | Function-based DDL still works | integration (sqllogictest) | `just test-sql` | Existing: phase2_ddl.test, phase4_query.test |
| BUILD-03 | LOAD in CLI and Python | integration | `just test-sql && just test-ducklake-ci` | Existing: semantic_views.test + DuckLake CI |

### Sampling Rate
- **Per task commit:** `cargo test` (Rust unit tests, including parse module tests)
- **Per wave merge:** `just test-all` (full suite: Rust + sqllogictest + DuckLake CI)
- **Phase gate:** `just test-all` green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Update `test/sql/phase16_parser.test` -- replace stub assertions with real DDL execution tests (create view, query it, verify results)
- [ ] Add Rust unit tests for `parse_ddl_text()` and `rewrite_ddl_to_function_call()` in `src/parse.rs`
- [ ] Verify `just test-ducklake-ci` passes (BUILD-03: Python client compatibility)

## Sources

### Primary (HIGH confidence)
- DuckDB v1.4.4 amalgamation header (`duckdb.hpp`) -- `ParserExtensionPlanResult`, `TableFunction`, `Value`, `StatementReturnType` class definitions
- DuckDB v1.4.4 `bind_extension.cpp` -- confirms `BindTableFunction(function, parameters)` is called with empty named_parameter_map
- DuckDB v1.4.4 `bind_table_function.cpp` -- confirms the overload creates empty `named_parameter_map_t`
- DuckDB v1.4.4 `loadable_extension_demo.cpp` -- `QuackPlanFunction` pattern for constructing `ParserExtensionPlanResult` with TableFunction + parameters
- Existing codebase: `src/ddl/define.rs` -- `persist_define()` pattern for executing SQL via separate connection
- Existing codebase: `src/parse.rs` -- `detect_create_semantic_view()` and `sv_parse_rust()` FFI pattern
- Existing codebase: `cpp/src/shim.cpp` -- `SemanticViewParseData`, `sv_parse_stub`, `sv_plan_stub`, `sv_register_parser_hooks`
- [Project architecture research](.planning/research/ARCHITECTURE.md) -- Option 3 (passthrough rewrite) recommendation
- [Project feature research](.planning/research/FEATURES.md) -- ParserExtensionPlanResult fields, BindTableFunction flow

### Secondary (MEDIUM confidence)
- [DuckDB Runtime-Extensible Parsers (CIDR 2025)](https://duckdb.org/pdf/CIDR2025-muehleisen-raasveldt-extensible-parsers.pdf) -- parse_function vs parser_override semantics
- [DuckDB parser_tools extension](https://duckdb.org/community_extensions/extensions/parser_tools) -- Community extension using parser hooks
- [DuckDB TPC-H extension](https://github.com/duckdb/duckdb/blob/main/extension/tpch/tpch_extension.cpp) -- ClientContext usage patterns in extensions

### Tertiary (LOW confidence)
- Cross-connection table function visibility -- inferred from DuckDB's database-scoped function registration but not empirically verified

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in use, no new dependencies
- Architecture (plan function -> TableFunction): HIGH -- verified from DuckDB source that BindTableFunction uses only positional params; QuackPlanFunction pattern confirmed working
- Architecture (statement rewrite): HIGH -- Option 3 from project architecture research, well-understood string manipulation
- Architecture (connection passing): MEDIUM -- file-scope static approach is standard C pattern but cross-connection function visibility needs verification
- Pitfalls: HIGH -- ClientContext deadlock documented in Phase 10; persist_conn pattern proven
- DDL text parsing: HIGH -- simple prefix strip + parenthesis extraction; no complex grammar needed

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable -- DuckDB v1.4.4 is a pinned version)
