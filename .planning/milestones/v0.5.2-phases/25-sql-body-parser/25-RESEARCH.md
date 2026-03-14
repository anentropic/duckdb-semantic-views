# Phase 25: SQL Body Parser - Research

**Researched:** 2026-03-11
**Domain:** Rust hand-written parser, DDL rewrite pipeline, C++ shim buffer
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- `AS` is required between the view name and the clause block
- No outer parentheses wrapping the whole body — clauses appear at top level after `AS`
- Fixed clause order required: TABLES → RELATIONSHIPS (optional) → DIMENSIONS → METRICS
- TABLES is required; both DIMENSIONS and METRICS are required (at least one of each)
- Unknown clause keyword → "did you mean X?" error with position (extends existing fuzzy-match system)
- RELATIONSHIPS clause is optional; empty `RELATIONSHIPS ()` is also valid
- Every relationship entry must have a name: `order_to_customer AS o(customer_id) REFERENCES c`
- DIMENSIONS/METRICS entries: `alias.name AS sql_expr` — alias prefix is required
- Expressions after `AS` are treated as opaque SQL strings — no expression parsing
- Entries separated by commas; trailing commas are allowed
- TABLES entries: `alias AS schema.table PRIMARY KEY (col1, col2)` with composite PK support
- Error position = byte offset into original query string (before any trimming) — existing convention
- Fail-fast: stop and report on first error
- 4096-byte C++ shim buffer must be fixed in Phase 25
- Snowflake semantic view DDL is the grammar model

### Claude's Discretion

- Exact recursive descent structure and module organization
- Whether the new keyword body parser lives in `parse.rs` or a new `src/body_parser.rs`
- JSON encoding strategy for passing parsed definition to `create_semantic_view()` function
- How `parse_create_body` is updated/replaced to support `AS` keyword path vs `(` path
- Parser library choice: hand-written (default) vs `winnow`/`nom`/`chumsky` combinator

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DDL-01 | `CREATE SEMANTIC VIEW` accepts SQL keyword body: `TABLES (...)`, `RELATIONSHIPS (...)`, `DIMENSIONS (...)`, `METRICS (...)` | New `parse_keyword_body()` function in `parse.rs`; detects `AS` after view name to route to keyword path |
| DDL-02 | TABLES clause parses `alias AS physical_table PRIMARY KEY (col, ...)` | Token-at-a-time parser within TABLES clause; PK columns fed into Phase 24 `TableRef.pk_columns` |
| DDL-03 | RELATIONSHIPS clause parses `name AS from_alias(fk_cols) REFERENCES to_alias` | Optional clause; name-required form; clause body parser extracts Phase 24 `Join.from_alias`, `Join.fk_columns`, `Join.name` |
| DDL-04 | DIMENSIONS clause parses `alias.dim_name AS sql_expr` | Qualified-name parser; split on first `.`; expression captured verbatim to `Dimension.name`, `Dimension.source_table`, `Dimension.expr` |
| DDL-05 | METRICS clause parses `alias.metric_name AS agg_expr` | Same pattern as DDL-04 for `Metric` struct |
| DDL-07 | All 7 DDL verbs work with new syntax | DROP, DROP IF EXISTS, DESCRIBE, SHOW: unchanged. CREATE/CREATE OR REPLACE/CREATE IF NOT EXISTS: both `(` and `AS` dispatch paths within `validate_and_rewrite` |
</phase_requirements>

## Summary

Phase 25 adds a new SQL keyword body syntax to `CREATE SEMANTIC VIEW`. Currently the only CREATE body form uses parentheses and function-call-style `:=` named parameters (e.g., `tables := [...]`). Phase 25 adds the `AS` keyword form: `CREATE SEMANTIC VIEW name AS TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)`.

The implementation is a pure Rust parsing problem. The existing `parse.rs` already handles DDL detection (`detect_ddl_kind`), validation (`validate_and_rewrite`), and rewriting (`rewrite_ddl`). Phase 25 adds a new dispatch path inside `validate_create_body`: when the text after the view name starts with `AS` (not `(`), it routes to a new `parse_keyword_body()` function that recursively parses each clause. The parsed result is assembled into a `SemanticViewDefinition` and serialized to JSON, which is then embedded in the rewritten SQL call to `create_semantic_view()`.

