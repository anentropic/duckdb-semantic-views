# Phase 64: Fix CREATE SEMANTIC VIEW quoted identifier handling - Research

**Researched:** 2026-05-17
**Domain:** SQL identifier parsing / DDL normalisation (Rust, this project's `src/parse.rs` + `src/expand/resolution.rs`)
**Confidence:** HIGH (the bug surfaces directly in two readable code paths; one Rust file + one helper module)

## Summary

The CREATE / DROP / ALTER / DESCRIBE / SHOW COLUMNS path in `src/parse.rs` extracts the view name with a naive "everything up to the next whitespace (or `(` for CREATE)" tokenizer. That tokenizer treats `"memory"."main"."orders_sv"` as a single opaque token and stores it verbatim into `semantic_layer._definitions(name)`. Lookups via `semantic_view('orders_sv', ...)` then miss because the row's `name` is the literal 27-character quoted string, not `orders_sv`. A symmetric bug exists in `quote_table_ref` (`src/expand/resolution.rs:29`): when a source-table reference in the TABLES clause is already quoted, splitting on `.` and re-`quote_ident`ing each part produces the triple-quoted SQL the user reported.

Both bugs are the same shape — code that should consume an *identifier (sequence of parts)* instead consumes a *raw string*. The fix is a small identifier-parsing helper (~40 LOC) that splits a dot-qualified identifier into parts while honoring `"…"` quoting (including `""`-escape), used in (a) every place that captures a view name from DDL source text and (b) `quote_table_ref` so it doesn't re-quote already-quoted parts.

**Primary recommendation:** Introduce `parse_qualified_identifier(&str) -> Vec<String>` (returns *unquoted* parts) and `normalize_view_name(&str) -> String` (returns the bare unquoted last part, used as the lookup key). Apply at the five capture sites in `src/parse.rs`. Replace `quote_table_ref` with a version that calls `parse_qualified_identifier` first, so it operates on unquoted parts and re-quotes deterministically. Do not change the storage column, the case-insensitive comparison contract, or introduce Snowflake-style upper-case folding — those are out of scope per the prompt.

## User Constraints (from phase prompt)

### Locked Decisions (in-scope)

- Identifier normalisation happens during DDL parsing (strip surrounding quotes; split FQN into catalog/schema/name).
- The **bare short name** (the last identifier part, unquoted) is the stored lookup key. This matches what unquoted-CREATE produces today.
- Expansion (specifically `quote_table_ref` and any caller of it) must not re-quote an already-quoted source-table FQN; produce single-pair-of-quotes output regardless of input quoting.
- Error messages reference the unquoted name.
- sqllogictest fixture covers: (a) fully-quoted FQN, (b) partially-quoted forms, (c) GET_DDL round-trip, (d) error messages reference the unquoted name.

### Claude's Discretion

- The exact name of the new helper module / functions (suggested: `src/ident.rs` with `parse_qualified_identifier`, `normalize_view_name`, `parse_table_ref_parts`; or inline in `src/parse.rs` alongside `extract_quoted_string`).
- Whether the helper lives next to `extract_quoted_string` in `parse.rs` (existing single-quote helper) or as a new module. Recommend a new module so `expand::resolution::quote_table_ref` and `parse.rs` can both depend on it without `parse.rs` → `expand` direction.
- Whether to keep the FQN parts (`catalog`, `schema`) on the side for richer error messages, or drop them after extracting the bare name. Recommend: parse them, log them in the validation error path if the FQN's schema/catalog mismatches the connection's, but otherwise discard.
- Proptest scope and seed for round-trip property `parse(emit(parse(x))) == parse(x)`.

### Deferred Ideas (OUT OF SCOPE)

- Catalog/schema namespacing. The lookup key remains the bare view name; FQN parts are not used to enforce schema scoping in this phase. If the user writes `CREATE … "db_a"."public"."v"` and later `semantic_view('v', …)` while connected to `db_b`, it still resolves — same as today's behaviour with unquoted `CREATE … v`. Document as a limitation if needed.
- Snowflake-style case-folding of unquoted identifiers (upper-casing). The project currently compares names case-insensitively via `eq_ignore_ascii_case` and stores the source case. Adopting Snowflake's "unquoted → uppercase, quoted → preserve-case-and-distinct-namespace" would be a breaking change to every existing on-disk catalog and is not in scope.
- Migrating existing rows that were already inserted with quoted-form names. The bug is so new that no production users have these rows (this is a downstream-discovered bug pre-tag); document as "drop and re-create."

## Phase Requirements

No QID-* IDs have been registered yet. Proposed (planner adds these to `.planning/REQUIREMENTS.md`):

| ID | Description | Research Support |
|----|-------------|------------------|
| QID-01 | `CREATE [OR REPLACE] SEMANTIC VIEW` accepts a fully-quoted FQN (e.g. `"db"."schema"."view"`); the row is stored under the bare unquoted last part. | §1 (capture sites in `parse.rs`), §2 (storage column), §3 (Snowflake reference). |
| QID-02 | `CREATE [OR REPLACE] SEMANTIC VIEW` accepts partial quoting (`db.schema."view"`, `"db".schema.view`, `"view"`); same bare-name normalisation. | §4 (helper design). |
| QID-03 | `DROP / ALTER / DESCRIBE / SHOW COLUMNS IN SEMANTIC VIEW` accept the same quoting forms; bare-name lookup resolves them. | §1 capture sites + §5 (lookup). |
| QID-04 | The expansion pipeline (`quote_table_ref` and its callers) emits exactly one pair of quotes per identifier part regardless of whether the source-table reference was already quoted. No `"""triple"""` strings appear in expanded SQL. | §6 (`quote_table_ref` re-quoting). |
| QID-05 | GET_DDL round-trip is stable: `CREATE` with FQN → store bare name → `GET_DDL` emits `CREATE OR REPLACE SEMANTIC VIEW <bare_name> …` (FQN intentionally not reconstructed; this is consistent with the bare-name-as-key contract). | §7 (`render_ddl.rs`). |
| QID-06 | Error messages for "view X does not exist" and "view X already exists" reference the unquoted bare name, never the quoted source string. | §8 (error sites). |
| QID-07 | sqllogictest `test/sql/phase64_quoted_idents.test` covers QID-01..06; unit tests in `src/ident.rs` (or wherever the helper lives) cover empty / unterminated / `""`-escape / dot-in-quoted-part edge cases. | §10 (validation architecture). |

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass. This runs `cargo test`, `just test-sql`, and `just test-ducklake-ci`. A phase that only runs `cargo test` is incomplete.
- **Before-push:** `just ci` adds clippy pedantic + fmt + cargo-deny + fuzz target compilation. The fuzz targets must still compile (`fuzz_ddl_parse` is closest to this change — confirm it still builds; ideally extend its seed corpus with a quoted-FQN sample).
- **Snowflake reference rule:** "If in doubt about SQL syntax or behaviour refer to what Snowflake semantic views does." See §3 — note the deliberate divergence (project keeps case-insensitive bare-name lookup; does *not* adopt Snowflake's case-folding because that would break every existing user's on-disk catalog).
- **Milestone branch:** Currently on `milestone/v0.9.0` (reopened pre-tag). Phase 64 commits go on this branch; do not branch off to `milestone/v0.9.1`.
- **Test file naming:** Use `test/sql/phase64_<name>.test` per existing convention. Add to `test/sql/TEST_LIST` so the runner picks it up (Phase 63 surfaced this gate — see decision log entry "Phase 63 Plan 02").
- **CHANGELOG:** Keep a Changelog 1.1.0 format. Phase 64 belongs in the existing `[0.9.0]` section under `Fixed`; do NOT introduce a Phase 64 subhead. The phase ships as part of v0.9.0.

## Standard Stack

This is a Rust-internal refactor — no new dependencies required.

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| (none — pure Rust string parsing) | — | Identifier tokenisation | Same approach as existing `extract_quoted_string` / `extract_single_quoted` in `src/parse.rs`. Hand-rolled is appropriate because `SQL identifier` is a tiny grammar (`bare \| "…[""…]*"`) and we already maintain the surrounding tokenizer. [VERIFIED: codebase grep, src/parse.rs:362, src/parse.rs:1280] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `proptest` (already in `Cargo.toml`) | existing | Round-trip property tests | Property: `for any (legal_input), parse(emit(parse(input))) == parse(input)`. [VERIFIED: workspace] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-rolled parser | `sqlparser-rs` | Would add a heavy dependency for a tiny use case; sqlparser-rs's `ObjectName` type does parse FQN with quotes, but pulling it in for this one helper is disproportionate. Existing parser is hand-rolled throughout (`body_parser.rs` is 4046 LOC of bespoke tokenizing). [ASSUMED — not benchmarked, but in line with rest of codebase] |
| Snowflake-style case folding | `to_ascii_uppercase` on unquoted | Would break every existing user's catalog. Out of scope per prompt. [VERIFIED: prompt + Snowflake docs] |

## Architecture Patterns

### Recommended Helper Module Layout

```
src/
├── ident.rs            # NEW: parse_qualified_identifier, normalize_view_name, parse_table_ref_parts
├── parse.rs            # MOD: 5 call-sites use ident:: helpers; existing logic untouched otherwise
├── expand/
│   └── resolution.rs   # MOD: quote_table_ref delegates to ident::parse_qualified_identifier
└── lib.rs              # MOD: `pub mod ident;` (or `mod ident; pub use …` if internal-only)
```

Why a new module: `expand::resolution::quote_table_ref` and `parse::*` both need the same logic; placing it in `parse.rs` and re-importing from `expand` creates an awkward direction (today `parse.rs` already depends on `expand::*` for type lookups, so the reverse would not cause a cycle, but a clean dependency is cheap). A leaf module `src/ident.rs` (like `src/util.rs` and `src/errors.rs`) is the established pattern for shared zero-dependency helpers.

### Pattern 1: Identifier Parsing Helper

**What:** A function that takes a raw identifier-shaped substring (everything up to the next non-identifier delimiter — whitespace, `(`, `;`) and returns `Vec<String>` of unquoted parts.

**When to use:** Every CREATE / DROP / ALTER / DESCRIBE / SHOW COLUMNS name-extraction site (5 capture points listed in §1). Also `quote_table_ref` in expansion (1 site).

**Sketch:**

```rust
// Source: NEW src/ident.rs (proposed)

/// Parse a dot-qualified SQL identifier into its unquoted parts.
///
/// Supports:
/// - bare:               `orders_sv`        -> ["orders_sv"]
/// - fully quoted:       `"orders_sv"`      -> ["orders_sv"]
/// - quoted with embedded quote (SQL standard `""` escape):
///                       `"with""quote"`    -> [r#"with"quote"#]
/// - dot-qualified:      `db.schema.view`   -> ["db", "schema", "view"]
/// - mixed quoting:      `"db".schema."v"`  -> ["db", "schema", "v"]
/// - dot inside quotes (treated as part of the identifier, NOT a separator):
///                       `"a.b"`            -> ["a.b"]
///
/// Returns `Err` for empty input, unterminated quotes, empty parts (`a..b`),
/// or a closing `"` not followed by `.` or end-of-input.
pub fn parse_qualified_identifier(input: &str) -> Result<Vec<String>, String> {
    // State machine: at start of each part either '"' begins quoted form or
    // a run of non-`.` non-`"` chars is the bare form. After a part, expect
    // either '.' (more parts to come) or end of input.
    // Quoted form: scan until matching '"', honoring `""` as escaped quote.
    // ...
}

/// Convenience: bare unquoted last part. The lookup key stored in
/// `semantic_layer._definitions(name)`.
pub fn normalize_view_name(input: &str) -> Result<String, String> {
    let parts = parse_qualified_identifier(input)?;
    parts.into_iter().next_back().ok_or_else(|| "empty identifier".to_string())
}
```

### Pattern 2: Capture-Site Surgery in `parse.rs`

Every capture site today uses `find(|c: char| c.is_whitespace() [|| c == '('])`. That stays — it's the *post-name* delimiter scan and it's correct *for bare names*. The change is: after capturing the raw token, pass it through `ident::normalize_view_name` before storing/escaping.

For quoted FQNs the existing delimiter scan still works because **double-quoted identifiers in SQL cannot contain a literal whitespace unless the whitespace is itself inside the quotes**, and the bug report's input `"memory"."main"."orders_sv"` contains no whitespace until the next clause. The single edge case is `"name with space"` — today the existing scanner truncates the captured token at the space inside the quotes. The fix needs to update the delimiter scan to **skip whitespace that appears between matching `"…"` pairs**.

Concretely: replace each occurrence of `after_prefix.find(|c: char| c.is_whitespace() || c == '(')` with a small `find_identifier_end(after_prefix, /*allow_paren=*/true)` helper that walks the bytes while honoring `"…[""…]*"` regions. ~15 LOC. The five sites are listed in §1.

### Anti-Patterns to Avoid

- **Strip quotes with `name.trim_matches('"')` then split on `.`.** Wrong: it doesn't handle `""`-escaping, doesn't handle mixed quoting, and silently drops legitimate-but-rare cases like an identifier that legitimately ends with a quote inside its name. The hand-rolled state machine is ~30 lines and unambiguous.
- **Strip quotes at the storage layer (in `emit_native_create_sql`).** Wrong: the name has already flowed through `extract_ddl_name` → `validate_create_body` → `rewrite_ddl_keyword_body` → `escape_sql_arg` by then; multiple paths embed it and stripping at one point leaves the other paths inconsistent. Normalise at the **capture point** in `parse.rs`.
- **Special-case the `"a"."b"."c"` shape and pass everything else through.** Wrong: partial quoting (`a."b".c`) and single-part-quoted (`"orders_sv"`) are the most common real-world forms. Handle them uniformly via the parser.
- **Re-quote on output without un-quoting on input in `quote_table_ref`.** This is the current bug. The fix is: `quote_table_ref(stored: &str)` → `ident::parse_qualified_identifier(stored)?` → `parts.iter().map(quote_ident).join(".")`. Same output for unquoted input as today; correct output for already-quoted input.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| String-escape SQL literal values (single quotes) | A new escape helper | Existing `escape_sql_arg` / `unescape_sql_arg` in `src/parse.rs:2047` | Already battle-tested for single-quoted SQL literals — view names get embedded that way after normalisation. |
| Quote a SQL identifier for emission | New code | Existing `quote_ident` in `src/expand/resolution.rs:16` | Already handles `"` → `""` escape correctly. The new `parse_qualified_identifier` reverses it; `quote_ident` is its inverse. |
| Tokenise a quoted SQL string literal | New code | Existing `extract_quoted_string` (single-quote) at `src/parse.rs:362` | NOTE: this is for `'…'` literals, not `"…"` identifiers — we still need a new helper for double-quoted identifiers, but follow the same shape (returns content + bytes consumed). |
| Walk SQL clauses honoring quoted regions | New code | Existing `split_at_depth0_commas` in `src/body_parser.rs:94` | Already shows the pattern of "scan while respecting quoted regions"; adapt for our delimiter scanner. |

**Key insight:** the surrounding code is already careful about *single-quoted SQL string literals*. It is sloppy about *double-quoted SQL identifiers*, because the project's tests never exercised them. Phase 64 is mostly about applying the existing carefulness pattern to the identifier slot.

## Runtime State Inventory

This is not a rename / refactor / migration phase per se — it's a code-fix phase. However, there is a potential **stored-data interaction**: existing rows in `semantic_layer._definitions` might already have quoted-form `name` values if a downstream user created them with quoted FQNs before the fix shipped.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data (catalog rows) | Any pre-existing rows in `semantic_layer._definitions` where `name` was inserted as a quoted FQN string (the very bug we're fixing). The PK constraint on `name` (VARCHAR PRIMARY KEY, `src/catalog.rs:38`) prevents a duplicate insert, but rows already there with quoted names will remain unresolvable by short-name lookup. | **Document as "drop and re-create"** in CHANGELOG `Fixed` bullet. This bug is downstream-discovered pre-tag and no production users have shipped data with quoted-FQN names. No data migration step required. [VERIFIED: prompt — Phase 64 added pre-v0.9.0 tag; STATE.md confirms milestone reopened pre-tag] |
| Live service config | None — extension state lives entirely in the host DB; no external service involvement. | None. |
| OS-registered state | None. | None. |
| Secrets/env vars | None. | None. |
| Build artifacts | `cargo build` re-compiles. `fuzz/target` artifacts independent. | None. |

