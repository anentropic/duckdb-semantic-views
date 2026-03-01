# Phase 11: CREATE SEMANTIC VIEW Parser Hook - Research

**Researched:** 2026-03-01
**Domain:** DuckDB C++ Parser Extension API + DDL grammar design
**Confidence:** HIGH (verified from vendored source + DuckDB build cache)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**DDL Syntax — Snowflake-compatible:**

```sql
CREATE [OR REPLACE] SEMANTIC VIEW [IF NOT EXISTS] <name>
  TABLES (
    alias AS physical_table [PRIMARY KEY (col [, ...])]
    [, ...]
  )
  [RELATIONSHIPS (
    from_alias(fk_col [, ...]) REFERENCES ref_alias
    [, ...]
  )]
  [FACTS (
    alias.name AS sql_expr
    [, ...]
  )]
  [DIMENSIONS (
    alias.name AS sql_expr
    [, ...]
  )]
  [METRICS (
    alias.name AS sql_expr
    [, ...]
  )]
```

- TABLES clause: all physical tables declared together. Table alias is used in subsequent clauses.
- RELATIONSHIPS: `from_alias(fk_col) REFERENCES ref_alias`. Parser infers equi-join condition. No raw SQL ON clause.
- FACTS: unaggregated computed values, scoped to a table alias. Requires adding `facts: Vec<Fact>` to the model.
- DIMENSIONS: SQL expressions, scoped to a table alias. No special TIME GRANULARITY annotation.
- METRICS: aggregation expressions, scoped to a table alias.
- FILTERS: NOT surfaced in DDL.
- `DROP SEMANTIC VIEW [IF EXISTS] <name>`

**Legacy Function Removal:**
- `define_semantic_view()` and `drop_semantic_view()` scalar functions removed in this phase (DDL-05).
- `src/ddl/define.rs` and `src/ddl/drop.rs` are deleted.
- Registration of those functions in `lib.rs` `init_extension` is removed.
- Existing JSON definitions in `semantic_layer._definitions` are kept as-is.
- PRAGMA callbacks remain registered — used internally by the parser hook.

**Error Messaging:**
- Syntax errors: standard DuckDB parser error style.
- Success: silent, no output rows. Matches DuckDB `CREATE TABLE` / `CREATE VIEW` convention.
- `DROP SEMANTIC VIEW IF EXISTS name`: succeeds silently if view does not exist.
- Parse-time validation: unknown table aliases in DIMENSIONS/METRICS/FACTS are caught at parse time.

### Claude's Discretion

- Connection strategy for the parser hook (whether to call PRAGMA via `persist_conn` or a different mechanism).
- How to determine the "base table" from the TABLES clause.

### Deferred Ideas (OUT OF SCOPE)

- COMMENT, SYNONYMS, AI_SQL_GENERATION, AI_QUESTION_CATEGORIZATION clauses
- PRIVATE/PUBLIC visibility modifiers on FACTS and METRICS
- Window function METRICS
- ASOF join support in RELATIONSHIPS
- Derived metrics
- FILTERS in DDL
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DDL-01 | User can create a semantic view with `CREATE SEMANTIC VIEW` SQL syntax | Parser hook: parse_function_t tokenizes and validates; plan_function_t returns TableFunction that calls persist_conn |
| DDL-02 | User can drop a semantic view with `DROP SEMANTIC VIEW` SQL syntax | Same parser hook handles DROP branch; IF EXISTS variant silently succeeds |
| DDL-03 | `CREATE OR REPLACE SEMANTIC VIEW` overwrites existing definition | Requires new `catalog_upsert` function in catalog.rs (skips duplicate check) |
| DDL-04 | Native DDL supports all capabilities of `define_semantic_view()` | Model changes: Join.on → FK pair, new Fact struct, TABLES clause maps to base_table + joins |
| DDL-05 | `define_semantic_view()` and `drop_semantic_view()` functions removed | Delete src/ddl/define.rs, src/ddl/drop.rs, remove registrations in lib.rs |
| DDL-06 | Non-semantic-view SQL is unaffected by parser hook | parse_function_t returns DISPLAY_ORIGINAL_ERROR for all non-matching input; DuckDB falls back to native parser |
</phase_requirements>

## Summary

