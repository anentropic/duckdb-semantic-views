---
phase: quick-6
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/ddl/define.rs
  - src/query/table_function.rs
  - build.rs
autonomous: true
requirements: [CI-FMT, CI-LINK]
must_haves:
  truths:
    - "cargo fmt --check passes with zero diffs"
    - "build.rs generates a named ELF version script that does not conflict with rustc's own cdylib version script"
    - "cargo test passes (no regressions from changes)"
  artifacts:
    - path: "src/ddl/define.rs"
      provides: "rustfmt-compliant closure on line 154"
      contains: ".map(|t| crate::query::table_function::normalize_type_id"
    - path: "src/query/table_function.rs"
      provides: "rustfmt-compliant unsafe block on line 580"
      contains: "unsafe { ffi::duckdb_get_type_id"
    - path: "build.rs"
      provides: "Named version tag in ELF version script"
      contains: "SEMANTIC_VIEWS"
  key_links:
    - from: "build.rs"
      to: "linker invocation"
      via: "cargo:rustc-link-arg=-Wl,--version-script"
      pattern: "SEMANTIC_VIEWS.*global.*semantic_views_init_c_api"
---

<objective>
Fix the two CI failures on main: (1) cargo fmt check failing due to multiline closures/blocks
that rustfmt wants collapsed, and (2) linux_arm64 linker error where our anonymous ELF version
script conflicts with rustc's own cdylib version script.

Purpose: Unblock the CI pipeline so all platforms build and pass quality checks.
Output: Three modified files committed to main, CI green.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@build.rs
@src/ddl/define.rs
@src/query/table_function.rs

<interfaces>
<!-- No new interfaces created or consumed. All changes are in-place fixes. -->
</interfaces>

CI failure details:

1. Code Quality workflow (cargo fmt --check):
   - src/ddl/define.rs:154-159: multiline `.map(|t| { ... })` closure should be single-line
   - src/query/table_function.rs:580-584: multiline `unsafe { ... }` block should be single-line

2. Main Distribution Pipeline (linux_arm64 linker error):
   - Error: "anonymous version tag cannot be combined with other version tags"
   - Root cause: build.rs writes an anonymous ELF version script (no version tag name),
     but rustc also generates its own --version-script for cdylib targets. GNU ld
     (gcc-toolset-14 in manylinux_2_28) rejects combining anonymous + named version tags.
   - The linker invocation shows TWO --version-script flags:
     `-Wl,--version-script=/tmp/rustc.../list` (rustc's) and
     `-Wl,--version-script=.../out/semantic_views.map` (ours)
   - Fix: add a named version tag to our script so both are named tags, which GNU ld allows.
</context>

<tasks>

<task type="auto">
  <name>Task 1: Fix cargo fmt violations</name>
  <files>src/ddl/define.rs, src/query/table_function.rs</files>
  <action>
Run `cargo fmt` to auto-fix both formatting issues:

1. In src/ddl/define.rs around line 154-159, the multiline closure:
   ```rust
   .map(|t| {
       crate::query::table_function::normalize_type_id(*t as u32)
   })
   ```
   becomes single-line:
   ```rust
   .map(|t| crate::query::table_function::normalize_type_id(*t as u32))
   ```

2. In src/query/table_function.rs around line 580-584, the multiline unsafe block:
   ```rust
   unsafe {
       ffi::duckdb_get_type_id(col_logical_types[col_idx]) as u32
   }
   ```
   becomes single-line:
   ```rust
   unsafe { ffi::duckdb_get_type_id(col_logical_types[col_idx]) as u32 }
   ```

Simply running `cargo fmt` will apply both changes. Verify with `cargo fmt --check` afterward.
  </action>
  <verify>
    <automated>cargo fmt --check</automated>
  </verify>
  <done>cargo fmt --check exits 0 with no diff output</done>
</task>

<task type="auto">
  <name>Task 2: Fix ELF version script to use named version tag</name>
  <files>build.rs</files>
  <action>
In build.rs, modify the Linux ELF version script generation (around line 42) to use a named
version tag instead of an anonymous one.

Current (anonymous, line 42):
```rust
std::fs::write(
    &map_path,
    "{\n  global:\n    semantic_views_init_c_api;\n  local: *;\n};\n",
)
```

Change to (named version tag):
```rust
std::fs::write(
    &map_path,
    "SEMANTIC_VIEWS_1.0 {\n  global:\n    semantic_views_init_c_api;\n  local: *;\n};\n",
)
```

WHY: rustc generates its own `--version-script` for cdylib targets with a named version tag.
GNU ld (used in the manylinux_2_28 Docker build environment) rejects combining an anonymous
version tag with a named one. By giving our version script a name ("SEMANTIC_VIEWS_1.0"),
both scripts have named tags and GNU ld merges them correctly.

The macOS (`-exported_symbols_list`) and Windows (`__declspec(dllexport)`) paths are unaffected
by this change.

Also update the comment above the version script to explain the named tag requirement:
```rust
// ELF version script: only the Rust entry point is globally visible.
// MUST use a named version tag (not anonymous) because rustc also emits a
// --version-script for cdylib targets; GNU ld rejects mixing anonymous
// and named version tags.
```
  </action>
  <verify>
    <automated>cargo test 2>&1 | tail -5</automated>
  </verify>
  <done>build.rs generates "SEMANTIC_VIEWS_1.0 { ... };" instead of "{ ... };". cargo test passes (build.rs is not exercised by cargo test since it requires --features extension, but no regressions introduced). The fix will be validated when CI runs linux_arm64 after push.</done>
</task>

<task type="auto">
  <name>Task 3: Run full local test suite to confirm no regressions</name>
  <files></files>
  <action>
Run the full local test suite per CLAUDE.md quality gate:

1. `cargo test` -- Rust unit + proptest + doc tests
2. `just build` -- rebuild extension binary with the modified build.rs
3. `just test-sql` -- SQL logic tests via sqllogictest runner

All three must pass. If any fail, investigate whether the formatting or build.rs changes
caused a regression (they should not -- formatting is whitespace-only and the version script
change only affects the extension feature which is not used in cargo test).
  </action>
  <verify>
    <automated>cargo test 2>&1 | tail -3 && just build 2>&1 | tail -3 && just test-sql 2>&1 | tail -5</automated>
  </verify>
  <done>All three commands pass: cargo test (136+ tests), just build (extension binary produced), just test-sql (all SQL logic tests pass)</done>
</task>

</tasks>

<verification>
1. `cargo fmt --check` exits 0
2. `grep "SEMANTIC_VIEWS_1.0" build.rs` returns a match
3. `cargo test` passes all tests
4. `just build && just test-sql` passes
5. After push, CI Code Quality workflow passes (cargo fmt + clippy)
6. After push, CI Main Distribution Pipeline linux_arm64 build succeeds
</verification>

<success_criteria>
- cargo fmt --check produces zero diffs
- build.rs contains named version tag "SEMANTIC_VIEWS_1.0" in the ELF version script
- Full local test suite passes (cargo test + just test-sql)
- On next CI run: Code Quality and Main Distribution Pipeline both pass
</success_criteria>

<output>
After completion, create `.planning/quick/6-fix-all-outstanding-ci-failures/6-SUMMARY.md`
</output>
