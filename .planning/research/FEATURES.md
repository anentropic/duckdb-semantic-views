# Feature Landscape: v0.5.0 Parser Extension Spike

**Domain:** DuckDB Rust extension -- parser hook integration for native SQL DDL syntax
**Researched:** 2026-03-07
**Milestone:** v0.5.0 -- Native `CREATE SEMANTIC VIEW` syntax via C++ parser extension hooks
**Status:** Subsequent milestone research (v0.4.0 already shipped)
**Overall confidence:** HIGH (API surface verified from DuckDB v1.4.1 source code)

---

## Scope

This document covers the DuckDB parser extension hook API surface and what is needed to add native `CREATE SEMANTIC VIEW` DDL syntax. All existing features (function-based DDL, `semantic_view()` table function, zero-copy typed output, catalog persistence) are already built and not re-researched here.

**Focus:** Parser extension hook API, C++ shim requirements, FFI bridge, statement handling pipeline.

---

## DuckDB Parser Extension API Surface

### Core Types (from `parser_extension.hpp`, verified against v1.4.1 source)

**Confidence: HIGH** -- extracted verbatim from DuckDB v1.4.1 and `main` branch source.

#### `ParserExtensionInfo` (base struct)

```cpp
struct ParserExtensionInfo {
    virtual ~ParserExtensionInfo() {}
};
```

Static information kept alive for the database lifetime. Passed to both `parse_function` and `plan_function`. Extensions subclass this to carry custom state (e.g., pointers to Rust-side catalog). Optional -- can be left as nullptr if no static info is needed.

#### `ParserExtensionResultType` (enum)

```cpp
enum class ParserExtensionResultType : uint8_t {
    PARSE_SUCCESSFUL,           // Extension parsed the statement successfully
    DISPLAY_ORIGINAL_ERROR,     // Extension cannot handle this -- show DuckDB's original error
    DISPLAY_EXTENSION_ERROR     // Extension recognized but rejected the statement -- show extension's error
};
```

Three-way return: success, pass-through, or custom error. `DISPLAY_ORIGINAL_ERROR` is the default (constructed by default ctor), meaning "not my statement, try the next extension."

#### `ParserExtensionParseData` (abstract base)

```cpp
struct ParserExtensionParseData {
    virtual ~ParserExtensionParseData() {}
    virtual unique_ptr<ParserExtensionParseData> Copy() const = 0;
    virtual string ToString() const = 0;
};
```

Extensions subclass this to carry parsed statement data between the parse and plan stages. Must implement `Copy()` (DuckDB may copy statements internally) and `ToString()` (used for debugging/error messages). This is where the extension stores its parsed representation -- for semantic views, this would be the view name + definition parameters.

#### `ParserExtensionParseResult` (return type of parse_function)

```cpp
struct ParserExtensionParseResult {
    ParserExtensionParseResult();                                        // default: DISPLAY_ORIGINAL_ERROR
    explicit ParserExtensionParseResult(string error_p);                 // DISPLAY_EXTENSION_ERROR
    explicit ParserExtensionParseResult(unique_ptr<ParserExtensionParseData>); // PARSE_SUCCESSFUL

    ParserExtensionResultType type;
    unique_ptr<ParserExtensionParseData> parse_data;   // populated on success
    string error;                                       // populated on extension error
    optional_idx error_location;                        // char offset for error highlighting
};
```

Three constructors for three outcomes. The default constructor returns `DISPLAY_ORIGINAL_ERROR` -- the "not my problem" response.

#### `parse_function_t` (function pointer typedef)

```cpp
typedef ParserExtensionParseResult (*parse_function_t)(
    ParserExtensionInfo *info,    // static info (or nullptr)
    const string &query           // the raw SQL statement text
);
```

Receives the raw SQL string for a single statement. DuckDB's statement splitter runs first, so multi-statement scripts are already split. The hook only fires for statements that DuckDB's PostgreSQL parser failed to parse.

#### `ParserExtensionPlanResult` (return type of plan_function)

