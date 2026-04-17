---
phase: 49-security-correctness-hardening
verified: 2026-04-14T09:00:00Z
status: passed
score: 4/4 must-haves verified
---

# Phase 49: Security & Correctness Hardening Verification Report

**Phase Goal:** Harden the extension against FFI panics, lock poisoning, and resource exhaustion from malformed definitions
**Verified:** 2026-04-14T09:00:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #   | Truth                                                                                                                        | Status     | Evidence                                                                                                                                                              |
| --- | ---------------------------------------------------------------------------------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | All FFI query-path entry points are wrapped in catch_unwind so Rust panics cannot unwind through C++ stack frames           | ✓ VERIFIED | 25 catch_unwind call sites: 18 VTab bind(), SemanticViewVTab func(), GetDdlScalar invoke(), extension init, 4 FFI catalog functions. See src/util.rs, src/lib.rs, src/ddl/*, src/query/*, src/catalog.rs |
| 2   | Poisoned RwLock/Mutex states are handled gracefully (error returned, not panic) in catalog and query paths                  | ✓ VERIFIED | catalog.rs lines 103-172: all write()/read() use .map_err(); 5 poisoned-lock unit tests pass; CatalogPoisoned ExpandError variant added |
| 3   | Derived metric and fact resolution enforces a cycle detection check and a maximum nesting depth limit, returning a clear error on violation | ✓ VERIFIED | toposort_derived returns Result with cycle error; inline_derived_metrics returns Result and enforces MAX_DERIVATION_DEPTH=64; toposort_facts errors propagated via CycleDetected; 6 unit tests covering cycles and depth |
| 4   | Unsafe pointer arithmetic in test helpers has bounds checks guarding against out-of-range row indices                        | ✓ VERIFIED | src/lib.rs lines 92-99: debug_assert! on both row_idx and col_idx before pointer arithmetic in read_typed_value                                                      |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact                        | Expected                                                  | Status     | Details                                                                                   |
| ------------------------------- | --------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------- |
| `src/catalog.rs`                | Lock acquisition via map_err instead of .unwrap()         | ✓ VERIFIED | Lines 103-172: all write()/read() use .map_err(); 4 FFI catalog functions in catch_unwind (lines 212, 232, 251, 270) |
| `src/expand/types.rs`           | CatalogPoisoned, CycleDetected, MaxDepthExceeded variants | ✓ VERIFIED | Lines 94, 96, 101: all three variants present with Display implementations                |
| `src/expand/facts.rs`           | Cycle detection in toposort_derived, MAX_DERIVATION_DEPTH | ✓ VERIFIED | Line 9: const MAX_DERIVATION_DEPTH = 64; line 213: toposort_derived returns Result; line 336: depth check; 6 tests |
| `src/lib.rs`                    | debug_assert! bounds + catch_unwind on extension init     | ✓ VERIFIED | Lines 92-99: debug_assert! on row_idx and col_idx; line 666: catch_unwind on init         |
| `src/util.rs`                   | catch_unwind_to_result helper                             | ✓ VERIFIED | Lines 77-100: helper function for FFI panic conversion with &str/String payload handling  |
| `src/query/table_function.rs`   | catch_unwind in VTab bind and func                        | ✓ VERIFIED | Lines 403 and 661: both bind() and func() wrapped                                        |
| `src/expand/sql_gen.rs`         | toposort_facts errors propagated (no unwrap_or_default)   | ✓ VERIFIED | Lines 94 and 299: both call sites use .map_err() with ExpandError::CycleDetected         |

### Key Link Verification

| From                        | To                           | Via                                                       | Status     | Details                                                                          |
| --------------------------- | ---------------------------- | --------------------------------------------------------- | ---------- | -------------------------------------------------------------------------------- |
| `src/catalog.rs`            | All VTab bind() methods      | catalog_insert/delete/upsert/rename return Result on poisoned lock | ✓ WIRED | map_err pattern confirmed in all 4 write/read lock acquisitions                 |
| `src/query/table_function.rs` | `src/expand/types.rs`       | ExpandError::CatalogPoisoned variant for poisoned lock    | ✓ WIRED    | CatalogPoisoned variant exists in types.rs; table_function.rs uses map_err      |
| `src/expand/facts.rs`       | `src/expand/types.rs`        | toposort_derived returns ExpandError::CycleDetected on cycle | ✓ WIRED | toposort_derived returns Err(String); call sites in sql_gen.rs map to CycleDetected |
| `src/expand/sql_gen.rs`     | `src/expand/facts.rs`        | toposort_facts error propagated instead of .unwrap_or_default() | ✓ WIRED | Both call sites (lines 94, 299) use .map_err(ExpandError::CycleDetected) |
| VTab bind/init/func         | DuckDB C++ runtime           | catch_unwind converts panics to error strings             | ✓ WIRED    | 25 catch_unwind sites confirmed via grep; util.rs helper centralizes the pattern |

### Data-Flow Trace (Level 4)

Not applicable — this phase modifies error-handling paths, not data rendering. No new components that render dynamic data were introduced.

### Behavioral Spot-Checks

| Behavior                                        | Command                                                              | Result                          | Status  |
| ----------------------------------------------- | -------------------------------------------------------------------- | ------------------------------- | ------- |
| Poisoned lock tests pass                        | cargo test -- catalog_insert_poisoned_lock_returns_error             | ok                              | ✓ PASS  |
| All cargo tests pass (665 tests)                | cargo test                                                           | 665 passed; 0 failed            | ✓ PASS  |
| Cycle detection test passes                     | cargo test -- toposort_derived_detects_cycle                         | ok                              | ✓ PASS  |
| Depth limit test passes                         | cargo test -- inline_derived_metrics_depth_limit_exceeded            | ok (included in 665)            | ✓ PASS  |
| MAX_DERIVATION_DEPTH constant is correct        | grep MAX_DERIVATION_DEPTH src/expand/facts.rs                        | const MAX_DERIVATION_DEPTH = 64 | ✓ PASS  |

### Requirements Coverage

| Requirement | Source Plan  | Description                                                        | Status       | Evidence                                                                        |
| ----------- | ------------ | ------------------------------------------------------------------ | ------------ | ------------------------------------------------------------------------------- |
| SEC-01      | 49-02-PLAN   | FFI query-path entry points wrapped in catch_unwind                | ✓ SATISFIED  | 25 catch_unwind sites across ddl/, query/, lib.rs, catalog.rs                  |
| SEC-02      | 49-01-PLAN   | Poisoned RwLock/Mutex handled gracefully with error returns        | ✓ SATISFIED  | .map_err() in all catalog lock acquisitions; Mutex in table_function.rs         |
| SEC-03      | 49-02-PLAN   | Cycle detection and max depth limit in derived metric/fact resolution | ✓ SATISFIED | toposort_derived returns Err on cycle; MAX_DERIVATION_DEPTH=64 enforced        |
| SEC-04      | 49-01-PLAN   | Test helper unsafe pointer arithmetic has bounds checks            | ✓ SATISFIED  | debug_assert! on row_idx and col_idx in read_typed_value (lib.rs:92-99)        |

**Note:** SEC-01 through SEC-04 are not listed in REQUIREMENTS.md's traceability table. They appear only in ROADMAP.md. This is a documentation gap — the requirements exist in the roadmap contract but were not backfilled into REQUIREMENTS.md. The implementation satisfies all four requirements. REQUIREMENTS.md should be updated to include the SEC series.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | — | No TODO/FIXME/placeholder anti-patterns found in modified files | — | — |

All `.unwrap()` / `.expect()` calls remaining in catalog.rs are confined to the `#[cfg(test)] mod tests` block (line 284 onwards), which is acceptable. Tests use `.unwrap()` intentionally to fail loudly on unexpected errors.

### Human Verification Required

None. All success criteria for this phase are verifiable programmatically.

### Gaps Summary

No gaps. All four success criteria are met with concrete, substantive code backed by 665 passing tests. The commits (47a1973, 7c3bb79, 834612d, e998f7a) all exist in git history.

**Documentation note (non-blocking):** SEC-01 through SEC-04 should be added to the REQUIREMENTS.md traceability table to maintain consistency with the tracking format used for other requirements. This is editorial — it does not affect implementation correctness.

---

_Verified: 2026-04-14T09:00:00Z_
_Verifier: Claude (gsd-verifier)_