The 4096-byte C++ stack buffer in `sv_ddl_bind` and `sv_parse_stub` must be upgraded. The recommended approach is to replace the stack-allocated `char sql_buf[4096]` in both functions with a `std::string` (dynamic allocation), which eliminates the size limit entirely. The JSON-encoded rewritten SQL for a large TPC-H view with 30 dims and 20 metrics is approximately 4,700 bytes — already over the limit. For the validation path (`sv_parse_stub`), the sql_buf result is not executed, so it is lower priority but should still be fixed for consistency.

**Primary recommendation:** Hand-written recursive descent parser in a new `src/body_parser.rs` module. Use `std::string` in C++ for the DDL buffer. Encode the parsed definition as JSON embedded in the rewritten function-call SQL.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `strsim` | 0.11 | Levenshtein distance for "did you mean" suggestions | Already in Cargo.toml; existing `suggest_clause_keyword()` uses it |
| `serde_json` | 1.x | Serialize parsed `SemanticViewDefinition` to JSON for embedding in rewritten SQL | Already in Cargo.toml; the catalog persistence path already uses it |
| `serde` | 1.x | Derive Serialize/Deserialize on model structs | Already in Cargo.toml with derive feature |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `proptest` | 1.9 | Property-based tests for parser | Already in dev-dependencies; `parse_proptest.rs` establishes the pattern |

### Alternatives Considered (Parser Library)
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-written parser | `winnow` 0.6 | winnow provides zero-copy byte slice parsing and good error messages, but adds a new Cargo dependency (STATE.md says "Zero new Cargo dependencies"). The grammar is small enough that hand-written is straightforward and already matches project convention. |
| Hand-written parser | `nom` 7.x | Same tradeoff as winnow. nom's error types are less ergonomic for byte-position-anchored errors than the existing `ParseError { message, position }` type. |
| Hand-written parser | `chumsky` 0.9 | Higher compile-time cost, no existing project usage, overkill for this grammar size. |

**Decision:** Hand-written parser. STATE.md explicitly documents "Zero new Cargo dependencies" as a project decision. The grammar is small (4 clauses, 3 entry formats) and the existing error model (`ParseError { message, position }`) maps cleanly to a manual recursive descent approach.

**Installation:** No new dependencies.

## Architecture Patterns

### Recommended Project Structure

```
src/
├── parse.rs           -- DdlKind detection, validate_and_rewrite (add AS dispatch)
├── body_parser.rs     -- NEW: parse_keyword_body(), clause parsers, entry parsers
├── model.rs           -- SemanticViewDefinition (unchanged; Phase 24 adds fields)
├── ddl/
│   ├── define.rs      -- create_semantic_view() VTab (unchanged in Phase 25)
│   └── parse_args.rs  -- (unchanged in Phase 25)
cpp/
└── src/
    └── shim.cpp       -- sv_ddl_bind: char sql_buf[4096] -> std::string
                          sv_parse_stub: char sql_buf[4096] -> std::string
```

### Pattern 1: AS vs ( Dispatch in validate_create_body

**What:** After extracting the view name, detect whether the next non-whitespace token is `AS` (keyword body) or `(` (old function-call body). Route accordingly.
**When to use:** All three CREATE forms (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS).

```rust
// In validate_create_body (parse.rs):
let after_name = after_prefix[name_end..].trim_start();
if after_name.eq_ignore_ascii_case("as") || after_name.starts_with_ascii_case("as ") {
    // Keyword body path: parse AS TABLES (...) ...
    parse_keyword_body(query, trimmed_no_semi, trim_offset, plen, name)
} else if after_name.starts_with('(') {
    // Old path: parse ( tables := [...] )
    validate_old_body(...)
} else {
    Err(ParseError {
        message: "Expected 'AS' or '(' after view name.".to_string(),
        position: Some(trim_offset + plen + name_end),
    })
}
```

### Pattern 2: Keyword Body Parser Structure (body_parser.rs)

**What:** `parse_keyword_body` scans for `TABLES`, `RELATIONSHIPS`, `DIMENSIONS`, `METRICS` clause keywords at the top level (depth 0, outside parens), then parses the parenthesized content of each.
**When to use:** When the `AS` path is detected.

```rust
pub struct KeywordBody {
    pub tables: Vec<TableRef>,
    pub relationships: Vec<Join>,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
}

/// Parse the keyword body (everything after "AS") into structured clauses.
pub fn parse_keyword_body(
    text: &str,       // full text after "AS", trimmed
    base_offset: usize, // byte offset of text[0] in original query
) -> Result<KeywordBody, ParseError> {
    // 1. Scan top-level clause keywords in order
    // 2. For each found clause, call its sub-parser
    // 3. Validate required clauses (TABLES, at least one dim or metric)
}
```

