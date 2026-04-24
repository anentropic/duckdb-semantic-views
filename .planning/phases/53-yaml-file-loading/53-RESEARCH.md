# Phase 53: YAML File Loading - Research

**Researched:** 2026-04-18
**Domain:** File I/O via DuckDB abstraction, parser hook FFI protocol, security enforcement
**Confidence:** HIGH

## Summary

Phase 53 adds `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'` syntax, building on Phase 52's inline YAML support. The core challenge is that file reading cannot happen in the Rust parse/rewrite layer (which has no connection handle), but must use DuckDB's `read_text()` function to respect the `enable_external_access` security setting. The solution is a two-layer approach: (1) Rust detects and validates `FROM YAML FILE` syntax and returns a sentinel with the file path, and (2) the C++ `sv_ddl_bind` function intercepts the sentinel, reads the file via `read_text()` on the existing `sv_ddl_conn`, then substitutes the content as inline YAML and re-invokes the Rust rewrite to produce the final `_from_json` function call.

The security requirement (YAML-07) is satisfied automatically: `read_text()` is a DuckDB built-in that respects `enable_external_access`. When the GLOBAL setting is `false`, `read_text()` throws an error, which propagates as a `BinderException` from `sv_ddl_bind`. No custom security checks are needed -- DuckDB enforces the boundary.

The implementation touches two files: `src/parse.rs` (Rust: ~40 lines for FILE detection + path extraction) and `cpp/src/shim.cpp` (C++: ~60 lines for file reading and query reconstruction). All existing YAML parsing, validation, and persistence infrastructure from Phases 51-52 is reused without modification.

**Primary recommendation:** Detect `FROM YAML FILE` in `validate_create_body` as a sibling branch to `FROM YAML $$`, return a sentinel encoding the file path, and add file-read-then-resubmit logic to `sv_ddl_bind` in the C++ shim.

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + proptests + sqllogictest + DuckLake CI)
- **Test coverage:** Every phase must include unit tests, proptests, sqllogictest, and consider fuzz targets
- **Build:** `cargo test` runs without the extension feature (in-memory DuckDB)
- **SQL logic tests:** Require `just build` first; cover integration paths Rust tests miss
- **Linting:** clippy pedantic + fmt + cargo-deny before pushing to main
- **Fuzz targets:** Compilation checked via `just check-fuzz` (nightly)
- **Snowflake reference:** When in doubt about SQL syntax or behaviour, refer to what Snowflake semantic views does

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| YAML-02 | User can create a semantic view from a YAML file using `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'` | `validate_create_body()` gains a `FROM YAML FILE` branch that extracts the single-quoted file path. The C++ `sv_ddl_bind` reads the file via `read_text()` on `sv_ddl_conn`, substitutes content as inline YAML, and re-invokes the Rust rewrite. The final path converges to the existing `_from_json` function call -- identical validation, persistence, and query behavior. |
| YAML-07 | YAML FILE loading respects DuckDB's `enable_external_access` security setting | `read_text()` is a DuckDB built-in that inherits `enable_external_access` enforcement. When the GLOBAL setting is `false`, `read_text()` throws an error which propagates as a `BinderException`. No custom security checks needed -- DuckDB enforces the boundary automatically. Integration test verifies: `SET enable_external_access = false; CREATE SEMANTIC VIEW ... FROM YAML FILE '...'` produces error. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| yaml_serde | 0.10.4 | YAML deserialization (already in Cargo.toml from Phase 51) | Phase 51 dependency; `from_yaml_with_size_cap()` already implemented [VERIFIED: Cargo.toml, src/model.rs] |
| serde_json | 1 (existing) | JSON serialization for rewrite output | Already used by `rewrite_ddl_keyword_body()` [VERIFIED: src/parse.rs line 1117] |

