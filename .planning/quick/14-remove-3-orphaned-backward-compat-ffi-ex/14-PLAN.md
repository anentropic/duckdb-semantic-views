---
phase: quick-14
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/parse.rs
  - cpp/src/shim.cpp
autonomous: true
requirements: [CLEANUP-01]
must_haves:
  truths:
    - "No orphaned FFI exports remain in parse.rs"
    - "No stale extern C declarations remain in shim.cpp"
    - "No backward-compat wrapper functions remain in parse.rs"
    - "Module doc comment accurately describes current FFI surface"
    - "All existing tests still pass (wrappers replaced with canonical functions)"
  artifacts:
    - path: "src/parse.rs"
      provides: "Clean FFI surface with only sv_validate_ddl_rust, sv_rewrite_ddl_rust as active entry points"
    - path: "cpp/src/shim.cpp"
      provides: "C++ shim with only used extern C declarations"
  key_links:
    - from: "cpp/src/shim.cpp"
      to: "src/parse.rs"
      via: "extern C FFI declarations match #[no_mangle] exports"
      pattern: "sv_validate_ddl_rust|sv_rewrite_ddl_rust"
---

<objective>
Remove 3 orphaned backward-compat FFI exports and their associated test wrappers from the codebase.

Purpose: Eliminate dead code that creates a false impression of active entry points, reducing maintenance burden and reader confusion.
Output: Clean parse.rs with accurate doc comment, no orphaned FFI exports, no wrapper functions, and all tests migrated to canonical function names.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/parse.rs
@cpp/src/shim.cpp
</context>

<tasks>

<task type="auto">
  <name>Task 1: Remove orphaned FFI exports and C++ declaration</name>
  <files>src/parse.rs, cpp/src/shim.cpp</files>
  <action>
In src/parse.rs:

1. DELETE the `sv_parse_rust` FFI function (lines ~667-682). This `#[cfg(feature = "extension")] #[no_mangle] pub extern "C" fn sv_parse_rust(...)` is declared in shim.cpp but never called -- replaced by `sv_validate_ddl_rust` in Phase 21.

2. DELETE the `sv_execute_ddl_rust` FFI function (lines ~854-919). This `#[cfg(feature = "extension")] #[no_mangle] pub extern "C" fn sv_execute_ddl_rust(...)` has no C++ caller at all -- DDL execution moved to C++ side in shim.cpp `sv_ddl_bind`.

In cpp/src/shim.cpp:

3. DELETE the stale `sv_parse_rust` extern declaration (line 34) and its comment (line 33). Keep the `sv_rewrite_ddl_rust` and `sv_validate_ddl_rust` declarations which ARE called.
  </action>
  <verify>
    <automated>cargo build --features extension 2>&1 | tail -5</automated>
  </verify>
  <done>sv_parse_rust and sv_execute_ddl_rust no longer exist in Rust source; sv_parse_rust declaration removed from shim.cpp; extension still compiles.</done>
</task>

<task type="auto">
  <name>Task 2: Remove backward-compat wrappers, migrate tests, fix doc comment</name>
  <files>src/parse.rs</files>
  <action>
1. DELETE the `detect_create_semantic_view` wrapper function (lines ~83-88). It is a one-line delegation to `detect_semantic_view_ddl`.

2. DELETE the `rewrite_ddl_to_function_call` wrapper function (lines ~236-240). It is a one-line delegation to `rewrite_ddl`.

3. MIGRATE the "Backward compatibility" test section (lines ~1233-1318) that calls `detect_create_semantic_view`: replace every call to `detect_create_semantic_view(...)` with `detect_semantic_view_ddl(...)`. Update the section comment from "Backward compatibility: existing tests using old function names" to "Additional detect_semantic_view_ddl coverage (legacy test cases)".

4. MIGRATE the "rewrite_ddl_to_function_call backward-compat tests" section (lines ~1381-1418) that calls `rewrite_ddl_to_function_call`: replace every call to `rewrite_ddl_to_function_call(...)` with `rewrite_ddl(...)`. Update the section comment from "rewrite_ddl_to_function_call backward-compat tests" to "Additional rewrite_ddl coverage (legacy test cases)".

5. UPDATE the module-level doc comment (lines 1-7) to accurately describe the current FFI surface. Replace:
```
// Parse detection for semantic view DDL statements.
//
// This module provides two layers:
// 1. Pure detection/rewrite functions (`detect_semantic_view_ddl`, `rewrite_ddl`,
//    `extract_ddl_name`) testable under `cargo test` without the extension feature.
// 2. FFI entry points (`sv_parse_rust`, `sv_execute_ddl_rust`) that wrap detection
//    in `catch_unwind` for panic safety, feature-gated on `extension`.
```
With:
```
// Parse detection and rewriting for semantic view DDL statements.
//
// This module provides two layers:
// 1. Pure detection/rewrite functions (`detect_semantic_view_ddl`, `rewrite_ddl`,
//    `extract_ddl_name`, `validate_and_rewrite`) testable under `cargo test`
//    without the extension feature.
// 2. FFI entry points (`sv_validate_ddl_rust`, `sv_rewrite_ddl_rust`)
//    feature-gated on `extension`, with `catch_unwind` for panic safety.
```
  </action>
  <verify>
    <automated>cargo test -- parse 2>&1 | tail -20</automated>
  </verify>
  <done>No backward-compat wrappers remain. All legacy tests call canonical function names. Module doc comment lists only the 2 active FFI entry points (sv_validate_ddl_rust, sv_rewrite_ddl_rust). Full test suite passes.</done>
</task>

</tasks>

<verification>
Run the full quality gate to confirm nothing is broken:

```bash
just test-all
```

Confirm no references to removed symbols remain:

```bash
grep -rn 'sv_parse_rust\|sv_execute_ddl_rust\|detect_create_semantic_view\|rewrite_ddl_to_function_call' src/ cpp/
```

Expected: zero matches.
</verification>

<success_criteria>
- `just test-all` passes (cargo test + sqllogictest + ducklake CI)
- Zero grep hits for `sv_parse_rust`, `sv_execute_ddl_rust`, `detect_create_semantic_view`, `rewrite_ddl_to_function_call` in src/ and cpp/
- Module doc comment in src/parse.rs references only `sv_validate_ddl_rust` and `sv_rewrite_ddl_rust` as FFI entry points
</success_criteria>

<output>
After completion, create `.planning/quick/14-remove-3-orphaned-backward-compat-ffi-ex/14-SUMMARY.md`
</output>
