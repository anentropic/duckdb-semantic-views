---
phase: 51
slug: yaml-parser-core
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-18
---

# Phase 51 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| YAML input -> from_yaml | YAML strings provided by privileged warehouse administrators. DDL operation requiring database write access. NOT an untrusted input boundary. | YAML text (low sensitivity — schema definitions, not data) |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-51-01 | Denial of Service | from_yaml (anchor/alias bomb) | accept | 1MB size cap (`YAML_SIZE_CAP`) as sanity guard. Trust assumption: input from privileged administrators only. | closed |
| T-51-02 | Tampering | from_yaml (type confusion via YAML tags) | mitigate | serde typed deserialization rejects unexpected types at compile time. No custom `Deserialize` impls. YAML tags cannot inject arbitrary types. | closed |
| T-51-03 | Elevation of Privilege | from_yaml (code execution via YAML tags) | accept | Not applicable — serde/yaml_serde/libyaml-rs is a pure data parser. No code execution path exists. No custom deserializers. | closed |

*Status: open · closed*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-51-01 | T-51-01 | YAML input comes from privileged warehouse administrators only. Anchor/alias bomb is an untrusted-input attack vector irrelevant to this trust model. 1MB size cap provides sanity guard. | User (assumptions discussion) | 2026-04-18 |
| AR-51-02 | T-51-03 | serde deserialization is pure data mapping — no constructors, no code execution. yaml_serde/libyaml-rs has no eval/exec path. | Threat analysis | 2026-04-18 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-18 | 3 | 3 | 0 | gsd-secure-phase |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-18
