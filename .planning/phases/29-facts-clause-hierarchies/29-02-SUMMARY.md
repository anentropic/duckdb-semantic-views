---
phase: 29-facts-clause-hierarchies
plan: 02
subsystem: expansion
tags: [fact-inlining, describe, sqllogictest, proptest, word-boundary, toposort]

# Dependency graph
requires:
  - phase: 29-facts-clause-hierarchies
    plan: 01
    provides: "FACTS/HIERARCHIES parsing, validate_facts, validate_hierarchies, Fact/Hierarchy structs"
provides:
  - "Fact expression inlining in expand.rs (replace_word_boundary, toposort_facts, inline_facts)"
  - "DESCRIBE SEMANTIC VIEW with 8 columns (facts + hierarchies added)"
  - "End-to-end sqllogictest for FACTS + HIERARCHIES lifecycle"
  - "Proptest generators for adversarial FACTS/HIERARCHIES input"
  - "Fuzz seed corpus entries for FACTS/HIERARCHIES DDL"
affects: [30-derived-metrics, 31-role-playing-using]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Fact expression inlining: toposort facts, resolve in order, parenthesize"
    - "Word-boundary string replacement for safe identifier substitution"
    - "Null-to-empty-array fallback in DESCRIBE for backward compat"

key-files:
  created:
    - "test/sql/phase29_facts_hierarchies.test"
    - "fuzz/seeds/fuzz_ddl_parse/seed_facts.txt"
    - "fuzz/seeds/fuzz_ddl_parse/seed_hierarchies.txt"
    - "fuzz/seeds/fuzz_ddl_parse/seed_facts_and_hierarchies.txt"
  modified:
    - "src/expand.rs"
    - "src/ddl/describe.rs"
    - "test/sql/phase28_e2e.test"
    - "test/sql/phase20_extended_ddl.test"
    - "test/sql/phase21_error_reporting.test"
    - "test/sql/phase25_keyword_body.test"
    - "test/sql/TEST_LIST"
    - "tests/parse_proptest.rs"

key-decisions:
  - "Fact inlining uses own toposort_facts (not graph.rs) since expand.rs needs indices into the facts slice"
  - "Word-boundary replacement is case-sensitive (fact names are identifiers)"
  - "Null-to-empty-array fallback in DESCRIBE for old definitions without facts/hierarchies fields"
  - "Replaced plan's 'unknown fact reference' error test with 'self-reference' test (find_fact_references only finds known names)"

patterns-established:
  - "Fact inlining: toposort -> resolve in order -> parenthesize -> apply to metric expr"
  - "DESCRIBE column count: 8 (name, base_table, dimensions, metrics, filters, joins, facts, hierarchies)"

requirements-completed: [FACT-02, FACT-05, HIER-03]

# Metrics
duration: 15min
completed: 2026-03-14
---

# Phase 29 Plan 02: Fact Inlining, DESCRIBE Update, and E2E Tests Summary

**Fact expression inlining with topological resolution and parenthesization, DESCRIBE extended to 8 columns, end-to-end sqllogictest with arithmetic verification**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-14T12:09:18Z
- **Completed:** 2026-03-14T12:24:16Z
- **Tasks:** 3
- **Files modified:** 12

## Accomplishments
- Fact expression inlining: metrics referencing fact names expand with parenthesized fact expressions via topological sort
- Multi-level fact chains resolve correctly (e.g., tax_amount -> net_price -> extended_price * (1 - discount))
- DESCRIBE SEMANTIC VIEW now returns 8 columns (added facts + hierarchies JSON arrays)
- End-to-end sqllogictest verifies arithmetic correctness of fact inlining against known data
- 4 proptests cover adversarial FACTS/HIERARCHIES clause input without panics
- Backward compatible: all existing sqllogictests pass with updated 8-column DESCRIBE

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement fact expression inlining in expand.rs** - `5de21d7` (feat)
2. **Task 2: Update DESCRIBE output to include facts and hierarchies columns** - `cfe0575` (feat)
3. **Task 3: End-to-end sqllogictest + proptests for FACTS and HIERARCHIES** - `ebaf428` (test)

