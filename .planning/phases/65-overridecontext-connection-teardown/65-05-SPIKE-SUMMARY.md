# Phase 65 Plan 05 — Wave 0 Bridge Spike Summary

**Phase:** 65-overridecontext-connection-teardown
**Plan:** 05 (read-path migration wave)
**Task:** 1 of 6 (Wave 0 bridge spike)
**Status:** **SUCCESS — bridge mechanism validated, awaiting user sign-off before Tasks 2-6**
**Branch:** `milestone/v0.10.0`
**Commits:**
- `2db2b9b` — `feat(65-05): Wave 0 bridge spike — migrate list_semantic_views to C++ Catalog API`
- `928189c` — `test(65-05): sqllogictest gate for Wave 0 bridge spike`

---

## One-liner

The Rust↔C++ bridge for read-path callbacks is `reinterpret_cast<duckdb_connection>(Connection*)` — direct pointer cast, zero new infrastructure required. The C-API handle is literally a `Connection *` (confirmed by reading `duckdb.cpp:266432-266447`). All 16 remaining read-side migrations can adopt the same pattern.

---

## (a) Chosen bridge mechanism

**Choice A (selected) — `reinterpret_cast` of stack `Connection *` to `duckdb_connection`.**

The `duckdb_connection` C-API handle is defined by `duckdb_connect` as exactly:

```cpp
// duckdb.cpp:266440-266446
Connection *connection;
try {
    connection = new Connection(*wrapper->database);
} catch (...) {
    return DuckDBError;
}
*out = reinterpret_cast<duckdb_connection>(connection);
return DuckDBSuccess;
```

So `duckdb_connection` is a typedef for `void *` whose payload is always a `Connection *`. Casting in the other direction is symmetric and lossless. The spike's C++ bind callback uses:

```cpp
// cpp/src/shim.cpp::sv_list_semantic_views_bind
Connection probe(*context.db);
duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
sv_list_semantic_views_bind_rust(borrowed, ...);
// probe destructs at end-of-scope -> ConnectionManager::RemoveConnection
```

**Critical lifecycle property — BORROW, not transfer:**

The Rust dispatcher receives a borrowed handle. It MUST NOT call `duckdb_disconnect(conn)` — that would `delete` a stack-allocated `Connection` and corrupt the C++ stack. Teardown is handled by the C++ bind scope's `~Connection()` (which acquires `connections_lock` and calls `ConnectionManager::RemoveConnection`). The READ-PATH-SPIKE's READ-BIND-RC0 result (in `65-READ-PATH-SPIKE.md` lines 67-92) already confirmed that ctor + dtor of `Connection(*context.db)` from the bind thread are deadlock-free on three consecutive bind invocations.

**Choice B (not needed) — C++ helper trait object / inverted-call vtable.**

This was the planned fallback (`65-05-PLAN.md` Step A option 3) for if Choice A failed to compile or segfaulted at runtime. Choice A works cleanly with no compile-time or runtime fuss, so Choice B is shelved. The plan's "halt-on-failure gate" did NOT fire — no user sign-off escalation needed for the mechanism itself.

---

## (b) Rust dispatcher shape

`src/ddl/list.rs::sv_list_semantic_views_bind_rust` — `extern "C"` entry, same FFI conventions as `src/ddl/alter_helpers_ffi.rs::sv_compute_create_from_yaml_rust` and `src/parse.rs::sv_parser_override_rust`:

```rust
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_list_semantic_views_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // ... probe catalog table presence on the per-call conn ...
        let reader = CatalogReader::new(conn, table_present);
        let entries = reader.list_all()?;
        // ... serialize entries to length-prefixed binary buffer ...
        publish_owned_buffer(buf, out_ptr, out_len);
        0_u8
    }));
    // catch_unwind -> rc=2 on panic; otherwise pass through rc.
}
```

Return codes match the established Plan 04 convention:

- `0` — success; `(out_ptr, out_len)` populated, caller frees via `sv_free_buffer`.
- `1` — catalog read error; `error_buf` populated.
- `2` — internal error (panic across FFI); `error_buf` populated.

**Wire format (binary, length-prefixed):**

```
u32 row_count (little-endian)
for each row:
    for each of 6 columns:
        u32 byte_len (little-endian)
        byte_len bytes (UTF-8, NOT NUL-terminated)
```

