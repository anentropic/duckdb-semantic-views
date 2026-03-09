# Phase 19: Parser Hook Validation Spike - Research

**Researched:** 2026-03-09
**Domain:** DuckDB parser extension fallback mechanism -- which DDL prefixes trigger the hook
**Confidence:** HIGH

## Summary

The parser fallback hook (`parse_function`) is invoked ONLY when DuckDB's own Postgres-based parser fails to parse a statement. The parser uses a fixed grammar (derived from libpg_query) with hardcoded keyword lists for object types. Since `SEMANTIC` is not a keyword in DuckDB's grammar, ALL 7 DDL prefixes in scope will cause parser errors and therefore trigger the fallback hook.

This means **all planned DDL statements can use native syntax via the existing parser hook infrastructure**. No statements need to fall back to function-only interfaces. Phase 20 can proceed with full native DDL coverage for all 6 requirements (DDL-03 through DDL-08).

**Primary recommendation:** All 7 DDL prefixes trigger the parser fallback hook. Proceed with native syntax for all of them. The spike's empirical test should confirm this, then document the scope decision.

## Parser Fallback Mechanism: How It Works

### The Core Algorithm (from `duckdb.cpp` line 321955)

```
1. PostgresParser.Parse(query)
2. If parse succeeds:
   - Transform parse tree to SQL statements
   - RETURN (no fallback, extensions never see it)
3. If parse fails:
   - Split query by semicolons
   - For each sub-statement:
     a. Try PostgresParser again on the individual statement
     b. If parser succeeds: transform and continue
     c. If parser fails: iterate registered parser extensions
        - Call ext.parse_function(query_statement)
        - If PARSE_SUCCESSFUL: use extension's result
        - If DISPLAY_EXTENSION_ERROR: throw extension's error
        - If DISPLAY_ORIGINAL_ERROR: try next extension
     d. If no extension claims it: throw original parser error
```

**Key insight:** The fallback hook is ONLY invoked on parser errors (syntax errors from the Postgres grammar). Catalog errors, binder errors, and execution errors occur AFTER parsing succeeds and never trigger the hook.

### Source: `Parser::ParseQuery` in duckdb.cpp

```cpp
// Line 322033-322051 of duckdb.cpp (vendored amalgamation v1.4.4)
// let extensions parse the statement which DuckDB failed to parse
bool parsed_single_statement = false;
for (auto &ext : *options.extensions) {
    D_ASSERT(ext.parse_function);
    auto result = ext.parse_function(ext.parser_info.get(), query_statement);
    if (result.type == ParserExtensionResultType::PARSE_SUCCESSFUL) {
        // Extension claimed it -- use extension's parse result
        ...
        parsed_single_statement = true;
        break;
    } else if (result.type == ParserExtensionResultType::DISPLAY_EXTENSION_ERROR) {
        throw ParserException::SyntaxError(query, result.error, result.error_location);
    } else {
        // DISPLAY_ORIGINAL_ERROR -- try next extension
    }
}
```

## Analysis: Each DDL Prefix

### Grammar Constraints

DuckDB's Postgres-based parser uses FIXED keyword lists:

- **DROP** grammar (`drop.y`): Accepts only TABLE, VIEW, SEQUENCE, FUNCTION, MACRO, MACRO TABLE, INDEX, FOREIGN TABLE, COLLATION, CONVERSION, SCHEMA, STATISTICS, TEXT SEARCH *, TYPE, ACCESS METHOD, EVENT TRIGGER, EXTENSION, etc. `SEMANTIC` is NOT in this list.

- **CREATE** grammar (`create.y`): After `CREATE`, `CREATE OR REPLACE`, and `CREATE ... IF NOT EXISTS`, the parser expects TABLE, VIEW, FUNCTION, MACRO, INDEX, SEQUENCE, TYPE, SCHEMA, etc. `SEMANTIC` is NOT in this list.

- **DESCRIBE/SHOW** grammar (`variable_show.y`): Accepts `SHOW/DESCRIBE qualified_name`, `SHOW TABLES FROM ...`, `SHOW ALL TABLES`, `SHOW TIME ZONE`, `SHOW TRANSACTION ISOLATION LEVEL`. Does NOT accept arbitrary multi-word object types.

