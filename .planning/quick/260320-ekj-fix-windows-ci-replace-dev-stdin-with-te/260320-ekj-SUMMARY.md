---
quick_id: 260320-ekj
description: "Fix Windows CI: replace /dev/stdin with temp file in per-process sqllogictest loop"
status: complete
---

# Summary: Fix Windows CI

## What changed

**Makefile** — Both `test_extension_debug_internal` and `test_extension_release_internal` targets updated:

- Before: `--file-list /dev/stdin ... <<< "$$testfile"` (Linux-only, fails on Windows)
- After: `mktemp` creates a temp file, each test path written there, passed as `--file-list "$$TMPLIST"`, cleaned up after loop

## Root cause

DuckDB's Python sqllogictest runner resolves `/dev/stdin` to `/proc/self/fd/0` (Linux procfs), which doesn't exist on Windows. All 13 tests failed identically.

## Verification

- `just test-all` passes (cargo test + sqllogictest + DuckLake CI)
- `just test-sql` confirms 13/13 tests pass with the mktemp approach
- Linux/macOS: `mktemp` is standard, works identically
- Windows: Git Bash (used by GitHub Actions) includes `mktemp` from GNU coreutils