Phase 11 replaces the scalar function DDL API with native SQL DDL via DuckDB's `ParserExtension` mechanism. The extension registers a `parse_function_t` + `plan_function_t` pair in `DBConfig::parser_extensions` at load time. The parse function matches `CREATE SEMANTIC VIEW` / `DROP SEMANTIC VIEW` tokens and returns a `ParserExtensionParseData` subclass containing the fully parsed definition. The plan function converts this to a `ParserExtensionPlanResult` wrapping a `TableFunction` that carries the parse data as bind data. DuckDB binds and executes this TableFunction, whose scan function writes to `semantic_layer._definitions` via `persist_conn` (separate connection, same pattern as the scalar invoke path).

**Primary recommendation:** Implement the parser extension entirely in `src/shim/shim.cpp` (the C++ layer already in place), register it via `DBConfig::GetConfig(db_instance).parser_extensions.push_back(...)` in `semantic_views_register_shim()`. Use `persist_conn` for DDL persistence — the same connection strategy as Phase 10 scalar functions. This avoids a new connection management complexity and reuses the established write path.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `duckdb/parser/parser_extension.hpp` | v1.2.1 (vendored) | Defines `ParserExtension`, `parse_function_t`, `plan_function_t` | The only official API for custom SQL syntax in DuckDB extensions |
| `duckdb/main/config.hpp` | v1.2.1 (vendored) | `DBConfig::GetConfig(db)` → access `parser_extensions` vector | Already included in shim.cpp |
| `duckdb/function/table_function.hpp` | v1.2.1 (vendored) | `TableFunction` for plan result execution | Already transitively included via `parser_extension.hpp` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `duckdb/common/string_util.hpp` | v1.2.1 (vendored) | `StringUtil::Replace` for quote escaping | Already included; used in pragma callbacks |

## Architecture Patterns

### Recommended Project Structure

No new files. All new C++ code goes into `src/shim/shim.cpp`. New Rust functions go into new file `src/ddl/parser_hook.rs` (state type carrying catalog + persist_conn for the TableFunction FunctionData).

```
src/
├── shim/shim.cpp     # All C++ parser extension code added here
├── shim/shim.h       # Add new FFI declarations if needed (none expected)
├── shim/mod.rs       # No changes expected (persist_conn path unchanged)
├── ddl/mod.rs        # Remove define/drop modules; no parser_hook module needed (all C++)
├── ddl/define.rs     # DELETED
├── ddl/drop.rs       # DELETED
├── model.rs          # Join struct change; new Fact struct
├── catalog.rs        # New catalog_upsert(); catalog_delete_if_exists()
└── lib.rs            # Remove DefineSemanticView / DropSemanticView registrations
```

### Pattern 1: DuckDB Parser Extension Registration

**What:** Register a `ParserExtension` struct in `DBConfig.parser_extensions` at load time.
**When to use:** Extension load time, inside `semantic_views_register_shim()` after pragmas are registered.

```cpp
// Source: verified from duckdb/src/include/duckdb/parser/parser_extension.hpp
//         and duckdb/src/include/duckdb/main/config.hpp

// In semantic_views_register_shim():
auto &config = DBConfig::GetConfig(db_instance);

ParserExtension ext;
ext.parse_function = SemanticViewsParseFunction;
ext.plan_function = SemanticViewsPlanFunction;
ext.parser_info = make_shared_ptr<SemanticViewsParserInfo>(catalog_state, persist_conn);
config.parser_extensions.push_back(ext);
```

`SemanticViewsParserInfo` subclasses `ParserExtensionInfo` and holds the catalog state pointer and `persist_conn` — both needed by the TableFunction scan function.

### Pattern 2: parse_function_t — Token-based DDL Parser

**What:** A function matching the `parse_function_t` signature that tokenizes the query string and returns a parsed definition or falls through.
**When to use:** Called by DuckDB only when its native PostgreSQL parser fails. Called for every statement that DuckDB can't parse natively.

```cpp
// Source: verified from duckdb/src/parser/parser.cpp
// parse_function_t is: ParserExtensionParseResult (*)(ParserExtensionInfo*, const string&)

static ParserExtensionParseResult SemanticViewsParseFunction(
    ParserExtensionInfo *info, const string &query) {

    // Trim and uppercase for keyword detection only
    auto trimmed = StringUtil::Upper(StringUtil::Strip(query));

    // Return DISPLAY_ORIGINAL_ERROR (fall through) for non-matching SQL.
    // DDL-06: this is the "pass through cleanly" guarantee.
    if (!StringUtil::StartsWith(trimmed, "CREATE") &&
        !StringUtil::StartsWith(trimmed, "DROP")) {
        return ParserExtensionParseResult();  // type = DISPLAY_ORIGINAL_ERROR
    }

    // Tokenize and parse. On error: return DISPLAY_EXTENSION_ERROR with message.
    // On success: return parsed data.
    try {
        auto parse_data = ParseSemanticViewDDL(query);
        return ParserExtensionParseResult(std::move(parse_data));
    } catch (const std::exception &e) {
        return ParserExtensionParseResult(e.what());  // DISPLAY_EXTENSION_ERROR
    }
}
```