**The canonical question — "After every file in the repo is updated, what runtime systems still have the old string cached?"** — answer for Phase 64: only existing on-disk catalog rows. Mitigated by "this bug existed for ~0 production user-days" timing.

## Common Pitfalls

### Pitfall 1: Forgetting the ALTER RENAME target name

**What goes wrong:** `ALTER SEMANTIC VIEW v RENAME TO "memory"."main"."v2"`. The current code at `src/parse.rs:639` (`rewrite_alter`) extracts `v` correctly for the source but the RENAME TO target undergoes the same naive split. If only the source-name capture is fixed, the rename target still ends up stored as `"memory"."main"."v2"`.

**Why it happens:** ALTER has *two* identifier slots; the bug exists in both.

**How to avoid:** Normalize **both** the old name and the new name in `rewrite_alter`. The new name flows through to `rewrite_alter_rename` at `src/parse.rs:2147` as `new_escaped`; normalize before escaping.

**Warning signs:** a test that does CREATE-then-RENAME-with-quoted-target and then `semantic_view('new_short_name', …)` fails to resolve.

### Pitfall 2: SHOW COLUMNS IN SEMANTIC VIEW + DESCRIBE SEMANTIC VIEW

**What goes wrong:** These are *read-side* DDL forms that route via `extract_name_only` (`src/parse.rs:311`) and embed the captured name as a SQL string literal into the read-side table-function call (e.g. `SELECT * FROM describe_semantic_view('"memory"."main"."v"')`). The read-side function then calls `catalog.lookup("\"memory\".\"main\".\"v\"")` and misses.

