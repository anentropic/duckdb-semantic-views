# Feature Landscape: v0.5.1 DDL Polish

**Domain:** DuckDB Rust extension -- completing the native DDL surface for semantic views
**Researched:** 2026-03-08
**Milestone:** v0.5.1 -- Extended DDL verbs + error reporting + README docs
**Status:** Subsequent milestone research (v0.5.0 shipped 2026-03-08)
**Overall confidence:** HIGH (all function-based implementations exist; parser hook architecture proven; DuckDB DDL semantics verified from official docs; Snowflake reference model verified)

---

## Scope

This document covers the feature surface for v0.5.1: six additional DDL verbs via parser hooks (DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW, DROP IF EXISTS), error location reporting in DDL parse errors, and README documentation. All underlying function-based implementations are already built and tested in v0.1.0-v0.5.0.

**Focus:** DDL statement detection and routing via parser hook, expected behavior per statement type, error reporting patterns, dependencies on existing code.

---

## Table Stakes

Features that complete the DDL surface. Without these, native DDL feels half-finished.

| Feature | Why Expected | Complexity | Dependencies | Notes |
|---------|--------------|------------|--------------|-------|
| `DROP SEMANTIC VIEW name` | Every CREATE needs a DROP. DuckDB has `DROP TABLE`, `DROP VIEW`, `DROP SEQUENCE`. SQL fundamentals. | **Low** | Parser hook detection; existing `drop_semantic_view()` function | Rewrite to `SELECT * FROM drop_semantic_view('name')`. Already registered in `lib.rs` with `DropState { if_exists: false }`. |
| `DROP SEMANTIC VIEW IF EXISTS name` | Standard DuckDB pattern. Scripts that run repeatedly need idempotent drops. DuckDB docs: "do not throw an error if the view does not exist." | **Low** | Parser hook detection; existing `drop_semantic_view_if_exists()` function | Rewrite to `SELECT * FROM drop_semantic_view_if_exists('name')`. Already registered with `DropState { if_exists: true }`. |
| `CREATE OR REPLACE SEMANTIC VIEW name (...)` | Standard DuckDB pattern (`CREATE OR REPLACE VIEW`). DuckDB docs: "if a view of the same name already exists, it is replaced." Snowflake supports it for semantic views. Avoids DROP+CREATE. | **Low** | Parser hook detection; existing `create_or_replace_semantic_view()` function | Rewrite to `SELECT * FROM create_or_replace_semantic_view('name', ...)`. Already registered with `DefineState { or_replace: true }`. |
| `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | Standard DuckDB pattern (`CREATE TABLE IF NOT EXISTS`). Safety for migration scripts. Silently succeeds if view already exists. | **Low** | Parser hook detection; existing `create_semantic_view_if_not_exists()` function | Rewrite to `SELECT * FROM create_semantic_view_if_not_exists('name', ...)`. Already registered with `DefineState { if_not_exists: true }`. |
| `DESCRIBE SEMANTIC VIEW name` | Inspection is expected alongside creation. DuckDB has `DESCRIBE tbl`. Snowflake has `DESCRIBE SEMANTIC VIEW`. Users need to see what they built. | **Low** | Parser hook detection; existing `describe_semantic_view()` function | Rewrite to `SELECT * FROM describe_semantic_view('name')`. Keep existing output format (name, base_table, dimensions, metrics, filters, joins) for v0.5.1. |
| `SHOW SEMANTIC VIEWS` | Listing is expected alongside creation. DuckDB has `SHOW TABLES`. Snowflake has `SHOW SEMANTIC VIEWS`. Users need to discover what exists. | **Low** | Parser hook detection; existing `list_semantic_views()` function | Rewrite to `SELECT * FROM list_semantic_views()`. Returns `(name, base_table)` rows. |
| README DDL syntax reference | Users need to know the syntax. The extension currently has no user-facing documentation for the DDL surface. | **Low** | All DDL verbs implemented and tested | Minimal: syntax reference + 1-2 worked examples. Must follow existing repo style. |

## Differentiators

Features that improve the DX beyond "it works." Not strictly expected, but significantly improve the user experience.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| Clause-level error hints | "Error in DIMENSIONS clause: ..." vs generic "parse failed." Tells users exactly where to look. | **Medium** | `parse_args.rs` error message formatting | Extend error messages in `parse_args.rs` to include clause name context. No byte-position tracking needed -- structured error strings. |
| "Did you mean" for DDL clause names | Auto-suggests `dimensions` when user types `dimesions` | **Low** | `strsim` crate (already in deps); `suggest_closest()` (already in `expand.rs`) | Fixed vocabulary of 4 keywords: `tables`, `relationships`, `dimensions`, `metrics`. Apply `suggest_closest()` when unknown keyword arg encountered. |
| "Did you mean" for view names in DROP/DESCRIBE | Helps when view name is misspelled. "Semantic view 'saels_view' not found. Did you mean 'sales_view'?" | **Low** | Catalog access at bind time; `suggest_closest()` | Already implemented at query time (`QueryError::ViewNotFound`). Same pattern for DDL. |
| README worked examples | Users see end-to-end usage with real-ish data. Copy-paste ready. | **Low** | All DDL verbs implemented | 1-2 examples covering create, query, describe, drop lifecycle. |

## Anti-Features

Features to explicitly NOT build in v0.5.1.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| `ALTER SEMANTIC VIEW` | Not in Snowflake (they use `CREATE OR REPLACE`). Not in DuckDB for views. Adds complexity without clear benefit. | Use `CREATE OR REPLACE SEMANTIC VIEW`. Document this as the update path. |
| Schema-qualified names (`myschema.myview`) | Semantic views live in a flat in-memory HashMap. Schema qualification requires fundamental architecture changes to use DuckDB's catalog system. | Keep simple names. Document the limitation. |
| `SHOW SEMANTIC VIEWS LIKE '%pattern%'` / `STARTS WITH` / `LIMIT` | Snowflake has this. Our list is small enough that client-side filtering suffices. Over-engineering for v0.5.1. | Return all views. Users can wrap in `SELECT * FROM list_semantic_views() WHERE name LIKE ...` |
| `DESC` as alias for `DESCRIBE` | DuckDB supports `DESC` as alias. Requires detecting a different prefix (`desc semantic view`). Minor convenience vs. added parser complexity. | Support `DESCRIBE` only. Add `DESC` later if users request it. |
| Byte-exact error positions in DuckDB's structured error format | DuckDB's internal `ErrorData` includes position tracking. Parser extensions return error strings, not structured errors. Matching the internal format requires C++ `BinderException` customization. | Use human-readable clause hints in the error message text. |
| Full SQL parser for DDL body (sqlparser, nom, pest) | Grammar is 7 prefix patterns + clause extraction. A parser framework adds ~500KB dependency for no benefit. The DDL body is already parsed by DuckDB's STRUCT/LIST literal parser. | Hand-written prefix detection + clause scanning in `parse.rs`. |
| `COPY GRANTS` on `CREATE OR REPLACE` | Snowflake supports this for permission transfer. DuckDB has no grant system for extension-defined objects. | Not applicable. |
| `TERSE` mode for `SHOW SEMANTIC VIEWS` | Snowflake supports `SHOW TERSE SEMANTIC VIEWS`. Unnecessary when the output is already only 2 columns. | Not applicable. |
| ANSI-colored terminal error output | DuckDB error channel is plain text. Colors render as garbage in JDBC/ODBC/Python clients. | Follow DuckDB's plain-text `Did you mean` / `Hint:` conventions. |
| Row-per-field DESCRIBE format | Snowflake returns one row per dimension/metric/table. More readable but requires a new VTab with different output schema. | Defer to future milestone. Keep existing `describe_semantic_view()` output for v0.5.1. |
| `miette` / `ariadne` / `codespan-reporting` | Terminal diagnostic renderers are incompatible with DuckDB's plain-text error channel. | Hand-crafted `fmt::Display` matching DuckDB error style. |
| DDL validation of SQL expressions at define time | Validating dimension/metric exprs adds fragile coupling. Expression validity depends on the source table existing. | DuckDB validates at query time via LIMIT 0 type inference. Errors surface naturally. |

---

## Detailed Design: Statement Detection and Routing

### Parser Hook Architecture (Recap)

The parser extension fallback fires when DuckDB's own parser fails on a statement:

1. DuckDB parser tries to parse the SQL
2. If parsing fails, DuckDB splits by `;` and calls each registered `parse_function`
3. `parse_function` returns PARSE_SUCCESSFUL (with data) or DISPLAY_ORIGINAL_ERROR
4. If successful, DuckDB calls `plan_function` to get a TableFunction + parameters

**Critical insight:** DuckDB's parser will fail on ALL semantic view statements because `SEMANTIC VIEW` / `SEMANTIC VIEWS` is not a recognized object type after `CREATE`, `DROP`, `DESCRIBE`, or `SHOW`. The fallback will fire for every statement type.

### Detection Table

| Statement | DuckDB parser result | Prefix to detect (case-insensitive) | Rewrite target |
|-----------|---------------------|--------------------------------------|----------------|
| `CREATE SEMANTIC VIEW name (...)` | Fails on `SEMANTIC` after `CREATE` | `create semantic view` | `SELECT * FROM create_semantic_view('name', ...)` |
| `CREATE OR REPLACE SEMANTIC VIEW name (...)` | Fails on `SEMANTIC` after `REPLACE` | `create or replace semantic view` | `SELECT * FROM create_or_replace_semantic_view('name', ...)` |
| `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | Fails on `SEMANTIC` after `CREATE` | `create semantic view if not exists` | `SELECT * FROM create_semantic_view_if_not_exists('name', ...)` |
| `DROP SEMANTIC VIEW name` | Fails on `SEMANTIC` after `DROP` | `drop semantic view` | `SELECT * FROM drop_semantic_view('name')` |
| `DROP SEMANTIC VIEW IF EXISTS name` | Fails on `SEMANTIC` after `DROP` | `drop semantic view if exists` | `SELECT * FROM drop_semantic_view_if_exists('name')` |
| `DESCRIBE SEMANTIC VIEW name` | Fails on `SEMANTIC` after `DESCRIBE` | `describe semantic view` | `SELECT * FROM describe_semantic_view('name')` |
| `SHOW SEMANTIC VIEWS` | Fails on `SEMANTIC` after `SHOW` | `show semantic views` | `SELECT * FROM list_semantic_views()` |

