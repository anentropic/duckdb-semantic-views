---
plan: 41-02
phase: 41-describe-rewrite
status: complete
started: 2026-04-02
completed: 2026-04-02
---

# Plan 41-02: Update existing test files + error.rs — Summary

## What Was Built

Updated the error help message in `src/query/error.rs` to use `DESCRIBE SEMANTIC VIEW` syntax instead of the old `FROM describe_semantic_view()` function call syntax.

Note: The 7 test file updates originally planned for this plan were already completed by Plan 41-01's executor as a deviation (auto-fixed to maintain `just test-all` green).

## Key Files

### Modified
- `src/query/error.rs` — help message updated from `FROM describe_semantic_view('{view_name}')` to `DESCRIBE SEMANTIC VIEW {view_name}`

## Decisions

- Plan 41-01 already updated all 7 test files as a deviation, so this plan only needed the error.rs update
- Error message uses native DDL syntax (`DESCRIBE SEMANTIC VIEW`) which is cleaner for users

## Self-Check: PASSED

- [x] `cargo test` — 483 tests pass
- [x] Error help message uses DESCRIBE SEMANTIC VIEW syntax
