# Phase 20: Extended DDL Statements - Research

**Researched:** 2026-03-09
**Domain:** DuckDB parser hook extension -- multi-prefix detection and statement rewriting
**Confidence:** HIGH

## Summary

Phase 20 extends the existing parser hook infrastructure (v0.5.0) from handling one DDL prefix (`CREATE SEMANTIC VIEW`) to handling seven. All six new DDL prefixes have been empirically validated in Phase 19 to trigger the parser fallback hook. Every target function (`create_or_replace_semantic_view`, `create_semantic_view_if_not_exists`, `drop_semantic_view`, `drop_semantic_view_if_exists`, `describe_semantic_view`, `list_semantic_views`) is already registered and working via function-based syntax. No new function implementations are needed.

The implementation requires changes to exactly three functions in `src/parse.rs` (detection, parsing, rewriting) and one error message string in `cpp/src/shim.cpp`. The C++ parser hook registration, plan function, and DDL execution infrastructure remain unchanged. The architecture is a direct extension of the proven v0.5.0 pattern: detect prefix, rewrite to function call, execute on `sv_ddl_conn`.

**Primary recommendation:** Extend the Rust detection and rewrite functions to handle all 7 prefixes with longest-prefix-first ordering. No changes to C++ hook registration, no new functions, no new connections.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DDL-03 | `DROP SEMANTIC VIEW name` removes the view | Rewrite to `SELECT * FROM drop_semantic_view('name')` -- function already registered and tested in phase2_ddl.test |
| DDL-04 | `DROP SEMANTIC VIEW IF EXISTS name` is idempotent | Rewrite to `SELECT * FROM drop_semantic_view_if_exists('name')` -- function already registered and tested in phase2_ddl.test |
| DDL-05 | `CREATE OR REPLACE SEMANTIC VIEW name (...)` updates in place | Rewrite to `SELECT * FROM create_or_replace_semantic_view('name', ...)` -- function already registered and tested in phase2_ddl.test |
| DDL-06 | `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` is idempotent | Rewrite to `SELECT * FROM create_semantic_view_if_not_exists('name', ...)` -- function already registered and tested in phase2_ddl.test |
| DDL-07 | `DESCRIBE SEMANTIC VIEW name` shows dimensions/metrics/types | Rewrite to `SELECT * FROM describe_semantic_view('name')` -- function already registered and tested in phase2_ddl.test |
| DDL-08 | `SHOW SEMANTIC VIEWS` lists all views | Rewrite to `SELECT * FROM list_semantic_views()` -- function already registered and tested in phase2_ddl.test |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust (parse.rs) | -- | Prefix detection + DDL-to-function rewriting | Existing pattern from v0.5.0; all logic lives here |
| C++ shim.cpp | -- | Parser hook registration + DDL bind/execute | Unchanged from v0.5.0; passes raw query to Rust FFI |
| libduckdb-sys | 1.4.4 | DuckDB C API for duckdb_query on sv_ddl_conn | Already vendored; no version change |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| sqllogictest | -- | Integration tests via `just test-sql` | All DDL statements must be tested via sqllogictest |
| cargo test | -- | Rust unit tests for detection/rewrite | Pure function tests without extension loading |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Multi-prefix string matching | sqlparser-rs / nom / pest | Massive overkill -- 7 fixed prefixes need only ASCII prefix comparison; framework adds ~500KB |
| Enum-based DDL type dispatch | String-based prefix matching | Enum is cleaner but the current code uses u8 constants; introducing an enum is optional refactoring |

## Architecture Patterns

### Current Architecture (v0.5.0 -- unchanged by Phase 20)

