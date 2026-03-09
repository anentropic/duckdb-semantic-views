---
phase: 15-fix-ci-amalgamation
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - Makefile
  - .github/workflows/PullRequestCI.yml
  - justfile
autonomous: true
requirements: []
must_haves:
  truths:
    - "Main Extension Distribution Pipeline passes on all platforms (linux_amd64, linux_arm64, osx_amd64, osx_arm64, windows_amd64)"
    - "PullRequestCI ducklake-ci-check job can build the extension without amalgamation files in repo"
    - "just update-headers reads version from .duckdb-version (not by parsing Makefile)"
  artifacts:
    - path: "Makefile"
      provides: "Amalgamation auto-download target with build dependency"
      contains: "ensure_amalgamation"
    - path: ".github/workflows/PullRequestCI.yml"
      provides: "Amalgamation download step before cargo build"
      contains: "libduckdb-src.zip"
    - path: "justfile"
      provides: "Fixed update-headers reading .duckdb-version"
      contains: "cat .duckdb-version"
  key_links:
    - from: "Makefile (build_extension_library_release)"
      to: "ensure_amalgamation target"
      via: "make prerequisite"
      pattern: "build_extension_library_release:.*ensure_amalgamation"
    - from: "Makefile (ensure_amalgamation)"
      to: ".duckdb-version"
      via: "TARGET_DUCKDB_VERSION variable"
      pattern: "TARGET_DUCKDB_VERSION"
---

<objective>
Fix the failing Main Extension Distribution Pipeline CI by adding auto-download of DuckDB amalgamation files.

Purpose: Since v0.5.0 introduced C++ shim compilation (build.rs compiles cpp/include/duckdb.cpp), CI fails because the amalgamation files (~25MB) are gitignored and not present after checkout. The reusable workflow calls `make configure_ci && make release` which we cannot modify, but we CAN add a prerequisite target to our Makefile that downloads the amalgamation when missing.

Output: Working CI on all platforms; fixed PullRequestCI ducklake-ci-check job; cleaned up justfile version source.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@Makefile
@justfile
@build.rs
@.duckdb-version
@.github/workflows/MainDistributionPipeline.yml
@.github/workflows/PullRequestCI.yml
@.gitignore
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add amalgamation auto-download to Makefile and fix justfile</name>
  <files>Makefile, justfile</files>
  <action>
**Makefile changes:**

Add an `ensure_amalgamation` target that downloads the DuckDB amalgamation zip from GitHub releases ONLY when `cpp/include/duckdb.cpp` does not exist. This must work on Linux (CI uses ubuntu), macOS (CI uses macos), and Windows (CI uses windows).

Place this target AFTER the `UNSTABLE_C_API_FLAG` override line (around line 19) and BEFORE the `configure:` target:

```makefile
# Auto-download DuckDB amalgamation (gitignored, ~25MB) if not present.
# CI checks out the repo without these files; local devs fetch via `just update-headers`.
# The version comes from .duckdb-version via TARGET_DUCKDB_VERSION.
AMALGAMATION_URL=https://github.com/duckdb/duckdb/releases/download/$(TARGET_DUCKDB_VERSION)/libduckdb-src.zip

cpp/include/duckdb.cpp:
	@echo "Downloading DuckDB $(TARGET_DUCKDB_VERSION) amalgamation..."
	@mkdir -p cpp/include
	@curl -sL -o /tmp/libduckdb-src.zip "$(AMALGAMATION_URL)"
	@unzip -o -j /tmp/libduckdb-src.zip "duckdb.hpp" "duckdb.cpp" -d cpp/include/
	@rm -f /tmp/libduckdb-src.zip
	@echo "Downloaded cpp/include/duckdb.{hpp,cpp}"

ensure_amalgamation: cpp/include/duckdb.cpp
```

Use a file-based target (`cpp/include/duckdb.cpp:`) so Make's own dependency resolution handles the "only if missing" check. The phony `ensure_amalgamation` alias makes it readable as a prerequisite.

Then modify the two overridden build targets to add `ensure_amalgamation` as a prerequisite:

```makefile
build_extension_library_debug: check_configure ensure_amalgamation
build_extension_library_release: check_configure ensure_amalgamation
```

Keep the recipe bodies (cargo build + copy lines) unchanged. Only add `ensure_amalgamation` to the prerequisite list.

IMPORTANT: Use `/tmp/libduckdb-src.zip` for the temp file path. On Windows CI, the reusable workflow runs in a Bash shell (Git Bash / MSYS2) where `/tmp` is valid. On Linux/macOS it's native. This matches the existing `justfile update-headers` pattern.

**justfile changes:**

Fix the `update-headers` recipe (around line 114-121). Currently it reads the version by parsing Makefile:
```
VER=$$(grep '^TARGET_DUCKDB_VERSION=' Makefile | cut -d= -f2)
```

But the Makefile now reads from `.duckdb-version` via `$(shell cat .duckdb-version)`, so this grep returns `$(shell cat .duckdb-version)` as a literal string, not the actual version.

Change it to read directly from `.duckdb-version`:
```
VER=$$(cat .duckdb-version)
```

