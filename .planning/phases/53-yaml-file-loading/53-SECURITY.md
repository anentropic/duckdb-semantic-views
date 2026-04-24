---
phase: 53
slug: yaml-file-loading
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-24
---

# Phase 53 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| User DDL -> Rust parse | Untrusted file paths in FROM YAML FILE enter the parser | File path string |
| Rust sentinel -> C++ shim | File path crosses FFI boundary via sentinel protocol | Path + metadata via SOH-delimited sentinel |
| C++ shim -> DuckDB read_text() | File path embedded in SQL query for file reading | SQL-escaped file path |
| File content -> YAML parser | External file content enters deserializer | YAML text from filesystem |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-53-01 | Tampering | shim.cpp read_text query | mitigate | File path SQL-escaped (replace `'` with `''`) before embedding in `SELECT content FROM read_text('...')` (shim.cpp:189-193). | closed |
| T-53-02 | Information Disclosure | shim.cpp read_text | mitigate | DuckDB's `read_text()` enforces `enable_external_access` setting. Integration test verifies blocking (phase53_yaml_file.test:192-197). | closed |
| T-53-03 | Information Disclosure | File path traversal | accept | DuckDB's file access controls apply to read_text(). Extension does not add custom path validation. When enable_external_access=true, user has opted into file access. | closed |
| T-53-04 | Denial of Service | Large YAML file | mitigate | `from_yaml_with_size_cap()` enforces 1MB limit before YAML parsing. DuckDB's built-in limits apply to read_text(). | closed |
| T-53-05 | Tampering | Dollar-quote collision | mitigate | Tagged dollar-quote `$__sv_file$...$__sv_file$` used in reconstructed query (shim.cpp:232). Prevents YAML content from breaking out of delimiter. | closed |

*Status: open / closed*
*Disposition: mitigate (implementation required) / accept (documented risk) / transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-53-01 | T-53-03 | DuckDB is the security boundary for file access. Extension delegates to read_text() which respects enable_external_access and allowed_directories. No additional path validation needed. | Threat analysis | 2026-04-24 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-24 | 5 | 5 | 0 | gsd-secure-phase |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-24