```
DuckDB Parser fails on "... SEMANTIC ..."
       |
       v
[C++ sv_parse_stub] -- calls sv_parse_rust(query) via FFI
       |
       v
[Rust detect_*] -- returns PARSE_DETECTED or PARSE_NOT_OURS
       |
       v (PARSE_DETECTED)
[C++ sv_plan_function] -- wraps query in sv_ddl_internal TableFunction
       |
       v
[C++ sv_ddl_bind] -- calls sv_execute_ddl_rust(query, sv_ddl_conn) via FFI
       |
       v
[Rust rewrite_*] -- rewrites DDL to "SELECT * FROM target_function(...)"
       |
       v
[duckdb_query on sv_ddl_conn] -- executes the rewritten SQL
```

### What Changes in Phase 20

Only three Rust functions and one C++ error string need modification:

| File | Function | Change |
|------|----------|--------|
| `src/parse.rs` | `detect_create_semantic_view` | Rename to `detect_semantic_view_ddl`; add 6 new prefix checks with longest-first ordering |
| `src/parse.rs` | `parse_ddl_text` | Extend to parse all 7 DDL forms (not just `CREATE SEMANTIC VIEW`) |
| `src/parse.rs` | `rewrite_ddl_to_function_call` | Extend to route each DDL form to its corresponding function call |
| `cpp/src/shim.cpp` | `sv_ddl_bind` error message | Change `"CREATE SEMANTIC VIEW failed"` to `"Semantic view DDL failed"` (cosmetic) |

The C++ hook registration, parse data structures, plan function, DDL execution flow, and connection management are all unchanged.

### Pattern: Multi-Prefix Detection (Longest-First)

The detection function must check prefixes in this exact order to avoid substring overlap:

```
Priority 1 (longest CREATE variants first):
  1. "create or replace semantic view"   (31 chars)
  2. "create semantic view if not exists" (34 chars)
  3. "create semantic view"              (20 chars)

Priority 2 (longest DROP variant first):
  4. "drop semantic view if exists"      (28 chars)
  5. "drop semantic view"                (18 chars)

Priority 3 (read-only, no overlap):
  6. "describe semantic view"            (22 chars)
  7. "show semantic views"               (19 chars)
```

Note: Prefixes 1 and 2 do NOT overlap with each other ("create or replace" vs "create semantic view if not exists" diverge at the 8th character). They both overlap with prefix 3 ("create semantic view"). Similarly, prefix 4 overlaps with prefix 5. Prefixes 6 and 7 have no overlap with anything.

### Pattern: DDL Form to Function Call Mapping

| DDL Form | Prefix to Strip | Extract | Rewrite Target |
|----------|----------------|---------|----------------|
| `CREATE SEMANTIC VIEW x (...)` | `create semantic view` | name + body | `SELECT * FROM create_semantic_view('x', body)` |
| `CREATE OR REPLACE SEMANTIC VIEW x (...)` | `create or replace semantic view` | name + body | `SELECT * FROM create_or_replace_semantic_view('x', body)` |
| `CREATE SEMANTIC VIEW IF NOT EXISTS x (...)` | `create semantic view if not exists` | name + body | `SELECT * FROM create_semantic_view_if_not_exists('x', body)` |
| `DROP SEMANTIC VIEW x` | `drop semantic view` | name | `SELECT * FROM drop_semantic_view('x')` |
| `DROP SEMANTIC VIEW IF EXISTS x` | `drop semantic view if exists` | name | `SELECT * FROM drop_semantic_view_if_exists('x')` |
| `DESCRIBE SEMANTIC VIEW x` | `describe semantic view` | name | `SELECT * FROM describe_semantic_view('x')` |
| `SHOW SEMANTIC VIEWS` | `show semantic views` | (nothing) | `SELECT * FROM list_semantic_views()` |

### Pattern: DDL Categories

The 7 DDL forms fall into three parsing categories:

1. **CREATE-with-body** (3 forms): Strip prefix, extract name, extract parenthesized body, rewrite to `function('name', body)`
2. **Name-only** (3 forms: DROP, DROP IF EXISTS, DESCRIBE): Strip prefix, extract name, rewrite to `function('name')`
3. **No-args** (1 form: SHOW): Strip prefix, rewrite to `function()`

### Recommended Refactoring Approach

Replace the three current functions with a cleaner enum-based dispatch:

```rust
enum DdlKind {
    Create,              // "create semantic view"
    CreateOrReplace,     // "create or replace semantic view"
    CreateIfNotExists,   // "create semantic view if not exists"
    Drop,                // "drop semantic view"
    DropIfExists,        // "drop semantic view if exists"
    Describe,            // "describe semantic view"
    Show,                // "show semantic views"
}

fn detect_ddl_kind(query: &str) -> Option<DdlKind> { ... }
fn rewrite_ddl(query: &str, kind: DdlKind) -> Result<String, String> { ... }
```

The FFI boundary stays simple: `sv_parse_rust` calls `detect_ddl_kind` and returns `PARSE_DETECTED` or `PARSE_NOT_OURS`. The `sv_execute_ddl_rust` function calls `rewrite_ddl` (which internally calls `detect_ddl_kind` again to determine the rewrite target).

### Anti-Patterns to Avoid

- **Regex-based detection:** Regex is overkill and introduces allocation for what is pure ASCII prefix comparison. Use byte-slice `eq_ignore_ascii_case` as the current code does.
- **Modifying the C++ hook registration:** The same `sv_parse_stub` -> `sv_plan_function` -> `sv_ddl_bind` path works for all DDL forms. Do not add additional parser extensions or plan functions.
- **Creating new connections:** All DDL forms execute on the existing `sv_ddl_conn`. DROP/DESCRIBE/SHOW do not need special connection handling.
- **Parsing inside shim.cpp:** All parsing stays in Rust. The C++ side only passes the raw query string through.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DROP logic | New catalog deletion code | Existing `drop_semantic_view()` function via rewrite | Proven implementation handles persist_conn, catalog delete, error handling |
| DESCRIBE logic | Custom catalog read + format | Existing `describe_semantic_view()` function via rewrite | Already outputs 6 VARCHAR columns with JSON fields |
| LIST logic | Custom catalog iteration | Existing `list_semantic_views()` function via rewrite | Already outputs sorted (name, base_table) rows |
| CREATE OR REPLACE logic | Custom upsert path | Existing `create_or_replace_semantic_view()` function via rewrite | DefineState with `or_replace: true` handles persistence and catalog upsert |
| IF NOT EXISTS logic | Custom existence check | Existing `create_semantic_view_if_not_exists()` function via rewrite | DefineState with `if_not_exists: true` handles silent no-op |

**Key insight:** ALL business logic already exists in the registered table functions. Phase 20 is purely a syntactic translation layer -- detect the DDL form, rewrite to the corresponding function call, execute. Zero new business logic needed.

## Common Pitfalls

### Pitfall 1: Prefix Overlap (CREATE SEMANTIC VIEW vs IF NOT EXISTS / OR REPLACE)
**What goes wrong:** The current `detect_create_semantic_view` matches "create semantic view" which is a substring of both "create or replace semantic view" and "create semantic view if not exists". If checked first, the shorter prefix matches incorrectly.
**Why it happens:** Simple prefix matching without length ordering.
**How to avoid:** Check longer prefixes before shorter ones. The detection function must try "create or replace semantic view" and "create semantic view if not exists" BEFORE "create semantic view". Similarly, "drop semantic view if exists" must be checked BEFORE "drop semantic view".
**Warning signs:** A view named "IF" or "OR" appears in the catalog.
**Empirically confirmed:** Phase 19 spike demonstrated this exact bug -- `CREATE SEMANTIC VIEW IF NOT EXISTS test_view (...)` created a view named "IF".

### Pitfall 2: DESCRIBE SEMANTIC vs DESCRIBE SEMANTIC VIEW
**What goes wrong:** `DESCRIBE SEMANTIC` is a valid DuckDB statement (describes a table named "semantic"). If the detection only checks the two-word prefix "describe semantic", it intercepts a valid DuckDB statement.
**Why it happens:** Not requiring the full three-word prefix.
**How to avoid:** The detection function must require "describe semantic view" (3 words). Similarly, "show semantic views" must require "views" (plural), not just "show semantic".
**Warning signs:** `DESCRIBE SEMANTIC` (without VIEW) fails with a semantic views error instead of DuckDB's normal table-not-found message.

