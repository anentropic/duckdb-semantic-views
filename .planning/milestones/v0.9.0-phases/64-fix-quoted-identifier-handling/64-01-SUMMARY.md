---
phase: 64
plan: 01
subsystem: parser
tags: [ident, parser, ddl, quoted-identifiers, leaf-module]
requires: []
provides:
  - module: crate::ident
    surface: |
      pub fn parse_qualified_identifier(input: &str) -> Result<Vec<String>, String>
      pub fn normalize_view_name(input: &str) -> Result<String, String>
      pub fn find_identifier_end(input: &str, allow_paren: bool) -> usize
affects: [src/lib.rs]
tech_stack:
  added: []
  patterns:
    - byte-level state machine (mirrors extract_quoted_string in src/parse.rs)
    - leaf module with String-typed errors (matches body_parser.rs / parse.rs convention)
    - proptest round-trip property (alphabet includes `"`, `.`, ` `)
key_files:
  created:
    - src/ident.rs
  modified:
    - src/lib.rs (single-line `pub mod ident;` insertion, alphabetical position)
decisions:
  - Helper lives in its own leaf module rather than inline in parse.rs — so that
    src/expand/resolution.rs::quote_table_ref (Plan 64-03) can depend on it
    without introducing a parse.rs → expand reverse direction.
  - `String` error type (no new enum) matches existing convention in
    parse.rs::extract_quoted_string and body_parser.rs helpers.
  - Empty quoted identifier `""` is rejected (Snowflake aligns). Bare-quoted
    abutment (`foo"bar"`) is rejected — the parts must be separated by `.`.
  - `find_identifier_end` returns `input.len()` on unterminated quotes; the
    caller's structural parser surfaces the error. This keeps the helper
    monotonic and lets capture sites call it without an Option/Result wrap.
metrics:
  duration_minutes: 4
  tasks: 2
  files_created: 1
  files_modified: 1
  tests_added: 35
  completed_at: "2026-05-17T15:12:00Z"
---

# Phase 64 Plan 01: src/ident.rs Identifier Parser Summary

**One-liner:** Leaf module `src/ident.rs` providing dot-qualified, `"..."`-aware SQL identifier parsing (`parse_qualified_identifier`, `normalize_view_name`, `find_identifier_end`) with 33 unit tests + 2 round-trip proptests; foundation for Plans 64-02 capture-site wiring and 64-03 `quote_table_ref` fix.

## Objective Recap

Introduce a leaf module that owns SQL identifier parsing for double-quoted, dot-qualified identifiers. All five capture sites in `src/parse.rs` and the expansion-side `quote_table_ref` will delegate to these helpers in subsequent plans. Coverage: QID-01 (fully-quoted FQN parsing), QID-02 (partial / mixed quoting), QID-07 (helper-level unit & proptest coverage).

## Public API (verbatim — downstream plans quote this)