- **`SEMANTIC` keyword status**: NOT a keyword in DuckDB's grammar. Treated as a regular identifier (ColId).

### Prefix-by-Prefix Verdict

| # | DDL Prefix | Parser Behavior | Error Type | Hook Triggered? | Confidence |
|---|-----------|-----------------|------------|-----------------|------------|
| 1 | `DROP SEMANTIC VIEW x` | `DROP` expects fixed object type keyword; `SEMANTIC` not in list | Parser Error | YES | HIGH |
| 2 | `DROP SEMANTIC VIEW IF EXISTS x` | Same as above; grammar never reaches `IF EXISTS` | Parser Error | YES | HIGH |
| 3 | `CREATE OR REPLACE SEMANTIC VIEW x (...)` | `CREATE OR REPLACE` expects TABLE/VIEW/FUNCTION/MACRO; `SEMANTIC` not in list | Parser Error | YES | HIGH |
| 4 | `CREATE SEMANTIC VIEW IF NOT EXISTS x (...)` | `CREATE` does not accept `SEMANTIC` as next token | Parser Error | YES | HIGH |
| 5 | `DESCRIBE SEMANTIC VIEW x` | `DESCRIBE` parses `SEMANTIC` as qualified_name, then `VIEW x` are unexpected extra tokens | Parser Error | YES | HIGH |
| 6 | `SHOW SEMANTIC VIEWS` | `SHOW` parses `SEMANTIC` as qualified_name, then `VIEWS` is unexpected extra token | Parser Error | YES | HIGH |
| 7 | `CREATE SEMANTIC VIEW x (...)` | Already proven in v0.5.0 -- same mechanism | Parser Error | YES | PROVEN |

### Reasoning Detail for DESCRIBE/SHOW

The `variable_show.y` grammar rule for `DESCRIBE qualified_name` expects the statement to end after the qualified name. When parsing `DESCRIBE SEMANTIC VIEW x`:
1. `DESCRIBE` matches the `describe_or_desc` production
2. `SEMANTIC` matches as a `qualified_name` (regular identifier)
3. `VIEW x` are leftover tokens that don't match any grammar continuation
4. The Postgres parser fails because the full statement was not consumed
5. Parser error -> fallback hook invoked

Similarly for `SHOW SEMANTIC VIEWS`:
1. `SHOW` matches the `show_or_describe` production
2. `SEMANTIC` matches as a `qualified_name`
3. `VIEWS` is an unexpected token
4. Parser error -> fallback hook invoked

## Architecture Patterns

### Existing Parser Hook Architecture (v0.5.0)

The current system handles only `CREATE SEMANTIC VIEW`. The pattern is:

```
[C++ shim.cpp]
1. sv_parse_stub(query):
   - Calls sv_parse_rust(query) via FFI
   - Rust checks "CREATE SEMANTIC VIEW" prefix
   - Returns PARSE_SUCCESSFUL or DISPLAY_ORIGINAL_ERROR

2. sv_plan_function(parse_data):
   - Creates TableFunction "sv_ddl_internal"
   - Passes raw query text as VARCHAR parameter

3. sv_ddl_bind(input):
   - Calls sv_execute_ddl_rust(query, ddl_conn) via FFI
   - Rust rewrites "CREATE SEMANTIC VIEW x (...)" to
     "SELECT * FROM create_semantic_view('x', ...)"
   - Executes rewritten SQL on sv_ddl_conn
```

### Extension Pattern for Phase 20

The same architecture extends to all 7 prefixes. The detection function (`sv_parse_rust`) needs to recognize 7 prefix patterns instead of 1. The rewrite function needs to map each prefix to the corresponding function call:

