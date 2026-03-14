---
phase: 25-sql-body-parser
plan: 01
subsystem: parser
tags: [rust, cpp, body-parser, sqllogictest, proptest, ffi, model]

# Dependency graph
requires:
  - phase: 25-sql-body-parser
    provides: Phase context and research for SQL keyword body parser
provides:
  - "64 KB heap-allocated C++ DDL buffer in sv_parse_stub and sv_ddl_bind"
  - "body_parser.rs module skeleton with #[should_panic] unit test stubs"
  - "test/sql/phase25_keyword_body.test with all 7 DDL verb integration tests"
  - "parse_proptest.rs TEST-06 AS-body proptest block (3 properties)"
  - "model.rs TableRef.pk_columns and Join.{from_alias, fk_columns, name} fields"
affects: [25-02, 25-03, 26-query-builder, 27-execution]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Wave 0 gap contract: #[should_panic] test stubs document expected behavior, turn green in Wave 1"
    - "std::string heap allocation for C++ FFI output buffers (replaces fixed-size stack arrays)"
    - "skip_serializing_if = Vec::is_empty / String::is_empty for backward-compatible JSON evolution"
    - "Default derive on Join struct enables ..Default::default() for struct literal updates"

key-files:
  created:
    - src/body_parser.rs
    - test/sql/phase25_keyword_body.test
  modified:
    - cpp/src/shim.cpp
    - src/lib.rs
    - src/model.rs
    - src/expand.rs
    - src/ddl/parse_args.rs
    - tests/expand_proptest.rs
    - tests/parse_proptest.rs

key-decisions:
  - "16 KB for validation path (sv_parse_stub), 64 KB for execution path (sv_ddl_bind) — matches buffer analysis from RESEARCH.md"
  - "Add Phase 24 model fields (pk_columns, from_alias, fk_columns, name) in Plan 01 as Rule 3 auto-fix — Phase 24 plan exists but was not executed"
  - "skip_serializing_if on all new model fields to preserve backward-compatible JSON for existing stored definitions"

patterns-established:
  - "Phase 24 model extension pattern: new Vec/String fields use skip_serializing_if to keep JSON compact"
  - "Stub function pattern: _body/_base_offset prefix + #[allow(dead_code)] for Wave 0 skeleton functions"

requirements-completed: [DDL-01, DDL-07]

# Metrics
duration: 19min
completed: 2026-03-11
---

# Phase 25 Plan 01: SQL Body Parser Foundation Summary

**64 KB C++ DDL buffer, body_parser.rs skeleton with 8 should_panic stubs, sqllogictest coverage for all 7 DDL verbs, and 3 TEST-06 AS-body proptest properties**

## Performance

- **Duration:** 19 min
- **Started:** 2026-03-11T22:46:56Z
- **Completed:** 2026-03-11T23:05:00Z
- **Tasks:** 3
- **Files modified:** 8

## Accomplishments

- Fixed C++ DDL buffer truncation risk: replaced both `char sql_buf[4096]` occurrences with `std::string` heap allocations (16 KB for validation path, 64 KB for execution path)
- Created `src/body_parser.rs` skeleton with 5 public functions and 8 unit tests — all stubs use `todo!()` so `#[should_panic]` tests pass, establishing the Wave 0 gap contract
- Created `test/sql/phase25_keyword_body.test` with all 7 DDL verb integration tests and 2 error cases for Plan 03 acceptance criteria
- Added TEST-06 AS-body proptest block (3 properties) to `tests/parse_proptest.rs`

## Task Commits

1. **Task 1: Fix C++ DDL buffer** - `d0cf2b3` (fix)
2. **Task 2: Create body_parser.rs skeleton** - `16dedac` (feat)
3. **Task 3: sqllogictest file + TEST-06 proptest block** - `5b46274` (feat)

## Files Created/Modified

- `cpp/src/shim.cpp` - Replaced char sql_buf[4096] with std::string in sv_parse_stub (16 KB) and sv_ddl_bind (64 KB)
- `src/body_parser.rs` - New module: 5 stub functions + 8 #[should_panic] unit tests
- `src/lib.rs` - Added pub mod body_parser
- `src/model.rs` - Added pk_columns to TableRef; from_alias, fk_columns, name to Join (all with skip_serializing_if)
- `src/expand.rs` - Updated 10 Join and 4 TableRef struct initializers with ..Default::default()
- `src/ddl/parse_args.rs` - Updated TableRef and Join struct initializers with ..Default::default()
- `tests/expand_proptest.rs` - Updated 2 Join struct initializers with ..Default::default()
- `tests/parse_proptest.rs` - Added build_as_body_suffix helper + TEST-06 proptest block (3 properties)
- `test/sql/phase25_keyword_body.test` - New sqllogictest file with 7 DDL verb tests + 2 error cases

## Decisions Made

- 16 KB for validation path (sv_parse_stub), 64 KB for execution path (sv_ddl_bind) — matches buffer size analysis from Phase 25 RESEARCH.md
- Add Phase 24 model fields in Plan 01 as Rule 3 auto-fix — Phase 24 exists as a plan document but was never executed; the model fields were prerequisites for body_parser.rs tests to compile
- Use `skip_serializing_if` on all new model fields to preserve backward-compatible JSON for existing stored semantic view definitions

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added Phase 24 model fields to TableRef and Join**
- **Found during:** Task 2 (body_parser.rs skeleton creation)
- **Issue:** body_parser.rs tests reference `pk_columns` on `TableRef` and `from_alias`, `fk_columns`, `name` on `Join` — fields that Phase 24 was supposed to add but Phase 24 was never executed. Compilation failed with 19 E0063 errors.
- **Fix:** Added `pk_columns: Vec<String>` to `TableRef`; added `from_alias: String`, `fk_columns: Vec<String>`, `name: Option<String>` to `Join` — all with `#[serde(default, skip_serializing_if)]` for backward compat. Also added `#[derive(Default)]` to `Join`. Updated all struct initializers across expand.rs, parse_args.rs, model tests, and expand_proptest.rs with `..Default::default()`.
- **Files modified:** src/model.rs, src/expand.rs, src/ddl/parse_args.rs, tests/expand_proptest.rs
- **Verification:** `cargo test` passes (178 + 6 + 36 + 36 + 5 + 1 = 262 tests green); existing round-trip tests still pass; new fields don't appear in JSON when empty
- **Committed in:** `16dedac` (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 3 — blocking issue)
**Impact on plan:** The Phase 24 model fields were a prerequisite for the Plan 01 body_parser.rs skeleton to compile. No scope creep — these are exactly the fields Phase 24-01-PLAN.md documents as its target state.

## Issues Encountered

- Pre-commit hook (rustfmt + clippy with `-D warnings`) required two fix cycles on body_parser.rs: first to apply rustfmt formatting, second to add `#[allow(dead_code)]` and prefix unused parameters with `_`. Clean on third attempt.

## Next Phase Readiness

- Plan 02 can implement body_parser.rs parse functions — all test stubs are in place as acceptance criteria
- Plan 03 can wire AS-body dispatch into validate_and_rewrite — TEST-06 proptest block documents the expected detection behavior
- `test/sql/phase25_keyword_body.test` will fail at integration level until Plan 03 is complete (expected)
- Phase 24 model fields are now present in model.rs — Phase 24's remaining plans (24-02) can execute without needing to re-add them

---
*Phase: 25-sql-body-parser*
*Completed: 2026-03-11*
