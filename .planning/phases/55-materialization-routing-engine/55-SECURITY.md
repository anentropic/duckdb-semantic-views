---
phase: 55
slug: materialization-routing-engine
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-24
---

# Phase 55 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Stored definition -> SQL generation | Materialization table names from stored catalog JSON generate SQL FROM clauses | Table/column names via quote_table_ref/quote_ident |
| Query parameters -> name resolution | Requested dimension/metric names from user query resolved against definition | Name strings for HashSet matching |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-55-01 | Tampering | build_materialized_sql | mitigate | Table name passed through `quote_table_ref()` (splits on `.`, wraps each part in double quotes). Column names passed through `quote_ident()`. No raw string interpolation. (materialization.rs:136,145,157) | closed |
| T-55-02 | Information Disclosure | try_route_materialization | accept | Routing may expose materialization table existence via DuckDB error. Acceptable -- metadata already visible via GET_DDL and DESCRIBE. | closed |
| T-55-03 | Denial of Service | HashSet construction | accept | O(n) where n = dims+mets+materializations. Bounded by definition size (typically < 100 entries). No amplification vector. | closed |
| T-55-04 | Elevation of Privilege | Routing to unauthorized table | accept | Extension generates SELECT; DuckDB's privilege system enforces access control. Extension does not bypass any DuckDB security boundary. | closed |

*Status: open / closed*
*Disposition: mitigate (implementation required) / accept (documented risk) / transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-55-01 | T-55-02 | Materialization metadata is already user-visible via GET_DDL and DESCRIBE. DuckDB errors revealing table existence are consistent with standard DuckDB behavior. | Threat analysis | 2026-04-24 |
| AR-55-02 | T-55-03 | Definition size bounded by YAML_SIZE_CAP (1 MiB) and practical DDL limits. HashSet ops are O(1) per lookup. | Threat analysis | 2026-04-24 |
| AR-55-03 | T-55-04 | DuckDB enforces its own access control on all SQL execution. Extension-generated SQL is subject to the same privilege checks as user-written SQL. | Threat analysis | 2026-04-24 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-24 | 4 | 4 | 0 | gsd-secure-phase |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-24
