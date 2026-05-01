---
quick_id: 260430-vdz
description: Fix parser hook to skip leading SQL comments (-- and /* */) before prefix matching
mode: quick
---

# Quick Task 260430-vdz: Parser Hook Comment-Stripping Fix

## Goal

Fix the bug reported at https://github.com/anentropic/dbt-duckdb/blob/claude/fix-duckdb-extension-loading-hhWnp/notes/semantic_views_parser_comment_bug.md

The semantic_views parser hook (`detect_ddl_prefix` and friends in `src/parse.rs`) does case-insensitive prefix matching on the raw query but does not strip leading SQL comments. dbt-duckdb (and many other tools) prepend `/* {"app": "dbt", ...} */` annotations to every statement, causing `CREATE/ALTER/DROP/SHOW SEMANTIC VIEW` DDL to be classified as `PARSE_NOT_OURS`. Result: `Parser Error: syntax error at or near "SEMANTIC"`.

Fix: add a `skip_leading_whitespace_and_comments(&str) -> usize` helper, apply at five trimming sites, preserve byte offsets so error-position carets (v0.5.1) still point into the original query.

## Constraints

- Stay on `hotfix/0.7.2` branch (already checked out).
- No worktree isolation. No parallel builds. Foreground only.
- Do NOT bump version numbers; user handles release tagging.
- Failing-test-first: tests committed BEFORE the implementation that makes them pass.
- Quality gate: `just test-all` must pass.

## Tasks

### Task 1: Write failing tests (sqllogictest + Rust unit tests)

**files:**
- `test/sql/quick_260430_vdz_leading_comments.test` (new)
- `src/parse.rs` (extend existing `#[cfg(test)] mod tests`)

**action:**
1. Create new sqllogictest file `test/sql/quick_260430_vdz_leading_comments.test` covering:
   - Baseline: plain DDL works
   - Baseline: leading whitespace works
   - Bug repro: leading `/* ... */` block comment before `CREATE OR REPLACE SEMANTIC VIEW` (dbt-style annotation)
   - Bug repro: leading `-- ...\n` line comment before `CREATE`
   - Mixed comments and whitespace
   - Other DDL forms with leading comments: `DESCRIBE`, `SHOW`, `ALTER`, `DROP`
   - Comment-only statement is NOT classified as semantic-view DDL
   - Unterminated block comment does NOT panic
   See RESEARCH.md section "Failing sqllogictest" for the exact file body.
2. Add Rust unit tests to the `mod tests` block in `src/parse.rs`:
   - Helper tests (will fail to compile until Task 2 adds the helper): `skip_lws_empty`, `skip_lws_only_whitespace`, `skip_lws_line_comment`, `skip_lws_block_comment`, `skip_lws_multiple_comments_and_ws`, `skip_lws_block_does_not_nest`, `skip_lws_unterminated_block_consumes_to_eof`, `skip_lws_no_leading_match`, `skip_lws_dash_dash_at_eof`
   - Integrated detection tests: `detect_create_with_leading_block_comment`, `detect_create_with_leading_line_comment`, `detect_create_or_replace_with_dbt_style_annotation`, `detect_other_ddl_forms_with_leading_comment`, `comment_only_is_not_semantic_view_ddl`, `validate_and_rewrite_with_leading_comment_succeeds`, `extract_ddl_name_with_leading_comment`, `error_position_accounts_for_leading_comment`
   See RESEARCH.md sections "Rust unit tests for the helper" and "Rust unit tests for the integrated detection layer" for exact test code.

**verify:**
- `just build` succeeds (the test file references the helper, but `cfg(test)` items only fail at test compile time — confirm the build still produces the extension binary).
- `cargo test --no-run` will fail to compile because the helper doesn't exist yet — that's expected and proves the tests are wired up. Confirm the failure is "cannot find function `skip_leading_whitespace_and_comments`".
- Alternative if test compile failure is undesirable in the commit: gate the helper unit tests with `#[cfg(all(test, FALSE))]` momentarily; cleaner is to commit broken-on-purpose. Choose: commit broken-on-purpose with a comment marking the failing-test-first intent, OR temporarily `#[ignore]` and remove in Task 2.
  - **Decision:** Commit failing tests broken-on-purpose. The task 1 commit message will say "tests: add failing reproducer for parser comment bug (260430-vdz)". The task 2 commit makes them pass.