### Pattern 3: Clause Keyword Scanner

**What:** Reuse the existing `scan_clause_keywords` logic but for the `AS` body (where clause delimiters are the keyword names themselves, not `:=` or `(`).

The new scanner walks the text at depth 0, collecting runs of `[a-zA-Z]` followed by whitespace then `(`. It validates that each keyword is a known clause keyword in the correct order (TABLES before RELATIONSHIPS before DIMENSIONS before METRICS).

**Key difference from old scanner:** The old `scan_clause_keywords` detects `:=` or `(` as the delimiter after a keyword. The new scanner for the AS body uses only `(` as the delimiter (keywords appear directly followed by `(...)`).

### Pattern 4: TABLES Clause Entry Parser

**What:** Parse `alias AS schema.table PRIMARY KEY (col1, col2)` entries.

```rust
fn parse_tables_clause(
    body: &str,       // content inside TABLES ( ... )
    base_offset: usize,
) -> Result<Vec<TableRef>, ParseError> {
    // Split on commas at depth 0 (respecting parentheses for PRIMARY KEY (...))
    // For each entry, parse:
    //   token[0] = alias
    //   token[1] = "AS" (required, case-insensitive)
    //   token[2] = physical table name (may be schema.table)
    //   token[3] = "PRIMARY" (required)
    //   token[4] = "KEY" (required)
    //   token[5] = "(" followed by comma-separated column names followed by ")"
}
```

The physical table name `schema.table` contains a dot; the parser must not confuse it with the `alias.dim_name` pattern of DIMENSIONS entries. Approach: the table name is the full token before `PRIMARY KEY`.

### Pattern 5: DIMENSIONS / METRICS Clause Entry Parser

**What:** Parse `alias.name AS sql_expr` entries. Expression is opaque — captured verbatim to end of comma or end of clause.

```rust
fn parse_qualified_entries(
    body: &str,
    base_offset: usize,
) -> Result<Vec<(String, String, String)>, ParseError> {
    // Returns Vec<(source_alias, bare_name, expr)>
    // Split on commas at depth 0 (expressions may contain nested parens: SUM(x + y))
    // For each entry:
    //   split on first "." to get alias + bare_name
    //   require "AS" after bare_name
    //   capture everything after "AS" as expr (trimmed)
}
```

### Pattern 6: Opaque Expression Capture

**What:** Expressions after `AS` in DIMENSIONS/METRICS may contain arbitrary SQL: function calls `SUM(x * (1 - discount))`, CASE expressions, CAST, etc. They must NOT be parsed — only captured.

The boundary of one expression is: a comma at nesting depth 0 within the clause body. Since the clause body is already the content inside `DIMENSIONS (...)`, depth is reset at each `(` and `)` within the expression.

```rust
fn capture_expr_to_comma(text: &str) -> &str {
    // Walk until depth-0 comma or end of string
    // Return the trimmed slice before the comma
}
```

### Pattern 7: C++ Buffer Fix — std::string Replacement

**What:** Replace both `char sql_buf[4096]` stack buffers in `shim.cpp` with heap-allocated `std::string`.

**Why std::string:** C++ `std::string` is dynamically sized; the Rust FFI write-into-buffer interface (`sv_rewrite_ddl_rust`, `sv_validate_ddl_rust`) takes a raw `char*` and `size_t` length. To use `std::string`, call `resize` to pre-allocate a working size, then pass `.data()` and `.size()` to the Rust FFI.

```cpp
// In sv_ddl_bind (BEFORE fix):
char sql_buf[4096];
memset(sql_buf, 0, sizeof(sql_buf));
uint8_t rc = sv_rewrite_ddl_rust(query.c_str(), query.size(),
    sql_buf, sizeof(sql_buf), error_buf, sizeof(error_buf));

// In sv_ddl_bind (AFTER fix):
std::string sql_buf(65536, '\0');  // 64 KB heap allocation
char error_buf[1024];
memset(error_buf, 0, sizeof(error_buf));
uint8_t rc = sv_rewrite_ddl_rust(query.c_str(), query.size(),
    sql_buf.data(), sql_buf.size(),
    error_buf, sizeof(error_buf));
// After success, sql_buf.c_str() is the null-terminated rewritten SQL
```

