# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** -- Phases 1-7 (shipped 2026-02-28)
- ✅ **v0.2.0 Native DDL + Time Dimensions** -- Phases 8-14 (shipped 2026-03-03)
- ✅ **v0.3.0 Zero-Copy Query Pipeline** -- (shipped 2026-03-03)
- ✅ **v0.4.0 Simplified Dimensions** -- (shipped 2026-03-03)
- ✅ **v0.5.0 Parser Extension Spike** -- Phases 15-18 (shipped 2026-03-08)
- ✅ **v0.5.1 DDL Polish** -- Phases 19-23 (shipped 2026-03-09)
- ✅ **v0.5.2 SQL DDL & PK/FK Relationships** -- Phases 24-28 (shipped 2026-03-13)
- ✅ **v0.5.3 Advanced Semantic Features** -- Phases 29-32 (shipped 2026-03-15)
- 🚧 **v0.5.4 Snowflake-Parity & Registry Publishing** -- Phases 33-36 (in progress)

## Phases

<details>
<summary>v0.1.0 MVP (Phases 1-7) -- SHIPPED 2026-02-28</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-02-24
- [x] Phase 2: Storage and DDL (4/4 plans) -- completed 2026-02-24
- [x] Phase 3: Expansion Engine (3/3 plans) -- completed 2026-02-25
- [x] Phase 4: Query Interface (3/3 plans) -- completed 2026-02-25
- [x] Phase 5: Hardening and Docs (2/2 plans) -- completed 2026-02-26
- [x] Phase 6: Tech Debt Code Cleanup (1/1 plan) -- completed 2026-02-26
- [x] Phase 7: Verification & Formal Closure (2/2 plans) -- completed 2026-02-27

