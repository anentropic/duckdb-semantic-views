---
phase: quick-7
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - tests/vector_reference_test.rs
autonomous: true
requirements: []
must_haves:
  truths:
    - "cargo fmt --check exits 0 (no formatting diffs)"
    - "cargo test still passes after formatting"
  artifacts:
    - path: "tests/vector_reference_test.rs"
      provides: "Correctly formatted vector reference integration test"
  key_links: []
---

<objective>
Fix rustfmt CI failure in tests/vector_reference_test.rs.

Purpose: The Code Quality workflow fails because `cargo fmt --check` detects formatting diffs in `tests/vector_reference_test.rs`. Three blocks have line-length / indentation issues that rustfmt wants to reformat.
Output: Properly formatted test file that passes `cargo fmt --check`.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@CLAUDE.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: Run cargo fmt and verify all checks pass</name>
  <files>tests/vector_reference_test.rs</files>
  <action>
Run `cargo fmt` to auto-format the codebase. The only file with diffs is `tests/vector_reference_test.rs` which has three formatting issues:
1. Line 47-49: `execute_sql_raw` call chain has unnecessary line break before the function call
2. Line 57: `duckdb_column_logical_type` args should be split across lines (line too long)
3. Line 70: `duckdb_create_data_chunk` call is too long for one line

After formatting, verify:
- `cargo fmt --check` exits 0 (no remaining diffs)
- `cargo test` passes (formatting is cosmetic, but confirm no breakage)
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo fmt --check && cargo test 2>&1 | tail -5</automated>
  </verify>
  <done>cargo fmt --check returns 0, cargo test passes, tests/vector_reference_test.rs is properly formatted</done>
</task>

</tasks>

<verification>
- `cargo fmt --check` exits 0
- `cargo test` passes (all unit + proptest + doc tests)
</verification>

<success_criteria>
The Code Quality CI workflow formatting check will pass on the next push. The test file is properly formatted with no functional changes.
</success_criteria>

<output>
After completion, create `.planning/quick/7-check-gh-run-list-and-fix/7-SUMMARY.md`
</output>