### Supporting
No new dependencies required for Phase 53. File reading uses DuckDB's built-in `read_text()` table function via the existing `sv_ddl_conn` connection. All YAML parsing infrastructure exists from Phase 51, and all DDL integration infrastructure exists from Phase 52.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `read_text()` via `sv_ddl_conn` (C++ shim) | `std::fs::read_to_string()` in Rust | Bypasses DuckDB's FileSystem abstraction -- no `enable_external_access` enforcement, no S3/GCS support, no cloud storage. Registry may reject extension. [CITED: .planning/research/PITFALLS.md line 141] |
| C++ shim two-step (sentinel) | New `_from_yaml` table functions + subquery | DuckDB table functions cannot accept subquery parameters ("Table function cannot contain subqueries" BinderException). Would require `SELECT * FROM create_semantic_view_from_yaml('name', (SELECT content FROM read_text('path')))` which is invalid. [VERIFIED: DuckDB docs + GitHub discussion #12985] |
| C++ shim two-step (sentinel) | New Rust FFI function with `duckdb_connection` param | Would work but adds a connection parameter to the parse module, breaking the pure-function design of `parse.rs`. The C++ shim already has `sv_ddl_conn` available -- reading there is architecturally cleaner. |

## Architecture Patterns

### Recommended Project Structure
```
src/
  parse.rs       # Modified: FROM YAML FILE detection, path extraction, sentinel return
  model.rs       # Unchanged (from_yaml_with_size_cap exists from Phase 51)
cpp/src/
  shim.cpp       # Modified: file reading in sv_ddl_bind, query reconstruction
test/sql/
  phase53_yaml_file.test  # New: sqllogictest integration tests for FROM YAML FILE
```

### Pattern 1: Two-Phase File Loading (C++ reads, Rust rewrites)
**What:** The C++ `sv_ddl_bind` detects a file-loading sentinel from `sv_rewrite_ddl_rust`, reads the file via `read_text()`, reconstructs the query as inline YAML, and re-invokes `sv_rewrite_ddl_rust`.
**When to use:** Every `FROM YAML FILE` statement.
**Why:** The Rust rewrite layer (`parse.rs`) is intentionally a pure function with no connection handle. File I/O must go through DuckDB's SQL engine (for `enable_external_access` enforcement). The C++ shim already has `sv_ddl_conn` for DDL execution -- adding file reading there follows the existing architecture.

**Flow diagram:**
```
User: CREATE SEMANTIC VIEW v FROM YAML FILE '/path/to/def.yaml'
  |
  v
[sv_parse_stub] --> sv_validate_ddl_rust --> validate_and_rewrite
  |  Detects FROM YAML FILE, extracts path, returns success (rc=0)
  |  sql_out = sentinel string (not executed in parse phase)
  v
[sv_ddl_bind] --> sv_rewrite_ddl_rust --> validate_and_rewrite
  |  Returns sentinel: "__SV_YAML_FILE__<path>\x00<kind>\x00<name>\x00<comment>"
  |
  |  C++ detects __SV_YAML_FILE__ prefix:
  |  1. Parse sentinel to extract path, kind, name, comment
  |  2. Execute: SELECT content FROM read_text('<path>') on sv_ddl_conn
  |     - If enable_external_access=false: read_text throws -> BinderException
  |     - If file not found: read_text throws -> BinderException with clear message
  |  3. Reconstruct query: CREATE [kind] SEMANTIC VIEW <name> [COMMENT] FROM YAML $$<content>$$
  |  4. Call sv_rewrite_ddl_rust again with reconstructed query
  |     -> Returns: SELECT * FROM create_semantic_view_from_json('name', 'json')
  |  5. Execute final SQL on sv_ddl_conn
  v
View created and queryable
```

[VERIFIED: cpp/src/shim.cpp lines 134-201 for sv_ddl_bind structure]
[VERIFIED: src/parse.rs lines 1055-1065 for FROM YAML detection pattern]

### Pattern 2: FROM YAML FILE Detection in validate_create_body
**What:** After detecting `FROM YAML` (9 chars, case-insensitive), check if the next token is `FILE` (4 chars, case-insensitive, whitespace-delimited). If so, route to the FILE path instead of the dollar-quote path.
**When to use:** In `validate_create_body()`, as a sub-branch within the `is_yaml_body` detection.
**Example:**
```rust
// Source: pattern from validate_create_body() in src/parse.rs lines 1055-1065
// MODIFIED: Split FROM YAML into two sub-branches

// --- FROM YAML body path (Phase 52 + Phase 53) ---
let is_yaml_body = after_name_trimmed
    .get(..9)
    .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
    && (after_name_trimmed.len() == 9
        || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());
if is_yaml_body {
    let yaml_text = after_name_trimmed[9..].trim_start();

    // Phase 53: FROM YAML FILE '/path' sub-branch
    let is_file_body = yaml_text
        .get(..4)
        .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
        && (yaml_text.len() == 4
            || yaml_text.as_bytes()[4].is_ascii_whitespace());
    if is_file_body {
        let file_text = yaml_text[4..].trim_start();
        return rewrite_ddl_yaml_file_body(kind, name, file_text, view_comment);
    }

    // Phase 52: FROM YAML $$ ... $$ (inline) sub-branch
    return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment);
}
// --- End FROM YAML body path ---
```
[VERIFIED: src/parse.rs lines 1055-1065 for existing FROM YAML detection]

### Pattern 3: File Path Extraction from Single-Quoted String
**What:** Extract a file path from a single-quoted SQL string literal. Handle escaped single quotes (`''`).
**When to use:** When `FROM YAML FILE '...'` is detected.
**Example:**
```rust
// Source: derived from SQL string literal conventions
/// Extract a single-quoted string literal from the input.
/// Returns (unescaped_content, bytes_consumed) on success.
/// Handles SQL-standard escaped single quotes ('').
fn extract_single_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('\'') {
        return Err(ParseError {
            message: "Expected single-quoted file path after FILE keyword. \
                      Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            position: None,
        });
    }
    let mut result = String::new();
    let mut i = 1; // skip opening quote
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                result.push('\'');
                i += 2; // skip escaped ''
            } else {
                return Ok((result, i + 1)); // closing quote
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Err(ParseError {
        message: "Unterminated file path string (missing closing single quote)".to_string(),
        position: None,
    })
}
```

### Pattern 4: Sentinel Protocol for File Loading
**What:** The Rust rewrite function returns a well-defined sentinel string that the C++ shim intercepts. The sentinel encodes all information needed to read the file and reconstruct the query.
**When to use:** When `FROM YAML FILE` is detected in the rewrite phase.
**Format:** `__SV_YAML_FILE__` prefix followed by NUL-separated fields:
```
__SV_YAML_FILE__<file_path>\x00<kind_number>\x00<view_name>\x00<comment_or_empty>
```
Where `kind_number` is: 0=Create, 1=CreateOrReplace, 2=CreateIfNotExists.

**Example Rust implementation:**
```rust
fn rewrite_ddl_yaml_file_body(
    kind: DdlKind,
    name: &str,
    file_text: &str,  // text after "FILE", starting at the single-quoted path
    view_comment: Option<String>,
) -> Result<Option<String>, ParseError> {
    let (file_path, consumed) = extract_single_quoted(file_text)?;

    // Check for trailing content after the file path
    let trailing = file_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after file path: '{trailing}'"),
            position: None,
        });
    }

    if file_path.is_empty() {
        return Err(ParseError {
            message: "File path cannot be empty".to_string(),
            position: None,
        });
    }

    // Encode as sentinel for C++ shim to intercept
    let kind_num = match kind {
        DdlKind::Create => 0,
        DdlKind::CreateOrReplace => 1,
        DdlKind::CreateIfNotExists => 2,
        _ => unreachable!("rewrite_ddl_yaml_file_body only called for CREATE forms"),
    };
    let comment_str = view_comment.as_deref().unwrap_or("");
    Ok(Some(format!(
        "__SV_YAML_FILE__{file_path}\x00{kind_num}\x00{name}\x00{comment_str}"
    )))
}
```

**Example C++ shim handling:**
```cpp
// In sv_ddl_bind, after calling sv_rewrite_ddl_rust:
string sql(sql_str.c_str());

if (sql.rfind("__SV_YAML_FILE__", 0) == 0) {
    // Parse sentinel fields (NUL-separated after prefix)
    auto payload = sql.substr(16); // skip "__SV_YAML_FILE__"
    auto parts = split_nul(payload); // split on \x00
    // parts[0]=file_path, parts[1]=kind_num, parts[2]=view_name, parts[3]=comment

    // Step 1: Read file via read_text() on sv_ddl_conn
    string read_sql = "SELECT content FROM read_text('" +
                       escape_sql(parts[0]) + "')";
    duckdb_result file_result;
    if (duckdb_query(sv_ddl_conn, read_sql.c_str(), &file_result) != DuckDBSuccess) {
        auto err_ptr = duckdb_result_error(&file_result);
        string err_msg = err_ptr ? string(err_ptr) : "File read failed";
        duckdb_destroy_result(&file_result);
        throw BinderException("FROM YAML FILE failed: %s", err_msg);
    }
    // Extract content
    char *content_ptr = duckdb_value_varchar(&file_result, 1, 0); // column 1 = content
    string yaml_content = content_ptr ? string(content_ptr) : "";
    if (content_ptr) duckdb_free(content_ptr);
    duckdb_destroy_result(&file_result);

    // Step 2: Reconstruct query as inline YAML
    string kind_prefix;
    if (parts[1] == "0") kind_prefix = "CREATE";
    else if (parts[1] == "1") kind_prefix = "CREATE OR REPLACE";
    else kind_prefix = "CREATE SEMANTIC VIEW IF NOT EXISTS"; // needs special handling

    string reconstructed;
    if (parts[1] == "2") {
        reconstructed = "CREATE SEMANTIC VIEW IF NOT EXISTS " + parts[2];
    } else {
        reconstructed = kind_prefix + " SEMANTIC VIEW " + parts[2];
    }
    if (!parts[3].empty()) {
        reconstructed += " COMMENT = '" + escape_sql(parts[3]) + "'";
    }
    reconstructed += " FROM YAML $$" + yaml_content + "$$";

    // Step 3: Re-invoke Rust rewrite with inline YAML query
    memset(sql_str.data(), 0, sql_str.size());
    memset(error_buf, 0, sizeof(error_buf));
    rc = sv_rewrite_ddl_rust(
        reconstructed.c_str(), reconstructed.size(),
        sql_str.data(), sql_str.size(),
        error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException("Semantic view DDL failed: %s", error_buf);
    }
    sql = string(sql_str.c_str());
}

// Execute the final rewritten SQL on sv_ddl_conn (existing code)
duckdb_result result;
if (duckdb_query(sv_ddl_conn, sql.c_str(), &result) != DuckDBSuccess) {
    // ... existing error handling ...
}
```

### Pattern 5: read_text Column Index
**What:** `read_text()` returns a table with columns: `filename` (0), `content` (1), `size` (2), `last_modified` (3). The YAML content is in column index 1.
**When to use:** When extracting file content from `read_text()` result.
[CITED: duckdb.org/docs/current/guides/file_formats/read_file]

### Anti-Patterns to Avoid
- **Reading files via `std::fs` in Rust:** Do NOT use `std::fs::read_to_string()`. This bypasses DuckDB's FileSystem abstraction, circumvents `enable_external_access`, and provides no cloud storage support. The community extension registry may reject extensions that bypass security controls. [CITED: .planning/research/PITFALLS.md lines 141-156]
- **Reading files at parse time:** Do NOT attempt file I/O in `sv_parse_stub` or `sv_validate_ddl_rust`. The parser hook context has no access to the execution engine, connection state, or file system. File reads must happen at bind time. [CITED: .planning/research/ARCHITECTURE.md lines 501-503]
- **Subquery parameters to table functions:** Do NOT pass `(SELECT content FROM read_text('...'))` as a parameter to a table function like `create_semantic_view_from_json()`. DuckDB throws "Table function cannot contain subqueries". [VERIFIED: DuckDB GitHub discussion #12985]
- **New DdlKind variants:** Do NOT add `CreateFromYamlFile` etc. to the DdlKind enum. The FILE vs inline format is a sub-dispatch within the `FROM YAML` branch, not a separate statement kind. [CITED: .planning/research/ARCHITECTURE.md line 118]
- **Custom `enable_external_access` checks:** Do NOT manually query `PRAGMA enable_external_access` to check the setting. DuckDB's `read_text()` already enforces this automatically. Custom checks are redundant and may not cover all edge cases (e.g., `allowed_paths`). [ASSUMED]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| File reading | Custom file I/O in Rust | DuckDB's `read_text()` via `sv_ddl_conn` | Respects `enable_external_access`, supports cloud storage (S3/GCS/Azure), handles encoding validation |
| Security enforcement | Manual `enable_external_access` check | DuckDB's built-in enforcement on `read_text()` | Automatic, covers all edge cases, future-proof for new security settings |
| YAML parsing | Separate file-YAML parser | `from_yaml_with_size_cap()` (Phase 51) | Already tested with 11 unit tests + 256-case proptest [VERIFIED: src/model.rs] |
| DDL validation/persistence | File-specific validation path | Existing `_from_json` pipeline (Phase 52) | Same validation, same persistence, same query behavior |
| Dollar-quote extraction | File-specific content wrapping | `extract_dollar_quoted()` (Phase 52) | C++ reconstructs query with `$$content$$`, Rust rewrite handles it identically to inline YAML |

**Key insight:** Phase 53 adds approximately 40 lines of Rust (path detection + extraction + sentinel) and 60 lines of C++ (file reading + query reconstruction). Everything else -- YAML parsing, validation, cardinality inference, persistence, querying -- is reused unchanged from Phases 51-52.

## Common Pitfalls

### Pitfall 1: `enable_external_access` is GLOBAL, not per-connection
**What goes wrong:** The `sv_ddl_conn` is a separate connection created at extension init time. If `enable_external_access` were per-connection, setting it on the user's connection would not affect `sv_ddl_conn`, and file loading would bypass the security setting.
**Why it doesn't happen:** `enable_external_access` has GLOBAL scope (per-database). Setting it on any connection affects the entire database instance, including `sv_ddl_conn`. File reading via `read_text()` on `sv_ddl_conn` is correctly blocked when the setting is `false`.
**How to verify:** Integration test: `SET enable_external_access = false;` then `CREATE SEMANTIC VIEW v FROM YAML FILE '/path'` must fail with a security error.
[CITED: duckdb.org/docs/lts/configuration/overview -- enable_external_access scope is GLOBAL]

### Pitfall 2: read_text column index for content
**What goes wrong:** `read_text()` returns 4 columns: `filename` (0), `content` (1), `size` (2), `last_modified` (3). If the C++ code reads column 0 (filename) instead of column 1 (content), the file path is used as YAML content, causing a confusing parse error.
**Why it happens:** Column indices are easy to get wrong; the names are not visible in the C API.
**How to avoid:** Use column index 1 for content. Add a comment in the code documenting the schema. Consider using `SELECT content FROM read_text('...')` so only one column is returned, making `duckdb_value_varchar(&result, 0, 0)` safe.
**Warning signs:** YAML parse errors mentioning the file path string as YAML content.
[CITED: duckdb.org/docs/current/guides/file_formats/read_file]

### Pitfall 3: Dollar-quote collision in file content
**What goes wrong:** When the C++ shim reconstructs the query as `FROM YAML $$<content>$$`, if the YAML file content contains the literal string `$$`, the dollar-quote extractor in Rust will terminate early, truncating the YAML.
**Why it happens:** YAML files can contain arbitrary strings, including SQL expressions with `$$`.
**How to avoid:** Use tagged dollar-quoting. The C++ shim should use a unique tag that is unlikely to appear in YAML content. Recommended: `$__sv_file$...$__sv_file$` (double-underscore prefix convention). The Phase 52 `extract_dollar_quoted()` already supports tagged delimiters.
**Warning signs:** Parse errors on YAML files that work fine when loaded inline with the same content.
[VERIFIED: src/parse.rs extract_dollar_quoted supports tagged delimiters]

### Pitfall 4: File path SQL injection
**What goes wrong:** The file path extracted from the DDL is embedded in a SQL query (`SELECT content FROM read_text('...')`). If the path contains single quotes (e.g., `/path/to/file's.yaml`), it could break or inject SQL.
**Why it happens:** User-supplied file paths are strings that can contain any characters.
**How to avoid:** SQL-escape the file path before embedding in the `read_text()` query. Replace `'` with `''` (standard SQL escaping). The Rust `extract_single_quoted()` function already handles `''` unescaping for the DDL syntax; the C++ side must re-escape for the `read_text()` query.
**Warning signs:** SQL syntax errors when using file paths with apostrophes.

### Pitfall 5: Sentinel prefix in user YAML content
**What goes wrong:** If the Rust rewrite of an inline YAML DDL somehow produces output starting with `__SV_YAML_FILE__`, the C++ shim would incorrectly treat it as a file-loading sentinel.
**Why it doesn't happen:** The inline YAML rewrite path always produces `SELECT * FROM create_semantic_view_from_json(...)`, which starts with `SELECT`. The sentinel prefix `__SV_YAML_FILE__` is not a valid SQL start. The only code path that produces this prefix is `rewrite_ddl_yaml_file_body()`.
**How to avoid:** Use a distinct, unlikely prefix. `__SV_YAML_FILE__` with double underscores is safe. Alternatively, add an assertion in C++ that the sentinel payload has the expected structure.

### Pitfall 6: 64KB buffer overflow with large YAML files
**What goes wrong:** The C++ shim allocates a 64KB buffer for the rewritten SQL (`sv_ddl_bind` line 142). When the YAML file content is large and gets embedded as inline YAML in the reconstructed query, the final rewritten SQL (which includes the full JSON-serialized definition) could exceed 64KB.
**Why it happens:** The 1MB YAML size cap means large YAML files are possible. The JSON serialization of a large definition could easily exceed 64KB.
**How to avoid:** For Phase 53, the C++ shim should allocate a larger buffer for the reconstructed-query rewrite call. The sentinel-path buffer can be dynamically sized based on the file content length. Recommended: `max(65536, content.size() * 2 + 4096)` bytes for the rewrite buffer.
**Warning signs:** Silent truncation of the rewritten SQL, causing invalid function call syntax and cryptic "DDL execution failed" errors.
[VERIFIED: cpp/src/shim.cpp line 142 shows 64KB static buffer]

### Pitfall 7: Error message quality for file-not-found
**What goes wrong:** DuckDB's `read_text()` error message for a missing file may be generic or mention internal DuckDB details rather than the user's `FROM YAML FILE` context.
**Why it happens:** The error propagates from `read_text()` through `duckdb_query` to the C++ shim. The raw error message may not mention `FROM YAML FILE`.
**How to avoid:** In the C++ shim, wrap the `read_text()` error with context: `"FROM YAML FILE failed: <duckdb_error>"`. This way users see that the error is from the file loading step, not from YAML parsing or DDL processing.

## Code Examples

### FROM YAML FILE Detection in validate_create_body
```rust
// Source: modified from src/parse.rs lines 1055-1065
// Phase 53: Add FILE sub-branch within FROM YAML detection

// --- FROM YAML body path (Phase 52 + Phase 53) ---
let is_yaml_body = after_name_trimmed
    .get(..9)
    .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
    && (after_name_trimmed.len() == 9
        || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());
if is_yaml_body {
    let yaml_text = after_name_trimmed[9..].trim_start();

    // Phase 53: FROM YAML FILE '/path' sub-branch
    let is_file = yaml_text
        .get(..4)
        .is_some_and(|s| s.eq_ignore_ascii_case("FILE"))
        && (yaml_text.len() == 4
            || yaml_text.as_bytes()[4].is_ascii_whitespace());
    if is_file {
        let file_text = yaml_text[4..].trim_start();
        return rewrite_ddl_yaml_file_body(kind, name, file_text, view_comment);
    }

    // Phase 52: FROM YAML $$...$$ inline sub-branch (existing)
    return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment);
}
// --- End FROM YAML body path ---
```
[VERIFIED: src/parse.rs lines 1055-1065 for existing FROM YAML detection structure]

### File Path Extraction
```rust
// Source: derived from SQL string literal conventions
/// Extract a single-quoted string from `input`.
///
/// Returns `(unescaped_content, bytes_consumed)`. Handles SQL-standard
/// escaped single quotes (`''` -> `'`).
fn extract_single_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('\'') {
        return Err(ParseError {
            message: "Expected single-quoted file path after FILE keyword. \
                      Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            position: None,
        });
    }
    let mut result = String::new();
    let mut i = 1; // skip opening quote
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                result.push('\'');
                i += 2;
            } else {
                return Ok((result, i + 1));
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Err(ParseError {
        message: "Unterminated file path string (missing closing single quote)".to_string(),
        position: None,
    })
}
```

### Sentinel Generation (rewrite_ddl_yaml_file_body)
```rust
// Source: new function for Phase 53
/// Generate a sentinel string for C++ shim to intercept and read the file.
///
/// Sentinel format: __SV_YAML_FILE__<path>\x00<kind>\x00<name>\x00<comment>
/// The C++ shim reads the file via read_text(), reconstructs as inline YAML,
/// and re-invokes sv_rewrite_ddl_rust.
fn rewrite_ddl_yaml_file_body(
    kind: DdlKind,
    name: &str,
    file_text: &str,
    view_comment: Option<String>,
) -> Result<Option<String>, ParseError> {
    let (file_path, consumed) = extract_single_quoted(file_text)?;

    let trailing = file_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after file path: '{trailing}'"),
            position: None,
        });
    }

    if file_path.is_empty() {
        return Err(ParseError {
            message: "File path cannot be empty. \
                      Use: FROM YAML FILE '/path/to/file.yaml'"
                .to_string(),
            position: None,
        });
    }

    let kind_num = match kind {
        DdlKind::Create => 0,
        DdlKind::CreateOrReplace => 1,
        DdlKind::CreateIfNotExists => 2,
        _ => unreachable!("rewrite_ddl_yaml_file_body only called for CREATE forms"),
    };
    let comment = view_comment.as_deref().unwrap_or("");
    Ok(Some(format!(
        "__SV_YAML_FILE__{file_path}\x00{kind_num}\x00{name}\x00{comment}"
    )))
}
```

### C++ File Reading in sv_ddl_bind
```cpp
// Source: modification to cpp/src/shim.cpp sv_ddl_bind function
// Insert after sv_rewrite_ddl_rust call, before existing duckdb_query execution

