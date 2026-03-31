# Quick Task 260331-ta2: Summary

**Task:** Write a justfile recipe for the release process
**Date:** 2026-03-31
**Commit:** 0390bab

## What Changed

Added a `release` recipe to the justfile that automates the CE registry release process:

1. Precondition checks (main branch, clean tree, `gh` CLI)
2. Extracts HEAD SHA and version from Cargo.toml
3. Updates `description.yml` ref + version via sed, commits in this repo
4. Copies `description.yml` to CE fork (configurable via `CE_REPO` env var, default `~/Documents/Dev/Sources/community-extensions`)
5. Commits, pushes, and opens PR to `duckdb/community-extensions` via `gh pr create`

## Files Modified

| File | Change |
|------|--------|
| `Justfile` | Added `release` recipe (62 lines) |
