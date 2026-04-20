# Requirements: DuckDB Semantic Views

**Defined:** 2026-04-18
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand

## v0.7.0 Requirements

Requirements for YAML Definitions & Materialization Routing milestone. Each maps to roadmap phases.

### YAML Definition Format

- [ ] **YAML-01**: User can create a semantic view from inline YAML using `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$`
- [ ] **YAML-02**: User can create a semantic view from a YAML file using `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'`
- [x] **YAML-03**: YAML schema supports all SemanticViewDefinition fields: tables, relationships, dimensions, metrics, facts, and metadata annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC)
- [x] **YAML-04**: User can export a stored semantic view as YAML via `SELECT READ_YAML_FROM_SEMANTIC_VIEW('name')` (supports fully qualified names)
- [x] **YAML-05**: YAML and SQL DDL produce identical internal representations — same validation, persistence, and query behavior
- [ ] **YAML-06**: `CREATE OR REPLACE` and `IF NOT EXISTS` modifiers work with `FROM YAML` syntax
- [ ] **YAML-07**: YAML FILE loading respects DuckDB's `enable_external_access` security setting
- [x] **YAML-08**: YAML round-trip is lossless — `READ_YAML_FROM_SEMANTIC_VIEW` output can recreate an identical semantic view
- [x] **YAML-09**: YAML input is size-capped to prevent anchor/alias bomb denial-of-service

### Materialization Routing

- [ ] **MAT-01**: User can declare materializations in `CREATE SEMANTIC VIEW` via `MATERIALIZATIONS` clause with `mat_name AS (TABLE catalog.schema.table, DIMENSIONS (...), METRICS (...))`
- [x] **MAT-02**: At query time, the engine routes to a materialization when it exactly covers the requested dimensions and metrics
- [x] **MAT-03**: When no materialization matches, the query falls back to raw table expansion (no error)
- [x] **MAT-04**: Semi-additive and window function metrics are excluded from materialization routing (always expand from raw)
- [x] **MAT-05**: Materialization routing is transparent — no user-visible behavior change without matching materializations
- [ ] **MAT-06**: `MATERIALIZATIONS` clause works in both SQL DDL and YAML definitions
- [ ] **MAT-07**: Materialization metadata persists across DuckDB restarts

### Introspection

- [ ] **INTR-01**: `explain_semantic_view()` output includes materialization routing decision (materialization name or "none") and expanded SQL reflects the routed table
- [ ] **INTR-02**: `DESCRIBE SEMANTIC VIEW` includes materialization entries
- [ ] **INTR-03**: `SHOW SEMANTIC MATERIALIZATIONS IN view_name` lists all declared materializations with covered dimensions and metrics

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Materialization Routing

- **MAT-F01**: Re-aggregation routing — subset dimension matching with additivity classification
- **MAT-F02**: Additivity metadata on Metric struct (SUM/COUNT/MIN/MAX = additive, AVG/COUNT DISTINCT = non-additive)
- **MAT-F03**: Materialization freshness tracking / staleness warnings

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Materialization creation/refresh | Engine only routes to pre-existing tables; creation is out of scope (e.g. via dbt) |
| Re-aggregation routing | Correctness risk for non-additive metrics; deferred to v2 with additivity classification |
| AVG decomposition (SUM/COUNT) | Depends on re-aggregation routing |
| Cross-view materialization sharing | Non-goal by design |
| OSI YAML format | Using our own JSON-equivalent schema instead |
| YAML filters clause | Not supported in current SQL DDL either |
| Automatic refresh pipelines | Out of scope; materializations are managed externally |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| YAML-01 | Phase 52 | Pending |
| YAML-02 | Phase 53 | Pending |
| YAML-03 | Phase 51 | Complete |
| YAML-04 | Phase 56 | Complete |
| YAML-05 | Phase 51 | Complete |
| YAML-06 | Phase 52 | Pending |
| YAML-07 | Phase 53 | Pending |
| YAML-08 | Phase 56 | Complete |
| YAML-09 | Phase 51 | Complete |
| MAT-01 | Phase 54 | Pending |
| MAT-02 | Phase 55 | Complete |
| MAT-03 | Phase 55 | Complete |
| MAT-04 | Phase 55 | Complete |
| MAT-05 | Phase 55 | Complete |
| MAT-06 | Phase 54 | Pending |
| MAT-07 | Phase 54 | Pending |
| INTR-01 | Phase 57 | Pending |
| INTR-02 | Phase 57 | Pending |
| INTR-03 | Phase 57 | Pending |

**Coverage:**
- v0.7.0 requirements: 19 total
- Mapped to phases: 19
- Unmapped: 0

---
*Requirements defined: 2026-04-18*
*Last updated: 2026-04-17 after roadmap creation*