```cpp
struct ParserExtensionPlanResult {
    TableFunction function;                                              // the table function to execute
    vector<Value> parameters;                                            // parameters to the function
    unordered_map<string, StatementProperties::CatalogIdentity>          // v1.4.1
        modified_databases;                                              // databases modified (empty = read-only)
    bool requires_valid_transaction = true;                              // transaction requirement
    StatementReturnType return_type = StatementReturnType::NOTHING;      // result set type
};
```

**Note:** On `main` branch, `CatalogIdentity` was renamed to `ModificationInfo`. Version-pin to v1.4.1 for now. The `return_type = NOTHING` default is correct for DDL statements that don't return result rows.

#### `plan_function_t` (function pointer typedef)

```cpp
typedef ParserExtensionPlanResult (*plan_function_t)(
    ParserExtensionInfo *info,
    ClientContext &context,
    unique_ptr<ParserExtensionParseData> parse_data
);
```

Called by the binder when it encounters an `ExtensionStatement`. Receives the `parse_data` from the parse stage. Must return a `ParserExtensionPlanResult` containing a `TableFunction` + parameters, OR throw a `BinderException` to use the stash pattern (see below).

#### `ParserExtension` (the registration class)

```cpp
class ParserExtension {
public:
    parse_function_t parse_function = nullptr;
    plan_function_t plan_function = nullptr;
    parser_override_function_t parser_override = nullptr;  // main branch only, not in v1.4.1
    shared_ptr<ParserExtensionInfo> parser_info;

    static void Register(DBConfig &config, ParserExtension extension);
};
```

Registered via `config.parser_extensions.push_back(ext)` or the static `Register()` method. The `parser_override` field (pre-parse hook, for entire language replacements like PRQL) exists on `main` but not in v1.4.1. Not needed for our use case -- `parse_function` (fallback hook) is correct.

#### `ExtensionStatement` (the statement wrapper)

```cpp
class ExtensionStatement : public SQLStatement {
public:
    static constexpr const StatementType TYPE = StatementType::EXTENSION_STATEMENT;

    ExtensionStatement(ParserExtension extension, unique_ptr<ParserExtensionParseData> parse_data);

    ParserExtension extension;                         // which extension parsed this
    unique_ptr<ParserExtensionParseData> parse_data;   // the parsed data
};
```

Created by DuckDB's parser when `parse_function` returns `PARSE_SUCCESSFUL`. Flows through DuckDB's pipeline as a normal `SQLStatement` until it reaches the binder, which calls `plan_function`.

---

## How the Parser Fallback Flow Works

**Confidence: HIGH** -- verified from `parser.cpp` source (v1.4.1).

### Trigger Condition

The fallback fires when DuckDB's built-in PostgreSQL parser fails to parse a statement. For `CREATE SEMANTIC VIEW ...`, the standard parser recognizes `CREATE` but fails at the unknown keyword `SEMANTIC`, generating a syntax error.

### Exact Flow

```
1. User submits: "CREATE SEMANTIC VIEW sales (...)"
2. DuckDB's main parser attempts to parse the full query string
3. PostgreSQL parser FAILS (syntax error at "SEMANTIC")
4. IF no parser extensions registered: throw ParserException (original error)
5. IF parser extensions exist:
   a. Split query into individual statements (semicolon-delimited)
   b. For EACH statement:
      i.   Try DuckDB's parser again on this individual statement
      ii.  If DuckDB succeeds: transform to AST, continue to next statement
      iii. If DuckDB fails: iterate through registered parser extensions
           - Call ext.parse_function(ext.parser_info.get(), query_statement)
           - If PARSE_SUCCESSFUL:
               Create ExtensionStatement(ext, result.parse_data)
               Set stmt_length, stmt_location
               Add to statements vector
               Break (stop trying other extensions)
           - If DISPLAY_EXTENSION_ERROR:
               Throw ParserException with extension's error message
           - If DISPLAY_ORIGINAL_ERROR:
               Continue to next extension
      iv.  If no extension parsed it: throw original ParserException
```

**Key implications:**
- The extension receives the raw SQL string, not a partial AST or token stream
- Statement splitting (by `;`) happens before our hook, so we get a single clean statement
- Multiple parser extensions are tried in registration order; first success wins
- Our hook is only called for statements DuckDB cannot parse -- zero overhead for normal SQL

---

## Two Execution Paths for plan_function

