# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — MVP

**Shipped:** 2026-02-28
**Phases:** 7 | **Plans:** 18 | **Commits:** 99

### What Was Built
- Loadable DuckDB extension in Rust with function-based DDL for semantic view definitions
- Pure Rust expansion engine: GROUP BY inference, join dependency resolution, filter composition, identifier quoting
- `semantic_query` table function with FFI SQL execution via independent DuckDB connection
- `explain_semantic_view` for SQL expansion transparency
- Sidecar file persistence with atomic writes for catalog survival across restarts
- Multi-platform CI (5 targets), DuckDB version monitor, code quality gates
- Three cargo-fuzz targets, proptest property-based tests, DuckLake/Iceberg integration test
- Comprehensive MAINTAINER.md and TECH-DEBT.md for contributor onboarding

### What Worked
- TDD approach in Phase 3 (expansion engine) — 14 unit tests drove clean implementation
- Feature split (bundled/extension) — solved the fundamental DuckDB Rust extension testing problem early
- Phase-by-phase execution with summaries — clear audit trail and easy verification
- Property-based testing caught edge cases in GROUP BY inference that unit tests missed
- Sidecar file pattern — pragmatic workaround for DuckDB's execution lock limitation

### What Was Inefficient
- Phase 2 took longest (35 min, 4 plans) — DDL-05 persistence gap required an unplanned 4th plan
- Phase 4 table function FFI work (53 min) — duckdb_string_t decode and VARCHAR casting required multiple debugging iterations
- Some ROADMAP.md progress table entries were inconsistent (Phase 3 showed 0/3 but was complete)
- Audit identified tech debt that could have been caught during execution (dead code, feature-gate inconsistency)

### Patterns Established
- Cargo feature split pattern: `default=["duckdb/bundled"]` for testing, `extension=["duckdb/loadable-extension"]` for builds
- Manual FFI entrypoint pattern: capture raw duckdb_database handle for independent connections
- Sidecar file persistence: write-to-tmp-then-rename for atomic writes
- VARCHAR-cast wrapper pattern for safe FFI value reading
- CTE-based expansion with flat namespace for join flattening
- PRAGMA database_list for host DB path resolution (not filtered by name)

### Key Lessons
1. DuckDB's execution locks during scalar `invoke` make SQL execution from within callbacks impossible — design for this constraint from the start
2. `duckdb-rs` loadable-extension feature replaces ALL C API calls with stubs — standalone test binaries can't use them; the bundled/extension feature split is mandatory
3. Property-based tests are more valuable for SQL generation than additional unit tests — they explore the combinatorial space automatically
4. Manual FFI is sometimes necessary even with good Rust bindings — the duckdb_entrypoint_c_api macro hides the database handle needed for independent connections
5. Always prototype the highest-risk integration point first (Phase 4's re-entrant query execution was flagged early)

### Cost Observations
- Total execution time: ~90 min across 18 plans
- Average plan duration: 5 min (median), 6 min (mean)
- Longest phase: Query Interface (53 min, 3 plans) — FFI debugging dominated
- Shortest phases: Hardening (6 min, 2 plans), Tech Debt (3 min, 1 plan)
- Documentation/verification plans completed in 1-3 min each

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Commits | Phases | Key Change |
|-----------|---------|--------|------------|
| v1.0 | 99 | 7 | Initial release — established all patterns |

### Cumulative Quality

| Milestone | Unit Tests | PBT Properties | Fuzz Targets | Integration Tests |
|-----------|-----------|----------------|-------------|-------------------|
| v1.0 | 14+ | 4 properties (256 cases each) | 3 targets | 2 (SQLLogicTest + DuckLake) |

### Top Lessons (Verified Across Milestones)

1. Design around DuckDB's execution lock constraint from the start — it affects every callback pattern
2. The bundled/extension feature split is the foundational pattern for testable DuckDB Rust extensions
