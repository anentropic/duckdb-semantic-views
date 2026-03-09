# Technology Stack: v0.5.1 DDL Polish

**Project:** DuckDB Semantic Views Extension
**Researched:** 2026-03-08
**Milestone:** v0.5.1 -- DDL Polish (DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW + error reporting + README)
**Scope:** What library/crate additions or changes are needed for v0.5.1 features

---

## Bottom Line Up Front

**Zero new Cargo dependencies.** The existing stack is sufficient for all v0.5.1 features. The new DDL verbs (DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW) are parser detection + statement rewriting in Rust and routing in the C++ shim -- the same pattern as CREATE. Error location reporting with clause hints, character positions, and "did you mean" suggestions is achievable with `strsim` (already present) and hand-crafted error formatting that follows DuckDB's own error message conventions. No error reporting framework (miette, ariadne, codespan-reporting) is needed or appropriate because errors flow through DuckDB's plain-text error channel, not a terminal renderer.

---

## Existing Dependency Inventory (v0.5.0 Cargo.toml)

All dependencies are current and sufficient:

| Crate | Version | Sufficient For v0.5.1 | Why |
|-------|---------|----------------------|-----|
| `duckdb` | `=1.4.4` | Yes | VTab, BindInfo, TableFunctionInfo -- all DDL functions use these |
| `libduckdb-sys` | `=1.4.4` | Yes | Raw FFI (duckdb_query, duckdb_connection) for DDL execution path |
| `serde` + `serde_json` | `1` | Yes | Catalog JSON serialization/deserialization unchanged |
| `strsim` | `0.11` | Yes | "Did you mean" suggestions via Levenshtein distance -- already used in `expand.rs::suggest_closest()` |
| `cc` | `1` (build-dep, optional) | Yes | C++ shim compilation unchanged |
| `proptest` | `1.9` (dev-dep) | Yes | PBTs for new parse detection and error formatting |
| `cargo-husky` | `1` (dev-dep) | Yes | Pre-commit hooks unchanged |

### strsim 0.11 Verification

**Latest version:** 0.11.1 (released 2024-04-02). No newer release exists. The project uses `strsim = "0.11"` which resolves to 0.11.1. The `levenshtein()` function used by `suggest_closest()` in `expand.rs` is sufficient for DDL error suggestions -- no additional string similarity algorithms needed.

**Confidence:** HIGH -- verified via crates.io version listing.

---

## Feature-by-Feature Stack Analysis

### 1. Extended DDL Verbs (DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW)

**What exists:** The function-based implementations are already complete:
- `drop_semantic_view()` and `drop_semantic_view_if_exists()` -- `src/ddl/drop.rs`
- `create_or_replace_semantic_view()` and `create_semantic_view_if_not_exists()` -- `src/ddl/define.rs` via `DefineState.or_replace` / `DefineState.if_not_exists`
- `describe_semantic_view()` -- `src/ddl/describe.rs`
- `list_semantic_views()` -- `src/ddl/list.rs`

**What v0.5.1 adds:** Native DDL syntax support via the parser hook -- the same pattern used for `CREATE SEMANTIC VIEW` in v0.5.0:

| Native DDL | Rewrites To | Existing Function |
|------------|-------------|-------------------|
| `DROP SEMANTIC VIEW name` | `SELECT * FROM drop_semantic_view('name')` | `drop_semantic_view` |
| `DROP SEMANTIC VIEW IF EXISTS name` | `SELECT * FROM drop_semantic_view_if_exists('name')` | `drop_semantic_view_if_exists` |
| `CREATE OR REPLACE SEMANTIC VIEW name (...)` | `SELECT * FROM create_or_replace_semantic_view('name', ...)` | `create_or_replace_semantic_view` |
| `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` | `SELECT * FROM create_semantic_view_if_not_exists('name', ...)` | `create_semantic_view_if_not_exists` |
| `DESCRIBE SEMANTIC VIEW name` | `SELECT * FROM describe_semantic_view('name')` | `describe_semantic_view` |
| `SHOW SEMANTIC VIEWS` | `SELECT * FROM list_semantic_views()` | `list_semantic_views` |