**Why it happens:** Same root cause — `extract_name_only` doesn't unquote.

**How to avoid:** Normalize inside `extract_name_only`. That covers DROP, DROP IF EXISTS, DESCRIBE, SHOW COLUMNS, and ALTER (source-name slot). The other call site for read-side bind — `bind.get_parameter(0)` in `src/query/table_function.rs:482` — also needs normalisation, because `semantic_view('"orders_sv"', …)` (the user double-quoting the *string-literal contents*) should also resolve. Normalise the runtime view-name argument the same way.

**Warning signs:** `DESCRIBE SEMANTIC VIEW "v"` returns a `semantic view '"v"' does not exist` error when `v` exists in the catalog.

### Pitfall 3: Existence pre-check uses the wrong name

**What goes wrong:** `emit_native_create_sql` (`src/parse.rs:1866`) calls `ctx.catalog.exists(name)` before emitting the INSERT. If `name` is the raw quoted string and the existing row was created with an unquoted name, `exists()` returns false → CREATE silently succeeds → second row sneaks in under a different `name` value → PK violation if normalisation is applied later in the same transaction (or worse: two effective rows for the same logical view).

**Why it happens:** existence check operates on the unfixed name; the INSERT operates on the fixed name; they disagree.

**How to avoid:** Normalise *before* the existence check, *before* the INSERT, *before* error message construction. Single point of normalisation at the capture site keeps all downstream paths consistent.

