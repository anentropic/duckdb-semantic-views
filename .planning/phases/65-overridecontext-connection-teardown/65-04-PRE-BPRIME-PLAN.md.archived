---
phase: 65-overridecontext-connection-teardown
plan: 04
type: execute
wave: 4
depends_on:
  - 65-03
files_modified:
  - .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md
  - src/conn_guard.rs
  - TECH-DEBT.md
  - .planning/phases/65-overridecontext-connection-teardown/65-04-SUMMARY.md
files_audited:
  - test/integration/test_readonly_load.py
  - src/parse.rs
autonomous: true
requirements:
  - LIFE-02
  - LIFE-03
  - LIFE-04
tags:
  - duckdb
  - documentation
  - verification
  - quality-gate
  - structural-guard
  - sc-3-evidence

must_haves:
  truths:
    - "D-13: Plan 01's B1..B4 + B11 watchdog tests in `test/integration/test_readonly_load.py` (planted as failing-on-baseline) are RE-RUN and confirmed PASSING — this is the LIFE-03 SC-3 evidence (the tests fail on v0.9.0 baseline AND on the post-Plan-02 intermediate state on `milestone/v0.9.1`, and they pass after Plans 02+03 land)"
    - "LIFE-04: `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` entry 'In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)' is marked RESOLVED with a forward pointer to Phase 65 (v0.9.1) and the commit SHA range"
    - "B13 structural guard: a Rust unit test in `src/conn_guard.rs` (or `tests/structural_invariants.rs`) reads `src/parse.rs`, locates the `PHASE-65-GUARD` marker comment inside the OverrideContext struct body, brace-depth-scans the struct body, and asserts it does NOT contain `conn: ffi::duckdb_connection` or `catalog: CatalogReader`. Asserts positively that `db_handle` IS present."
    - "B14 RAII idempotency: extended unit test in `src/conn_guard.rs` covers both the null-pointer Drop path (already in Plan 01) AND a manual-null-after-construct Drop path (idempotency under defensive nulling)."
    - "Structural guard for read-side: a new grep-style test that asserts `grep -E 'parser_override' cpp/src/shim.cpp` shows only the deregistration site (`ext.parser_override = nullptr`) and not a function-pointer assignment to a real callback. This is the read-side analog of B13 — confirms RESEARCH §16.6 #1 was honoured."
    - "`#[allow(dead_code)]` / `#[allow(unused_imports)]` annotations on `src/conn_guard.rs` (added in Plan 01 with a forward-pointer to Plan 04) are REMOVED — `ConnGuard::open` and `raw` are now consumed in production by Plans 02 + 03."
    - "RESEARCH §16.5 surfacing (per D-03): `TECH-DEBT 26 — replace ParserExtension with StorageExtension+ATTACH for semantic views (v1.x architectural change)` is filed in TECH-DEBT.md. This is the cross-extension survey finding (duckdb-postgres / iceberg / mysql / delta all avoid ParserExtension)."
    - "`just test-all` exits 0 on `milestone/v0.9.1` (Rust unit + proptest + sqllogictest + DuckLake CI). HARD GATE."
    - "`just ci` passes the diff gate against the phase-65 merge-base: failure summaries (clippy lints by file:line, failing test names) on milestone/v0.9.1 vs the merge-base are EMPTY or strictly subtractive — Phase 65 introduces no new lint or test failures."
    - "All four Phase 65 success criteria from ROADMAP.md are met (in-process bootstrap-then-RO returns <5s; chosen mechanism documented in RESEARCH §6 + §16; new test exists and was failing on baseline; deferred-items.md updated)."
  artifacts:
    - path: ".planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md"
      provides: "LIFE-04 resolution + forward pointer to v0.9.1 Phase 65 + commit SHA range"
      contains: "RESOLVED in v0.9.1"
    - path: "src/conn_guard.rs"
      provides: "B13 + B14 + read-side structural guard tests; #[allow(dead_code)] annotations removed"
      contains: "override_context_carries_no_long_lived_connection"
    - path: "TECH-DEBT.md"
      provides: "TECH-DEBT 26 — StorageExtension+ATTACH architecture surfacing (RESEARCH §16.5)"
      contains: "TECH-DEBT 26"
  key_links:
    - from: ".planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md"
      to: ".planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md"
      via: "forward pointer in the resolved entry"
      pattern: "v0.9.1.*Phase 65"
    - from: "src/conn_guard.rs (B13 test)"
      to: "src/parse.rs (OverrideContext struct body)"
      via: "PHASE-65-GUARD marker comment + brace-depth scan"
      pattern: "PHASE-65-GUARD"
