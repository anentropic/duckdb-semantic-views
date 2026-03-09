# Architecture: v0.5.1 DDL Polish — Extended DDL Surface + Error Reporting

**Project:** DuckDB Semantic Views Extension (v0.5.1)
**Researched:** 2026-03-08
**Scope:** How DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW integrate with the existing parser hook architecture; how error location reporting works within the Rust FFI parse function; what data flow changes are needed
**Confidence:** HIGH -- all new features use patterns already proven in v0.5.0, with no new architectural unknowns

---

## Current Architecture (v0.5.0 Baseline)

The v0.5.0 parser extension spike established a two-layer DDL architecture:

```
User SQL Input
    |
    v
DuckDB Native Parser
    |
    +--[parses OK]---> Standard DuckDB execution
    |                  (includes function-based DDL:
    |                   create_semantic_view(), drop_semantic_view(), etc.)
    |
    +--[parse fails]--> Parser Extension Fallback Chain
                        |
                        v
                   sv_parse_stub (C++, shim.cpp)
                        |
                        v
                   sv_parse_rust (Rust FFI, parse.rs)
                        |
                   Detects "CREATE SEMANTIC VIEW" prefix?
                        |
                   +--[no]--> DISPLAY_ORIGINAL_ERROR (pass through)
                   |
                   +--[yes]--> PARSE_SUCCESSFUL + SemanticViewParseData
                                    |
                                    v
                              sv_plan_function (C++, shim.cpp)
                                    |
                              Returns TableFunction "sv_ddl_internal"
                              with original query text as parameter
                                    |
                                    v
                              sv_ddl_bind (C++, shim.cpp)
                                    |
                              Calls sv_execute_ddl_rust (Rust FFI)
                                    |
                              Rewrites: CREATE SEMANTIC VIEW name (body)
                              Into:     SELECT * FROM create_semantic_view('name', body)
                                    |
                              Executes via sv_ddl_conn (dedicated connection)
                                    |
                                    v
                              Existing VTab bind path
                              (DefineSemanticViewVTab in define.rs)
```

**Key observation:** The current architecture already has separate Rust functions in `parse.rs`:

| Function | Purpose | Signature |
|----------|---------|-----------|
| `detect_create_semantic_view(query)` | Prefix detection (0/1) | `&str -> u8` |
| `parse_ddl_text(query)` | Extract `(name, body)` | `&str -> Result<(&str, &str), String>` |
| `rewrite_ddl_to_function_call(query)` | Full rewrite to function call | `&str -> Result<String, String>` |
| `sv_parse_rust(ptr, len)` | FFI: detection trampoline | `*const u8, usize -> u8` |
| `sv_execute_ddl_rust(...)` | FFI: rewrite + execute | Multiple params -> u8 |

The function-based DDL layer (registered at extension init) already has all the variants:

| Function name | Behavior |
|---------------|----------|
| `create_semantic_view` | INSERT, error on duplicate |
| `create_or_replace_semantic_view` | UPSERT, overwrite existing |
| `create_semantic_view_if_not_exists` | INSERT, silent on duplicate |
| `drop_semantic_view` | DELETE, error if not found |
| `drop_semantic_view_if_exists` | DELETE, silent if not found |
| `list_semantic_views` | List all views |
| `describe_semantic_view` | Describe one view |

**The function layer is complete. The parser hook layer only exposes `CREATE SEMANTIC VIEW`.**

---

## Target Architecture (v0.5.1)

The new features extend the parse layer to detect and rewrite more statement variants, without touching the function layer or the plan/bind/execute layer.

### Statement Detection Matrix

| Native DDL Syntax | Rewrites To | Function Already Registered |
|-------------------|-------------|---------------------------|
| `CREATE SEMANTIC VIEW name (...)` | `SELECT * FROM create_semantic_view('name', ...)` | Yes (v0.5.0) |
| `CREATE OR REPLACE SEMANTIC VIEW name (...)` | `SELECT * FROM create_or_replace_semantic_view('name', ...)` | Yes (lib.rs:366) |
| `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | `SELECT * FROM create_semantic_view_if_not_exists('name', ...)` | Yes (lib.rs:378) |
| `DROP SEMANTIC VIEW name` | `SELECT * FROM drop_semantic_view('name')` | Yes (lib.rs:389) |
| `DROP SEMANTIC VIEW IF EXISTS name` | `SELECT * FROM drop_semantic_view_if_exists('name')` | Yes (lib.rs:400) |
| `DESCRIBE SEMANTIC VIEW name` | `SELECT * FROM describe_semantic_view('name')` | Yes (lib.rs:412) |
| `SHOW SEMANTIC VIEWS` | `SELECT * FROM list_semantic_views()` | Yes (lib.rs:406) |

**Every new native DDL statement rewrites to an existing registered table function.** No new VTab implementations are needed.

---

## Integration Points: What Changes Where

### 1. `src/parse.rs` -- MODIFY (Primary Change)

The detection function (`detect_create_semantic_view`) and the rewrite function must be extended to handle all seven statement forms. This is the largest code change.

#### Detection: Multi-Prefix Matching

The current detection is a single prefix match. It must become a multi-prefix dispatcher:

```
Input:  raw query string (from DuckDB parser fallback)
Output: StatementKind enum variant + u8 detection flag