**Warning signs:** CREATE OR REPLACE with quoted name appears to succeed but `list_semantic_views()` shows the OLD row still there.

### Pitfall 4: `quote_table_ref` and `qualify_and_quote_table_ref`'s `contains('.')` heuristic

**What goes wrong:** `qualify_and_quote_table_ref` at `src/expand/resolution.rs:47` checks `if table.contains('.')` to decide "already-qualified, skip prepending database/schema". A name like `"memory.main"` (one quoted part containing a dot) trips the heuristic and is treated as qualified, then `quote_table_ref` re-splits-and-re-quotes producing wrong SQL.

**Why it happens:** Same root cause — string-level dot test instead of structural parts test.

**How to avoid:** Use `parse_qualified_identifier(table).map(|parts| parts.len() > 1).unwrap_or(false)` to test "is qualified" structurally. Re-emit via `parts.iter().map(quote_ident).join(".")`.

**Warning signs:** A semantic view defined over a source table whose actual physical name has a dot in it produces malformed FROM-clause SQL.

### Pitfall 5: The body-parser TABLES clause re-splits on whitespace

**What goes wrong:** `parse_single_table_entry` at `src/body_parser.rs:660` does `split_first_token(entry)` to peel off the alias, then expects `AS`, then takes everything up to the next `PRIMARY KEY` / `UNIQUE` keyword as the source-table name. If the source-table name is `"my db"."schema"."t"` (whitespace inside the quoted catalog part), `split_first_token` truncates mid-name and the parse breaks.