---

<objective>
Close the phase: re-run Plan 01's failing-on-baseline B1..B4 + B11 tests and confirm they FLIP GREEN (LIFE-03 SC-3
evidence); update the LIFE-04 deferred-items ledger; add the remaining structural / RAII guards (B13 + B14 +
read-side analog) that pin the new invariants in code so future regressions are caught at `cargo test` time;
surface the RESEARCH §16.5 finding (StorageExtension alternative) as a new TECH-DEBT entry; remove the
`#[allow(dead_code)]` annotations from `src/conn_guard.rs`; and run the full quality gate (`just test-all` +
`just ci`) to confirm Phase 65 ships green.

Purpose: LIFE-03 SC-3 explicitly requires the new test to "fail on v0.9.0 baseline and pass on v0.9.1" — this plan
is where we run it on v0.9.1 to satisfy the second half of that requirement. LIFE-04 mandates the v0.9.0
deferred-items.md entry is updated in place with the resolution. B13/B14 in VALIDATION.md require structural
assertions that the new invariants (no long-lived `duckdb_connection` in `OverrideContext`, ConnGuard Drop closes
exactly once) are enforced beyond a one-time code review. Quality gate is the project's mandated pre-verification
bar per CLAUDE.md.

Output: Updated deferred-items.md; structural guards committed; CI green on `milestone/v0.9.1`; surfaced
StorageExtension TECH-DEBT for future milestone.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT.md
@.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
@.planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md
@.planning/phases/65-overridecontext-connection-teardown/65-01-SUMMARY.md
@.planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md
@.planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md
@.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md
@TECH-DEBT.md
@CLAUDE.md
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Re-run B1..B4 + B11 (LIFE-03 SC-3 evidence); add B13 + B14 + read-side structural guards in src/conn_guard.rs</name>
  <files>
    src/conn_guard.rs
  </files>
  <read_first>
    src/conn_guard.rs
    src/parse.rs
    cpp/src/shim.cpp
    test/integration/test_readonly_load.py
    .planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md
    .planning/phases/65-overridecontext-connection-teardown/65-01-SUMMARY.md
    .planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md
    .planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md
  </read_first>
  <behavior>
    - The five Plan-01 in-process tests (`test_in_process_bootstrap_then_readonly_fresh`,
      `test_in_process_bootstrap_then_readonly_existing`, `test_in_process_load_only_then_readonly`,
      `test_in_process_readonly_then_readwrite`, `test_repeated_load_close_no_busy_spin`) are re-run on
      `milestone/v0.9.1` HEAD (post-Plan-03) and ALL PASS within their 5s watchdog. This is the LIFE-03 SC-3
      evidence: the same tests that failed on the v0.9.0 baseline pass on v0.9.1.
    - A Rust unit test in `src/conn_guard.rs` (B13) asserts that `OverrideContext` (from `crate::parse`) does NOT
      carry a `duckdb_connection`-typed field. Implementation uses the grep-style approach (option iv): reads
      `src/parse.rs`, locates the `PHASE-65-GUARD` marker comment inside the struct body, brace-depth-scans the
      struct body, asserts no `conn: ffi::duckdb_connection` / `catalog: CatalogReader` strings inside the body
      AND that `db_handle` IS present.
    - A second Rust unit test (B14) exercises `ConnGuard` idempotency: the existing Plan 01 `drop_is_idempotent_when_null`
      stays; add `manual_null_then_drop_is_safe` that constructs a guard, manually sets `conn = null_mut()`, and
      drops — must not panic and must not call duckdb_disconnect.
    - A third Rust unit test (NEW for Plan 04, read-side analog of B13): `parser_override_deregistered` reads
      `cpp/src/shim.cpp` and asserts `ext.parser_override = nullptr` is present AND there is NO line of the form
      `ext.parser_override = sv_parser_override` or similar function-pointer assignment to a real callback.
    - Tests must compile under both `default` and `--features extension --no-default-features`.
    - The `#[allow(dead_code)]` and `#[allow(unused_imports)]` annotations added in Plan 01 (`src/conn_guard.rs`
      lines ~42 + ~98) are REMOVED — Plans 02 + 03 now consume `ConnGuard::open` and `raw` in production code, so
      the warnings should no longer fire.
  </behavior>
  <action>
    Step 0 — Pre-build verification: confirm we're on `milestone/v0.9.1`:
    `git branch --show-current | grep -q "^milestone/v0.9.1$"` — must be true; abort otherwise.

    Step 1 — Re-run the five Plan-01 watchdog tests via `uv run test/integration/test_readonly_load.py` (or
    however the project's PEP 723 script is invoked — see Plan 01 SUMMARY for the exact invocation). Capture log
    to `$TMPDIR/65_04_t1_readonly.log`. ALL FIVE must pass; the file must exit 0. If any fails, STOP — Plans 02
    or 03 have a regression and this plan cannot close the phase.

    Step 2 — Remove the `#[allow(dead_code)]` annotation on `impl ConnGuard` (`src/conn_guard.rs` around line 42)
    and the `#[allow(unused_imports)]` on the crate-root re-export (around line 98). Rebuild to confirm the warnings
    no longer fire: `cargo build --features extension --no-default-features` (must succeed without `dead_code` or
    `unused_imports` warnings on the conn_guard module).

    Step 3 — Append to the existing `#[cfg(all(test, feature = "extension"))] mod tests` block in
    `src/conn_guard.rs` (the block is currently at ~line 113 onwards):

    The B13 test (grep-style structural guard with brace-depth scan):
    Add a test function named `override_context_carries_no_long_lived_connection`. The function:
    1. Reads `src/parse.rs` via `std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/parse.rs"))`.
    2. Locates the marker string
       `"// PHASE-65-GUARD: do not reintroduce duckdb_connection or CatalogReader field here."` via `parse_rs.find(marker)`.
       If absent, panic with a clear message instructing maintenance to consult Phase 65 Plan 02 Task 1.
    3. Rewinds to the most recent `{` before the marker via `parse_rs[..marker_pos].rfind('{')` — that's the
       struct's opening brace (depth 1).
    4. Forward-scans bytes tracking brace depth: `{` increments, `}` decrements; when depth returns to 0, the
       struct body is `parse_rs[open_brace..end]`.
    5. Asserts the body slice does NOT contain `"conn: ffi::duckdb_connection"` or
       `"conn: libduckdb_sys::duckdb_connection"`, and does NOT contain `"catalog: crate::catalog::CatalogReader"`
       or `"catalog: CatalogReader"`. Each assertion has a clear failure message referencing Phase 65 RESEARCH §6.
    6. Asserts the body slice DOES contain `"db_handle"` (positive structural invariant).

    Doc comment above the function: explains the Phase 65 invariant (no long-lived duckdb_connection on
    OverrideContext) and points to RESEARCH §6 + Plan 02 Task 1 (which placed the PHASE-65-GUARD marker).

    The B14 idempotency test extension:
    Augment the existing `drop_is_idempotent_when_null` (already in Plan 01 at ~line 124) with a sibling test
    `manual_null_then_drop_is_safe`. The new test:
    1. Construct `let mut guard = unsafe { ConnGuardForTest::from_raw(std::ptr::null_mut()) };` (using the
       existing `ConnGuardForTest` helper from Plan 01).
    2. Manually re-null is no-op since the field is already null; the test's value is the documentation that the
       Drop impl is robust to "guard.conn manually nulled before drop" — this guards against future hand-rolled
       Drop changes.
    3. Drop the guard. Must not panic. (Test body is ~3 lines.)

    The read-side structural guard `parser_override_deregistered`:
    A new test function reads `cpp/src/shim.cpp` via `std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"),
    "/cpp/src/shim.cpp"))`. Asserts:
    1. The string `"ext.parser_override = nullptr"` appears at least once (deregistration is present).
    2. The string `"ext.parser_override = sv_parser_override"` appears ZERO times (no live function-pointer
       assignment that would re-introduce the Phase 62 callback).
    Failure message references RESEARCH §16.6 #1 and Plan 02 Task 2.

    Test gating:
    - The B13 + read-side grep tests do NOT need the extension feature — they only read source text. If the
      module's existing `#[cfg(all(test, feature = "extension"))]` gating prevents them from running under default
      features, move them to a separate `#[cfg(test)] mod tests_structural` block inside `src/conn_guard.rs` (no
      feature gate, no FFI dependency). The B14 test stays inside the existing extension-feature block since it
      uses the FFI-symbol `ConnGuardForTest` helper.

    Step 4 — Build & test:
    - `cargo test --lib override_context_carries_no_long_lived_connection drop_is_idempotent_when_null
      manual_null_then_drop_is_safe parser_override_deregistered` exits 0 with all four tests reported `ok`.
    - `cargo test --lib --features extension --no-default-features` exits 0 (full suite).
    - Demonstrate B13 actually guards: temporarily revert `OverrideContext` to carry
      `catalog: crate::catalog::CatalogReader` in a scratch branch (`git stash` after the revert) and confirm
      `cargo test override_context_carries_no_long_lived_connection` FAILS with the documented message. Do NOT
      commit the revert; `git stash pop` to restore. Record the demonstration outcome verbatim in summary
      (text-only documentation, not an automated step).

    Commit message: `test(65-04): B13 + B14 + read-side structural guards + remove conn_guard dead_code allows`.
  </action>
  <verify>
    <automated>git branch --show-current | grep -q "^milestone/v0.9.1$" && timeout 120 uv run test/integration/test_readonly_load.py 2>&1 | tee $TMPDIR/65_04_t1_readonly.log | tail -10 && cargo test --lib override_context_carries_no_long_lived_connection drop_is_idempotent_when_null manual_null_then_drop_is_safe parser_override_deregistered 2>&1 | tee $TMPDIR/65_04_t1_cargo.log | grep -E "test result: ok"</automated>
  </verify>
  <acceptance_criteria>
    - `git branch --show-current` returns `milestone/v0.9.1`.
    - `uv run test/integration/test_readonly_load.py` exits 0. All 5 in-process tests
      (`test_in_process_bootstrap_then_readonly_fresh`, `..._existing`, `test_in_process_load_only_then_readonly`,
      `test_in_process_readonly_then_readwrite`, `test_repeated_load_close_no_busy_spin`) report PASS.
    - Test `override_context_carries_no_long_lived_connection` exists in `src/conn_guard.rs` and passes. Verify
      with `grep -E "fn override_context_carries_no_long_lived_connection" src/conn_guard.rs` ≥1.
    - Test `drop_is_idempotent_when_null` still passes (Plan 01 regression — must not be broken).
    - Test `manual_null_then_drop_is_safe` exists and passes. Verify with
      `grep -E "fn manual_null_then_drop_is_safe" src/conn_guard.rs` ≥1.
    - Test `parser_override_deregistered` exists and passes. Verify with
      `grep -E "fn parser_override_deregistered" src/conn_guard.rs` ≥1.
    - `cargo test --lib` exits 0 with all four named tests reported `ok`.
    - `cargo test --lib --features extension --no-default-features` exits 0.
    - `#[allow(dead_code)]` and `#[allow(unused_imports)]` on `src/conn_guard.rs` lines ~42 and ~98 are REMOVED.
      Verify with `grep -E "allow\(dead_code\)|allow\(unused_imports\)" src/conn_guard.rs` returns 0 matches.
    - Summary records the "deliberately broken" verification: revert OverrideContext field locally → B13 grep test
      fails → restore → test passes. (Documentation evidence, not an automated step.)
  </acceptance_criteria>
  <done>
    LIFE-03 SC-3 evidence captured (B1..B4 + B11 flip green on v0.9.1). B13 structural guard + B14 idempotency
    test + read-side parser_override-deregistered guard committed. Forward regressions to OverrideContext shape OR
    to parser_override re-registration are caught by `cargo test` at the next build. ConnGuard production-consumed
    state is now reflected in the source (dead_code allows removed).
  </done>
</task>

<task type="auto">
  <name>Task 2: Update LIFE-04 deferred-items.md ledger + surface RESEARCH §16.5 as TECH-DEBT 26</name>
  <files>
    .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md
    TECH-DEBT.md
  </files>
  <read_first>
    .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md
    TECH-DEBT.md
    .planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md
    .planning/phases/65-overridecontext-connection-teardown/65-01-SUMMARY.md
    .planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md
    .planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md
    CLAUDE.md
  </read_first>
  <action>
    Step 1 — Update LIFE-04 ledger entry.

    Edit `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md`. Find the section
    titled "In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)" (the last ~30 lines of
    the file per the original analysis). Append a new subsection AT THE END of that existing section (do NOT
    delete the original analysis — the original discovery context remains valuable history). The new subsection
    MUST contain:

    - Header: `### Resolution — v0.9.1 Phase 65 (2026-05-22)`
    - Status line: `**Status:** RESOLVED in v0.9.1 milestone, Phase 65 (milestone/v0.9.1).`
    - One paragraph summarising the root cause confirmed (busy-spin in `DBInstanceCache::GetInstanceInternal`,
      not a lock; the two extension-owned long-lived `duckdb_connection`s — H1 `catalog_conn` and H2 `query_conn`
      — held `shared_ptr<DatabaseInstance>` past the user's `close()`).
    - One paragraph summarising the ACTUAL fix that shipped (Option A bind/plan-time architecture per RESEARCH
      §16: parser_override deregistered; sv_parse_function returns PARSE_SUCCESSFUL with SemanticViewParseData;
      sv_plan_function performs catalog reads on a per-call ConnGuard derived from ClientContext via
      OverrideContext.db_handle FFI accessor; QueryState carries db_handle + flag with per-query ConnGuard on
      SemanticViewBindData; CatalogReader shape (b) carries db_handle + opens per-method ConnGuard. Note: the
      original Plan 02 attempted per-call ConnGuard from inside parser_override which D-10 empirically falsified
      on DuckDB 1.5.2 with rc=1 from `duckdb_connect`; the shipped fix is the bind/plan-time reshape per D-11.).
    - Note on TECH-DEBT impact: if Plan 02 Task 1 picked A1 or A3 (transactional regression accepted),
      reference `TECH-DEBT 25` and the regression's documented forward direction. If A2 was picked, note
      "transactional DDL semantics preserved byte-identical with v0.8.0 / v0.9.0".
    - Forward pointer block listing:
      - `RESEARCH.md`: `.planning/phases/65-overridecontext-connection-teardown/65-RESEARCH.md` (§6 for original
        D-07-1 reasoning; §16 for the bind/plan-time architecture that actually shipped).
      - SUMMARY trail: `65-01-SUMMARY.md`, `65-02-SUMMARY.md`, `65-03-SUMMARY.md`, `65-04-SUMMARY.md`.
      - SPIKES trail: `65-01-SPIKES.md` (A4/A6/A7), `65-02-SPIKES.md` (A2 viability + bind-thread duckdb_connect).
      - In-process regression test: `test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_fresh`
        and the four siblings (B2/B3/B4/B11).
      - Commit SHA range: insert a placeholder `[INSERT_SHA_RANGE_AT_VERIFY_TIME]`; the executor will replace
        this with the real `git log --oneline milestone/v0.9.1 ^main` range when running this task. The
        placeholder MUST be replaced before this plan's summary closes.
    - One sentence noting `TECH-DEBT 25` (transactional regression if A1/A3 path was taken — conditional) and
      `TECH-DEBT 26` (StorageExtension+ATTACH alternative for v1.x — see Step 2 below) remain surfaced for future
      milestones and are NOT part of this resolution.

    Step 2 — Surface RESEARCH §16.5 finding as a NEW TECH-DEBT entry.

    Edit `TECH-DEBT.md` (repo root). Append a new entry per the existing format. Title:
    `TECH-DEBT 26 — Replace ParserExtension with StorageExtension+ATTACH for semantic views (v1.x architectural change)`.

    Body must include:
    - Source: RESEARCH §16.5 — the community-extension survey (duckdb-postgres, duckdb-iceberg, duckdb-mysql,
      duckdb-delta) found that NONE of them use `ParserExtension` for DDL; all use `StorageExtension::Register` +
      ATTACH. The semantic_views extension is the outlier.
    - Why deferred: A StorageExtension migration is a milestone-sized refactor that would touch every CREATE / DROP
      / ALTER code path; potentially breaks SHOW/DESCRIBE shape; and likely requires a v1.0 major-version commitment
      first to absorb the breaking changes. The v0.9.1 patch milestone scope is "fix the leak", not "rearchitect
      DDL".
    - Reference: cite RESEARCH §16.5 + §9.1 (Phase 65 surfaced this and decided it's a future-milestone item).
    - Per CONTEXT.md D-03 (bounded scope with signal surfacing): this entry is the "surface finding, do not absorb
      silently" deliverable. NOT in scope for v0.9.1.

    Step 3 — Replace the `[INSERT_SHA_RANGE_AT_VERIFY_TIME]` placeholder in `deferred-items.md` with the actual
    range. Use: `git log --oneline milestone/v0.9.1 ^main | tail -1` for the first SHA (oldest in the range) and
    `git log --oneline -1 milestone/v0.9.1` for the last SHA (HEAD). Write the range as `<first_short_sha>..<last_short_sha>`.

    Step 4 — Commit. Two atomic commits is preferred:
    1. `docs(65-04): close LIFE-04 deferred-items.md entry with Phase 65 resolution + SHA range`
    2. `docs(65-04): surface TECH-DEBT 26 (StorageExtension+ATTACH alternative) from RESEARCH §16.5`

    DO NOT run `just test-all` or `just ci` here — Task 3 owns the quality gate.
  </action>
  <verify>
    <automated>git branch --show-current | grep -q "^milestone/v0.9.1$" && grep -E "RESOLVED in v0.9.1|Phase 65" .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md && ! grep "INSERT_SHA_RANGE_AT_VERIFY_TIME" .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md && grep -E "TECH-DEBT 26" TECH-DEBT.md</automated>
  </verify>
  <acceptance_criteria>
    - `git branch --show-current` returns `milestone/v0.9.1`.
    - `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` contains the new
      `### Resolution — v0.9.1 Phase 65` subsection. Verify with
      `grep -E "Resolution — v0.9.1 Phase 65|RESOLVED in v0.9.1" .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` ≥1.
    - Forward pointer to Phase 65 RESEARCH.md (specifically §6 + §16) and SUMMARYs present. Verify with
      `grep -E "65-RESEARCH.md|65-03-SUMMARY.md|65-02-SPIKES.md" .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` ≥1.
    - Commit SHA range filled in (no placeholder remains). Verify with
      `grep "INSERT_SHA_RANGE_AT_VERIFY_TIME" .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` returns 0 matches.
    - Original analysis section preserved (do not delete the original Phase 63 discovery write-up). Verify with
      `grep -E "Discovered during Plan 02 Task 1|Without the extension load in step 1" .planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` ≥1 (these phrases from the original entry must still be present).
    - `TECH-DEBT.md` contains a new `TECH-DEBT 26` entry. Verify with `grep -E "TECH-DEBT 26" TECH-DEBT.md` ≥1.
    - The new entry references RESEARCH §16.5 + §9.1. Verify with
      `grep -E "RESEARCH §16.5|RESEARCH §9.1|StorageExtension" TECH-DEBT.md` ≥1.
    - Two commit subjects matching the prescribed messages above are present in `git log --oneline -10`.
  </acceptance_criteria>
  <done>
    LIFE-04 ledger update lands. RESEARCH §16.5 finding is surfaced as TECH-DEBT 26 per D-03. Ready for Task 3
    quality gate.
  </done>
</task>

<task type="auto">
  <name>Task 3: Full quality gate — just test-all + just ci diff gate</name>
  <files>
    .planning/phases/65-overridecontext-connection-teardown/65-04-SUMMARY.md
  </files>
  <read_first>
    CLAUDE.md
    justfile
    .planning/phases/65-overridecontext-connection-teardown/65-VALIDATION.md
    .planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md
  </read_first>
  <action>
    Step 1 — Pre-flight: confirm we're on the right branch.
    `git branch --show-current` must show `milestone/v0.9.1`. Abort otherwise.

    Step 2 — Run the full quality gate per CLAUDE.md.

    (a) `just test-all` — Rust unit + proptest + sqllogictest + DuckLake CI. Save log to
        `$TMPDIR/65_04_t3_test_all.log`. MUST exit 0. If any test fails, STOP and investigate; do not paper over.

    (b) `just ci` — lint (clippy pedantic + fmt + cargo-deny) + test-all + fuzz target compile + docs-check. Save
        log to `$TMPDIR/65_04_t3_just_ci.log`. The pass condition is the diff gate described below — NOT absolute
        exit code.

    Gate policy:
    - `just test-all` is a HARD gate. The acceptance criterion is `just test-all` exits 0. Any failure here is
      blocking and owned by Phase 65; investigate and resolve before closing the plan.
    - `just ci` is a DIFF gate (NOT a soft-pass against absolute exit code). The assertion is the diff between
      `just ci` output on `milestone/v0.9.1` (HEAD) and the phase-65 merge-base (the commit
      `git merge-base milestone/v0.9.1 main`). Steps:
        1. Resolve the merge-base SHA: `MERGE_BASE=$(git merge-base milestone/v0.9.1 main)` and record it in
           summary.
        2. **Primary path — stash + checkout merge-base, run, restore** (per project memory
           `feedback_worktree_isolation.md`: worktree isolation causes Cargo lock contention and wrong branch base):
           `git stash --include-untracked -m "phase-65-04-baseline" && git checkout $MERGE_BASE`, run `just ci` and
           capture log to `$TMPDIR/65_04_t3_ci_baseline.log`, then `git checkout milestone/v0.9.1 && git stash pop`.
           Confirm working tree is restored (`git status` matches pre-stash state) before continuing.
        3. **Fallback path — temporary git worktree (ONLY if stash fails for a specific, recorded reason** — e.g.,
           a mid-rebase state that cannot be stashed). Run `git worktree add $TMPDIR/mb $MERGE_BASE`, run `just ci`
           inside the worktree capturing to `$TMPDIR/65_04_t3_ci_baseline.log`, then `git worktree remove $TMPDIR/mb`.
           Per project memory this path is fragile (Cargo lock contention, process proliferation) and must be the
           exception, not the default.
        4. Run `just ci` on `milestone/v0.9.1` (HEAD, restored from stash per Step 2 primary path), capture log to
           `$TMPDIR/65_04_t3_ci_current.log`.
        5. Diff the two logs' failure summaries (clippy lints by file:line, failing test names). The diff MUST be
           empty OR strictly subtractive (Phase 65 only resolved pre-existing items, never introduced new ones).
        6. ANY new failure (clippy lint not present at merge-base, test failure not present at merge-base) is
           blocking and owned by Phase 65. Pre-existing failures in v0.9.0 `deferred-items.md` (clippy backlog,
           `#[cfg(not(feature = "extension"))]`-gated catalog tests) are NOT blocking but must appear in BOTH logs
           to qualify as pre-existing.

    Step 3 — Final ROADMAP / STATE update is handled by `/gsd:verify-work` and `/gsd:close-phase` downstream; this
    task only needs to deliver the green CI gates and record the SHA range, merge-base SHA, both log paths, and
    the diff result in the SUMMARY.

    Commit message (if any incidental fixes are required to make `just ci` diff-clean — e.g., a single new clippy
    lint introduced by Plan 02 or Plan 03): `chore(65-04): fix new clippy lint introduced by Phase 65`. Plans 02
    and 03 should have caught these via their per-task verify gates; this is a defensive cleanup.
  </action>
  <verify>
    <automated>git branch --show-current | grep -q "^milestone/v0.9.1$" && just test-all 2>&1 | tee $TMPDIR/65_04_t3_test_all.log | tail -10 && grep -E "test result: ok|0 failed|All tests passed" $TMPDIR/65_04_t3_test_all.log</automated>
  </verify>
  <acceptance_criteria>
    - `git branch --show-current` returns `milestone/v0.9.1`.
    - `just test-all` exits 0. Log saved at `$TMPDIR/65_04_t3_test_all.log` referenced in summary.
    - `just ci` diff gate holds: the diff between `$TMPDIR/65_04_t3_ci_current.log` and
      `$TMPDIR/65_04_t3_ci_baseline.log` (failure summaries — clippy lints by file:line, failing test names) is
      EMPTY or strictly subtractive. Any new failure (item present on `milestone/v0.9.1` and absent at the
      merge-base) is blocking.
    - Summary records:
      - The merge-base SHA (`git merge-base milestone/v0.9.1 main` output).
      - Both log paths (`$TMPDIR/65_04_t3_ci_baseline.log`, `$TMPDIR/65_04_t3_ci_current.log`).
      - The diff result (verbatim — empty diff, or list of removed-only items).
    - Working tree restored to milestone/v0.9.1 HEAD after baseline `just ci` run: `git branch --show-current`
      returns `milestone/v0.9.1` AND `git status` shows the same working-tree state as before the diff-gate run
      (the stash was popped successfully). If the fallback worktree path was used instead, `git worktree list` does
      NOT show a `$TMPDIR/mb` entry (worktree was removed via `git worktree remove $TMPDIR/mb`).
  </acceptance_criteria>
  <done>
    Phase 65 is green-gated. `just test-all` passes. `just ci` diff is empty / strictly subtractive against the
    phase-65 merge-base. Phase 65 is ready for `/gsd:verify-work`.
  </done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Phase artifact ↔ documentation | LIFE-04 ledger update is the project's record of resolution; misstating the fix or omitting the forward pointer breaks the audit trail. |
| CI quality gate ↔ Phase 65 invariants | `just ci`'s clippy/lint gates may catch unrelated regressions or pre-existing items; the merge-base diff is the attribution mechanism. |
| Worktree isolation ↔ Cargo lock | Project memory documents a known issue with worktree-based parallel builds. Task 3 acceptance criteria mitigate via "remove worktree on completion" + fallback to stash-based baseline run. |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-65-13 | Repudiation | LIFE-04 ledger update missing forward pointer | mitigate | Task 2 acceptance criteria explicitly checks for the forward pointer + SHA range + RESEARCH §16 reference; placeholder removal verified. |
| T-65-14 | Tampering | Structural guard test false-negative (grep matches a comment instead of the field) | mitigate | Task 1's B13 test locates the `PHASE-65-GUARD` marker comment and brace-depth-scans the struct body precisely; this is robust against the struct moving in the file or growing. Deliberate-revert demonstration confirms the guard fires. |
| T-65-15 | Denial of Service | `just ci` blocked by pre-existing 89-clippy backlog | mitigate | Task 3 uses a diff gate against the phase-65 merge-base (not absolute exit code); pre-existing items appear in both logs and cancel out, new items are blocking and attributed to Phase 65. |
| T-65-16 | Tampering | Worktree isolation causes Cargo lock deadlock during baseline `just ci` run | mitigate | Task 3 uses `git stash + checkout merge-base + checkout back + stash pop` as the PRIMARY path per project memory `feedback_worktree_isolation.md`; the `git worktree add` path is a documented fallback used only when stash is impossible (e.g., mid-rebase), per the same memory. |
| T-65-SC | Tampering | No new package installs in this plan | accept | Plan 04 modifies documentation + tests only; no new crates added. Cargo.toml unchanged. No legitimacy gate required. |
</threat_model>

<verification>
After all tasks in this plan complete:

1. `uv run test/integration/test_readonly_load.py` exits 0 — all 5 B1..B4 + B11 in-process tests PASS on
   `milestone/v0.9.1` HEAD. LIFE-03 SC-3 evidence captured.
2. `cargo test --lib override_context_carries_no_long_lived_connection drop_is_idempotent_when_null
   manual_null_then_drop_is_safe parser_override_deregistered` exits 0 with all four tests passing.
3. `grep -E "allow\(dead_code\)|allow\(unused_imports\)" src/conn_guard.rs` returns 0 matches (Plan 01
   forward-pointer annotations removed).
4. `git branch --show-current` returns `milestone/v0.9.1`.
5. `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` contains the v0.9.1
   Phase 65 resolution subsection with forward pointers + concrete SHA range.
6. `TECH-DEBT.md` contains `TECH-DEBT 26` (StorageExtension+ATTACH alternative surfaced per D-03 + RESEARCH §16.5).
7. `just test-all` exits 0.
8. `just ci` diff gate passes — output on `milestone/v0.9.1` vs the merge-base shows empty or strictly subtractive
   failure-summary diff.
9. All four Phase 65 ROADMAP success criteria are satisfiable (cross-check):
   - In-process bootstrap-then-RO returns within 5s (verified by B1-B4 tests passing in Task 1).
   - Chosen mechanism documented in RESEARCH §6 + §16 (both the original D-07-1 and the shipped bind/plan-time
     Option A).
   - `test_in_process_bootstrap_then_readonly` exists (Plan 01) and was failing on baseline (Plan 01 SUMMARY) /
     passes now (Task 1 evidence).
   - `deferred-items.md` updated with resolution + forward pointer (Task 2).
</verification>

<success_criteria>
- LIFE-03 SC-3 evidence: B1..B4 + B11 tests flip from baseline-fail to GREEN on `milestone/v0.9.1` HEAD.
- LIFE-04 ledger entry resolved with SHA range + forward pointers (+ RESEARCH §6/§16 dual reference).
- Structural guards (B13 + read-side parser_override-deregistered + B14 idempotency) committed and passing.
- ConnGuard `#[allow(dead_code)]` annotations removed.
- TECH-DEBT 26 surfaced per D-03 (RESEARCH §16.5 StorageExtension+ATTACH alternative).
- `just test-all` exits 0 (HARD gate).
- `just ci` passes the diff gate against the phase-65 merge-base (no new failures).
- Phase 65 is ready for `/gsd:verify-work`.
</success_criteria>

<output>
Create `.planning/phases/65-overridecontext-connection-teardown/65-04-SUMMARY.md` when done. Summary MUST include:
- The exact `git log --oneline milestone/v0.9.1 ^main` range inserted into deferred-items.md.
- Path to the B1..B4 + B11 re-run log (`$TMPDIR/65_04_t1_readonly.log`) + a one-line summary of each test's
  PASS/FAIL outcome (must be all PASS).
- Paths to test-all + ci logs.
- Merge-base SHA + both `just ci` log paths + the diff result (verbatim — empty diff, or list of subtracted items).
- Outcome of the deliberate-revert demonstration that B13 fires (text record only).
- Cross-check of all four ROADMAP success criteria with a single-line status each.
- Confirmation that no Phase 66 scope (ADBC qualification, CHANGELOG, version bump) was touched.
- TECH-DEBT 26 entry verbatim + forward-pointer.
- Note on TECH-DEBT 25 status (filed in Plan 02 if A1/A3 path was taken; not applicable if A2 path).
</output>