### Detection Order (Longest-First to Avoid False Matches)

The detection function must check longer prefixes first:

1. `create or replace semantic view` (35 chars -- longest CREATE variant)
2. `create semantic view if not exists` (34 chars -- second longest)
3. `create semantic view` (20 chars -- base CREATE)
4. `drop semantic view if exists` (28 chars -- longer DROP variant)
5. `drop semantic view` (18 chars -- base DROP)
6. `describe semantic view` (22 chars -- DESCRIBE)
7. `show semantic views` (19 chars -- SHOW; note plural "views")

### Statement Type Enum

Replace the current boolean (PARSE_NOT_OURS / PARSE_DETECTED) with a statement type:

```
0 = NOT_OURS
1 = CREATE
2 = CREATE_OR_REPLACE
3 = CREATE_IF_NOT_EXISTS
4 = DROP
5 = DROP_IF_EXISTS
6 = DESCRIBE
7 = SHOW
```

This type is communicated from `sv_parse_rust` through `SemanticViewParseData` to `sv_plan_function`, which routes to the correct rewrite.

### Rewrite Patterns

**CREATE variants** (types 1-3): Same as current -- extract `(name, body)`, rewrite to `SELECT * FROM <function>('name', body)`.

**DROP variants** (types 4-5): Extract `name` after the prefix. No body (no parentheses). Rewrite to `SELECT * FROM drop_semantic_view[_if_exists]('name')`.

