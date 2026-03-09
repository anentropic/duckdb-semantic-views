# Phase 21: Error Location Reporting - Research

**Researched:** 2026-03-09
**Domain:** Parser error reporting with position info for DuckDB parser extension hook
**Confidence:** HIGH

## Summary

Phase 21 adds actionable, positioned error messages for malformed DDL statements. The research confirms that DuckDB's `ParserExtensionParseResult` struct already has full support for extension-provided errors with character position: the `DISPLAY_EXTENSION_ERROR` result type carries both an `error` string and an `error_location` (`optional_idx` character offset). When returned from `sv_parse_stub`, DuckDB calls `ParserException::SyntaxError(query, result.error, result.error_location)` which renders a caret (`^`) at the specified position in the original query.

The implementation requires changes at two layers: the C++ `sv_parse_stub` must support a third return path (error with position), and the Rust parsing functions must grow a validation layer that detects structural errors (missing clauses, unbalanced brackets, etc.) and near-miss DDL prefixes. All fuzzy matching reuses the existing `strsim` crate and `suggest_closest()` function pattern. No new dependencies are needed.

**Primary recommendation:** Add a new Rust FFI function (e.g., `sv_validate_ddl_rust`) that returns a tri-state: success (rewrite SQL), error (message + position), or not-ours. The C++ `sv_parse_stub` maps these to `PARSE_SUCCESSFUL`, `DISPLAY_EXTENSION_ERROR`, or `DISPLAY_ORIGINAL_ERROR` respectively.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Error messages identify the problem AND state what was expected (e.g., "Error in DIMENSIONS clause: expected list of STRUCT definitions, got empty value")
- No recovery hints (no "Run DESCRIBE..." suggestions)
- No prefix -- error messages stand alone, matching DuckDB's native error feel
- Plain error text only -- DuckDB's renderer handles caret/position display
- "Did you mean" scope: near-miss DDL prefixes, clause keyword typos, view names on DROP/DESCRIBE (NOT struct field names)
- Common mistake patterns: missing required clauses, bracket/paren mismatch, empty body
- Caret position strategy: clause errors point at clause keyword start; structural errors point at end of prefix; fallback includes "at position N" in text
- Reuse `strsim` crate and `suggest_closest()` from `expand.rs`
- Reuse `QueryError::ViewNotFound` pattern for "Did you mean" formatting

### Claude's Discretion
- Internal error struct design (ParseError type, position encoding)
- Whether to parse clause boundaries with simple string scanning or a more structured approach
- Exact fuzzy match threshold for DDL prefix near-misses (existing code uses edit distance <= 3)
- Test strategy and error message wording details

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ERR-01 | Malformed DDL statements show clause-level error hints | Validation layer in `parse.rs` detects missing/malformed clauses in CREATE body; caret points at clause keyword; error message names the clause and expected content |
| ERR-02 | Error messages include character position for DuckDB caret rendering | `ParserExtensionParseResult.error_location` (type `optional_idx`) is passed through to `ParserException::SyntaxError` which renders the caret. Confirmed in amalgamation source. |
| ERR-03 | Misspelled keywords and view names show "did you mean" suggestions | `strsim::levenshtein` with threshold <= 3 for DDL prefix near-misses and clause keyword typos; existing `suggest_closest()` pattern reusable; view name suggestions on DROP/DESCRIBE use catalog lookup |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| strsim | 0.11 | Levenshtein distance for fuzzy matching | Already in Cargo.toml deps; used by `suggest_closest()` in expand.rs |

### Supporting
No additional dependencies needed. All error reporting uses DuckDB's built-in `ParserExtensionParseResult` error path.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| strsim::levenshtein | strsim::jaro_winkler | Levenshtein is simpler, already used, threshold of 3 is well-understood |
| String scanning for clauses | nom/pest/sqlparser | REQUIREMENTS.md explicitly excludes full SQL parsers -- 4 clause keywords (tables, relationships, dimensions, metrics) are trivially found with string scanning |
| miette/ariadne error crates | Plain text errors | REQUIREMENTS.md explicitly excludes these -- DuckDB error channel is plain text |