string sql(sql_str.c_str());

// Phase 53: Intercept YAML FILE sentinel and read file before final rewrite
if (sql.rfind("__SV_YAML_FILE__", 0) == 0) {
    // Parse sentinel: __SV_YAML_FILE__<path>\0<kind>\0<name>\0<comment>
    auto payload = sql.substr(16);
    vector<string> parts;
    size_t pos = 0;
    for (int i = 0; i < 3; i++) {
        auto nul = payload.find('\0', pos);
        if (nul == string::npos) {
            parts.push_back(payload.substr(pos));
            break;
        }
        parts.push_back(payload.substr(pos, nul - pos));
        pos = nul + 1;
    }
    if (pos < payload.size()) {
        parts.push_back(payload.substr(pos));
    }

    if (parts.size() < 3) {
        throw BinderException("Internal error: malformed YAML FILE sentinel");
    }

    auto &file_path = parts[0];
    auto &kind_str = parts[1];
    auto &view_name = parts[2];
    auto comment = parts.size() > 3 ? parts[3] : string();

    // Step 1: Read file via read_text()
    // SQL-escape the file path for the query
    string escaped_path;
    for (char c : file_path) {
        escaped_path += c;
        if (c == '\'') escaped_path += '\'';
    }
    string read_sql = "SELECT content FROM read_text('" + escaped_path + "')";

    duckdb_result file_result;
    if (duckdb_query(sv_ddl_conn, read_sql.c_str(), &file_result) != DuckDBSuccess) {
        auto err_ptr = duckdb_result_error(&file_result);
        string err_msg = err_ptr ? string(err_ptr) : "File read failed";
        duckdb_destroy_result(&file_result);
        throw BinderException("FROM YAML FILE failed: %s", err_msg);
    }

    // Check row count
    auto row_count = duckdb_row_count(&file_result);
    if (row_count == 0) {
        duckdb_destroy_result(&file_result);
        throw BinderException("FROM YAML FILE: no content returned from '%s'",
                              file_path);
    }

    // read_text returns: filename(0), content(1), size(2), last_modified(3)
    // When using SELECT content FROM ..., content is column 0
    char *content_ptr = duckdb_value_varchar(&file_result, 0, 0);
    string yaml_content = content_ptr ? string(content_ptr) : "";
    if (content_ptr) duckdb_free(content_ptr);
    duckdb_destroy_result(&file_result);

    // Step 2: Reconstruct query as inline YAML with tagged dollar-quote
    string kind_prefix;
    if (kind_str == "0") kind_prefix = "CREATE SEMANTIC VIEW ";
    else if (kind_str == "1") kind_prefix = "CREATE OR REPLACE SEMANTIC VIEW ";
    else kind_prefix = "CREATE SEMANTIC VIEW IF NOT EXISTS ";

    string reconstructed = kind_prefix + view_name;
    if (!comment.empty()) {
        // SQL-escape the comment
        string escaped_comment;
        for (char c : comment) {
            escaped_comment += c;
            if (c == '\'') escaped_comment += '\'';
        }
        reconstructed += " COMMENT = '" + escaped_comment + "'";
    }
    // Use tagged dollar-quote to avoid collision with $$ in YAML content
    reconstructed += " FROM YAML $__sv_file$" + yaml_content + "$__sv_file$";

    // Step 3: Re-invoke Rust rewrite with the inline YAML query
    // Allocate buffer large enough for the potentially large content
    size_t rewrite_buf_size = std::max(size_t(65536), yaml_content.size() * 2 + 4096);
    std::string rewrite_sql(rewrite_buf_size, '\0');
    memset(error_buf, 0, sizeof(error_buf));

    rc = sv_rewrite_ddl_rust(
        reconstructed.c_str(), reconstructed.size(),
        rewrite_sql.data(), rewrite_sql.size(),
        error_buf, sizeof(error_buf));

    if (rc != 0) {
        throw BinderException("Semantic view DDL failed: %s", error_buf);
    }
    sql = string(rewrite_sql.c_str());
}

