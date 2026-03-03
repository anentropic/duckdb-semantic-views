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

## Milestone: v0.2.0 — Native DDL + Time Dimensions

**Shipped:** 2026-03-03
**Phases:** 8 (including 11.1) | **Plans:** 25 | **Commits:** 125

### What Was Built
- C++ shim infrastructure with cc crate, vendored DuckDB headers, feature-gated compilation
- Time dimensions with date_trunc codegen, granularity coarsening (day/week/month/year), per-query override
- pragma_query_t catalog persistence replacing sidecar file — transactional, write-first pattern
- Scalar function DDL (create_semantic_view, drop_semantic_view) after architecture pivot from parser hooks
- Snowflake-aligned STRUCT/LIST DDL syntax with 6-arg typed parameters
- Typed output columns with binary-read dispatch (replacing all-VARCHAR)
- 36 property-based tests for type dispatch covering TIMESTAMP, BOOLEAN, DECIMAL, LIST, ENUM, NULL
- DuckLake integration test refresh to v0.2.0 API with parallel CI job

### What Worked
- Architecture pivot handled cleanly — Phase 11 discovered parser hooks impossible, pivoted to scalar DDL without wasted effort
- Binary-read dispatch (Phase 13) with PBTs caught real bugs: TIMESTAMP all-NULL, BOOLEAN UB, DECIMAL-as-string
- Phase 11.1 (inserted decimal phase) worked well for urgent syntax alignment without disrupting roadmap numbering
- pragma_query_t write-first pattern with separate persist_conn solved the deadlock-free persistence problem elegantly
- Quick tasks (6 total) kept CI green without disrupting phase flow

### What Was Inefficient
- Phase 11 plans 01-03 built C++ parser hook infrastructure that was ultimately discarded when `-fvisibility=hidden` was discovered
- ROADMAP.md progress table drifted from reality (Phase 9 showed "0/?" despite being complete; Phase 11 showed "2/4")
- REQUIREMENTS.md traceability table was never updated after initial creation — all TIME/PERSIST requirements stayed "Pending" despite phases completing
- Phase 12 SUMMARY files had empty `provides` fields — one-liner extraction failed for these

### Patterns Established
- Write-first pragma persistence: invoke → pragma → table → in-memory (avoids lock conflicts)
- cc crate C++ compilation gated behind `CARGO_FEATURE_EXTENSION` env var
- Symbol visibility: `--version-script` on Linux, `-exported_symbols_list` on macOS
- Binary-read type dispatch: match on DuckDB logical type, read directly from chunk (no VARCHAR cast)
- LIMIT 0 type inference at define time for zero-cost column type discovery
- Decimal phase insertion (11.1) for urgent work between existing phases

### Key Lessons
1. Python's DuckDB compiles ALL C++ with `-fvisibility=hidden` — any extension feature depending on C++ symbol resolution is impossible when loaded via Python
2. C API function pointers (via `loadable-extension` stubs) are the ONLY reliable entry point — design all extension interfaces around them
3. PBT-driven type dispatch is dramatically more effective than manual test cases — 36 properties found 3 real bugs that unit tests missed
4. Keep traceability tables updated during execution, not just at milestone close — stale tables create confusion
5. Quick tasks for CI fixes are essential — 6 quick tasks kept the pipeline green without blocking phase work

### Cost Observations
- 125 commits in 3 days
- 8 phases, 25 plans, ~102 commits of substance + 23 CI/fmt fixes
- Notable: Phase 11 architecture pivot (parser hook → scalar DDL) was the highest-risk moment; recovery was clean

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Commits | Phases | Key Change |
|-----------|---------|--------|------------|
| v1.0 | 99 | 7 | Initial release — established all patterns |
| v0.2.0 | 125 | 8 | Architecture pivot (parser hook → scalar DDL), typed output, PBTs |

### Cumulative Quality

| Milestone | Total Tests | PBT Properties | Fuzz Targets | Integration Tests |
|-----------|------------|----------------|-------------|-------------------|
| v1.0 | ~30 | 4 properties (256 cases each) | 3 targets | 2 (SQLLogicTest + DuckLake) |
| v0.2.0 | 136 | 40 properties (256+ cases each) | 3 targets | 3 (SQLLogicTest + DuckLake CI + DuckLake local) |

### Top Lessons (Verified Across Milestones)

1. Design around DuckDB's execution lock constraint from the start — it affects every callback pattern
2. The bundled/extension feature split is the foundational pattern for testable DuckDB Rust extensions
3. Property-based tests catch bugs that unit tests miss — especially for type dispatch and SQL generation
4. Keep traceability/progress tables updated during execution, not at milestone close
