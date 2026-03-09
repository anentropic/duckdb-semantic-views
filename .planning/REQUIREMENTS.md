# Requirements: DuckDB Semantic Views

**Defined:** 2026-03-09
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand -- the extension handles expansion, DuckDB handles execution.

## v0.5.2 Requirements

Requirements for SQL DDL syntax, PK/FK relationship model, and qualified column support. Each maps to roadmap phases.

### DDL Syntax

- [ ] **DDL-01**: `CREATE SEMANTIC VIEW` accepts SQL keyword body: `TABLES (...)`, `RELATIONSHIPS (...)`, `DIMENSIONS (...)`, `METRICS (...)`
- [ ] **DDL-02**: TABLES clause parses `alias AS physical_table PRIMARY KEY (col, ...)`
- [ ] **DDL-03**: RELATIONSHIPS clause parses `[name AS] from_alias(fk_cols) REFERENCES to_alias`
- [ ] **DDL-04**: DIMENSIONS clause parses `alias.dim_name AS sql_expr`
- [ ] **DDL-05**: METRICS clause parses `alias.metric_name AS agg_expr`
- [ ] **DDL-06**: Function-based `create_semantic_view()` accepts equivalent PK/FK model parameters
- [ ] **DDL-07**: All 7 DDL verbs work with new syntax (CREATE, CREATE OR REPLACE, IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW)

### Model

- [ ] **MDL-01**: `TableRef` stores alias, physical table, and primary key columns
- [ ] **MDL-02**: Relationships store from_alias, from_cols, to_alias (PK inferred from TABLES)
- [ ] **MDL-03**: Dimensions and metrics store source table alias from qualified `alias.name` prefix
- [ ] **MDL-04**: Composite primary keys supported (`PRIMARY KEY (col1, col2)`)
- [ ] **MDL-05**: Relationship names stored (informational, from `name AS ...` syntax)

### Expansion

- [ ] **EXP-01**: Query expansion generates alias-based `FROM base AS alias LEFT JOIN t AS alias ON ...` (no CTE flattening)
- [ ] **EXP-02**: JOIN ON clauses synthesized from PK/FK declarations
- [ ] **EXP-03**: Join ordering via topological sort of relationship graph
- [ ] **EXP-04**: Transitive join inclusion -- requesting dims from A and C auto-joins through B
- [ ] **EXP-05**: Qualified column references (`alias.column`) work in generated SQL
- [ ] **EXP-06**: Define-time validation: relationship graph must be a tree (error on diamonds/cycles)

### Cleanup

- [ ] **CLN-01**: Remove old `:=`/struct literal DDL body parsing (no backward compat)
- [ ] **CLN-02**: Remove CTE-based `_base` flattening expansion path
- [ ] **CLN-03**: Remove ON-clause substring matching join heuristic

### Documentation

- [ ] **DOC-01**: README updated with new SQL DDL syntax reference, PK/FK relationship examples, and qualified column usage

## Future Requirements

Deferred to future milestones. Tracked but not in current roadmap.

### Registry & Demos

- **DIST-01**: Published to DuckDB community extension registry
- **DIST-02**: Real-world TPC-H demo notebook

### Advanced Features

- **ADV-01**: FACTS clause (`alias.fact_name AS raw_expr` -- named sub-expressions for metrics)
- **ADV-02**: Derived metrics (metric referencing other metrics)
- **ADV-03**: Hierarchies / drill-down paths (country -> region -> city)
- **ADV-04**: Fan trap detection and deduplication warnings
- **ADV-05**: Role-playing dimensions (same table joined multiple times via different relationships)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| UNIQUE constraints on tables | PK suffices for join inference |
| ASOF / temporal relationships | Complex temporal join semantics; standard equi-joins cover 95% of cases |
| NON ADDITIVE BY on metrics | Requires query-time validation; all metrics treated as additive |
| Window function metrics | Requires special expansion without GROUP BY |
| WITH SYNONYMS | AI/natural-language discovery not relevant for SQL-only DuckDB |
| Per-expression COMMENT | No runtime effect; can be added later without breaking changes |
| PUBLIC/PRIVATE visibility | No access control in DuckDB extensions |
| YAML definition format | SQL DDL first per PROJECT.md |
| Backward compat for old DDL syntax | Pre-release; finding the right design |
| Multiple join paths between same tables | Error on diamonds; defer explicit paths |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| DDL-01 | Phase 25 | Pending |
| DDL-02 | Phase 25 | Pending |
| DDL-03 | Phase 25 | Pending |
| DDL-04 | Phase 25 | Pending |
| DDL-05 | Phase 25 | Pending |
| DDL-06 | Phase 24 | Pending |
| DDL-07 | Phase 25 | Pending |
| MDL-01 | Phase 24 | Pending |
| MDL-02 | Phase 24 | Pending |
| MDL-03 | Phase 24 | Pending |
| MDL-04 | Phase 24 | Pending |
| MDL-05 | Phase 24 | Pending |
| EXP-01 | Phase 27 | Pending |
| EXP-02 | Phase 26 | Pending |
| EXP-03 | Phase 26 | Pending |
| EXP-04 | Phase 26 | Pending |
| EXP-05 | Phase 27 | Pending |
| EXP-06 | Phase 26 | Pending |
| CLN-01 | Phase 27 | Pending |
| CLN-02 | Phase 27 | Pending |
| CLN-03 | Phase 27 | Pending |
| DOC-01 | Phase 28 | Pending |

**Coverage:**
- v0.5.2 requirements: 22 total
- Mapped to phases: 22
- Unmapped: 0

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-09 after roadmap creation*
