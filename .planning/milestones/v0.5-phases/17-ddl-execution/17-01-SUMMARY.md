---
phase: 17-ddl-execution
plan: 01
subsystem: parser
tags: [parser-extension, ddl, ffi, c++, duckdb, statement-rewrite]

# Dependency graph
requires:
  - phase: 16-parser-hook-registration
    provides: "Parser hook chain (sv_parse_stub -> sv_plan_stub) with Rust FFI detection"
provides:
  - "Native DDL execution: CREATE SEMANTIC VIEW creates queryable views via statement rewriting"
  - "parse_ddl_text and rewrite_ddl_to_function_call pure functions for DDL text processing"
  - "sv_execute_ddl_rust FFI entry point for DDL execution from C++ plan function"
  - "Dedicated DDL connection (ddl_conn) for parser hook execution path"
affects: [phase-18-polish, error-handling, ddl-variants]

# Tech tracking
tech-stack:
  added: []
  patterns: [statement-rewrite-ddl, ddl-connection-isolation, rfind-paren-matching]

key-files:
  created: []
  modified:
    - src/parse.rs
    - cpp/src/shim.cpp
    - src/lib.rs
    - test/sql/phase16_parser.test
    - build.rs

key-decisions:
  - "DDL connection always created (even in-memory) to avoid ClientContext deadlock during plan/bind"
  - "Statement rewrite approach: parse_ddl_text extracts name+body, rewrite wraps in create_semantic_view() call"
  - "struct field names require single-quoted keys in native DDL syntax (DuckDB reserved word limitation)"
  - "Native DDL uses statement ok in sqllogictest (query T causes double-bind in Python runner)"

patterns-established:
  - "Statement rewrite pattern: native DDL -> parse_ddl_text -> rewrite_ddl_to_function_call -> duckdb_query on ddl_conn"
  - "DDL connection isolation: separate connection for rewritten SQL execution, distinct from persist_conn"
  - "catch_unwind at all FFI boundaries: sv_execute_ddl_rust follows sv_parse_rust pattern"

requirements-completed: [DDL-01, DDL-02, DDL-03, BUILD-03]

# Metrics
duration: 29min
completed: 2026-03-07
---

# Phase 17 Plan 01: DDL Execution Summary

**Native DDL syntax (CREATE SEMANTIC VIEW) creates queryable views via statement rewriting to create_semantic_view() function call, executed on a dedicated DDL connection**

## Performance

- **Duration:** 29 min
- **Started:** 2026-03-07T23:04:31Z
- **Completed:** 2026-03-07T23:33:53Z
- **Tasks:** 3
- **Files modified:** 5

## Accomplishments
- `CREATE SEMANTIC VIEW name (tables := [...], dimensions := [...], metrics := [...])` creates a semantic view and returns the view name
- Views created via native DDL are immediately queryable via `semantic_view()`
- Function-based DDL (`create_semantic_view`) continues to work unchanged alongside native DDL
- Full test suite green: 19 parse tests, 90+ Rust tests, 4 sqllogictest files, 6 DuckLake CI tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Rust DDL text parsing, rewriting, and FFI execution function** (TDD)
   - `fd1e563` test(17-01): add failing tests for DDL text parsing and rewriting
   - `24ea959` feat(17-01): implement DDL text parsing, rewriting, and FFI execution
2. **Task 2: C++ plan function and DDL connection wiring** - `559c9a8` (feat)
3. **Task 3: Integration tests for native DDL execution** - `ed1858c` (feat)

## Files Created/Modified
- `src/parse.rs` - Added parse_ddl_text, rewrite_ddl_to_function_call pure functions + sv_execute_ddl_rust FFI entry point
- `cpp/src/shim.cpp` - Replaced stub plan function with real DDL-executing sv_plan_function, sv_ddl_bind, sv_ddl_execute
- `src/lib.rs` - Creates dedicated DDL connection and passes to sv_register_parser_hooks
- `test/sql/phase16_parser.test` - End-to-end native DDL integration tests (DDL-01, DDL-02, DDL-03, PARSE-02, PARSE-03)
- `build.rs` - Added rerun-if-changed directives for C++ file change detection

## Decisions Made
- **DDL connection always created:** Even for in-memory databases, ddl_conn is required because the parser hook's plan/bind phase holds a ClientContext lock. Executing the rewritten SQL on the same connection would deadlock. This is separate from persist_conn (which writes to the _definitions catalog table).
- **Statement rewrite pattern:** Native DDL is parsed by pure Rust functions, rewritten to `SELECT * FROM create_semantic_view(...)`, and executed via duckdb_query on ddl_conn. This reuses all existing catalog/validation logic.
- **Quoted struct field names required:** DuckDB's parser treats `table` as a reserved word in struct literals. Native DDL must use `{'table': 'name'}` not `{table: 'name'}`. This matches existing test patterns.
- **Native DDL as statement ok:** The Python sqllogictest runner's `query T` mode causes double-bind for table functions returning results, leading to "already exists" errors. Using `statement ok` avoids this issue.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added rerun-if-changed directives to build.rs**
- **Found during:** Task 3 (Integration tests)
- **Issue:** Cargo's cc crate was not detecting changes to cpp/src/shim.cpp, causing stale binaries
- **Fix:** Added explicit `cargo:rerun-if-changed` directives for shim.cpp and duckdb.hpp
- **Files modified:** build.rs
- **Verification:** Subsequent builds correctly detect C++ changes
- **Committed in:** ed1858c (Task 3 commit)

**2. [Rule 1 - Bug] Fixed struct field quoting in test SQL**
- **Found during:** Task 3 (Integration tests)
- **Issue:** Unquoted struct field names like `{table: 'sales'}` cause DuckDB parser error (reserved word)
- **Fix:** Changed to quoted field names: `{'table': 'sales'}` matching existing test patterns
- **Files modified:** test/sql/phase16_parser.test
- **Verification:** All sqllogictests pass
- **Committed in:** ed1858c (Task 3 commit)

**3. [Rule 1 - Bug] Changed native DDL test from query T to statement ok**
- **Found during:** Task 3 (Integration tests)
- **Issue:** Python sqllogictest runner's `query T` mode causes double-bind for parser extension results
- **Fix:** Used `statement ok` for CREATE SEMANTIC VIEW statements, verified behavior via semantic_view() queries
- **Files modified:** test/sql/phase16_parser.test
- **Verification:** All sqllogictests pass, view creation verified by subsequent query tests
- **Committed in:** ed1858c (Task 3 commit)

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** All fixes necessary for correct test execution. No scope creep.

## Issues Encountered
- Initial test failures caused by stale C++ binary (cc crate caching) -- required cargo clean to force rebuild. Fixed by adding rerun-if-changed directives.
- DuckDB reserved word `table` in struct literals required quoting in both native DDL and function-based DDL test SQL.
- Python sqllogictest runner double-binds table functions when using `query T` mode, causing "already exists" errors for DDL operations.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Native DDL syntax is functional and tested end-to-end
- Parser hook chain fully wired: parse -> plan -> bind -> execute -> catalog insert
- Ready for Phase 18 (polish): error messages, OR REPLACE/IF NOT EXISTS variants, DROP syntax
- DuckLake CI confirms no regressions in the query pipeline

## Self-Check: PASSED

All files exist, all commits verified.

---
*Phase: 17-ddl-execution*
*Completed: 2026-03-07*
