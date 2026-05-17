# Phase 63 — Deferred Items

Pre-existing issues encountered during Phase 63 execution that are out of
scope per the GSD scope-boundary rule (only auto-fix issues directly
caused by the current task's changes). Logged here for future cleanup.

## Pre-existing clippy errors (89 total)

Running `cargo clippy --lib --all-targets -- -D warnings` on the
`milestone/v0.9.0` branch surfaces ~89 pre-existing clippy errors,
none of them introduced by Phase 63. Examples:

- `src/parse.rs:2959` / `:2988` — `borrow_as_ptr` on `&mut position as *mut u32`
- `src/parse.rs:4764-4768` — `uninlined_format_args` in test assertion
- `src/catalog.rs:379` — `.err().expect(...)` on a `Result` (in
  pre-existing `two_statement_guard_then_dml_smoke` test)
- `src/catalog.rs:453` / `:476` — `redundant_closure` for
  `Result::ok` (in pre-existing `pragma_database_list_*` tests)

These existed before Phase 63 work began; verified by stashing all
Phase 63 changes and re-running clippy. The Plan 63-01 verification
step #4 (`cargo clippy --all-targets -- -D warnings`) was aspirational
relative to the codebase baseline. CLAUDE.md's `just ci` chain
includes clippy and would also fail; this is a milestone-level
cleanup task rather than a Phase 63 obligation.

**Recommendation:** Open a separate quick-task to either fix the
pre-existing clippy backlog or relax the clippy gate to project
needs. Not a blocker for Phase 63.

## Pre-existing test breakage under `--features extension --no-default-features`

Several `src/catalog.rs::tests::*` tests use `Connection::open_in_memory`
(or `Connection::open(path)`) which require the `bundled` DuckDB API.
Under the `extension` feature DuckDB-rs swaps in `loadable-extension`
stubs that panic at runtime ("DuckDB API not initialized or DuckDB
feature omitted"). Affected pre-existing tests:

- `catalog::tests::two_statement_guard_then_dml_smoke`
- `catalog::tests::init_catalog_creates_schema_and_table`
- `catalog::tests::pragma_database_list_returns_file_path`
- `catalog::tests::pragma_database_list_returns_none_for_in_memory`
- `catalog::tests::persist_02_rollback_leaves_catalog_unchanged`

These never ran under the extension feature on baseline either. Phase
63 added `#[cfg(not(feature = "extension"))]` gates on all of them
(plus the new Phase 63 `init_catalog_*` and `access_mode_*` tests
which face the same constraint) so that
`cargo test --lib --features extension --no-default-features` now
exits 0 with 764 passing tests. This was the minimum fix needed to
satisfy Plan 63-01 Task 3's verification command.

## Pre-existing test compile error in `src/ddl/describe.rs`

The `window_spec_property_row_emitted` test referenced a
`SemanticViewDefinition::base_table` field removed in commit `cbacbed`
("remove vestigial filters field from SemanticViewDefinition"). This
made the `--features extension --no-default-features` build fail
entirely. Phase 63 fixed this inline (one-line removal) as a Rule 3
blocker.

## In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)

Discovered during Plan 02 Task 1 (Python integration test). Sequence
that reproduces:

1. Open the DB writable; FORCE INSTALL + LOAD `semantic_views`.
2. CREATE TABLE / CREATE SEMANTIC VIEW.
3. Close the writable connection (`conn.close()`); drop all Python
   refs and `gc.collect()`.
4. In the SAME Python process, call
   `duckdb.connect(db, read_only=True)` against the same path.

Step 4 hangs indefinitely (verified ≥5s with a watchdog thread).
Without the extension load in step 1, step 4 returns immediately.

Root cause hypothesis: Phase 62 attaches `OverrideContext` (which holds
a `duckdb_connection` opened via `duckdb_connect` in `init_extension`)
to `SemanticViewsParserInfo` on `DBConfig`. RESEARCH §Q2 documented
this as an intentional bounded leak. The catalog connection keeps the
underlying `Database` alive past the user's connection close. DuckDB's
in-process `DatabaseManager` then refuses to reopen the same path in a
different access mode while a writable handle is still referenced.

Workaround used in `test/integration/test_readonly_load.py`: bootstrap
in a `subprocess.run(...)` so the OS reclaims the DBConfig at child
exit, releasing the file lock so the parent can reopen RO cleanly. The
helper `bootstrap_in_subprocess()` documents this rationale inline.

Real-world impact: low. Production deployments separate bootstrap
(CI/build pipeline) from read-only query (analytics worker) across
process boundaries; the hang only manifests in scripts that try to do
both in one process.

Recommendation: future phase to either (a) drop the `OverrideContext`
catalog connection deterministically at extension teardown via a DuckDB
extension-unload hook, or (b) detect access-mode mismatch on reopen
inside `init_extension` and return a clear error instead of hanging.
Not a Phase 63 obligation per the GSD scope-boundary rule (the bug
predates Phase 63 — Phase 63 only made it observable by adding the
RO-reopen scenario to the test suite).