**Why it happens:** Same bug in another file. The TABLES clause is a separate parser from the view-name extractor in `parse.rs`.

**How to avoid:** Phase 64's prompt scopes this as the **secondary expansion-side fix**. The minimum fix is: make `quote_table_ref` tolerate already-quoted input (so `"memory"."main"."orders"` round-trips to itself). The body parser's whitespace-splitting issue only matters if the user puts spaces inside quoted parts of a source table name — vanishingly rare. Document the limitation; defer the body-parser fix to a future phase unless trivial.

**Warning signs:** sqllogictest fixture with `TABLES (o AS "my db"."s"."t" PRIMARY KEY (id))` fails to parse.

## Code Examples

### Example A: Normalisation at capture point in `validate_create_body`

```rust
// Source: PROPOSED change at src/parse.rs:1136-1145
// (CREATE / CREATE OR REPLACE / CREATE IF NOT EXISTS form)

let (raw_name, name_end) = find_identifier_token(after_prefix)
    .ok_or_else(|| ParseError {
        message: "Missing view name after DDL prefix.".to_string(),
        position: Some(trim_offset + plen),
    })?;
let name = crate::ident::normalize_view_name(raw_name)
    .map_err(|e| ParseError {
        message: format!("Invalid view name: {e}"),
        position: Some(trim_offset + plen),
    })?;
// ... existing flow continues with `name` (the bare unquoted last part)
```

### Example B: `quote_table_ref` rewritten

```rust
// Source: PROPOSED replacement at src/expand/resolution.rs:29

#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    match crate::ident::parse_qualified_identifier(table) {
        Ok(parts) => parts.iter().map(|p| quote_ident(p)).collect::<Vec<_>>().join("."),
        // Fallback for inputs we can't parse (legacy or malformed): emit
        // verbatim wrapped in quotes once, escaping any literal `"`. This
        // matches today's behaviour for the unquoted case and prevents
        // double-quoting for any input.
        Err(_) => quote_ident(table),
    }
}
```

### Example C: sqllogictest fixture skeleton

```
# test/sql/phase64_quoted_idents.test

require semantic_views

statement ok
CREATE TABLE orders (id INTEGER PRIMARY KEY, amount DECIMAL(10,2), region VARCHAR);

statement ok
INSERT INTO orders VALUES (1, 100.00, 'US'), (2, 200.00, 'EU');

# QID-01 — fully-quoted FQN, lookup by short name resolves
statement ok
CREATE OR REPLACE SEMANTIC VIEW "memory"."main"."orders_sv" AS
  TABLES (o AS orders PRIMARY KEY (id))
  DIMENSIONS (o.region AS o.region)
  METRICS (o.total AS SUM(o.amount))

query R
FROM semantic_view('orders_sv', metrics := ['total'])
----
300.00

# QID-02 — partial quoting variants resolve to the same bare key
statement ok
DROP SEMANTIC VIEW orders_sv

statement ok
CREATE SEMANTIC VIEW main."orders_sv" AS
  TABLES (o AS orders PRIMARY KEY (id))
  DIMENSIONS (o.region AS o.region)
  METRICS (o.total AS SUM(o.amount))

query R
FROM semantic_view('orders_sv', metrics := ['total'])
----
300.00

# QID-03 — DROP / DESCRIBE / ALTER accept the same forms
statement ok
DESCRIBE SEMANTIC VIEW "orders_sv"

statement ok
ALTER SEMANTIC VIEW "orders_sv" RENAME TO "orders_sv_v2"

statement ok
DROP SEMANTIC VIEW "main"."orders_sv_v2"

