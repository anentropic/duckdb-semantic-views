---
phase: quick-13
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/shim/shim.cpp
  - src/shim/shim.h
  - src/shim/mod.rs
  - src/lib.rs
  - src/catalog.rs
  - build.rs
  - Cargo.toml
  - README.md
  - MAINTAINER.md
  - TECH-DEBT.md
  - .planning/PROJECT.md
autonomous: true
requirements: [QUICK-13]

must_haves:
  truths:
    - "C++ shim files are deleted from the repository"
    - "Extension builds and loads without the C++ shim"
    - "All tests pass (cargo test, just test-sql, just test-ducklake-ci)"
    - "README accurately describes the project as pure Rust"
    - "No references to C++ shim remain in docs or source (except archived planning files)"
  artifacts:
    - path: "src/lib.rs"
      provides: "Extension entrypoint without shim call"
    - path: "build.rs"
      provides: "Build script with symbol visibility only (no cc compilation)"
    - path: "Cargo.toml"
      provides: "No cc build dependency"
    - path: "README.md"
      provides: "Updated building section (pure Rust, no C++ shim)"
  key_links:
    - from: "build.rs"
      to: "src/lib.rs"
      via: "symbol visibility linker flags still applied for extension feature"
      pattern: "cargo:rustc-link-arg"
---

<objective>
Remove the unused no-op C++ shim and update documentation to reflect that the extension is pure Rust.

The C++ shim (`src/shim/`) was originally created for parser hook integration but was discovered to be architecturally impossible (Python DuckDB uses `-fvisibility=hidden`). The shim has been a no-op stub since Phase 11 of v0.2.0. All DDL functionality is implemented in Rust. Removing the dead code eliminates the `cc` build dependency, simplifies the build, and removes the C++ toolchain requirement for contributors.

Purpose: Remove dead code and bring documentation in sync with reality.
Output: Cleaner codebase, updated README and docs, all tests passing.
</objective>

<context>
@README.md
@src/lib.rs
@build.rs
@Cargo.toml
@src/shim/shim.cpp
@src/shim/shim.h
@src/shim/mod.rs
@MAINTAINER.md
@TECH-DEBT.md
@.planning/PROJECT.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: Remove C++ shim code and build infrastructure</name>
  <files>src/shim/shim.cpp, src/shim/shim.h, src/shim/mod.rs, src/lib.rs, src/catalog.rs, build.rs, Cargo.toml</files>
  <action>
1. Delete `src/shim/shim.cpp`, `src/shim/shim.h`, and `src/shim/mod.rs` entirely.

2. In `src/lib.rs`:
   - Remove the `#[cfg(feature = "extension")] pub mod shim;` declaration (line 7).
   - Inside the `mod extension` block, remove the `unsafe extern "C"` block declaring `semantic_views_register_shim` (lines 300-309).
   - Remove the call to `semantic_views_register_shim` and the variables used only for it. Specifically, remove lines 455-461:
     ```rust
     // Call C++ shim (no-op in Phase 11 -- kept for ABI compatibility).
     let catalog_raw = Arc::as_ptr(&catalog_state) as *const std::ffi::c_void;
     let raw_persist_conn = persist_conn.unwrap_or(std::ptr::null_mut());
     unsafe {
         semantic_views_register_shim(db_handle.cast(), catalog_raw, raw_persist_conn);
     }
     ```
   - Update the comment on the `init_extension` function if it references the shim.

3. In `src/catalog.rs`:
   - Update the comment on line 165 from "FFI-callable catalog mutation functions -- called from the C++ parser hook scan function." to "FFI-callable catalog mutation functions." (remove the stale C++ reference).

4. In `build.rs`:
   - Remove the entire `cc::Build` block (lines 20-26) that compiles `src/shim/shim.cpp`.
   - Remove the `.include("duckdb_capi/")` reference.
   - Keep the symbol visibility section (Linux dynamic-list, macOS exported_symbols_list) -- this is still needed for the cdylib to restrict exported symbols.
   - Update the top-of-file comment to remove references to the C++ shim. New purpose: "Cargo build script -- restricts exported symbols when building the loadable extension."

5. In `Cargo.toml`:
   - Remove `cc = "1.2"` from `[build-dependencies]` section (line 63). If this was the only build dependency, remove the entire `[build-dependencies]` section.

6. Delete the `duckdb_capi/` directory entirely -- it was only used as an include path for the shim's `#include "duckdb.h"`. The `duckdb.h` header is not needed by the remaining build.rs (symbol visibility only uses string writes, no C compilation).
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo test 2>&1 | tail -5</automated>
  </verify>
  <done>
    - `src/shim/` directory does not exist
    - `duckdb_capi/` directory does not exist
    - `cc` not in Cargo.toml
    - No `semantic_views_register_shim` in src/lib.rs
    - `cargo test` passes
  </done>
