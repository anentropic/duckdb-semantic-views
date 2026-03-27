---
quick_id: 260320-ekj
description: "Fix Windows CI: replace /dev/stdin with temp file in per-process sqllogictest loop"
---

# Quick Plan: Fix Windows CI

## Problem

Windows CI (Build Extension workflow) fails with 13/13 tests failing:
```
FileNotFoundError: [Errno 2] No such file or directory: '/proc/self/fd/0'
```

The per-process test loop in Makefile uses `--file-list /dev/stdin <<< "$testfile"`.
The Python sqllogictest runner resolves `/dev/stdin` to `/proc/self/fd/0` (Linux procfs), which doesn't exist on Windows.

## Fix

Replace `/dev/stdin <<<` with `mktemp` temp file approach:
1. Create temp file with `mktemp` before the loop
2. Write each test file path to the temp file
3. Pass temp file as `--file-list`
4. Clean up temp file after the loop

Works on Linux, macOS, and Windows (Git Bash includes `mktemp`).

## Files
- `Makefile` — test_extension_debug_internal and test_extension_release_internal targets