| DDL Prefix | Rewrite Target |
|-----------|---------------|
| `CREATE SEMANTIC VIEW x (...)` | `SELECT * FROM create_semantic_view('x', ...)` |
| `CREATE OR REPLACE SEMANTIC VIEW x (...)` | `SELECT * FROM create_or_replace_semantic_view('x', ...)` |
| `CREATE SEMANTIC VIEW IF NOT EXISTS x (...)` | `SELECT * FROM create_semantic_view_if_not_exists('x', ...)` |
| `DROP SEMANTIC VIEW x` | `SELECT * FROM drop_semantic_view('x')` |
| `DROP SEMANTIC VIEW IF EXISTS x` | `SELECT * FROM drop_semantic_view_if_exists('x')` |
| `DESCRIBE SEMANTIC VIEW x` | `SELECT * FROM describe_semantic_view('x')` |
| `SHOW SEMANTIC VIEWS` | `SELECT * FROM list_semantic_views()` |

All target functions already exist and are registered at extension init time (see `lib.rs` lines 354-413).

### Detection Logic Changes

The current `detect_create_semantic_view` in `parse.rs` only checks the `CREATE SEMANTIC VIEW` prefix. For Phase 20, this needs to become a multi-prefix detector. The pattern:

```rust
// Pseudocode for extended detection
fn detect_semantic_view_ddl(query: &str) -> u8 {
    let trimmed = query.trim().trim_end_matches(';').trim();
    let upper = // case-insensitive prefix check

    if starts_with("CREATE SEMANTIC VIEW") -> PARSE_DETECTED
    if starts_with("CREATE OR REPLACE SEMANTIC VIEW") -> PARSE_DETECTED
    if starts_with("DROP SEMANTIC VIEW") -> PARSE_DETECTED
    if starts_with("DESCRIBE SEMANTIC VIEW") -> PARSE_DETECTED
    if starts_with("SHOW SEMANTIC VIEWS") -> PARSE_DETECTED

    PARSE_NOT_OURS
}
```

The rewrite function needs a similar multi-branch pattern.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DDL execution | Custom SQL execution path | Existing rewrite-to-function pattern | Already proven; all target functions exist |
| DDL connection | New connection per statement | Existing `sv_ddl_conn` static | Reuse the DDL connection from v0.5.0 |
| Prefix detection | Full SQL parser | ASCII prefix matching | 7 fixed prefixes; no parsing ambiguity |
| Catalog operations | Direct catalog manipulation from C++ | Rust function-based DDL | All catalog logic is in Rust; rewrite bridge maintains this |

## Common Pitfalls

### Pitfall 1: Assuming Catalog Errors Trigger the Hook
**What goes wrong:** Someone assumes `DROP VIEW semantic_view_name` (without `SEMANTIC`) would trigger the fallback. It would NOT -- DuckDB's parser accepts `DROP VIEW x` successfully, then the binder/catalog reports "view not found." The fallback hook is NEVER invoked for catalog errors.
**How to avoid:** Only the `SEMANTIC` keyword in the DDL prefix causes parser failure. All DDL must include `SEMANTIC` to route through the extension.

### Pitfall 2: Case Sensitivity in Prefix Detection
**What goes wrong:** Users type `create Semantic View`, `DROP semantic VIEW`, etc. If detection is case-sensitive, these variants fail silently (DuckDB shows its parser error instead of the extension handling it).
**How to avoid:** Use `eq_ignore_ascii_case` for all prefix comparisons, as the existing `detect_create_semantic_view` already does.

### Pitfall 3: Semicolon Stripping Inconsistency (DuckDB Issue #18485)
**What goes wrong:** DuckDB's `SplitQueries` re-appends `;` to middle statements but not the last one. If the detection or rewrite function doesn't handle both cases, multi-statement batches may fail.
**How to avoid:** Always `trim_end_matches(';').trim()` before prefix detection, as the existing code does.

### Pitfall 4: Three-Connection Lock Conflict During DROP
**What goes wrong:** DROP needs to delete from `semantic_layer._definitions` (persist_conn), remove from in-memory HashMap (catalog), and the DDL connection (sv_ddl_conn) is executing the rewritten SQL. If these overlap, there could be lock conflicts.
**How to avoid:** The rewrite pattern avoids this: sv_ddl_conn executes `SELECT * FROM drop_semantic_view('x')`, which internally uses persist_conn for the catalog table delete. The connections are used sequentially, not concurrently. But this should be tested empirically in Phase 20.