```rust
// src/ident.rs

/// Parse a dot-qualified SQL identifier into its *unquoted* parts.
/// Honours `"..."` quoting with `""` escape; treats `.` inside quotes
/// as part of the identifier rather than a part separator.
pub fn parse_qualified_identifier(input: &str) -> Result<Vec<String>, String>;

/// Convenience: return the bare unquoted *last* part of a dot-qualified
/// identifier. This is the lookup key stored in
/// `semantic_layer._definitions(name)`.
pub fn normalize_view_name(input: &str) -> Result<String, String>;

/// Locate the byte offset of the FIRST delimiter that is NOT inside a
/// quoted region. Delimiters are ASCII whitespace, `;`, and (when
/// `allow_paren` is true) `(`. Returns `input.len()` if no delimiter
/// is found or the scan ran off the end inside a quoted region.
#[must_use]
pub fn find_identifier_end(input: &str, allow_paren: bool) -> usize;
```

Module declared in `src/lib.rs`:

```rust
pub mod graph;
pub mod ident;   // <-- inserted alphabetically
pub mod model;
```

## Behaviour Summary

| Input                          | `parse_qualified_identifier` | `normalize_view_name` |
| ------------------------------ | ---------------------------- | --------------------- |
| `orders_sv`                    | `Ok(["orders_sv"])`          | `Ok("orders_sv")`     |
| `"orders_sv"`                  | `Ok(["orders_sv"])`          | `Ok("orders_sv")`     |
| `"db"."sch"."v"`               | `Ok(["db","sch","v"])`       | `Ok("v")`             |
| `db."sch".v`                   | `Ok(["db","sch","v"])`       | `Ok("v")`             |
| `"with""q"`                    | `Ok([r#"with"q"#])`          | `Ok(r#"with"q"#)`     |
| `"a.b"`                        | `Ok(["a.b"])`                | `Ok("a.b")`           |
| `"my table"`                   | `Ok(["my table"])`           | `Ok("my table")`      |
| `""` (empty input)             | `Err("empty identifier")`    | propagates            |
| `"foo`                         | `Err("unterminated quoted identifier")` | propagates |
| `a..b`                         | `Err(empty part)`            | propagates            |
| `.foo`                         | `Err(leading dot)`           | propagates            |
| `foo.`                         | `Err(trailing dot)`          | propagates            |
| `"foo"bar`                     | `Err(trailing garbage)`      | propagates            |
| `""` (the two-char input)      | `Err(empty quoted)`          | propagates            |
| `foo"bar"`                     | `Err(bare-quoted abutment)`  | propagates            |

`find_identifier_end` examples:

| Input                  | `allow_paren` | Result |
| ---------------------- | ------------- | ------ |
| `orders_sv AS x`       | true          | 9      |
| `"my table" AS x`      | true          | 10     |
| `"a.b".c PRIMARY`      | true          | 7      |
| `v(foo)`               | true          | 1      |
| `v(foo)`               | false         | 6      |
| `orders_sv;`           | true          | 9      |
| `"foo bar` (unterm.)   | true          | 8 (= input.len()) |
| `"a""b" rest`          | true          | 6      |
| `"db"."sch"."v" AS x`  | true          | 14     |

## Tests Added (35 total)

- `parse_qualified_identifier_tests` — 18: bare, FQN, mixed quoting, embedded `""` escape, dot/whitespace/semicolon inside quotes, plus error paths (empty input, unterminated quote, empty parts between dots, leading dot, trailing dot, empty `""` quoted, bare-quoted abutment, trailing garbage after closing quote).
- `normalize_view_name_tests` — 5: bare, quoted FQN, mixed, embedded quote preservation, error propagation.
- `find_identifier_end_tests` — 10: bare/whitespace, quoted/inner-ws, quoted/inner-dot, paren toggle, semicolon, unterminated, end-of-input, doubled-quote escape, FQN with quoted parts.
- `proptests` — 2: `parse_emit_roundtrip_is_identity` (256 cases) and `normalize_returns_last_part` (256 cases); alphabet `[\x20-\x7E]{1,16}` deliberately includes `"`, `.`, and space.

## Commits

| Hash      | Type | Subject                                                       |
| --------- | ---- | ------------------------------------------------------------- |
| `37488ee` | feat | feat(64-01): add src/ident.rs identifier parser + normaliser   |
| `f801ae1` | test | test(64-01): add round-trip proptests for ident parser         |

## Verification

- `cargo test --lib ident::tests` — 33 unit tests pass.
- `cargo test --lib ident::tests::proptests` — 2 proptests pass (256 cases each).
- `cargo test --lib ident::` — 35 tests pass overall.
- `cargo build --lib` — clean (no warnings introduced by this module).
- `git diff --name-only` against pre-Plan-01 base — exactly `src/ident.rs` (created) and `src/lib.rs` (one-line insert). No changes to `src/parse.rs`, `src/expand/`, or any test file (those belong to Plans 64-02/03/04).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Import path for `quote_ident` in proptest module**

- **Found during:** Task 2 (first proptest compile)
- **Issue:** Plan §`<action>` step 3 referenced `crate::expand::resolution::quote_ident`, but `resolution` is a private submodule of `expand`. Compile error E0603.
- **Fix:** Switched to `crate::expand::quote_ident`, the re-export at `src/expand/mod.rs:18` (`pub use resolution::{quote_ident, quote_table_ref};`). Equivalent symbol, public path.
- **Files modified:** `src/ident.rs` (test-only `use` statement)
- **Commit:** folded into `f801ae1`

**2. [Rule 3 - Blocking] Pre-commit `cargo fmt` reformatted some `assert_eq!` calls**

- **Found during:** Task 1 commit
- **Issue:** Pre-commit hook ran rustfmt; some `assert_eq!(parse_qualified_identifier("\"a.b\"").unwrap(), vec!["a.b"],)` calls collapsed onto a single line.
- **Fix:** Re-staged and re-committed (idempotent — rustfmt's output is the canonical form).
- **Files modified:** `src/ident.rs` (whitespace only)
- **Commit:** folded into `37488ee` (re-commit after fmt)

No other deviations. All planned signatures, test names, and acceptance criteria met.

## Edge Cases Discovered (none required new tests)

Proptest shrinking did not surface any failing cases — the 1..=4 part × 1..=16 byte × full printable-ASCII alphabet exercises the `""` escape, dot-in-quote, space-in-quote, and embedded-quote paths and all 256 cases of both proptests pass on first run.

## Downstream Plan Inputs

Plans 64-02 and 64-03 can quote the public signatures from this SUMMARY verbatim:

- **64-02 (capture-site wiring):** Will call `ident::normalize_view_name` at each DDL capture site in `src/parse.rs` (CREATE / DROP / ALTER / DESCRIBE / SHOW COLUMNS — 5 sites) and at the runtime `bind.get_parameter(0)` in `src/query/table_function.rs:482`. Will also replace each `find(|c: char| c.is_whitespace() || c == '(')` delimiter scan with `ident::find_identifier_end(after_prefix, /*allow_paren=*/...)` so that `"my table"` does not truncate at the inner space.
- **64-03 (`quote_table_ref` fix):** Will replace the current `table.split('.').map(quote_ident).join(".")` implementation in `src/expand/resolution.rs:29` with `ident::parse_qualified_identifier(table).map(parts → parts.map(quote_ident).join(".")).unwrap_or_else(|_| quote_ident(table))`. Will also replace `if table.contains('.')` heuristic in `qualify_and_quote_table_ref` with `ident::parse_qualified_identifier(table).map(|p| p.len() > 1).unwrap_or(false)`.

## Threat Flags

None. Phase 64 is a parser hardening change with no new external surface; the new module is a pure leaf with no I/O, no FFI, no allocation beyond `String` parts, and no panics (all error paths return `Err(String)`).

## Self-Check: PASSED

- `src/ident.rs` exists — FOUND.
- `src/lib.rs` declares `pub mod ident;` — FOUND.
- Commit `37488ee` (Task 1) — FOUND in `git log`.
- Commit `f801ae1` (Task 2) — FOUND in `git log`.
- `cargo test --lib ident::` exits 0 with 35 tests — VERIFIED.
- `cargo build --lib` succeeds — VERIFIED.
- `grep -c "pub fn parse_qualified_identifier" src/ident.rs` → 1 — VERIFIED.
- `grep -c "pub fn normalize_view_name" src/ident.rs` → 1 — VERIFIED.
- `grep -c "pub fn find_identifier_end" src/ident.rs` → 1 — VERIFIED.
- `grep -c "pub mod ident" src/lib.rs` → 1 — VERIFIED.
- `grep -c "proptest!" src/ident.rs` → 2 — VERIFIED.
