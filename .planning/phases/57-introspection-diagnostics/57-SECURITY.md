---
phase: 57
slug: introspection-diagnostics
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-21
---

# Phase 57 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| DDL input -> parser | User-supplied SQL text (view names, LIKE patterns) crosses into parser detection and rewriting | VARCHAR strings (view names, filter patterns) |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-57-01 | Tampering | parse.rs rewrite_ddl (SHOW MATERIALIZATIONS) | mitigate | View name sanitized via `.replace('\'', "''")` in rewrite_ddl at line 623 — same match arm as SHOW DIMENSIONS/METRICS/FACTS (line 612). No new escaping code needed. | closed |
| T-57-02 | Tampering | parse.rs LIKE pattern (SHOW MATERIALIZATIONS LIKE) | accept | LIKE pattern handled by existing `parse_show_filter_clauses` + `build_filter_suffix` which sanitize quoted strings via `extract_quoted_string`. No new code path. | closed |
| T-57-03 | Information Disclosure | show_materializations.rs | accept | Materialization metadata (table names, covered dims/mets) is user-defined catalog data. Users who can query already see this via DESCRIBE or GET_DDL. No new information surface. | closed |

*Status: open · closed*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-57-01 | T-57-02 | LIKE pattern sanitization already handled by shared infrastructure (parse_show_filter_clauses). No new code path introduced. | gsd-plan-phase | 2026-04-21 |
| AR-57-02 | T-57-03 | Materialization metadata is user-defined and already accessible via DESCRIBE and GET_DDL. No privilege escalation. | gsd-plan-phase | 2026-04-21 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-21 | 3 | 3 | 0 | gsd-secure-phase |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-21