// Execute the final rewritten SQL (existing code follows)
duckdb_result result;
if (duckdb_query(sv_ddl_conn, sql.c_str(), &result) != DuckDBSuccess) {
    // ... existing error handling ...
}
```

### Updated Error Message
```rust
// Source: src/parse.rs line 1070 (existing error message)
// Updated to mention FROM YAML FILE syntax:
Err(ParseError {
    message: "Expected 'AS' or 'FROM YAML' after view name. Use: \
              CREATE SEMANTIC VIEW name AS TABLES (...) DIMENSIONS (...) METRICS (...) or: \
              CREATE SEMANTIC VIEW name FROM YAML $$ ... $$ or: \
              CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'"
        .to_string(),
    position: Some(trim_offset + pos_in_trimmed),
})
```

### SQLLogicTest: Create YAML File and Load It
```
# Phase 53: YAML file loading integration tests
require semantic_views

# Setup: create backing tables
statement ok
CREATE TABLE p53_orders (id INTEGER PRIMARY KEY, amount DOUBLE, region VARCHAR);

statement ok
INSERT INTO p53_orders VALUES (1, 100.0, 'East'), (2, 200.0, 'West');

# Write a YAML definition file to __TEST_DIR__
statement ok
COPY (SELECT 'base_table: p53_orders
tables:
  - alias: o
    table: p53_orders
    pk_columns:
      - id
dimensions:
  - name: region
    expr: o.region
    source_table: o
metrics:
  - name: total_amount
    expr: SUM(o.amount)
    source_table: o' AS content)
TO '__TEST_DIR__/p53_test.yaml' (FORMAT CSV, HEADER FALSE, QUOTE '');

# YAML-02: Load semantic view from YAML file
statement ok
CREATE SEMANTIC VIEW p53_from_file FROM YAML FILE '__TEST_DIR__/p53_test.yaml'

# Verify the view is queryable
query TR rowsort
SELECT region, total_amount FROM semantic_view('p53_from_file', dimensions := ['region'], metrics := ['total_amount'])
----
East	100.0
West	200.0

# YAML-07: Security enforcement -- enable_external_access=false blocks file loading
statement ok
SET enable_external_access = false

statement error
CREATE SEMANTIC VIEW p53_blocked FROM YAML FILE '__TEST_DIR__/p53_test.yaml'
----
FROM YAML FILE failed

# Re-enable external access for cleanup
statement ok
SET enable_external_access = true
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Inline YAML only (`FROM YAML $$...$$`) | Inline YAML + file-based (`FROM YAML FILE '...'`) | Phase 53 (v0.7.0) | Users can define semantic views from external YAML files for version control workflows |
| No file I/O in extension | File reading via DuckDB's `read_text()` | Phase 53 (v0.7.0) | Respects security settings, supports cloud storage paths |

**Deprecated/outdated:**
- Function-based DDL (retired in v0.5.2) -- not relevant to Phase 53
- Direct `std::fs` file reading -- never used, explicitly rejected for security reasons

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `read_text()` returns columns in order: filename(0), content(1), size(2), last_modified(3) | Common Pitfalls / Pitfall 2 | If column order differs, wrong data is extracted. Risk: LOW -- verified via DuckDB docs. Using `SELECT content FROM read_text(...)` mitigates by projecting only the content column. |
| A2 | Custom `enable_external_access` checks are unnecessary because `read_text()` enforces automatically | Anti-Patterns to Avoid | If `read_text()` does NOT enforce the setting, file loading would bypass security. Risk: VERY LOW -- multiple DuckDB documentation sources confirm enforcement. Integration test verifies. |
| A3 | Tagged dollar-quote `$__sv_file$` is safe against collision with YAML content | Common Pitfalls / Pitfall 3 | If a YAML file contains the literal string `$__sv_file$`, the dollar-quote extractor terminates early. Risk: EXTREMELY LOW -- the tag uses double-underscore prefix convention unlikely in user YAML. |
| A4 | `SET enable_external_access = false` can be re-enabled with `SET enable_external_access = true` in the same session | Code Examples / SQLLogicTest | If the setting cannot be re-enabled (like safe mode), the cleanup step of the test would fail and subsequent tests requiring file access would break. Risk: LOW -- DuckDB docs indicate SET is reversible; safe mode (which is not reversible) is a separate mechanism. |

## Open Questions

1. **`enable_external_access` re-enablement in tests**
   - What we know: `SET enable_external_access = false` disables file access. DuckDB safe mode makes this irreversible. But `SET` alone (not safe mode) should be reversible.
   - What's unclear: Whether `SET enable_external_access = true` after `SET enable_external_access = false` actually works in all DuckDB versions. Some global settings have one-way ratchets.
   - Recommendation: Test this in the sqllogictest. If re-enablement fails, split the security test into a separate test file that runs in isolation (like the persistence tests).

2. **Cloud storage paths (S3/GCS)**
   - What we know: `read_text()` supports cloud storage via the httpfs extension. `FROM YAML FILE 's3://bucket/definition.yaml'` would work if httpfs is loaded.
   - What's unclear: Should we test cloud paths or document this as a supported use case?
   - Recommendation: Not for Phase 53. Cloud paths are a natural extension that works because of `read_text()`. Document as a capability but don't test (requires cloud credentials in CI).

3. **Relative path resolution**
   - What we know: DuckDB resolves relative paths relative to the database file directory (for file-backed DBs) or CWD (for in-memory DBs).
   - What's unclear: Whether `read_text()` on `sv_ddl_conn` resolves relative paths the same way as the user's connection.
   - Recommendation: Use absolute paths in tests (via `__TEST_DIR__`). Document that relative paths resolve relative to DuckDB's working directory. No custom path resolution needed -- `read_text()` handles it.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + sqllogictest runner |
| Config file | Cargo.toml + test/sql/*.test |
| Quick run command | `cargo test parse::tests::yaml_file` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| YAML-02 | `FROM YAML FILE '/path'` creates a queryable view | unit + sqllogictest | `cargo test parse::tests::yaml_file` + `just test-sql` | Wave 0 |
| YAML-02 | `CREATE OR REPLACE ... FROM YAML FILE` replaces existing view | unit + sqllogictest | `cargo test parse::tests::yaml_file` + `just test-sql` | Wave 0 |
| YAML-02 | `CREATE IF NOT EXISTS ... FROM YAML FILE` is no-op for existing view | unit + sqllogictest | `cargo test parse::tests::yaml_file` + `just test-sql` | Wave 0 |
| YAML-02 | File path with escaped single quotes works | unit | `cargo test parse::tests::yaml_file` | Wave 0 |
| YAML-02 | Empty file path rejected | unit | `cargo test parse::tests::yaml_file` | Wave 0 |
| YAML-02 | Trailing content after file path rejected | unit | `cargo test parse::tests::yaml_file` | Wave 0 |
| YAML-02 | Missing closing quote on file path rejected | unit | `cargo test parse::tests::yaml_file` | Wave 0 |
| YAML-02 | COMMENT = '...' FROM YAML FILE works | unit + sqllogictest | `cargo test parse::tests::yaml_file` + `just test-sql` | Wave 0 |
| YAML-07 | `SET enable_external_access = false` blocks FROM YAML FILE | sqllogictest | `just test-sql` | Wave 0 |
| YAML-07 | File-not-found error includes contextual message | sqllogictest | `just test-sql` | Wave 0 |
| -- | FROM YAML FILE detection is case-insensitive | unit | `cargo test parse::tests::yaml_file` | Wave 0 |
| -- | FROM YAML (inline) still works after FILE branch added | unit + sqllogictest | `cargo test parse::tests::yaml` + `just test-sql` | Wave 0 |
| -- | Sentinel format is correct (path, kind, name, comment) | unit | `cargo test parse::tests::yaml_file` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase53_yaml_file.test` -- sqllogictest integration tests for FROM YAML FILE
- [ ] `src/parse.rs` unit tests -- `extract_single_quoted`, `rewrite_ddl_yaml_file_body`, FROM YAML FILE detection
- [ ] `cpp/src/shim.cpp` -- file reading + query reconstruction in sv_ddl_bind
- [ ] Update `test/sql/TEST_LIST` with new test file

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | yes | DuckDB's `enable_external_access` GLOBAL setting restricts file access. `read_text()` enforces this automatically. |
| V5 Input Validation | yes | File path extracted from single-quoted string with proper escaping. Trailing content rejected. Empty path rejected. YAML size cap (1MB) applies to file content via `from_yaml_with_size_cap`. |
| V6 Cryptography | no | N/A |

### Known Threat Patterns for File-Based YAML Loading

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal (`FROM YAML FILE '/etc/passwd'`) | Information Disclosure | `read_text()` respects DuckDB's file access controls. When `enable_external_access=false`, all file reads are blocked. DuckDB may also enforce `allowed_directories` for additional path restriction. |
| YAML anchor/alias bomb via file | Denial of Service | `from_yaml_with_size_cap()` enforces 1MB size limit before parsing. This mitigates memory expansion attacks. Note: the 1MB cap is a sanity guard, not a security boundary (per Phase 51 design decision). |
| SQL injection via file path | Tampering | File path is SQL-escaped (single quotes doubled) before embedding in `read_text()` query. The path never passes through SQL as raw text. |
| Large file exhaustion | Denial of Service | `read_text()` reads the full file. DuckDB's built-in limits apply (max 3.9 GiB). The 1MB YAML size cap provides additional protection at the application level. |
| Cloud credential leakage via S3 paths | Information Disclosure | `read_text()` with httpfs follows DuckDB's credential management. No custom credential handling in the extension. |

## Sources

### Primary (HIGH confidence)
- [src/parse.rs lines 1055-1227] -- Phase 52 FROM YAML detection, dollar-quote extraction, YAML rewrite (the patterns to extend) [VERIFIED]
- [cpp/src/shim.cpp lines 134-296] -- sv_ddl_bind structure, sv_ddl_conn usage, sv_rewrite_ddl_rust FFI call [VERIFIED]
- [src/ddl/define.rs lines 1-120] -- DefineState struct, catalog_conn, persist_conn usage (bind-time execution pattern) [VERIFIED]
- [src/model.rs lines 427-457] -- from_yaml_with_size_cap signature and behavior (Phase 51) [VERIFIED]
- [.planning/research/ARCHITECTURE.md lines 224-273] -- FROM YAML FILE architecture, read_text approach, table function alternatives [VERIFIED]
- [.planning/research/PITFALLS.md lines 139-156] -- File I/O security pitfall, enable_external_access enforcement [VERIFIED]

### Secondary (MEDIUM confidence)
- [DuckDB docs: read_text](https://duckdb.org/docs/current/guides/file_formats/read_file) -- Column schema, UTF-8 validation, file size limit [CITED]
- [DuckDB docs: configuration](https://duckdb.org/docs/lts/configuration/overview) -- enable_external_access is GLOBAL scope [CITED]
- [DuckDB docs: securing](https://duckdb.org/docs/stable/operations_manual/securing_duckdb/overview) -- enable_external_access blocks read_csv, read_parquet, etc. [CITED]
- [DuckDB GitHub discussion #12985](https://github.com/duckdb/duckdb/discussions/12985) -- "Table function cannot contain subqueries" confirmation [CITED]

### Tertiary (LOW confidence)
- None -- all critical claims verified against codebase or official DuckDB documentation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all infrastructure exists from Phases 51-52
- Architecture: HIGH -- two-layer approach (Rust detect + C++ read) follows existing patterns. read_text() behavior verified. Sentinel protocol is well-defined.
- Pitfalls: HIGH -- security enforcement verified via DuckDB docs. Dollar-quote collision mitigated by tagged delimiters. Buffer size issue documented with solution.

**Research date:** 2026-04-18
**Valid until:** 2026-05-18 (stable domain; read_text and enable_external_access are core DuckDB features)