**DESCRIBE** (type 6): Extract `name` after the prefix. No body. Rewrite to `SELECT * FROM describe_semantic_view('name')`.

**SHOW** (type 7): No name, no body. Rewrite directly to `SELECT * FROM list_semantic_views()`.

---

## Detailed Design: DuckDB DDL Behavioral Semantics

### DROP behavior (from DuckDB official docs)

- `DROP <type> <name>` -- error if object does not exist
- `DROP <type> IF EXISTS <name>` -- no-op if object does not exist, no error
- DuckDB supports `CASCADE` and `RESTRICT` but these are irrelevant for semantic views (no inter-object dependencies in the extension catalog)
- Existing `drop_semantic_view()` already errors on missing view; `drop_semantic_view_if_exists()` already silently succeeds

### CREATE OR REPLACE behavior (from DuckDB official docs)

- "If a view of the same name already exists, it is replaced"
- Atomically deletes old and creates new (in Snowflake: "single transaction")
- NOT a silent no-op -- it always overwrites
- Existing `create_or_replace_semantic_view()` already uses `catalog_upsert()`

### CREATE IF NOT EXISTS behavior (from DuckDB official docs)

- Silently succeeds (no-op) if the object already exists
- Does NOT overwrite the existing object
- Existing `create_semantic_view_if_not_exists()` already catches "already exists" error and swallows it

### DESCRIBE behavior (from DuckDB official docs)

- DuckDB's `DESCRIBE tbl` returns: `column_name`, `column_type`, `null`, `key`, `default`, `extra`
- `SHOW` is an alias for `DESCRIBE` when used with a table name
- Our `describe_semantic_view()` returns semantic view metadata, not column schema -- this is intentional and different from DuckDB's `DESCRIBE`

### SHOW TABLES behavior (from DuckDB official docs)