**CRITICAL pitfall (DDL-06):** The parse function is called for EVERY statement DuckDB fails to parse natively. It must return `DISPLAY_ORIGINAL_ERROR` (the default-constructed result) immediately for any input that is not `CREATE SEMANTIC VIEW` / `DROP SEMANTIC VIEW`. Returning `DISPLAY_EXTENSION_ERROR` for non-matching input would suppress DuckDB's own error message.

### Pattern 3: ParserExtensionParseData Subclass

**What:** Holds the fully parsed DDL statement — everything needed for both the plan function and the TableFunction execution.

```cpp
enum class SemanticViewsDDLType { CREATE, DROP };

struct SemanticViewsDDLData : ParserExtensionParseData {
    SemanticViewsDDLType ddl_type;
    bool or_replace = false;
    bool if_not_exists = false;
    bool if_exists = false;   // for DROP IF EXISTS
    string view_name;
    // For CREATE: the fully serialized JSON definition
    string definition_json;

    unique_ptr<ParserExtensionParseData> Copy() const override { ... }
    string ToString() const override { return "SemanticViewsDDL:" + view_name; }
};
```

The parse function builds this in full — including constructing and serializing the `SemanticViewDefinition` to JSON — before returning. Parse-time validation (unknown alias references) happens here.

### Pattern 4: plan_function_t — Returns TableFunction for Execution

**What:** Called by DuckDB during statement binding. Converts parse data into a `ParserExtensionPlanResult` that DuckDB will bind and execute as a TableFunction scan.
**Critical constraint:** The plan function is called while the `ClientContext` lock is held (`context_lock` is a plain `std::mutex`). Calling `context.Query()` from within the plan function **deadlocks**. Similarly, `table_function_t` (the scan) also executes while the lock is held — no `context.Query()` from there either.

```cpp
// Source: verified from duckdb/src/planner/binder/statement/bind_extension.cpp
// plan_function_t is: ParserExtensionPlanResult (*)(ParserExtensionInfo*, ClientContext&,
//                                                   unique_ptr<ParserExtensionParseData>)

static ParserExtensionPlanResult SemanticViewsPlanFunction(
    ParserExtensionInfo *info_base, ClientContext &context,
    unique_ptr<ParserExtensionParseData> parse_data) {

    auto &info = *static_cast<SemanticViewsParserInfo*>(info_base);
    auto &ddl = *static_cast<SemanticViewsDDLData*>(parse_data.get());

    // Build a TableFunction that carries the DDL data.
    TableFunction func("semantic_view_ddl_exec", {}, SemanticViewsDDLScan,
                       SemanticViewsDDLBind);
    // No output columns — DDL returns nothing.

    ParserExtensionPlanResult result;
    result.function = func;
    result.parameters = {
        Value(ddl.view_name),
        Value(ddl.definition_json),
        Value((int32_t)ddl.ddl_type),
        Value((int32_t)ddl.or_replace),
        Value((int32_t)ddl.if_exists),
        // ... or pass a single JSON blob encoding all fields
    };
    result.modified_databases["main"] = {};  // signals write operation
    result.requires_valid_transaction = true;
    result.return_type = StatementReturnType::NOTHING;
    return result;
}
```

### Pattern 5: TableFunction Scan — Actual DDL Execution via persist_conn

**What:** The scan function is called during physical execution. It uses `persist_conn` (separate connection, pre-created at load time) to write to `semantic_layer._definitions`, then updates the in-memory catalog.
**Why persist_conn:** The context lock is held during the scan function's execution (verified in `ClientContext::ExecuteTaskInternal`). Direct `context.Query()` would deadlock. `persist_conn` is a second `duckdb_connection` on the same database — it has its own context, no lock conflict.