# QID-04 — no triple-quoting in expanded SQL (uses TABLES with FQN)
statement ok
CREATE SEMANTIC VIEW orders_sv AS
  TABLES (o AS "memory"."main"."orders" PRIMARY KEY (id))
  DIMENSIONS (o.region AS o.region)
  METRICS (o.total AS SUM(o.amount))

# explain_semantic_view should show "memory"."main"."orders", not """memory""".…
query T
SELECT plan FROM explain_semantic_view('orders_sv', metrics := ['total'])
WHERE plan LIKE '%FROM%'
LIMIT 1
----
... (regex check: contains FROM "memory"."main"."orders", not """memory""")

# QID-05 — GET_DDL round-trip
query T
SELECT GET_DDL('SEMANTIC_VIEW', 'orders_sv')
----
... (contains: CREATE OR REPLACE SEMANTIC VIEW orders_sv AS …)

# QID-06 — error messages reference unquoted bare name
statement error semantic view 'nonexistent_view' does not exist
DESCRIBE SEMANTIC VIEW "memory"."main"."nonexistent_view"
```

(Don't forget to add `phase64_quoted_idents.test` to `test/sql/TEST_LIST`.)

## State of the Art

| Old Approach (today's bug) | Current Approach (proposed) | When Changed | Impact |
|----------------------------|-----------------------------|--------------|--------|
| `after_prefix.find(|c: char| c.is_whitespace() \|\| c == '(')` then store raw token | `find_identifier_token` (quote-aware end-detection) → `ident::normalize_view_name` → bare unquoted name stored | Phase 64 | Quoted-FQN CREATE works; lookup by short name resolves; error messages unquoted. |
| `quote_table_ref(s) = s.split('.').map(quote_ident).join(".")` (raw string split) | `quote_table_ref(s) = parse_qualified_identifier(s).map(parts → parts.map(quote_ident).join(".")).unwrap_or_else(|_| quote_ident(s))` | Phase 64 | Triple-quoting on already-quoted source-table FQNs eliminated; unquoted case unchanged. |

**Deprecated/outdated:** None. No existing helper is being removed; the change is additive (new module + small surgery in two existing files).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | No production user has on-disk catalog rows with quoted-FQN names today (this bug is downstream-discovered pre-tag). | Runtime State Inventory | Low — STATE.md confirms milestone reopened pre-tag; v0.9.0 is unreleased. If wrong, those rows would need a one-shot migration script. [ASSUMED — based on STATE.md and prompt timing] |
| A2 | Snowflake parity is *not* required for case-folding; the project's existing case-insensitive comparison is the accepted convention. | §3, Out-of-scope | None — explicit in the prompt's "out of scope" section. [VERIFIED: prompt] |
| A3 | The user-reported reproduction case (`"memory"."main"."orders_sv"`) is the dominant shape; partial-quoting variants exist but are rarer. | sqllogictest fixture design | Low — the fix handles all three forms uniformly via one helper. [ASSUMED] |
| A4 | DuckDB's `parser_override` callback receives the literal source query bytes including quotes, as confirmed by inspecting `sv_parser_override_rust` at `src/parse.rs:2506`. | Step 9 / Tool Strategy | None — verified by reading the FFI entry point: `std::slice::from_raw_parts(query_ptr, query_len)` → `from_utf8(bytes)`. [VERIFIED: src/parse.rs:2525-2528] |
| A5 | `fuzz_ddl_parse` already exercises arbitrary UTF-8 inputs through `validate_and_rewrite`; the quoted-identifier helper will be covered transitively as long as the existing fuzz target keeps building. | Validation Architecture | Low — confirmed by reading `fuzz/fuzz_targets/fuzz_ddl_parse.rs`. [VERIFIED] |
| A6 | The body parser's whitespace-splitting in `parse_single_table_entry` is *not* exercised by the bug report (the reported reproduction doesn't put spaces inside quoted source-table parts). Deferring its fix is safe. | Pitfall 5 | Medium — if a user does try `TABLES (o AS "my db"."schema"."t" …)` they'll get a parse error. Document as a known limitation. [ASSUMED] |

## Open Questions

1. **Should `RENAME TO` accept a quoted new-name?**
   - What we know: ALTER RENAME has two slots; the source-name slot is captured by `extract_name_only`, the target-name slot by `rewrite_alter` itself (`src/parse.rs:631-700`).
   - What's unclear: Plan-time choice — apply `normalize_view_name` to both slots, or only the source? Recommend: both. The user-facing contract is "any of these identifier forms produce the same bare name."
   - Recommendation: Normalise both. Cost is one extra helper call.

2. **Should the `semantic_view('name', …)` runtime argument also be normalised?**
   - What we know: `bind.get_parameter(0)` at `src/query/table_function.rs:482` reads the SQL string literal verbatim.
   - What's unclear: Today the literal is a single-quoted SQL string (e.g. `'orders_sv'`). If a user writes `semantic_view('"orders_sv"', …)` (double-quoting the *contents* of a SQL string literal), should that resolve?
   - Recommendation: Yes — normalise the runtime argument too. It's one call to `normalize_view_name` and it keeps the rule "any quoting form → bare key" universally consistent. Trivial to add to QID-03.

3. **Schema/catalog mismatch — silent ignore or warn?**
   - What we know: Today the bare name is the lookup key, so `"db_a"."public"."v"` and `"db_b"."public"."v"` both resolve to `v` if either was created.
   - What's unclear: Should we at least warn? Or store the FQN parts in metadata for future use?
   - Recommendation: Silently ignore FQN parts for v0.9.0 (matches "out of scope: catalog/schema namespacing"). Document in CHANGELOG `Known limitations` if the planner wants to be explicit.

## Environment Availability

Pure-code phase. No external tools, services, runtimes, or CLI utilities beyond the existing build chain.

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` | Build / unit tests | ✓ | (pinned in `rust-toolchain.toml`) | — |
| `just` | Recipe runner (`just build`, `just test-all`, `just ci`) | ✓ | — | Direct `cargo` invocation |
| `sqllogictest` (DuckDB's runner) | `just test-sql` | ✓ (vendored in the project's CI scripts) | — | — |

