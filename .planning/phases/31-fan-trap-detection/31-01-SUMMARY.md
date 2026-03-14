---
phase: 31-fan-trap-detection
plan: 01
subsystem: database
tags: [cardinality, relationships, serde, parser, duckdb]

# Dependency graph
requires:
  - phase: 29-facts-hierarchies
    provides: "Relationship model (Join struct) with PK/FK columns"
provides:
  - "Cardinality enum (ManyToOne, OneToOne, OneToMany) on Join struct"
  - "Parser support for MANY TO ONE / ONE TO ONE / ONE TO MANY after REFERENCES"
  - "Backward-compatible serde: old JSON without cardinality defaults to ManyToOne"
affects: [31-02-PLAN (fan trap detection logic uses Cardinality enum)]

# Tech tracking
tech-stack:
  added: []
  patterns: ["skip_serializing_if for enum default variant", "parse_cardinality_tokens token-based keyword matching"]

key-files:
  created: []
  modified:
    - src/model.rs
    - src/body_parser.rs
    - tests/parse_proptest.rs

key-decisions:
  - "Cardinality::is_default() + skip_serializing_if keeps serialized JSON backward-compatible (no cardinality field for ManyToOne)"
  - "Token-split approach for to_alias extraction: first token is alias, remaining tokens are cardinality"

patterns-established:
  - "skip_serializing_if with is_default() method for enum defaults on model structs"

requirements-completed: [FAN-01]

# Metrics
duration: 9min
completed: 2026-03-14
---

# Phase 31 Plan 01: Cardinality Model and Parser Summary

**Cardinality enum (ManyToOne/OneToOne/OneToMany) on Join struct with DDL parser support for optional cardinality keywords after REFERENCES**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-14T18:12:23Z
- **Completed:** 2026-03-14T18:21:23Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Cardinality enum with ManyToOne (default), OneToOne, OneToMany variants added to model
- Parser accepts `rel AS a(fk) REFERENCES b MANY TO ONE` syntax (case-insensitive)
- Backward-compatible serde: old JSON without cardinality field deserializes as ManyToOne
- Invalid cardinality values (e.g., MANY TO MANY) rejected with clear error message
- 11 new unit tests + 2 proptests covering all cardinality scenarios

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Cardinality enum to model and extend Join struct** - `e520720` (feat)
2. **Task 2: Extend relationship parser to accept cardinality keywords** - `4eeb7ff` (feat)

_Both tasks followed TDD flow (tests written first, then implementation)_

## Files Created/Modified
- `src/model.rs` - Cardinality enum, is_default() method, cardinality field on Join with skip_serializing_if
- `src/body_parser.rs` - parse_cardinality_tokens helper, modified parse_single_relationship_entry to split to_alias from cardinality tokens
- `tests/parse_proptest.rs` - 2 proptests for cardinality keyword variants and default handling

## Decisions Made
- Used `skip_serializing_if = "Cardinality::is_default"` to avoid emitting the cardinality field when it is ManyToOne. This preserves backward-compatible JSON output and avoids updating all existing sqllogictest expectations.
- Token-split approach: after REFERENCES, split remaining text into whitespace tokens. First token is to_alias, remaining tokens (if any) form the cardinality keyword sequence. Clean separation without regex.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added skip_serializing_if for default cardinality**
- **Found during:** Task 2 (parser implementation)
- **Issue:** Adding `#[serde(default)]` without `skip_serializing_if` caused `"cardinality":"ManyToOne"` to appear in all serialized Join JSON, breaking sqllogictest expectations
- **Fix:** Added `Cardinality::is_default()` method and `skip_serializing_if = "Cardinality::is_default"` serde attribute
- **Files modified:** src/model.rs
- **Verification:** `just test-sql` passes with all 9 sqllogictest files
- **Committed in:** 4eeb7ff (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Essential fix to preserve backward-compatible JSON serialization. No scope creep.

## Issues Encountered
- Clippy pedantic required backticks around `ManyToOne` in doc comments (fixed inline during Task 1)

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Cardinality data model and parser are complete and ready for Plan 02 (fan trap detection logic)
- Plan 02 can inspect `Join.cardinality` during query expansion to detect and block fan trap scenarios

---
*Phase: 31-fan-trap-detection*
*Completed: 2026-03-14*
