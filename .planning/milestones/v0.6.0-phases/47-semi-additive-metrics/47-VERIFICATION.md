---
phase: 47-semi-additive-metrics
verified: 2026-04-12T16:30:00Z
status: passed
score: 5/5 must-haves verified
---

# Phase 47: Semi-Additive Metrics Verification Report

**Phase Goal:** Users can declare and query snapshot-style metrics that aggregate correctly over non-additive dimensions
**Verified:** 2026-04-12T16:30:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (Roadmap Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | User can declare NON ADDITIVE BY (dimension [ASC/DESC]) on a metric in DDL and the view stores successfully | VERIFIED | `src/body_parser.rs` parses full ASC/DESC/NULLS FIRST/LAST syntax; validated by define-time dimension check; sqllogictest Test 1 creates view with `NON ADDITIVE BY (report_date DESC)` |
| 2 | Querying a semi-additive metric produces correct results via CTE-based ROW_NUMBER snapshot selection before aggregation | VERIFIED | `src/expand/semi_additive.rs` (786 lines) generates `WITH __sv_snapshot AS (... ROW_NUMBER() OVER (...))` CTE; sqllogictest Test 2 produces Alice=200.00, Bob=250.00 (latest snapshot per customer) |
| 3 | A query mixing regular aggregate metrics and semi-additive metrics in the same request produces correct results for both | VERIFIED | Conditional `CASE WHEN __sv_rn = 1 THEN val END` aggregation for semi-additive; plain aggregation for regular; sqllogictest Test 5 shows Alice=(3, 200.00), Bob=(2, 250.00) |
| 4 | Semi-additive metrics interact correctly with fan trap detection (no false positives or missed fan traps) | VERIFIED | `fan_trap.rs` skips metrics with non-empty `non_additive_by`; unit test `test_fan_trap_skips_semi_additive` confirms no false positive |
| 5 | Semi-additive metrics work correctly with multi-table JOINs and USING RELATIONSHIPS | VERIFIED | `expand_semi_additive()` calls `resolve_joins_pkfk` and `synthesize_on_clause_scoped` for CTE JOINs; unit test `test_semi_additive_multi_table_join` and sqllogictest Tests 2/5 both use two-table `p47_accounts JOIN p47_customers` |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/model.rs` | NonAdditiveDim, SortOrder, NullsOrder types on Metric struct | VERIFIED | Lines 61, 81, 102 define the enums/struct; line 151 adds `non_additive_by: Vec<NonAdditiveDim>` to Metric with `#[serde(default, skip_serializing_if = "Vec::is_empty")]` |
| `src/body_parser.rs` | NON ADDITIVE BY parser in metric entries | VERIFIED | `find_non_additive_by_keyword`, `parse_non_additive_dims` helpers; MetricEntry extended to 8-tuple; define-time dim reference validation at line 421 |
| `src/render_ddl.rs` | NON ADDITIVE BY emission in GET_DDL output | VERIFIED | Line 159 emits `NON ADDITIVE BY (`; always emits explicit NULLS LAST/FIRST |
| `src/ddl/describe.rs` | NON_ADDITIVE_BY property row in DESCRIBE output | VERIFIED | Line 422 pushes property `"NON_ADDITIVE_BY"` |
| `src/expand/test_helpers.rs` | with_non_additive_by builder method | VERIFIED | Lines 141 and 281 define the trait method and implementation |
| `src/expand/semi_additive.rs` | CTE-based snapshot selection SQL generation | VERIFIED | 786-line module with `expand_semi_additive()`, `ROW_NUMBER() OVER`, collect_na_groups, extract helpers; 12 unit tests |
| `src/expand/sql_gen.rs` | Dispatch to semi-additive expansion when any metric has active non_additive_by | VERIFIED | Lines 318-326: `has_active_semi_additive` check dispatches to `super::semi_additive::expand_semi_additive()` |
| `src/expand/fan_trap.rs` | Skip fan trap check for semi-additive metrics | VERIFIED | Lines 65-67: `if !met.non_additive_by.is_empty() { continue; }` |
| `test/sql/phase47_semi_additive.test` | End-to-end sqllogictest for semi-additive queries | VERIFIED | 200-line file with 8 test cases: DDL, snapshot query, effectively-regular, validation error, mixed metrics, GET_DDL, DESCRIBE, global aggregate |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/expand/sql_gen.rs` | `src/expand/semi_additive.rs` | `expand()` calls `expand_semi_additive` when `has_active_semi_additive` is true | WIRED | Lines 318-326 confirmed |
| `src/expand/fan_trap.rs` | `src/model.rs` | `check_fan_traps` filters metrics with non-empty `non_additive_by` | WIRED | Line 65 confirmed |
| `src/expand/semi_additive.rs` | `src/expand/sql_gen.rs` | Uses `resolve_joins_pkfk`, `synthesize_on_clause`, `synthesize_on_clause_scoped` | WIRED | Lines 29, 200, 222, 240 confirmed |
| `src/body_parser.rs` | `src/model.rs` | MetricEntry 8-tuple carries `Vec<NonAdditiveDim>` into Metric struct | WIRED | 8-element MetricEntry type alias confirmed; `.map()` conversion includes `non_additive_by` field |
| `src/render_ddl.rs` | `src/model.rs` | `emit_metrics` reads `non_additive_by` field from Metric | WIRED | Line 159 iterates `metric.non_additive_by` |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|--------------|--------|--------------------|--------|
| `semi_additive.rs expand_semi_additive()` | `resolved_mets[i].non_additive_by` | Parsed from DDL body via `parse_keyword_body` -> Metric struct | Yes — populated from user DDL; serde backward compat ensures empty Vec for old views | FLOWING |
| `sqllogictest Test 2` | `total_balance` result rows | `p47_accounts` real table data (5 rows inserted) | Yes — ROW_NUMBER selects latest-date snapshot rows; Alice=200.00 (2024-01-03), Bob=250.00 (2024-01-02) | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| cargo tests pass | `cargo test` | 617 tests (528+36+42+5+5+1), 0 failed | PASS |
| sqllogictest suite passes | `just test-sql` | 29 files, 0 failed, including `phase47_semi_additive.test` | PASS |
| `semi_additive.rs` contains ROW_NUMBER | grep `ROW_NUMBER` | Found at lines 116, 185, 434, 435, 484, 485 | PASS |
| `sql_gen.rs` dispatches to semi_additive | grep `expand_semi_additive` | Found at line 326 | PASS |
| `fan_trap.rs` skips semi-additive metrics | grep `non_additive_by` | Found at line 65 with `continue` | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|---------|
| SEMI-01 | 47-01-PLAN.md | User can declare NON ADDITIVE BY (dimension [ASC/DESC]) on a metric in DDL | SATISFIED | Parser in `body_parser.rs`; serde on model; DESCRIBE property; GET_DDL round-trip; sqllogictest Test 1 |
| SEMI-02 | 47-02-PLAN.md | Semi-additive metrics use CTE-based snapshot selection (ROW_NUMBER) before aggregation | SATISFIED | `semi_additive.rs` generates CTE with ROW_NUMBER; sqllogictest Test 2 produces correct snapshot results |
| SEMI-03 | 47-02-PLAN.md | Queries mixing regular and semi-additive metrics produce correct results | SATISFIED | Conditional CASE WHEN aggregation; sqllogictest Test 5 (row_count=3/2, balance=200.00/250.00) |
| SEMI-04 | 47-02-PLAN.md | Semi-additive metrics interact correctly with fan trap detection | SATISFIED | `fan_trap.rs` skips non_additive_by metrics; unit test `test_fan_trap_skips_semi_additive` |
| SEMI-05 | 47-02-PLAN.md | Semi-additive metrics work with multi-table JOINs and USING RELATIONSHIPS | SATISFIED | `expand_semi_additive` calls `resolve_joins_pkfk` and `synthesize_on_clause_scoped`; unit test `test_semi_additive_multi_table_join`; sqllogictest uses two-table join |

All 5 SEMI requirements are satisfied. No orphaned requirements — REQUIREMENTS.md traceability table maps SEMI-01 through SEMI-05 to Phase 47 and marks all as Complete.

### Anti-Patterns Found

None. No TODO/FIXME/placeholder patterns found in any modified files. No empty implementations. The sqllogictest validation error test (Test 4) correctly confirms define-time checking with a real error message.

### Human Verification Required

None. All success criteria are verifiable programmatically via unit tests and sqllogictest integration tests. The sqllogictest suite directly exercises the DuckDB extension with real data and verifies numeric output.

### Gaps Summary

No gaps. All 5 roadmap success criteria are verified with substantive artifacts, correct wiring, and real data flow confirmed by a passing sqllogictest suite (29 files, 0 failures). The quality gate (`just test-all`) passed: 617 cargo tests + 29 sqllogictest files. Commits f1a0527, d02c87e, ea487d4, and 0b5e0e8 are all present in the git log.

---

_Verified: 2026-04-12T16:30:00Z_
_Verifier: Claude (gsd-verifier)_