### Path A: Direct TableFunction Return (simple, recommended for our case)

The `plan_function` returns a `ParserExtensionPlanResult` with a `TableFunction` and parameters. DuckDB's binder calls `BindTableFunction()` on it, producing a `LOGICAL_GET` plan node. The table function then executes the actual DDL logic.

```cpp
// In bind_extension.cpp (verified from v1.4.1 source):
BoundStatement Binder::Bind(ExtensionStatement &stmt) {
    auto parse_result = stmt.extension.plan_function(
        stmt.extension.parser_info.get(),
        context,
        std::move(stmt.parse_data)
    );
    // ... set statement properties ...
    result.plan = BindTableFunction(parse_result.function, std::move(parse_result.parameters));
    // ... extract column names/types from LOGICAL_GET ...
    return result;
}
```

**For semantic views:** The `plan_function` returns the existing `create_semantic_view` table function with the parsed parameters serialized as `Value` arguments. DuckDB then binds and executes the same table function that the function-based DDL currently uses. This is the simplest path and reuses all existing logic.

### Path B: BinderException Stash Pattern (complex, for query rewriting)

Used by prql where the extension needs to re-parse the result as standard SQL and bind it through DuckDB's normal binder. The `plan_function` intentionally throws `BinderException`, stashing state in `context.registered_state`. A separate `OperatorExtension::Bind` picks it up.

```cpp
// prql's plan_function:
ParserExtensionPlanResult prql_plan(...) {
    auto prql_state = make_shared_ptr<PrqlState>(std::move(parse_data));
    context.registered_state->Remove("prql");
    context.registered_state->Insert("prql", prql_state);
    throw BinderException("Use prql_bind instead");
}

// OperatorExtension picks up:
BoundStatement prql_bind(ClientContext &context, Binder &binder,
                         OperatorExtensionInfo *info, SQLStatement &statement) {
    auto lookup = context.registered_state->Get<PrqlState>("prql");
    auto prql_binder = Binder::CreateBinder(context, &binder);
    return prql_binder->Bind(*(prql_parse_data->statement));
}
```

**Not needed for semantic views.** The stash pattern is only necessary when the extension produces a standard SQL statement that needs to go through DuckDB's normal binding pipeline (prql compiles PRQL to SQL, then needs DuckDB to bind the resulting SQL). Our DDL executes via table functions, so Path A is sufficient.

---

## Table Stakes

Features users expect for native DDL syntax. Missing = the parser hook feels incomplete.

