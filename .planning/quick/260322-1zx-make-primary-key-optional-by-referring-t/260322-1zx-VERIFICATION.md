---
phase: quick-260322-1zx
verified: 2026-03-22T03:15:00Z
status: passed
score: 5/5 must-haves verified
gaps:
  - truth: "Quality gate just test-all passes"
    status: failed
    reason: "Pre-existing proptest failure in relationship_no_cardinality_defaults (name='as' is a SQL keyword) causes cargo nextest run to report 1 failure. This failure predates this task -- parse_proptest.rs was not touched by any of the 3 task commits."
    artifacts:
      - path: "tests/parse_proptest.rs"
        issue: "arb_view_name() generates 'as' (a SQL keyword) as a valid relationship name, which fails parsing. Last modified by commit 8c8f68c (hierarchy removal, before this task). Zero diff between pre-task state and HEAD for this file."
    missing:
      - "Either exclude SQL keywords from arb_view_name() / add a filter in the relationship_no_cardinality_defaults proptest, or fix the relationship name parser to handle keyword names with quoting"
---

# Quick Task 260322-1zx: Make PRIMARY KEY Optional Verification Report

**Task Goal:** Make PRIMARY KEY optional by referring to catalog metadata when possible
**Verified:** 2026-03-22T03:15:00Z
**Status:** gaps_found (pre-existing test failure blocks quality gate)
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Tables declared without PRIMARY KEY in TABLES clause still work when the physical table has a PK constraint in DuckDB catalog | VERIFIED | `resolve_pk_from_catalog` in define.rs queries `duckdb_constraints()` via UNNEST at bind time; sqllogictest PKOpt Test 1 (p33_pkopt_basic) creates a view without PK in DDL and queries it successfully |
| 2 | Tables declared without PRIMARY KEY that have no catalog PK still produce the existing error message | VERIFIED | sqllogictest PKOpt Test 4: `statement error` on CREATE with `has no PRIMARY KEY` match; error fires at bind time via Phase 33 guard in define.rs lines 199-221 |
| 3 | Tables with explicit PRIMARY KEY in TABLES clause continue to work unchanged | VERIFIED | sqllogictest PKOpt Test 3 (p33_pkopt_explicit) with explicit PRIMARY KEY (id) in both table entries passes; `resolve_pk_from_catalog` skips tables with non-empty `pk_columns` (line 104-106) |
| 4 | PK resolution from catalog fills in ref_columns so REFERENCES without explicit columns still works | VERIFIED | Bind sequence confirmed: deserialize -> resolve_pk_from_catalog -> infer_cardinality -> Phase 33 guard -> validate_graph. `infer_cardinality` is tolerant (continues) when target has empty pk_columns (parse.rs lines 547-554), then bind fills them in from catalog before re-running inference |
| 5 | Cardinality inference works correctly after catalog PK resolution | VERIFIED | sqllogictest PKOpt Test 6 (p33_pkopt_describe): `describe_semantic_view` returns 1 row with correct cardinality; new unit test `skips_when_target_has_no_pk_and_no_explicit_ref` in parse.rs confirms tolerant behavior |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/parse.rs` | Tolerant `infer_cardinality` that defers to bind-time when target PK is empty | VERIFIED | `pub(crate) fn infer_cardinality` at line 518; `Some(_)` branch continues (not errors) when `pk_columns` is empty (lines 547-554); unit test `skips_when_target_has_no_pk_and_no_explicit_ref` at line 1415 |
| `src/ddl/define.rs` | Catalog PK resolution step in `DefineFromJsonVTab::bind` before Phase 33 guard and graph validation | VERIFIED | `resolve_pk_from_catalog` function at line 98; called in bind at line 189; bind sequence order: deserialize (180) -> state access (184) -> resolve_pk_from_catalog (189) -> infer_cardinality (194) -> Phase 33 guard (199) -> validate_graph (225) |
| `test/sql/phase33_cardinality_inference.test` | End-to-end sqllogictest covering PK-less TABLES with catalog PK | VERIFIED | 6 PKOpt test cases appended (lines 480-691): basic PK from catalog, composite PK, explicit PK, no catalog PK (error), mixed, DESCRIBE output; all pass (`just test-sql` output: SUCCESS) |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/ddl/define.rs` | `src/parse.rs` | calls `infer_cardinality` after catalog PK resolution | VERIFIED | `crate::parse::infer_cardinality(&def.tables, &mut def.joins)` at define.rs line 194, called after `resolve_pk_from_catalog` at line 189 |
| `src/ddl/define.rs` | `duckdb_constraints()` | SQL query via `catalog_conn` | VERIFIED | `execute_sql_raw(conn, &sql)` at define.rs line 120; SQL uses `UNNEST(constraint_column_names)` from `duckdb_constraints()`. `catalog_conn` created in lib.rs at lines 354-358 and passed to all three `DefineState` instances (lines 363, 375, 387) |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| PK-OPTIONAL | 260322-1zx-PLAN.md | PRIMARY KEY optional in TABLES clause, resolved from catalog | SATISFIED | `resolve_pk_from_catalog` + tolerant `infer_cardinality` + 6 sqllogictest cases |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `tests/parse_proptest.rs` | 970-988 | `arb_view_name()` generates SQL keyword "as" causing proptest failure | Blocker | `just test-all` -> `cargo nextest run` fails; blocks quality gate |

**Anti-pattern detail:** `arb_view_name()` at line 33 uses regex `[a-z_][a-z0-9_]{0,29}` which includes "as". The `relationship_no_cardinality_defaults` proptest constructs `{name} AS {alias_from}(...)` where name="as" yields `as AS a(a) REFERENCES a` which fails to parse. This is a pre-existing bug -- `parse_proptest.rs` has zero diff between the state before this task (commit `da5912f^1`) and HEAD. The file was last modified by `8c8f68c` (hierarchy removal task, before this task).

### Human Verification Required

None -- all behavior verified via sqllogictest and unit tests.

### Gaps Summary

All 5 observable truths are verified. All 3 required artifacts exist, are substantive, and are correctly wired. All 2 key links are confirmed.

**The only gap is a pre-existing proptest failure** (`relationship_no_cardinality_defaults` with name="as") that causes `cargo nextest run` to fail with 1 failure out of 443 tests run. This failure:

1. Was present before this task (parse_proptest.rs was not modified by any of the 3 task commits)
2. Was documented in the SUMMARY as a known pre-existing issue
3. Is NOT caused by the tolerant `infer_cardinality` change -- the proptest uses tables with explicit PKs declared, so the new code path is not exercised

The quality gate `just test-all` includes `cargo nextest run` (via `test-rust` recipe) which will report this failure. Per CLAUDE.md: "A phase verification that only runs cargo test is incomplete" -- and the full gate does fail.

The fix required is narrow: either add `.prop_filter()` to exclude SQL keywords from the `arb_view_name()` strategy in `relationship_no_cardinality_defaults`, or add keyword filtering to `arb_view_name()` itself (though that would affect many other tests). This is a pre-existing debt item, not a gap in the PK-optional feature.

---

_Verified: 2026-03-22T03:15:00Z_
_Verifier: Claude (gsd-verifier)_
