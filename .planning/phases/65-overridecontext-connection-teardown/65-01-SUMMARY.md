---
phase: 65-overridecontext-connection-teardown
plan: 01
subsystem: infra
tags: [duckdb, rust, ffi, lifecycle, raii, testing, python]

# Dependency graph
requires:
  - phase: 62-caret-restoration-lru-removal
    provides: "OverrideContext attached to SemanticViewsParserInfo (the leak we are about to dismantle)"
  - phase: 63-readonly-database-load-support
    provides: "test_readonly_load.py bootstrap_in_subprocess pattern, deferred-items entry that Plan 04 will close"
provides:
  - "ConnGuard RAII type (src/conn_guard.rs) that Plans 02/03 will consume"
  - "_connect_with_watchdog helper + 5 in-process tests (B1..B4 + B11) that fail on baseline and will pass after Plans 02/03"
  - "Wave-0 spike evidence file 65-01-SPIKES.md confirming A4 busy-spin, A6 BindInfo surface, A7 deferral rationale"
affects: [65-02, 65-03, 65-04, 66-overridecontext-and-adbc]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "ConnGuard mirrors the PreparedStmt / QueryResult RAII pattern (src/catalog.rs:176-230) — same shape, different C handle"
    - "Daemon-thread watchdog with TimeoutError exit for uninterruptible C++ busy-spin paths (analog of test_concurrent_ddl.py worker pattern)"

key-files:
  created:
    - ".planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md"
    - "src/conn_guard.rs"
  modified:
    - "src/lib.rs"
    - "test/integration/test_readonly_load.py"

key-decisions:
  - "Spike A4 CONFIRMED — main thread at 99.4% CPU in tight ldr/cmn/b.ne loop; planner's DBInstanceCache busy-spin diagnosis stands"
  - "Spike A6 BindInfo does NOT expose duckdb_database in duckdb-rs 1.10502.0 → Plan 03 must adopt shape (a) CatalogHandle { db, catalog_table_present } threaded via extra_info"
  - "Spike A7 DEFERRED-TO-PLAN-02 (acceptable per plan's stated guidance) — empirical falsification will fire automatically when Plan 02 first runs a parser_override sqllogictest under the per-call shape"
  - "conn_guard module declared without #[cfg(feature = \"extension\")] gate so the null-drop test runs under default `bundled` features; the inner FFI body remains gated"
  - "ConnGuard is Send but deliberately NOT Sync (per-scope ownership); not Clone / Copy"
  - "Defensive `self.conn = null_mut()` in Drop after duckdb_disconnect — redundant today (cpp/include/duckdb.cpp:266477 already zeroes) but survives any libduckdb-sys signature change"

patterns-established:
  - "ConnGuard RAII pattern: pub(crate) struct + unsafe fn open(db) -> Result<Self, String> + fn raw(&self) + impl Drop with null-check guard; mirrors PreparedStmt"
  - "Watchdog test pattern: daemon thread + join(timeout) + TimeoutError on is_alive(); documents thread-leak caveat; new tests registered LAST in run_test aggregator so leaks don't contaminate earlier tests"

requirements-completed: []  # LIFE-01 / LIFE-03 are not satisfied until Plans 02/03 + Plan 04 verification flip the new tests green; Plan 01 only delivers scaffolding + baseline-fail evidence.

# Metrics
duration: 31min
completed: 2026-05-21
---

# Phase 65 Plan 01: Wave-0 Scaffolding & Spike Evidence Summary

**Scaffolding ships: ConnGuard RAII type (null-drop unit test + proptest), 5 in-process watchdog tests (B1..B4 + B11) that fail-fast on the v0.9.0 baseline busy-spin, and verbatim lldb evidence pinning the DBInstanceCache::GetInstanceInternal diagnosis.**

## Performance

- **Duration:** ~31 min
- **Started:** 2026-05-21T16:53Z (approx — first build kicked off after read-pass)
- **Completed:** 2026-05-21T17:24Z
- **Tasks:** 3 of 3
- **Files created:** 2 (`.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md`, `src/conn_guard.rs`)
- **Files modified:** 2 (`src/lib.rs`, `test/integration/test_readonly_load.py`)

## Accomplishments