**Stack requirement:** None new. Changes needed:
1. **`src/parse.rs`**: Extend `detect_create_semantic_view()` to handle all 6 DDL patterns. Rename to something broader (e.g., `detect_semantic_view_ddl()`). Add `parse_ddl_text()` variants or a unified parser that returns a DDL variant enum.
2. **`cpp/src/shim.cpp`**: The `sv_parse_stub` calls `sv_parse_rust` which returns a u8. Extend to return the DDL type (or return a detected flag and let `sv_execute_ddl_rust` handle routing). The `sv_ddl_bind` function calls `sv_execute_ddl_rust` which rewrites and executes -- extend `rewrite_ddl_to_function_call()` in Rust to handle all 6 patterns.
3. **No new C++ symbols needed** -- all rewriting is in Rust. The C++ shim just routes the query text through the same `sv_parse_rust` / `sv_execute_ddl_rust` path.

**Confidence:** HIGH -- direct code inspection confirms the pattern. The v0.5.0 architecture was explicitly designed for extensibility.

### 2. Error Location Reporting

**The requirement:** When a `CREATE SEMANTIC VIEW` statement has errors (missing clauses, unknown keywords, malformed syntax), report:
- Which clause the error is in (e.g., "in `dimensions` clause")
- Character position within the original DDL
- "Did you mean" suggestions for misspelled clause names

**Why NOT miette/ariadne/codespan-reporting:**

These are terminal diagnostic renderers. They produce ANSI-colored, multi-line error output with source spans, underlines, and margin annotations. DuckDB's error channel is plain text -- errors from `BinderException` or table function `Err(...)` are rendered by DuckDB's own error formatter. Fancy terminal rendering would:
1. **Be mangled** -- DuckDB strips/wraps error text; ANSI codes would appear as garbage in many clients (DBeaver, Python, JDBC).
2. **Conflict** -- DuckDB has its own `LINE 1:` / caret error format convention. Adding a competing format confuses users.
3. **Be overkill** -- The DDL grammar is ~6 keywords with simple structure. A full diagnostic framework for a handful of error cases wastes binary size (~200KB for miette).
4. **Add dependency churn** -- miette is at 7.6.0 with frequent breaking changes; ariadne is less stable. Neither is needed.

**What to do instead:** Follow DuckDB's own error conventions:

```
CREATE SEMANTIC VIEW failed: missing 'dimensions' clause.
  Hint: Expected one of: tables, relationships, dimensions, metrics
  Did you mean 'dimesions'? (found 'dimesions' at position 45)
```

This matches DuckDB's style:
- Error type prefix (`CREATE SEMANTIC VIEW failed:`)
- Descriptive message
- `Hint:` for additional context (DuckDB uses this)
- `Did you mean` for suggestions (DuckDB uses this exact phrasing)

**Stack requirement:** `strsim` (already present) for clause name suggestions. `std::fmt` for error formatting. No new crates.

**Implementation approach:**
1. Add a `ParseError` enum in `src/parse.rs` with variants for each failure mode, carrying byte offset and clause context.
2. Use byte offsets from the string scanning already done in `parse_ddl_text()`.
3. Format errors with clause hints and optional character position.
4. The `suggest_closest()` function in `expand.rs` can be reused or extracted to a shared module for clause name suggestions.

**Confidence:** HIGH -- DuckDB's error format verified from issue examples and documentation. The extension already uses this pattern in `ExpandError` and `QueryError`.

### 3. "Did You Mean" Suggestions

**Already implemented for:**
- View names at query time (`QueryError::ViewNotFound` in `src/query/error.rs`)
- Dimension/metric names at query time (`ExpandError::UnknownDimension`, `ExpandError::UnknownMetric` in `src/expand.rs`)
- Both use `strsim::levenshtein` via `suggest_closest()` with threshold of 3

**New for v0.5.1:**
- DDL clause names (tables, relationships, dimensions, metrics) -- small fixed vocabulary
- View names at DDL time (DROP, DESCRIBE for non-existent views)

