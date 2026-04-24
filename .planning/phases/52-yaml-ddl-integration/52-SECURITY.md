---
phase: 52
slug: yaml-ddl-integration
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-24
---

# Phase 52 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| DDL input -> parser | User-supplied YAML content in dollar-quoted block crosses into deserialization | YAML text (schema definitions, not data) |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-52-01 | Denial of Service | rewrite_ddl_yaml_body | mitigate | `from_yaml_with_size_cap()` enforces 1MB size cap before YAML parsing begins (parse.rs:1298, inherited from Phase 51 YAML_SIZE_CAP) | closed |
| T-52-02 | Tampering | extract_dollar_quoted | mitigate | Trailing content after closing delimiter rejected with ParseError (parse.rs:1289-1295). Prevents SQL injection after YAML block. | closed |
| T-52-03 | Tampering | rewrite_ddl_yaml_body | mitigate | YAML deserialized into typed Rust structs via serde, re-serialized to JSON. Single quotes SQL-escaped via `replace('\'', "''")` (parse.rs:1315-1316). No raw YAML passes through SQL. | closed |
| T-52-04 | Denial of Service | extract_dollar_quoted | accept | Unterminated dollar-quote scans full input. Bounded by 1MB YAML size cap + DDL prefix. Acceptable for CREATE DDL. | closed |

*Status: open / closed*
*Disposition: mitigate (implementation required) / accept (documented risk) / transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-52-01 | T-52-04 | Input bounded by existing 1MB YAML size cap. Linear scan cost acceptable for DDL operations. | Threat analysis | 2026-04-24 |

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