StatementKind:
  CreateSemanticView           -> "create semantic view"
  CreateOrReplaceSemanticView  -> "create or replace semantic view"
  CreateIfNotExists            -> "create semantic view if not exists"
  DropSemanticView             -> "drop semantic view"
  DropIfExists                 -> "drop semantic view if exists"
  DescribeSemanticView         -> "describe semantic view"
  ShowSemanticViews            -> "show semantic views"
  NotOurs                      -> fallback
```

**Detection order matters.** Longer prefixes must be checked before shorter ones:
1. `create or replace semantic view` (36 chars) -- before `create semantic view`
2. `create semantic view if not exists` (35 chars) -- before `create semantic view`
3. `create semantic view` (20 chars) -- after the above two
4. `drop semantic view if exists` (28 chars) -- before `drop semantic view`
5. `drop semantic view` (19 chars) -- after the above
6. `describe semantic view` (22 chars)
7. `show semantic views` (19 chars)

#### Rewrite: Statement-Specific Function Call Generation

Each statement kind has a different rewrite shape:

**CREATE variants** (have a body with parenthesized clauses):
```
CREATE SEMANTIC VIEW name (body)
  -> SELECT * FROM create_semantic_view('name', body)

CREATE OR REPLACE SEMANTIC VIEW name (body)
  -> SELECT * FROM create_or_replace_semantic_view('name', body)

CREATE SEMANTIC VIEW IF NOT EXISTS name (body)
  -> SELECT * FROM create_semantic_view_if_not_exists('name', body)
```

**DROP variants** (name only, no body):
```
DROP SEMANTIC VIEW name
  -> SELECT * FROM drop_semantic_view('name')

DROP SEMANTIC VIEW IF EXISTS name
  -> SELECT * FROM drop_semantic_view_if_exists('name')
```

**DESCRIBE** (name only):
```
DESCRIBE SEMANTIC VIEW name
  -> SELECT * FROM describe_semantic_view('name')
```

**SHOW** (no arguments):
```
SHOW SEMANTIC VIEWS
  -> SELECT * FROM list_semantic_views()
```

#### Proposed Internal API

```rust
/// Recognized statement kinds for the parser hook.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatementKind {
    Create,
    CreateOrReplace,
    CreateIfNotExists,
    Drop,
    DropIfExists,
    Describe,
    ShowAll,
}

/// Detect which (if any) semantic view statement this query is.
/// Returns None for non-matching queries.
pub fn detect_statement(query: &str) -> Option<StatementKind> { ... }

