---
phase: 29-facts-clause-hierarchies
plan: 01
subsystem: parser
tags: [ddl, body-parser, facts, hierarchies, dag-validation, kahn-algorithm]

# Dependency graph
requires:
  - phase: 11-keyword-ddl
    provides: "parse_qualified_entries, KeywordBody, SemanticViewDefinition with facts field"
  - phase: 26-graph-validation
    provides: "validate_graph, RelationshipGraph, suggest_closest"
provides:
  - "FACTS clause parsing via parse_qualified_entries (alias.name AS expr)"
  - "HIERARCHIES clause parsing via parse_hierarchies_clause (name AS (dim1, dim2, ...))"
  - "Hierarchy struct in model.rs with serde derives"
  - "validate_facts: source table reachability, cycle detection, unknown fact reference checking"
  - "validate_hierarchies: unknown dimension level checking"
  - "find_fact_references: word-boundary matching for fact name references in expressions"
affects: [30-derived-metrics, 31-role-playing-using, 32-fan-trap-detection]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Fact DAG validation via Kahn's algorithm (same pattern as relationship graph)"
    - "Word-boundary matching for expression scanning (is_word_boundary_byte helper)"
    - "Hierarchies as pure metadata validated against declared dimensions"

key-files:
  created: []
  modified:
    - "src/body_parser.rs"
    - "src/model.rs"
    - "src/parse.rs"
    - "src/graph.rs"
    - "src/ddl/define.rs"
    - "src/expand.rs"
    - "tests/expand_proptest.rs"

key-decisions:
  - "FACTS reuse parse_qualified_entries (same alias.name AS expr pattern as dims/metrics)"
  - "Hierarchies are pure metadata -- only validated against dimension names, not used in expansion"
  - "Fact cycle detection uses Kahn's algorithm (same as relationship graph validation)"
  - "Word-boundary matching uses is_word_boundary_byte (NOT alphanumeric or underscore)"

patterns-established:
  - "Clause ordering: TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS"
  - "Fact DAG validation pattern: build adjacency list from expression scanning, Kahn's toposort"

requirements-completed: [FACT-01, FACT-03, FACT-04, HIER-01, HIER-02]

# Metrics
duration: 72min
completed: 2026-03-14
---

# Phase 29 Plan 01: FACTS/HIERARCHIES Clause Parsing and Validation Summary

**FACTS clause parsing via qualified entries, HIERARCHIES clause with parenthesized levels, fact DAG cycle detection, and hierarchy-to-dimension validation at CREATE time**

## Performance

- **Duration:** 72 min
- **Started:** 2026-03-14T10:52:47Z
- **Completed:** 2026-03-14T12:05:23Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- FACTS and HIERARCHIES clauses parsed in DDL with enforced clause ordering
- Hierarchy struct added to model.rs with backward-compatible serde (skip_serializing_if empty)
- Fact DAG validation: source table reachability, self-reference detection, cycle detection via Kahn's algorithm, word-boundary expression scanning
- Hierarchy validation: all levels checked against declared dimension names with fuzzy suggestions
- Both validations wired into DefineFromJsonVTab bind() at CREATE time
- 32 new tests across body_parser (10), model (4), graph (18) modules

## Task Commits

Each task was committed atomically:

1. **Task 1: Parse FACTS and HIERARCHIES clauses in body_parser + wire through parse.rs** - `f6559a8` (feat)
2. **Task 2: Define-time validation for facts and hierarchies** - `b82df96` (feat)

## Files Created/Modified
- `src/body_parser.rs` - Added FACTS/HIERARCHIES to CLAUSE_KEYWORDS/CLAUSE_ORDER, parse_hierarchies_clause function, facts/hierarchies fields in KeywordBody
- `src/model.rs` - Added Hierarchy struct, hierarchies field to SemanticViewDefinition with serde attributes
- `src/parse.rs` - Wired facts/hierarchies from KeywordBody into SemanticViewDefinition (replaced hardcoded empty vecs)
- `src/graph.rs` - Added validate_facts, validate_hierarchies, find_fact_references, check_fact_source_tables, build_fact_dag, check_fact_cycles helpers
- `src/ddl/define.rs` - Added validate_facts and validate_hierarchies calls after graph validation
- `src/expand.rs` - Added hierarchies field to all SemanticViewDefinition struct literals in tests
- `tests/expand_proptest.rs` - Added hierarchies field to test fixture definitions

## Decisions Made
- FACTS clause reuses parse_qualified_entries (same alias.name AS expr pattern as dims/metrics) -- consistent syntax, no new parser needed
- Hierarchies are pure metadata validated against dimension names -- not used in query expansion
- Word-boundary matching uses byte-level check (is_word_boundary_byte) to avoid substring collisions like "net_price" matching in "net_price_total"
- Refactored validate_facts into 4 helper functions to satisfy clippy::too_many_lines

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added hierarchies field to all SemanticViewDefinition struct literals**
- **Found during:** Task 1
- **Issue:** Adding the hierarchies field to SemanticViewDefinition broke compilation in 23 locations across expand.rs, graph.rs, model.rs, and expand_proptest.rs where struct literals listed all fields explicitly
- **Fix:** Added `hierarchies: vec![]` to all affected struct literals
- **Files modified:** src/expand.rs, src/graph.rs, src/model.rs, tests/expand_proptest.rs
- **Verification:** All existing tests pass unchanged
- **Committed in:** f6559a8 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed unused variable warning and clippy violations**
- **Found during:** Task 2
- **Issue:** Clippy pedantic flagged: unused variable, missing #[must_use], too_many_lines, type_complexity, redundant closure, while_let_loop
- **Fix:** Added #[must_use], extracted helper functions (check_fact_source_tables, build_fact_dag, check_fact_references_exist, check_fact_cycles), added FactDag type alias, replaced loop with while-let, replaced closure with method reference
- **Files modified:** src/graph.rs
- **Verification:** cargo clippy -- -D warnings passes cleanly
- **Committed in:** b82df96 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes necessary for compilation and CI quality gate. No scope creep.

## Issues Encountered
- Pre-commit hook runs rustfmt + clippy -- first commit attempt failed due to formatting differences. Resolved by running `cargo fmt` before commit.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- FACTS clause parsing and validation complete, ready for Plan 02 (derived metrics, fact-to-metric inlining)
- Hierarchy validation complete, ready for any hierarchy-aware expansion in future phases
- Word-boundary matching pattern (find_fact_references) available for reuse in derived metric expression substitution (Phase 30)

---
*Phase: 29-facts-clause-hierarchies*
*Completed: 2026-03-14*
