# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** — Phases 1-7 (shipped 2026-02-28)
- ✅ **v0.2.0 Native DDL + Time Dimensions** — Phases 8-14 (shipped 2026-03-03)
- ✅ **v0.3.0 Zero-Copy Query Pipeline** — (shipped 2026-03-03)
- ✅ **v0.4.0 Simplified Dimensions** — (shipped 2026-03-03)
- ✅ **v0.5.0 Parser Extension Spike** — Phases 15-18 (shipped 2026-03-08)
- **v0.5.1 DDL Polish** — Phases 19-22 (in progress)

## Phases

<details>
<summary>v0.1.0 MVP (Phases 1-7) — SHIPPED 2026-02-28</summary>

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
<summary>v0.2.0 Native DDL + Time Dimensions (Phases 8-14) — SHIPPED 2026-03-03</summary>

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
<summary>v0.3.0 Zero-Copy Query Pipeline — SHIPPED 2026-03-03</summary>

Replaced binary-read dispatch with zero-copy vector references (`duckdb_vector_reference_vector`).
Eliminated ~600 LOC of per-type read/write code. Type mismatches handled at SQL generation time
via `build_execution_sql` cast wrapper. Streaming chunk-by-chunk instead of collect-all-then-write.

</details>

<details>
<summary>v0.4.0 Simplified Dimensions — SHIPPED 2026-03-03</summary>

Breaking change: removed `time_dimensions` DDL parameter and `granularities` query parameter.
Time truncation expressed via dimension `expr` directly (e.g., `date_trunc('month', created_at)`).
DDL simplified from 6 to 4 named params; query function from 3 to 2 named params.

</details>

<details>
<summary>v0.5.0 Parser Extension Spike (Phases 15-18) — SHIPPED 2026-03-08</summary>

- [x] Phase 15: Entry Point POC (2/2 plans) — completed 2026-03-07
- [x] Phase 16: Parser Hook Registration (1/1 plan) — completed 2026-03-07
- [x] Phase 17: DDL Execution (1/1 plan) — completed 2026-03-07
- [x] Phase 17.1: Python vtab crash investigation (2/2 plans) — completed 2026-03-08
- [x] Phase 18: Verification and Integration (2/2 plans) — completed 2026-03-08

Full details: [milestones/v0.5-ROADMAP.md](milestones/v0.5-ROADMAP.md)

</details>

### v0.5.1 DDL Polish (In Progress)

**Milestone Goal:** Complete the native DDL surface for semantic views -- DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW -- with quality error reporting and updated documentation.

- [ ] **Phase 19: Parser Hook Validation Spike** - Empirically determine which DDL prefixes trigger the parser fallback hook
- [ ] **Phase 20: Extended DDL Statements** - Native syntax for DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW
- [ ] **Phase 21: Error Location Reporting** - Clause-level hints, character positions, and "did you mean" suggestions
- [ ] **Phase 22: Documentation** - README DDL syntax reference with worked examples

## Phase Details

### Phase 19: Parser Hook Validation Spike
**Goal**: Confirmed scope for v0.5.1 -- which DDL statements can use the parser fallback hook and which cannot
**Depends on**: Phase 18 (v0.5.0 parser hook infrastructure)
**Requirements**: None (scope-determination phase; informs DDL-07, DDL-08 feasibility)
**Success Criteria** (what must be TRUE):
  1. Each of the 7 DDL prefixes (DROP, DROP IF EXISTS, CREATE OR REPLACE, CREATE IF NOT EXISTS, DESCRIBE, SHOW) has been tested against DuckDB with the extension loaded
  2. For each prefix, the error type is recorded: Parser Error (triggers hook) or Catalog Error (bypasses hook)
  3. A concrete scope decision is documented: which statements get native syntax in v0.5.1 and which remain function-only
**Plans**: TBD

### Phase 20: Extended DDL Statements
**Goal**: Users can manage semantic views entirely through native DDL syntax -- create, replace, drop, inspect, and list
**Depends on**: Phase 19 (scope confirmation)
**Requirements**: DDL-03, DDL-04, DDL-05, DDL-06, DDL-07, DDL-08
**Success Criteria** (what must be TRUE):
  1. User can run `DROP SEMANTIC VIEW name` and the view is removed from the catalog
  2. User can run `DROP SEMANTIC VIEW IF EXISTS name` without error when the view does not exist
  3. User can run `CREATE OR REPLACE SEMANTIC VIEW name (...)` and the existing view is updated in place
  4. User can run `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` and no error occurs when the view already exists
  5. User can run `DESCRIBE SEMANTIC VIEW name` and see dimensions, metrics, and types (native DDL or function fallback per Phase 19 findings)
  6. User can run `SHOW SEMANTIC VIEWS` and see all defined semantic views (native DDL or function fallback per Phase 19 findings)
**Plans**: TBD

### Phase 21: Error Location Reporting
**Goal**: Users get actionable, positioned error messages when DDL statements are malformed
**Depends on**: Phase 20 (DDL parse infrastructure and `ParseError` struct)
**Requirements**: ERR-01, ERR-02, ERR-03
**Success Criteria** (what must be TRUE):
  1. A malformed DDL statement shows which clause is wrong (e.g., "Error in DIMENSIONS clause: expected list of dimension definitions")
  2. Error messages include a character position that DuckDB renders as a caret (`^`) under the offending location in the original DDL text
  3. Misspelled clause names or view names produce "Did you mean '...'?" suggestions using fuzzy matching
**Plans**: TBD

### Phase 22: Documentation
**Goal**: Users can learn the full DDL syntax from the README without reading source code
**Depends on**: Phase 20 (all DDL verbs confirmed), Phase 21 (error behavior confirmed)
**Requirements**: DOC-01
**Success Criteria** (what must be TRUE):
  1. README contains a DDL syntax reference section covering CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, and SHOW
  2. README includes at least one worked example showing the full lifecycle: create a semantic view, query it, describe it, drop it
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 19 -> 20 -> 21 -> 22

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
| Simplified Dimensions | v0.4.0 | — | Complete | 2026-03-03 |
| 15. Entry Point POC | v0.5.0 | 2/2 | Complete | 2026-03-07 |
| 16. Parser Hook Registration | v0.5.0 | 1/1 | Complete | 2026-03-07 |
| 17. DDL Execution | v0.5.0 | 1/1 | Complete | 2026-03-07 |
| 17.1. Python vtab crash investigation | v0.5.0 | 2/2 | Complete | 2026-03-08 |
| 18. Verification and Integration | v0.5.0 | 2/2 | Complete | 2026-03-08 |
| 19. Parser Hook Validation Spike | v0.5.1 | 0/? | Not started | - |
| 20. Extended DDL Statements | v0.5.1 | 0/? | Not started | - |
| 21. Error Location Reporting | v0.5.1 | 0/? | Not started | - |
| 22. Documentation | v0.5.1 | 0/? | Not started | - |
