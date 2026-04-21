---
phase: 57-introspection-diagnostics
plan: 01
subsystem: introspection
tags: [explain, describe, show, materializations, vtab, parser]

# Dependency graph
requires:
  - phase: 55-materialization-routing-engine
    provides: "try_route_materialization() and materialization model"
  - phase: 54-materialization-model-ddl
    provides: "MATERIALIZATIONS clause in CREATE SEMANTIC VIEW DDL"
provides:
  - "find_routing_materialization_name() helper for materialization name lookup"
  - "EXPLAIN materialization routing header (-- Materialization: name/none)"
  - "DESCRIBE SEMANTIC VIEW MATERIALIZATION rows (TABLE, DIMENSIONS, METRICS)"
  - "SHOW SEMANTIC MATERIALIZATIONS command (single-view and cross-view forms)"
  - "DdlKind::ShowMaterializations parser variant with detection, rewriting, and near-miss"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "VTab pair pattern (single-view + cross-view AllVTab) for SHOW commands"
    - "Feature-gated re-export for cross-module access to extension-only code"

key-files:
  created:
    - "src/ddl/show_materializations.rs"
    - "test/sql/phase57_introspection.test"
  modified:
    - "src/expand/materialization.rs"
    - "src/expand/mod.rs"
    - "src/query/explain.rs"
    - "src/ddl/describe.rs"
    - "src/ddl/mod.rs"
    - "src/lib.rs"
    - "src/parse.rs"
    - "tests/parse_proptest.rs"
    - "test/sql/TEST_LIST"

key-decisions:
  - "find_routing_materialization_name duplicates resolution logic rather than changing expand() return type -- trivial cost, avoids signature change"
  - "Materialization module made pub(crate) via feature-gated re-export (not module visibility) to match existing pattern"
  - "#[allow(dead_code)] on find_routing_materialization_name since it's only used under extension feature"

patterns-established:
  - "SHOW command pattern: DdlKind variant + detect_ddl_prefix + function_name + rewrite_ddl + VTab pair + lib.rs registration"

requirements-completed: [INTR-01, INTR-02, INTR-03]

# Metrics
duration: 95min
completed: 2026-04-20
---

# Phase 57 Plan 01: Introspection & Diagnostics Summary

**Materialization awareness added to EXPLAIN, DESCRIBE, and new SHOW SEMANTIC MATERIALIZATIONS command with 7-column VTab pair, parser integration, and 12 new unit tests + 1 sqllogictest file**

## Performance

- **Duration:** 95 min
- **Started:** 2026-04-20T23:05:16Z
- **Completed:** 2026-04-21T00:40:34Z
- **Tasks:** 3
- **Files modified:** 11

## Accomplishments
- explain_semantic_view() now includes "-- Materialization: <name>" or "-- Materialization: none" in the header section for every query
- DESCRIBE SEMANTIC VIEW emits MATERIALIZATION rows with TABLE, DIMENSIONS, METRICS properties for each declared materialization
- New SHOW SEMANTIC MATERIALIZATIONS command with single-view (IN view_name) and cross-view forms, supporting LIKE/STARTS WITH/LIMIT filtering via existing infrastructure
- 12 new Rust unit tests (5 for find_routing_materialization_name, 7 for parser detection/rewriting) and 1 sqllogictest integration file covering all three requirements

## Task Commits

Each task was committed atomically:

1. **Task 1: Add find_routing_materialization_name helper and EXPLAIN/DESCRIBE materialization introspection** - `50ebef1` (feat)
2. **Task 2: Add SHOW SEMANTIC MATERIALIZATIONS command with parser detection, VTab pair, and registration** - `a34c9bc` (feat)
3. **Task 3: Integration tests and full suite verification** - `53b5ed9` (test)