### Pitfall 3: Three-Connection Lock Conflict During DROP
**What goes wrong:** The DDL path uses sv_ddl_conn, which executes `SELECT * FROM drop_semantic_view('x')`, which internally uses persist_conn for the catalog table DELETE. If there is a lock conflict between sv_ddl_conn and persist_conn, the DROP hangs or errors.
**Why it happens:** DuckDB uses per-connection context locks. If two connections attempt conflicting writes, one blocks.
**How to avoid:** The connections are used sequentially: sv_ddl_conn executes the function call, which calls `drop_semantic_view` bind, which uses persist_conn for the DELETE. The bind completes before sv_ddl_conn's execution continues. This is the same sequential pattern proven for CREATE in v0.5.0. Test empirically early in Phase 20 as a smoke test.
**Warning signs:** DROP SEMANTIC VIEW hangs indefinitely or returns a lock-related error.
**Risk level:** LOW -- the same sequential connection pattern works for CREATE. But test it.

### Pitfall 4: sv_ddl_bind Error Message Hardcodes "CREATE"
**What goes wrong:** The C++ `sv_ddl_bind` function throws `BinderException("CREATE SEMANTIC VIEW failed: %s", error_buf)` on any DDL error. For DROP, DESCRIBE, and SHOW operations, this message is confusing/incorrect.
**Why it happens:** The error message was written when only CREATE was supported.
**How to avoid:** Update the error message to be generic: `"Semantic view DDL failed: %s"` or extract the DDL verb from the query.

### Pitfall 5: sv_execute_ddl_rust Returns Name but DROP/DESCRIBE/SHOW Don't Have "Name" Semantics
**What goes wrong:** The current `sv_execute_ddl_rust` extracts a view name via `parse_ddl_text` and writes it to `name_out`. For SHOW SEMANTIC VIEWS (no name parameter), this will fail.
**Why it happens:** The FFI contract was designed for CREATE only.
**How to avoid:** Two options: (a) change the rewrite function to return a generic success message instead of a view name, or (b) make the name extraction DDL-form-aware (return the view name for CREATE/DROP/DESCRIBE, return empty/placeholder for SHOW). The C++ side already handles the name_out buffer gracefully (it displays whatever string is returned). Either approach works; option (a) is simpler.

### Pitfall 6: SHOW SEMANTIC VIEWS Has No View Name to Extract
**What goes wrong:** All other DDL forms have a view name after the prefix. `SHOW SEMANTIC VIEWS` does not -- the entire statement is just the prefix itself. The current `parse_ddl_text` would fail trying to extract a name.
**How to avoid:** The rewrite function must handle SHOW as a special case: no name extraction, rewrite directly to `SELECT * FROM list_semantic_views()`.

### Pitfall 7: Case Sensitivity in View Names
**What goes wrong:** DuckDB uppercases unquoted identifiers. If a user writes `DROP SEMANTIC VIEW MyView`, the name "MyView" should be matched case-sensitively against the catalog (which stores exact names). The parser hook receives the raw text, preserving case.
**Why it happens:** The function-based path handles this correctly (names are always passed as quoted strings). The DDL rewrite must do the same -- wrap the extracted name in single quotes.
**How to avoid:** All rewrite targets already use `'name'` (single-quoted string literal), which preserves case in DuckDB. The existing `safe_name.replace('\'', "''")` pattern handles quote escaping. Continue using this pattern.

## Code Examples