```cpp
static void SemanticViewsDDLScan(ClientContext &context,
                                  TableFunctionInput &data, DataChunk &output) {
    // output.SetCardinality(0) — DDL returns nothing

    auto &bind_data = data.bind_data->Cast<SemanticViewsDDLBindData>();

    if (bind_data.ddl_type == SemanticViewsDDLType::CREATE) {
        // 1. Persist via persist_conn (does NOT hold the context lock)
        int rc = semantic_views_pragma_define(
            bind_data.persist_conn,
            bind_data.view_name.c_str(),
            bind_data.definition_json.c_str());
        if (rc != 0 && !bind_data.or_replace) {
            // raise error
        }

        // 2. Update in-memory catalog
        if (bind_data.or_replace) {
            catalog_upsert(bind_data.catalog, bind_data.view_name,
                           bind_data.definition_json);
        } else {
            catalog_insert(bind_data.catalog, bind_data.view_name,
                           bind_data.definition_json);
        }
    } else { // DROP
        // Similar: persist_conn for DELETE, catalog_delete for in-memory
    }

    output.SetCardinality(0);
}
```

**Tradeoff:** Using `persist_conn` means `CREATE SEMANTIC VIEW` inside a `BEGIN/ROLLBACK` does NOT roll back the persistent write. This is the same limitation as the Phase 10 scalar functions and is an accepted tradeoff (documented in shim.cpp comments). Only PRAGMA DDL (issued as standalone statements) is truly transactional via `pragma_query_t`. The PRAGMA callbacks remain available as an escape hatch.

### Pattern 6: Registration in semantic_views_register_shim

The `ParserExtensionInfo` subclass must carry both `catalog_state` (a `CatalogState` = `Arc<RwLock<HashMap>>` shared pointer) and `persist_conn`. The challenge is passing Rust types into C++.

**Solution:** Store a raw pointer to the Rust catalog state (already pattern-established via `extra_info` in Rust table function registration). For the parser hook, capture the catalog pointer and persist_conn by passing them through `semantic_views_register_shim` as additional parameters, OR store them in a C++ struct that holds the raw pointers.

Since `semantic_views_register_shim` is called from Rust's `init_extension` after both the catalog and `persist_conn` are created, the simplest approach is to change the function signature to pass these:

```c
// shim.h addition:
void semantic_views_register_parser_hook(
    void* db_instance_ptr,
    void* catalog_raw_ptr,      // raw ptr to Arc<RwLock<HashMap>> (opaque to C++)
    duckdb_connection persist_conn
);
```

Or keep `semantic_views_register_shim` but add parameters. The shim stores them in the `ParserExtensionInfo` subclass by keeping a raw pointer to the catalog and the `persist_conn`. The C++ scan function calls back into Rust via a function pointer or FFI call.

**Alternative (simpler):** The scan function calls back to Rust FFI functions (`semantic_views_pragma_define`, `semantic_views_pragma_drop`) which already exist and handle both the persist and the in-memory update. But currently `semantic_views_pragma_define`/`_drop` only do the SQL write, not the catalog update. The catalog update still needs to go through Rust.

**Best approach:** The scan function calls `semantic_views_pragma_define` or `semantic_views_pragma_drop` for the SQL write, then calls new Rust FFI functions `semantic_views_catalog_insert`, `semantic_views_catalog_upsert`, `semantic_views_catalog_delete`, `semantic_views_catalog_delete_if_exists` that update the in-memory catalog. These Rust functions accept a raw catalog pointer + name + json as C strings.

### Pattern 7: DDL Grammar — Hand-written Tokenizer

No grammar library is needed. The DDL grammar is fixed and simple. A hand-written recursive descent tokenizer in C++ handles the keywords. This avoids adding a dependency and keeps the build simple.

Token sequence for CREATE:
```
CREATE [OR REPLACE] SEMANTIC VIEW [IF NOT EXISTS] <name>
TABLES ( <alias> AS <table> [PRIMARY KEY (<cols>)] [, ...] )
[RELATIONSHIPS ( <from_alias>(<fk_cols>) REFERENCES <ref_alias> [, ...] )]
[FACTS ( <alias>.<name> AS <expr> [, ...] )]
[DIMENSIONS ( <alias>.<name> AS <expr> [, ...] )]
[METRICS ( <alias>.<name> AS <expr> [, ...] )]
```

Token sequence for DROP:
```
DROP SEMANTIC VIEW [IF EXISTS] <name>
```

**Expression parsing:** The `AS <sql_expr>` portion may contain commas (e.g., `COALESCE(a, b)`) and nested parentheses. The parser must track parenthesis depth when consuming expressions. Each item in a clause list is terminated by a `,` at depth 0, and the clause ends at `)` at depth 0.

### Anti-Patterns to Avoid

