# Phase 25: SQL Body Parser - Context

**Gathered:** 2026-03-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Parse `CREATE SEMANTIC VIEW name AS TABLES (...) RELATIONSHIPS (...) DIMENSIONS (...) METRICS (...)` keyword clause syntax into a `SemanticViewDefinition`. All 7 DDL verbs continue to work with the new body form. No expansion or join resolution — that is Phase 26 and 27. No backward compat with old `:=`/struct-literal body syntax (pre-release; old syntax removal is Phase 27).

</domain>

<decisions>
## Implementation Decisions

### `AS` keyword and body structure
- `AS` is required between the view name and the clause block
- No outer parentheses wrapping the whole body — clauses appear at top level after `AS`
- Example: `CREATE SEMANTIC VIEW myview AS TABLES (...) DIMENSIONS (...) METRICS (...)`

### Clause ordering
- Fixed order required: TABLES → RELATIONSHIPS (optional) → DIMENSIONS → METRICS
- TABLES is required (must appear even for single-table views)
- Both DIMENSIONS and METRICS are required (a view must have at least one of each)
- Unknown clause keyword → "did you mean X?" error with position (extends existing fuzzy-match system)

### RELATIONSHIPS clause
- Optional: may be omitted entirely for single-table views
- Empty clause `RELATIONSHIPS ()` is also valid (both omit and empty mean "no relationships")
- When present, every relationship entry must have a name: `order_to_customer AS o(customer_id) REFERENCES c`
- Relationship name is required (no nameless form)
- Phase 25 validates structure only (alias, columns, REFERENCES keyword); semantic validation (do aliases exist in TABLES?) is Phase 26

### DIMENSIONS and METRICS clauses
- `alias.name AS sql_expr` — the `alias.` prefix is **required** on every entry
- Expressions (after `AS`) are treated as opaque SQL strings — Phase 25 captures them verbatim, no expression parsing
- Entries separated by commas only (not newlines)
- Trailing commas are allowed after the last entry

### TABLES clause
- Each entry: `alias AS schema.table PRIMARY KEY (col1, col2)`
- Composite primary keys supported
- Schema-qualified table names (e.g., `main.orders`) are valid

### Error reporting
- Extends the existing `ParseError { message, position }` / caret system from `parse.rs`
- Clause-level detail: errors name which part failed (e.g., "Expected AS after alias in TABLES clause")
- Fail-fast: stop and report on first error (consistent with existing validator)
- Position reporting covers errors inside clause bodies (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS)

### Parser library
- Hand-written parser is the default approach (grammar is small and well-defined)
- **Open to a parser combinator library** (`winnow`, `nom`, `chumsky`) if it meaningfully simplifies error position tracking or reduces implementation complexity
- Researcher should evaluate trade-offs: compile time impact, error message quality, fit with existing byte-offset error model
- Decision delegated to researcher/planner; hand-written is the fallback

### 4096-byte C++ shim buffer
- Must be fixed in Phase 25 — real keyword bodies can easily exceed 4096 bytes
- Resolution strategy: researcher to identify best approach (dynamic allocation in C++ or alternative; possibly parse-then-JSON-encode in Rust so the rewritten SQL is compact regardless of body size)

### Claude's Discretion
- Exact recursive descent structure and module organization
- Whether the new keyword body parser lives in `parse.rs` or a new `src/body_parser.rs`
- JSON encoding strategy for passing parsed definition to `create_semantic_view()` function
- How `parse_create_body` is updated/replaced to support `AS` keyword path vs `(` path

</decisions>

<specifics>
## Specific Ideas

- Snowflake semantic view DDL is the grammar model — when in doubt, match Snowflake behavior
- The caret error system from v0.5.1 (clause-level hints + byte position + "did you mean" suggestions) should extend cleanly into clause body errors

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/parse.rs: detect_ddl_kind()` — already identifies all 7 DDL forms; no changes needed here
- `src/parse.rs: scan_clause_keywords()` — already recognizes `tables`, `relationships`, `dimensions`, `metrics` keyword names; can be replaced or extended by the new body parser
- `src/parse.rs: ParseError { message, position }` — existing error type with byte offset; new parser should produce the same type
- `src/parse.rs: suggest_clause_keyword()` — fuzzy clause keyword suggestion via `strsim`; reuse for "did you mean" errors
- `src/parse.rs: validate_brackets()` — balanced bracket validation respecting string literals; reuse inside clause body parsing
- `src/model.rs: SemanticViewDefinition, TableRef, Join, Dimension, Metric` — target types the parser produces; updated by Phase 24 to include PK/FK fields

### Established Patterns
- Error position = byte offset into the **original query string** (before any trimming); all existing parse errors follow this convention
- `strsim::levenshtein` for fuzzy "did you mean" suggestions; already a Cargo dep
- Case-insensitive keyword matching via `eq_ignore_ascii_case` on byte slices

### Integration Points
- `src/parse.rs: parse_create_body()` — currently expects `(` after view name; needs to detect `AS` keyword path for new syntax
- `src/parse.rs: rewrite_ddl()` — calls `parse_create_body` for CREATE forms; output for new syntax must fit in C++ shim buffer (see Buffer concern above)
- `cpp/src/shim.cpp: sv_ddl_bind()` — `char sql_buf[4096]` at line 139 is the buffer that must be expanded or the rewrite strategy changed
- `src/ddl/define.rs` — `create_semantic_view()` function that receives the parsed body; Phase 25 must ensure the parsed definition reaches it correctly

### Buffer Concern (flagged in STATE.md)
- `cpp/src/shim.cpp` lines 70 and 139 both have `char sql_buf[4096]`
- Line 70 is in `sv_parse_stub` (validation path) — uses the buffer but doesn't execute; less critical
- Line 139 is in `sv_ddl_bind` (execution path) — the rewritten SQL here must fit; this is the one that matters
- With keyword bodies containing many dimensions/metrics, the rewritten SQL can easily be 5–20+ KB
- Researcher should evaluate: (a) dynamic `std::string` in C++, (b) parse-to-JSON in Rust so rewritten SQL is compact, (c) pass original DDL text and parse in `define.rs` instead of rewriting to function-call SQL

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 25-sql-body-parser*
*Context gathered: 2026-03-11*
