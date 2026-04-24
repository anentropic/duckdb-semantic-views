---
phase: 56
slug: yaml-export
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-24
---

# Phase 56 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| User SQL -> scalar function | View name string from user query enters the scalar function | VARCHAR argument used as HashMap key lookup |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-56-01 | Tampering | resolve_bare_name | accept | Name used only as HashMap key lookup in CatalogState. `rsplit('.')` is safe string operation. No SQL interpolation. | closed |
| T-56-02 | Information Disclosure | render_yaml_export | mitigate | 5 internal fields stripped via clone + clear + skip_serializing_if: column_type_names, column_types_inferred, created_on, database_name, schema_name. (render_yaml.rs:21-29, model.rs serde annotations) | closed |
| T-56-03 | Denial of Service | yaml_serde::to_string | accept | Output bounded by stored definition size. No amplification possible. Definitions already passed YAML_SIZE_CAP at ingestion. | closed |

*Status: open / closed*
*Disposition: mitigate (implementation required) / accept (documented risk) / transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-56-01 | T-56-01 | View name is used solely as a dictionary key. rsplit is a pure string operation with no code execution path. | Threat analysis | 2026-04-24 |
| AR-56-02 | T-56-03 | Serialization output size is proportional to input definition size. No recursive expansion or amplification exists in yaml_serde serialization. | Threat analysis | 2026-04-24 |

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