The wire format avoids needing matched struct layouts across the FFI boundary — the C++ side parses with two `sv_read_u32_le` / `sv_read_string` helpers (which throw `BinderException` on truncation, defensive against a panic in the Rust serializer). For TFs with named/typed columns identical across all read-side functions, the same wire format generalizes trivially — pass a column count to the dispatcher and serialize `M cols × N rows`.

---

## (c) Per-call Connection lifecycle verification

**Empirically confirmed by 5 sqllogictest assertions in `test/sql/65_read_bridge_spike.test`:**

- **B1:** Empty catalog (no `semantic_layer._definitions` table) — `Connection(*context.db)` opens, probes for catalog presence, short-circuits to 0 rows. Drop is clean (sqllogictest exits without hang).
- **B2:** Single view after CREATE — bind opens a NEW per-call connection (the previous one already dropped), reads, drops. Returns 1 correct row.
- **B5:** Two successive identical SELECTs — each call opens its own per-call connection. No stale state between invocations. Confirms the "per-call, not per-statement" lifecycle.

No deadlock, no leak, no use-after-free. The `~Connection()` destructor runs at the end of every bind scope, matching the READ-BIND-RC0 evidence from the C++-only spike.

**ADBC transactional invariant (D-21) verified:** `test_adbc_transactions.py` 6/6 PASS after the spike — no regression in the write-path connection model.

---

## (d) Rough edges discovered