**Size recommendation:** 65536 bytes (64 KB) for `sv_ddl_bind`. This covers views with hundreds of dimensions/metrics and long SQL expressions. For `sv_parse_stub`, 16384 bytes (16 KB) is sufficient (validation path does not execute and real views rarely exceed this in the validation phase).

**Alternative: pass original DDL text, parse in define.rs.** Instead of serializing the definition to JSON and embedding it in the rewritten SQL, the rewritten SQL could be `SELECT * FROM create_semantic_view_ddl('view_name', 'original_ddl_text')` — a new function variant that receives the raw DDL text and parses it in Rust at execute time (inside the VTab bind). This eliminates the buffer problem entirely. However, it requires a new function variant, changes to `define.rs`, and means the body is parsed twice (once for validation in sv_parse_stub, once for execution in define.rs). The JSON-encoding approach is simpler and more consistent with the existing architecture. **Recommended: JSON-in-SQL with 64KB std::string buffer.**

### Pattern 8: JSON Encoding of Parsed Definition

**What:** After `parse_keyword_body` produces a `SemanticViewDefinition`, serialize it to JSON and embed it in the rewritten SQL.

```rust
// In rewrite_ddl (parse.rs), for the AS keyword body path:
let def = parse_keyword_body(body_text, body_offset)?;
let json = serde_json::to_string(&def)
    .map_err(|e| ParseError { message: e.to_string(), position: None })?;
let safe_json = json.replace('\'', "''");
let safe_name = name.replace('\'', "''");
Ok(format!("SELECT * FROM create_semantic_view_json('{safe_name}', '{safe_json}')"))
```

**However:** This requires a new `create_semantic_view_json` function (or a mode-switch in the existing one). A simpler alternative: translate the parsed definition into the existing `create_semantic_view` named-parameter syntax at rewrite time. But the existing syntax uses DuckDB struct literals (`[{'alias': ..., 'table': ...}]`) which are hard to quote correctly inside SQL strings.

**Recommended approach:** Add a new `create_semantic_view_from_json` VTab function that accepts `(name VARCHAR, json VARCHAR)` and deserializes the JSON into a `SemanticViewDefinition`. This cleanly separates the parse path from the execution path.

**Single-arg JSON function interface:**
```sql
-- Rewritten SQL:
SELECT * FROM create_semantic_view_from_json('view_name', '{"tables":[...],...}')
```

The `create_semantic_view_from_json` function simply calls `SemanticViewDefinition::from_json(name, json)` then runs the same catalog/persist logic as the existing `create_semantic_view`. CREATE OR REPLACE and CREATE IF NOT EXISTS need parallel `_from_json` variants, or a single `create_semantic_view_from_json` that accepts flags.

**Simplified variant:** Pass the `or_replace` and `if_not_exists` flags as additional VARCHAR args or use separate function names (matching the existing pattern): `create_or_replace_semantic_view_from_json`, `create_semantic_view_if_not_exists_from_json`.

### Anti-Patterns to Avoid

- **Parsing expressions (DIMENSIONS AS clause):** Never try to parse the SQL expression; capture it verbatim. The expression may reference DuckDB functions, operators, window functions, or arbitrary identifiers.
- **Using rfind(')') to find clause end:** The existing `parse_create_body` uses `rfind(')')` which assumes no nested parens in the body. The new body has deeply nested parens (`TABLES ( ... PRIMARY KEY (col) )`). Use depth-tracking bracket walks instead.
- **Confusing schema.table with alias.dim_name:** `schema.table` has a dot in the table name (TABLES clause). `alias.dim_name` has a dot in the entry name (DIMENSIONS clause). The parser must not apply qualified-name splitting to TABLES entries.
- **Forgetting trailing comma support:** The spec allows trailing commas in all clause entry lists. The entry splitter must tolerate a trailing empty entry (after the last comma) and discard it.
- **Using scan_clause_keywords for the AS body:** The existing `scan_clause_keywords` is tuned for the old `:=`/`(` body syntax. It will misidentify tokens in the new body. Write a separate top-level clause scanner for the AS path.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON serialization | Manual JSON building | `serde_json::to_string(&def)` | Already a dependency; handles escaping, nesting, Unicode |
| "Did you mean" suggestions | Edit distance table | `strsim::levenshtein` + `suggest_clause_keyword()` | Already exists in `parse.rs`; reuse for clause typos in AS body |
| Bracket balance in expressions | ad-hoc depth counter | Reuse `validate_brackets()` logic inline | The existing function handles single-quoted strings and all three bracket types |