- `SHOW TABLES` returns a `name` column
- `SHOW ALL TABLES` returns `database`, `schema`, `table_name`, `column_names`, `column_types`, `temporary`
- Our `SHOW SEMANTIC VIEWS` returns `(name, base_table)` -- appropriate for the object type

### Snowflake reference (closest comparable system)

- `CREATE [OR REPLACE] SEMANTIC VIEW [IF NOT EXISTS] <name> ...` -- both modifiers supported
- `DROP SEMANTIC VIEW [IF EXISTS] <name>` -- standard DROP with IF EXISTS
- `DESCRIBE SEMANTIC VIEW <name>` (alias `DESC`) -- returns row-per-entity format with `object_kind`, `object_name`, `parent_entity`, `property`, `property_value`
- `SHOW [TERSE] SEMANTIC VIEWS [LIKE pattern] [IN scope] [LIMIT n]` -- rich filtering with 8 output columns

---

## Detailed Design: Error Location Reporting

### Current Error Landscape

Errors in semantic view DDL come from two places:

1. **Parse-time errors** (in `parse.rs`): Missing view name, missing parens, malformed structure. Returned as strings via `sv_execute_ddl_rust`.
2. **Bind-time errors** (in `parse_args.rs` / `define.rs`): Invalid STRUCT fields, type mismatches, duplicate views. Returned as DuckDB `BinderException` via `sv_ddl_bind` in the C++ shim.

### Clause-Level Hints (Medium Complexity, High Value)

The DDL body is a comma-separated list of keyword arguments:
```sql
CREATE SEMANTIC VIEW name (
    tables := [...],
    dimensions := [...],
    metrics := [...]
)
```

When an error occurs, the message should indicate which clause failed:
- "Error in TABLES clause: expected list of {alias, table} structs"
- "Error in DIMENSIONS clause: missing required field 'expr' in dimension at index 2"
- "Error in METRICS clause: missing required field 'source_table' in metric 'revenue'"

**Implementation:** Extend `parse_args.rs` error messages to include the clause name. This is primarily string formatting changes -- no byte-position tracking needed.

### "Did You Mean" Suggestions (Low Complexity, High Value)

Already implemented at query time in `expand.rs::suggest_closest()` using `strsim::levenshtein` with threshold of 3. Extend to DDL-time:

- **Misspelled clause keywords**: `table` -> "Did you mean 'tables'?" (vocabulary: tables, relationships, dimensions, metrics)
- **Misspelled STRUCT field names**: `alais` -> "Did you mean 'alias'?" (vocabulary per clause: alias/table, name/expr/source_table, from_table/to_table/join_columns)
- **Misspelled view names in DROP/DESCRIBE**: "Semantic view 'saels_view' not found. Did you mean 'sales_view'?"

The last case is already implemented for `semantic_view()` queries (`QueryError::ViewNotFound`). The same pattern applies to DDL.

---

## Feature Dependencies

```
Parser hook detection (extended) --> All native DDL statements
  |
  +-> DROP SEMANTIC VIEW          --> drop_semantic_view()        [exists]
  +-> DROP SEMANTIC VIEW IF EXISTS --> drop_semantic_view_if_exists() [exists]
  +-> CREATE OR REPLACE SEMANTIC VIEW --> create_or_replace_semantic_view() [exists]
  +-> CREATE SEMANTIC VIEW IF NOT EXISTS --> create_semantic_view_if_not_exists() [exists]
  +-> DESCRIBE SEMANTIC VIEW      --> describe_semantic_view()    [exists]
  +-> SHOW SEMANTIC VIEWS         --> list_semantic_views()       [exists]

Error location reporting (depends on DDL verb detection)
  +-> Clause-level hints          --> parse_args.rs error messages
  +-> "Did you mean" for clauses  --> suggest_closest() [exists] + fixed keyword vocabulary
  +-> "Did you mean" for views    --> catalog access + suggest_closest() [exists]

README documentation              --> Depends on all DDL verbs + error reporting being complete
```

**Key observation:** All function-based backends already exist and are registered. The work for native DDL is entirely in the parser detection/rewrite layer (`parse.rs` + `shim.cpp`). No new VTab implementations are needed for v0.5.1.

---

## MVP Recommendation

### Wave 1: Extended DDL Detection and Routing (Highest Priority)

1. **Extend `parse.rs` detection** to recognize all 7 statement types
2. **Add rewrite functions** for DROP, DESCRIBE, SHOW (simpler than CREATE -- no body parsing)
3. **Extend FFI interface** (`sv_parse_rust` + `sv_execute_ddl_rust`) to pass statement type
4. **Update C++ shim** (`sv_plan_function`) to route based on statement type
5. **sqllogictest** for each new statement type

