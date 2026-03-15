---
phase: 33-unique-constraints-cardinality-inference
plan: 01
subsystem: parser
tags: [cardinality, inference, unique-constraints, pk, fk, serde, duckdb]

# Dependency graph
requires:
  - phase: 31-cardinality
    provides: "Cardinality enum (ManyToOne, OneToOne, OneToMany), fk_columns on Join"
provides:
  - "TableRef.unique_constraints (Vec<Vec<String>>) for UNIQUE constraint storage"
  - "Join.ref_columns (Vec<String>) for resolved target-side columns"
  - "Two-variant Cardinality enum (ManyToOne, OneToOne) -- OneToMany removed"
  - "UNIQUE (col, ...) parsing in TABLES entries"
  - "REFERENCES target(col, ...) parsing for explicit ref_columns"
  - "Cardinality keyword removal (MANY TO ONE / ONE TO ONE / ONE TO MANY no longer accepted)"
  - "infer_cardinality function in parse.rs for PK/UNIQUE-based inference"
affects: [33-02-sqllogictest-updates, graph-validation, expand, on-clause-synthesis]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "HashSet-based column matching for cardinality inference (case-insensitive)"
    - "Optional PRIMARY KEY on TABLES entries (fact tables need no PK)"
    - "ref_columns resolution: implicit from target PK or explicit from REFERENCES(cols)"

key-files:
  created: []
  modified:
    - src/model.rs
    - src/body_parser.rs
    - src/parse.rs
    - src/expand.rs
    - src/graph.rs
    - tests/parse_proptest.rs

key-decisions:
  - "Removed OneToMany variant entirely -- cardinality is always from FK-side perspective"
  - "ref_columns resolved at parse time, not deferred to graph validation"
  - "FK/ref column count mismatch caught as define-time error in infer_cardinality"
  - "Case-insensitive column matching using to_ascii_lowercase + HashSet comparison"

patterns-established:
  - "Inference before serialization: infer_cardinality runs between parse_keyword_body and JSON output"
  - "Optional PK: fact tables parsed with empty pk_columns, UNIQUE constraints still allowed"
  - "Explicit ref_columns override: REFERENCES target(col) skips PK resolution"

requirements-completed: [CARD-01, CARD-02, CARD-04, CARD-05, CARD-06, CARD-07]

# Metrics
duration: 25min
completed: 2026-03-15
---

# Phase 33 Plan 01: Model, Parser, and Inference Summary

**UNIQUE constraints on TableRef, ref_columns on Join, two-variant Cardinality enum, and PK/UNIQUE-based cardinality inference at parse time**

## Performance

- **Duration:** 25 min
- **Started:** 2026-03-15T18:00:00Z
- **Completed:** 2026-03-15T18:25:00Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments
- Extended TableRef with unique_constraints and Join with ref_columns, both backward-compatible via serde defaults
- Removed OneToMany Cardinality variant; updated fan trap detection in expand.rs for 2-variant model
- Rewrote TABLES and RELATIONSHIPS parsing: optional PK, UNIQUE constraints, REFERENCES(cols), no cardinality keywords
- Implemented infer_cardinality in parse.rs: resolves ref_columns from target PK, infers OneToOne vs ManyToOne from FK-side PK/UNIQUE match

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend model with UNIQUE constraints, ref_columns, and two-variant Cardinality** - `b7bfce9` (feat)
2. **Task 2: Update parser -- UNIQUE parsing, remove cardinality keywords, REFERENCES(cols)** - `8f38735` (feat)
3. **Task 3: Implement cardinality inference in parse.rs** - `705e517` (feat)

## Files Created/Modified
- `src/model.rs` - Added unique_constraints on TableRef, ref_columns on Join, removed OneToMany from Cardinality
- `src/body_parser.rs` - Rewrote table/relationship parsing: optional PK, UNIQUE constraints, REFERENCES(cols), deleted parse_cardinality_tokens
- `src/parse.rs` - Added infer_cardinality function, inserted inference call in rewrite_ddl_keyword_body, 9 inference tests
- `src/expand.rs` - Updated fan trap detection for 2-variant Cardinality (removed OneToMany checks)
- `src/graph.rs` - Updated TableRef struct literals with unique_constraints field
- `tests/parse_proptest.rs` - Removed cardinality keyword proptests, kept relationship_no_cardinality_defaults

## Decisions Made
- Removed OneToMany variant entirely rather than keeping it as an alias -- cleaner 2-variant model
- ref_columns resolved at parse time in infer_cardinality rather than deferring to graph validation -- keeps all inference centralized
- FK/ref column count mismatch caught as a define-time error for immediate feedback
- Case-insensitive column matching via to_ascii_lowercase + HashSet for robust PK/UNIQUE matching

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated expand.rs fan trap detection for 2-variant model**
- **Found during:** Task 1 (model changes)
- **Issue:** Removing OneToMany from Cardinality broke fan trap detection in expand.rs which matched on all 3 variants
- **Fix:** Removed OneToMany match arms from check_path_up and check_path_down; restructured fan trap test to use ManyToOne with reversed direction
- **Files modified:** src/expand.rs
- **Verification:** cargo test expand -- all fan trap tests pass
- **Committed in:** b7bfce9 (Task 1 commit)

**2. [Rule 3 - Blocking] Updated graph.rs and expand.rs TableRef struct literals**
- **Found during:** Task 1 (model changes)
- **Issue:** Adding unique_constraints field to TableRef broke ~35 struct literals across graph.rs and expand.rs that used explicit fields without ..Default::default()
- **Fix:** Added unique_constraints: vec![] to all affected struct literals
- **Files modified:** src/graph.rs, src/expand.rs
- **Verification:** cargo test -- all tests pass
- **Committed in:** b7bfce9 (Task 1 commit)

**3. [Rule 3 - Blocking] Updated body_parser.rs for OneToMany removal**
- **Found during:** Task 1 (model changes)
- **Issue:** body_parser.rs referenced Cardinality::OneToMany in parse_cardinality_tokens and tests, preventing compilation
- **Fix:** Minimal fix: changed OneToMany match arm to return error, updated test to expect rejection, added unique_constraints/ref_columns to struct literals
- **Files modified:** src/body_parser.rs
- **Verification:** cargo test body_parser passes
- **Committed in:** b7bfce9 (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All auto-fixes necessary for compilation after model changes. No scope creep.

## Issues Encountered
- Pre-commit hook (rustfmt) reformatted function signatures across multiple commits -- resolved by running cargo fmt before re-staging
- Clippy doc_markdown lint caught unquoted identifiers in doc comments -- resolved with backtick escaping
- Clippy too_many_lines lint on rewritten parse functions -- resolved with #[allow] annotations

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Model, parser, and inference logic are complete
- cargo test passes (462 tests)
- sqllogictests and DuckLake CI tests will FAIL because existing .slt files still use cardinality keywords -- Plan 02 updates those
- Graph validation for REFERENCES(cols) against target PK/UNIQUE (CARD-03) deferred to Plan 02

## Self-Check: PASSED

- All 6 modified files exist on disk
- All 3 task commits verified (b7bfce9, 8f38735, 705e517)
- infer_cardinality function present in parse.rs
- keyword_body is mutable, inference call inserted before serialization
- arb_cardinality_keyword and ONE TO MANY removed from parse_proptest.rs
- cargo test: 462 tests pass (370 + 6 + 36 + 44 + 5 + 1)

---
*Phase: 33-unique-constraints-cardinality-inference*
*Completed: 2026-03-15*