**Key insight:** The expression capture problem (capturing opaque SQL expressions) is solved by depth-0 comma splitting, not by building an expression parser. Everything after `AS` up to the next depth-0 comma is the expression.

## Common Pitfalls

### Pitfall 1: Position Tracking Across the AS Gap

**What goes wrong:** The error position for errors inside clause bodies must be a byte offset into the original query string. After parsing `CREATE SEMANTIC VIEW name AS`, the parser's cursor is several bytes into the original string. If offsets are computed relative to a sub-slice rather than the original query, the caret renders in the wrong position.

**Why it happens:** The body parser receives sub-slices of the original query (e.g., the content inside `TABLES (...)`). If it returns positions as offsets into its sub-slice, those must be added to the sub-slice's start offset in the original query.

**How to avoid:** Thread `base_offset: usize` through every parser function. All returned positions are `base_offset + local_offset`. The invariant: every `ParseError { position: Some(p) }` satisfies `query[p..]` is the byte where the error occurred.

**Warning signs:** Caret renders in the wrong place in DuckDB output. The existing proptest `position_invariant_clause_typo` (which tests for this property) should be extended to cover the AS body.

### Pitfall 2: TABLES Clause Primary Key Parentheses vs Clause Boundary

**What goes wrong:** The top-level clause scanner walks at depth 0 to find where `TABLES (...)` ends and `RELATIONSHIPS (...)` begins. But `PRIMARY KEY (col1, col2)` inside the TABLES body creates nested depth. If the scanner doesn't properly track bracket depth, it may end the TABLES clause at the first `)` of `PRIMARY KEY (...)`.

**Why it happens:** `validate_brackets` and `scan_clause_keywords` in the old code track depth for the full body. The new top-level clause scanner must do the same: track depth as it walks the full body, and only transition between clauses when depth returns to 0.

**How to avoid:** The top-level clause scanner must find the MATCHING closing paren for each clause's opening paren. Use a depth counter: `depth += 1` on `(`, `depth -= 1` on `)`, with string literal awareness. The clause content is from the `(` following the clause keyword to the matching `)`.

**Warning signs:** Parse error "unknown clause keyword" reporting the first entry in RELATIONSHIPS when the view has a composite PRIMARY KEY; or the TABLES clause body being cut short at the PK paren.

### Pitfall 3: Relationship Entry Name-Required Validation

**What goes wrong:** The CONTEXT.md specifies relationship names are required in the keyword body. But the old struct-literal DDL path and the function-based DDL path allow unnamed relationships. The validator must reject unnamed relationships in the keyword body without breaking the old path.

**Why it happens:** `Join.name` is `Option<String>` and `None` is a valid model state for legacy/function-call relationships. Phase 25's keyword body parser should set `Join.name = Some(name)` always — and error if no name is present.

**How to avoid:** The keyword body parser is separate from the function-call argument parser. Only the keyword body parser enforces "name is required". The existing `parse_args.rs` remains unchanged (allows unnamed relationships).

**Warning signs:** A relationship without a name silently succeeds when it should error; or the function-call path starts failing because it no longer accepts unnamed relationships.

### Pitfall 4: Expression Contains Unbalanced SQL Constructs

**What goes wrong:** A user writes `DIMENSIONS (o.discount AS CASE WHEN x < 0.1 THEN 'low' ELSE 'high' END)`. The CASE expression contains keywords like `WHEN`, `THEN`, `ELSE`, `END` — none of which should be treated as clause boundaries. Similarly, string literals inside expressions may contain commas.

**Why it happens:** The expression parser uses depth-0 comma splitting. If it doesn't track string literals (single-quoted), a comma inside `'East, West'` would be treated as an entry separator.

**How to avoid:** The depth-0 comma splitter must inherit `validate_brackets`-style string literal awareness: skip characters inside single-quoted strings (respecting `''` escape). The existing `validate_brackets` code can be adapted directly.

**Warning signs:** Parse error "Expected AS after name" when the expression contains a comma inside a string literal.

### Pitfall 5: Rewritten SQL Size Exceeds C++ Buffer

**What goes wrong:** The C++ `sv_rewrite_ddl_rust` writes the rewritten SQL into `char sql_buf[4096]`. With 30+ dimensions and long expressions, the JSON-encoded rewritten SQL exceeds 4096 bytes. `write_to_buffer` silently truncates the output (it copies only `min(len, sql_out_len - 1)` bytes). DuckDB then executes a truncated SQL string, producing a cryptic parse error.