Estimated: ~170 lines Rust + ~30 lines C++ + ~100 lines test

### Wave 2: Error Reporting (Medium Priority)

6. **Clause-level error hints** in `parse_args.rs`
7. **"Did you mean" at DDL time** for keyword arguments and STRUCT field names

Estimated: ~60 lines Rust

### Wave 3: Documentation (Depends on Waves 1-2)

8. **README update** with DDL syntax reference and worked examples

Estimated: ~100 lines markdown

**Defer from v0.5.1:** Row-per-field DESCRIBE format (nice but adds a new VTab), `DESC` alias, `SHOW ... LIKE` filtering, schema-qualified names.

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC | Risk |
|---------|------------|----------|------|
| Extended parser detection (7 types) | Low | ~80 (parse.rs) | Low -- prefix matching, well-tested pattern |
| Rewrite routing for DROP/DESCRIBE/SHOW | Low | ~60 (parse.rs) + ~30 (shim.cpp) | Low -- simpler than CREATE (no body parsing) |
| CREATE OR REPLACE / IF NOT EXISTS detection | Low | ~30 (parse.rs) | Low -- variant of existing CREATE detection |
| FFI interface changes | Low | ~30 (parse.rs) | Low -- extend existing pattern |
| Clause-level error hints | Medium | ~40 (parse_args.rs) | Low -- string formatting changes |
| "Did you mean" at DDL time | Low | ~20 (parse_args.rs) | Low -- reuses suggest_closest() |
| README documentation | Low | ~100 (markdown) | None |
| sqllogictest for new DDL verbs | Low | ~100 (test file) | None |
| **Total** | **Low-Medium** | **~490 lines** | **Low** |

---

## Sources

### DuckDB Official Documentation (HIGH confidence)

- [DuckDB CREATE VIEW Statement](https://duckdb.org/docs/stable/sql/statements/create_view) -- CREATE OR REPLACE semantics
- [DuckDB DROP Statement](https://duckdb.org/docs/stable/sql/statements/drop) -- DROP IF EXISTS, CASCADE, RESTRICT
- [DuckDB DESCRIBE Statement](https://duckdb.org/docs/stable/sql/statements/describe) -- DESCRIBE syntax, SHOW alias
- [DuckDB DESCRIBE Guide](https://duckdb.org/docs/stable/guides/meta/describe) -- output columns (column_name, column_type, null, key, default, extra)
- [DuckDB SHOW and SHOW DATABASE](https://duckdb.org/docs/stable/sql/statements/show) -- SHOW DATABASES variant
- [DuckDB List Tables Guide](https://duckdb.org/docs/stable/guides/meta/list_tables) -- SHOW TABLES output format

### Snowflake Semantic View Documentation (HIGH confidence)

- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- OR REPLACE + IF NOT EXISTS combined syntax
- [Snowflake DROP SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/drop-semantic-view) -- IF EXISTS modifier
- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- row-per-entity output format
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- LIKE, IN, LIMIT, TERSE options

### DuckDB Parser Extension Mechanism (HIGH confidence)

- [Runtime-Extensible SQL Parsers (DuckDB blog)](https://duckdb.org/2024/11/22/runtime-extensible-parsers) -- fallback fires on parse failure
- [DuckDB parser.cpp source](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- parser extension invocation path
- [DuckDB issue #18485](https://github.com/duckdb/duckdb/issues/18485) -- semicolon inconsistency (already handled in parse.rs)

### Extension Reference (MEDIUM confidence)

- [DuckPGQ extension](https://github.com/cwida/duckpgq-extension) -- existence proof for multi-DDL-type parser extensions (CREATE/DROP PROPERTY GRAPH)
- [parser_tools extension](https://duckdb.org/community_extensions/extensions/parser_tools) -- parser introspection community extension

### Project Source Code (HIGH confidence -- direct analysis)

- `src/parse.rs` -- current detection and rewrite (CREATE only)
- `cpp/src/shim.cpp` -- C++ parser hook registration, plan_function routing
- `src/ddl/define.rs` -- DefineState with `or_replace` and `if_not_exists` flags
- `src/ddl/drop.rs` -- DropState with `if_exists` flag
- `src/ddl/describe.rs` -- DescribeSemanticViewVTab (6-column output)
- `src/ddl/list.rs` -- ListSemanticViewsVTab (2-column output)
- `src/lib.rs` -- all 8 functions registered at init time
- `src/expand.rs` -- `suggest_closest()` fuzzy matching with strsim
- `src/query/error.rs` -- `QueryError::ViewNotFound` with suggestion pattern