- **Calling `context.Query()` from plan function or scan function:** deadlock — the context lock (`std::mutex context_lock`) is held. Verified in `ClientContext::LockContext()` using `lock_guard<mutex>`.
- **Returning `DISPLAY_EXTENSION_ERROR` for non-matching SQL:** this silences DuckDB's own parser errors for valid SQL that the extension doesn't handle (DDL-06 regression).
- **Parsing in the plan function:** parse_function_t is called first; plan_function_t receives `unique_ptr<ParserExtensionParseData>` already filled. Don't re-parse.
- **Using `context.Query()` in the plan function to call PRAGMA:** deadlock, same reason.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parser extension API | Custom FFI hooks | `parser_extension.hpp` `ParserExtension` struct | The only public API; direct C++ use is mandatory |
| TableFunction for DDL | Macro/macro hack | Standard `TableFunction` with empty output columns | Established pattern (TPCH uses this for create-then-fill operations) |
| SQL execution from scan | Re-entrant locking | `persist_conn` (pre-created separate connection) | Context lock is non-reentrant (`std::mutex`); second lock from same thread = deadlock |
| Grammar library | PEG/LALR parser | Hand-written tokenizer | Fixed grammar, no external dependency, builds with `cc` crate as-is |

**Key insight:** The `plan_function_t` + `TableFunction` pattern is the ONLY way to implement custom DDL in a DuckDB extension without modifying DuckDB's core C++ code. There is no simpler path.

## Common Pitfalls

### Pitfall 1: Context Lock Deadlock
**What goes wrong:** Calling `context.Query()` from `plan_function_t` or the scan `table_function_t` hangs indefinitely.
**Why it happens:** `ClientContext::Query(const string&, ...)` calls `LockContext()` which does `lock_guard<mutex>(context_lock)`. During planning and execution, `context_lock` is already held on the same thread. `std::mutex` is not reentrant.
**How to avoid:** All catalog writes from the parser hook go through `persist_conn` (separate `duckdb_connection`). This is the same pattern as the scalar invoke path.
**Warning signs:** Hang on `CREATE SEMANTIC VIEW` — no error, no result, process stalled.

### Pitfall 2: parse_function_t Falls Through Incorrectly
**What goes wrong:** DDL-06 regression — standard SQL like `SELECT 1` triggers parser errors instead of executing.
**Why it happens:** `DISPLAY_EXTENSION_ERROR` returned for input that is not CREATE/DROP SEMANTIC VIEW.
**How to avoid:** The parse function must return the default `ParserExtensionParseResult()` (which constructs with `type = DISPLAY_ORIGINAL_ERROR`) for any input that doesn't start with matching tokens.
**Warning signs:** `SELECT 1` fails after extension load; integration tests fail on basic queries.

### Pitfall 3: Expression Parsing Terminates Prematurely
**What goes wrong:** A metric like `COALESCE(a, b)` gets truncated at the comma inside the function call.
**Why it happens:** Naive comma-split without tracking parenthesis depth.
**How to avoid:** Track paren depth; only split on commas at depth 0.
**Warning signs:** Parse errors on expressions containing nested function calls.

### Pitfall 4: Model Backwards Compatibility Break
**What goes wrong:** Existing JSON definitions stored in `semantic_layer._definitions` fail to load after model changes.
**Why it happens:** Changing `Join.on: String` to a new struct without `#[serde(default)]` breaks deserialization of old rows.
**How to avoid:** Keep `Join` with BOTH the old `on: String` (marked `#[serde(default, skip_serializing_if = "String::is_empty")]`) and the new FK fields. Use serde `#[serde(default)]` on new fields. Existing rows load fine; new DDL writes the new format.
**Warning signs:** `catalog_insert` fails on extension reload with existing definitions.

### Pitfall 5: Base Table Selection Ambiguity (Claude's Discretion Resolution)
**What goes wrong:** `semantic_query` fails because the inferred `base_table` is wrong.
**Why it happens:** Snowflake has no concept of a base table; our model requires one.
**How to avoid (RECOMMENDATION):** Define "base table" as the table alias that is NOT referenced as the target of any REFERENCES clause. If all tables are referenced (circular), use the first declared table. If only one table, it is the base table. This is deterministic and intuitive.
**Warning signs:** `semantic_query` uses the wrong table in the FROM clause.

### Pitfall 6: `CREATE OR REPLACE` Duplicate Check
**What goes wrong:** `CREATE OR REPLACE` fails with "already exists" from `catalog_insert`.
**Why it happens:** `catalog_insert` in `catalog.rs` checks for duplicates before writing.
**How to avoid:** Add `catalog_upsert(state, name, json)` that skips the duplicate check and overwrites.
**Warning signs:** `CREATE OR REPLACE SEMANTIC VIEW` always errors when view exists.