</task>

<task type="auto">
  <name>Task 2: Update README and documentation</name>
  <files>README.md, MAINTAINER.md, TECH-DEBT.md, .planning/PROJECT.md</files>
  <action>
1. In `README.md`:
   - Line 170: Change "Rust with a C++ shim, built on the [DuckDB extension template for Rust](...)" to "Rust, built on the [DuckDB extension template for Rust](...)". Remove "with a C++ shim".

2. In `MAINTAINER.md`:
   - In "Architecture Overview > Source Tree": Remove `src/shim/` from the tree listing if present (check lines 56-99). Currently the source tree does not show shim/, so this may be a no-op. But verify and remove any shim references.
   - In the "How the Build Works" section (around line 168-174): If there's any mention of C++ compilation or the cc crate, remove it.
   - In the "Sidecar Persistence" section (around line 143-149): Update any references to the C++ shim pattern.
   - In "Worked Examples > Adding a New DDL Function": The example uses VScalar which is correct. No shim changes needed.
   - Search for any remaining "shim" or "C++" references and remove them. Do NOT modify archived planning files (.planning/milestones/).

3. In `TECH-DEBT.md`:
   - Update accepted decisions and deferred items that reference "C++ shim" to note the shim has been removed. Specifically:
     - Decision 1 (sidecar persistence): Update the Action field -- the C++ shim approach is no longer viable; note the shim was removed.
     - Decision 3 (explain interception): Update the Action -- note C++ shim was removed; this requires a different approach if ever pursued.
     - Deferred items QUERY-V2-01, QUERY-V2-03, and sidecar replacement: Update to note the C++ shim approach is not available; these remain architecturally blocked.

4. In `.planning/PROJECT.md`:
   - Line 61: Update tech stack from "Rust, C++ (shim), duckdb-rs 1.4.4, cc crate, serde_json, strsim, proptest" to "Rust, duckdb-rs 1.4.4, serde_json, strsim, proptest".
   - Line 75: Update from "Rust + C++ -- Rust for extension logic, C++ shim for pragma callbacks" to "Rust -- pure Rust extension, no C++ shim needed".
   - Line 28: Update the bullet "C++ shim infrastructure with feature-gated cc crate compilation and symbol visibility" to "Symbol visibility for extension builds (feature-gated in build.rs)".
   - Remove or update any other C++ shim references, keeping the historical context about WHY it was removed (Python DuckDB -fvisibility=hidden).
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && ! grep -rn 'C++ shim\|shim\.cpp\|shim\.h\|cc crate' README.md MAINTAINER.md src/ build.rs Cargo.toml 2>/dev/null; echo "exit: $?"</automated>
  </verify>
  <done>
    - README says "Rust, built on" (no C++ shim mention)
    - No active source or build files reference the shim
    - TECH-DEBT.md reflects shim removal
    - PROJECT.md tech stack updated
  </done>
</task>

<task type="auto">
  <name>Task 3: Full test suite verification</name>
  <files></files>
  <action>
Run the full test suite per CLAUDE.md quality gate:

```bash
just test-all
```

This runs: cargo test (Rust unit + proptest), just test-sql (SQL logic tests via sqllogictest), and just test-ducklake-ci (DuckLake integration tests).

The SQL logic tests are critical here because they exercise the full extension load path -- if removing the shim broke the cdylib linking or symbol visibility, `just test-sql` will catch it.

If `just test-sql` fails with linker errors, the issue is likely that `build.rs` still references `cc` or the shim. Verify `build.rs` only contains the symbol visibility section.

If `just test-sql` fails with "symbol not found" at LOAD time, verify the exported symbols list in build.rs still includes `_semantic_views_init_c_api`.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && just test-all 2>&1 | tail -20</automated>
  </verify>
  <done>
    - `just test-all` exits 0
    - All test suites pass: cargo test, just test-sql, just test-ducklake-ci
  </done>
</task>

</tasks>

<verification>
1. `cargo test` passes (Rust unit + proptest)
2. `just build && just test-sql` passes (extension loads and DDL/query work)
3. `just test-ducklake-ci` passes (integration)
4. `grep -rn 'shim' src/ build.rs Cargo.toml` returns no results
5. `ls src/shim/ duckdb_capi/` returns "No such file or directory"
6. README building section says "Rust, built on" without C++ mention
</verification>

<success_criteria>
- C++ shim completely removed (files, build infra, extern declarations, call sites)
- `duckdb_capi/` vendored headers removed
- `cc` build dependency removed from Cargo.toml
- Extension builds, loads, and passes all tests without the shim
- README and documentation accurately describe pure Rust architecture
- No stale C++ shim references in active source or docs (archived planning files excluded)
</success_criteria>

<output>
After completion, create `.planning/quick/13-update-readme-and-remove-unused-c-shim/13-SUMMARY.md`
</output>