### Pitfall 5: DESCRIBE/SHOW Prefix Ambiguity
**What goes wrong:** `DESCRIBE SEMANTIC` (without `VIEW`) could match as "describe a table called semantic." The extension should NOT intercept this -- it's a valid DuckDB statement. Only `DESCRIBE SEMANTIC VIEW` should trigger the hook.
**How to avoid:** Prefix detection must require the full multi-word prefix: `DESCRIBE SEMANTIC VIEW` (3 words), not just `DESCRIBE SEMANTIC` (2 words).

## Code Examples

### Current Detection (v0.5.0)
```rust
// Source: src/parse.rs (current implementation)
pub fn detect_create_semantic_view(query: &str) -> u8 {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();
    let prefix = "create semantic view";
    if trimmed.len() < prefix.len() {
        return PARSE_NOT_OURS;
    }
    if trimmed.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
        PARSE_DETECTED
    } else {
        PARSE_NOT_OURS
    }
}
```

### Rewrite Pattern (v0.5.0)
```rust
// Source: src/parse.rs (current implementation)
pub fn rewrite_ddl_to_function_call(query: &str) -> Result<String, String> {
    let (name, body) = parse_ddl_text(query)?;
    let safe_name = name.replace('\'', "''");
    Ok(format!(
        "SELECT * FROM create_semantic_view('{safe_name}', {body})"
    ))
}
```

