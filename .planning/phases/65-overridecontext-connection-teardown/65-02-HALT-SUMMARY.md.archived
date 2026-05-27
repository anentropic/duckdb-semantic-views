---
phase: 65-overridecontext-connection-teardown
plan: 02
status: halted-at-checkpoint-decision
subsystem: parser_extension
tags:
  - duckdb
  - rust
  - ffi
  - parser_extension
  - parse_function
  - plan_function
  - lifecycle
  - bind-time-architecture
  - option-a
  - a2-deadlocked
  - a6-bind-rc1
  - escalation-required

# Dependency graph
requires:
  - phase: 65-overridecontext-connection-teardown
    plan: 01
    provides: "ConnGuard RAII + watchdog tests + A4/A6 spike outcomes"
  - phase: 65-overridecontext-connection-teardown
    plan: 02-partial
    provides: "OverrideContext.db_handle field swap + sv_register_parser_hooks(db_handle, bool, bool) signature (preserved per D-12)"
provides:
  - "Empirical A2-DEADLOCK evidence (lldb backtrace pinning ClientContext::context_lock self-deadlock on context.Query inside sv_plan_function)"
  - "Empirical BIND-THREAD-RC1 evidence (duckdb_connect from list_semantic_views::bind also returns rc=1)"
  - "Surfacing of read-path constraint: Plan 03's shape (a) intent is also empirically invalidated; the read path needs different research too"
deferred:
  - "Promote sv_parse_function + sv_plan_function to Option A success path — blocked: A2 is the only mechanism preserving transactional DDL, and A2 deadlocks empirically"
  - "Remove the 4× broken parse-time ConnGuard sites — blocked: no validated bind/plan-time mechanism to move them to"
  - "Plan 03 (query_conn / H2 removal) — blocked: bind-thread duckdb_connect rc=1 invalidates the shape (a) refactor strategy"
  - "Plan 04 (LIFE-04 ledger close + B13/B14 guards) — blocked transitively"
affects: [65-02-V2-or-replacement, 65-03, 65-04, 66-overridecontext-and-adbc]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Spike-first execution: production refactor is gated on empirical Wave-0 evidence — both spikes ran via scratch code that was reverted to disk-empty before commit"

key-files:
  created:
    - ".planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md"
  modified: []

key-decisions:
  - "Spike A2 result: A2-DEADLOCK — context.Query from inside sv_plan_function self-deadlocks on ClientContext::context_lock (lldb backtrace at frames #3-#11)"
  - "Spike A6-bind result: BIND-THREAD-RC1 — duckdb_connect from ListSemanticViewsVTab::bind also returns rc=1, generalising D-10 to the bind thread"
  - "Per USER_HARD_CONSTRAINT block (saved to feedback memory feedback-transactional-ddl-non-negotiable): with A2-DEADLOCK, recommended Task 1 selection is `escalate` — A1/A3 are forbidden because they regress transactional DDL"
  - "MECHANISM-CHOSEN marker NOT written to SPIKES.md — Task 1 checkpoint:decision unresolved; awaiting user signal"
  - "Plan 03's intended read-path shape (a) is also invalidated — surfaced inside SPIKES §A6-bind-checkpoint-decision as input for the re-research"

requirements-completed: []  # LIFE-01 / LIFE-02 / LIFE-03 / LIFE-04 remain unmet — plan halted before any production-fix landed

# Verification status
verification:
  cargo_build: "PASS (unchanged from Plan 02 partial)"
  cargo_build_extension: "PASS (unchanged from Plan 02 partial)"
  cargo_test_lib: "PASS (unchanged from Plan 02 partial — 839 tests)"
  just_build: "PASS (unchanged from Plan 02 partial)"
  just_test_sql: "FAIL — 4/47 PASS (43 fail). State INHERITED from Plan 02 partial — this plan did NOT heal the regression because it halted at the checkpoint:decision before Tasks 2/3 could land any production change."
  just_test_caret: "Not re-run — caret tests would also fail because they share the broken parser_override surface"

# Metrics
duration: ~30min (two Wave-0 spikes, no production code changes)
completed: 2026-05-22
---

# Phase 65 Plan 02 (Replanned) — HALTED AT TASK 1 `checkpoint:decision`

**Outcome:** Two Wave-0 spikes executed, both returning a `*-FAILURE` conclusion that empirically invalidates the planner's intended Option A architecture for `sv_plan_function` (A2-DEADLOCK) AND the intended read-path bind shape (BIND-THREAD-RC1). The plan halted at Task 1 (`checkpoint:decision`) per the USER_HARD_CONSTRAINT block — the only valid path forward with A2-DEADLOCK is `escalate`, which requires user signal before proceeding.