/// Rewrite a detected statement into its function-call equivalent.
/// Requires: detect_statement(query) returned Some(_).
pub fn rewrite_to_function_call(query: &str) -> Result<String, ParseError> { ... }
```

The existing `detect_create_semantic_view` and `rewrite_ddl_to_function_call` can be refactored into this wider API. The old functions can remain as thin wrappers for backward compatibility in tests if desired, or be replaced entirely.

### 2. `src/parse.rs` -- FFI Functions: MODIFY

The current FFI functions need adjustment:

**`sv_parse_rust`** -- Currently returns `u8` (0 or 1) for "is this CREATE SEMANTIC VIEW?". Must now return 1 for ALL seven statement kinds. Since any match means the parser hook should claim the statement, the return type stays the same: `0 = not ours, 1 = ours`.

The C++ side does not need to know which kind it is. The kind is determined again in `sv_execute_ddl_rust` when the full query text is rewritten.

**`sv_execute_ddl_rust`** -- Currently calls `rewrite_ddl_to_function_call`, which only handles CREATE. Must now call the new `rewrite_to_function_call` which handles all seven forms. The output contract changes slightly:
- For CREATE variants: `name_out` = view name, return 0
- For DROP variants: `name_out` = view name, return 0
- For DESCRIBE: `name_out` = view name, return 0
- For SHOW: `name_out` = "semantic_views" (or empty), return 0

The C++ `sv_ddl_bind` receives the name in `name_buf` for the single-row output. For SHOW, the output schema is different (two columns: name + base_table), so SHOW needs special handling. Two options:

**Option A (recommended): SHOW bypasses sv_ddl_bind entirely.** The rewrite `SELECT * FROM list_semantic_views()` is a standard table function call that DuckDB can parse natively. The parse function could rewrite SHOW and then return `DISPLAY_ORIGINAL_ERROR` to let DuckDB re-parse the rewritten SQL. But this is not how the fallback mechanism works -- the parse function cannot modify the query string and then reject it.

**Option B (recommended): Execute SHOW via the same sv_ddl_conn path.** The `sv_execute_ddl_rust` function executes `SELECT * FROM list_semantic_views()` on the DDL connection, but `sv_ddl_bind` currently outputs a single VARCHAR column `view_name`. For SHOW, the result has two columns (name, base_table) with multiple rows.

**Option C (recommended, simplest): All statements go through sv_execute_ddl_rust, which runs the rewritten SQL on sv_ddl_conn. The sv_ddl_bind/execute C++ functions become passthrough: they execute any rewritten SQL and return its result.** This requires generalizing sv_ddl_bind to not hardcode a single-column output schema.

**Chosen approach: Generalize the C++ plan function to be schema-flexible.** Currently `sv_ddl_bind` hardcodes one VARCHAR column. The cleanest approach:

1. `sv_execute_ddl_rust` rewrites the query and executes it on `sv_ddl_conn`
2. The Rust FFI returns a result summary (for CREATE/DROP: view name; for DESCRIBE: full row; for SHOW: all rows)
3. The C++ bind/execute functions emit the appropriate schema

However, this adds significant C++ complexity. A better approach:

**Final recommendation: Two C++ plan paths.**

The `sv_plan_function` inspects the `SemanticViewParseData` query text and returns different table functions:
- For CREATE/DROP: `sv_ddl_internal` (existing, single VARCHAR output)
- For DESCRIBE/SHOW: `sv_query_internal` (new, executes rewritten SQL and returns DuckDB's native result)

Actually, the simplest approach that requires minimal C++ changes:

**Simplest approach: Have sv_execute_ddl_rust handle everything, and the C++ side just shows the view name for DDL mutations, while DESCRIBE and SHOW are handled differently.**

Let me reconsider the architecture more carefully.

### 3. C++ shim (`cpp/src/shim.cpp`) -- MODIFY

The C++ shim currently has:
- `sv_parse_stub`: Calls `sv_parse_rust`, returns PARSE_SUCCESSFUL or default (DISPLAY_ORIGINAL_ERROR)
- `sv_plan_function`: Returns a TableFunction `sv_ddl_internal` with the query text as a VARCHAR parameter
- `sv_ddl_bind`: Calls `sv_execute_ddl_rust` to rewrite + execute, returns single `view_name` VARCHAR column
- `sv_ddl_execute`: Emits one row with the view name

**Changes needed:**

For CREATE/DROP variants: **No C++ changes needed.** The existing `sv_ddl_bind` calls `sv_execute_ddl_rust`, which will be updated in Rust to handle all CREATE/DROP variants. The output is still a single view name. The Rust side handles the rewrite; C++ is oblivious to the variant.

For DESCRIBE and SHOW: The output schema differs (DESCRIBE = 6 columns, SHOW = 2 columns with N rows). Two approaches:

**Approach A: Separate C++ table functions for DESCRIBE/SHOW.**
- Add `sv_describe_bind` + `sv_describe_execute` (6 VARCHAR columns, 1 row)
- Add `sv_show_bind` + `sv_show_execute` (2 VARCHAR columns, N rows)
- `sv_plan_function` dispatches to the appropriate table function based on query prefix
- Each calls its own Rust FFI function

**Approach B (recommended): Rewrite DESCRIBE/SHOW in Rust, execute via sv_ddl_conn, read results back via C API.**
The Rust FFI function `sv_execute_ddl_rust` already has the DDL connection. For DESCRIBE/SHOW:
1. Rust rewrites the query to the function call equivalent
2. Rust executes it on `sv_ddl_conn`
3. Rust reads the result columns and rows
4. Rust writes them into output buffers
5. C++ `sv_ddl_bind` declares the appropriate output schema
6. C++ `sv_ddl_execute` emits the data from the buffers

This is complex. Let me reconsider.

**Approach C (simplest, recommended): The Rust detect function classifies statement kind. The C++ plan function uses this to choose the appropriate rewritten SQL. For DESCRIBE/SHOW, the plan function returns a TableFunction that simply redirects to the existing registered table function by executing the rewritten SQL.**

Actually, the simplest architecture is:

**Approach D: Extend sv_execute_ddl_rust to return a "kind" code alongside the result. In the C++ side, use the kind to adjust the output schema.**

This is still complex. Let me step back and think about this differently.

**The real simplest approach:**

Currently, `sv_plan_function` returns `ParserExtensionPlanResult` with a custom internal `TableFunction`. But `ParserExtensionPlanResult` has a `function` field -- it can point to ANY `TableFunction`.

What if `sv_plan_function`:
1. Receives the raw query text via `SemanticViewParseData`
2. Calls a Rust FFI function to get the rewritten SQL string
3. Uses `Connection::Query(rewritten_sql)` to execute directly -- NO, this would deadlock (we're in the plan phase, ClientContext lock is held)

The DDL connection (`sv_ddl_conn`) exists precisely for this reason. The existing pattern works: plan returns a table function, bind executes via sv_ddl_conn.

**Final recommended approach for DESCRIBE/SHOW:**

Keep the existing pattern. All seven statements go through `sv_execute_ddl_rust`. For CREATE/DROP, the rewritten SQL executes and returns the view name. For DESCRIBE/SHOW, the rewritten SQL executes and the result is... discarded? No.

The issue is that `sv_ddl_bind` hardcodes `return_types.push_back(LogicalType::VARCHAR)` and `names.push_back("view_name")`. This works for CREATE/DROP but not for DESCRIBE (6 columns) or SHOW (2 columns, N rows).

**Cleanest solution: Have the Rust FFI return the statement kind, and let the C++ side dispatch to different bind/execute functions.**

Here is the concrete architecture:

```
sv_parse_stub
    |
    sv_parse_rust(query) -> u8 (0 = not ours, 1 = ours)
    |
    [if 1] -> PARSE_SUCCESSFUL + SemanticViewParseData(query)
    |
