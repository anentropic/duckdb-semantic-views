---
phase: quick-4
plan: 01
type: execute
wave: 1
depends_on: []
files_modified: [src/ddl/define.rs, src/ddl/describe.rs, src/ddl/drop.rs, src/ddl/list.rs, src/query/error.rs, src/query/explain.rs, src/query/table_function.rs]
autonomous: true
requirements: [QUICK-4]

must_haves:
  truths:
    - "Code Quality CI run 22528925235 result is checked and understood"
    - "If coverage is below 80%, tests are added to bring it above threshold"
    - "All Code Quality CI steps pass on main branch"
  artifacts:
    - path: ".github/workflows/CodeQuality.yml"
      provides: "Coverage threshold enforcement"
      contains: "fail-under-lines 80"
  key_links:
    - from: "cargo llvm-cov nextest"
      to: "src/**/*.rs tests/**/*.rs"
      via: "coverage instrumentation"
      pattern: "fail-under-lines 80"
---

<objective>
Check the result of Code Quality CI run 22528925235 (triggered by commit d080d2b). If all steps pass, record the result. If coverage fails (the `cargo llvm-cov nextest --fail-under-lines 80` step), diagnose which modules are under-covered and add unit tests to bring coverage above 80%.

Purpose: Ensure the CI pipeline is fully green on main after the quick-3 CI fixes.
Output: Either a verified green CI result, or new tests that fix coverage + a re-triggered green CI run.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@.github/workflows/CodeQuality.yml
@src/lib.rs
@src/model.rs
@src/expand.rs
@src/catalog.rs
</context>

<tasks>

<task type="auto">
  <name>Task 1: Check CI run result and diagnose any failures</name>
  <files></files>
  <action>
Wait for CI run 22528925235 to complete (poll with `gh run view 22528925235 --json status,conclusion` every 30-60 seconds, max 15 minutes).

Once complete, check the conclusion:

1. If conclusion is "success": Record result, skip Task 2, proceed to summary.

2. If conclusion is "failure": Identify which step failed using `gh run view 22528925235 --json jobs`. Get the failing step's logs with `gh run view 22528925235 --log-failed`.

   - If Clippy fails: Read clippy warnings from logs, fix the lints in affected source files.
   - If cargo-deny fails: Read deny output, update deny.toml allow list if license issue.
   - If coverage fails: Run `cargo llvm-cov nextest --fail-under-lines 80` locally (or without --fail-under-lines to see actual percentage). Identify which files in src/ have low coverage. The following files likely have NO unit tests currently: src/ddl/define.rs, src/ddl/describe.rs, src/ddl/drop.rs, src/ddl/list.rs, src/query/error.rs, src/query/explain.rs, src/query/table_function.rs, src/lib.rs. Focus on the modules contributing most to the gap.

3. If conclusion is "cancelled": Check if a newer run superseded it. Find and monitor the latest Code Quality run instead.
  </action>
  <verify>CI run status is known and recorded. If failure, root cause is identified.</verify>
  <done>CI run 22528925235 (or its superseding run) has a known conclusion, and any failure root cause is documented.</done>
</task>

<task type="auto">
  <name>Task 2: Fix coverage or other CI failures (conditional)</name>
  <files>src/ddl/define.rs, src/ddl/describe.rs, src/ddl/drop.rs, src/ddl/list.rs, src/query/error.rs, src/query/explain.rs, src/query/table_function.rs</files>
  <action>
SKIP THIS TASK if Task 1 found CI passed (all green).

If coverage is below 80%:
1. Run `cargo llvm-cov nextest` locally to get the per-file coverage report.
2. Identify the files with lowest coverage percentages.
3. Add unit tests to the files that are easiest to cover meaningfully:
   - src/model.rs and src/catalog.rs already have tests -- check if they need more.
   - src/ddl/*.rs functions: Test the DDL scalar functions (define, describe, drop, list) with mock/test inputs. These are likely serialization/deserialization + catalog operations that can be unit-tested.
   - src/query/error.rs: Test error Display implementations and error construction.
   - src/query/explain.rs: Test explain output formatting.
   - src/expand.rs already has extensive tests (37 test annotations) -- likely well covered.
4. Add tests as `#[cfg(test)] mod tests { ... }` blocks in each file, or in the existing test modules.
5. Re-run `cargo llvm-cov nextest --fail-under-lines 80` locally to confirm coverage passes.
6. Commit the new tests.
7. Push to main and verify the new CI run passes.

If Clippy or cargo-deny failed:
1. Apply the specific fix identified in Task 1.
2. Run the failing check locally to confirm the fix.
3. Commit, push, verify CI.

Important constraints:
- Do NOT lower the 80% coverage threshold.
- Do NOT add `#[cfg(not(tarpaulin_include))]` or similar coverage exclusion attributes.
- Tests should be meaningful, not just coverage padding. Test actual behavior and edge cases.
- Follow existing test patterns in src/model.rs and src/expand.rs.
  </action>
  <verify>
Run locally: `cargo llvm-cov nextest --fail-under-lines 80` exits 0.
Run locally: `cargo clippy -- -D warnings` exits 0.
After push: `gh run list --limit 1 --json conclusion` shows success.
  </verify>
  <done>Code Quality CI pipeline is fully green on main. Coverage is at or above 80%. All clippy and cargo-deny checks pass.</done>
</task>

</tasks>

<verification>
- `gh run view {latest_run_id} --json conclusion` returns `{"conclusion":"success"}`
- All Code Quality steps (fmt, clippy, cargo-deny, coverage) show green
</verification>

<success_criteria>
Code Quality CI is fully green on main branch. If fixes were needed, they are committed and pushed with a passing CI run to prove it.
</success_criteria>

<output>
After completion, create `.planning/quick/4-check-ci-results-and-fix-coverage-if-nee/4-SUMMARY.md`
</output>
