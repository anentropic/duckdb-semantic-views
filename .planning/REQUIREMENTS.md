# Requirements: DuckDB Semantic Views

**Defined:** 2026-03-01
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## v0.2.0 Requirements

Requirements for milestone v0.2.0 — Native DDL + Time Dimensions. Each maps to roadmap phases.

### Infrastructure

- [x] **INFRA-01**: C++ shim compiles via `cc` crate on all 5 CI targets without breaking `cargo test` workflow

### Persistence

- [ ] **PERSIST-01**: Semantic view definitions persist via DuckDB native tables (`pragma_query_t`) — no sidecar `.semantic_views` file
- [ ] **PERSIST-02**: A `ROLLBACK` reverts a definition change in both persistent storage and in-memory catalog
- [ ] **PERSIST-03**: Sidecar file mechanism removed from codebase

### DDL

- [ ] **DDL-01**: User can create a semantic view with `CREATE SEMANTIC VIEW` SQL syntax
- [ ] **DDL-02**: User can drop a semantic view with `DROP SEMANTIC VIEW` SQL syntax
- [x] **DDL-03**: `CREATE OR REPLACE SEMANTIC VIEW` overwrites an existing definition
- [x] **DDL-04**: Native DDL supports all capabilities of `define_semantic_view()` (dimensions, metrics, joins, filters)
- [x] **DDL-05**: `define_semantic_view()` and `drop_semantic_view()` functions removed after native DDL is validated
- [ ] **DDL-06**: Non-semantic-view SQL is unaffected by parser hook (no regression)

### Time Dimensions

- [ ] **TIME-01**: User can declare a dimension as time-typed with a granularity (day, week, month, year) in a semantic view definition
- [ ] **TIME-02**: `semantic_query` truncates time dimension values to the declared granularity using `date_trunc`
- [ ] **TIME-03**: User can override time dimension granularity at query time via a `granularities` parameter
- [ ] **TIME-04**: Time dimensions on DATE source columns return DATE values (not TIMESTAMP)

### EXPLAIN

- [ ] **EXPL-01**: `EXPLAIN FROM semantic_query(...)` shows DuckDB's full physical query plan for the expanded SQL

### Output Typing

- [ ] **OUT-01**: `semantic_query` returns typed columns (BIGINT, DOUBLE, DATE, etc.) matching source column types instead of all-VARCHAR

## v0.3.0 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Distribution

- **DIST-01**: Extension installable via `INSTALL semantic_views FROM community`
- **DIST-02**: Real-world TPC-H demo notebook

### Quality

- **QUAL-01**: Typed output columns with WEEK and QUARTER time granularities
- **QUAL-02**: Fiscal calendar / Sunday-start week convention

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Pre-aggregation / materialization | Orthogonal complexity — deferred to v0.3.0+ |
| YAML definition format | SQL DDL only; YAML adds no value for v0.2.0 |
| Derived metrics (profit = revenue - cost) | Future milestone |
| Hierarchies (drill-down paths) | Future milestone |
| Community extension registry publication | Deferred to v0.3.0 — native DDL must be stable first |
| BI tool HTTP API | Not a DuckDB extension concern |
| Column-level security | Beyond row-level filter scope |
| Custom time spine table | `date_trunc` is sufficient |
| Fiscal calendar / Sunday-start weeks | ISO 8601 only in v0.2.0 |
| QUARTER granularity | Deferred with WEEK for consistency; trivial to add in v0.3.0 |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| INFRA-01 | Phase 8 | Complete |
| TIME-01 | Phase 9 | Pending |
| TIME-02 | Phase 9 | Pending |
| TIME-03 | Phase 9 | Pending |
| TIME-04 | Phase 9 | Pending |
| PERSIST-01 | Phase 10 | Pending |
| PERSIST-02 | Phase 10 | Pending |
| PERSIST-03 | Phase 10 | Pending |
| DDL-01 | Phase 11 | Pending |
| DDL-02 | Phase 11 | Pending |
| DDL-03 | Phase 11 | Complete |
| DDL-04 | Phase 11 | Complete |
| DDL-05 | Phase 11 | Complete |
| DDL-06 | Phase 11 | Pending |
| EXPL-01 | Phase 12 | Pending |
| OUT-01 | Phase 12 | Pending |

**Coverage:**
- v0.2.0 requirements: 16 total
- Mapped to phases: 16
- Unmapped: 0 ✓

---
*Requirements defined: 2026-03-01*
*Last updated: 2026-03-01 — traceability complete after roadmap creation*
