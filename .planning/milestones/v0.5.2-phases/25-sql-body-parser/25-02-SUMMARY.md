---
phase: 25-sql-body-parser
plan: 02
subsystem: parser
tags: [rust, body-parser, recursive-descent, tdd]

# Dependency graph
requires:
  - phase: 25-sql-body-parser
    plan: 01
    provides: body_parser.rs skeleton with #[should_panic] stubs and Phase 24 model fields
provides:
  - "Complete implementation of split_at_depth0_commas"
  - "Complete implementation of find_clause_bounds (clause scanner)"
  - "Complete implementation of parse_tables_clause"
  - "Complete implementation of parse_relationships_clause"
  - "Complete implementation of parse_qualified_entries"
  - "Complete implementation of parse_keyword_body"
  - "28 green unit tests, 0 #[should_panic] stubs, 0 todo!() macros"
affects: [25-03, 26-query-builder, 27-execution]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Depth-tracking comma splitter: split_at_depth0_commas handles nested parens and string literals at depth 0"
    - "Clause scanner: find_clause_bounds scans top-level KEYWORD (...) patterns with ordering/required validation"
    - "Inline format args: Rust 2021 '{var}' interpolation throughout for clippy::uninlined_format_args compliance"
    - "is_some_and: used instead of map_or(false, ...) per clippy::unnecessary_map_or"
    - "Word-boundary keyword matching: find_keyword_ci checks alphanum boundaries to avoid false positives"

key-files:
  created: []
  modified:
    - src/body_parser.rs

key-decisions:
  - "Single commit for both tasks: Tasks 1 and 2 operate on the same file; combined commit is the atomic unit"
  - "find_clause_bounds allowed too_many_lines: function is intentionally long to keep the clause-scanning state machine readable in one place"
  - "PRIMARY KEY keyword uses word-boundary matching via find_keyword_ci to avoid matching 'PRIMARY KEY' inside a string literal or column name containing those words"

patterns-established:
  - "Clause parser pattern: split_at_depth0_commas -> per-entry parse function -> Vec of typed structs"
  - "Error propagation with base_offset: all sub-parsers thread base_offset to produce accurate byte positions"

requirements-completed: [DDL-02, DDL-03, DDL-04, DDL-05]

# Metrics
duration: 8min
completed: 2026-03-11
---

# Phase 25 Plan 02: SQL Body Parser Implementation Summary

**Complete recursive descent parser for AS TABLES/RELATIONSHIPS/DIMENSIONS/METRICS keyword body — 28 unit tests green, 0 stubs remaining**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-11T23:10:08Z
- **Completed:** 2026-03-11T23:17:50Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments

- Replaced all `todo!()` stubs with working implementations across 6 parser functions
- `split_at_depth0_commas`: depth-tracking comma splitter; handles nested parens (`SUM(a, b)` not split), string literals (`'x, y'` not split), trailing commas (discarded)
- `find_clause_bounds`: top-level AS-body scanner using `KEYWORD (...)` pattern; validates clause ordering (TABLES < RELATIONSHIPS? < DIMENSIONS? < METRICS?), required clauses, unknown keywords with Levenshtein "did you mean?" suggestion
- `parse_tables_clause`: parses `alias AS physical_table PRIMARY KEY (col1, col2)` — schema-qualified names (`main.orders`) handled correctly (dot not confused with alias.dim_name split)
- `parse_relationships_clause`: parses `rel_name AS from_alias(fk_cols) REFERENCES to_alias` — relationship name required, composite FK columns supported
- `parse_qualified_entries`: parses `alias.name AS expr` — nested paren expressions captured verbatim, trailing commas tolerated
- `parse_keyword_body`: top-level assembler; strips leading "AS", calls `find_clause_bounds`, dispatches to per-clause parsers, assembles `KeywordBody`
- Removed all 8 `#[should_panic]` stubs from Plan 01; added 20 new behavior tests
- All clippy pedantic warnings resolved (inline format args, is_some_and, too_many_lines allow)

## Task Commits

1. **Tasks 1 + 2: Complete keyword body parser** — `129a6b7` (feat)
   - Both tasks combined in one commit since they operate on the same file as a coherent unit

## Files Created/Modified

- `src/body_parser.rs` — Full implementation: 5 public/crate-public functions + 6 private helpers + 28 unit tests

## Decisions Made

- Single commit for both tasks: Tasks 1 and 2 share the same file and form a single coherent implementation unit; splitting would produce a non-compilable intermediate commit
- `#[allow(clippy::too_many_lines)]` on `find_clause_bounds`: the function is intentionally 150 lines to keep clause-scanning state machine readable in one place without artificial helper extraction
- Word-boundary matching in `find_keyword_ci`: checks that the keyword is preceded/followed by non-alphanumeric chars to avoid matching "AS" inside "CAST" or "PRIMARY KEY" inside a longer identifier

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Added `#[derive(Debug)]` to `KeywordBody`**
- **Found during:** Task 1 implementation (compile error)
- **Issue:** `Result<KeywordBody, ParseError>::unwrap_err()` requires `T: Debug`; test assertions calling `result.map(|_| ())` triggered the constraint
- **Fix:** Added `#[derive(Debug)]` to `KeywordBody`
- **Files modified:** src/body_parser.rs
- **Commit:** 129a6b7

**2. [Rule 1 - Bug] Fixed 22 clippy pedantic warnings**
- **Found during:** Task 2 verification (pre-commit hook)
- **Issue:** Clippy -D warnings rejects uninlined format args, map_or(false), and casts. Also requires backticks in doc markdown and too_many_lines allow attribute.
- **Fix:** Converted all format! calls to inline `{var}` style; `map_or(false, ...)` -> `is_some_and(...)`; added `#[allow(clippy::too_many_lines)]`; used local variables for multi-use uppercase strings
- **Files modified:** src/body_parser.rs
- **Commit:** 129a6b7

---

**Total deviations:** 2 auto-fixed (Rule 1 — bugs found during implementation)
**Impact on plan:** No scope change. Both fixes were required for compilation and CI to pass.

## Issues Encountered

None beyond the auto-fixed clippy/Debug issues above.

## Next Phase Readiness

- Plan 03 can wire `parse_keyword_body` into `validate_and_rewrite` — all 6 parser functions are implemented and tested
- `test/sql/phase25_keyword_body.test` sqllogictest file (created in Plan 01) will turn green after Plan 03 connects the dispatch path
- The `KeywordBody` struct and all public functions are ready for use by Plan 03

## Self-Check: PASSED

- src/body_parser.rs: FOUND
- 25-02-SUMMARY.md: FOUND
- commit 129a6b7: FOUND

---
*Phase: 25-sql-body-parser*
*Completed: 2026-03-11*