**Why it happens:** `write_to_buffer` in `parse.rs` is designed to truncate silently (to avoid buffer overflows). There is no error path for "output too large".

**How to avoid:** Fix the C++ buffer size FIRST (task wave 0 or wave 1 of this phase). Use `std::string(65536, '\0')` instead of `char sql_buf[4096]`. Add a check in `write_to_buffer` or in the FFI caller that detects if the output was truncated (the null terminator would be at `sql_out_len - 1` and the string length equals `sql_out_len - 1`).

**Warning signs:** DuckDB reports a syntax error on valid large views; the error references a fragment of the JSON/SQL string.

### Pitfall 6: Schema-Qualified Table Names Containing Dots

**What goes wrong:** The TABLES clause parser sees `o AS main.orders PRIMARY KEY (o_orderkey)`. When it tokenizes `main.orders`, it must treat this as a single table name token, not as two tokens separated by `.`.

**Why it happens:** The DIMENSIONS/METRICS parser splits on the first `.` to extract the alias. If the TABLES parser reuses that splitting logic, it would incorrectly set alias=`main` and bare_name=`orders`.

**How to avoid:** The TABLES clause parser has a different entry structure: `alias AS table_name PRIMARY KEY (...)`. The table name is everything between `AS` and `PRIMARY KEY`. The dot in `schema.table` is inside the table name, not a qualifier separator. Parse TABLES entries by anchoring on the `PRIMARY KEY` keyword, not by dot-splitting.

**Warning signs:** TableRef with `alias="main"` and `table="orders"` when the user wrote `o AS main.orders`.

## Code Examples

Verified patterns from existing source:

### Existing: DdlKind Detection (parse.rs)
```rust
// Source: src/parse.rs detect_ddl_kind
pub fn detect_ddl_kind(query: &str) -> Option<DdlKind> {
    let trimmed = query.trim().trim_end_matches(';').trim();
    let bytes = trimmed.as_bytes();
    if starts_with_ci(bytes, b"create or replace semantic view") {
        Some(DdlKind::CreateOrReplace)
    } else if starts_with_ci(bytes, b"create semantic view if not exists") {
        Some(DdlKind::CreateIfNotExists)
    } else if starts_with_ci(bytes, b"create semantic view") {
        Some(DdlKind::Create)
    }
    // ... etc.
}
```

### Existing: ParseError with Byte Position (parse.rs)
```rust
// Source: src/parse.rs ParseError
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the original query string (0-based, before any trimming).
    pub position: Option<usize>,
}
```

### Existing: validate_brackets (parse.rs) — Reuse Inside Body Parser
```rust
// Source: src/parse.rs validate_brackets
// Can be called on the content of individual clause bodies, with body_offset
// set to the byte position of the opening '(' + 1 in the original query.
fn validate_brackets(body: &str, body_offset: usize) -> Result<(), ParseError>
```

### Existing: suggest_clause_keyword (parse.rs) — Reuse for AS Body Errors
```rust
// Source: src/parse.rs suggest_clause_keyword
// Returns Some(keyword) if word is within Levenshtein distance <= 3 of a known clause
fn suggest_clause_keyword(word: &str) -> Option<&'static str>
```

### Existing: serde_json for Definition Serialization (model.rs)
```rust
// Source: src/ddl/define.rs (bind function)
let json = serde_json::to_string(&parsed.def)
    .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
```

### New: Depth-0 Comma Split (body_parser.rs)
```rust
// Split body at depth-0 commas, respecting parens and single-quoted strings.
// Returns trimmed, non-empty slices (trailing comma produces no extra entry).
fn split_at_depth0_commas<'a>(body: &'a str) -> Vec<(usize, &'a str)> {
    // Returns Vec<(start_offset, slice)> so callers can compute positions
    let mut entries = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut start = 0;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            if in_string && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                i += 2; continue;
            }
            in_string = !in_string;
        } else if !in_string {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    let entry = body[start..i].trim();
                    if !entry.is_empty() { entries.push((start, entry)); }
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let tail = body[start..].trim();
    if !tail.is_empty() { entries.push((start, tail)); }
    entries
}
```

### New: TABLES Entry Parser Pattern
```rust
// Parse "alias AS schema.table PRIMARY KEY (col1, col2)"
fn parse_table_entry(entry: &str, base_offset: usize) -> Result<TableRef, ParseError> {
    // 1. Find first whitespace to split alias from the rest
    // 2. Find "AS" keyword (case-insensitive)
    // 3. Find "PRIMARY KEY" substring (case-insensitive)
    // 4. Table name = text between AS and PRIMARY KEY (trimmed)
    // 5. PK columns = content inside PRIMARY KEY (...), split on comma
    // Returns TableRef { alias, table, pk_columns }
}
```