- For the sqllogictest: `just test-sql` runs against the *currently built* extension. If we build before the fix, the new test cases that should pass will fail with the original `Parser Error: syntax error at or near "SEMANTIC"` — that confirms the repro.

**done:**
- New sqllogictest file committed.
- Rust unit tests committed in `src/parse.rs` test module.
- Atomic commit: `tests(parse): add failing reproducers for leading-comment bug (260430-vdz)`.

### Task 2: Implement comment-stripping helper and apply at five sites

**files:**
- `src/parse.rs` (add helper + modify five trimming sites)

**action:**
1. Add private helper `skip_leading_whitespace_and_comments(input: &str) -> usize` near `match_keyword_prefix` in `src/parse.rs`. Implementation per RESEARCH.md "Helper function" section:
   - Skip ASCII whitespace
   - Skip `-- ... \n` line comments (terminate at `\n` or EOF)
   - Skip `/* ... */` block comments (NON-NESTING, Postgres semantics)
   - Unterminated block comment consumes to EOF
   - Returns byte offset into original input
2. Apply the helper at five trimming sites. Replace each occurrence of:
   ```rust
   let trimmed = query.trim();
   let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
   // possibly: let trim_offset = query.len() - query.trim_start().len();
   ```
   with the offset-preserving form (see RESEARCH.md "Apply at five sites"):
   ```rust
   let lead = skip_leading_whitespace_and_comments(query);
   let trimmed = query[lead..].trim_end();
   let trimmed_no_semi = trimmed.trim_end_matches(';').trim();
   let trim_offset = lead;
   ```
   Sites:
   - `validate_and_rewrite` (~src/parse.rs:824) — primary site, preserves `trim_offset` for error-position math
   - `detect_ddl_kind` (~src/parse.rs:179)
   - `extract_ddl_name` (~src/parse.rs:669)
   - `detect_near_miss` (~src/parse.rs:775)
   - `rewrite_ddl` (~src/parse.rs:586)
3. Do NOT modify `match_keyword_prefix`, `detect_ddl_prefix`, or `body_parser.rs` — they operate on already-stripped slices and the byte-offset semantics carry through.

**verify:**
- `cargo test` — green (all helper unit tests + integrated detection tests pass)
- `just build` — extension binary builds cleanly (foreground)
- `just test-sql` — sqllogictest passes including the new `quick_260430_vdz_leading_comments.test` file
- `just test-all` — full quality gate green (Rust unit + proptest + sqllogictest + DuckLake)
- Critical regression check: `error_position_accounts_for_leading_comment` test must pass — proves byte-offset preservation works.

**done:**
- Helper added, all five sites updated.
- All previously-failing tests from Task 1 now pass.
- `just test-all` green.
- Atomic commit: `fix(parse): skip leading SQL comments before DDL prefix match (260430-vdz)`.

## must_haves

### truths
- Bug repro from BUG-REPORT.md: `p.execute("/* hi */ " + DDL)` and `p.execute("-- hi\n" + DDL)` must succeed where `DDL = "CREATE OR REPLACE SEMANTIC VIEW sv AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.x AS xx) METRICS (t.y AS sum(y))"`.
- Comment-only statements remain `PARSE_NOT_OURS` (DuckDB falls back to its own parser error).
- Block comments do NOT nest (Postgres-aligned).
- Error-position byte offsets in `ParseError.position` continue to reference the *original* query string after a leading comment.
- All existing tests continue to pass — no regression on non-commented DDL or whitespace-only-prefix DDL.

### artifacts
- `test/sql/quick_260430_vdz_leading_comments.test` exists with comment-prefix DDL cases.
- New `mod tests` entries in `src/parse.rs` covering helper + integrated detection + error-position regression.
- Helper function `skip_leading_whitespace_and_comments` exists in `src/parse.rs`.
- Five trimming sites updated to use the helper.
- `just test-all` exits 0.

### key_links
- Bug report: `.planning/quick/260430-vdz-review-bug-report-semantic-views-parser-/BUG-REPORT.md`
- Research: `.planning/quick/260430-vdz-review-bug-report-semantic-views-parser-/260430-vdz-RESEARCH.md`
- Source: `src/parse.rs:62, 98, 179, 586, 669, 775, 824, 1454, 1549`
- C++ shim (no changes needed): `cpp/src/shim.cpp:72, 146, 240`
- Quality gate: `CLAUDE.md` — `just test-all`