sv_plan_function
    |
    sv_classify_rust(query) -> u8 kind code
    |
    [kind 0-4: CREATE/DROP] -> return sv_ddl_internal table function
    [kind 5: DESCRIBE]      -> return sv_describe_internal table function
    [kind 6: SHOW]          -> return sv_show_internal table function
```

Three table functions in C++:
1. **`sv_ddl_internal`** (existing) -- CREATE/DROP variants, single VARCHAR output
2. **`sv_describe_internal`** (new) -- DESCRIBE, 6 VARCHAR columns, calls Rust to get data
3. **`sv_show_internal`** (new) -- SHOW, 2 VARCHAR columns + N rows, calls Rust to get data

Each has its own bind/execute. The FFI boundary for DESCRIBE/SHOW can use fixed-size buffers (DESCRIBE is always 6 fields; SHOW rows can use a JSON array serialized to a buffer).

### Revised Component Diagram

```
User SQL Input
    |
    v
DuckDB Native Parser
    |
    +--[parses OK]---> Standard execution (function-based DDL, queries)
    |
    +--[parse fails]--> sv_parse_stub (C++)
                            |
                        sv_parse_rust (Rust) -- detect_statement()
                            |
                        [None] -> DISPLAY_ORIGINAL_ERROR / DISPLAY_EXTENSION_ERROR
                        [Some] -> PARSE_SUCCESSFUL + SemanticViewParseData
                                    |
                                sv_plan_function (C++)
                                    |
                                sv_classify_rust (Rust) -- detect_statement() again
                                    |
                        +-----------+-----------+
                        |           |           |
                   CREATE/DROP   DESCRIBE     SHOW
                        |           |           |
                sv_ddl_internal  sv_describe  sv_show
                (existing)       (new)        (new)
                        |           |           |
                sv_execute_ddl_rust   |        |
                (Rust, extended)      |        |
                        |             |        |
                  rewrite + execute   |        |
                  on sv_ddl_conn      |        |
                        |             |        |
                  return view_name    |        |
                                      |        |
                        sv_describe_rust        sv_show_rust
                        (Rust FFI, new)         (Rust FFI, new)
                              |                       |
                        Read catalog              Read catalog
                        Return JSON               Return JSON
                              |                       |
                        C++ parses JSON          C++ parses JSON
                        Emits 6 columns          Emits N x 2 columns
```

Wait -- this is overcomplicating things. DESCRIBE and SHOW don't need to go through the DDL connection at all. They are read-only operations that only need the in-memory catalog. The existing `describe_semantic_view` and `list_semantic_views` VTab functions access the catalog via `get_extra_info::<CatalogState>()`.

But through the parser hook path, we don't have access to `extra_info` -- the table function returned by `sv_plan_function` is a raw C++ `TableFunction`, not a registered Rust VTab.

**Simplest correct approach: For DESCRIBE and SHOW, the C++ plan function returns a table function whose bind callback rewrites and executes the SQL on sv_ddl_conn, then reads the result back.**

This is exactly what `sv_ddl_bind` does for CREATE/DROP, but the output schema and row count differ.

Let me lay out the final, concrete approach:

### Final Architecture: Unified Rewrite-and-Execute

**All seven statements use the same pattern:**
1. Rust detects and classifies the statement
2. Rust rewrites to the function-call equivalent
3. The rewritten SQL is executed on `sv_ddl_conn`
4. The C++ table function reads the result and outputs it

The key change is making the C++ bind/execute functions schema-flexible:

**Option: Use a single generalized table function in C++ that reads the result schema from the executed query.**

```cpp
// sv_ddl_bind (modified):
// 1. Call sv_execute_ddl_rust to rewrite + execute
// 2. After execution, query the result metadata to learn column names/types
// 3. Declare matching output columns
// 4. Store the result data for emit in execute phase