Full details: [milestones/v1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

<details>
<summary>v0.2.0 Native DDL + Time Dimensions (Phases 8-14) -- SHIPPED 2026-03-03</summary>

- [x] Phase 8: C++ Shim Infrastructure (2/2 plans) -- completed 2026-03-01
- [x] Phase 9: Time Dimensions (2/2 plans) -- completed 2026-03-01
- [x] Phase 10: pragma_query_t Catalog Persistence (3/3 plans) -- completed 2026-03-01
- [x] Phase 11: Scalar Function DDL (4/4 plans) -- completed 2026-03-02
- [x] Phase 11.1: Snowflake-aligned STRUCT/LIST DDL Syntax (5/5 plans) -- completed 2026-03-02
- [x] Phase 12: EXPLAIN + Typed Output (4/4 plans) -- completed 2026-03-02
- [x] Phase 13: Type-mapping + PBTs (2/2 plans) -- completed 2026-03-02
- [x] Phase 14: DuckLake Integration Test + CI (3/3 plans) -- completed 2026-03-02

Full details: [milestones/v0.2-ROADMAP.md](milestones/v0.2-ROADMAP.md)

</details>

<details>
<summary>v0.3.0 Zero-Copy Query Pipeline -- SHIPPED 2026-03-03</summary>

Replaced binary-read dispatch with zero-copy vector references (`duckdb_vector_reference_vector`).
Eliminated ~600 LOC of per-type read/write code. Type mismatches handled at SQL generation time
via `build_execution_sql` cast wrapper. Streaming chunk-by-chunk instead of collect-all-then-write.

</details>

<details>
<summary>v0.4.0 Simplified Dimensions -- SHIPPED 2026-03-03</summary>

Breaking change: removed `time_dimensions` DDL parameter and `granularities` query parameter.
Time truncation expressed via dimension `expr` directly (e.g., `date_trunc('month', created_at)`).
DDL simplified from 6 to 4 named params; query function from 3 to 2 named params.

</details>

<details>
<summary>v0.5.0 Parser Extension Spike (Phases 15-18) -- SHIPPED 2026-03-08</summary>

- [x] Phase 15: Entry Point POC (2/2 plans) -- completed 2026-03-07
- [x] Phase 16: Parser Hook Registration (1/1 plan) -- completed 2026-03-07
- [x] Phase 17: DDL Execution (1/1 plan) -- completed 2026-03-07
- [x] Phase 17.1: Python vtab crash investigation (2/2 plans) -- completed 2026-03-08
- [x] Phase 18: Verification and Integration (2/2 plans) -- completed 2026-03-08

Full details: [milestones/v0.5-ROADMAP.md](milestones/v0.5-ROADMAP.md)

</details>

<details>
<summary>v0.5.1 DDL Polish (Phases 19-23) -- SHIPPED 2026-03-09</summary>

- [x] Phase 19: Parser Hook Validation Spike (1/1 plan) -- completed 2026-03-09
- [x] Phase 20: Extended DDL Statements (2/2 plans) -- completed 2026-03-09
- [x] Phase 21: Error Location Reporting (3/3 plans) -- completed 2026-03-09
- [x] Phase 22: Documentation (1/1 plan) -- completed 2026-03-09
- [x] Phase 23: Parser Proptests and Caret Integration Tests (2/2 plans) -- completed 2026-03-09

Full details: [milestones/v0.5.1-ROADMAP.md](milestones/v0.5.1-ROADMAP.md)

</details>

<details>
<summary>v0.5.2 SQL DDL & PK/FK Relationships (Phases 24-28) -- SHIPPED 2026-03-13</summary>

- ~~Phase 24: PK/FK Model~~ -- CANCELLED (absorbed into Phase 25-01)
- [x] Phase 25: SQL Body Parser (4/4 plans) -- completed 2026-03-12
- [x] Phase 25.1: Parser Robustness & Security (2/2 plans) -- completed 2026-03-13
- [x] Phase 26: PK/FK Join Resolution (2/2 plans) -- completed 2026-03-13
- [x] Phase 27: Alias-Based Query Expansion (3/3 plans) -- completed 2026-03-13
- [x] Phase 28: Integration Testing & Docs (3/3 plans) -- completed 2026-03-13

Full details: [milestones/v0.5.2-ROADMAP.md](milestones/v0.5.2-ROADMAP.md)

</details>

<details>
<summary>v0.5.3 Advanced Semantic Features (Phases 29-32) -- SHIPPED 2026-03-15</summary>

- [x] Phase 29: FACTS Clause & Hierarchies (2/2 plans) -- completed 2026-03-14
- [x] Phase 30: Derived Metrics (2/2 plans) -- completed 2026-03-14
- [x] Phase 31: Fan Trap Detection (2/2 plans) -- completed 2026-03-14
- [x] Phase 32: Role-Playing Dimensions & USING RELATIONSHIPS (2/2 plans) -- completed 2026-03-14

Full details: [milestones/v0.5.3-ROADMAP.md](milestones/v0.5.3-ROADMAP.md)

</details>

### v0.5.4 Snowflake-Parity & Registry Publishing (In Progress)

**Milestone Goal:** Align the relationship model with Snowflake-style cardinality inference (UNIQUE constraints replace explicit cardinality keywords), support multiple DuckDB versions (1.5.x latest + 1.4.x LTS), ship a documentation site, and publish to the DuckDB Community Extension Registry.

- [x] **Phase 33: UNIQUE Constraints & Cardinality Inference** - Snowflake-aligned cardinality from PK/UNIQUE declarations (completed 2026-03-15)
- [x] **Phase 34: DuckDB 1.5 Upgrade & LTS Branch** - Multi-version support with dual CI (completed 2026-03-16)
- [ ] **Phase 35: Documentation Site** - Zensical docs on GitHub Pages
- [ ] **Phase 36: Registry Publishing & Maintainer Docs** - CE submission and MAINTAINER.md updates

## Phase Details

### Phase 33: UNIQUE Constraints & Cardinality Inference
**Goal**: Users declare UNIQUE constraints on tables and the extension infers relationship cardinality automatically -- no explicit cardinality keywords needed
**Depends on**: Phase 32 (v0.5.3 complete)
**Requirements**: CARD-01, CARD-02, CARD-03, CARD-04, CARD-05, CARD-06, CARD-07, CARD-08, CARD-09
**Success Criteria** (what must be TRUE):
  1. User can declare `UNIQUE (col, ...)` on a table in the TABLES clause and the extension stores it in the model
  2. User omits explicit cardinality keywords from RELATIONSHIPS and the extension infers ONE-TO-ONE (when FK references PK/UNIQUE) or MANY-TO-ONE (when FK is bare) correctly
  3. User who writes `REFERENCES target` without column list gets automatic resolution to the target's PRIMARY KEY
  4. Old v0.5.3 definitions with explicit cardinality keywords are REJECTED on load with a clear error (breaking change -- users must recreate views)
  5. Fan trap detection correctly blocks fan-out scenarios using inferred cardinality values
**Plans**: 2 plans

Plans:
- [ ] 33-01-PLAN.md -- Model + parser + inference (UNIQUE constraints, ref_columns, 2-variant Cardinality, REFERENCES(cols), cardinality inference)
- [ ] 33-02-PLAN.md -- Validation + fan trap + tests (CARD-03/09 FK reference validation, fan trap adaptation, old-JSON guard, sqllogictest updates)

### Phase 34: DuckDB 1.5 Upgrade & LTS Branch
**Goal**: Extension builds, loads, and passes all tests against both DuckDB 1.5.x (latest) and 1.4.x (Andium LTS), with CI running both versions
**Depends on**: Phase 33
**Requirements**: DKDB-01, DKDB-02, DKDB-03, DKDB-04, DKDB-05, DKDB-06
**Success Criteria** (what must be TRUE):
  1. `just test-all` passes on main branch with DuckDB 1.5.x and duckdb-rs 1.10500.0
  2. `just test-all` passes on duckdb/1.4.x branch with DuckDB 1.4.x and duckdb-rs 1.4.4
  3. CI build matrix runs both DuckDB versions and reports results independently
  4. `.duckdb-version` file on each branch correctly identifies its target DuckDB version
  5. DuckDB Version Monitor checks for new releases of both latest and LTS lines
**Plans**: 2 plans

Plans:
- [ ] 34-01-PLAN.md -- Upgrade to DuckDB 1.5.0 (version pins, amalgamation, C++ compat, test suite, PEG smoke test)
- [ ] 34-02-PLAN.md -- LTS branch + CI + Version Monitor (duckdb/1.4.x branch, CI workflow updates, dual-track monitoring)

### Phase 35: Documentation Site
**Goal**: Extension has a proper documentation site deployed to GitHub Pages with DDL reference, query guide, and examples
**Depends on**: Phase 33 (needs stable DDL syntax for docs content)
**Requirements**: DOCS-01, DOCS-02, DOCS-03, DOCS-04
**Success Criteria** (what must be TRUE):
  1. Running `mkdocs build` locally produces a complete static site from `docs/` directory
  2. Pushing to main triggers GitHub Actions workflow that deploys updated docs to GitHub Pages
  3. Documentation site includes getting started guide, DDL reference, query reference, clause-level pages, and examples
  4. README contains a prominent link to the documentation site
**Framework**: mkdocs-material (MkDocs + Material for MkDocs theme)
**Plans**: TBD

Plans:
- [ ] 35-01: TBD
- [ ] 35-02: TBD

### Phase 36: Registry Publishing & Maintainer Docs
**Goal**: Extension is installable via `INSTALL semantic_views FROM community` and MAINTAINER.md covers the dual-branch workflow and CE update process
**Depends on**: Phase 34 (needs dual-version builds), Phase 35 (needs docs site for CE page link)
**Requirements**: CREG-01, CREG-02, CREG-03, CREG-04, CREG-05, MAINT-01, MAINT-02, MAINT-03
**Success Criteria** (what must be TRUE):
  1. `description.yml` exists with correct fields including `ref` (latest) and `andium` (LTS) commit hashes
  2. PR to `duckdb/community-extensions` is submitted and the CE build pipeline passes
  3. A fresh DuckDB instance can run `INSTALL semantic_views FROM community; LOAD semantic_views;` and execute a semantic view query
  4. MAINTAINER.md documents the dual-branch strategy, CE registry update process, and DuckDB version bump procedure for both branches
**Plans**: TBD

Plans:
- [ ] 36-01: TBD
- [ ] 36-02: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 33 -> 34 -> 35 -> 36

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
| Zero-Copy Query Pipeline | v0.3.0 | -- | Complete | 2026-03-03 |
| Simplified Dimensions | v0.4.0 | -- | Complete | 2026-03-03 |
| 15. Entry Point POC | v0.5.0 | 2/2 | Complete | 2026-03-07 |
| 16. Parser Hook Registration | v0.5.0 | 1/1 | Complete | 2026-03-07 |
| 17. DDL Execution | v0.5.0 | 1/1 | Complete | 2026-03-07 |
| 17.1. Python vtab crash investigation | v0.5.0 | 2/2 | Complete | 2026-03-08 |
| 18. Verification and Integration | v0.5.0 | 2/2 | Complete | 2026-03-08 |
| 19. Parser Hook Validation Spike | v0.5.1 | 1/1 | Complete | 2026-03-09 |
| 20. Extended DDL Statements | v0.5.1 | 2/2 | Complete | 2026-03-09 |
| 21. Error Location Reporting | v0.5.1 | 3/3 | Complete | 2026-03-09 |
| 22. Documentation | v0.5.1 | 1/1 | Complete | 2026-03-09 |
| 23. Parser Proptests + Caret Tests | v0.5.1 | 2/2 | Complete | 2026-03-09 |
| 24. PK/FK Model | v0.5.2 | -- | Cancelled | - |
| 25. SQL Body Parser | v0.5.2 | 4/4 | Complete | 2026-03-12 |
| 25.1. Parser Robustness & Security | v0.5.2 | 2/2 | Complete | 2026-03-13 |
| 26. PK/FK Join Resolution | v0.5.2 | 2/2 | Complete | 2026-03-13 |
| 27. Alias-Based Query Expansion | v0.5.2 | 3/3 | Complete | 2026-03-13 |
| 28. Integration Testing & Docs | v0.5.2 | 3/3 | Complete | 2026-03-13 |
| 29. FACTS Clause & Hierarchies | v0.5.3 | 2/2 | Complete | 2026-03-14 |
| 30. Derived Metrics | v0.5.3 | 2/2 | Complete | 2026-03-14 |
| 31. Fan Trap Detection | v0.5.3 | 2/2 | Complete | 2026-03-14 |
| 32. Role-Playing & USING | v0.5.3 | 2/2 | Complete | 2026-03-14 |
| 33. UNIQUE & Cardinality Inference | v0.5.4 | 2/2 | Complete | 2026-03-15 |
| 34. DuckDB 1.5 Upgrade & LTS Branch | 2/2 | Complete   | 2026-03-16 | - |
| 35. Documentation Site | v0.5.4 | 0/? | Not started | - |
| 36. Registry Publishing & Maintainer Docs | v0.5.4 | 0/? | Not started | - |
