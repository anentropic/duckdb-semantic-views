# Quick Task 260430-vdz: Parser hook ignores leading SQL comments — Research

**Researched:** 2026-04-30
**Domain:** parse hook prefix matching (Rust + C++ FFI)
**Confidence:** HIGH (codebase fully traced; bug report includes a verified reproducer)

## Summary

The parse hook's prefix matcher (`detect_ddl_prefix` in `src/parse.rs`) anchors at the trimmed start of the query and matches keyword tokens via `match_keyword_prefix`. It tolerates whitespace via `query.trim()` but does not strip SQL comments (`-- …\n`, `/* … */`). dbt-duckdb prepends a `/* {"app": "dbt", …} */` annotation to every statement, so every CREATE/ALTER/DROP/SHOW SEMANTIC VIEW statement is classified as `PARSE_NOT_OURS` (rc=2) and DuckDB's stock parser then errors with `syntax error at or near "SEMANTIC"`.

There is a single canonical entry point — `validate_and_rewrite(query)` in `src/parse.rs` — that all FFI paths funnel through. Both `sv_validate_ddl_rust` (parser hook) and `sv_rewrite_ddl_rust` (bind-time re-rewrite) call it. The C++ shim (`cpp/src/shim.cpp`) does no prefix matching of its own. So the fix lives in exactly one Rust function chain.

**Primary recommendation:** Add a small `skip_leading_whitespace_and_comments(&str) -> usize` helper in `src/parse.rs` (or `src/util.rs`) that returns a byte offset, and apply it at the two `query.trim()` sites in `validate_and_rewrite`, `detect_ddl_kind`, `detect_near_miss`, `extract_ddl_name`, and `rewrite_ddl`. Return an offset rather than a stripped slice, so the existing `trim_offset` math (used for error position reporting) absorbs the comment span and error carets continue to point into the *original* query string. Match Postgres semantics: line comments terminate at `\n`, block comments DO NOT nest.

## Bug Location

All prefix-match sites collapse onto **one** lexical entry: `match_keyword_prefix` (called only from `detect_ddl_prefix`). Every public detection function then passes a *trimmed* slice into it. The fix must be applied to the trimming step at every public entry point.

### FFI surface (C++ → Rust)

| Site | File:Line | What it does |
|---|---|---|
| Parser hook stub | `cpp/src/shim.cpp:72` (`sv_parse_stub`) | Calls `sv_validate_ddl_rust` (no own prefix logic) |
| FFI validate entry | `src/parse.rs:1454` (`sv_validate_ddl_rust`) | Wraps `validate_and_rewrite(query)` |
| FFI rewrite entry | `src/parse.rs:1549` (`sv_rewrite_ddl_rust`) | Also wraps `validate_and_rewrite(query)` (called from bind path `cpp/src/shim.cpp:146`, `cpp/src/shim.cpp:240`) |

### Rust prefix-match call chain

| Site | File:Line | Trimming step that misses comments |
|---|---|---|
| Core matcher | `src/parse.rs:62` (`match_keyword_prefix`) | Token-level matcher; agnostic — no change needed |
| Prefix dispatcher | `src/parse.rs:98` (`detect_ddl_prefix`) | Caller-trimmed input — no change here |
| `detect_ddl_kind` | `src/parse.rs:179` | `query.trim().trim_end_matches(';').trim()` — needs comment-skip |
| `detect_semantic_view_ddl` | `src/parse.rs:189` | Delegates to `detect_ddl_kind` — fixed transitively |
| `extract_ddl_name` | `src/parse.rs:669` | `query.trim().trim_end_matches(';').trim()` — needs comment-skip |
| `detect_near_miss` | `src/parse.rs:776` | `query.trim().trim_end_matches(';').trim()` — needs comment-skip |
| **`validate_and_rewrite`** | `src/parse.rs:824` | `query.trim().trim_end_matches(';').trim()` + `trim_offset = query.len() - query.trim_start().len()` — **primary fix site** |
| `rewrite_ddl` | `src/parse.rs:586` | `query.trim().trim_end_matches(';').trim()` — needs comment-skip (called from `validate_and_rewrite` for non-CREATE forms) |

**Key observation:** None of these functions parse comments today. The trimming pattern is uniform — `.trim().trim_end_matches(';').trim()` — making the fix mechanical. `validate_and_rewrite` additionally computes a `trim_offset` (line 827) used by every `ParseError { position: Some(trim_offset + …) }` site for error caret reporting (introduced v0.5.1).

### Trailing comments