**1. Catalog-table-present probe moved inline.** The original `init_extension` probes once at LOAD time and stashes the bool in `CatalogReader::new(conn, catalog_table_present)`. Under the per-call connection model, that LOAD-time bool no longer applies (the per-call conn doesn't exist yet, and on a fresh per-call conn against a read-only attached DB, the table may genuinely be missing). The Rust dispatcher now does a small `information_schema.tables` probe on every bind. Cost: one extra SQL round-trip per call. Acceptable for read-side TFs; if it shows up as a perf hot spot for `semantic_view()` (Task 6) we can cache the presence-flag on the `ClientContext` via a small registered-state map. Documented inline in `src/ddl/list.rs::probe_catalog_table_present`.

**2. Legacy VTab struct kept as dead code for one wave.** `ListSemanticViewsVTab` + `ListBindData` + `ListInitData` are marked `#[allow(dead_code)]` in `src/ddl/list.rs` until Task 6 lands. Rationale: keeping them visible during the migration makes the per-TF migration commits a clean "swap one registration site" rather than a mixed "swap registration + delete + add" — easier to review. A single cleanup commit at the end of Plan 05 deletes the dead code (and the equivalent for the 16 other VTabs). Tracked as a follow-up item, not a TECH-DEBT entry.

**3. No new package dependencies.** Threat-model row T-65-05-SC (npm/pip/cargo install verification) — passes trivially. The spike uses only existing dependencies (`libduckdb-sys` is already in `Cargo.toml`).

**4. `Connection` is unaware of the borrow.** Standard C++ — there's no compile-time enforcement that the Rust side doesn't try to call `duckdb_disconnect`. The protection is doc-only: a critical comment block in both `cpp/src/shim.cpp::sv_list_semantic_views_bind` and `src/ddl/list.rs::sv_list_semantic_views_bind_rust` documenting the borrow contract. Tasks 2-6 must replicate the same comment block at each new dispatcher. Future-proofing idea (not required for v0.10.0): wrap the borrowed handle in a `#[repr(transparent)] struct BorrowedConn(duckdb_connection)` newtype on the Rust side with no `Drop` impl that would call disconnect — purely typed-safety with zero runtime cost.

**5. `cargo fmt` runs in pre-commit.** Cargo-husky hook auto-formatted the Rust file in the spike commit. Harmless but added one round-trip. Note for future task commits: run `cargo fmt` before `git add` to avoid the re-stage.

---

## (e) LOC estimate for the 16 remaining migrations

**Spike LOC actually written (commit `2db2b9b`):**

| Surface | Lines added | What it includes |
|---|---|---|
| `cpp/src/shim.cpp` | 246 | FFI decl block (40), wire-format helpers `sv_read_u32_le`/`sv_read_string` (28), bind callback (130), init_local + exec callbacks (48) |
| `cpp/src/shim.hpp` | 16 | 1 `extern "C"` decl + doc comment |
| `src/ddl/list.rs` | 245 | Doc header (38), FFI dispatcher body (110), `probe_catalog_table_present` (24), `write_err` helper (14), `#[allow(dead_code)]` markers (5), retained imports + structural docs |
| `src/lib.rs` | 32 | 1 `extern "C"` decl + new registration call site + retired-VTab comment |
| **Total spike (production code)** | **539** | |
| `test/sql/65_read_bridge_spike.test` | 175 | 5 behavioural assertions, cleanup |
| `test/sql/TEST_LIST` | 1 | Test list registration |
| **Total spike (test)** | **176** | |

**Reusable scaffolding now in place:**

- `sv_read_u32_le`, `sv_read_string` (C++) — wire-format parsers, reusable verbatim by every future migration.
- `write_err`, `probe_catalog_table_present` (Rust) — error-write helper and catalog probe, reusable verbatim.
- `sv_register_table_function` shim (already there from Plan 04).
- Wire format convention (length-prefixed `u32 row_count; for each row × col: u32 len + bytes`) — applies to every TF that emits structured rows.

**Per-additional-TF estimate (assuming reuse of helpers):**

| TF complexity | C++ LOC | Rust LOC | Notes |
|---|---|---|---|
| Zero-arg TF (5 of them: `list_terse_semantic_views`, `show_semantic_dimensions_all`, `show_semantic_metrics_all`, `show_semantic_facts_all`, `show_semantic_materializations_all`) | ~80-100 | ~70-100 | Reuses helpers; bind body differs only in BindData struct + which CatalogReader method + column schema |
| Single-VARCHAR-arg TF (6 of them: `show_columns_in_semantic_view`, `describe_semantic_view`, `show_semantic_dimensions`, `show_semantic_metrics`, `show_semantic_facts`, `show_semantic_materializations`) | ~95-120 | ~90-130 | Adds arg extraction in bind + name lookup branch; the 3 that probe column types (`describe`, `show_columns`, `show_semantic_dimensions`) add LIMIT-0 probe + type-cache integration (Task 2) |
| Two-arg TF (1: `show_semantic_dimensions_for_metric`) | ~110 | ~110 | Like single-arg + one extra arg |
| Scalar (2: `get_ddl`, `read_yaml_from_semantic_view`) | ~140 each | ~110 each | Needs new `sv_register_scalar_function` shim (Task 2 Step A, ~80 LOC C++) + scalar exec callback (DataChunk per-row loop) |
| Varargs TF (1: `explain_semantic_view`) | ~130 | ~120 | Like single-arg + named-parameter extraction |
| Varargs TF main expansion (1: `semantic_view`) | ~150-200 | ~150-200 | Highest blast radius; runs expand pipeline + may invoke sub-query on the per-call conn |

**Total estimate for Tasks 2-6 (16 migrations):**

- C++ (`cpp/src/shim.cpp`): **~1,500-1,900 LOC** added (plus ~80 LOC for `sv_register_scalar_function` shim in Task 2 Step A; less than initially feared — the helpers and wire format generalize cleanly).
- Rust (`src/ddl/*.rs`, `src/query/*.rs`): **~1,400-1,800 LOC** added (most is per-TF dispatcher + BindData; type cache module from Task 2 Step B is ~100-150 LOC independently).
- `src/lib.rs`: ~25-30 LOC retired (each `register_table_function_with_extra_info` line replaced with `sv_register_<name>` call) + the final H2 retirement block (10 lines deleted).

The estimate is **higher than the plan's implicit assumption** of "thin per-callback adapters" — each migration is closer to a small-but-real rewrite than a one-line registration swap, because the bind body has to be ported from duckdb-rs's `BindInfo` API to a hand-written C++ TableFunction bind + a Rust FFI dispatcher. But it's nowhere near the worst-case "rewrite all 17 in C++" feared in the READ-PATH-SPIKE interpretation (line 123). **The architecture remains the cheapest viable shape.**

Net Plan 05 size estimate (after Task 1):
- Production code: ~3,000-3,800 LOC added; ~50 LOC retired (H2 + dead VTab cleanup at end).
- Tests: 16 SQL test files (likely 2-3 will be new spike-style files; the rest exercise existing test suites which stay byte-identical) + `src/type_cache.rs` unit tests + `test/integration/test_concurrent_reads_per_call_conn.py`.

---

## (f) Recommended sequencing for the follow-up agent run

The spike has locked the bridge mechanism, so Tasks 2-6 can proceed with high confidence. Recommended dispatch:

**Single follow-up agent run, all 5 remaining tasks in one shot, in the plan's sequence (Wave 1 → Wave 6):**

1. **Task 2 (Wave 1)** is the next critical task — it introduces `sv_register_scalar_function` (Task 4 dependency) AND the process-local `type_cache` module (Tasks 2-3-6 dependency). It also migrates the 5 zero-arg TFs which are the simplest mechanical reuses of the spike's pattern. Estimated effort: high (new shim + new module + 5 migrations), but unblocks everything else.
2. **Task 3 (Wave 2+3)** — 7 single/two-arg TFs. Pure pattern reuse from Task 1+2. The 3 that probe column types (`describe`, `show_columns`, `show_semantic_dimensions`) consume the type cache.
3. **Task 4 (Wave 4)** — 2 scalars via the `sv_register_scalar_function` shim from Task 2. Slightly different exec callback shape (per-row scalar vs whole-chunk TF) but the bind side is the same.
4. **Task 5 (Wave 5)** — `explain_semantic_view`. Higher blast radius because of varargs + named params, but the expand pipeline itself is unchanged.
5. **Task 6 (Wave 6)** — `semantic_view` (main expansion path) + H2 retirement + new `test_concurrent_reads_per_call_conn.py`. The final atomic step is the H2 deletion in `src/lib.rs:498-507` — gates `just test-all` + `just ci` green.

**Splitting Tasks 2-6 across multiple agent runs is NOT recommended** unless the user wants to review LOC progress incrementally. Each task builds on the previous, and the cross-task dependencies (type cache, sv_register_scalar_function shim) make a single run more efficient than checkpointing between tasks.

**One thing worth user sign-off before dispatch:** the LOC estimate (~3,000-3,800 production LOC added) is materially larger than a naive read of the plan might suggest. If the user wants a tighter scope (e.g. defer Tasks 5-6 to a v0.10.1 patch and ship Plan 05 in v0.10.0 with only the 13 TFs that don't touch `semantic_view`), that's a viable scope split — but it would push H2 retirement to v0.10.1, deferring the LIFE-01 fix.

---

## Self-check

- [x] **Plan 05 Task 1 acceptance criteria 1-7:** all pass (see verification block below).
- [x] **Test count:** `just test-sql` 52/52 → 53/53 PASS. Zero regressions.
- [x] **Cargo nextest:** 933/933 PASS.
- [x] **D-21 invariant:** `test_adbc_transactions.py` 6/6 PASS.
- [x] **Two commits:** `2db2b9b` (spike code) + `928189c` (test).
- [x] **Bridge mechanism documented:** in both `cpp/src/shim.cpp` (lines around bind callback) and `src/ddl/list.rs` (FFI dispatcher doc header).
- [x] **Bridge does NOT trigger BIND-THREAD-RC1:** the C-API path (`duckdb_connect(db_handle)`) failed in BIND-THREAD-RC1; the C++ direct path (`Connection(*context.db)`) succeeded in READ-BIND-RC0. The spike uses the C++ direct path with a `reinterpret_cast` to the C-API surface — same lifecycle, no BIND-THREAD-RC1 trigger.
- [x] **Halt-on-failure gate did not fire:** Choice A worked; no escalation to Choice B (trait object).

---

## Verification commands (rerun-friendly)

```bash
# Build
just build

# Spike test in isolation
./configure/venv/bin/python3 -u -m duckdb_sqllogictest --test-dir test/sql \
    --file-list <(echo test/sql/65_read_bridge_spike.test) \
    --external-extension build/debug/semantic_views.duckdb_extension

# Full sqllogictest suite (must be 53/53)
just test-sql

# Rust unit tests (must be 933/933)
cargo nextest run

# D-21 invariant
uv run test/integration/test_adbc_transactions.py

# Acceptance-criteria greps
grep -nE 'register_table_function_with_extra_info.*list_semantic_views' src/lib.rs  # → 0 matches
grep -c 'sv_list_semantic_views_bind' cpp/src/shim.cpp                             # → 2+
grep -c 'sv_list_semantic_views_bind_rust' src/ddl/list.rs                         # → 3+
```

---

## Awaiting

**Tasks 2-6 await user sign-off on the bridge mechanism + LOC extrapolation.** Once the user reviews this summary and confirms:

1. Choice A (`reinterpret_cast<duckdb_connection>(Connection*)`) is the correct mechanism to lock in for the remaining 16 migrations, AND
2. The ~3,000-3,800 LOC estimate is an acceptable Plan 05 budget,

a fresh agent invocation can dispatch Tasks 2-6 in a single run. This SPIKE-SUMMARY remains the canonical bridge-mechanism reference for that run.

If the user prefers Choice B or a different scope split (e.g. ship Plan 05 partial in v0.10.0 + Tasks 5-6 in v0.10.1), this is the decision point.