The rest of the recipe (curl, unzip, rm, echo) stays the same.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && grep -q 'ensure_amalgamation' Makefile && grep -q 'cpp/include/duckdb.cpp:' Makefile && grep -q 'cat .duckdb-version' justfile && echo "PASS"</automated>
  </verify>
  <done>Makefile has ensure_amalgamation target with file-based dependency; both build_extension_library_debug and build_extension_library_release list ensure_amalgamation as prerequisite; justfile update-headers reads from .duckdb-version directly.</done>
</task>

<task type="auto">
  <name>Task 2: Fix PullRequestCI ducklake-ci-check amalgamation download</name>
  <files>.github/workflows/PullRequestCI.yml</files>
  <action>
The `ducklake-ci-check` job in PullRequestCI.yml runs `cargo build --features extension` directly (line 36), bypassing Make entirely. This also needs the amalgamation files.

Add a step BEFORE the "Build extension" step that downloads the amalgamation. Insert after "Install Rust stable" (line 33) and before "Build extension" (line 35):

```yaml
      - name: Download DuckDB amalgamation
        run: |
          VER=$(cat .duckdb-version)
          curl -sL -o /tmp/libduckdb-src.zip \
            "https://github.com/duckdb/duckdb/releases/download/${VER}/libduckdb-src.zip"
          mkdir -p cpp/include
          unzip -o -j /tmp/libduckdb-src.zip "duckdb.hpp" "duckdb.cpp" -d cpp/include/
          rm -f /tmp/libduckdb-src.zip
```

This matches the same pattern used in DuckDBVersionMonitor.yml (lines 67-73) and the new Makefile target.

Do NOT modify the `linux-fast-check` job -- it uses the reusable workflow which calls `make release`, and the Makefile's `ensure_amalgamation` prerequisite handles it.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && grep -q 'libduckdb-src.zip' .github/workflows/PullRequestCI.yml && grep -q 'Download DuckDB amalgamation' .github/workflows/PullRequestCI.yml && echo "PASS"</automated>
  </verify>
  <done>PullRequestCI ducklake-ci-check job downloads amalgamation before cargo build --features extension.</done>
</task>

<task type="auto">
  <name>Task 3: Verify Makefile build locally and commit</name>
  <files></files>
  <action>
Verify the Makefile changes work correctly by testing the ensure_amalgamation target locally:

1. Temporarily rename (do NOT delete) the existing amalgamation files to simulate a fresh checkout:
   ```bash
   mv cpp/include/duckdb.cpp cpp/include/duckdb.cpp.bak
   mv cpp/include/duckdb.hpp cpp/include/duckdb.hpp.bak
   ```

2. Run `make ensure_amalgamation` and verify it downloads the files:
   ```bash
   make ensure_amalgamation
   ls -la cpp/include/duckdb.cpp cpp/include/duckdb.hpp
   ```

3. Verify the downloaded files match the backed-up originals (same size, same version):
   ```bash
   diff cpp/include/duckdb.cpp cpp/include/duckdb.cpp.bak
   diff cpp/include/duckdb.hpp cpp/include/duckdb.hpp.bak
   ```

4. Clean up backup files:
   ```bash
   rm cpp/include/duckdb.cpp.bak cpp/include/duckdb.hpp.bak
   ```

5. Run `make ensure_amalgamation` again — it should be a no-op since the files exist (Make's file target dependency handles this).

6. Run `just test-all` to verify nothing is broken by the Makefile/justfile changes. Per CLAUDE.md, this is the quality gate.

If all passes, stage and commit with message:
```
fix(ci): auto-download DuckDB amalgamation in CI builds

Since v0.5.0 the build.rs compiles cpp/include/duckdb.cpp (the DuckDB
amalgamation) for the C++ shim. These ~25MB files are gitignored, so CI
fails after checkout with "No such file or directory".

Add ensure_amalgamation Makefile target that downloads the amalgamation
from GitHub releases when missing. Build targets depend on it. Also fix
PullRequestCI ducklake-ci-check job (bypasses Make) and justfile
update-headers version source.
```
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && make ensure_amalgamation 2>&1 | grep -E "(Downloaded|Nothing to be done|up to date)" && just test-all</automated>
  </verify>
  <done>Makefile ensure_amalgamation target works idempotently (downloads when missing, no-op when present). Full test suite passes. Changes committed.</done>
</task>

</tasks>

<verification>
- `make ensure_amalgamation` downloads files when missing, is a no-op when present
- `just test-all` passes (quality gate per CLAUDE.md)
- `just update-headers` correctly reads version from `.duckdb-version`
- Makefile build targets (debug + release) list ensure_amalgamation as prerequisite
- PullRequestCI ducklake-ci-check has amalgamation download step
</verification>

<success_criteria>
- Main Extension Distribution Pipeline passes on next push to main (all 5 platforms)
- PullRequestCI ducklake-ci-check job can build extension from fresh checkout
- No local test regressions (just test-all passes)
</success_criteria>

<output>
After completion, create `.planning/quick/15-check-gh-run-list-and-fix-the-failing-jo/15-SUMMARY.md`
</output>