### Pitfall 7: Parser Hook Registration Order
**What goes wrong:** Parser extension not called; custom DDL silently falls to native parser error.
**Why it happens:** `parser_extensions` vector on `DBConfig` must be populated before any statements are parsed. If registration happens after the first query, existing connections won't see the extension.
**How to avoid:** Register in `semantic_views_register_shim()` which is called at load time before any user queries.
**Warning signs:** `CREATE SEMANTIC VIEW` always returns "syntax error near SEMANTIC" without custom error message.

## Code Examples

### C++ Parser Extension Registration

```cpp
// Source: verified from duckdb/src/include/duckdb/main/config.hpp lines 252, 279-280
//         and duckdb/src/planner/binder/statement/bind_extension.cpp

void semantic_views_register_shim(void* db_instance_ptr) {
    auto* db_c = reinterpret_cast<duckdb_database>(db_instance_ptr);
    auto* wrapper = reinterpret_cast<DatabaseWrapper*>(db_c->internal_ptr);
    DatabaseInstance& db_instance = *wrapper->database->instance;

    // ... existing PRAGMA registration ...

    // Register parser extension
    auto &config = DBConfig::GetConfig(db_instance);
    ParserExtension ext;
    ext.parse_function = SemanticViewsParseFunction;
    ext.plan_function = SemanticViewsPlanFunction;
    ext.parser_info = make_shared_ptr<SemanticViewsParserInfo>(...);
    config.parser_extensions.push_back(ext);
}
```

### parse_function_t Fall-through (DDL-06)

```cpp
// Source: verified from duckdb/src/parser/parser.cpp lines 271-292
// Result types:
//   ParserExtensionParseResult()         = DISPLAY_ORIGINAL_ERROR (fall through)
//   ParserExtensionParseResult("msg")    = DISPLAY_EXTENSION_ERROR (custom error)
//   ParserExtensionParseResult(data_ptr) = PARSE_SUCCESSFUL

static ParserExtensionParseResult SemanticViewsParseFunction(
    ParserExtensionInfo *info, const string &query) {

    string upper = StringUtil::Upper(query);
    // Fast prefix check — only try to parse if it looks like our DDL
    bool is_create = upper.find("CREATE") != string::npos &&
                     upper.find("SEMANTIC") != string::npos;
    bool is_drop = upper.find("DROP") != string::npos &&
                   upper.find("SEMANTIC") != string::npos;

    if (!is_create && !is_drop) {
        return ParserExtensionParseResult();  // DISPLAY_ORIGINAL_ERROR — fall through
    }

    try {
        auto data = ParseSemanticViewStatement(query);
        return ParserExtensionParseResult(std::move(data));
    } catch (std::exception &e) {
        return ParserExtensionParseResult(string(e.what()));
    }
}
```

### plan_function_t with NOTHING return type

```cpp
// Source: verified from duckdb/src/include/duckdb/common/enums/statement_type.hpp
//         StatementReturnType::NOTHING = "the statement returns nothing"

static ParserExtensionPlanResult SemanticViewsPlanFunction(
    ParserExtensionInfo *info_base, ClientContext &context,
    unique_ptr<ParserExtensionParseData> parse_data) {

    auto &info = static_cast<SemanticViewsParserInfo&>(*info_base);
    auto &ddl = static_cast<SemanticViewsDDLData&>(*parse_data);

    // Build zero-output TableFunction
    TableFunction func;
    func.name = "semantic_view_ddl_exec";
    func.function = SemanticViewsDDLScan;
    func.bind = SemanticViewsDDLBind;

    ParserExtensionPlanResult result;
    result.function = func;
    result.parameters.push_back(Value(ddl.view_name));
    result.parameters.push_back(Value(ddl.definition_json));
    result.parameters.push_back(Value((int32_t)ddl.ddl_type));
    result.parameters.push_back(Value((bool)ddl.or_replace));
    result.parameters.push_back(Value((bool)ddl.if_exists));

    result.requires_valid_transaction = true;
    result.return_type = StatementReturnType::NOTHING;
    // modified_databases signals to DuckDB that this statement modifies data.
    // Use empty CatalogIdentity — we don't have the exact catalog OID here.
    result.modified_databases["main"] = {};

    return result;
}
```

### Rust: New catalog functions needed