| Feature | Why Expected | Complexity | Dependencies | Notes |
|---------|--------------|------------|--------------|-------|
| `CREATE SEMANTIC VIEW name (...)` | Core syntax -- the whole point of this milestone | High | C++ shim, cc crate, duckdb.hpp amalgamation | Must parse the statement text and delegate to existing Rust DDL logic |
| `CREATE OR REPLACE SEMANTIC VIEW name (...)` | Standard SQL pattern for DDL | Low | Parse function prefix matching | DuckDB fails at `SEMANTIC` after `CREATE OR REPLACE`, so we get the full text in parse_function |
| `DROP SEMANTIC VIEW name` | Symmetric with CREATE; users expect it | Low | Same parse_function hook | Same fallback mechanism -- DuckDB fails at `SEMANTIC` |
| `DROP SEMANTIC VIEW IF EXISTS name` | Standard SQL DDL guard | Low | Parse function prefix matching | Already implemented as `drop_semantic_view_if_exists` function |
| `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | Standard SQL DDL guard | Low | Parse function prefix matching | Already implemented as `create_semantic_view_if_not_exists` function |
| C++ shim entry point (`DUCKDB_CPP_EXTENSION_ENTRY`) | Required for parser hook registration -- C++ API only | High | cc crate, duckdb.hpp, build.rs changes | ~40-50 lines of C++; replaces `semantic_views_init_c_api` as primary entry point |
| ABI footer switch (C_STRUCT to CPP) | DuckDB loader checks footer to decide which entry symbol to call | Low | extension-ci-tools footer injection | Change one flag in the footer stamping script |
| Semicolon handling | Inconsistent across CLI vs Python (DuckDB issue #18485) | Low | Parse function | Strip trailing semicolons before prefix matching; known DuckDB bug |
| Existing function-based DDL continues working | Users with existing scripts must not break | None | Registration happens alongside parser hooks | Both paths (function-based and native DDL) coexist permanently |

## Differentiators

Features that set the native DDL apart from the function-based approach. Not expected, but valued.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| Readable SQL-native syntax | `CREATE SEMANTIC VIEW sales (...)` vs `FROM create_semantic_view('sales', tables := [...])` -- dramatically cleaner | Part of core implementation | Parser hook | The primary value of this milestone |
| Error messages with source location | `ParserExtensionParseResult.error_location` enables DuckDB to highlight the error position in the SQL string | Medium | Parse function error reporting | `optional_idx error_location` field in the result struct |
| `DESCRIBE SEMANTIC VIEW name` | Natural SQL syntax for introspection (currently `FROM describe_semantic_view('name')`) | Low | Same parse_function hook | DuckDB fails at `SEMANTIC`, we get the text |
| `SHOW SEMANTIC VIEWS` / `LIST SEMANTIC VIEWS` | Natural SQL syntax for listing (currently `FROM list_semantic_views()`) | Low | Same parse_function hook | Same pattern |
| Custom `ParserExtensionInfo` with Rust catalog pointer | Pass catalog state through parser_info instead of through table function extra_info | Medium | FFI pointer management | Cleaner architecture but not required for MVP |

## Anti-Features

Features to explicitly NOT build for v0.5.0.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| `parser_override` (pre-parse hook) | Only needed for entire language replacements (PRQL, pgq). Intercepts ALL queries, adding overhead. Our statements fail DuckDB's parser naturally. | Use `parse_function` (fallback hook) -- zero overhead for normal SQL |
| Full SQL grammar in the parser | `parse_function` only handles statements DuckDB rejects. No need to parse SELECT, INSERT, etc. | Parse only the 3-5 statement prefixes we define (`CREATE SEMANTIC VIEW`, `DROP SEMANTIC VIEW`, etc.) |
| OperatorExtension / stash pattern | Adds complexity (BinderException + registered_state + OperatorExtension::Bind). Only needed when producing standard SQL for DuckDB to re-bind. | Return TableFunction directly from plan_function (Path A) |
| Native query syntax change | Changing `FROM semantic_view('name', ...)` to something else is a separate concern and not needed for DDL | Keep the existing `semantic_view()` table function as the query interface |
| YAML/file-based DDL | SQL DDL is the primary interface; YAML adds a second definition path | Defer to future milestone |
| Transaction support in plan_function | DDL statements don't need complex transaction semantics | Use `requires_valid_transaction = true` (default) |

---

## Feature Dependencies

```
C++ shim (DUCKDB_CPP_EXTENSION_ENTRY)
  --> Parser hook registration (config.parser_extensions.push_back)
      --> parse_function implementation (sv_parse)
          --> prefix matching (CREATE/DROP SEMANTIC VIEW)
          --> statement text parsing (extract name + body)
          --> ParserExtensionParseData subclass (carries parsed args)
      --> plan_function implementation (sv_plan)
          --> map parsed args to existing table function + Value params
          --> return ParserExtensionPlanResult
  --> Entry point conflict resolution
      --> Suppress or stub semantic_views_init_c_api
      --> Export semantic_views_init (C++ entry) as primary
  --> ABI footer switch (C_STRUCT -> CPP)

Existing Rust code (fully reusable):
  parse_args.rs logic --> reuse for extracting definition from SQL text
  define.rs (DefineState, catalog_insert/upsert) --> called by plan_function's table function
  drop.rs (DropState, catalog_delete) --> called by plan_function's table function
  catalog.rs --> unchanged
  model.rs --> unchanged