Statements with trailing comments (`CREATE … METRICS (…) -- comment`) are not the reported bug, but they may also currently break body validation. Out of scope for this fix unless trivially handled by existing token-aware body parser. The `body_parser.rs` state machine (line 117, 128) uses `trim()` and may already strip trailing whitespace correctly; trailing block comments inside the AS-body are an open question — defer.

## Recommended Fix

### Helper function

Add to `src/parse.rs` (private, near `match_keyword_prefix`). Keeping it co-located with `detect_ddl_prefix` is the natural home; it's specific to the parser hook context and not generally useful in `util.rs`.

```rust
/// Return the byte offset of the first character that is neither ASCII whitespace
/// nor part of a SQL comment. Recognises:
///   - `-- ... \n` line comments (terminated by newline or end-of-input)
///   - `/* ... */` block comments (NOT nested — matches PostgreSQL/DuckDB behaviour)
///
/// Designed for prefix-matching: never errors. An unterminated `/* …` consumes to
/// end of input (so the keyword match below it will simply fail and fall through
/// to PARSE_NOT_OURS, matching today's behaviour for malformed queries).
///
/// Returns the byte offset where real SQL begins, in the *original* slice.
fn skip_leading_whitespace_and_comments(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut i = 0;
    loop {
        // ASCII whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        // Line comment: -- ... \n
        if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue; // re-enter loop to consume more whitespace/comments
        }
        // Block comment: /* ... */ (non-nesting, Postgres semantics)
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() { i += 2; } // consume "*/"
            else { i = bytes.len(); }          // unterminated — consume to end
            continue;
        }
        break;
    }
    i
}
```

### Why offset, not stripped slice

The error-position reporting subsystem (added v0.5.1) computes positions as `trim_offset + plen + …` against the *original* `query` string. If we returned a stripped slice, every position downstream would be shifted by the comment length and carets would point at the wrong character. Returning a byte offset lets us update `trim_offset` once and have everything propagate correctly.

### Apply at five sites

Replace this pattern:

```rust
let trimmed = query.trim();
let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
let trim_offset = query.len() - query.trim_start().len();
```

With:

```rust
let lead = skip_leading_whitespace_and_comments(query);
let trimmed = query[lead..].trim_end();
let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
let trim_offset = lead + (query[lead..].len() - query[lead..].trim_start().len());
// (in practice, after skip_leading_whitespace_and_comments, there is no leading
// whitespace, so `trim_offset = lead + 0`, but keep the form symmetric for safety)
```

Sites: `validate_and_rewrite` (parse.rs:824), `detect_ddl_kind` (parse.rs:179), `extract_ddl_name` (parse.rs:669), `detect_near_miss` (parse.rs:775), `rewrite_ddl` (parse.rs:586).

`detect_semantic_view_ddl` is fixed transitively. `match_keyword_prefix` and `detect_ddl_prefix` are unchanged — they continue to receive an already-stripped slice.

### Block-comment nesting decision (CONFIRMED)

PostgresParser does NOT nest block comments. The bug report's suggestion to match that is correct. Snowflake also does not nest (simple `/* … */`). DuckDB's own parser doesn't nest either (uses libpg_query lex rules). Non-nesting also keeps the helper trivial (no depth counter). Decision: **non-nesting**.

Edge cases worth knowing:
- `/* outer /* inner */ trailing */ CREATE …` — outer comment ends at the *first* `*/`, leaving `trailing */ CREATE …` which fails to match (PARSE_NOT_OURS). Same as Postgres. Acceptable.
- `/* unterminated CREATE …` — helper consumes to EOF, returns `len`, slice is empty, no match, PARSE_NOT_OURS. DuckDB's primary parser will then produce the real error. Correct fallback.

## Test Plan (failing-test-first)

CLAUDE.md gate: `just test-all` runs Rust unit tests + proptests + sqllogictest + DuckLake. Add tests in **all three** of: Rust unit tests (helper), Rust unit tests (validate_and_rewrite + detect_*), and sqllogictest (end-to-end through the C++ FFI + DuckDB parser hook).

### 1. Failing sqllogictest (RECOMMENDED FIRST)

This matches the bug reporter's reproducer most directly and exercises the full extension load → parser-hook → DDL → query pipeline.

**File:** `test/sql/quick_260430_vdz_leading_comments.test` (new)

```text
# Quick task 260430-vdz: parser hook should accept DDL preceded by SQL comments.
# Before the fix: every statement under the dbt query annotation comment fails
# with "Parser Error: syntax error at or near \"SEMANTIC\"" because the prefix
# match anchors at the trimmed start of the query and does not skip /* */ or --.

