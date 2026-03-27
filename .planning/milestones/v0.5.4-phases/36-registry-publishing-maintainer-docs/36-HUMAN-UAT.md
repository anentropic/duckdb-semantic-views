---
status: partial
phase: 36-registry-publishing-maintainer-docs
source: [36-VERIFICATION.md]
started: 2026-03-27T22:00:00Z
updated: 2026-03-27T22:00:00Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. Submit draft PR to duckdb/community-extensions (CREG-04)
expected: Draft PR submitted with description.yml at extensions/semantic_views/description.yml, ref field contains real merge SHA
result: [pending]

### 2. Verify INSTALL semantic_views FROM community works (CREG-05)
expected: Extension installs, loads, and hello_world query returns EU=150, US=300
result: [pending]

### 3. Run just test-all quality gate
expected: All tests pass (Rust + sqllogictest + DuckLake CI + caret tests)
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
