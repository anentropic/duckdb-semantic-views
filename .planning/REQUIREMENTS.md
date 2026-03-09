# Requirements: DuckDB Semantic Views

**Defined:** 2026-03-09
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.

## v0.5.1 Requirements

Requirements for DDL polish milestone. Each maps to roadmap phases.

### DDL Statements

- [ ] **DDL-03**: User can drop a semantic view with `DROP SEMANTIC VIEW name`
- [ ] **DDL-04**: User can drop a semantic view idempotently with `DROP SEMANTIC VIEW IF EXISTS name`
- [ ] **DDL-05**: User can replace a semantic view with `CREATE OR REPLACE SEMANTIC VIEW name (...)`
- [ ] **DDL-06**: User can create a semantic view idempotently with `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)`
- [ ] **DDL-07**: User can inspect a semantic view with `DESCRIBE SEMANTIC VIEW name` showing dimensions, metrics, and types
- [ ] **DDL-08**: User can list all semantic views with `SHOW SEMANTIC VIEWS`

### Error Reporting

- [ ] **ERR-01**: Malformed DDL statements show clause-level error hints (e.g., "Error in DIMENSIONS clause")
- [ ] **ERR-02**: Error messages include character position for DuckDB caret rendering
- [ ] **ERR-03**: Misspelled keywords and view names show "did you mean" suggestions

### Documentation

- [ ] **DOC-01**: README includes DDL syntax reference with worked examples

## Future Requirements

Deferred to future milestones. Tracked but not in current roadmap.

### Registry

- **REG-01**: Extension published to DuckDB community extension registry (`INSTALL semantic_views FROM community`)

### Demo

- **DEMO-01**: Real-world TPC-H demo notebook

### Extended DDL (Future)

- **DDL-09**: `DESC` as alias for `DESCRIBE SEMANTIC VIEW`
- **DDL-10**: `SHOW SEMANTIC VIEWS LIKE '%pattern%'` filtering
- **DDL-11**: Schema-qualified names (`myschema.myview`)
- **DDL-12**: Row-per-field DESCRIBE format (one row per dimension/metric/table)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| `ALTER SEMANTIC VIEW` | Not in Snowflake or DuckDB for views. Use `CREATE OR REPLACE` instead. |
| Schema-qualified names | Flat HashMap catalog. Would require fundamental architecture change. |
| `SHOW ... LIKE` filtering | Client-side filtering suffices. Wrap in `SELECT * FROM list_semantic_views() WHERE ...` |
| `DESC` alias | Minor convenience vs. added parser complexity. Add later if requested. |
| ANSI-colored error output | DuckDB error channel is plain text. Colors render as garbage in JDBC/ODBC/Python. |
| `miette`/`ariadne` error crates | Terminal diagnostic renderers incompatible with DuckDB plain-text errors. |
| Full SQL parser (sqlparser/nom/pest) | 7 prefix patterns + clause extraction. Framework adds ~500KB for no benefit. |
| DDL expression validation at define time | Expression validity depends on source table existing. DuckDB validates at query time. |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| DDL-03 | — | Pending |
| DDL-04 | — | Pending |
| DDL-05 | — | Pending |
| DDL-06 | — | Pending |
| DDL-07 | — | Pending |
| DDL-08 | — | Pending |
| ERR-01 | — | Pending |
| ERR-02 | — | Pending |
| ERR-03 | — | Pending |
| DOC-01 | — | Pending |

**Coverage:**
- v0.5.1 requirements: 10 total
- Mapped to phases: 0
- Unmapped: 10 ⚠️

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-09 after initial definition*
