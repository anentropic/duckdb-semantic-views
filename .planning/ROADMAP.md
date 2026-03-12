# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** -- Phases 1-7 (shipped 2026-02-28)
- ✅ **v0.2.0 Native DDL + Time Dimensions** -- Phases 8-14 (shipped 2026-03-03)
- ✅ **v0.3.0 Zero-Copy Query Pipeline** -- (shipped 2026-03-03)
- ✅ **v0.4.0 Simplified Dimensions** -- (shipped 2026-03-03)
- ✅ **v0.5.0 Parser Extension Spike** -- Phases 15-18 (shipped 2026-03-08)
- ✅ **v0.5.1 DDL Polish** -- Phases 19-23 (shipped 2026-03-09)
- **v0.5.2 SQL DDL & PK/FK Relationships** -- Phases 24-28 (in progress)

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

Full details: [milestones/v0.1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

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

### v0.5.2 SQL DDL & PK/FK Relationships (In Progress)

**Milestone Goal:** Replace function-call DDL body syntax with proper SQL keyword clauses (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS) and adopt Snowflake-style PK/FK relationship model with table aliases, eliminating ON-clause heuristics and enabling qualified column names.

- [ ] **Phase 24: PK/FK Model** - Extend model structs with primary keys, table aliases, and relationship metadata
- [x] **Phase 25: SQL Body Parser** - Parse TABLES/RELATIONSHIPS/DIMENSIONS/METRICS keyword clauses in DDL bodies (completed 2026-03-11)
- [ ] **Phase 26: PK/FK Join Resolution** - Synthesize JOIN ON clauses from PK/FK declarations with graph validation
- [ ] **Phase 27: Alias-Based Query Expansion** - Replace CTE flattening with direct FROM+JOIN expansion and qualified columns
- [ ] **Phase 28: Integration Testing & Documentation** - End-to-end validation and README update with new syntax

## Phase Details

### Phase 24: PK/FK Model
**Goal**: Semantic view definitions can represent tables with aliases, primary keys, and FK-based relationships
**Depends on**: Nothing (first phase of v0.5.2)
**Requirements**: MDL-01, MDL-02, MDL-03, MDL-04, MDL-05, DDL-06
**Success Criteria** (what must be TRUE):
  1. A `SemanticViewDefinition` created via `create_semantic_view()` stores table aliases, physical table names, and primary key columns per table
  2. Relationships store source alias, FK columns, and target alias -- with PK columns inferred from the target table's declaration
  3. Dimension and metric definitions carry a source table alias parsed from qualified `alias.name` syntax
  4. Composite primary keys (multi-column) round-trip correctly through JSON serialization
  5. All existing tests pass unchanged (serde defaults guarantee backward compat during transition)
**Plans:** 2 plans
Plans:
- [ ] 24-01-PLAN.md -- Extend TableRef and Join structs with PK/FK fields and serde tests
- [ ] 24-02-PLAN.md -- Update function DDL interface and migrate sqllogictest files

### Phase 25: SQL Body Parser
**Goal**: Users can write `CREATE SEMANTIC VIEW` with SQL keyword clauses instead of function-call syntax
**Depends on**: Phase 24
**Requirements**: DDL-01, DDL-02, DDL-03, DDL-04, DDL-05, DDL-07
**Success Criteria** (what must be TRUE):
  1. `CREATE SEMANTIC VIEW name AS` followed by `TABLES (...)`, `RELATIONSHIPS (...)`, `DIMENSIONS (...)`, `METRICS (...)` parses successfully and creates a view
  2. TABLES clause accepts `alias AS schema.table PRIMARY KEY (col1, col2)` syntax
  3. RELATIONSHIPS clause accepts `name AS from_alias(fk_cols) REFERENCES to_alias` syntax
  4. DIMENSIONS and METRICS clauses accept `alias.name AS sql_expr` with comma separation
  5. All 7 DDL verbs (CREATE, CREATE OR REPLACE, IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW) work with the new body syntax
**Plans:** 4/4 plans complete
Plans:
- [x] 25-01-PLAN.md -- C++ buffer fix, body_parser.rs skeleton, test scaffolding (complete 2026-03-11)
- [ ] 25-02-PLAN.md -- Implement TABLES, RELATIONSHIPS, DIMENSIONS, METRICS clause parsers
- [ ] 25-03-PLAN.md -- Wire AS dispatch in parse.rs + DefineFromJsonVTab in define.rs
- [ ] 25-04-PLAN.md -- End-to-end integration verification and human checkpoint

### Phase 25.1: Parser Robustness & Security Hardening
**Goal**: The DDL parser correctly handles all valid whitespace variants and is hardened against adversarial inputs
**Depends on**: Phase 25
**Requirements**: (cross-cutting quality concern — no specific req ID)
**Success Criteria** (what must be TRUE):
  1. `detect_ddl_kind` uses token-based keyword matching — `CREATE\tSEMANTIC\tVIEW`, `CREATE  SEMANTIC  VIEW`, and `CREATE\nSEMANTIC\nVIEW` all parse correctly
  2. All 7 DDL forms tolerate arbitrary inter-keyword whitespace (spaces, tabs, newlines, carriage returns, mixed)
  3. Adversarial inputs (very long strings, null bytes, embedded semicolons in view names, Unicode homoglyphs, control characters) are handled safely — no panic, no buffer overrun, no silent wrong-detection
  4. `body_parser.rs` clause keyword matching is similarly whitespace-tolerant
  5. `just test-all` passes
**Plans:** 2 plans
Plans:
- [ ] 25.1-01-PLAN.md -- Write failing TEST-07 proptests + TEST-08 adversarial tests + fuzz_ddl_parse target
- [ ] 25.1-02-PLAN.md -- Implement token-based detect_ddl_kind, decouple prefix_len callers, close TECH-DEBT item 4

### Phase 26: PK/FK Join Resolution
**Goal**: JOIN ON clauses are deterministically synthesized from PK/FK declarations, with invalid graphs rejected at define time
**Depends on**: Phase 24
**Requirements**: EXP-02, EXP-03, EXP-04, EXP-06
**Success Criteria** (what must be TRUE):
  1. Given tables with PK declarations and FK relationships, the expansion engine generates correct `ON a.fk = b.pk` clauses without any user-written ON expressions
  2. Requesting dimensions from tables A and C that are connected through B automatically includes the A-B and B-C joins (transitive inclusion)
  3. Defining a semantic view with a cyclic or diamond relationship graph produces a clear error at define time (not at query time)
  4. Join ordering follows topological sort of the relationship graph, producing deterministic SQL regardless of declaration order
**Plans**: TBD

### Phase 27: Alias-Based Query Expansion
**Goal**: Query expansion generates direct FROM+JOIN SQL with qualified column references instead of CTE flattening
**Depends on**: Phase 24, Phase 26
**Requirements**: EXP-01, EXP-05, CLN-01, CLN-02, CLN-03
**Success Criteria** (what must be TRUE):
  1. Expanded SQL uses `FROM physical_table AS alias LEFT JOIN physical_table AS alias ON ...` instead of the `_base` CTE pattern
  2. Expressions containing qualified column references (`alias.column`) resolve correctly in generated SQL
  3. The old `:=`/struct-literal DDL body parsing code is removed
  4. The CTE-based `_base` flattening expansion path is removed
  5. The ON-clause substring matching join heuristic is removed
**Plans**: TBD

### Phase 28: Integration Testing & Documentation
**Goal**: The complete DDL-to-query pipeline is validated end-to-end and documented for users
**Depends on**: Phase 24, Phase 25, Phase 26, Phase 27
**Requirements**: DOC-01
**Success Criteria** (what must be TRUE):
  1. A multi-table semantic view (3+ tables with PK/FK relationships) can be created with SQL DDL syntax and queried with correct results
  2. The same semantic view definition works through both `CREATE SEMANTIC VIEW` DDL and `create_semantic_view()` function, producing identical query results
  3. `just test-all` passes (Rust unit tests, proptests, sqllogictest, DuckLake CI)
  4. README shows the new SQL DDL syntax with a worked PK/FK relationship example
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 24 -> 25 -> 25.1 -> 26 -> 27 -> 28
(Phase 25.1 hardens the parser before the PK/FK work begins; 25 and 26 both depend on 24)

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
| 24. PK/FK Model | v0.5.2 | 0/2 | Not started | - |
| 25. SQL Body Parser | v0.5.2 | 4/4 | Complete | 2026-03-12 |
| 25.1. Parser Robustness & Security | v0.5.2 | 0/2 | Not started | - |
| 26. PK/FK Join Resolution | v0.5.2 | 0/? | Not started | - |
| 27. Alias-Based Query Expansion | v0.5.2 | 0/? | Not started | - |
| 28. Integration Testing & Docs | v0.5.2 | 0/? | Not started | - |