## Architecture Patterns

### Error Flow Architecture (New)

```
User DDL statement
      |
      v
DuckDB Parser fails --> calls sv_parse_stub(query)
      |
      v
sv_parse_stub calls sv_validate_ddl_rust(query_ptr, query_len,
                                          sql_out, sql_out_len,
                                          error_out, error_out_len,
                                          position_out)
      |
      v
Rust: detect_ddl_kind(query)
      |
      +--> Some(kind) : DDL detected
      |    |
      |    +--> validate_ddl_body(query, kind)
      |         |
      |         +--> Ok(rewritten_sql) --> return SUCCESS (0)
      |         |
      |         +--> Err(ParseError{msg, position}) --> return ERROR (1)
      |
      +--> None : not an exact DDL match
           |
           +--> detect_near_miss(query)
                |
                +--> Some(NearMiss{msg, position}) --> return ERROR (1)
                |
                +--> None --> return NOT_OURS (2)
```

### C++ sv_parse_stub Return Mapping

```
sv_validate_ddl_rust returns:
  0 (success)  --> ParserExtensionParseResult(make_uniq<SemanticViewParseData>(query))
  1 (error)    --> ParserExtensionParseResult(string(error_out))
                   with .error_location = position_out (if valid)
  2 (not ours) --> ParserExtensionParseResult()  [DISPLAY_ORIGINAL_ERROR]
```

### Recommended Project Structure Change

```
src/
  parse.rs              # Add: ParseError struct, validate_ddl_body(),
                        #       detect_near_miss(), sv_validate_ddl_rust FFI
                        # Existing: detect_ddl_kind, rewrite_ddl, etc.
  expand.rs             # Unchanged (suggest_closest reused via crate path)
  query/error.rs        # Unchanged (pattern reference for "Did you mean")
cpp/
  src/shim.cpp          # Modify: sv_parse_stub to call sv_validate_ddl_rust
                        #         instead of sv_parse_rust, handle 3 return codes
```

### Pattern 1: ParseError Struct (Rust)

**What:** A struct carrying error message and optional character position.
**When to use:** Returned by all validation functions in parse.rs.
**Example:**
```rust
// Source: Internal design, follows DuckDB's optional_idx pattern
pub struct ParseError {
    pub message: String,
    /// Character offset (0-based) into the original query string.
    /// Corresponds to DuckDB's `optional_idx error_location`.
    pub position: Option<usize>,
}
```

### Pattern 2: Near-Miss DDL Prefix Detection

**What:** Fuzzy matching against the 7 known DDL prefixes to detect typos like "CREAT SEMANTIC VIEW".
**When to use:** When `detect_ddl_kind()` returns `None` -- before giving up.
**Example:**
```rust
// Source: Existing suggest_closest pattern from expand.rs
const DDL_PREFIXES: &[&str] = &[
    "create semantic view",
    "create or replace semantic view",
    "create semantic view if not exists",
    "drop semantic view",
    "drop semantic view if exists",
    "describe semantic view",
    "show semantic views",
];

fn detect_near_miss(query: &str) -> Option<ParseError> {
    let trimmed = query.trim().to_ascii_lowercase();
    // Extract first N words (enough to cover longest prefix)
    // Compare against known prefixes using Levenshtein
    // If edit distance <= 3, suggest the correct prefix
    // Position: 0 (start of query after trimming)
}
```

### Pattern 3: Clause Validation for CREATE Body

**What:** After extracting the body of a CREATE statement, check for missing required clauses and detect typos in clause keywords.
**When to use:** During `validate_ddl_body()` for CREATE/CREATE OR REPLACE/CREATE IF NOT EXISTS.
**Example:**
```rust
const CLAUSE_KEYWORDS: &[&str] = &["tables", "relationships", "dimensions", "metrics"];

fn validate_clauses(body: &str, body_offset: usize) -> Result<(), ParseError> {
    // Check for required clauses: tables must be present, plus dimensions or metrics
    // For each word followed by `:=`, check if it's a known clause keyword
    // If not, suggest closest using strsim::levenshtein
    // Position: byte offset of the unknown keyword within the original query
}
```