require semantic_views

statement ok
CREATE TABLE t(x INTEGER, y INTEGER);

statement ok
INSERT INTO t VALUES (1, 10), (2, 20);

# Baseline: plain DDL works
statement ok
CREATE OR REPLACE SEMANTIC VIEW sv_plain AS
TABLES (t AS t PRIMARY KEY (x))
DIMENSIONS (t.xx AS t.x)
METRICS (t.sy AS SUM(t.y))

# Baseline: leading whitespace works (current behaviour)
statement ok


   CREATE OR REPLACE SEMANTIC VIEW sv_ws AS
TABLES (t AS t PRIMARY KEY (x))
DIMENSIONS (t.xx AS t.x)
METRICS (t.sy AS SUM(t.y))

# Bug repro #1: leading block comment (the dbt-duckdb case)
statement ok
/* {"app": "dbt", "node_id": "model.x"} */ CREATE OR REPLACE SEMANTIC VIEW sv_block AS
TABLES (t AS t PRIMARY KEY (x))
DIMENSIONS (t.xx AS t.x)
METRICS (t.sy AS SUM(t.y))

# Bug repro #2: leading line comment
statement ok
-- annotation
CREATE OR REPLACE SEMANTIC VIEW sv_line AS
TABLES (t AS t PRIMARY KEY (x))
DIMENSIONS (t.xx AS t.x)
METRICS (t.sy AS SUM(t.y))

# Multiple comments + interleaved whitespace
statement ok
-- a
/* b */
   -- c
/* d */ CREATE OR REPLACE SEMANTIC VIEW sv_mixed AS
TABLES (t AS t PRIMARY KEY (x))
DIMENSIONS (t.xx AS t.x)
METRICS (t.sy AS SUM(t.y))

# Other DDL forms must also work with leading comments
statement ok
/* x */ DESCRIBE SEMANTIC VIEW sv_block

statement ok
/* x */ SHOW SEMANTIC VIEWS

statement ok
/* x */ ALTER SEMANTIC VIEW sv_block SET COMMENT = 'hello'

statement ok
/* x */ DROP SEMANTIC VIEW sv_line

# Comment-only must NOT be classified as semantic-view DDL.
# (DuckDB will give its own error for the empty/comment-only statement.)
statement error
/* nothing */
----

# Unterminated block comment must NOT panic and must NOT match.
statement error
/* unterminated CREATE SEMANTIC VIEW broken AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))
----
```

Run it: `just build && just test-sql` (sqllogictest requires a fresh build per CLAUDE.md).

### 2. Rust unit tests for the helper

**File:** `src/parse.rs` (extend the existing `#[cfg(test)] mod tests` block).

```rust
#[test]
fn skip_lws_empty() {
    assert_eq!(skip_leading_whitespace_and_comments(""), 0);
}

#[test]
fn skip_lws_only_whitespace() {
    assert_eq!(skip_leading_whitespace_and_comments("   \n\t"), 5);
}

#[test]
fn skip_lws_line_comment() {
    let q = "-- hi\nCREATE";
    assert_eq!(&q[skip_leading_whitespace_and_comments(q)..], "CREATE");
}

#[test]
fn skip_lws_block_comment() {
    let q = "/* hi */ CREATE";
    assert_eq!(&q[skip_leading_whitespace_and_comments(q)..], "CREATE");
}

#[test]
fn skip_lws_multiple_comments_and_ws() {
    let q = "-- a\n  /* b */\n\t-- c\n/*d*/CREATE";
    assert_eq!(&q[skip_leading_whitespace_and_comments(q)..], "CREATE");
}

#[test]
fn skip_lws_block_does_not_nest() {
    // Outer ends at first */, leaving "trailing */ CREATE"
    let q = "/* outer /* inner */ trailing */ CREATE";
    let rest = &q[skip_leading_whitespace_and_comments(q)..];
    assert!(rest.starts_with("trailing"), "got: {rest:?}");
}

#[test]
fn skip_lws_unterminated_block_consumes_to_eof() {
    let q = "/* never ends";
    assert_eq!(skip_leading_whitespace_and_comments(q), q.len());
}

#[test]
fn skip_lws_no_leading_match() {
    // No comments and no whitespace -> offset 0
    assert_eq!(skip_leading_whitespace_and_comments("CREATE"), 0);
}

