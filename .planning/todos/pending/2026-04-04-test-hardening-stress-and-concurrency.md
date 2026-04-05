---
title: Test hardening — large-schema stress and concurrent access tests
created: 2026-04-04
source: Code review (code-review-2026-04-04.md)
priority: low
---

# Test Hardening: Stress and Concurrency

Coverage gaps identified in code review, deferred from Phase 42 (refactor/tidy-ups):

## Large-schema stress tests
- Views with 50+ dimensions/metrics
- Deep join chains (10+ tables)
- Large fact dependency graphs
- Verify topological sort and join resolution at scale

## Concurrent access tests
- Concurrent catalog reads/writes
- Validates that the single-threaded assumption holds or catches regressions if the connection model changes
- Related to the TOCTOU fix in catalog_insert (Phase 42 fixes the pattern, this would test it under contention)
