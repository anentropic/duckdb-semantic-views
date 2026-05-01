---
quick_id: 260430-vdz
description: Fix parser hook to skip leading SQL comments (-- and /* */) before prefix matching
status: completed
date: 2026-04-30
commits:
  - ca9197a tests(parse): add failing reproducers for leading-comment bug (260430-vdz)
  - edf5196 fix(parse): skip leading SQL comments before DDL prefix match (260430-vdz)
---

# Quick Task 260430-vdz Summary

## Bug

`semantic_views`' parser hook (`detect_ddl_prefix` in `src/parse.rs`) anchored at the trimmed start of the query string but did not strip leading SQL comments. Tools that prepend annotations — most notably **dbt-duckdb**, which unconditionally prefixes every statement with `/* {"app": "dbt", "node_id": "model.X"} */` — caused every `CREATE / ALTER / DROP / SHOW SEMANTIC VIEW` to be classified as `PARSE_NOT_OURS` (rc=2). DuckDB then surfaced its primary-parser error: `syntax error at or near "SEMANTIC"`.

Reported at https://github.com/anentropic/dbt-duckdb/blob/claude/fix-duckdb-extension-loading-hhWnp/notes/semantic_views_parser_comment_bug.md

## Failing-test-first

Commit `ca9197a` landed reproducer tests *before* the implementation, demonstrating the bug:

- `test/sql/quick_260430_vdz_leading_comments.test` — end-to-end via the C++ FFI + DuckDB parser hook. Covers leading block comments, leading line comments, mixed comments + whitespace, all DDL forms (DESCRIBE / SHOW / ALTER / DROP), comment-only rejection, and unterminated-block safety.
- `src/parse.rs` `mod tests` additions — 9 helper unit tests (`skip_lws_*`) and 8 integrated detection tests including a regression test that locks in v0.5.1 error-position byte-offset semantics.

## Fix

Commit `edf5196`. Added a private helper to `src/parse.rs`:

```rust
fn skip_leading_whitespace_and_comments(input: &str) -> usize
```

- Skips ASCII whitespace.
- Skips `-- ... \n` line comments (terminate at `\n` or EOF).
- Skips `/* ... */` block comments (NON-NESTING, Postgres-aligned).
- Unterminated `/*` consumes to EOF — keyword match fails, falls through to `PARSE_NOT_OURS`.
- Returns a **byte offset** into the original string (not a stripped slice) so existing `trim_offset` math used by `ParseError.position` continues to point at the correct byte in the original query (the v0.5.1 error-caret invariant).

Applied at five trimming sites previously using `query.trim()`:

| Site | Old | New |
|---|---|---|
| `validate_and_rewrite` | `query.trim()` + `trim_offset = ws_count` | `skip_leading_whitespace_and_comments(query)` + `trim_offset = lead` |
| `detect_ddl_kind` | `query.trim()` | `query[lead..].trim_end()` |
| `extract_ddl_name` | `query.trim()` | `query[lead..].trim_end()` |
| `detect_near_miss` | `query.trim()` + `trim_offset = ws_count` | `query[lead..].trim_end()` + `trim_offset = lead` |
| `rewrite_ddl` | `query.trim()` | `query[lead..].trim_end()` |

`match_keyword_prefix`, `detect_ddl_prefix`, and `body_parser.rs` were intentionally not touched — they operate on already-stripped slices and the new offset semantics propagate correctly.

## Quality Gate

`just test-all` (CLAUDE.md mandate) — green:
- cargo: 841 tests pass (749 lib + 5 + 36 + 42 + 5 + 3 + 1 doc)
- sqllogictest: 37 files pass (including new reproducer)
- DuckLake CI: 6 tests pass

## Files Changed

- `src/parse.rs` — added helper + 17 new tests + 5 site updates
- `test/sql/quick_260430_vdz_leading_comments.test` — new (sqllogictest reproducer)
- `test/sql/TEST_LIST` — registered new test file

## Notes for Future Work

- Trailing comments inside DDL bodies (`CREATE … METRICS (…) -- comment`) were not in scope. The body parser uses `trim()` and may not handle inline comments. Defer until reported.
- v0.7.2 hotfix branch — user will handle release tagging.