- Captured verbatim lldb backtrace of the in-process RW→RO hang and confirmed the planner's `DBInstanceCache::GetInstanceInternal` busy-spin diagnosis at the ARM64 assembly level (`ldr/cmn/b.ne` tight loop on a `weak_ptr::expired()` sentinel; 99.4 % CPU on a single thread; 14 other threads asleep on `semaphore_wait_trap`).
- Verified by direct source-read of `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/duckdb-1.10502.0/src/vtab/function.rs` that `BindInfo` / `InitInfo` / `TableFunctionInfo` expose ZERO `duckdb_database` accessor — Plan 03 must adopt shape (a) (`CatalogHandle { db, catalog_table_present }` via `extra_info`).
- Documented Spike A7's deferral to Plan 02 with explicit rationale: the probe requires `db_handle` plumbing through `OverrideContext` which IS Plan 02's deliverable, and Plan 02's first parser_override sqllogictest run will deadlock if A7 is wrong — a strictly better falsification signal than a contrived spike on the current baseline.
- Introduced `src/conn_guard.rs` mirroring the `PreparedStmt` / `QueryResult` RAII shape from `src/catalog.rs:176-230`. The guard's Drop calls `duckdb_disconnect` exactly once on a non-null handle and re-nulls the field defensively.
- Added five in-process tests + `_connect_with_watchdog` helper to `test/integration/test_readonly_load.py` that on the v0.9.0 baseline reproduce the bug (5 × `TimeoutError` after ~5 s wall-clock each, with daemon-thread leaks documented in the helper's docstring). On v0.9.1 (after Plans 02/03 land) all five should flip green and Plan 04 will re-run them.

## Task Commits

Each task was committed atomically on `milestone/v0.9.1`:

1. **Task 1: Wave-0 spikes (A4, A6, A7)** — `fac190b` (docs)
2. **Task 2: ConnGuard RAII module + crate wiring** — `e38ae7a` (feat)
3. **Task 3: `_connect_with_watchdog` + B1..B4 + B11 tests** — `53496bc` (test)

**Plan metadata commit:** (this commit) — `docs(65-01): complete Plan 01 scaffolding`

Task 2 is `tdd="true"` in the plan; in practice the test (`drop_is_idempotent_when_null` + the `conn_guard_drop_handles_arbitrary_pointer_state` proptest) and the implementation live in the same file (the proptest exercises the type's actual layout, so it has to be co-located). They were committed together in a single `feat` commit because splitting RED/GREEN across the same file would have produced an intermediate state where the test references the not-yet-existing struct.

## TDD Gate Compliance

The plan-level RED → GREEN sequencing is not externally observable from `git log` because Task 2's test and implementation are co-located in `src/conn_guard.rs` (see paragraph above). The validation that matters here:

- `cargo test --lib conn_guard` (default `bundled` feature) — 1 test passing (`module_compiles_without_extension_feature`).
- `cargo test --lib --features extension --no-default-features conn_guard` — 2 tests passing (`drop_is_idempotent_when_null`, `conn_guard_drop_handles_arbitrary_pointer_state` proptest).

Both runs return 0; both pre-fix invariants hold:

- The null path of `impl Drop` never panics.
- For any arbitrary `usize`, only the null case actually drops (non-null pointers are forgotten before Drop can reach the FFI — we must NOT call `duckdb_disconnect` on a fabricated address).

## Files Created/Modified

- `.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md` — Wave-0 spike evidence: A4 lldb backtrace + CPU sample, A6 grep results, A7 deferral rationale.
- `src/conn_guard.rs` — new module. Inner `mod inner { ... }` gated on `feature = "extension"` carries the runtime; outer module exposes `tests` (extension feature) and `tests_bundled` (default feature) so the null-drop signature test compiles under both configurations.
- `src/lib.rs` — added `pub mod conn_guard;` (no feature gate at the module declaration so the bundled-feature test still runs; the FFI-touching body is feature-gated internally).
- `test/integration/test_readonly_load.py` — added `_connect_with_watchdog` helper, `_connect_config` + `_minimal_create_sql` factory helpers, and five new tests (`test_in_process_bootstrap_then_readonly_fresh`, `..._existing`, `test_in_process_load_only_then_readonly`, `test_in_process_readonly_then_readwrite`, `test_repeated_load_close_no_busy_spin`). All five wired into `main()`'s `run_test` aggregator AFTER the existing subprocess tests, in the order documented in the test-file inline comments.

## Spike Outcomes (with pointer to `65-01-SPIKES.md`)

| Spike | Outcome | Implication for downstream plans |
|-------|---------|----------------------------------|
| A4 | **CONFIRMED** — `DBInstanceCache::GetInstanceInternal` busy-spin (99.4 % CPU; tight `ldr/cmn/b.ne` loop) | Plan 02 / 03 fix shape (release the leaked `shared_ptr<DatabaseInstance>` by NOT owning a long-lived `duckdb_connection`) is justified. |
| A6 | **`BindInfo`-DOES-NOT-EXPOSE-db_handle** in duckdb-rs 1.10502.0 | Plan 03 must adopt shape (a) — `CatalogHandle { db, catalog_table_present }` carried in `extra_info`, not shape (b). |
| A7 | **DEFERRED-TO-PLAN-02** (no current `db_handle` to probe with) | Plan 02 inherits the falsification responsibility; first parser_override sqllogictest under the per-call shape will deadlock if re-entrancy is unsafe. |

Full evidence in `.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md`.

## ConnGuard Public API

| Item | Visibility | Signature |
|---|---|---|
| `struct ConnGuard` | `pub(crate)` | `{ conn: ffi::duckdb_connection }` (single-field, no Clone / Copy) |
| `unsafe fn open(db: ffi::duckdb_database) -> Result<Self, String>` | `pub(crate)` | Calls `duckdb_connect`; returns `Err("duckdb_connect failed (rc={rc})")` on failure |
| `fn raw(&self) -> ffi::duckdb_connection` | `pub(crate)` | Borrows the raw handle (pointer-sized, cheap copy) |
| `impl Drop for ConnGuard` | — | `if !self.conn.is_null() { duckdb_disconnect(&mut self.conn); self.conn = null_mut(); }` |
| `unsafe impl Send for ConnGuard` | — | Per-scope ownership; transfer is safe |
| (no `impl Sync`) | — | Deliberately omitted — a guard belongs to one scope |
| (no `impl Clone` / `Copy`) | — | Deliberately omitted — single-owner is the invariant |

**Send-not-Sync rationale:** DuckDB serialises in-flight statement execution on a single connection internally, so transferring the guard between threads is safe (no aliasing — single owner). `Sync` would allow concurrent shared references, but the per-call usage pattern (each parse-override / bind callback opens its own guard, uses it, drops it) never needs a `&ConnGuard` shared across threads — and shipping `Sync` would invite a future caller to do so under the false assumption that DuckDB synchronises arbitrary concurrent C-API calls on the same handle.

**`#[allow(dead_code)]` on `impl ConnGuard`** — Plan 01 introduces the type; Plans 02 and 03 will consume `open` and `raw` from `rewrite_*` (parser_override) and the read-side bind callbacks. The allow-attribute keeps `just ci` (clippy pedantic) green during the single-plan window. Plan 04 verification should confirm both `open` and `raw` are referenced in production code and remove the attribute.

## Watchdog Test Behaviour (Baseline vs Post-Fix)

| Test | Phase 65 ID | Baseline (v0.9.0) | Post-fix (v0.9.1 after Plans 02/03) |
|------|-------------|-------------------|-----------------|
| `test_in_process_bootstrap_then_readonly_fresh` | B1 | TimeoutError after 5 s (busy-spin) | Returns in ms; `list_semantic_views()` → `[("v",)]` |
| `test_in_process_bootstrap_then_readonly_existing` | B2 | TimeoutError after 5 s | Returns in ms; same assertion |
| `test_in_process_load_only_then_readonly` | B3 | TimeoutError after 5 s — isolates leak to LOAD (no CREATE) | Returns in ms |
| `test_in_process_readonly_then_readwrite` | B4 / D-09 | TimeoutError after 5 s (reverse direction; same root cause) | Returns in ms |
| `test_repeated_load_close_no_busy_spin` | B11 | First iteration aborts via TimeoutError; subsequent 49 iterations never run | All 50 iterations complete in << 10 s total |

**Documented thread-leak caveat (per VALIDATION §Manual-Only row 2):** the daemon thread cannot kill an uninterruptible C++ tight loop. On baseline each failing watchdog leaks one daemon thread for the rest of the process lifetime. Mitigation: the five new tests are registered LAST in `main()`'s `run_test` aggregator so leaks do not contaminate the earlier subprocess-style tests (B5 group). The leak is acceptable because (a) the tests are fail-once regression guards rather than continuous CI, (b) the daemon threads die at process exit, and (c) the leak disappears entirely once Plans 02/03 land.

## Decisions Made

- **Co-located TDD for Task 2** (deviation from strict RED→GREEN external observability) — the proptest needs the actual struct layout to exercise the `mem::transmute` path, so splitting test and impl across commits would have produced an intermediate compile failure. Acceptable since both invariants are validated by the final `cargo test --lib --features extension conn_guard` run.
- **`pub mod conn_guard;` without `#[cfg(feature = "extension")]` gate** — chosen so that `cargo test --lib conn_guard` (no features) at least exercises the cfg-gate-resolution path via the `tests_bundled` placeholder. The inner FFI body is still gated. This trades one always-loaded outer module for a tighter `cargo test` story under default features.
- **Defensive re-null in Drop** — `duckdb_disconnect` already zeroes the pointer (`cpp/include/duckdb.cpp:266477`), but I added an explicit `self.conn = std::ptr::null_mut()` after the FFI call. Negligible cost; survives any future libduckdb-sys signature change that might drop the zeroing.

## Deviations from Plan

### Auto-fixed issues

**1. [Rule 3 — Blocking] `pub mod conn_guard` gating change so default-feature tests run**

- **Found during:** Task 2 (verification step `cargo test --lib conn_guard`)
- **Issue:** Original wiring `#[cfg(feature = "extension")] pub mod conn_guard;` meant the module wasn't compiled at all under default `bundled` features, so `cargo test --lib conn_guard` reported "0 tests" rather than running the bundled-feature `tests_bundled` smoke test the plan called for.
- **Fix:** Removed the `#[cfg]` on the `mod` declaration; the inner `mod inner { ... }` keeps the FFI body gated. The outer module always compiles; only the `ConnGuard` type itself is conditional.
- **Files modified:** `src/lib.rs`
- **Verification:** `cargo test --lib conn_guard` now runs 1 test under default features and 2 tests under `--features extension`.
- **Committed in:** `e38ae7a` (Task 2 commit)

**2. [Rule 2 — Critical, dead-code suppression for forward consumers] `#[allow(dead_code)]` on ConnGuard::open / raw**

- **Found during:** Task 2 (verification step `cargo build --features extension`)
- **Issue:** `cargo build --features extension --no-default-features` emitted "associated items `open` and `raw` are never used" warnings, which would trip `just ci`'s clippy pedantic gate in Plans 02/03's per-task `just ci` runs.
- **Fix:** Added `#[allow(dead_code)]` on the `impl ConnGuard` block introducing `open` / `raw`. Documented inline that Plans 02/03 will consume the API and that the attribute should be removed in Plan 04 verification.
- **Files modified:** `src/conn_guard.rs`
- **Verification:** `cargo build --features extension --no-default-features` is now warning-free.
- **Committed in:** `e38ae7a` (Task 2 commit)

**3. [Rule 2 — Critical] `#[allow(unused_imports)]` on the crate-root re-export**

- **Found during:** Same step as deviation 2 above.
- **Issue:** Same "unused import" warning on `pub(crate) use inner::ConnGuard;`.
- **Fix:** `#[allow(unused_imports)]` with inline comment explaining Plans 02/03 will consume this re-export.
- **Files modified:** `src/conn_guard.rs`
- **Verification:** Same as #2.
- **Committed in:** `e38ae7a` (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 blocking, 2 critical-but-cosmetic). All three are forward-looking — they keep `just ci` green during the Plans-02-and-03 window where the new API exists but is not yet consumed in production. Plan 04 verification removes them as part of confirming the API is actually wired.

**Impact on plan:** Zero scope creep. All three deviations are mechanical cleanups; the API surface and behaviour match the planner's prescription exactly.

## Issues Encountered

None of substance. The `nice(5) failed: operation not permitted` shell artefact when starting the repro in background (Task 1) was the macOS sandbox's expected denial of `nice` syscalls and did not affect the repro PID capture (read straight from the script's first `print` line). The hung repro process was cleanly killed with `kill -9 28714` after capturing the lldb backtrace.

## User Setup Required

None — this plan is internal scaffolding and Wave-0 evidence. No external configuration changes.

## Next Phase Readiness

- **Plan 02 (Wave 1)** can now import `crate::conn_guard::ConnGuard` and refactor `OverrideContext` to carry `db_handle: duckdb_database`. Spike A7's falsification will fire automatically when Plan 02 first runs an existing parser_override sqllogictest under the per-call shape.
- **Plan 03 (Wave 2/3)** must adopt shape (a) per Spike A6's conclusion: `CatalogHandle { db, catalog_table_present }` as the `extra_info` payload; per-call `ConnGuard` opened in each of the 14 read-side table-function binds + 2 scalar `invoke`s.
- **Plan 04 (Wave 5)** re-runs the five new tests in `test/integration/test_readonly_load.py` and confirms they flip green; closes the deferred-items entry from Phase 63 per LIFE-04.
- **`just test-all` is intentionally NOT a Plan 01 gate** — the plan's `<verification>` block explicitly notes that the new in-process tests are expected to fail on baseline. Plan 04 verification owns the full-suite green requirement.

---

## Self-Check: PASSED

Verified post-write (before final commit):

- `.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md` — present
- `.planning/phases/65-overridecontext-connection-teardown/65-01-SUMMARY.md` — present
- `src/conn_guard.rs` — present
- `src/lib.rs` — present (modified)
- `test/integration/test_readonly_load.py` — present (modified)
- Commit `fac190b` (Task 1 — spikes) — present in `git log --all`
- Commit `e38ae7a` (Task 2 — ConnGuard) — present in `git log --all`
- Commit `53496bc` (Task 3 — watchdog + 5 tests) — present in `git log --all`

---

*Phase: 65-overridecontext-connection-teardown*
*Completed: 2026-05-21*