### Current Detection (to be replaced)
```rust
// Source: src/parse.rs (v0.5.0)
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

### Target: Multi-Prefix Detection
```rust
// Pseudocode for Phase 20 (not final)
fn detect_semantic_view_ddl(query: &str) -> u8 {
    let trimmed = query.trim().trim_end_matches(';').trim();
    // Check each prefix in longest-first order for each family
    let prefixes = [
        "create or replace semantic view",
        "create semantic view if not exists",
        "create semantic view",
        "drop semantic view if exists",
        "drop semantic view",
        "describe semantic view",
        "show semantic views",
    ];
    for prefix in &prefixes {
        if trimmed.len() >= prefix.len()
            && trimmed.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
        {
            return PARSE_DETECTED;
        }
    }
    PARSE_NOT_OURS
}
```

### Target: Multi-Form Rewrite
```rust
// Pseudocode for Phase 20 (not final)
fn rewrite_ddl_to_function_call(query: &str) -> Result<String, String> {
    let trimmed = query.trim().trim_end_matches(';').trim();

    // Determine DDL kind (longest-first matching)
    if starts_with_ci(trimmed, "create or replace semantic view") {
        let (name, body) = extract_name_and_body(trimmed, "create or replace semantic view")?;
        Ok(format!("SELECT * FROM create_or_replace_semantic_view('{name}', {body})"))
    } else if starts_with_ci(trimmed, "create semantic view if not exists") {
        let (name, body) = extract_name_and_body(trimmed, "create semantic view if not exists")?;
        Ok(format!("SELECT * FROM create_semantic_view_if_not_exists('{name}', {body})"))
    } else if starts_with_ci(trimmed, "create semantic view") {
        let (name, body) = extract_name_and_body(trimmed, "create semantic view")?;
        Ok(format!("SELECT * FROM create_semantic_view('{name}', {body})"))
    } else if starts_with_ci(trimmed, "drop semantic view if exists") {
        let name = extract_name_only(trimmed, "drop semantic view if exists")?;
        Ok(format!("SELECT * FROM drop_semantic_view_if_exists('{name}')"))
    } else if starts_with_ci(trimmed, "drop semantic view") {
        let name = extract_name_only(trimmed, "drop semantic view")?;
        Ok(format!("SELECT * FROM drop_semantic_view('{name}')"))
    } else if starts_with_ci(trimmed, "describe semantic view") {
        let name = extract_name_only(trimmed, "describe semantic view")?;
        Ok(format!("SELECT * FROM describe_semantic_view('{name}')"))
    } else if starts_with_ci(trimmed, "show semantic views") {
        Ok("SELECT * FROM list_semantic_views()".to_string())
    } else {
        Err("Not a semantic view DDL statement".to_string())
    }
}
```

### Target: sv_execute_ddl_rust Adaptation
```rust
// The key change: sv_execute_ddl_rust must handle DDL forms that don't extract a view name.
// For SHOW SEMANTIC VIEWS, there is no name to return.
// Simplest fix: return a descriptive string for name_out when no name is applicable.

// Current: parse_ddl_text(query) -> (name, _body) -> write name to name_out
// New: rewrite_ddl returns the SQL; name extraction is DDL-form-aware

// Option A: Return "ok" for name_out when the DDL form has no name
// Option B: Extract name from the rewrite function as a second return value
```

### Function Call Signatures (already registered, from lib.rs)
```rust
// All target functions are already registered at extension init time:
// create_semantic_view('name', tables := [...], dimensions := [...], metrics := [...])
// create_or_replace_semantic_view('name', tables := [...], dimensions := [...], metrics := [...])
// create_semantic_view_if_not_exists('name', tables := [...], dimensions := [...], metrics := [...])
// drop_semantic_view('name')
// drop_semantic_view_if_exists('name')
// describe_semantic_view('name')
// list_semantic_views()
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Function-only DDL | Native DDL (CREATE only, v0.5.0) | 2026-03-08 | Users write `CREATE SEMANTIC VIEW` instead of `FROM create_semantic_view(...)` |
| Single-prefix detection | Multi-prefix detection (Phase 20) | Planned | All DDL verbs get native syntax |
| `detect_create_semantic_view` | `detect_semantic_view_ddl` (Phase 20) | Planned | Handles 7 prefixes instead of 1 |