## Files Created/Modified
- `src/expand.rs` - Added replace_word_boundary, toposort_facts, inline_facts functions; wired into expand() for metric expressions; 23 new unit tests
- `src/ddl/describe.rs` - Extended DescribeBindData to 8 fields (facts, hierarchies); null-to-[] fallback for old definitions
- `test/sql/phase29_facts_hierarchies.test` - 11 test cases: DDL, fact inlining, multi-level chains, DESCRIBE, error cases, optional clauses
- `test/sql/phase28_e2e.test` - Updated DESCRIBE assertion from 6 to 8 columns
- `test/sql/phase20_extended_ddl.test` - Updated 4 DESCRIBE assertions from 6 to 8 columns
- `test/sql/phase21_error_reporting.test` - Updated DESCRIBE assertion from 6 to 8 columns
- `test/sql/phase25_keyword_body.test` - Updated DESCRIBE assertion from 6 to 8 columns
- `test/sql/TEST_LIST` - Added phase29_facts_hierarchies.test
- `tests/parse_proptest.rs` - Added 4 proptests for FACTS/HIERARCHIES adversarial input
- `fuzz/seeds/fuzz_ddl_parse/seed_facts.txt` - Fuzz seed for FACTS DDL
- `fuzz/seeds/fuzz_ddl_parse/seed_hierarchies.txt` - Fuzz seed for HIERARCHIES DDL
- `fuzz/seeds/fuzz_ddl_parse/seed_facts_and_hierarchies.txt` - Fuzz seed for combined DDL

## Decisions Made
- Fact inlining uses its own `toposort_facts` in expand.rs rather than reusing graph.rs's Kahn's implementation, because expand needs indices into the facts slice for inline resolution
- Word-boundary replacement (`replace_word_boundary`) is case-sensitive since fact names are SQL identifiers
- Old stored definitions without `hierarchies` field produce `null` in JSON; added null-to-`[]` fallback in DESCRIBE to maintain consistent output
- Replaced plan's "unknown fact reference" error test case with "self-reference" test because `find_fact_references` only scans for known fact names (unknown identifiers in expressions are just column references)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Null-to-empty-array fallback for old definitions in DESCRIBE**
- **Found during:** Task 2
- **Issue:** Old stored definitions (created before Phase 29) don't have `hierarchies` field; `def["hierarchies"]` returns `null` instead of `[]`
- **Fix:** Added `is_null()` check before serialization, defaulting to `"[]"` for null facts/hierarchies
- **Files modified:** src/ddl/describe.rs
- **Verification:** All 8 sqllogictests pass with 8-column DESCRIBE
- **Committed in:** cfe0575 (Task 2 commit)

**2. [Rule 3 - Blocking] Updated all existing DESCRIBE assertions across 4 test files**
- **Found during:** Task 2
- **Issue:** Adding 2 new columns to DESCRIBE broke all existing sqllogictest DESCRIBE assertions (6 -> 8 columns)
- **Fix:** Updated `query TTTTTT` to `query TTTTTTTT` and appended `\t[]\t[]` to expected output in phase20, phase21, phase25, and phase28 tests
- **Files modified:** test/sql/phase20_extended_ddl.test, test/sql/phase21_error_reporting.test, test/sql/phase25_keyword_body.test, test/sql/phase28_e2e.test
- **Verification:** All 8 sqllogictest files pass
- **Committed in:** cfe0575 (Task 2 commit)

**3. [Rule 1 - Bug] Replaced invalid "unknown fact reference" error test case**
- **Found during:** Task 3
- **Issue:** Plan specified an "unknown fact reference" error test, but `find_fact_references` only scans for known fact names -- unknown identifiers in expressions are treated as column references, not errors
- **Fix:** Replaced with "fact self-reference" error test (which correctly triggers `cycle detected in facts`)
- **Files modified:** test/sql/phase29_facts_hierarchies.test
- **Verification:** sqllogictest passes
- **Committed in:** ebaf428 (Task 3 commit)

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** All fixes necessary for correctness and test suite integrity. No scope creep.

## Issues Encountered
- Pre-commit hook requires `cargo fmt` + clippy clean pass before commit; first attempt on Task 1 failed due to formatting and clippy `needless_range_loop` lint
- DuckLake CI test fails in sandboxed environment (UV cache permission error); passes when run without sandbox restrictions

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 29 complete: FACTS clause parsing, validation, inlining, and HIERARCHIES metadata all implemented
- Fact inlining pattern (word-boundary replacement + topological sort) available for reuse in Phase 30 derived metrics
- DESCRIBE 8-column schema established; future features adding more columns will follow the same pattern

## Self-Check: PASSED

All files verified present, all commit hashes verified in git log.

---
*Phase: 29-facts-clause-hierarchies*
*Completed: 2026-03-14*