### C++ Plan Function (v0.5.0)
```cpp
// Source: cpp/src/shim.cpp (current implementation)
static ParserExtensionPlanResult sv_plan_function(
    ParserExtensionInfo *, ClientContext &,
    unique_ptr<ParserExtensionParseData> parse_data) {
    auto &sv_data = dynamic_cast<SemanticViewParseData &>(*parse_data);
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

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Function-only DDL | Native DDL via parser hook (v0.5.0) | 2026-03-08 | Users write `CREATE SEMANTIC VIEW` instead of `FROM create_semantic_view(...)` |
| C++ symbol resolution | Static-linked amalgamation | v0.5.0 | Bypasses `-fvisibility=hidden`; works in Python DuckDB |
| Custom SQL grammar | Statement rewriting | v0.5.0 | Simpler; reuses all existing function-based DDL code |

**Reference:** DuckPGQ extension uses the same parser hook approach for `CREATE PROPERTY GRAPH`, `DROP PROPERTY GRAPH`, `CREATE OR REPLACE PROPERTY GRAPH`, etc. This validates the pattern for custom DDL statements.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test + sqllogictest + DuckLake CI + vtab crash tests |
| Config file | justfile (task runner), Makefile (build system) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map

This phase has no formal requirement IDs (it's a scope-determination spike). The validation is empirical:

| Test | Behavior | Test Type | Automated Command | File Exists? |
|------|----------|-----------|-------------------|-------------|
| Prefix 1 | `DROP SEMANTIC VIEW x` triggers parser error | integration (manual) | `just build && duckdb -cmd "LOAD ..." -c "DROP SEMANTIC VIEW x"` | No -- spike |
| Prefix 2 | `DROP SEMANTIC VIEW IF EXISTS x` triggers parser error | integration (manual) | Same pattern | No -- spike |
| Prefix 3 | `CREATE OR REPLACE SEMANTIC VIEW x (...)` triggers parser error | integration (manual) | Same pattern | No -- spike |
| Prefix 4 | `CREATE SEMANTIC VIEW IF NOT EXISTS x (...)` triggers parser error | integration (manual) | Same pattern | No -- spike |
| Prefix 5 | `DESCRIBE SEMANTIC VIEW x` triggers parser error | integration (manual) | Same pattern | No -- spike |
| Prefix 6 | `SHOW SEMANTIC VIEWS` triggers parser error | integration (manual) | Same pattern | No -- spike |
| Prefix 7 | `CREATE SEMANTIC VIEW x (...)` (existing, already works) | integration | `just test-sql` (phase16_parser.test) | Yes |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] SQL logic test file for prefix validation (`test/sql/phase19_parser_hook_validation.test`) -- covers all 7 prefixes
- No framework install needed -- test infrastructure exists

## Open Questions

1. **DESCRIBE SEMANTIC VIEW vs DESCRIBE SEMANTIC**
   - What we know: Grammar accepts `DESCRIBE qualified_name`, so `DESCRIBE SEMANTIC` parses successfully as "describe table semantic." Only `DESCRIBE SEMANTIC VIEW x` has leftover tokens causing parser failure.
   - What's unclear: Empirical confirmation needed that the grammar does not have a special multi-token production that could consume `VIEW` after a name.
   - Recommendation: Test empirically in the spike. HIGH confidence this is a parser error based on grammar analysis.

2. **SHOW SEMANTIC VIEWS vs SHOW SEMANTIC**
   - What we know: Same pattern -- `SHOW SEMANTIC` would parse as "show table semantic" but `SHOW SEMANTIC VIEWS` has `VIEWS` as leftover.
   - What's unclear: Same as above.
   - Recommendation: Test empirically. HIGH confidence.

3. **Multi-statement batches**
   - What we know: `SplitQueries` splits on `;` and retries each statement individually. The parser hook works per-statement.
   - What's unclear: Whether mixing native DDL with regular SQL in a single batch (e.g., `CREATE TABLE t (x INT); CREATE SEMANTIC VIEW v (...)`) works correctly.
   - Recommendation: Test in the spike with a multi-statement batch.

## Sources

### Primary (HIGH confidence)
- DuckDB amalgamation source (`cpp/include/duckdb.cpp`, vendored v1.4.4) -- `Parser::ParseQuery` implementation at line 321955, grammar actions at lines 507244+
- DuckDB amalgamation header (`cpp/include/duckdb.hpp`, vendored v1.4.4) -- `ParserExtensionResultType` enum at line 32912
- Project source: `src/parse.rs`, `cpp/src/shim.cpp`, `src/lib.rs`
- [DuckDB DROP grammar](https://raw.githubusercontent.com/duckdb/duckdb/main/third_party/libpg_query/grammar/statements/drop.y) -- fixed object type keyword list
- [DuckDB parser.cpp](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- ParseQuery with extension fallback logic

### Secondary (MEDIUM confidence)
- [DuckDB Runtime-Extensible Parsers blog post](https://duckdb.org/2024/11/22/runtime-extensible-parsers) -- confirms fallback mechanism design
- [DuckDB DROP Statement docs](https://duckdb.org/docs/stable/sql/statements/drop) -- lists supported object types
- [DuckDB DESCRIBE Statement docs](https://duckdb.org/docs/stable/sql/statements/describe) -- confirms DESCRIBE syntax
- [DuckDB SHOW Statement docs](https://duckdb.org/docs/stable/sql/statements/show) -- confirms SHOW syntax

### Tertiary (LOW confidence)
- [DuckPGQ extension](https://duckpgq.org/documentation/property_graph/) -- uses similar parser hook pattern for custom DDL (CREATE/DROP PROPERTY GRAPH)

## Metadata

**Confidence breakdown:**
- Parser fallback mechanism: HIGH -- verified from vendored source code (ParseQuery implementation)
- DROP prefix triggers hook: HIGH -- grammar file confirms fixed keyword list, `SEMANTIC` not present
- CREATE OR REPLACE prefix triggers hook: HIGH -- same grammar analysis
- CREATE IF NOT EXISTS prefix triggers hook: HIGH -- same as existing CREATE SEMANTIC VIEW (proven in v0.5.0)
- DESCRIBE prefix triggers hook: HIGH -- grammar analysis shows `DESCRIBE qualified_name` cannot consume multi-word `SEMANTIC VIEW x`
- SHOW prefix triggers hook: HIGH -- same grammar analysis as DESCRIBE
- All 7 prefixes feasible: HIGH -- grammar + source code analysis, no dependency on external claims

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- DuckDB parser architecture unlikely to change within one minor version)