No missing dependencies. Step 2.6: nothing further to audit.

## Validation Architecture

Nyquist validation is enabled (config.json `workflow.nyquist_validation` defaults to true).

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` (unit + proptest + doctests) + `sqllogictest` (via `just test-sql`) + Python `pytest` (integration; not required for this phase) |
| Config file | `Cargo.toml` (workspace + dev-deps), `test/sql/TEST_LIST` (sqllogictest registry) |
| Quick run command | `cargo test --lib ident` (helper unit tests only) |
| Full suite command | `just test-all` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| QID-01 | Fully-quoted FQN CREATE → bare-name lookup resolves | sqllogictest | `just test-sql phase64_quoted_idents` | ❌ Wave 0 |
| QID-01 | Helper: `parse_qualified_identifier("\"a\".\"b\".\"c\"") == ["a","b","c"]` | unit | `cargo test --lib ident::tests::fully_quoted_fqn -x` | ❌ Wave 0 |
| QID-02 | Mixed/partial quoting → same bare key | sqllogictest | `just test-sql phase64_quoted_idents` | ❌ Wave 0 |
| QID-02 | Helper: `normalize_view_name("a.\"b\".c") == "c"` etc. | unit | `cargo test --lib ident::tests::partial_quoting -x` | ❌ Wave 0 |
| QID-03 | DROP / ALTER / DESCRIBE / SHOW COLUMNS accept quoted forms | sqllogictest | `just test-sql phase64_quoted_idents` | ❌ Wave 0 |
| QID-04 | `quote_table_ref("\"db\".\"sch\".\"t\"")` emits single-pair quotes | unit | `cargo test --lib expand::resolution::tests::already_quoted -x` | ❌ Wave 0 (extends existing tests in `src/expand/resolution.rs`) |
| QID-04 | EXPAND-side end-to-end: no `"""` substrings in expanded SQL when TABLES uses quoted FQN | sqllogictest | `just test-sql phase64_quoted_idents` | ❌ Wave 0 |
| QID-05 | GET_DDL round-trip with quoted-CREATE → emits bare-name CREATE OR REPLACE | sqllogictest | `just test-sql phase64_quoted_idents` | ❌ Wave 0 |
| QID-06 | Error messages reference unquoted bare name | sqllogictest (`statement error`) | `just test-sql phase64_quoted_idents` | ❌ Wave 0 |
| QID-07 | Property: `for any legal identifier string s, parse(emit(parse(s))) == parse(s)` | proptest | `cargo test --lib ident::proptests` | ❌ Wave 0 |
| (cross-cutting) | Fuzz target `fuzz_ddl_parse` still compiles and runs > 1M iterations on `parse_qualified_identifier`-bearing inputs | fuzz (compile-only in CI) | `just ci` | ✓ exists; add `"\"a\".\"b\".\"c\""` to seed corpus |

### Sampling Rate
- **Per task commit:** `cargo test --lib ident` (helper unit tests; sub-second).
- **Per wave merge:** `just test-all` (full Rust + sqllogictest + DuckLake CI).
- **Phase gate:** `just ci` green before `/gsd-verify-work` (includes clippy pedantic + cargo-deny + fuzz-target compilation per `CLAUDE.md`).

### Wave 0 Gaps
- [ ] `src/ident.rs` — new module: `parse_qualified_identifier`, `normalize_view_name`, unit tests + proptests for round-trip.
- [ ] `src/lib.rs` — `pub mod ident;` (or `mod ident;` with selective re-exports).
- [ ] `test/sql/phase64_quoted_idents.test` — sqllogictest fixture covering QID-01..06.
- [ ] `test/sql/TEST_LIST` — register the new fixture (Phase 63 Plan 02 surfaced this gate).
- [ ] `fuzz/fuzz_targets/fuzz_ddl_parse.rs` — extend seed corpus with at least three quoted-FQN samples (`"v"`, `"db"."s"."v"`, `db."s".v`). Compile check satisfies `just ci`; running the target longer is optional.
- [ ] `src/expand/resolution.rs` — modify `quote_table_ref`; extend its existing `#[cfg(test)] mod tests { mod quote_table_ref_tests { … } }` with `already_quoted_simple`, `already_quoted_fqn`, `mixed_quoting`, `embedded_double_quote_in_part`.

(No new framework install needed.)

## Security Domain

Security enforcement is enabled by default. Reviewing applicable ASVS categories for an identifier-parsing change:

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes | New parser must reject malformed input (unterminated quote, empty part, trailing garbage) with `Err`. SQL-string-literal embedding continues to use existing `escape_sql_arg` (`src/parse.rs:2047`). |
| V6 Cryptography | no | — |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via the view-name slot (e.g. `"foo"; DROP TABLE x; --"`) | Tampering | The normalised bare name is *embedded in SQL via existing `escape_sql_arg`* (single-quote escape for VALUES/WHERE clauses). The new parser produces a `String` of identifier content; `escape_sql_arg` handles `'` → `''`. No new sink. |
| Parser DoS via a pathological input (e.g. millions of `""` escapes) | Denial of Service | Existing `fuzz_ddl_parse` exercises the whole `validate_and_rewrite` path. The new helper is O(n) and allocates one `String` per part — same big-O as `escape_sql_arg`. |
| Identifier confusion: same-looking-but-different identifiers resolve to the same row (Unicode confusables) | Tampering | Out of scope — DuckDB itself doesn't normalize confusables. The project's `eq_ignore_ascii_case` is ASCII-only, so Unicode case variants don't collide. [VERIFIED: project uses `eq_ignore_ascii_case` everywhere; see `src/expand/facts.rs:18`+] |
| Malicious quoted name containing `"` to break out of `escape_sql_arg` SQL-string-literal embedding | Tampering | `escape_sql_arg` escapes single quotes (`'`), not double quotes — but the name is embedded inside *single*-quoted SQL string literals (`VALUES ('{name_escaped}', …)`), so double quotes are inert in that context. The parser strips `"` from the identifier and converts `""` to `"`; the resulting `String` is then single-quote-escaped before SQL embedding. No new bypass. [VERIFIED: src/parse.rs:1917, escape_sql_arg semantics] |

## Sources

### Primary (HIGH confidence)
- Codebase: `src/parse.rs` (the bug's home: extract_name_only, validate_create_body, rewrite_alter, extract_ddl_name, emit_native_create_sql)
- Codebase: `src/expand/resolution.rs:29` (the secondary bug's home: `quote_table_ref`)
- Codebase: `src/catalog.rs:35-55` (PK constraint on `name VARCHAR PRIMARY KEY`)
- Codebase: `src/query/table_function.rs:482` (semantic_view runtime bind)
- Codebase: `src/render_ddl.rs:293-305` (GET_DDL emission; emits stored `name` verbatim)
- Codebase: `src/ddl/read_yaml.rs:21-23` (existing `resolve_bare_name(input: &str) -> &str` for FQN handling — precedent, but doesn't strip quotes)
- [Snowflake — Identifier requirements](https://docs.snowflake.com/sql-reference/identifiers-syntax) — case-folding rules (quoted preserves case; unquoted → upper). Confirms the project's *deliberate divergence* (case-insensitive compare without folding).
- [Snowflake — CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) — view name must be unique within schema; standard identifier rules apply.

### Secondary (MEDIUM confidence)
- TECH-DEBT.md item 19 (DESCRIBE/SHOW read committed state) — orthogonal but reminds us that read-side functions route through `catalog_conn`. Normalisation must happen at parse time, not at read-side bind time, to be consistent across CREATE and SHOW.
- v0.8.0 phases 58-62 retroactive RESEARCH/PLAN files in `.planning/phases/58..62` — establish the parser_override → rewrite_to_native_sql flow that Phase 64 plugs into.

### Tertiary (LOW confidence)
- (none — all claims either verified by direct code inspection or explicitly tagged `[ASSUMED]` in the Assumptions Log.)

## Metadata

**Confidence breakdown:**
- Capture sites & storage path: HIGH — read line-by-line from `src/parse.rs`.
- `quote_table_ref` re-quoting bug: HIGH — exact mechanism reproducible from the helper's 7 lines of code.
- Snowflake reference behaviour: HIGH — official docs cited.
- Pitfalls (ALTER RENAME target, existence pre-check, body-parser whitespace): MEDIUM — derived from code structure, not all confirmed by failing tests yet; the planner should add the relevant test cases first to make the surface explicit.
- No production-rows-with-quoted-names assumption: MEDIUM — strong based on STATE.md timing but could be wrong if a downstream user has been running on `milestone/v0.9.0` HEAD and created data.

**Research date:** 2026-05-17
**Valid until:** 2026-06-16 (30 days — stable area of the codebase; no upstream DuckDB change expected to shift this analysis).