## Files Created/Modified
- `src/expand/materialization.rs` - Added find_routing_materialization_name() helper with 5 unit tests
- `src/expand/mod.rs` - Feature-gated re-export of find_routing_materialization_name
- `src/query/explain.rs` - Materialization name lookup and header line in EXPLAIN output
- `src/ddl/describe.rs` - collect_materialization_rows() function and bind() call
- `src/ddl/show_materializations.rs` - New VTab pair (ShowSemanticMaterializationsVTab + AllVTab) with 7-column output
- `src/ddl/mod.rs` - Module declaration for show_materializations
- `src/lib.rs` - Import and registration of both VTabs
- `src/parse.rs` - DdlKind::ShowMaterializations variant, detect/rewrite/extract/DDL_PREFIXES/validate_and_rewrite updates, 7 unit tests
- `tests/parse_proptest.rs` - Exhaustive DdlKind match update
- `test/sql/phase57_introspection.test` - Integration tests for INTR-01, INTR-02, INTR-03
- `test/sql/TEST_LIST` - Added phase57_introspection.test

## Decisions Made
- Duplicated name resolution in explain.rs (linear scan of definition arrays) rather than changing expand()'s return type -- the cost is trivial and avoids a signature change that would affect multiple callers
- Used `#[allow(dead_code)]` on find_routing_materialization_name since it's only callable under the `extension` feature (explain.rs is gated behind `#[cfg(feature = "extension")]`)
- Feature-gated re-export in expand/mod.rs follows existing pattern used by collect_derived_metric_source_tables and ancestors_to_root

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Made materialization module accessible across crate**
- **Found during:** Task 1
- **Issue:** explain.rs (in src/query/) couldn't access find_routing_materialization_name in the private materialization module (in src/expand/)
- **Fix:** Added feature-gated re-export in expand/mod.rs and #[allow(dead_code)] on the function
- **Files modified:** src/expand/mod.rs, src/expand/materialization.rs
- **Verification:** cargo test passes, no dead_code warning
- **Committed in:** 50ebef1

**2. [Rule 3 - Blocking] Updated parse_proptest.rs for exhaustive DdlKind match**
- **Found during:** Task 2
- **Issue:** Adding ShowMaterializations to DdlKind enum caused non-exhaustive pattern match in parse_proptest.rs
- **Fix:** Added DdlKind::ShowMaterializations arm to build_suffix match
- **Files modified:** tests/parse_proptest.rs
- **Verification:** cargo test parse passes with 299 tests
- **Committed in:** a34c9bc

**3. [Rule 1 - Bug] Fixed sqllogictest assertion for EXPLAIN agg table reference**
- **Found during:** Task 3
- **Issue:** EXPLAIN output matched two rows for `%p57_agg_region%` (expanded SQL line + DuckDB plan line)
- **Fix:** Changed LIKE filter to `'FROM%p57_agg_region%'` to match only the SQL line
- **Files modified:** test/sql/phase57_introspection.test
- **Verification:** sqllogictest passes
- **Committed in:** 53b5ed9

**4. [Rule 1 - Bug] Fixed statement error syntax for DuckDB 1.5.2 sqllogictest runner**
- **Found during:** Task 3
- **Issue:** Bare `statement error` without expected error message caused parser error in runner
- **Fix:** Added `---- does not exist` expected error lines after statement error
- **Files modified:** test/sql/phase57_introspection.test
- **Verification:** sqllogictest passes
- **Committed in:** 53b5ed9

---

**Total deviations:** 4 auto-fixed (2 blocking, 2 bug)
**Impact on plan:** All auto-fixes necessary for correctness and compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- This is the final phase of milestone v0.7.0
- All introspection surfaces now support materializations
- 819 cargo tests + 36 sqllogictests + 6 DuckLake CI tests pass
- Ready for milestone completion (CHANGELOG, example, version bump)

## Self-Check: PASSED

- FOUND: src/ddl/show_materializations.rs
- FOUND: test/sql/phase57_introspection.test
- FOUND: 50ebef1 (Task 1)
- FOUND: a34c9bc (Task 2)
- FOUND: 53b5ed9 (Task 3)

---
*Phase: 57-introspection-diagnostics*
*Completed: 2026-04-20*