#[test]
fn skip_lws_dash_dash_at_eof() {
    let q = "-- no newline at end";
    assert_eq!(skip_leading_whitespace_and_comments(q), q.len());
}
```

### 3. Rust unit tests for the integrated detection layer

Same `mod tests` block in `src/parse.rs` — verifies the fix flows through public entry points:

```rust
#[test]
fn detect_create_with_leading_block_comment() {
    assert_eq!(
        detect_semantic_view_ddl("/* hi */ CREATE SEMANTIC VIEW x AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))"),
        PARSE_DETECTED
    );
}

#[test]
fn detect_create_with_leading_line_comment() {
    assert_eq!(
        detect_semantic_view_ddl("-- hi\nCREATE SEMANTIC VIEW x AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))"),
        PARSE_DETECTED
    );
}

#[test]
fn detect_create_or_replace_with_dbt_style_annotation() {
    let q = "/* {\"app\": \"dbt\", \"node_id\": \"model.x\"} */ CREATE OR REPLACE SEMANTIC VIEW x AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.xx AS t.x) METRICS (t.sy AS SUM(t.y))";
    assert_eq!(detect_semantic_view_ddl(q), PARSE_DETECTED);
    let kind = detect_ddl_kind(q);
    assert_eq!(kind, Some(DdlKind::CreateOrReplace));
}

#[test]
fn detect_other_ddl_forms_with_leading_comment() {
    for q in [
        "/* x */ DROP SEMANTIC VIEW v",
        "/* x */ ALTER SEMANTIC VIEW v RENAME TO w",
        "/* x */ DESCRIBE SEMANTIC VIEW v",
        "/* x */ SHOW SEMANTIC VIEWS",
        "/* x */ SHOW SEMANTIC METRICS IN v",
        "-- annotation\nDROP SEMANTIC VIEW v",
    ] {
        assert_eq!(detect_semantic_view_ddl(q), PARSE_DETECTED, "failed: {q}");
    }
}

#[test]
fn comment_only_is_not_semantic_view_ddl() {
    assert_eq!(detect_semantic_view_ddl("/* just a comment */"), PARSE_NOT_OURS);
    assert_eq!(detect_semantic_view_ddl("-- just a comment\n"), PARSE_NOT_OURS);
}

#[test]
fn validate_and_rewrite_with_leading_comment_succeeds() {
    let q = "/* annotation */ DROP SEMANTIC VIEW v";
    let result = validate_and_rewrite(q).expect("should not error");
    assert!(result.is_some(), "expected DDL detection");
    let sql = result.unwrap();
    assert!(sql.contains("drop_semantic_view"), "got: {sql}");
}

#[test]
fn extract_ddl_name_with_leading_comment() {
    assert_eq!(
        extract_ddl_name("/* annotation */ DROP SEMANTIC VIEW my_view").unwrap(),
        Some("my_view".to_string())
    );
}

