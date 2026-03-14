---
phase: 30-derived-metrics
verified: 2026-03-14T14:30:00Z
status: passed
score: 9/9 must-haves verified
re_verification: false
---

# Phase 30: Derived Metrics Verification Report

**Phase Goal:** Users can compose metrics from other metrics without writing raw aggregate expressions
**Verified:** 2026-03-14T14:30:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Derived metrics (no table prefix) are parsed alongside qualified metrics in the METRICS clause | VERIFIED | `parse_metrics_clause` in `src/body_parser.rs:711` handles both qualified (`alias.name AS expr`) and unqualified (`name AS expr`) entries, returning `Option<String>` for source alias |
| 2 | Defining a semantic view with derived metric cycles produces a clear error at CREATE time | VERIFIED | `check_derived_metric_cycles` (Kahn's algorithm) in `src/graph.rs`, wired through `validate_derived_metrics`; sqllogictest Test 9 confirms "cycle detected in derived metrics" error message |
| 3 | Defining a semantic view with derived metric references to non-existent metrics produces a clear error at CREATE time | VERIFIED | `check_derived_metric_references` in `src/graph.rs` with "did you mean?" suggestion; sqllogictest Test 10 confirms "unknown metric" error message |
| 4 | Defining a semantic view with an aggregate function inside a derived metric expression produces a clear error at CREATE time | VERIFIED | `contains_aggregate_function` in `src/graph.rs:617` with word-boundary + paren-follows detection; sqllogictest Tests 11-12 confirm "must not contain aggregate function" error for SUM/COUNT/AVG |
| 5 | Existing FACTS, DIMENSIONS, and qualified METRICS parsing is unchanged | VERIFIED | `parse_qualified_entries` untouched; FACTS and DIMENSIONS still route through it; 390 Rust tests pass with zero regressions |
| 6 | Derived metric expressions are resolved by inlining referenced metrics' aggregate expressions at expansion time | VERIFIED | `inline_derived_metrics` in `src/expand.rs:585` pre-computes all metric expressions; `expand()` uses this at line 772; sqllogictest Test 2 confirms `profit = 170.00/90.00` arithmetic |
| 7 | Stacked derived metrics (derived referencing derived) resolve correctly in topological order | VERIFIED | `toposort_derived` (Kahn's algorithm) in `src/expand.rs:504`; sqllogictest Test 6 confirms `margin = profit/revenue*100` stacking with values 56.67/60.00 |
| 8 | Facts are inlined into base metric expressions BEFORE those expressions are used to resolve derived metrics | VERIFIED | `inline_derived_metrics` resolves base metrics first (with `inline_facts`), then replaces metric references in derived metrics; sqllogictest Test 7 confirms `fact -> base -> derived` chain with `net_price` fact |
| 9 | Join resolution includes tables needed by base metrics referenced through derived metrics | VERIFIED | `collect_derived_metric_source_tables` in `src/expand.rs:636` walks dependency graph transitively; `resolve_joins_pkfk` at lines 276-290 uses it for metrics with `source_table: None`; sqllogictest Test 4 (derived-only query) confirms correct JOIN resolution |

**Score:** 9/9 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/body_parser.rs` | `parse_metrics_clause` for mixed qualified/unqualified metric entries | VERIFIED | Function at line 711, returns `Vec<(Option<String>, String, String)>`. Wired into `parse_keyword_body` METRICS arm. 10 unit tests. |
| `src/graph.rs` (validate) | `validate_derived_metrics` with cycle detection, unknown ref checking, aggregate prohibition | VERIFIED | Function at line 677, refactored into 4 helpers. Kahn's algorithm for cycle detection. `extract_identifiers` + SQL keyword skip list for unknown ref detection. 9 unit tests. |
| `src/graph.rs` (aggregate) | `contains_aggregate_function` scanner | VERIFIED | Function at line 617, word-boundary + paren-follows + string-literal-aware. Returns `Option<&'static str>` naming the found aggregate. |
| `src/ddl/define.rs` | `validate_derived_metrics` wired into `bind()` | VERIFIED | Lines 126-128: called after `validate_hierarchies`, maps error to boxed dyn Error. |
| `src/expand.rs` | `inline_derived_metrics`, `toposort_derived`, updated `expand()` and `resolve_joins_pkfk` | VERIFIED | All four functions exist and are substantive. `inline_derived_metrics` called from `expand()` at line 772. |
| `test/sql/phase30_derived_metrics.test` | 12-case end-to-end sqllogictest | VERIFIED | 12 test cases covering: basic derived, stacking, mixed base+derived, facts+derived chain, derived-only query, global aggregate, DESCRIBE output, 4 error cases (cycle, unknown ref, SUM/COUNT/AVG). Passes `just test-sql`. |
| `test/sql/TEST_LIST` | Entry for phase30_derived_metrics.test | VERIFIED | Line 9: `test/sql/phase30_derived_metrics.test` |
| `tests/parse_proptest.rs` | Proptests for derived metric parsing edge cases | VERIFIED | 3 new proptests: `derived_metric_parsing_no_panic`, `mixed_metrics_no_panic`, `quote_ident_safety` (word-boundary replacement). |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/body_parser.rs` | `src/model.rs` | `Metric` struct with `source_table: None` for derived metrics | VERIFIED | Test at line 1682 asserts `source_table.is_none()` for derived entries; qualified entries return `Some(alias)` |
| `src/ddl/define.rs` | `src/graph.rs` | `validate_derived_metrics` call in `bind()` | VERIFIED | Line 127: `crate::graph::validate_derived_metrics(&def)` after `validate_hierarchies` |
| `src/graph.rs` | `src/graph.rs` | `contains_aggregate_function` called from `validate_derived_metrics` | VERIFIED | `check_no_aggregates_in_derived` helper calls `contains_aggregate_function` |
| `src/expand.rs` | `src/expand.rs` | `inline_derived_metrics` called from `expand()` | VERIFIED | Line 772: `let resolved_exprs = inline_derived_metrics(&def.metrics, &def.facts, &topo_order)` |
| `src/expand.rs` | `src/expand.rs` | `resolve_joins_pkfk` collects source_tables through derived metric references | VERIFIED | Lines 283-290: `else` branch on `source_table: None` calls `collect_derived_metric_source_tables` |
| `test/sql/phase30_derived_metrics.test` | `src/expand.rs` | sqllogictest verifies arithmetic correctness of derived metric expansion | VERIFIED | Tests 2-7 verify numeric results for profit (170/90), margin (56.67/60.00), facts chain (120/90). All pass. |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| DRV-01 | 30-01 | User can declare derived metrics without a table prefix (`metric_name AS metric_a - metric_b`) | SATISFIED | `parse_metrics_clause` accepts unqualified entries; stored with `source_table: None`; sqllogictest Test 1 confirms DDL succeeds |
| DRV-02 | 30-02 | Derived metrics expand by inlining referenced metrics' aggregate expressions | SATISFIED | `inline_derived_metrics` replaces metric name references with parenthesized aggregate expressions; sqllogictest Tests 2-5 verify correct arithmetic |
| DRV-03 | 30-02 | Derived metrics can reference other derived metrics (stacking); expansion resolves in topological order | SATISFIED | `toposort_derived` (Kahn's algorithm); sqllogictest Test 6 verifies `margin = profit/revenue*100` resolves correctly |
| DRV-04 | 30-01 | Define-time validation rejects derived metric cycles and references to non-existent metrics | SATISFIED | `check_derived_metric_cycles` and `check_derived_metric_references` in `validate_derived_metrics`; sqllogictest Tests 9-10 verify error messages |
| DRV-05 | 30-01 | Derived metrics cannot contain aggregation functions (define-time validation) | SATISFIED | `contains_aggregate_function` + `check_no_aggregates_in_derived`; sqllogictest Tests 11-12 verify SUM/COUNT/AVG are rejected |

All 5 requirement IDs declared across Phase 30 plans are satisfied. No orphaned requirements found in REQUIREMENTS.md for Phase 30.

### Anti-Patterns Found

No anti-patterns detected. Scanned `src/body_parser.rs`, `src/graph.rs`, `src/expand.rs`, and `src/ddl/define.rs` for TODO/FIXME/HACK/placeholder comments, empty implementations, and stub handlers. All clear.

### Human Verification Required

None. All behaviors (parsing, validation, expansion, join resolution, DESCRIBE output) are verified programmatically via:
- 390 Rust unit tests + proptests (`cargo test` — all pass)
- 9 sqllogictests including 12-case phase30 suite (`just test-sql` — all pass)
- 6 DuckLake CI integration tests (`just test-ducklake-ci` — all pass)

### Full Test Suite Results

```
cargo test:     390 passed, 0 failed
just test-sql:  9/9 test files pass (includes phase30_derived_metrics.test with 12 cases)
just test-ducklake-ci: 6/6 passed
```

Quality gate `just test-all` passes per CLAUDE.md requirements.

### Verified Commits

| Commit | Description |
|--------|-------------|
| `a039d79` | feat(30-01): parse mixed qualified/unqualified metric entries in body_parser.rs |
| `19d56c2` | feat(30-01): validate derived metrics at CREATE time |
| `f02f681` | test(30-02): add failing tests for derived metric inlining |
| `ac52383` | feat(30-02): implement derived metric inlining in expand.rs |
| `14f8921` | test(30-02): end-to-end sqllogictest and proptests for derived metrics |

All 5 commits verified present in repository history.

---

_Verified: 2026-03-14T14:30:00Z_
_Verifier: Claude (gsd-verifier)_
