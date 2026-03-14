# Requirements: DuckDB Semantic Views

**Defined:** 2026-03-14
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand -- the extension handles expansion, DuckDB handles execution.

## v0.5.3 Requirements

Requirements for advanced semantic modeling capabilities. Each maps to roadmap phases.

### Semantic Modeling (FACTS)

- [x] **FACT-01**: User can declare named row-level expressions in a FACTS clause (`alias.fact_name AS sql_expr`)
- [x] **FACT-02**: Metric expressions can reference fact names; expansion inlines the fact expression with parenthesization
- [x] **FACT-03**: Facts can reference other facts; expansion resolves in topological order
- [x] **FACT-04**: Define-time validation rejects fact cycles and references to non-existent facts
- [x] **FACT-05**: DESCRIBE SEMANTIC VIEW shows facts alongside dimensions and metrics

### Derived Metrics

- [x] **DRV-01**: User can declare derived metrics without a table prefix (`metric_name AS metric_a - metric_b`)
- [x] **DRV-02**: Derived metrics expand by inlining referenced metrics' aggregate expressions
- [x] **DRV-03**: Derived metrics can reference other derived metrics (stacking); expansion resolves in topological order
- [x] **DRV-04**: Define-time validation rejects derived metric cycles and references to non-existent metrics
- [x] **DRV-05**: Derived metrics cannot contain aggregation functions (define-time validation)

### Hierarchies

- [x] **HIER-01**: User can declare drill-down paths in a HIERARCHIES clause (`name AS (dim1, dim2, dim3)`)
- [x] **HIER-02**: Define-time validation rejects hierarchies referencing non-existent dimensions
- [x] **HIER-03**: DESCRIBE SEMANTIC VIEW shows hierarchy definitions

### Multiple Join Paths

- [x] **JOIN-01**: Multiple named relationships between the same table pair are accepted (diamond rejection relaxed when relationships are named)
- [x] **JOIN-02**: Metrics can declare `USING (relationship_name)` to select a specific join path
- [ ] **JOIN-03**: Expansion generates separate JOINs with relationship-scoped aliases when USING is specified
- [x] **JOIN-04**: Define-time validation rejects USING references to non-existent relationships
- [ ] **JOIN-05**: Querying a dimension from an ambiguous multi-path table without USING produces a clear error

### Role-Playing Dimensions

- [ ] **ROLE-01**: Same physical table joined via different named relationships produces distinct aliases in expanded SQL
- [ ] **ROLE-02**: Dimensions from a role-playing table resolve to the correct alias based on co-queried metric's USING
- [ ] **ROLE-03**: Classic role-playing pattern works end-to-end (e.g., flights with departure/arrival airports)

### Fan Trap Detection

- [x] **FAN-01**: Relationships can optionally declare cardinality type (one_to_one, one_to_many, many_to_one)
- [x] **FAN-02**: Query expansion warns when a metric aggregates across a one-to-many boundary that could inflate results
- [x] **FAN-03**: Fan trap warnings do not block query execution -- query succeeds with warning

## Future Requirements

Deferred to future milestones. Tracked but not in current roadmap.

### Semi-Additive Metrics (v0.5.4)

- **SEMI-01**: Metrics can declare `NON ADDITIVE BY (dim1 DESC, dim2 DESC)` to mark non-additive dimensions
- **SEMI-02**: Expansion generates ROW_NUMBER() window function subquery to filter to latest snapshot before aggregating
- **SEMI-03**: Semi-additive metrics work correctly alongside regular metrics in the same query

### Registry & Demos

- **DIST-01**: Published to DuckDB community extension registry
- **DIST-02**: Real-world TPC-H demo notebook

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Fan trap auto-deduplication | Changes query semantics; detection + warning is the 80/20 |
| Window function metrics | Requires expansion without GROUP BY; orthogonal to aggregation model |
| ASOF / temporal relationships | Complex temporal join semantics; standard equi-joins cover 95% of cases |
| Aggregate facts (COUNT in FACTS) | Blurs row-level boundary; aggregation belongs in METRICS |
| Per-expression COMMENT | No runtime effect; can be added later without breaking changes |
| PUBLIC/PRIVATE visibility | No access control in DuckDB extensions |
| WITH SYNONYMS | AI/natural-language discovery not relevant for SQL-only DuckDB |
| YAML definition format | SQL DDL first per PROJECT.md |
| Cross-view hierarchies | Keep hierarchies within single semantic view's dimension space |
| Cube.dev-style Dijkstra path selection | Explicit USING is more deterministic and Snowflake-aligned |
| Qualified names in query syntax | Keep flat dimension names in queries; qualified names are DDL concern |
| Pre-aggregation / materialization | Deferred per PROJECT.md; DuckDB handles execution |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| FACT-01 | Phase 29 | Complete |
| FACT-02 | Phase 29 | Complete |
| FACT-03 | Phase 29 | Complete |
| FACT-04 | Phase 29 | Complete |
| FACT-05 | Phase 29 | Complete |
| HIER-01 | Phase 29 | Complete |
| HIER-02 | Phase 29 | Complete |
| HIER-03 | Phase 29 | Complete |
| DRV-01 | Phase 30 | Complete |
| DRV-02 | Phase 30 | Complete |
| DRV-03 | Phase 30 | Complete |
| DRV-04 | Phase 30 | Complete |
| DRV-05 | Phase 30 | Complete |
| FAN-01 | Phase 31 | Complete |
| FAN-02 | Phase 31 | Complete |
| FAN-03 | Phase 31 | Complete |
| JOIN-01 | Phase 32 | Complete |
| JOIN-02 | Phase 32 | Complete |
| JOIN-03 | Phase 32 | Pending |
| JOIN-04 | Phase 32 | Complete |
| JOIN-05 | Phase 32 | Pending |
| ROLE-01 | Phase 32 | Pending |
| ROLE-02 | Phase 32 | Pending |
| ROLE-03 | Phase 32 | Pending |

**Coverage:**
- v0.5.3 requirements: 24 total
- Mapped to phases: 24
- Unmapped: 0

---
*Requirements defined: 2026-03-14*
*Last updated: 2026-03-14 after 29-01 completion*