```

---

## Statement Surface Analysis

**Confidence: HIGH** -- verified from investigation doc and parser.cpp source.

### What parse_function receives

A single, already-split SQL statement as a raw string. DuckDB's statement splitter runs before the fallback, so:
- Multi-statement scripts: already split by `;`
- CTEs: parsed by DuckDB's parser (they succeed), never reach our hook
- Transactions (`BEGIN`/`COMMIT`): parsed by DuckDB, never reach our hook

### Statements to handle

| Statement | Prefix Pattern | Maps To | Existing Function |
|-----------|---------------|---------|-------------------|
| `CREATE SEMANTIC VIEW name (...)` | `CREATE SEMANTIC VIEW` | `create_semantic_view` | `DefineSemanticViewVTab` |
| `CREATE OR REPLACE SEMANTIC VIEW name (...)` | `CREATE OR REPLACE SEMANTIC VIEW` | `create_or_replace_semantic_view` | `DefineSemanticViewVTab` (or_replace=true) |
| `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | `CREATE SEMANTIC VIEW IF NOT EXISTS` | `create_semantic_view_if_not_exists` | `DefineSemanticViewVTab` (if_not_exists=true) |
| `DROP SEMANTIC VIEW name` | `DROP SEMANTIC VIEW` | `drop_semantic_view` | `DropSemanticViewVTab` |
| `DROP SEMANTIC VIEW IF EXISTS name` | `DROP SEMANTIC VIEW IF EXISTS` | `drop_semantic_view_if_exists` | `DropSemanticViewVTab` (if_exists=true) |

### Edge cases from known issues

| Case | Handling | Source |
|------|----------|--------|
| Leading whitespace / SQL comments | Must skip before prefix matching | Investigation doc |
| Trailing semicolon | Must strip -- inconsistent across CLI vs Python | DuckDB issue #18485 |
| Case insensitivity | `CREATE semantic view` should work | Standard SQL convention |
| `OR REPLACE` position | DuckDB fails at `SEMANTIC` in both `CREATE ...` and `CREATE OR REPLACE ...` -- we receive the full text | Verified from parser.cpp flow |

---

## Implementation Complexity Assessment

### High complexity (core work)

1. **C++ shim (`shim.cpp`)** -- ~40-50 lines. Must:
   - `#include "duckdb.hpp"` (amalgamation header)
   - Use `DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader)` macro
   - Get `DatabaseInstance&` from `ExtensionLoader&`
   - Register parser extension on `DBConfig::GetConfig(db).parser_extensions`
   - Create `duckdb_connection` for Rust via `Connection(db)`
   - Call Rust-side init (`sv_init_rust`) via FFI

2. **Build system changes** -- cc crate compiles `shim.cpp` against `duckdb.hpp`:
   - Vendor or fetch the DuckDB amalgamation header (duckdb.hpp + duckdb.cpp)
   - Update `build.rs` to compile C++ with cc crate
   - Update exported symbols list (`semantic_views_init` replaces `semantic_views_init_c_api`)
   - Update footer injection (C_STRUCT -> CPP)

3. **Entry point conflict** -- Two init symbols cannot coexist cleanly:
   - `semantic_views_init` (C++, new) -- called by DuckDB loader for CPP extensions
   - `semantic_views_init_c_api` (Rust, current) -- must be suppressed or made a no-op
   - Options: linker version script, feature-gate, or stub

### Medium complexity

4. **parse_function FFI bridge** -- C++ trampoline calls Rust:
   - C++ receives `const string&`, calls `extern "C" sv_parse(const char*, size_t)`
   - Rust does prefix matching, parses statement body
   - Rust returns a C struct that C++ wraps in `ParserExtensionParseData`

5. **plan_function FFI bridge** -- C++ trampoline calls Rust:
   - C++ receives `ParserExtensionParseData*`, calls `extern "C" sv_plan(...)`
   - Rust maps parsed args to existing table function + Value parameters
   - C++ constructs `ParserExtensionPlanResult` from Rust's return

6. **Statement text parsing** -- Extract view name and body from raw SQL:
   - Prefix matching: case-insensitive, skip whitespace/comments
   - Body parsing: extract the parenthesized argument block
   - Reuse or adapt `parse_args.rs` logic

### Low complexity

7. **Semicolon stripping** -- Trivial string manipulation
8. **`DISPLAY_ORIGINAL_ERROR` fast path** -- Return default `ParserExtensionParseResult()` for unrecognized statements
9. **ABI footer flag** -- One-line change in CI script
10. **Test updates** -- Add sqllogictest cases for native DDL syntax alongside existing function-based tests

---

## MVP Recommendation

### Must have (v0.5.0 spike scope)

