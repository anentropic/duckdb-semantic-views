---
phase: 57-introspection-diagnostics
verified: 2026-04-20T23:00:00Z
status: passed
score: 7/7 must-haves verified
re_verification: false
---

# Phase 57: Introspection & Diagnostics Verification Report

**Phase Goal:** Users can inspect materialization routing decisions and materialization metadata through existing introspection commands
**Verified:** 2026-04-20
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | explain_semantic_view() output includes a '-- Materialization: <name>' header line when a materialization covers the request | VERIFIED | explain.rs:228 `lines.push(format!("-- Materialization: {name}"))` + sqllogictest asserts `-- Materialization: region_agg` |
| 2 | explain_semantic_view() output includes '-- Materialization: none' when no materialization matches | VERIFIED | explain.rs:229 `lines.push("-- Materialization: none")` + sqllogictest asserts `-- Materialization: none` for both non-matching and view-without-mats cases |
| 3 | DESCRIBE SEMANTIC VIEW includes MATERIALIZATION rows with TABLE, DIMENSIONS, METRICS properties | VERIFIED | describe.rs:461-485 `collect_materialization_rows` emits TABLE/DIMENSIONS/METRICS rows with `object_kind="MATERIALIZATION"` + sqllogictest asserts 3 rows for p57_mat_view |
| 4 | DESCRIBE SEMANTIC VIEW without materializations produces no MATERIALIZATION rows (unchanged output) | VERIFIED | `collect_materialization_rows` iterates empty vec = no rows emitted + sqllogictest `COUNT(*) = 0` assertion for p57_no_mat_view |
| 5 | SHOW SEMANTIC MATERIALIZATIONS IN view_name lists materialization rows with name, table, dimensions, metrics | VERIFIED | `ShowSemanticMaterializationsVTab` in show_materializations.rs; 7-column output; parser detects and rewrites to function call; sqllogictest confirms row output |
| 6 | SHOW SEMANTIC MATERIALIZATIONS (cross-view form) lists materializations across all views | VERIFIED | `ShowSemanticMaterializationsAllVTab` iterates full catalog; `DdlKind::ShowMaterializations` with no IN clause rewrites to `show_semantic_materializations_all()`; sqllogictest confirms |
| 7 | SHOW SEMANTIC MATERIALIZATIONS supports LIKE/STARTS WITH/LIMIT filter clauses | VERIFIED | `ShowMaterializations` arm is in the same rewrite branch as ShowDimensions/ShowMetrics/ShowFacts, going through `parse_show_filter_clauses` + `build_filter_suffix` which handle all three; LIKE tested directly in phase57; STARTS WITH and LIMIT covered by phase34 tests via shared infrastructure |

**Score:** 7/7 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/ddl/show_materializations.rs` | ShowSemanticMaterializationsVTab and ShowSemanticMaterializationsAllVTab | VERIFIED | File exists, 230+ lines; both VTab structs present with bind/init/func/parameters; 7-column output (database_name, schema_name, semantic_view_name, name, table, dimensions, metrics); imported and registered in lib.rs |
| `src/expand/materialization.rs` | find_routing_materialization_name() helper function | VERIFIED | `pub(crate) fn find_routing_materialization_name<'a>` at line 88; 5 unit tests present; re-exported via expand/mod.rs feature gate; called in explain.rs |
| `test/sql/phase57_introspection.test` | Integration tests for INTR-01, INTR-02, INTR-03 | VERIFIED | File exists; covers all three requirements with matching/non-matching explain assertions, DESCRIBE MATERIALIZATION row assertions, SHOW single-view/cross-view/LIKE/error cases; listed in TEST_LIST; passes in test suite |

### Key Link Verification

| From | To | Via | Status | Details |
|------|-----|-----|--------|---------|
| src/query/explain.rs | src/expand/materialization.rs | find_routing_materialization_name call in bind | WIRED | explain.rs:9 imports via `use crate::expand::find_routing_materialization_name`; expand/mod.rs:28 re-exports; called at explain.rs:201 with dim_refs/met_refs; result used to emit header at lines 228-229 |
| src/parse.rs | src/ddl/show_materializations.rs | DdlKind::ShowMaterializations detection and rewrite | WIRED | parse.rs:42 DdlKind::ShowMaterializations variant; line 158 detect via match_keyword_prefix; line 240 function_name returns "show_semantic_materializations"; line 612 ShowMaterializations in rewrite arm; line 639 all_fn = "show_semantic_materializations_all"; line 766 DDL_PREFIXES entry; line 706 extract_ddl_name; line 877 validate_and_rewrite |
| src/lib.rs | src/ddl/show_materializations.rs | VTab registration | WIRED | lib.rs:322 imports both VTab types; lines 569+573 register both VTabs with catalog_state extra_info; ddl/mod.rs:13 declares module |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| src/ddl/show_materializations.rs | rows (Vec<ShowMatRow>) | catalog guard via `guard.iter()` / `guard.get(&view_name)` then `SemanticViewDefinition::from_json` -> `def.materializations` | Yes — reads from live catalog state; iterates real `def.materializations` from stored JSON | FLOWING |
| src/query/explain.rs | mat_name | `find_routing_materialization_name(&def, &dim_refs, &met_refs)` which iterates `def.materializations` | Yes — real materialization matching against parsed definition | FLOWING |
| src/ddl/describe.rs collect_materialization_rows | rows (Vec<DescribeRow>) | `def.materializations` from parsed catalog JSON | Yes — iterates real materialization structs from definition | FLOWING |

### Behavioral Spot-Checks

| Behavior | Result | Status |
|----------|--------|--------|
| cargo test (819 total) | 727 unit + 36 proptests + 42 + 5 + 3 + 1 doc = all pass, 0 failed | PASS |
| just test-sql (36 sqllogictests including phase57_introspection.test) | 36 tests run, 0 failed | PASS |
| just test-ducklake-ci (6 DuckLake CI tests) | 6 passed, 0 failed | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| INTR-01 | 57-01-PLAN.md | explain_semantic_view() output includes materialization routing decision (name or "none") and expanded SQL reflects routed table | SATISFIED | find_routing_materialization_name in explain.rs bind; "-- Materialization: {name}" and "-- Materialization: none" emitted; sqllogictest asserts both cases including agg table in SQL |
| INTR-02 | 57-01-PLAN.md | DESCRIBE SEMANTIC VIEW includes materialization entries | SATISFIED | collect_materialization_rows in describe.rs; called from bind() after collect_metric_rows; emits TABLE/DIMENSIONS/METRICS rows per materialization; sqllogictest confirms 3-row output and zero-row for views without mats |
| INTR-03 | 57-01-PLAN.md | SHOW SEMANTIC MATERIALIZATIONS IN view_name lists all declared materializations with covered dimensions and metrics | SATISFIED | ShowSemanticMaterializationsVTab + ShowSemanticMaterializationsAllVTab; full parser pipeline (DdlKind::ShowMaterializations through DDL_PREFIXES, rewrite, extract_ddl_name, validate_and_rewrite); registered in lib.rs; sqllogictest tests single-view, cross-view, LIKE filter, empty result, error case |

### Anti-Patterns Found

None found in any files created or modified by this phase.

### Human Verification Required

None. All behaviors are verifiable programmatically and the full test suite passes.

### Gaps Summary

No gaps. All seven must-have truths are satisfied, all three artifacts exist and are substantive and wired, all three key links are connected, all three requirements are satisfied, and the full test suite passes with zero failures.

---

_Verified: 2026-04-20_
_Verifier: Claude (gsd-verifier)_