### Pattern 4: DuckDB Error Position with Caret (Confirmed)

**What:** DuckDB's parser extension error path supports character positions natively.
**When to use:** Always, when returning `DISPLAY_EXTENSION_ERROR`.
**Example (C++ side):**
```cpp
// Source: duckdb.cpp lines 322047-322048 (DuckDB amalgamation 1.4.4)
// DuckDB calls: throw ParserException::SyntaxError(query, result.error, result.error_location);
// Which stores position in extra_info["position"] via Exception::SetQueryLocation

// In sv_parse_stub:
ParserExtensionParseResult err_result(string(error_buf));
if (position != UINT32_MAX) {  // sentinel for "no position"
    err_result.error_location = static_cast<idx_t>(position);
}
return err_result;
```

### Pattern 5: FFI Interface for Tri-State Return

**What:** New Rust FFI function returning 0/1/2 with output buffers for SQL/error + position.
**When to use:** Called from C++ `sv_parse_stub` to replace `sv_parse_rust`.
**Example:**
```rust
/// FFI entry point for DDL validation with error reporting.
///
/// Returns:
///   0 = success: rewritten SQL in sql_out
///   1 = error: error message in error_out, position in *position_out
///   2 = not ours: no output written
///
/// position_out is set to u32::MAX when no position is available.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_validate_ddl_rust(
    query_ptr: *const u8,
    query_len: usize,
    sql_out: *mut u8,
    sql_out_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
    position_out: *mut u32,
) -> u8 {
    // ...
}
```

### Anti-Patterns to Avoid

- **Computing position from the rewritten SQL instead of the original query:** The caret must point into the original user-typed DDL string, not the rewritten function call.
- **Using byte offsets when the query contains multi-byte characters:** DuckDB uses byte offsets for `error_location`. Since DDL keywords are ASCII, this is fine for all keyword detection. Document that positions are byte offsets.
- **Trying to validate SQL expressions within clauses at parse time:** Per CONTEXT.md, DuckDB validates expressions at query time. Only validate structural syntax (keywords, brackets, commas).
- **Returning PARSE_SUCCESSFUL and then erroring in sv_ddl_bind:** Errors in `sv_ddl_bind` are thrown as `BinderException`, which does NOT render a caret in the original query. The caret only works via `DISPLAY_EXTENSION_ERROR` from the parse function.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Fuzzy string matching | Custom edit distance | `strsim::levenshtein` | Already a dependency, well-tested, covers the use case |
| Error position rendering | Custom caret formatter | DuckDB's `ParserException::SyntaxError` via `error_location` | DuckDB handles all rendering (CLI, JDBC, Python). Custom formatting would look wrong in non-CLI contexts. |
| DDL prefix matching | Regex or grammar parser | ASCII byte comparison + Levenshtein | 7 fixed prefixes. Regex/parser adds ~500KB for no benefit. Explicitly excluded in REQUIREMENTS.md. |

**Key insight:** DuckDB already has the caret rendering infrastructure. We just need to provide the right position value through the existing `ParserExtensionParseResult.error_location` field.

## Common Pitfalls

### Pitfall 1: Position Off-By-One from Whitespace Trimming
**What goes wrong:** `parse.rs` trims leading whitespace from queries before processing. If position is calculated relative to the trimmed string, the caret will be shifted left in the original query.
**Why it happens:** `query.trim()` removes leading whitespace, changing byte offsets.
**How to avoid:** Calculate positions relative to the ORIGINAL query string (before trimming). Track the trim offset: `let trim_offset = query.len() - query.trim_start().len()`.
**Warning signs:** Caret points to wrong character in queries with leading whitespace (e.g., `  CREATE SEMANTIC VIEW ...`).