```rust
// Source: derived from existing catalog.rs patterns

/// Upsert: insert or replace. For CREATE OR REPLACE.
/// Validates JSON before writing; overwrites existing entry.
pub fn catalog_upsert(
    state: &CatalogState,
    name: &str,
    json: &str,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    SemanticViewDefinition::from_json(name, json)
        .map_err(Box::<dyn std::error::Error>::from)?;
    state.write().unwrap().insert(name.to_string(), json.to_string());
    Ok(())
}

/// Delete if exists. For DROP IF EXISTS.
/// Silently succeeds if view not present.
pub fn catalog_delete_if_exists(state: &CatalogState, name: &str) {
    state.write().unwrap().remove(name);
}
```

### Rust model.rs changes

```rust
// New Fact struct (parallel to Dimension/Metric)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Fact {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub source_table: Option<String>,
}

// Updated Join struct — support both old and new format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Join {
    pub table: String,
    // Old format (Phase 10 and earlier): raw SQL ON clause
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,
    // New format (Phase 11): FK-style REFERENCES relationship
    #[serde(default)]
    pub from_cols: Vec<String>,   // FK column names from this table
    // to_table is captured in Join.table already
}

// SemanticViewDefinition additions
pub struct SemanticViewDefinition {
    pub base_table: String,
    #[serde(default)]
    pub facts: Vec<Fact>,          // NEW in Phase 11
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default)]
    pub joins: Vec<Join>,
}
```

Note: `#[serde(deny_unknown_fields)]` must be REMOVED from `SemanticViewDefinition` to allow old JSON to load without the `facts` field. Since `facts` has `#[serde(default)]`, this is backward-compatible. But `deny_unknown_fields` would reject JSON from old `define_semantic_view()` calls that don't have `facts`. Use `#[serde(default)]` on the struct fields instead.

Actually — `deny_unknown_fields` prevents loading JSON written by future versions. Since we're ADDING the `facts` field, old JSON (without `facts`) loads fine with `#[serde(default)]`. `deny_unknown_fields` is fine. New JSON (with `facts`) loads fine in new code. No change needed here.

### Snowflake DDL → Internal Model Mapping

Snowflake syntax maps to `SemanticViewDefinition` as follows:

```
TABLES clause:
  First non-referenced alias → base_table: "physical_table"
  Remaining aliases → joins: [{table: "alias", on: "", from_cols: ["fk_col"]}]
  (base table determination: first table not referenced as REFERENCES target)

RELATIONSHIPS clause:
  from_alias(fk_cols) REFERENCES ref_alias →
    Join { table: from_alias, from_cols: [fk_cols] }
    (ref_alias is the base_table or another join table)

FACTS clause:
  alias.name AS expr → Fact { name, expr, source_table: Some(alias) }

DIMENSIONS clause:
  alias.name AS expr → Dimension { name, expr, source_table: Some(alias), ... }

METRICS clause:
  alias.name AS expr → Metric { name, expr, source_table: Some(alias) }
```

