---
phase: 45-alter-comment-get-ddl
plan: 01
subsystem: ddl
tags: [alter, comment, parser, vtab, catalog]

# Dependency graph
requires:
  - phase: 43-metadata-model
    provides: SemanticViewDefinition.comment field, COMMENT annotation in CREATE DDL
  - phase: 44-show-describe-metadata
    provides: SHOW/DESCRIBE output includes comment column/property
provides:
  - ALTER SEMANTIC VIEW SET COMMENT DDL command
  - ALTER SEMANTIC VIEW UNSET COMMENT DDL command
  - IF EXISTS variants for both SET and UNSET COMMENT
  - Generalized DdlKind::Alter/AlterIfExists variants with sub-operation dispatch
affects: [45-02-get-ddl, future ALTER operations]

# Tech tracking
tech-stack:
  added: []
  patterns: [alter sub-operation dispatch via rewrite_alter helper, alter_comment_impl shared VTab logic]

key-files:
  created:
    - test/sql/phase45_alter_comment.test
  modified:
    - src/parse.rs
    - src/ddl/alter.rs
    - src/lib.rs
    - test/sql/TEST_LIST
    - test/sql/phase34_1_alter_rename.test
    - tests/parse_proptest.rs

key-decisions:
  - "Generalize DdlKind::AlterRename to DdlKind::Alter with sub-operation dispatch in rewrite_alter helper"
  - "Share AlterCommentState between SET and UNSET VTabs, with alter_comment_impl shared logic"

patterns-established:
  - "ALTER sub-operation dispatch: rewrite_alter and validate_alter helpers route by keyword prefix"
  - "Shared VTab impl pattern: alter_comment_impl handles catalog read-modify-write for both SET and UNSET"

requirements-completed: [ALT-01, ALT-02]

# Metrics
duration: 64min
completed: 2026-04-11
---

# Phase 45 Plan 01: ALTER SET/UNSET COMMENT Summary

**ALTER SEMANTIC VIEW SET/UNSET COMMENT with generalized DdlKind sub-operation dispatch, catalog mutation, persistence, and 10 sqllogictest integration cases**

## Performance

- **Duration:** 64 min
- **Started:** 2026-04-11T15:18:26Z
- **Completed:** 2026-04-11T16:22:16Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Generalized DdlKind::AlterRename/AlterRenameIfExists to DdlKind::Alter/AlterIfExists with sub-operation dispatch for RENAME TO, SET COMMENT, and UNSET COMMENT
- Implemented AlterSetCommentVTab and AlterUnsetCommentVTab with catalog read-modify-write and persistence
- Registered 4 new table functions in extension init (set_comment, unset_comment, each with if_exists variant)
- 10 sqllogictest integration cases verifying SET, UNSET, overwrite, IF EXISTS no-op, escaping, errors, and query integrity

## Task Commits

Each task was committed atomically:

1. **Task 1: Generalize ALTER DdlKind and add SET/UNSET COMMENT parser support** - `2702639` (feat)
2. **Task 2: AlterComment VTab implementation, registration, and integration tests** - `c2161b4` (feat)

## Files Created/Modified
- `src/parse.rs` - Renamed DdlKind variants, added rewrite_alter/validate_alter helpers for sub-operation dispatch
- `src/ddl/alter.rs` - AlterCommentState, AlterSetCommentVTab, AlterUnsetCommentVTab with shared alter_comment_impl
- `src/lib.rs` - Registration of 4 new table functions for SET/UNSET COMMENT with IF EXISTS variants
- `tests/parse_proptest.rs` - Updated for renamed DdlKind variants
- `test/sql/phase45_alter_comment.test` - 10 integration test cases for ALTER SET/UNSET COMMENT
- `test/sql/TEST_LIST` - Added phase45_alter_comment.test
- `test/sql/phase34_1_alter_rename.test` - Updated error message to match generalized ALTER dispatch

## Decisions Made
- Generalized DdlKind::AlterRename to DdlKind::Alter to support multiple ALTER sub-operations without adding new variants per operation
- Extracted rewrite_alter and validate_alter helper functions to keep rewrite_ddl and validate_and_rewrite under clippy's 100-line limit
- Shared alter_comment_impl between SET and UNSET VTabs to avoid duplicating catalog read-modify-write logic

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Updated phase34_1_alter_rename.test error message**
- **Found during:** Task 2 (integration test run)
- **Issue:** Existing test expected old error message "Expected RENAME TO" but generalized ALTER dispatch now returns "Missing ALTER operation after view name. Supported: RENAME TO, SET COMMENT, UNSET COMMENT."
- **Fix:** Updated expected error pattern in test from "Expected RENAME TO" to "Missing ALTER operation"
- **Files modified:** test/sql/phase34_1_alter_rename.test
- **Verification:** Full sqllogictest suite passes (25/25)
- **Committed in:** c2161b4 (Task 2 commit)

**2. [Rule 1 - Bug] Extracted helper functions for clippy compliance**
- **Found during:** Task 1 (commit pre-commit hook)
- **Issue:** Adding ALTER sub-operation dispatch pushed rewrite_ddl (102 lines) and validate_and_rewrite (104 lines) over clippy's 100-line limit; also had match_same_arms warning
- **Fix:** Extracted rewrite_alter() and validate_alter() helper functions; merged DdlKind::Alter | AlterIfExists match arms in function_name()
- **Files modified:** src/parse.rs
- **Verification:** clippy passes, all cargo tests pass
- **Committed in:** 2702639 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Both auto-fixes necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- ALTER SET/UNSET COMMENT complete, ready for Plan 02 (GET_DDL)
- DdlKind::Alter generalization pattern established for future ALTER operations

---
*Phase: 45-alter-comment-get-ddl*
*Completed: 2026-04-11*