// sv_ddl_execute (modified):
// Emit all rows from the stored result
```

This is the approach. The C++ `sv_ddl_bind` already calls `sv_execute_ddl_rust` which executes on `sv_ddl_conn`. Currently it ignores the result data and just reads the view name from Rust's output buffer. The modification:

1. `sv_execute_ddl_rust` executes the rewritten SQL and returns the `duckdb_result` handle
2. The C++ side reads column count, column names, column types, and row data from the result
3. The C++ bind function declares matching output columns
4. The C++ execute function emits all rows

**But `sv_execute_ddl_rust` currently destroys the result after reading it.** The change: the Rust FFI should NOT destroy the result. Instead, it returns the raw `duckdb_result` to C++. But `duckdb_result` is a C API type, not a C++ type...

Actually, the cleanest approach is to have Rust NOT execute the query. Instead:

1. Rust rewrites the query and returns the rewritten SQL string
2. C++ executes it on `sv_ddl_conn` (which is already a C API connection)
3. C++ reads the native result

This moves execution from Rust to C++. The C++ side already has `sv_ddl_conn` as a `duckdb_connection` (C API handle).

### Final Final Architecture (Concrete)

**Step 1: Rust FFI -- rewrite only (no execute)**

New Rust FFI function (replaces `sv_execute_ddl_rust` in the DESCRIBE/SHOW path):

```rust
/// Rewrite any semantic view statement into its function-call equivalent.
/// Returns 0 on success (rewritten SQL in out buffer), 1 on error.
#[no_mangle]
pub extern "C" fn sv_rewrite_rust(
    query_ptr: *const u8, query_len: usize,
    sql_out: *mut u8, sql_out_len: usize,
    error_out: *mut u8, error_out_len: usize,
) -> u8
```

**Step 2: C++ plan function -- dispatch by kind**

The C++ `sv_plan_function` calls `sv_rewrite_rust` to get the rewritten SQL, then returns a table function that:
1. Stores the rewritten SQL
2. In bind: executes it on `sv_ddl_conn`, reads result metadata, declares matching schema
3. In execute: emits all rows from the result

This generalizes the current `sv_ddl_bind`/`sv_ddl_execute` pair.

**Step 3: C++ bind -- schema-flexible**

```cpp
static unique_ptr<FunctionData> sv_ddl_bind(
    ClientContext &, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {

    auto rewritten_sql = StringValue::Get(input.inputs[0]);

    // Execute on the DDL connection
    duckdb_result result;
    auto rc = duckdb_query(sv_ddl_conn, rewritten_sql.c_str(), &result);
    if (rc != DuckDBSuccess) {
        auto err = duckdb_result_error(&result);
        duckdb_destroy_result(&result);
        throw BinderException("%s", err);
    }

    // Read schema from result
    auto col_count = duckdb_column_count(&result);
    for (idx_t i = 0; i < col_count; i++) {
        names.push_back(duckdb_column_name(&result, i));
        // Map duckdb_column_type to LogicalType...
        return_types.push_back(LogicalType::VARCHAR); // Safe fallback
    }

    // Store result data for execute phase
    // ... read all rows into a vector of string vectors ...

    duckdb_destroy_result(&result);
    return make_uniq<SvDdlBindData>(row_data);
}
```

This is a solid approach. All seven statement types go through the same C++ table function. The schema adapts to whatever the underlying function returns.

**However, there is a simpler alternative that avoids ALL C++ changes:**

### Alternative: No C++ Changes At All

The existing `sv_execute_ddl_rust` already has access to `sv_ddl_conn` (passed as a parameter). It executes the rewritten SQL. Currently it only reads the view name.

For DESCRIBE and SHOW, the Rust code can:
1. Execute the rewritten SQL on `exec_conn`
2. Read the full result (all columns, all rows)
3. Serialize it to JSON
4. Write the JSON to `name_out` (repurposing the buffer)
5. Return a new code (e.g., 2 = "result is JSON, not a name")

The C++ `sv_ddl_bind` checks the return code:
- `0`: Single VARCHAR output with `name_buf` as the view name (existing behavior)
- `2`: The result is JSON, parse and emit multi-column output

This requires minimal C++ changes (an if-else in bind) and keeps all parsing logic in Rust.

**But this is hacky.** Let me recommend the clean approach.

---

## Recommended Architecture

### Approach: Generalized C++ Table Function + Pure Rust Rewrite

**Principle: Rust handles all parsing and rewriting. C++ handles execution and result forwarding.**

#### Changes by file:

**`src/parse.rs` (MODIFY -- primary Rust change)**
- Add `StatementKind` enum
- Add `detect_statement(query: &str) -> Option<StatementKind>` function
- Add `rewrite_to_function_call(query: &str) -> Result<String, ParseError>` function
- Add `ParseError` struct with `message` and `position` fields (for error reporting)
- Update `sv_parse_rust` to detect all seven statement kinds
- Add `sv_rewrite_rust` FFI function: rewrite only, no execute
- Keep `sv_execute_ddl_rust` for backward compatibility but mark as unused (or remove)

**`cpp/src/shim.cpp` (MODIFY -- generalize table function)**
- `sv_parse_stub`: No change (still calls `sv_parse_rust`)
- `sv_plan_function`: Call `sv_rewrite_rust` to get the rewritten SQL, pass it as parameter to the table function
- `sv_ddl_bind`: Execute rewritten SQL on `sv_ddl_conn`, read schema + data from result, declare matching output
- `sv_ddl_execute`: Emit stored rows
- `SvDdlBindData`: Expand to hold column names, types, and row data (not just view_name)

**`src/ddl/*` (NO CHANGES)**
- All function-based DDL VTab implementations remain unchanged
- They continue to serve the `FROM create_semantic_view(...)` path

**`src/catalog.rs` (NO CHANGES)**
- Both paths converge on the same catalog functions

**`src/expand.rs`, `src/query/*`, `src/model.rs` (NO CHANGES)**
- Query execution is orthogonal

---

## Error Location Reporting Architecture

### DuckDB's Error Location Mechanism

DuckDB's `ParserExtensionParseResult` has an `error_location` field of type `optional_idx_t`. When the parse function returns `DISPLAY_EXTENSION_ERROR`, DuckDB calls `ParserException::SyntaxError(query, result.error, result.error_location)`, which formats the error with a caret (^) pointing to the error position:

```
Parser Error: syntax error near "VEIW" in CREATE SEMANTIC VEIW
CREATE SEMANTIC VEIW sales (...)
                ^^^^
Did you mean: VIEW?
```

### Integration with Rust FFI

The current `sv_parse_rust` returns `u8` (0 or 1). To support error reporting, the FFI contract must expand:

```rust
/// FFI result from Rust parse function.
/// Returned as a struct so C++ can read both the status and error details.
#[repr(C)]
pub struct SvParseResult {
    /// 0 = not ours (DISPLAY_ORIGINAL_ERROR)
    /// 1 = detected (PARSE_SUCCESSFUL)
    /// 2 = ours but malformed (DISPLAY_EXTENSION_ERROR)
    pub status: u8,
    /// Character position of error (0 = not set). Only meaningful when status == 2.
    pub error_position: u32,
}
```

The C++ `sv_parse_stub` checks the status:
- `0`: Return default `ParserExtensionParseResult()` (DISPLAY_ORIGINAL_ERROR)
- `1`: Return `ParserExtensionParseResult(make_uniq<SemanticViewParseData>(query))`
- `2`: Return `ParserExtensionParseResult(error_message, error_location)`

For status 2, the error message comes from a second Rust FFI call or from a separate buffer:

```rust
#[repr(C)]
pub struct SvParseResult {
    pub status: u8,
    pub error_position: u32,
    // Error message written to a buffer provided by C++
}

// Or: use separate error buffer parameters (matching existing pattern):
#[no_mangle]
pub extern "C" fn sv_parse_rust(
    query_ptr: *const u8, query_len: usize,
    error_out: *mut u8, error_out_len: usize,
    error_position_out: *mut u32,
) -> u8
```

### When Errors Are Reported

Error reporting happens at **two stages**:

**Stage 1: Parse-time (sv_parse_rust) -- Prefix-level errors**

These are errors where the statement LOOKS like a semantic view DDL but is malformed at the prefix level:
- `CREATE SEMANTIC VEIW` -- typo in keyword (detected because DuckDB parser failed, and our prefix match failed, but we can suggest)
- `DROP SEMANTIC VIEW` with no name after it
- `DESCRIBE SEMANTIC` with no `VIEW` after it

For near-miss detection (e.g., `CREATE SEMANTIC VEIW`), the parse function can check if the query starts with `CREATE SEMANTIC` or `DROP SEMANTIC` and then fuzzy-match the next word against "VIEW" using the existing `strsim` crate.

**Stage 2: Bind-time (sv_ddl_bind -> sv_execute_ddl_rust) -- Body-level errors**

These are errors in the DDL body after rewriting:
- Missing required clauses (no `tables :=`)
- Invalid struct literal syntax
- Referenced table does not exist (caught by DuckDB when executing the rewritten SQL)

Body-level errors are already reported via the existing mechanism: `sv_execute_ddl_rust` catches the DuckDB error from executing the rewritten SQL and writes it to `error_out`. The C++ `sv_ddl_bind` throws `BinderException` with this message.

### Error Message Enhancement

The existing `ExpandError` variants in `expand.rs` already include fuzzy suggestions (via `strsim::levenshtein`). For the parse layer, new errors should follow the same pattern:

```rust
pub struct ParseError {
    pub message: String,
    /// Character offset in the original query where the error was detected.
    /// Used by DuckDB to render a caret (^) under the error location.
    pub position: Option<usize>,
    /// Optional "did you mean" suggestion (using strsim).
    pub suggestion: Option<String>,
}
```

Error messages should include:
1. **What went wrong** (clause-level context: "in DIMENSIONS clause")
2. **Where** (character position)
3. **Suggestion** (fuzzy match, if applicable)

Example error messages:
```
CREATE SEMANTIC VIEW sales: missing view name after 'CREATE SEMANTIC VIEW'
CREATE SEMANTIC VIEW sales (): expected 'tables :=' clause
DROP SEMANTIC VIEW nonexistent: semantic view 'nonexistent' does not exist. Did you mean 'sales_view'?
```

### Clause-Level Position Tracking

For body-level errors (after the prefix), the parse function can track positions within the original query:

```rust
fn parse_ddl_text(query: &str) -> Result<(&str, &str), ParseError> {
    let trimmed = query.trim();
    let prefix = detect_prefix(trimmed);
    match prefix {
        None => Err(ParseError {
            message: "Not a semantic view statement".into(),
            position: Some(0),
            suggestion: None,
        }),
        Some((kind, prefix_end)) => {
            let after_prefix = &trimmed[prefix_end..].trim_start();
            if after_prefix.is_empty() {
                return Err(ParseError {
                    message: format!("Missing view name after '{}'", &trimmed[..prefix_end]),
                    position: Some(prefix_end),
                    suggestion: None,
                });
            }
            // ... extract name, body, track positions
        }
    }
}
```

The `position` value is the byte offset in the original query string. DuckDB renders this as a caret under the error location.

---

## Data Flow Changes Summary

### Current Flow (v0.5.0)

```
parse: detect "CREATE SEMANTIC VIEW" -> u8 (0/1)
plan:  carry query text -> TableFunction
bind:  rewrite to function call + execute on sv_ddl_conn -> view_name
exec:  emit view_name (1 row, 1 column)
```

### New Flow (v0.5.1)

```
parse: detect 7 statement kinds -> u8 (0/1/2) + error info
plan:  get rewritten SQL from Rust -> TableFunction with rewritten SQL as param
bind:  execute rewritten SQL on sv_ddl_conn -> read result schema + data
exec:  emit result (N rows, M columns -- adapts to statement kind)
```

### Key Data Flow Differences

| Aspect | v0.5.0 | v0.5.1 |
|--------|--------|--------|
| Detection | Single prefix | Seven prefix patterns |
| Parse result | Binary (ours / not ours) | Ternary (ours / not ours / ours-but-error) |
| Rewrite | In Rust FFI (sv_execute_ddl_rust) | In Rust FFI (sv_rewrite_rust) |
| Execution | In Rust FFI via duckdb_query | In C++ via duckdb_query on sv_ddl_conn |
| Result reading | Rust reads view name only | C++ reads full result (schema + data) |
| Output schema | Fixed: 1 VARCHAR column | Dynamic: matches rewritten query result |
| Error info | Error message string | Error message + character position + suggestion |

---

## Suggested Build Order

### Wave 1: Parse Layer (No C++ Changes)

**1a. Refactor `parse.rs` detection and rewrite functions (Rust only, testable with `cargo test`)**
- Add `StatementKind` enum
- Implement `detect_statement()` for all 7 kinds
- Implement `rewrite_to_function_call()` for all 7 kinds
- Add `ParseError` with position tracking
- Unit tests for every statement kind and error case
- **No FFI changes yet -- pure Rust, full test coverage**

**1b. Add error location reporting to parse layer (Rust only)**
- Near-miss detection (e.g., `CREATE SEMANTIC VEIW`)
- Position tracking for missing-name, missing-body errors
- "Did you mean" suggestions using existing `strsim` crate
- Unit tests for error messages and positions

### Wave 2: FFI + C++ Integration

**2a. Update `sv_parse_rust` FFI to handle all 7 kinds + error reporting**
- Extend return type to include error status
- Add error buffer and position output parameters
- Keep backward-compatible for C++ side

**2b. Add `sv_rewrite_rust` FFI function**
- Takes query, returns rewritten SQL string
- C++ calls this from plan_function

**2c. Generalize C++ `sv_ddl_bind` / `sv_ddl_execute`**
- Execute rewritten SQL on sv_ddl_conn
- Read result schema dynamically (column count, names, types)
- Read all result rows
- Emit matching output

**2d. Update C++ `sv_parse_stub` for error reporting**
- Check new return status from sv_parse_rust
- Return DISPLAY_EXTENSION_ERROR with error message and position when status == 2

### Wave 3: Integration Tests + README

**3a. SQLLogicTest cases for all 7 statement kinds**
**3b. Error reporting tests (expected error messages with positions)**
**3c. README DDL reference section**

### Dependency Graph

```
Wave 1a (detect + rewrite)
    |
    +---> Wave 1b (error reporting)
    |         |
    |         v
    +---> Wave 2a (FFI detect) + Wave 2b (FFI rewrite)
              |
              v
          Wave 2c (C++ generalize) + Wave 2d (C++ error)
              |
              v
          Wave 3 (tests + docs)
```

Waves 1a and 1b are independent of each other and can proceed in parallel, but both must complete before Wave 2. Within Wave 2, 2a/2b can proceed in parallel, but 2c/2d depend on them.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Separate C++ Table Functions Per Statement Kind

**What:** Creating `sv_create_internal`, `sv_drop_internal`, `sv_describe_internal`, `sv_show_internal` as separate C++ table functions.

**Why bad:** Quadruples the C++ code surface. Each needs its own bind/state/execute. The user is not a C++ expert.

**Instead:** One generalized C++ table function that adapts its output schema to the rewritten SQL result. All parsing intelligence stays in Rust.

### Anti-Pattern 2: Parsing DDL Body in the Parse Phase

**What:** Attempting to parse the full DDL body (tables, dimensions, metrics) in `sv_parse_rust` during the parse phase.

**Why bad:** The parse function is called for every failed parse in DuckDB. It must be fast. Full body parsing is expensive and unnecessary -- the body is parsed later during bind when the rewritten SQL is executed by DuckDB's own parser.

**Instead:** The parse function only validates the prefix and extracts the view name. The body is passed through verbatim in the rewrite.

### Anti-Pattern 3: Hardcoding Error Positions

**What:** Calculating byte offsets manually with string slicing that doesn't account for multi-byte UTF-8.

**Why bad:** DuckDB SQL is ASCII for keywords, but view names and string literals can contain UTF-8. Byte offsets passed to `ParserException::SyntaxError` are character positions in the query string.

**Instead:** Use `query[..pos].chars().count()` if the error position needs to be character-based, or verify that DuckDB uses byte offsets (it does -- DuckDB's internal representation uses byte offsets for its own parser errors). Stick with byte offsets.

### Anti-Pattern 4: Duplicating Catalog Logic in Parse Layer

**What:** Having the parse layer check if a view exists (for DROP) or if it's a duplicate (for CREATE).

**Why bad:** The parse layer should only detect and rewrite. Existence checks belong in the bind/execute phase where they already happen via the existing VTab implementations.

**Instead:** The parse layer rewrites `DROP SEMANTIC VIEW sales` to `SELECT * FROM drop_semantic_view('sales')`. If `sales` doesn't exist, the error comes from `DropSemanticViewVTab::bind`, which already has the catalog check and error message.

---

## Component Boundaries (Updated for v0.5.1)

| Component | Responsibility | Communicates With | Changes |
|-----------|---------------|-------------------|---------|
| `src/parse.rs` | Detect all 7 DDL kinds, rewrite to function calls, error position tracking | `shim.cpp` (via FFI), `strsim` (for suggestions) | **MODIFY: major** |
| `cpp/src/shim.cpp` | C++ entry, parser hook registration, rewrite execution, schema-flexible result forwarding | `parse.rs` (via FFI), DuckDB (C API), `sv_ddl_conn` | **MODIFY: moderate** |
| `src/lib.rs` | Rust init, function registration | `catalog.rs`, `ddl/*`, `query/*` | **NO CHANGE** |
| `src/ddl/define.rs` | CREATE function-based DDL | `parse_args.rs`, `catalog.rs` | **NO CHANGE** |
| `src/ddl/drop.rs` | DROP function-based DDL | `catalog.rs` | **NO CHANGE** |
| `src/ddl/describe.rs` | DESCRIBE function-based DDL | `catalog.rs` | **NO CHANGE** |
| `src/ddl/list.rs` | SHOW/LIST function-based DDL | `catalog.rs` | **NO CHANGE** |
| `src/catalog.rs` | In-memory + persistent catalog | All DDL paths | **NO CHANGE** |
| `src/expand.rs` | Query expansion | `query/*` | **NO CHANGE** |
| `src/query/*` | Query execution | `expand.rs`, `catalog.rs` | **NO CHANGE** |

---

## Sources

- [DuckDB parser_extension.hpp](https://raw.githubusercontent.com/duckdb/duckdb/main/src/include/duckdb/parser/parser_extension.hpp) -- ParserExtensionParseResult.error_location field, DISPLAY_EXTENSION_ERROR result type (HIGH confidence)
- [DuckDB parser.cpp](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- Parser fallback chain, how error_location is passed to ParserException::SyntaxError (HIGH confidence)
- [DuckDB Runtime-Extensible Parsers (CIDR 2025)](https://duckdb.org/pdf/CIDR2025-muehleisen-raasveldt-extensible-parsers.pdf) -- Parser extension mechanism overview
- Project source: `src/parse.rs`, `cpp/src/shim.cpp`, `src/lib.rs` -- existing architecture (HIGH confidence, direct code inspection)
- Project source: `src/ddl/*.rs` -- existing function-based DDL implementations (HIGH confidence)
- Project source: `src/catalog.rs` -- catalog API (HIGH confidence)
- Project source: `src/expand.rs` -- ExpandError + suggest_closest for fuzzy matching pattern (HIGH confidence)