**Base table determination (Claude's Discretion Resolution):**
Define base_table as the table alias that is NOT the `from_alias` in any RELATIONSHIPS entry. If no RELATIONSHIPS, the first declared TABLES entry is the base table. This is deterministic and matches the intuitive "primary" table.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Scalar `define_semantic_view(name, json)` | `CREATE SEMANTIC VIEW name TABLES (...) DIMENSIONS (...) METRICS (...)` | Phase 11 | Users write SQL, not JSON |
| Phase 10 PRAGMA path (pragma_query_t, transactional) | Parser hook with TableFunction + persist_conn (non-transactional write) | Phase 11 | Same ROLLBACK limitation as scalar path; acceptable |
| `Join.on: String` (raw SQL) | `Join.from_cols: Vec<String>` (FK semantics) | Phase 11 | Backwards-compatible via `#[serde(default)]` |

**Deprecated/outdated after Phase 11:**
- `src/ddl/define.rs` / `src/ddl/drop.rs` — removed
- `define_semantic_view()` / `drop_semantic_view()` scalar functions — removed
- Phase 2 DDL tests in `test/sql/phase2_ddl.test` — rewritten for native DDL

## Open Questions

1. **C++ TableFunction accessing Rust catalog state**
   - What we know: The plan function carries `ParserExtensionInfo*` which can hold a raw pointer to the Rust `CatalogState` (an `Arc<RwLock<HashMap>>`). New Rust FFI functions (e.g., `semantic_views_catalog_insert`, `semantic_views_catalog_upsert`, `semantic_views_catalog_delete`) exposed via `shim.h` can accept this raw pointer and the view name/json as C strings.
   - What's unclear: Whether passing a raw `Arc` pointer as `void*` requires a `Arc::into_raw` / `Arc::from_raw` pattern in Rust to ensure the refcount stays alive. Answer: YES — the shim must call `Arc::into_raw` in Rust and pass the resulting pointer. The shim stores it; Rust calls `Arc::from_raw` to increment the refcount when calling the catalog FFI functions.
   - Recommendation: Define new extern "C" Rust functions for catalog mutation that accept `*const CatalogState` (opaque), name `*const c_char`, json `*const c_char`. Export via `shim.h`. Keep simple — one function per catalog operation.

2. **`modified_databases` map — correct catalog OID**
   - What we know: `ParserExtensionPlanResult.modified_databases` is `unordered_map<string, StatementProperties::CatalogIdentity>`. `CatalogIdentity` has `catalog_oid` and `catalog_version`. DuckDB uses this to track which databases a statement modifies.
   - What's unclear: Whether leaving `modified_databases` empty vs. inserting `{"main", {}}` (zero OID) causes any correctness or transaction issues.
   - Recommendation: Insert `{"main", {}}` with zero OID to signal a write. This matches the TPCH pattern (which also doesn't have exact OIDs at plan time). DuckDB does not enforce OID correctness for extension-defined statements.

3. **`#[serde(deny_unknown_fields)]` on SemanticViewDefinition with new `facts` field**
   - What we know: `deny_unknown_fields` rejects JSON that has fields not in the struct. Adding `facts: Vec<Fact>` to the struct means old JSON (without `facts`) still loads fine (it's `#[serde(default)]`). New JSON (with `facts`) loads fine in new code.
   - What's unclear: Whether any in-the-wild JSON was written with extra fields that `deny_unknown_fields` would reject.
   - Recommendation: Keep `deny_unknown_fields`. The only source of stored JSON is our own code; no unknown fields expected. If migration concerns arise, remove it — but it's defensive.

## Sources

### Primary (HIGH confidence)
- `/target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/parser/parser_extension.hpp` — Full parser extension API
- `/target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/planner/binder/statement/bind_extension.cpp` — Confirmed plan_function_t call site during binder, context lock held
- `/target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/parser/parser.cpp` lines 271-292 — Confirmed parse_function_t call site, fall-through logic
- `/target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/main/client_context.cpp` lines 168, 970 — Confirmed `context_lock` is `std::mutex` (non-reentrant); `Query()` calls `LockContext()` first
- `/target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb/function/table_function.hpp` — TableFunction API, `table_function_t`, `StatementReturnType::NOTHING`
- `/duckdb_capi/duckdb/parser/parser_extension.hpp` — Vendored header, confirms `ParserExtensionPlanResult.modified_databases` type
- `/duckdb_capi/duckdb/main/config.hpp` line 252 — `DBConfig.parser_extensions: vector<ParserExtension>`
- `/duckdb_capi/duckdb/main/extension/extension_loader.hpp` — ExtensionLoader has no parser extension registration method; must use DBConfig directly
- WebFetch: `github.com/duckdb/duckdb/v1.2.1/extension/tpch/dbgen/dbgen.cpp` — TPCH uses catalog APIs (not `context.Query()`) in scan functions
- WebFetch: Snowflake `CREATE SEMANTIC VIEW` documentation — confirmed DDL syntax for target grammar

### Secondary (MEDIUM confidence)
- WebSearch → "QuackPlanFunction" example pattern (parser extension demo from DuckDB test suite, not directly verified from DuckDB v1.2.1 source but consistent with all other verified findings)
- WebFetch: DuckDB CIDR 2025 paper (PDF inaccessible, but abstract confirms parser extension mechanism is stable and used in production)

### Tertiary (LOW confidence)
- None — all critical findings verified from authoritative sources above.

## Metadata

**Confidence breakdown:**
- Standard stack (parser extension API): HIGH — verified from vendored headers and build cache source
- Architecture (plan function → TableFunction → scan): HIGH — verified from bind_extension.cpp and client_context.cpp source
- Context lock deadlock: HIGH — verified from client_context.cpp `std::mutex context_lock`, `LockContext()` implementation
- DDL grammar design: HIGH — CONTEXT.md decisions locked; Snowflake docs consulted for exact syntax
- Pitfalls: HIGH — derived from verified source code behavior
- Model changes: HIGH — derived from existing model.rs and CONTEXT.md locked decisions

**Research date:** 2026-03-01
**Valid until:** 2026-06-01 (stable API; DuckDB parser extension mechanism is not fast-moving)
