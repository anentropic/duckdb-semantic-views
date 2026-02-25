# Deferred Items - Phase 04

## Pre-existing Issues Discovered

### phase2_ddl.test restart section hangs on re-run

**Discovered during:** Plan 04-03, Task 1
**Severity:** Low (does not affect CI on first run)
**Description:** The SQLLogicTest file `test/sql/phase2_ddl.test` section 10 (DDL-05 persistence test) creates a file-backed database with a sidecar file. On subsequent runs, the sidecar file persists from the previous test and causes a "semantic view already exists" error or a deadlock/hang during the restart step.
**Root cause:** The sidecar file `test/sql/restart_test.db.semantic_views` is not cleaned up between test runs. The SQLLogicTest `__TEST_DIR__` variable may resolve to the same path across runs.
**Workaround:** Delete `test/sql/restart_test.db*` before running tests.
**Suggested fix:** Add cleanup at the START of section 10 (before the `load` directive) or ensure the test runner uses a unique temp directory.