### Pitfall 2: sv_ddl_bind Errors Don't Get Carets
**What goes wrong:** Errors thrown by `sv_ddl_bind` (as `BinderException`) don't render a caret in the original query.
**Why it happens:** By the time `sv_ddl_bind` runs, the parse phase is complete. DuckDB's `ParserException::SyntaxError` with caret rendering only works during the parse phase (via `DISPLAY_EXTENSION_ERROR`).
**How to avoid:** Move ALL structural validation into the parse function (Rust `sv_validate_ddl_rust`). The `sv_ddl_bind` path should only fail for execution errors (e.g., "view already exists"), not syntax errors.
**Warning signs:** Error messages that should have carets don't have them.

### Pitfall 3: Fuzzy Match False Positives on Short Queries
**What goes wrong:** Short queries like "SHOW TABLES" have edit distance <= 3 to "SHOW SEMANTIC VIEWS" and might trigger false "Did you mean" suggestions.
**Why it happens:** Levenshtein distance between "SHOW TABLES" and "SHOW SEMANTIC VIEWS" is large (> 3), but partial prefix matching could be tricky if not careful.
**How to avoid:** Compare the full multi-word prefix, not individual words. The 7 DDL prefixes are long enough (18-34 chars) that edit distance <= 3 has very low false positive rate. Consider increasing the threshold analysis: "CREAT SEMANTIC VIEW" (distance 1), "SHOW SEMANTIC VIEW" vs "SHOW SEMANTIC VIEWS" (distance 1), "DESCRIBE SEMANTC VIEW" (distance 1).
**Warning signs:** Normal SQL statements get intercepted with "Did you mean" messages.

### Pitfall 4: Clause Keyword Detection in String Literals
**What goes wrong:** Scanning for clause keywords (tables, dimensions, metrics) finds matches inside string literals in the body (e.g., `'tables'` in a struct value).
**Why it happens:** Simple string search doesn't distinguish between keywords at the clause level and string content.
**How to avoid:** Only look for keywords followed by `:=` at the top level of the body. The pattern `word :=` is the clause assignment syntax. A simple scan for `\btables\s*:=` (word boundary + walrus operator) is sufficient since struct field values never contain `:=` at the same nesting level.
**Warning signs:** False positive clause detection when struct values contain clause keyword names.

### Pitfall 5: Bracket Counting for Paren Mismatch
**What goes wrong:** Simple open/close paren counting doesn't account for parens inside string literals.
**Why it happens:** The body `tables := [{'alias': 'a(b)', ...}]` contains a paren inside a string.
**How to avoid:** Track whether we're inside a single-quoted string when counting brackets. Toggle a flag on each unescaped `'`. This is the minimal string-awareness needed.
**Warning signs:** Bracket mismatch errors on valid DDL that contains parens in string values.

## Code Examples

Verified patterns from the codebase and DuckDB amalgamation source.

### DuckDB ParserExtensionParseResult Error Path (Confirmed)
```cpp
// Source: duckdb.cpp lines 322047-322048 (DuckDB amalgamation 1.4.4, vendored)
// When parse_function returns DISPLAY_EXTENSION_ERROR:
} else if (result.type == ParserExtensionResultType::DISPLAY_EXTENSION_ERROR) {
    throw ParserException::SyntaxError(query, result.error, result.error_location);
}

// ParserException::SyntaxError implementation (duckdb.cpp line 56389):
ParserException ParserException::SyntaxError(const string &query,
    const string &error_message, optional_idx error_location) {
    return ParserException(error_message,
        Exception::InitializeExtraInfo("SYNTAX_ERROR", error_location));
}