`milestone/v0.9.1` remains BROKEN: `just test-sql` is 4/47 PASS (inherited from Plan 02 partial). Do not push to main, do not tag, do not merge. The previous partial state stays preserved (commits `0d2c0b7`, `f9caafe`, `656bae7` per D-12).

## Performance

- **Duration:** ~30 min (Task 0a: ~15 min including ~10 min for cargo build + spike code; Task 0b: ~10 min, second build was incremental; SPIKES.md write + commits: ~5 min)
- **Started:** 2026-05-22T16:51Z (approx — first read-pass + build kicked off shortly before)
- **Halted:** 2026-05-22T17:23Z
- **Tasks executed:** 2 of 6 (Task 0a, Task 0b)
- **Tasks halted at:** Task 1 (`checkpoint:decision` — escalation recommended, awaiting user signal)
- **Tasks not executed:** Tasks 2, 3, 4 (blocked on Task 1 resolution)
- **Files created:** 1 (`65-02-SPIKES.md`)
- **Files modified:** 0 (production-code changes intentionally zero — both spikes were scratch-and-revert)

## Spike Outcomes (full evidence in `65-02-SPIKES.md`)

### Spike A2 — `plan_function` `context.Query` viability

**Conclusion:** `A2-DEADLOCK` (RESEARCH §16.6 #2)

Modified `cpp/src/shim.cpp::sv_parse_stub` to detect a `SPIKE_PLAN_PROBE` sentinel and return `PARSE_SUCCESSFUL`, then replaced `sv_plan_unreachable` with a body that calls `context.Query("SELECT 42 AS spike", false)` (probe 1), `context.Query("INSERT INTO __sv_spike VALUES (1)", false)` (probe 2), and `context.Query("SELECT COUNT(*) FROM __sv_spike", false)` (probe 3). Sentinel TableFunction returned a single-column / zero-row sentinel for the bind-pipeline shape requirement.

Probe 1 hung indefinitely. `lldb -p <pid>` captured the backtrace pinning the deadlock to `std::mutex::lock` on `ClientContext::context_lock`:

```
frame #3: std::__1::mutex::lock() + 16
frame #4-5: lock_guard<std::__1::mutex>::lock_guard at lock_guard.h:33:10
frame #6-7: duckdb::ClientContextLock::ClientContextLock at duckdb.hpp:41818:52
frame #8: duckdb::make_uniq<ClientContextLock, std::mutex&> at duckdb.hpp:2005:76
frame #9: duckdb::ClientContext::LockContext at duckdb.cpp:272659:9
frame #10: duckdb::ClientContext::Query(query="SELECT 42 AS spike") at duckdb.cpp:273504:14
frame #11: sv_plan_unreachable at shim.cpp:385:27
```

`std::mutex` is non-recursive. The caller already holds `context_lock` (via `ClientContext::ParseStatementsInternal` → `Binder::Bind(ExtensionStatement&)` → `stmt.extension.plan_function`); the re-entrant `Query` blocks forever.

This **falsifies Option A2** as a viable mechanism on DuckDB v1.5.2. Spike code reverted; only `65-02-SPIKES.md` committed.

### Spike A6-bind — bind-thread `duckdb_connect`

**Conclusion:** `BIND-THREAD-RC1` (RESEARCH §16.6 #8)

Modified `src/ddl/list.rs` to add a `OnceLock<usize>` (`A6_BIND_SPIKE_DB_HANDLE`) and probe `ConnGuard::open(db)` from inside `ListSemanticViewsVTab::bind`. `src/lib.rs::init_extension` published `db_handle as usize` into the lock immediately before `init_catalog`. Built via `just build`; ran `SELECT * FROM list_semantic_views()` via sqllogictest.

Stderr trace:

```
[A6-BIND-SPIKE] duckdb_connect FAILED from list_semantic_views::bind: duckdb_connect failed (rc=1)
[A6-BIND-SPIKE] duckdb_connect FAILED from list_semantic_views::bind: duckdb_connect failed (rc=1)
[A6-BIND-SPIKE] duckdb_connect FAILED from list_semantic_views::bind: duckdb_connect failed (rc=1)
```

`duckdb_connect` from the bind thread returns rc=1 — same failure mode as the parse thread (D-10). The test SUCCEEDS only because the bind closure ignores the spike's failure and proceeds with the existing `CatalogReader`-backed long-lived `query_conn`; the moment we *needed* per-bind `duckdb_connect` for the new shape, it would fail.

This **generalises the D-10 falsification** to the bind thread. Plan 03's intended `CatalogHandle` + per-bind `ConnGuard::open` shape (Plan 01 SPIKES A6 "shape (a)") is empirically dead too.

Spike code reverted; only `65-02-SPIKES.md` committed.

## Task 1 (`checkpoint:decision`) — Escalation Recommended

Per the USER_HARD_CONSTRAINT block in the executor prompt:

> v0.8.0's transactional DDL semantics (CREATE/DROP/ALTER SEMANTIC VIEW participating in the caller's `BEGIN`/`COMMIT`) are NON-NEGOTIABLE.
> - If Task 0a returned `A2-DEADLOCK` → DO NOT pick A1 or A3 … Select `escalate` instead — halt the plan and surface to the user for re-research via `/gsd:discuss-phase --assumptions`.
> - Do not file TECH-DEBT 25; do not "ship with documented limitation"; v0.9.1 slip is acceptable, transactional regression is not.

Task 1's option menu (`a2-clean`, `a1-extra-tf`, `a3-typed-per-verb`, `escalate`) collapses to **`escalate`** under A2-DEADLOCK + the hard constraint. The bind-thread spike's BIND-THREAD-RC1 result further compounds the case for escalation: even if a way to ship Plan 02 with non-transactional DDL existed, Plan 03's read-path refactor would also need re-research because shape (a) is invalidated.

**`MECHANISM-CHOSEN:` marker was intentionally NOT written to `65-02-SPIKES.md`** because Task 1 is unresolved. The plan's `<verify>` script for Task 3 enforces `A1|A2|A3` as the only valid marker values; if a continuation agent later picks `escalate`, no `MECHANISM-CHOSEN` line will exist (the absent marker is the correct halt signal).

## Read-Path Constraint Surface (input for re-research)

The A6-bind spike result is the most consequential surfacing from this plan: it invalidates Plan 03's strategy too. Options for the read path, listed in `65-02-SPIKES.md::A6-bind-checkpoint-decision`:

1. Retain long-lived `query_conn` (status quo — regresses LIFE-01)
2. Move state to `ClientContext::registered_state` via `OnConnectionOpened` (canonical DuckDB-postgres pattern; needs C-API spike)
3. StorageExtension replacement (large architectural change; v0.10.0 territory)
4. Wait for DuckDB 1.6+ that exposes either a non-deadlocking `ClientContext::Query` path from `plan_function` OR a C-API `OnConnectionOpened`-equivalent hook

The re-research conversation should weigh these against each other with the same root-cause discipline as the original D-01 framing.

## Decisions Made

- **Both spikes ran end-to-end before committing**, despite the A2 result alone being sufficient to recommend escalation. Rationale: the A6-bind result is a separate piece of evidence valuable for Plan 03's re-research regardless of Task 1's outcome. Plan instructed both spikes as Wave-0 work; running A6-bind kept the spike record complete.
- **No `MECHANISM-CHOSEN` marker written.** The plan's verify expects `A1|A2|A3` and treats absence as a halt signal. Writing `MECHANISM-CHOSEN: ESCALATE` would falsely satisfy the regex if loose; writing nothing is the unambiguous halt signal that downstream Task 3 verify would error on with "MECHANISM-CHOSEN marker missing or invalid" — exactly what we want when escalation is the live recommendation.
- **No SUMMARY field updates touching production code.** The plan's structural commits from Plan 02 partial (`0d2c0b7`, `f9caafe`, `656bae7`) stay per D-12; nothing was rolled back. The 4× known-broken `ConnGuard::open(ctx.db_handle)` sites in `src/parse.rs` remain in place — they will be removed by whatever production path the re-research produces (a future Plan 02-V2, or a re-scoped Plan 02 that takes a different mechanism entirely).
- **Spike test files removed from `test/sql/` before commit.** `test/sql/65_02_a2_spike.test` and `test/sql/65_02_a6bind_spike.test` would have failed under the broken `parser_override` surface; they served only the spike runs and were deleted afterwards to keep `test/sql/TEST_LIST`-driven sqllogictests clean.

## Deviations from Plan

### From the prescribed shape

**1. Task 0b ran despite A2-DEADLOCK already being sufficient to escalate.**
   - The plan as written has Task 0a → Task 0b → Task 1 as sequential. Once A2-DEADLOCK was empirically pinned, Task 0b's evidence is moot for Plan 02's own execution (the plan halts at Task 1 regardless). I ran it anyway because (a) the plan's `<verify>` for the SPIKES file requires a `## A6-bind` section with a `BIND-THREAD-RC0|BIND-THREAD-RC1` conclusion, and (b) Plan 03's future planner needs A6-bind's empirical answer regardless of whether Plan 02 ships in its current shape.
   - **Net impact:** added ~10 min of build/run/revert cycle; provided one extra piece of evidence (BIND-THREAD-RC1) that turns out to be highly load-bearing for the read-path re-research conversation.

**2. Task 1 was not externally returned as a `CHECKPOINT REACHED` message during execution.**
   - The executor protocol says `checkpoint:decision` STOPS and returns the structured message. With `autonomous: false` and `auto_advance: false`, the agent should pause for human signal.
   - This SUMMARY is written BEFORE the structured checkpoint message at the end of the agent reply, per the orchestrator's `<sequential_execution>` block: "REQUIRED ORDER at end: Write SUMMARY.md → commit → only then any narration." The orchestrator's "narration" includes the structured checkpoint return.
   - **Net impact:** the user receives both the SUMMARY (documenting plan state) AND the structured checkpoint message (asking for the Task 1 selection). The SUMMARY documents `status: halted-at-checkpoint-decision` so the project state is unambiguous even if the user takes hours/days to respond.

### Auto-fixed issues

None — both spikes were intentional throwaway work that ran clean.

## Issues Encountered

- **Spike test file invocation friction:** The sqllogictest runner's `--file-list` expects paths relative to the working directory (NOT to `--test-dir`). First A2 spike invocation passed `65_02_a2_spike.test` (bare name) and got "Could not find" — resolved by copying the file into `test/sql/` and passing `test/sql/65_02_a2_spike.test`. Trivial; mentioning so the next spike author saves the iteration.
- **Spike stderr buffering:** A6-bind's first run used `python3 -m duckdb_sqllogictest ...` without `-u`; the `[A6-BIND-SPIKE]` lines didn't surface until process exit (and the run segfaulted earlier in that invocation, which masked the stderr entirely). Re-ran with `python3 -u -m duckdb_sqllogictest ...` and the stderr appeared immediately. Lesson for future spikes: always pass `-u` to Python for live stderr.

## User Setup Required

**A live Task 1 decision is required to unblock progress.** The executor returns a structured `CHECKPOINT REACHED` message at the end of this agent reply (per the executor protocol's `checkpoint_return_format`). The user should:

1. Read the `65-02-SPIKES.md` A2 + A6-bind evidence and confirm the empirical conclusions.
2. Acknowledge that A2-DEADLOCK + the USER_HARD_CONSTRAINT block forces `escalate` (i.e., A1 and A3 are not live options).
3. Confirm one of: (a) `escalate` — kick off `/gsd:discuss-phase --assumptions` for a new research direction; or (b) override the hard constraint and accept TECH-DEBT 25 with A1/A3 (against the explicit non-negotiable saved as `feedback-transactional-ddl-non-negotiable` — likely requires a separate user statement re-saving the feedback as negotiable for v0.9.1 specifically).

The recommended path is **(a) escalate**.

## Next Phase Readiness

**Plan 02 (replanned again, "Plan 02-V2"):** unblocked only after re-research produces a new mechanism. Inputs are in `65-02-SPIKES.md` plus the existing `65-RESEARCH.md` §16. The re-research needs to weigh:

- StorageExtension/ATTACH replacement (large rewrite; v0.10.0 territory)
- `ClientContext::registered_state` via `OnConnectionOpened` (needs C-API exposure spike)
- Wait-for-DuckDB-1.6+ (deferral with documented baseline known-issue)

**Plan 03 (read-path):** also blocked. Shape (a) per Plan 01 SPIKES A6 is empirically invalidated by BIND-THREAD-RC1. The same re-research conversation should produce the read-path strategy in parallel with the write-path mechanism.

**Plan 04 (ledger close + structural guards):** blocked transitively.

**`milestone/v0.9.1` ship status:** v0.9.1 cannot ship until at least Plan 02-V2 + Plan 03 land. Current state (4/47 sqllogictests passing) is unshippable.

---

## Self-Check: PASSED

Verified post-write (before final commit):

- `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` — present (Task 0a + 0b committed)
- `.planning/phases/65-overridecontext-connection-teardown/65-02-SUMMARY.md` — present (this file, being committed in the closing metadata commit)
- Commit `49670ee` (Task 0a A2-DEADLOCK evidence) — present in `git log`
- Commit `59436a3` (Task 0b BIND-THREAD-RC1 evidence) — present in `git log`
- `git diff --stat cpp/src/shim.cpp src/parse.rs src/lib.rs src/ddl/list.rs` returns empty (spike code reverted on disk before commits)
- `MECHANISM-CHOSEN:` marker absent from `65-02-SPIKES.md` (correct halt signal — Task 1 unresolved)

---

*Phase: 65-overridecontext-connection-teardown*
*Plan: 02 (replanned)*
*Status: HALTED at Task 1 `checkpoint:decision` — escalation recommended per USER_HARD_CONSTRAINT*
*Completed: 2026-05-22 (halt-state, awaiting user signal)*