#[test]
fn error_position_accounts_for_leading_comment() {
    // Missing view name — error position should point at the offset AFTER
    // both the comment AND the prefix, in the ORIGINAL query string.
    let q = "/* hi */ DROP SEMANTIC VIEW";
    let err = validate_and_rewrite(q).expect_err("should error: missing name");
    let pos = err.position.expect("position should be set");
    // Position should be inside the original string (not into the stripped slice)
    // The prefix "DROP SEMANTIC VIEW" starts at byte 9 (after "/* hi */ ").
    // After consuming the prefix (18 bytes), we're at byte 27 == query.len().
    assert_eq!(pos, q.len(), "position should reference original query");
}
```

The last test is the critical regression test for the byte-offset preservation strategy — proves the v0.5.1 caret reporting still points into the original query.

### 4. Optional proptest extension

`tests/parse_proptest.rs` already has `arb_whitespace()`. Add an `arb_leading_comments()` strategy generating mixed `--` / `/* */` / whitespace prefixes, and assert detection still succeeds. Lower priority — unit tests above cover the deterministic edge cases.

### Implementation order (failing-test-first)

1. Add the sqllogictest file → run `just test-sql` → confirm failure with the expected `Parser Error: syntax error at or near "SEMANTIC"`.
2. Add the helper unit tests → run `cargo test` → fails (function doesn't exist).
3. Implement `skip_leading_whitespace_and_comments`.
4. Wire it into the five sites listed above.
5. `cargo test` → green.
6. `just build && just test-sql` → green.
7. `just test-all` → green (full quality gate).

## Pitfalls & Edge Cases

1. **Statement rewriting path consumes original string.** `sv_rewrite_ddl_rust` (`src/parse.rs:1549`) is called from C++ at bind time (`cpp/src/shim.cpp:146`, `cpp/src/shim.cpp:240`) with the *same* original query that the parse hook saw. Both delegate to `validate_and_rewrite`, so applying the fix once at the `validate_and_rewrite` entry covers both. Confirmed by tracing — there is no second prefix-match site in the bind path. **No multi-site coordination needed.**

2. **Re-invocation in YAML FILE bind path.** `cpp/src/shim.cpp:240` re-invokes `sv_rewrite_ddl_rust` with a *reconstructed* string (`kind_prefix + view_name + … + " FROM YAML $__sv_file$…$__sv_file$"`). That reconstructed string never carries comments, so the fix is irrelevant on that path — but it does prove that comments on the original input must be tolerated only on the first invocation. ✓ No regression risk.

3. **Error-position carets (v0.5.1).** Every `ParseError { position: Some(trim_offset + plen + …) }` in `validate_and_rewrite` and below assumes positions are relative to the *original* `query`. Returning an offset (rather than slicing) preserves this invariant. The new `lead` value replaces `trim_offset` and absorbs the comment span. **Add the regression test in section 3** to lock this in.

4. **`body_parser.rs` offsets.** `validate_create_body` (parse.rs:1010) computes `body_offset` via byte arithmetic relative to `trimmed_no_semi`, then passes it to `parse_keyword_body(body_text, base_offset)`. As long as `trim_offset` correctly points at where `trimmed_no_semi` begins in the original, all downstream math stays correct. The fix changes `trim_offset` from "leading whitespace count" to "leading whitespace + comments count" — same semantics, larger value. ✓ No body-parser changes needed.

5. **`detect_near_miss`** does its own `query.trim()` (parse.rs:776) and computes `trim_offset = query.len() - query.trim_start().len()` (parse.rs:804). Must apply the helper here too, or near-miss suggestions for comment-prefixed typos will report the wrong caret position.

6. **No existing test passes a comment.** Verified via grep — `tests/parse_proptest.rs` uses `arb_whitespace()` (parse.rs:28) which generates only ` `, `\t`, `\n`, `\r`. No existing test would accidentally regress. The `peg_compat.test` sqllogictest tests parser-extension interaction but does not exercise comments.

7. **DuckDB's own parser strips comments before calling extension hooks?** No. The bug report demonstrates that DuckDB's parser-extension fallback receives the *raw* query text (comments included). This is consistent with how `parse_function` works — it's called when DuckDB's primary parser already failed. Other parser extensions in the wild (substrait, etc.) handle comment-stripping themselves.

8. **Comment-only / empty statements.** With the fix, `skip_leading_whitespace_and_comments("/* x */")` returns 7 (full length), `trimmed_no_semi` is empty, `detect_ddl_prefix` returns `None`, and `validate_and_rewrite` returns `Ok(None)` → rc=2 → `DISPLAY_ORIGINAL_ERROR`. DuckDB shows its own error. ✓ Correct fallback.

9. **`trim_end_matches(';').trim()` after the helper.** Still needed: the helper only strips *leading* whitespace/comments. Trailing semicolons and trailing whitespace are still handled by the existing `.trim_end_matches(';').trim()` chain. No change needed there.

10. **Performance / O(n²).** The helper is O(n) and runs once per statement at the parse hook. The keyword-prefix matcher already documents avoiding O(n²) — this fix doesn't introduce any. ✓

11. **CR/LF line endings.** `--` line comments terminate at `\n`. Windows-style `\r\n` works fine because `\r` is then consumed by the whitespace loop on the next iteration before the next comment/keyword. ✓

## RESEARCH COMPLETE

**File:** `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/.planning/quick/260430-vdz-review-bug-report-semantic-views-parser-/260430-vdz-RESEARCH.md`

**Key findings:**
- Single canonical fix site: `validate_and_rewrite` in `src/parse.rs:824` is funneled to by both FFI entries (`sv_validate_ddl_rust`, `sv_rewrite_ddl_rust`); the C++ shim has no prefix logic of its own.
- Five Rust functions trim with `.trim()` and miss comments: `validate_and_rewrite`, `detect_ddl_kind`, `extract_ddl_name`, `detect_near_miss`, `rewrite_ddl`. All call `detect_ddl_prefix` after trimming, so the lexical layer is uniform.
- Fix returns a byte offset (not a stripped slice) to preserve v0.5.1 error-caret positions — the `trim_offset` variable simply absorbs the comment span.
- Block comments do NOT nest (Postgres/DuckDB-aligned). Confirmed against bug report.
- Failing-test-first sequence: sqllogictest reproducer → helper unit tests → integrated detection tests → implement → `just test-all`.
- Critical regression test: error-position must still point into the *original* query after a leading comment.