// Exception::SetQueryLocation (duckdb.cpp line 56679):
void Exception::SetQueryLocation(optional_idx error_location,
    unordered_map<string, string> &extra_info) {
    if (error_location.IsValid()) {
        extra_info["position"] = to_string(error_location.GetIndex());
    }
}
```

### ParserExtensionParseResult Struct (Confirmed)
```cpp
// Source: duckdb.hpp lines 32924-32942 (vendored amalgamation)
struct ParserExtensionParseResult {
    ParserExtensionParseResult()
        : type(ParserExtensionResultType::DISPLAY_ORIGINAL_ERROR) {}
    explicit ParserExtensionParseResult(string error_p)
        : type(ParserExtensionResultType::DISPLAY_EXTENSION_ERROR),
          error(std::move(error_p)) {}
    explicit ParserExtensionParseResult(unique_ptr<ParserExtensionParseData> parse_data_p)
        : type(ParserExtensionResultType::PARSE_SUCCESSFUL),
          parse_data(std::move(parse_data_p)) {}

    ParserExtensionResultType type;
    unique_ptr<ParserExtensionParseData> parse_data;
    string error;
    optional_idx error_location;  // <-- This is what we need for ERR-02
};
```

### Existing suggest_closest Pattern (Confirmed)
```rust
// Source: src/expand.rs lines 12-28
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let query = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let dist = strsim::levenshtein(&query, &candidate.to_ascii_lowercase());
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, candidate));
                }
            } else {
                best = Some((dist, candidate));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}
```

### Existing ViewNotFound Error Pattern (Confirmed)
```rust
// Source: src/query/error.rs lines 34-53
Self::ViewNotFound { name, suggestion, available } => {
    write!(f, "Semantic view '{name}' not found.")?;
    if let Some(s) = suggestion {
        write!(f, " Did you mean '{s}'?")?;
    }
    if !available.is_empty() {
        write!(f, " Available views: [{}].", available.join(", "))?;
    }
    // ...
}
```

### Current C++ sv_parse_stub (To Be Modified)
```cpp
// Source: cpp/src/shim.cpp lines 60-73
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *, const string &query) {
    uint8_t result = sv_parse_rust(
        reinterpret_cast<const char *>(query.c_str()),
        query.size());
    if (result == 1) {
        return ParserExtensionParseResult(
            make_uniq<SemanticViewParseData>(query));
    }
    return ParserExtensionParseResult();
}
```

### Current Rust FFI Rewrite Function (Reference)
```rust
// Source: src/parse.rs lines 334-371
// sv_rewrite_ddl_rust signature and return convention:
// Returns 0 on success (sql in sql_out), 1 on failure (error in error_out)
// This is the pattern to follow for the new tri-state function.
pub extern "C" fn sv_rewrite_ddl_rust(
    query_ptr: *const u8, query_len: usize,
    sql_out: *mut u8, sql_out_len: usize,
    error_out: *mut u8, error_out_len: usize,
) -> u8 { /* ... */ }
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `sv_parse_rust` binary detect (0/1) | `sv_validate_ddl_rust` tri-state (0/1/2) with error+position | Phase 21 | Enables caret rendering and clause-level hints |
| Errors only in `sv_ddl_bind` (BinderException) | Errors in parse phase (DISPLAY_EXTENSION_ERROR) | Phase 21 | Caret rendering only works from parse phase |
| No near-miss detection | Fuzzy DDL prefix matching | Phase 21 | Catches typos like "CREAT SEMANTIC VIEW" |

**Critical finding:** `DISPLAY_EXTENSION_ERROR` with `error_location` is the ONLY way to get DuckDB to render the caret (`^`) in the error output. Errors thrown from `sv_ddl_bind` (BinderException) do not get caret rendering against the original query text. This means ALL structural validation MUST happen in the parse phase, not the bind phase.

## Open Questions

1. **Position accuracy for multi-statement queries**
   - What we know: `sv_parse_stub` receives individual statements (after `SplitQueries`), but `SyntaxError` is thrown with the full original query. DuckDB does NOT adjust `error_location` by `stmt_loc`.
   - What's unclear: Whether the position from our extension should be relative to the individual statement or the full query.
   - Recommendation: Provide position relative to the individual statement. For single-statement DDL (the common case), this is identical to the full query position. Multi-statement edge cases (e.g., `SELECT 1; CREATE SEMANTIC VIEW ...;`) may have off-by-N positioning, but this is a DuckDB framework limitation and acceptable.