**Stack requirement:** `strsim` 0.11 (already present). The Levenshtein threshold of 3 is appropriate for short keywords. No additional algorithms needed.

**Confidence:** HIGH -- direct code inspection of existing implementation.

### 4. README Documentation

**No stack implications.** Markdown documentation -- no tools needed beyond a text editor. The existing README.md structure will be extended with DDL syntax reference and worked examples.

---

## What NOT to Add

| Candidate | Why Not |
|-----------|---------|
| `miette` 7.6.0 | Terminal diagnostic renderer -- incompatible with DuckDB's plain-text error channel. Adds ~200KB to binary. Frequent breaking changes. |
| `ariadne` 0.4.x | Same as miette -- terminal renderer, not appropriate for DuckDB error output. |
| `codespan-reporting` 0.11.x | Same category. Designed for compiler-style terminal diagnostics. |
| `thiserror` 2.x | Proc macro for `Display` on error types. The extension already hand-implements `Display` for `ExpandError` and `QueryError`. Adding `thiserror` for 2 more error types is not worth a new proc-macro dependency in the build chain. |
| `unicode-width` 0.2.x | For character-width-aware caret positioning. Not needed -- DDL text is ASCII SQL keywords; `str::len()` gives correct byte=char=display-width mapping. Unicode in identifiers is edge-case and not worth a dependency. |
| `sqlparser` | Full SQL parser. The DDL grammar is trivial (6 prefix patterns). A 500KB parser dependency for prefix matching is extreme overkill. |
| `nom` / `winnow` / `pest` | Parser combinators. Same reasoning as sqlparser -- the grammar is too simple to justify a parsing framework. |
| `regex` | For DDL detection. The existing `eq_ignore_ascii_case` byte comparison is allocation-free and faster than regex for prefix matching. |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Error formatting | Hand-crafted `fmt::Display` following DuckDB conventions | `miette` / `ariadne` | Terminal renderers; DuckDB error channel is plain text; ~200KB binary bloat |
| String similarity | `strsim` 0.11 (existing) | `rapidfuzz` 0.5 | Already have `strsim`; switching gains nothing for this use case |
| DDL parsing | Hand-written prefix matcher + `parse_ddl_text()` | `sqlparser-rs` / `nom` | Grammar is 6 keyword patterns; parser framework is massive overkill |
| Error derive | Manual `impl Display` | `thiserror` 2.x | Only 2 new error types; not worth adding a proc-macro dep |
| Caret positioning | `str::len()` (byte offset) | `unicode-width` | DDL keywords are ASCII; Unicode edge case not worth a dependency |

---

## C++ Shim Changes (No New Dependencies)

The `cpp/src/shim.cpp` needs logic changes but no new C++ dependencies:

### Current FFI Boundary (v0.5.0)

```
sv_parse_rust(query_ptr, query_len) -> u8   [0=not ours, 1=CREATE detected]
sv_execute_ddl_rust(query_ptr, ...) -> u8   [0=success, 1=failure]
```

### Extended FFI Boundary (v0.5.1)

Two approaches, both zero-new-deps:

**Option A: Extended return codes from `sv_parse_rust`**
```
0 = not ours
1 = CREATE SEMANTIC VIEW
2 = CREATE OR REPLACE SEMANTIC VIEW
3 = CREATE SEMANTIC VIEW IF NOT EXISTS
4 = DROP SEMANTIC VIEW
5 = DROP SEMANTIC VIEW IF EXISTS
6 = DESCRIBE SEMANTIC VIEW
7 = SHOW SEMANTIC VIEWS
```
`sv_execute_ddl_rust` already handles the rewriting -- just extend it to handle all variant codes.

**Option B: Keep `sv_parse_rust` returning 0/1, push all routing into `sv_execute_ddl_rust`**
`sv_parse_rust` returns 1 for any `SEMANTIC VIEW` DDL. `sv_execute_ddl_rust` does the fine-grained parsing and routing. Simpler C++ side, more logic in Rust (preferred).

