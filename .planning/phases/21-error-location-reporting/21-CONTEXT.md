# Phase 21: Error Location Reporting - Context

**Gathered:** 2026-03-09
**Status:** Ready for planning

<domain>
## Phase Boundary

Users get actionable, positioned error messages when DDL statements are malformed. Covers clause-level hints (ERR-01), character position for DuckDB caret rendering (ERR-02), and "did you mean" fuzzy suggestions (ERR-03). Applies to all 7 DDL forms parsed in `src/parse.rs`.

</domain>

<decisions>
## Implementation Decisions

### Error message style
- Identify the problem AND state what was expected (e.g., "Error in DIMENSIONS clause: expected list of STRUCT definitions, got empty value")
- No recovery hints (no "Run DESCRIBE..." suggestions)
- No prefix -- error messages stand alone, matching DuckDB's native error feel
- Plain error text only -- DuckDB's renderer handles caret/position display

### "Did you mean" scope
- Near-miss DDL prefixes: detect close matches like "CREAT SEMANTIC VIEW" or "DROP SEMANTC VIEW" and suggest the correct form (reuse strsim crate already in deps)
- Clause keyword typos: suggest corrections for misspelled clause keywords inside CREATE body (fixed set: tables, relationships, dimensions, metrics)
- View names on DROP/DESCRIBE: reuse existing `suggest_closest()` from `expand.rs` to suggest view names when the specified view doesn't exist
- NOT struct field names within clauses -- DuckDB's STRUCT type validation handles these

### Common mistake patterns
- Missing required clauses: detect when `tables` or `dimensions`/`metrics` clause is absent from CREATE body and give specific error
- Bracket/paren mismatch: detect unbalanced brackets/parens and point at the position
- Empty body: `CREATE SEMANTIC VIEW x ()` gets a specific error stating required clauses
- No special handling for SQL-style confusions (e.g., `AS SELECT`) -- DuckDB's own error is sufficient

### Caret position strategy
- Clause errors: caret points at the start of the clause keyword
- Structural errors (missing name, missing parens): caret points at end of prefix (where the expected token should be)
- Fallback: if DuckDB's error renderer doesn't support position through the parser hook error path, include "at position N" in the error text

### Claude's Discretion
- Internal error struct design (ParseError type, position encoding)
- Whether to parse clause boundaries with simple string scanning or a more structured approach
- Exact fuzzy match threshold for DDL prefix near-misses (existing code uses edit distance <= 3)
- Test strategy and error message wording details

</decisions>

<specifics>
## Specific Ideas

No specific requirements -- open to standard approaches matching DuckDB error conventions.

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `expand::suggest_closest()`: Levenshtein fuzzy matching (strsim crate, threshold <= 3 edits) -- reuse for DDL prefix near-misses, clause keywords, and view names on DROP/DESCRIBE
- `strsim` crate: already a dependency, no new deps needed
- `query/error.rs` `QueryError::ViewNotFound`: pattern for "Did you mean" error formatting -- follow same style

### Established Patterns
- Error strings returned via `Result<T, String>` in parse.rs -- current error path
- FFI errors written to `error_out` buffer via `write_to_buffer()` in parse.rs
- C++ side receives error string and raises DuckDB error via `SetParserError` / function error

### Integration Points
- `parse.rs` `rewrite_ddl()` and `parse_create_body()` -- primary parse functions that return `Err(String)` on malformed input
- `sv_rewrite_ddl_rust()` FFI entry point -- error path writes to `error_out` buffer
- C++ `sv_ddl_bind` / `sv_ddl_plan` -- receives error string from Rust, raises to DuckDB
- Catalog lookups needed for "did you mean" view name suggestions on DROP/DESCRIBE -- requires catalog access from parse path

</code_context>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 21-error-location-reporting*
*Context gathered: 2026-03-09*