2. **View name suggestions on DROP/DESCRIBE require catalog access**
   - What we know: The parse function does NOT have access to the semantic view catalog. The catalog is a Rust-side `HashMap` behind a `Mutex`.
   - What's unclear: Whether to pass the catalog into the parse validation, or handle view-not-found suggestions separately.
   - Recommendation: View-not-found "Did you mean" suggestions already work in the bind/execute phase (existing `QueryError::ViewNotFound`). Keep them there. The parse phase handles only SYNTAX errors (prefix typos, structural errors). This separation is clean and avoids threading catalog state into the FFI parse path.

3. **BinderException position support**
   - What we know: `BinderException` also accepts `optional_idx error_location` (duckdb.hpp line 20499). However, the position it renders may refer to the rewritten SQL (the function call), not the original DDL query.
   - What's unclear: Whether DuckDB's binder error rendering uses the original query or the rewritten SQL for caret placement.
   - Recommendation: Do not rely on BinderException for caret rendering. Move all syntax validation to the parse phase where `DISPLAY_EXTENSION_ERROR` is well-understood.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `#[test]` + sqllogictest (DuckDB test runner) |
| Config file | `test/sql/*.test` for sqllogictest; inline `#[cfg(test)] mod tests` for Rust |
| Quick run command | `cargo test -- parse` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ERR-01 | Clause-level error hints for malformed CREATE body | unit | `cargo test -- validate_ddl` | Wave 0 |
| ERR-01 | Clause-level errors render through extension load | integration | `just test-sql` (phase21_error_reporting.test) | Wave 0 |
| ERR-02 | Error position produces DuckDB caret rendering | integration | `just test-sql` (phase21_error_reporting.test) | Wave 0 |
| ERR-02 | Position calculation is correct for various whitespace patterns | unit | `cargo test -- parse_error_position` | Wave 0 |
| ERR-03 | Near-miss DDL prefix suggests correction | unit | `cargo test -- near_miss` | Wave 0 |
| ERR-03 | Clause keyword typo suggests correction | unit | `cargo test -- clause_typo` | Wave 0 |
| ERR-03 | View name suggestion on DROP/DESCRIBE nonexistent view | integration | `just test-sql` (existing phase20 tests cover this via ViewNotFound) | Exists |

### Sampling Rate
- **Per task commit:** `cargo test -- parse`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase21_error_reporting.test` -- integration tests for caret rendering and error messages through full extension load
- [ ] Rust unit tests in `src/parse.rs` for `validate_ddl_body()`, `detect_near_miss()`, position calculation

## Sources

### Primary (HIGH confidence)
- DuckDB amalgamation source (duckdb.hpp, duckdb.cpp) vendored at `cpp/include/` -- ParserExtensionParseResult struct, error_location field, DISPLAY_EXTENSION_ERROR handling in parser.cpp, ParserException::SyntaxError implementation
- Project source code (`src/parse.rs`, `src/expand.rs`, `src/query/error.rs`, `cpp/src/shim.cpp`) -- existing patterns, FFI conventions, error handling

### Secondary (MEDIUM confidence)
- [DuckDB parser.cpp source](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- verified via vendored amalgamation
- [DuckDB parser extension issue #18485](https://github.com/duckdb/duckdb/issues/18485) -- semicolon handling in parser extensions

### Tertiary (LOW confidence)
- None. All findings verified against vendored source code.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - strsim already in use, no new deps
- Architecture: HIGH - ParserExtensionParseResult.error_location verified in vendored amalgamation source (duckdb.hpp lines 32924-32942, duckdb.cpp lines 322047-322048, 56389-56392, 56679-56683)
- Pitfalls: HIGH - based on direct code reading of the error flow through C++ and Rust layers

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- DuckDB parser extension API unlikely to change within pinned version)