### New: C++ std::string Buffer Pattern
```cpp
// In sv_ddl_bind (shim.cpp), AFTER fix:
std::string sql_str(65536, '\0');  // 64 KB for execution path
char error_buf[1024];
memset(error_buf, 0, sizeof(error_buf));

uint8_t rc = sv_rewrite_ddl_rust(
    query.c_str(), query.size(),
    sql_str.data(), sql_str.size(),
    error_buf, sizeof(error_buf));

if (rc != 0) {
    throw BinderException("Semantic view DDL failed: %s", error_buf);
}

// Execute rewritten SQL (now null-terminated in sql_str)
if (duckdb_query(sv_ddl_conn, sql_str.c_str(), &result) != DuckDBSuccess) { ... }
```

### New: JSON-in-SQL Rewrite Pattern
```rust
// In rewrite_ddl or a new rewrite_ddl_keyword_body (parse.rs):
let def: SemanticViewDefinition = parse_keyword_body(body_text, body_offset)?;
let json = serde_json::to_string(&def).map_err(|e| ParseError {
    message: format!("Failed to serialize definition: {e}"),
    position: None,
})?;
let safe_name = name.replace('\'', "''");
let safe_json = json.replace('\'', "''");
let fn_name = match kind {
    DdlKind::Create => "create_semantic_view_from_json",
    DdlKind::CreateOrReplace => "create_or_replace_semantic_view_from_json",
    DdlKind::CreateIfNotExists => "create_semantic_view_if_not_exists_from_json",
    _ => unreachable!(),
};
Ok(format!("SELECT * FROM {fn_name}('{safe_name}', '{safe_json}')"))
```

### New: create_semantic_view_from_json VTab (define.rs)
```rust
// Thin wrapper: deserializes JSON then delegates to same persist/catalog logic.
// Parameters: name VARCHAR, json VARCHAR (positional only)
pub struct DefineFromJsonVTab;

impl VTab for DefineFromJsonVTab {
    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind.add_result_column("view_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        let name = bind.get_parameter(0).to_string();
        let json = bind.get_parameter(1).to_string();
        let mut def = SemanticViewDefinition::from_json(&name, &json)
            .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        // ... same DDL-time inference and persist logic as DefineSemanticViewVTab
    }
}
```

## Buffer Size Analysis

From size estimation with real data:

| Scenario | Input Size | JSON Body | Rewritten SQL |
|----------|-----------|-----------|---------------|
| Small (2 tables, 5 dims, 3 metrics) | ~500 B | ~800 B | ~860 B |
| TPC-H (3 tables, 11 dims, 6 metrics) | ~969 B | ~1,740 B | ~1,793 B |
| Large (5 tables, 30 dims, 20 metrics) | ~2,000 B | ~4,645 B | ~4,705 B |
| Very large (10 tables, 60 dims, 40 metrics) | ~4,000 B | ~9,000 B | ~9,060 B |

**Conclusion:** 4096 bytes is insufficient for any view with 25+ dimensions/metrics. 64 KB (`65536`) covers all practical cases with wide margin.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Function-call body only `(tables := [...])` | ADD keyword body `AS TABLES (...) METRICS (...)` | Phase 25 (v0.5.2) | More SQL-idiomatic DDL surface |
| Stack `char sql_buf[4096]` | `std::string(65536, '\0')` heap allocation | Phase 25 (v0.5.2) | Removes buffer overflow risk for large views |
| Body passed verbatim to DuckDB SQL | Body parsed in Rust, serialized to JSON, passed as a single VARCHAR arg | Phase 25 (v0.5.2) | Enables structured validation before execution |

**Deprecated/outdated after Phase 25:**
- Old `:=`/struct-literal body syntax: still works in Phase 25 (both paths coexist). Removed in Phase 27 (CLN-01).
- `char sql_buf[4096]`: removed in Phase 25.

## Open Questions

