---
phase: quick
plan: 260322-s2y
subsystem: parser
tags: [ddl, show, filtering, like, starts-with, limit, snowflake-parity]

provides:
  - LIKE/STARTS WITH/LIMIT filtering on SHOW SEMANTIC VIEWS
  - Snowflake parity for all four SHOW commands
affects: [docs, reference]

tech-stack:
  added: []
  patterns: [unified SHOW filter clause handling across all DdlKind::Show* variants]

key-files:
  created: []
  modified:
    - src/parse.rs
    - test/sql/phase34_1_1_show_filtering.test

key-decisions:
  - "Merged DdlKind::Show into combined match arm with ShowDimensions/ShowMetrics/ShowFacts to avoid code duplication"
  - "IN clause rejected at parse_show_filter_clauses level with clear error message for Show kind"

requirements-completed: []

duration: 9min
completed: 2026-03-22
---

# Quick Task 260322-s2y: Add LIKE/STARTS WITH/LIMIT Filtering to SHOW SEMANTIC VIEWS Summary

**LIKE/STARTS WITH/LIMIT filter clauses for SHOW SEMANTIC VIEWS, completing Snowflake parity across all four SHOW commands**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-22T20:18:36Z
- **Completed:** 2026-03-22T20:27:19Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- SHOW SEMANTIC VIEWS now supports LIKE, STARTS WITH, and LIMIT clauses matching Snowflake syntax
- IN and FOR METRIC produce clear error messages when used with SHOW SEMANTIC VIEWS
- All four SHOW commands (VIEWS, DIMENSIONS, METRICS, FACTS) now share unified filter clause handling
- 9 new sqllogictest cases covering filtering, composition, case sensitivity, and error paths

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend parse_show_filter_clauses and rewrite_ddl for DdlKind::Show** - `e5c1178` (test: TDD RED) + `285c3bc` (feat: TDD GREEN)
2. **Task 2: Add sqllogictest cases for SHOW SEMANTIC VIEWS filtering** - `5cbb09a` (test)

## Files Created/Modified
- `src/parse.rs` - Added DdlKind::Show to entity match in parse_show_filter_clauses, IN rejection guard, merged Show into combined match arm in rewrite_ddl, 6 unit tests
- `test/sql/phase34_1_1_show_filtering.test` - 9 new sqllogictest cases (tests 19-27) for SHOW SEMANTIC VIEWS filtering

## Decisions Made
- Merged DdlKind::Show into the existing combined match arm for ShowDimensions/ShowMetrics/ShowFacts rather than duplicating the filter-clause logic -- reduces code by sharing parse_show_filter_clauses and build_filter_suffix
- IN clause is rejected at the parse_show_filter_clauses level (before consuming the view name) so the error message is clear: "IN is not valid for SHOW SEMANTIC VIEWS (no scoping view)"

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] cargo fmt + clippy fixes on existing code**
- **Found during:** Task 1 (TDD RED commit)
- **Issue:** Pre-commit hook (`cargo fmt --check` + `cargo clippy -D warnings`) flagged formatting and clippy issues in existing files (alter.rs, show_dims.rs, show_facts.rs, show_metrics.rs, parse.rs) -- likely from a rustfmt version or edition change
- **Fix:** Applied `cargo fmt` across all Rust sources, fixed two clippy warnings (format_push_string in build_filter_suffix, useless_format in parse_show_filter_clauses)
- **Files modified:** src/ddl/alter.rs, src/ddl/show_dims.rs, src/ddl/show_facts.rs, src/ddl/show_metrics.rs, src/parse.rs
- **Verification:** Pre-commit hook passes on subsequent commits
- **Committed in:** e5c1178 (Task 1 RED commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Formatter/clippy fix was necessary to pass pre-commit hook. No scope creep.

## Issues Encountered
None beyond the formatter/clippy pre-commit hook issue described above.

## Next Phase Readiness
- All four SHOW SEMANTIC commands now have full Snowflake parity for filter clauses
- Reference documentation (docs/reference/) may need updating to reflect the new SHOW SEMANTIC VIEWS syntax

---
*Quick task: 260322-s2y*
*Completed: 2026-03-22*

## Self-Check: PASSED

- All files exist: src/parse.rs, test/sql/phase34_1_1_show_filtering.test, SUMMARY.md
- All commits verified: e5c1178, 285c3bc, 5cbb09a
- Quality gate: cargo test (482 pass), sqllogictest (17/17), DuckLake CI (6/6)