1. **C++ shim with `DUCKDB_CPP_EXTENSION_ENTRY`** -- the architectural foundation
2. **parse_function for `CREATE SEMANTIC VIEW`** -- proves the parser hook works end-to-end
3. **plan_function returning existing table function** -- proves DDL execution works
4. **Entry point migration** -- `semantic_views_init` replaces `semantic_views_init_c_api`
5. **ABI footer switch** -- CPP instead of C_STRUCT
6. **Existing function-based DDL still works** -- backward compatibility

### Defer to subsequent milestone

- `DROP SEMANTIC VIEW` via parser hook (same pattern, lower value for spike)
- `DESCRIBE SEMANTIC VIEW` / `SHOW SEMANTIC VIEWS` via parser hook
- Error location reporting (`error_location` in parse result)
- Custom `ParserExtensionInfo` with Rust catalog pointer
- Native query syntax changes

---

## Key API Signatures Summary (Quick Reference)

```cpp
// Registration:
auto &config = DBConfig::GetConfig(loader.GetDatabaseInstance());
ParserExtension ext;
ext.parse_function = sv_parse_trampoline;    // parse_function_t
ext.plan_function = sv_plan_trampoline;      // plan_function_t
ext.parser_info = nullptr;                   // or shared_ptr<ParserExtensionInfo>
config.parser_extensions.push_back(ext);

// parse_function signature:
ParserExtensionParseResult sv_parse(ParserExtensionInfo *info, const string &query);

// plan_function signature:
ParserExtensionPlanResult sv_plan(ParserExtensionInfo *info, ClientContext &context,
                                   unique_ptr<ParserExtensionParseData> parse_data);

// Success return from parse:
return ParserExtensionParseResult(make_uniq_base<ParserExtensionParseData, SvParseData>(...));

// "Not my statement" return from parse:
return ParserExtensionParseResult();  // default = DISPLAY_ORIGINAL_ERROR

// Error return from parse:
return ParserExtensionParseResult("error message");  // DISPLAY_EXTENSION_ERROR

// Plan result construction:
ParserExtensionPlanResult result;
result.function = create_semantic_view_function;  // TableFunction
result.parameters = {Value("view_name"), ...};    // vector<Value>
result.return_type = StatementReturnType::QUERY_RESULT;  // or NOTHING for DDL
return result;
```

---

## Sources

- [DuckDB `parser_extension.hpp` (v1.4.1)](https://github.com/duckdb/duckdb/blob/v1.4.1/src/include/duckdb/parser/parser_extension.hpp) -- HIGH confidence, verbatim source
- [DuckDB `parser_extension.hpp` (main)](https://github.com/duckdb/duckdb/blob/main/src/include/duckdb/parser/parser_extension.hpp) -- HIGH confidence, shows `parser_override` addition
- [DuckDB `extension_statement.hpp` (v1.4.1)](https://github.com/duckdb/duckdb/blob/v1.4.1/src/include/duckdb/parser/statement/extension_statement.hpp) -- HIGH confidence
- [DuckDB `bind_extension.cpp` (v1.4.1)](https://github.com/duckdb/duckdb/blob/v1.4.1/src/planner/binder/statement/bind_extension.cpp) -- HIGH confidence
- [DuckDB `parser.cpp` (main)](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- HIGH confidence, fallback flow verified
- [prql extension source](https://github.com/ywelsch/duckdb-prql/blob/main/src/prql_extension.cpp) -- HIGH confidence, stash pattern reference implementation
- [prql extension header](https://github.com/ywelsch/duckdb-prql/blob/main/src/include/prql_extension.hpp) -- HIGH confidence, OperatorExtension usage
- [DuckDB semicolon inconsistency issue #18485](https://github.com/duckdb/duckdb/issues/18485) -- MEDIUM confidence, known bug
- [DuckDB runtime-extensible parsers blog post](https://duckdb.org/2024/11/22/runtime-extensible-parsers) -- MEDIUM confidence (overview only, no API details)
- [Project investigation doc `_notes/parser-extension-investigation.md`](file:///Users/paul/Documents/Dev/Personal/duckdb-semantic-views/_notes/parser-extension-investigation.md) -- HIGH confidence, project-specific analysis
