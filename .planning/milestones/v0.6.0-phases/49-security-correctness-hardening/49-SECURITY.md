---
phase: 49
slug: security-correctness-hardening
status: verified
threats_open: 0
asvs_level: 1
created: 2026-04-14
---

# Phase 49 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Catalog lock acquisition | Any thread can poison the RwLock/Mutex; all subsequent acquisitions must handle gracefully | In-memory catalog state (HashMap<String, String>) |
| Test helper FFI | Raw pointer arithmetic on DuckDB data chunks; out-of-bounds = undefined behavior | duckdb_data_chunk raw pointers (test-only) |
| VTab bind/init/func -> DuckDB C++ | Rust panic crossing into C++ = undefined behavior | Function return values, error strings |
| VScalar invoke -> DuckDB C++ | Same boundary as VTab | Scalar function results |
| Extension init -> DuckDB C++ | Panic during LOAD = undefined behavior | Extension registration success/failure |
| FFI catalog functions -> C++ shim | Panic in catalog mutation = undefined behavior | Catalog insert/delete/upsert results (i32) |
| Derived metric expansion | Cyclic definitions bypass CREATE-time validation = infinite loop or stack overflow | Metric expression strings |

---

## Threat Register

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-49-01 | D (DoS) | catalog.rs lock acquisition | mitigate | Replace .unwrap() with .map_err() returning descriptive error string | closed |
| T-49-02 | D (DoS) | query/table_function.rs Mutex lock | mitigate | Replace .unwrap() with .map_err() on Mutex::lock() | closed |
| T-49-03 | T (Tampering) | lib.rs test helper pointer arithmetic | mitigate | Add debug_assert! bounds checks on row_idx and col_idx | closed |
| T-49-04 | D (DoS) | VTab bind/init/func | mitigate | Wrap all 18 VTab bind methods + func in catch_unwind_to_result | closed |
| T-49-05 | D (DoS) | VScalar invoke | mitigate | Wrap GetDdlScalar invoke in catch_unwind_to_result | closed |
| T-49-06 | D (DoS) | extension init | mitigate | Wrap semantic_views_init_c_api_internal call in catch_unwind | closed |
| T-49-07 | D (DoS) | FFI catalog functions | mitigate | Wrap all 4 extern "C" catalog functions in catch_unwind | closed |
| T-49-08 | D (DoS) | expand/facts.rs toposort_derived | mitigate | Return Err on cycle detection instead of silent partial result | closed |
| T-49-09 | D (DoS) | expand/facts.rs inline_derived_metrics | mitigate | Enforce MAX_DERIVATION_DEPTH=64 to prevent stack overflow | closed |
| T-49-10 | D (DoS) | expand/sql_gen.rs toposort_facts | mitigate | Propagate toposort_facts Err as ExpandError::CycleDetected instead of .unwrap_or_default() | closed |

*Status: open · closed*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

No accepted risks.

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-04-14 | 10 | 10 | 0 | gsd-secure-phase orchestrator |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-04-14