**Recommendation:** Option B. Keep the C++ shim as thin as possible. All intelligence in Rust.

### Why no new C++ symbols

The DDL verbs are all handled by existing Rust table functions. The C++ `sv_ddl_bind` function just calls `sv_execute_ddl_rust` with the query text. `sv_execute_ddl_rust` rewrites the DDL to the appropriate function call and executes it. The C++ shim does not need to know about DROP, DESCRIBE, or SHOW -- it just forwards the text.

---

## Complete v0.5.1 Cargo.toml Changes

**None.** The Cargo.toml is unchanged from v0.5.0:

```toml
# NO CHANGES to [dependencies]:
# duckdb = { version = "=1.4.4", default-features = false }
# libduckdb-sys = "=1.4.4"
# serde = { version = "1", features = ["derive"] }
# serde_json = "1"
# strsim = "0.11"

# NO CHANGES to [build-dependencies]:
# cc = { version = "1", optional = true }

# NO CHANGES to [dev-dependencies]:
# proptest = "1.9"
```

Version bump only: `version = "0.5.0"` -> `version = "0.5.1"` (at milestone completion).

---

## Integration Points

### Where new code touches existing code

| New Feature | Touches | How |
|-------------|---------|-----|
| DDL verb detection | `src/parse.rs` | Extend `detect_create_semantic_view` to detect all 6 DDL patterns |
| DDL rewriting | `src/parse.rs` | Extend `rewrite_ddl_to_function_call` to rewrite all 6 patterns |
| DDL execution | `src/parse.rs` `sv_execute_ddl_rust` | Route to correct function based on DDL variant |
| Parse hook routing | `cpp/src/shim.cpp` | Extend `sv_parse_stub` to detect broader `SEMANTIC VIEW` prefix |
| Error reporting | `src/parse.rs` (new `ParseError` type) | New error type for DDL parse failures with position + suggestions |
| "Did you mean" | `src/expand.rs::suggest_closest` | Reuse for clause name suggestions (may extract to shared util) |
| DESCRIBE/SHOW | No new Rust DDL code | Rewriting routes to existing `describe_semantic_view` / `list_semantic_views` |

### What stays untouched

- `src/expand.rs` -- expansion engine unchanged
- `src/model.rs` -- data model unchanged
- `src/catalog.rs` -- catalog operations unchanged
- `src/ddl/define.rs`, `drop.rs`, `describe.rs`, `list.rs` -- function implementations unchanged
- `src/query/` -- query pipeline unchanged
- `build.rs` -- build script unchanged
- `cpp/include/duckdb.hpp`, `duckdb.cpp` -- amalgamation unchanged

---

## Sources

- [strsim 0.11.1 -- crates.io](https://crates.io/crates/strsim) -- latest version confirmed (HIGH confidence)
- [strsim-rs -- GitHub](https://github.com/rapidfuzz/strsim-rs) -- API reference (HIGH confidence)
- [miette 7.6.0 -- crates.io](https://crates.io/crates/miette) -- evaluated and rejected (HIGH confidence)
- [ariadne -- crates.io](https://crates.io/crates/ariadne) -- evaluated and rejected (HIGH confidence)
- [DuckDB structured errors -- GitHub issue #13782](https://github.com/duckdb/duckdb/issues/13782) -- confirms DuckDB errors are plain text strings (HIGH confidence)
- [DuckDB "Did you mean" format -- GitHub issue #16829](https://github.com/duckdb/duckdb/issues/16829) -- confirms `Did you mean` and `LINE 1:` / caret format (HIGH confidence)
- [DuckDB Friendlier SQL](https://duckdb.org/2022/05/04/friendlier-sql) -- confirms DuckDB error message style with suggestions (HIGH confidence)
- Project source: `src/parse.rs`, `src/expand.rs`, `src/query/error.rs`, `src/ddl/*.rs`, `cpp/src/shim.cpp` -- first-party code inspection (HIGH confidence)
- Project `Cargo.toml` -- dependency versions confirmed via direct file read (HIGH confidence)
