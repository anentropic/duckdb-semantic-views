# Requirements: DuckDB Semantic Views

**Defined:** 2026-03-07
**Core Value:** Prove that native `CREATE SEMANTIC VIEW` DDL syntax is achievable via DuckDB parser extension hooks in a Rust+C++ mixed extension.

## v0.5.0 Requirements

Requirements for the parser extension spike. Each maps to roadmap phases.

### Build System

- [ ] **BUILD-01**: C++ shim compiles via `cc` crate against vendored DuckDB amalgamation header (`duckdb.hpp` v1.4.4)
- [ ] **BUILD-02**: Symbol visibility updated in `build.rs` to export the correct entry point symbol(s) for the chosen ABI strategy
- [ ] **BUILD-03**: Extension binary loads successfully via `LOAD` in DuckDB CLI and Python client (`import duckdb; conn.load_extension(...)`) — validates compatibility with Python DuckDB's `-fvisibility=hidden` compilation
- [ ] **BUILD-04**: `cargo test` (bundled mode) continues to pass without C++ compilation overhead
- [ ] **BUILD-05**: Extension is publishable to DuckDB community extension registry (correct footer, platform binaries, no CMake dependency)

### Entry Point

- [ ] **ENTRY-01**: POC Option A — keep `C_STRUCT` footer, Rust entry initializes normally, then calls a linked C++ function that registers parser hooks using the `duckdb_database` handle
- [ ] **ENTRY-02**: POC Option B — switch to `CPP` footer, C++ entry via `DUCKDB_CPP_EXTENSION_ENTRY`, delegates to Rust init via FFI, Rust C API stubs are initialized
- [ ] **ENTRY-03**: Chosen strategy preserves all existing `semantic_view()` query functionality (existing sqllogictest suite passes)

### Parser Hooks

- [ ] **PARSE-01**: `parse_function` registered as a fallback hook — only fires for statements DuckDB's parser cannot handle
- [ ] **PARSE-02**: `CREATE SEMANTIC VIEW name (...)` is recognized by the parse function (case-insensitive, handles leading whitespace and trailing semicolons)
- [ ] **PARSE-03**: Parse function returns `DISPLAY_ORIGINAL_ERROR` for all non-semantic-view statements (zero overhead for normal SQL)
- [ ] **PARSE-04**: Parse function delegates to Rust via FFI — C++ trampoline calls `extern "C"` Rust function
- [ ] **PARSE-05**: Rust parse function is panic-safe (`catch_unwind`) and thread-safe (no shared mutable state)

### DDL Execution

- [ ] **DDL-01**: `CREATE SEMANTIC VIEW name (tables := [...], dimensions := [...], metrics := [...])` creates a semantic view via parser hook → plan function → existing catalog code
- [ ] **DDL-02**: View created via native DDL is queryable via `FROM semantic_view('name', dimensions := [...], metrics := [...])`
- [ ] **DDL-03**: Existing function-based DDL (`FROM create_semantic_view(...)`) continues to work alongside native DDL

### Verification

- [ ] **VERIFY-01**: `just test-all` passes (Rust unit tests, sqllogictest, DuckLake CI)
- [ ] **VERIFY-02**: At least one sqllogictest test exercises the native `CREATE SEMANTIC VIEW` syntax end-to-end

## Future Requirements

Deferred to subsequent milestone after spike validates the approach.

### Extended DDL Surface

- **DDL-F01**: `DROP SEMANTIC VIEW name` via parser hook
- **DDL-F02**: `CREATE OR REPLACE SEMANTIC VIEW` via parser hook
- **DDL-F03**: `CREATE SEMANTIC VIEW IF NOT EXISTS` via parser hook
- **DDL-F04**: `DROP SEMANTIC VIEW IF EXISTS` via parser hook
- **DDL-F05**: `DESCRIBE SEMANTIC VIEW name` via parser hook
- **DDL-F06**: `SHOW SEMANTIC VIEWS` / `LIST SEMANTIC VIEWS` via parser hook

### Parser Quality

- **QUAL-F01**: Error location reporting (`error_location` in parse result) for syntax errors
- **QUAL-F02**: Custom `ParserExtensionInfo` carrying Rust catalog pointer
- **QUAL-F03**: Helpful error messages for malformed `CREATE SEMANTIC VIEW` statements

## Out of Scope

| Feature | Reason |
|---------|--------|
| Native query syntax change (`QUERY SEMANTIC VIEW` or similar) | Separate concern from DDL; existing `semantic_view()` table function works well |
| `parser_override` hook | Only needed for full language replacement (PRQL); `parse_function` fallback is correct for DDL |
| OperatorExtension / stash pattern | Not needed for DDL — direct TableFunction return (Path A) is simpler |
| YAML/file-based DDL | SQL DDL first; YAML is a future path |
| Custom SQL grammar parser | Spike uses statement rewriting (rewrites to existing function-based DDL); custom grammar deferred |
| Windows support | macOS and Linux are primary targets; Windows can follow |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| BUILD-01 | TBD | Pending |
| BUILD-02 | TBD | Pending |
| BUILD-03 | TBD | Pending |
| BUILD-04 | TBD | Pending |
| ENTRY-01 | TBD | Pending |
| ENTRY-02 | TBD | Pending |
| ENTRY-03 | TBD | Pending |
| PARSE-01 | TBD | Pending |
| PARSE-02 | TBD | Pending |
| PARSE-03 | TBD | Pending |
| PARSE-04 | TBD | Pending |
| PARSE-05 | TBD | Pending |
| DDL-01 | TBD | Pending |
| DDL-02 | TBD | Pending |
| DDL-03 | TBD | Pending |
| VERIFY-01 | TBD | Pending |
| VERIFY-02 | TBD | Pending |

**Coverage:**
- v0.5.0 requirements: 17 total
- Mapped to phases: 0
- Unmapped: 17

---
*Requirements defined: 2026-03-07*
*Last updated: 2026-03-07 after initial definition*