1. **Should `create_semantic_view_from_json` be a genuinely separate VTab or a mode parameter on the existing one?**
   - What we know: Adding a second VTab function requires registration in `lib.rs` (the extension init). The existing pattern (5 separate function names for 5 DDL verbs) suggests adding 3 new `_from_json` variants is idiomatic for this codebase.
   - What's unclear: Whether the DDL-time type inference (LIMIT 0 SQL) should run from the `_from_json` path. It should — the parsed definition has the same structure, so inference can run identically.
   - Recommendation: Add 3 new VTab registrations (`create_semantic_view_from_json`, `create_or_replace_semantic_view_from_json`, `create_semantic_view_if_not_exists_from_json`). Refactor the shared persist/catalog/inference logic into a shared function callable from both the existing and new VTabs.

2. **How much of `validate_clauses` in parse.rs can be shared with the new AS body validation?**
   - What we know: `validate_clauses` validates the old `(:= body)` syntax — it checks for empty body, balanced brackets, and known clause keywords. The new AS body has different syntax and delimiters.
   - Recommendation: Keep `validate_clauses` unchanged for the old path. Write a separate `validate_keyword_body` in `body_parser.rs`. They share `suggest_clause_keyword` for "did you mean" messages.

3. **Should Phase 25 update the proptest suite to generate AS-body DDL?**
   - What we know: `parse_proptest.rs` currently generates old `( body )` syntax for CREATE forms.
   - Recommendation: Yes — add a new proptest block that generates AS-body DDL with random views and verifies round-trip parsing. This provides high coverage for position invariants in the new path.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (proptest 1.9) + sqllogictest + Python integration |
| Config file | Cargo.toml, justfile |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DDL-01 | AS keyword body accepted and creates view end-to-end | integration (sqllogictest) | `just test-sql` | ❌ Wave 0 |
| DDL-02 | TABLES clause parses alias AS table PRIMARY KEY (...) | unit | `cargo test body_parser::tests -x` | ❌ Wave 0 |
| DDL-03 | RELATIONSHIPS clause parses name AS from(fk_cols) REFERENCES to | unit | `cargo test body_parser::tests -x` | ❌ Wave 0 |
| DDL-04 | DIMENSIONS clause parses alias.name AS sql_expr | unit | `cargo test body_parser::tests -x` | ❌ Wave 0 |
| DDL-05 | METRICS clause parses alias.name AS agg_expr | unit | `cargo test body_parser::tests -x` | ❌ Wave 0 |
| DDL-07 | All 7 DDL verbs work with keyword body | integration (sqllogictest) | `just test-sql` | ❌ Wave 0 |
| DDL-07 | Error position inside clause body points at correct byte | proptest | `cargo test parse_proptest -x` | ❌ Wave 0 (extend) |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/body_parser.rs` — create module with unit tests for each clause parser (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS)
- [ ] `test/sql/phase25_keyword_body.test` — sqllogictest integration: CREATE with keyword body, query, all 7 DDL verbs
- [ ] `tests/parse_proptest.rs` — extend with proptest block for AS-body DDL round-trip and position invariants
- [ ] `cpp/src/shim.cpp` — fix `char sql_buf[4096]` -> `std::string(65536, '\0')` in both `sv_ddl_bind` and `sv_parse_stub` (Wave 0: required before any large-body test can pass)

## Sources

### Primary (HIGH confidence)
- Project source code — `src/parse.rs`, `src/model.rs`, `src/ddl/define.rs`, `src/ddl/parse_args.rs`, `cpp/src/shim.cpp` — read directly; all patterns and integration points verified
- Project source code — `tests/parse_proptest.rs` — test patterns verified; position invariant tests documented
- `.planning/phases/25-sql-body-parser/25-CONTEXT.md` — locked decisions, grammar spec, code context — primary constraint source
- `.planning/phases/24-pk-fk-model/24-RESEARCH.md` — Phase 24 model fields (pk_columns, from_alias, fk_columns, name on Join) that Phase 25 populates

### Secondary (MEDIUM confidence)
- Buffer size calculations — computed from example definitions using Python; representative but not exhaustive
- Snowflake CREATE SEMANTIC VIEW syntax — cited in CONTEXT.md as "grammar model"; Snowflake docs consulted in Phase 24 research

### Tertiary (LOW confidence)
- None. All findings are from project source code or calculations derived from source code.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new dependencies; all existing libraries verified in source
- Architecture: HIGH — patterns derived from existing source code structure; no external dependencies
- Pitfalls: HIGH — all identified from reading existing code and tracing data flow
- Buffer analysis: HIGH — size calculations from actual data; conservative 64 KB recommendation

**Research date:** 2026-03-11
**Valid until:** 2026-04-11 (stable domain; no external dependencies)
