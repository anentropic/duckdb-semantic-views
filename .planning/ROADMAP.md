# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** — Phases 1-7 (shipped 2026-02-28)
- ✅ **v0.2.0 Native DDL + Time Dimensions** — Phases 8-14 (shipped 2026-03-03)
- ✅ **v0.3.0 Zero-Copy Query Pipeline** — (shipped 2026-03-03)

## Phases

<details>
<summary>✅ v0.1.0 MVP (Phases 1-7) — SHIPPED 2026-02-28</summary>

- [x] Phase 1: Scaffold (3/3 plans) — completed 2026-02-24
- [x] Phase 2: Storage and DDL (4/4 plans) — completed 2026-02-24
- [x] Phase 3: Expansion Engine (3/3 plans) — completed 2026-02-25
- [x] Phase 4: Query Interface (3/3 plans) — completed 2026-02-25
- [x] Phase 5: Hardening and Docs (2/2 plans) — completed 2026-02-26
- [x] Phase 6: Tech Debt Code Cleanup (1/1 plan) — completed 2026-02-26
- [x] Phase 7: Verification & Formal Closure (2/2 plans) — completed 2026-02-27

Full details: [milestones/v0.1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

<details>
<summary>✅ v0.2.0 Native DDL + Time Dimensions (Phases 8-14) — SHIPPED 2026-03-03</summary>

- [x] Phase 8: C++ Shim Infrastructure (2/2 plans) — completed 2026-03-01
- [x] Phase 9: Time Dimensions (2/2 plans) — completed 2026-03-01
- [x] Phase 10: pragma_query_t Catalog Persistence (3/3 plans) — completed 2026-03-01
- [x] Phase 11: Scalar Function DDL (4/4 plans) — completed 2026-03-02
- [x] Phase 11.1: Snowflake-aligned STRUCT/LIST DDL Syntax (5/5 plans) — completed 2026-03-02
- [x] Phase 12: EXPLAIN + Typed Output (4/4 plans) — completed 2026-03-02
- [x] Phase 13: Type-mapping + PBTs (2/2 plans) — completed 2026-03-02
- [x] Phase 14: DuckLake Integration Test + CI (3/3 plans) — completed 2026-03-02

Full details: [milestones/v0.2-ROADMAP.md](milestones/v0.2-ROADMAP.md)

</details>

<details>
<summary>✅ v0.3.0 Zero-Copy Query Pipeline — SHIPPED 2026-03-03</summary>

Replaced binary-read dispatch with zero-copy vector references (`duckdb_vector_reference_vector`).
Eliminated ~600 LOC of per-type read/write code. Type mismatches handled at SQL generation time
via `build_execution_sql` cast wrapper. Streaming chunk-by-chunk instead of collect-all-then-write.

</details>

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v0.1.0 | 3/3 | Complete | 2026-02-24 |
| 2. Storage and DDL | v0.1.0 | 4/4 | Complete | 2026-02-24 |
| 3. Expansion Engine | v0.1.0 | 3/3 | Complete | 2026-02-25 |
| 4. Query Interface | v0.1.0 | 3/3 | Complete | 2026-02-25 |
| 5. Hardening and Docs | v0.1.0 | 2/2 | Complete | 2026-02-26 |
| 6. Tech Debt Code Cleanup | v0.1.0 | 1/1 | Complete | 2026-02-26 |
| 7. Verification & Formal Closure | v0.1.0 | 2/2 | Complete | 2026-02-27 |
| 8. C++ Shim Infrastructure | v0.2.0 | 2/2 | Complete | 2026-03-01 |
| 9. Time Dimensions | v0.2.0 | 2/2 | Complete | 2026-03-01 |
| 10. pragma_query_t Catalog Persistence | v0.2.0 | 3/3 | Complete | 2026-03-01 |
| 11. Scalar Function DDL | v0.2.0 | 4/4 | Complete | 2026-03-02 |
| 11.1. Snowflake-aligned DDL Syntax | v0.2.0 | 5/5 | Complete | 2026-03-02 |
| 12. EXPLAIN + Typed Output | v0.2.0 | 4/4 | Complete | 2026-03-02 |
| 13. Type-mapping + PBTs | v0.2.0 | 2/2 | Complete | 2026-03-02 |
| 14. DuckLake Integration + CI | v0.2.0 | 3/3 | Complete | 2026-03-02 |
| Zero-Copy Query Pipeline | v0.3.0 | — | Complete | 2026-03-03 |
