---
phase: 54
slug: materialization-model-ddl
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-24
---

# Phase 54 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| User DDL input -> body parser | SQL/YAML input parsed into Materialization structs | Materialization names, table names, dim/metric references |
| Materialization table name -> stored JSON | User-provided table name stored as data, not interpolated into SQL at define time | JSON string data via serde |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-54-01 | Tampering | Materialization table name in stored JSON | accept | Table name stored as JSON string data via serde. NOT interpolated into SQL at define time. At query time (Phase 55), quoted via `quote_table_ref()`. | closed |
| T-54-02 | Denial of Service | Oversized MATERIALIZATIONS clause | mitigate | Body parser depth tracking (`split_at_depth0_commas`) limits nesting. YAML path has YAML_SIZE_CAP (1 MiB). SQL DDL bounded by DuckDB statement size limits. | closed |
| T-54-03 | Information Disclosure | Materialization table names in GET_DDL | accept | GET_DDL exposes stored definition metadata by design. Materialization table names are not secrets -- user explicitly declared them. Same trust model as TABLES clause. | closed |

*Status: open / closed*
*Disposition: mitigate (implementation required) / accept (documented risk) / transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-54-01 | T-54-01 | Table name is data, not code. Stored via serde JSON serialization. SQL injection prevented at query time by identifier quoting in Phase 55. | Threat analysis | 2026-04-24 |
| AR-54-02 | T-54-03 | GET_DDL is a metadata introspection function. Exposing user-declared table names is its purpose. | Threat analysis | 2026-04-24 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-24 | 3 | 3 | 0 | gsd-secure-phase |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-24