**Reference:** DuckPGQ extension uses the same parser hook pattern for multiple DDL statements: `CREATE PROPERTY GRAPH`, `DROP PROPERTY GRAPH`, `CREATE OR REPLACE PROPERTY GRAPH`. This validates the multi-prefix approach in production.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test + sqllogictest + DuckLake CI + vtab crash tests |
| Config file | justfile (task runner) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DDL-03 | `DROP SEMANTIC VIEW x` removes view from catalog | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| DDL-04 | `DROP SEMANTIC VIEW IF EXISTS x` silent on missing | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| DDL-05 | `CREATE OR REPLACE SEMANTIC VIEW x (...)` updates in place | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| DDL-06 | `CREATE SEMANTIC VIEW IF NOT EXISTS x (...)` silent on duplicate | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| DDL-07 | `DESCRIBE SEMANTIC VIEW x` shows dimensions/metrics | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| DDL-08 | `SHOW SEMANTIC VIEWS` lists all views | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| -- | Multi-prefix detection (unit) | unit (cargo test) | `cargo test` | No -- Wave 0 |
| -- | DDL rewrite correctness (unit) | unit (cargo test) | `cargo test` | No -- Wave 0 |
| -- | Case insensitivity for all DDL forms | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |
| -- | Normal SQL passthrough unaffected | integration (sqllogictest) | `just test-sql` | Partial -- phase16_parser.test has this |
| -- | Function-based DDL still works | integration (sqllogictest) | `just test-sql` | Yes -- phase2_ddl.test |
| -- | Three-connection lock (DROP smoke) | integration (sqllogictest) | `just test-sql` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase20_extended_ddl.test` -- sqllogictest covering DDL-03 through DDL-08 (all 6 requirements)
- [ ] Unit tests for `detect_semantic_view_ddl` -- all 7 prefixes, case variations, negative cases
- [ ] Unit tests for rewrite functions -- all 7 DDL forms, including SHOW (no-name case)
- No framework install needed -- test infrastructure exists

## Open Questions

1. **sv_execute_ddl_rust name_out semantics for SHOW SEMANTIC VIEWS**
   - What we know: The current FFI contract writes a view name to `name_out`. SHOW has no view name.
   - What's unclear: Whether the C++ side (sv_ddl_bind) relies on the name for anything other than the single-row result.
   - Recommendation: Check `sv_ddl_bind` -- it creates `SvDdlBindData(string(name_buf))` which becomes the single result row. For SHOW, the result should be the full table from `list_semantic_views()`, not a single name. This may require the rewrite path to change: instead of executing on sv_ddl_conn and returning a name, execute on sv_ddl_conn and forward the full result set. **Alternative: SHOW and DESCRIBE could return their full result sets by having sv_ddl_bind use a different output schema.** However, the simplest approach is: the current sv_ddl_internal TableFunction always returns a single VARCHAR "view_name" column. For SHOW/DESCRIBE, this is insufficient. The cleanest solution is to have `sv_execute_ddl_rust` just succeed/fail, and for the result the user sees, have it come from the rewritten SQL executed on sv_ddl_conn. But wait -- the current architecture returns the view_name from sv_ddl_internal, not the rewritten SQL's result. This means DESCRIBE and SHOW need a different approach.
   - **Resolution path:** The rewritten SQL `SELECT * FROM describe_semantic_view('x')` executes on sv_ddl_conn and its results are consumed and discarded by `sv_execute_ddl_rust`. The user only sees the sv_ddl_internal output (one row: "view_name" VARCHAR). For DESCRIBE and SHOW, the user needs to see the full result set from the function, not just a name. Two solutions:
     - **(A) Pass-through result:** Modify sv_ddl_bind to dynamically set output columns based on the DDL form. This is complex.
     - **(B) Execute and re-query:** Have sv_execute_ddl_rust execute the rewritten SQL on sv_ddl_conn, then have sv_ddl_bind execute the SAME rewritten SQL again on sv_ddl_conn and return those results. Wasteful.
     - **(C) Return rewritten SQL as result:** Instead of executing the rewritten SQL in `sv_execute_ddl_rust`, just return the rewritten SQL string. Then `sv_ddl_bind` in C++ executes it on sv_ddl_conn and captures the result. This requires C++ changes to forward arbitrary result sets.
     - **(D) Simplest: Return OK string for mutating DDL; for read DDL, execute and return result as single VARCHAR.** For DESCRIBE: execute on sv_ddl_conn, format the result as a single string, return it. For SHOW: same. This loses column structure but is trivially implementable.
     - **(E) Different code path for read vs write DDL:** The parse function could return different parse data types, routing to different plan functions. This is cleaner but more C++ changes.
     - **(F) Execute the rewritten SQL in C++ sv_ddl_bind directly, using DuckDB's internal execution APIs.** The C++ bind callback has access to the ClientContext and can execute queries via DuckDB's C++ API (not the C API). This would allow forwarding the full result schema.
     - **Recommended: Approach F or a simpler variant.** Refactor sv_ddl_bind to: (1) call Rust to get the rewritten SQL string (not execute it), (2) execute the rewritten SQL on sv_ddl_conn in C++ using `duckdb_query`, (3) read the result columns/types, (4) declare matching output columns on the sv_ddl_internal TableFunction, (5) store the result data for emission in sv_ddl_execute. This makes sv_ddl_internal a generic "execute SQL and forward results" function. It works for all 7 DDL forms. The C++ change is moderate but contained to sv_ddl_bind and sv_ddl_execute. Rust changes are minimal: add a new FFI function `sv_rewrite_ddl_rust` that returns the rewritten SQL without executing it. **This is the cleanest approach that handles all DDL forms uniformly.**

2. **Backward compatibility of existing phase16_parser.test and phase2_ddl.test**
   - What we know: The function-based DDL (`FROM create_semantic_view(...)`) must continue to work alongside native DDL.
   - What's unclear: Nothing -- the parser hook only intercepts statements that DuckDB's parser fails on. Function calls parse successfully and never reach the hook.
   - Recommendation: Existing tests will continue to pass without modification. Confirmed by architecture analysis.

## Sources

### Primary (HIGH confidence)
- `src/parse.rs` -- current detection, parsing, and rewrite implementation (lines 1-430)
- `cpp/src/shim.cpp` -- current C++ parser hook registration, plan function, DDL bind/execute (lines 1-201)
- `src/lib.rs` -- function registration for all 7 target functions (lines 347-413)
- `src/ddl/drop.rs` -- drop_semantic_view and drop_semantic_view_if_exists VTab implementation
- `src/ddl/describe.rs` -- describe_semantic_view VTab implementation (6 VARCHAR columns)
- `src/ddl/list.rs` -- list_semantic_views VTab implementation (name, base_table columns)
- `src/ddl/define.rs` -- create_semantic_view with or_replace and if_not_exists flags
- `.planning/phases/19-parser-hook-validation-spike/19-SPIKE-RESULTS.md` -- empirical validation of all 7 prefixes
- `test/sql/phase2_ddl.test` -- existing function-based DDL integration tests
- `test/sql/phase16_parser.test` -- existing native DDL integration tests

### Secondary (MEDIUM confidence)
- `.planning/phases/19-parser-hook-validation-spike/19-RESEARCH.md` -- parser fallback mechanism analysis from vendored source
- DuckPGQ extension -- validates multi-DDL parser hook pattern in production

### Tertiary (LOW confidence)
- None -- all findings verified from primary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all components are existing, proven code; no new libraries needed
- Architecture: HIGH -- direct extension of v0.5.0 pattern; all functions already exist and are tested
- Pitfalls: HIGH -- prefix overlap confirmed empirically in Phase 19; connection patterns proven in v0.5.0
- Open question (DESCRIBE/SHOW result forwarding): MEDIUM -- requires design decision on how sv_ddl_internal forwards multi-column results; multiple viable approaches identified

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- extending proven internal architecture)
