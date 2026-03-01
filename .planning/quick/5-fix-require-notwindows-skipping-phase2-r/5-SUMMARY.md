---
phase: quick-5
plan: 01
subsystem: testing
tags: [sqllogictest, platform-detection, patch, makefile, testing]
dependency_graph:
  requires: []
  provides: [notwindows-require-detection, patch-runner-makefile-target]
  affects: [test/sql/phase2_restart.test, PERSIST-01, PERSIST-02]
tech_stack:
  added: []
  patterns: [idempotent-patch-script, makefile-target-override]
key_files:
  created:
    - scripts/patch_sqllogictest.py
  modified:
    - Makefile
decisions:
  - Patch the installed package in-place (idempotent) rather than forking or vendoring the package
  - Use anchor string from skip_reload block to find injection point — brittle but pinned; error clearly if anchor moves
  - Wire as Makefile prerequisite so patch runs automatically on every test invocation
metrics:
  duration: ~1 min
  completed_date: "2026-03-01"
  tasks_completed: 2
  files_changed: 2
---

# Quick Task 5: Fix require notwindows Skipping Phase 2 Restart Test

**One-liner:** Idempotent Python patch script + Makefile wiring to make `require notwindows` run assertions on Linux/macOS instead of silently skipping.

## Problem

`test/sql/phase2_restart.test` uses `require notwindows` at the top, which is the standard sqllogictest directive to skip a test on Windows only. The installed `duckdb_sqllogictest` Python runner (pinned via extension-ci-tools submodule) does not handle `notwindows` — it falls through to `RequireResult.MISSING`, causing the test to be silently skipped on every platform including Linux and macOS.

This meant PERSIST-01 and PERSIST-02 (Phase 10 catalog persistence requirements) were never actually tested in CI or locally.

## What Was Changed

### Task 1: `scripts/patch_sqllogictest.py` (commit `4cc9b83`)

Created an idempotent Python patch script that:

1. Locates `duckdb_sqllogictest/result.py` via `importlib.util.find_spec`
2. Guards against double-application: if `"if param == 'notwindows':"` is already present, prints `"already applied."` and exits 0
3. Injects `import sys` if not already present
4. Inserts two platform checks immediately before the fallthrough `return RequireResult.MISSING`:
   - `notwindows` → `PRESENT` on non-Windows, `MISSING` on Windows
   - `windows` → `PRESENT` on Windows, `MISSING` on non-Windows
5. Uses a precise anchor string (the `skip_reload` block) for injection — exits with a clear error if the anchor is not found (meaning the upstream package was updated and the patch needs review)

### Task 2: `Makefile` patch-runner target (commit `b35746f`)

Appended to the end of `Makefile`:

- `.PHONY: patch-runner` target that depends on `check_configure` (ensures venv exists) and runs the patch script via `$(PYTHON_VENV_BIN)`
- Override of `test_extension_debug_internal` with `patch-runner` as a prerequisite — replaces base.Makefile recipe with identical recipe
- Override of `test_extension_release_internal` with `patch-runner` as a prerequisite — same pattern

SKIP_TESTS platforms (musl, mingw) resolve to the `tests_skipped` target before reaching these overrides, so `patch-runner` is never called on platforms where sqllogictest is not installed.

## Verification

```
make -n test_debug | grep patch_sqllogictest
# Output: patch-runner wired correctly
```

After `make configure`, running `make patch-runner` prints:
```
patch_sqllogictest: patched configure/venv/.../duckdb_sqllogictest/result.py
```

Running `make patch-runner` a second time prints:
```
patch_sqllogictest: already applied.
```

## How to Remove This Later

When `extension-ci-tools` updates its pinned `duckdb-sqllogictest-python` commit to include platform detection:

1. Delete `scripts/patch_sqllogictest.py`
2. Remove the `patch-runner` block from `Makefile` (last 17 lines: the `patch-runner` target + the two `test_extension_*_internal` overrides)
3. Verify `make -n test_debug` no longer references `patch_sqllogictest`
4. Verify `test/sql/phase2_restart.test` still runs (not skipped) with the updated package

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| 1    | `4cc9b83` | feat(quick-5): add idempotent patch script for notwindows require support |
| 2    | `b35746f` | feat(quick-5): wire patch-runner into Makefile test targets |

## Deviations from Plan

None — plan executed exactly as written.
