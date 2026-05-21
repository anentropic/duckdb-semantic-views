# Phase 65 Plan 01 — Wave-0 Spike Evidence

**Date:** 2026-05-21
**Build:** `just build` succeeded at HEAD `5684605` on `milestone/v0.9.1`
**Repro file:** `$TMPDIR/65_repro.py` (committed snapshot at `/tmp/65_repro.py` for the session — content reproduced in §A4 below)

---

## A4 — DBInstanceCache busy-spin (CONFIRMED)

### Repro

The minimal repro saved to `$TMPDIR/65_repro.py`:

```python
import duckdb, gc, os, tempfile
from pathlib import Path

EXT_DIR  = "/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/build/debug/extension"
EXT_PATH = "/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/build/debug/semantic_views.duckdb_extension"

tmp = tempfile.mkdtemp(prefix="65_repro_")
db = str(Path(tmp) / "repro.duckdb")
print(f"PID={os.getpid()}\nDB={db}", flush=True)

w = duckdb.connect(db, config={"allow_unsigned_extensions": "true",
                               "extension_directory": EXT_DIR})
w.execute(f"FORCE INSTALL '{EXT_PATH}'")
w.execute("LOAD semantic_views")
w.execute("CREATE TABLE t (i INT)")
w.execute("CREATE SEMANTIC VIEW v AS "
          "  TABLES (t1 AS t PRIMARY KEY (i)) "
          "  DIMENSIONS (t1.i AS t1.i) "
          "  METRICS (t1.c AS COUNT(*))")
w.close(); del w; gc.collect()

# This call enters the busy-spin and never returns.
r = duckdb.connect(db, read_only=True,
                   config={"allow_unsigned_extensions": "true",
                           "extension_directory": EXT_DIR})
```

### CPU sample

After the script printed `Reopening RO (this should hang)...`, `ps -p <pid>` showed:

```
  PID STAT  %CPU COMMAND
28714 R     99.4 ./configure/venv/bin/python3 /tmp/claude-501/65_repro.py
```

State `R` (running on CPU) at **99.4 % CPU** on a single core. This rules out a futex / mutex wait — a blocked thread would be `S` (interruptible sleep) at ~0 % CPU.

### lldb backtrace (verbatim)

```
(lldb) process attach --pid 28714
Process 28714 stopped
* thread #1, queue = 'com.apple.main-thread', stop reason = signal SIGSTOP
    frame #0: 0x000000010786f804 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol54141 + 940
_duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol54141:
->  0x10786f804 <+940>: ldr    x8, [x19, #0x8]
    0x10786f808 <+944>: cmn    x8, #0x1
    0x10786f80c <+948>: b.ne   0x10786f804               ; <+940>
    0x10786f810 <+952>: ldr    x0, [x21]
Target 0: (python3.11) stopped.

(lldb) bt all
* thread #1, queue = 'com.apple.main-thread', stop reason = signal SIGSTOP
  * frame #0: 0x000000010786f804 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol54141 + 940
    frame #1: 0x000000010787067c _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol54148 + 236
    frame #2: 0x0000000106060194 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1830 + 2164
    frame #3: 0x000000010603a774 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1590 + 84
    frame #4: 0x000000010603a714 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol1589 + 24
    frame #5: 0x0000000105fe6484 _duckdb.cpython-311-darwin.so`___lldb_unnamed_symbol709 + 3240
    frame #6: 0x000000010301a6e4 python3.11`cfunction_vectorcall_FASTCALL_KEYWORDS + 160
    frame #7: 0x0000000102fcd9c0 python3.11`PyObject_Vectorcall + 80
    frame #8: 0x00000001030bdc08 python3.11`_PyEval_EvalFrameDefault + 34740
    ... [Python frames calling duckdb.connect(...)]
  thread #2..#15  semaphore_wait_trap (idle background pool threads)
```

### Diagnosis: **CONFIRMED**

- Symbols in `_duckdb.cpython-311-darwin.so` are stripped (the Python `duckdb` package ships without debug symbols), so the `bt` shows `___lldb_unnamed_symbol####` names rather than `DBInstanceCache::GetInstanceInternal`.
- The frame-0 disassembly is unambiguous:
  ```
  0x10786f804: ldr   x8, [x19, #0x8]      ; load a word at [x19+8]
  0x10786f808: cmn   x8, #0x1             ; compare with -1 (the sentinel for `expired()` true)
  0x10786f80c: b.ne  0x10786f804          ; branch back to the load if NOT equal → tight spin
  ```
  This is the textbook compilation of `while (!weak_cache_entry.expired()) {}`. `weak_ptr::expired()` checks the control-block's weak-count sentinel; `cmn x, #1` tests whether the loaded value equals `-1` (all-ones), which is the marker for an expired control block on the libc++/libstdc++ refcount layout used on macOS arm64.
- All non-main threads are `semaphore_wait_trap` (background allocator / GC pool — idle, not contributing).
- Frame depth (only 6 native frames from C-API entry → `___lldb_unnamed_symbol709` (`duckdb_connect`/`duckdb_open_internal` area) → `___lldb_unnamed_symbol1589/1590` (`GetOrCreateInstance` / `GetInstanceInternal`) → `___lldb_unnamed_symbol1830` (cache lookup helper) → `___lldb_unnamed_symbol54141/54148` (the busy-spin shim)) matches the structural depth expected from `duckdb_open_internal → DBInstanceCache::GetOrCreateInstance → DBInstanceCache::GetInstanceInternal → while (!weak_cache_entry.expired())` documented in RESEARCH §2.1 with citations to `duckdb.cpp:278022-278024, 278090-278129`.

**Conclusion: `DBInstanceCache::GetInstanceInternal` busy-spin frame is CONFIRMED present.** The RESEARCH §2.1 root-cause chain is validated. The downstream-reported ">45s hang" is a CPU busy-spin (state `R`, 99.4 % CPU), not a mutex / futex wait. Diagnosis stands; planner's assumptions remain valid.

The hung process was killed with `kill -9 28714` after the backtrace was captured (busy-spin is uninterruptible from Python).

---

## A6 — `BindInfo` exposure of `duckdb_database` (NOT EXPOSED → Plan-03 shape (a))

### Search

Searched the duckdb-rs binding crate version pinned in `Cargo.toml` (`duckdb=1.10502.0`) at `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/duckdb-1.10502.0/`:

```bash
# Method 1 — grep the entire vtab / vscalar / extension / core surface.
$ grep -rn "duckdb_database\|db_handle\|get_database\|database_handle" \
    src/vtab/ src/vscalar/ src/extension.rs src/core/
(no output — zero hits)

# Method 2 — enumerate all BindInfo methods.
$ grep -n "BindInfo" src/vtab/function.rs src/vtab/mod.rs
src/vtab/function.rs:23:pub struct BindInfo { ptr: duckdb_bind_info }
src/vtab/function.rs:27:impl BindInfo {
src/vtab/function.rs:115:impl From<duckdb_bind_info> for BindInfo {
src/vtab/mod.rs:23:pub use function::{BindInfo, InitInfo, TableFunction, TableFunctionInfo};
...
```

The full method surface of `BindInfo` (verbatim from `src/vtab/function.rs:27-113`):

| Method | Returns |
|---|---|
| `add_result_column(name, type)` | `()` |
| `set_error(&str)` | `()` |
| `unsafe set_bind_data(*mut c_void, free_fn)` | `()` |
| `get_parameter_count()` | `u64` |
| `get_parameter(idx)` | `Value` |
| `get_named_parameter(name)` | `Option<Value>` |
| `set_cardinality(card, exact)` | `()` |
| `get_extra_info<T>()` | `*const T` |

`InitInfo` and `TableFunctionInfo` were also inspected — neither exposes a `duckdb_database` accessor.

### Conclusion: **`BindInfo`-DOES-NOT-EXPOSE-db_handle**

The Rust duckdb-rs crate at version `1.10502.0` does **not** surface `duckdb_database` from `BindInfo`, `InitInfo`, or `TableFunctionInfo`. The only avenue for bind callbacks to reach the `duckdb_database` is to store it inside the `extra_info` payload (which `get_extra_info::<T>()` returns).

This implies **Plan 03 must adopt the planner-prescribed shape (a):** introduce a lightweight `CatalogHandle { db: duckdb_database, catalog_table_present: bool }` and pass `&catalog_handle` to `register_table_function_with_extra_info` for each of the 14 read-side table functions and 2 scalar functions. Each bind callback retrieves the handle via `bind.get_extra_info::<CatalogHandle>()` and opens a per-call `ConnGuard` from `handle.db`. The bind callback drops the guard before returning, so no long-lived `duckdb_connection` survives bind.

The alternative (b) (refactor `CatalogReader` to carry `db` and internally connect per method) is also viable per RESEARCH §6.1, but shape (a) is preferred because (i) it matches the planner's shape labelling, (ii) it amortises one connect per bind across all reads in that bind, and (iii) it keeps `CatalogReader`'s `conn` field stable so the existing `prepared_lookup` / `execute_list_all` / `execute_list_names` helpers do not need refactoring.

Plan 03 will need to update `OverrideContext` in lockstep — the same `db_handle: duckdb_database` field, sourced from `sv_register_parser_hooks`'s first argument (already passed through from `src/lib.rs:416`).

---

## A7 — Parser-override re-entrancy (RESOLVED — RE-ENTRANCY-UNSAFE on DuckDB 1.5.2)

### Empirical outcome (updated 2026-05-21 by Plan 02 execution)

Plan 02 plumbed `db_handle` through `OverrideContext` (commits `0d2c0b7`, `f9caafe`) and converted every `rewrite_*` site to open a per-call `ConnGuard::open(ctx.db_handle)`. The first `just test-sql` run after the refactor produced concrete falsification evidence:

- **47 sqllogictests run, 43 FAILED** with `Parser Error: catalog connection failed: duckdb_connect failed (rc=1)` on every test that reaches a `ConnGuard::open` site inside `parser_override` (sites: `rewrite_drop_or_alter`, `emit_native_create_sql`, `rewrite_yaml_file_create`).
- **4 PASS** — `error_caret_create.test`, `error_caret_drop.test`, `error_caret_multiline.test`, `error_caret_unicode.test`. These exercise near-miss / invalid-syntax paths that never reach `ConnGuard::open` and thus confirm the failure is precisely scoped to opening a fresh connection from inside the parser callback.

**Conclusion: RE-ENTRANCY-UNSAFE.** The RESEARCH §3.3 / §6.5 standalone-library argument that `connections_lock` is per-`ConnectionManager` and does not gate the caller's existing connection's parse step is **falsified by the bundled DuckDB 1.5.2** used in the `--features extension` build path. We cannot open a fresh `duckdb_connection` from the parse thread mid-`Parser::ParseQuery` on this DuckDB version. Per-call ConnGuard from inside `parser_override` is not a viable architectural shape.

**Evidence log:** `.planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log` (verbatim sqllogictest output; preserved on `milestone/v0.9.1` via commit `656bae7`).

**Phase-level consequence:** Plan 02 hit the T-65-05 stop-and-revisit clause and was marked PARTIAL. The user chose Option A from the resulting checkpoint (defer catalog reads to bind/plan time, OUTSIDE the parse lock — aligns with CONTEXT.md D-07.1). The structural commits (`db_handle` plumbing, C++ shim signature update, `ConnGuard` consumer wiring) survive as foundation for the reshape; the `rewrite_*` per-call `ConnGuard` sites are now known-broken and will be replaced in the reshape.

### Original deferral rationale (preserved for context)

### Rationale for deferral

The planner's spike sketch requires patching `sv_parser_override_rust` to probe `duckdb_connect`/`duckdb_disconnect` mid-parse. That patch needs `db_handle` to be present inside `OverrideContext` (or otherwise retrievable from the existing C++ shim plumbing). On the current v0.9.0 baseline of `milestone/v0.9.1`:

- `OverrideContext` carries `catalog: CatalogReader` (which wraps `catalog_conn`, a `duckdb_connection`) and `is_file_backed: bool` — there is **no** `db_handle: duckdb_database` field today.
- `sv_register_parser_hooks` (in `cpp/src/shim.cpp`) does receive `db_handle` as its first parameter at extension-init time, but does not forward it through to `OverrideContext` — `OverrideContext` only ever sees the `duckdb_connection` that `init_extension` opened at `src/lib.rs:384`.
- Obtaining the `db_handle` from a `duckdb_connection` is not exposed by the C-API in libduckdb-sys 1.10502.0 (`duckdb_connection_get_client_context` returns a wrapped `ClientContext`, not a `duckdb_database`).

Therefore, exercising the A7 probe requires either (i) plumbing `db_handle` through the FFI surface — exactly Plan 02's job — or (ii) writing a throwaway intermediate spike that re-derives `db_handle` from `ctx.catalog.raw()` via internal C++ APIs, which would itself be a substantial scratch-branch change and would not survive past Plan 02.

The plan's `<action>` block explicitly permits this disposition: *"If A7 evidence cannot be obtained without plumbing `db_handle` (which is Plan 02's job), mark A7 as DEFERRED-to-Plan-02 and document the rationale; this is acceptable since the production work in Plan 02 will surface a deadlock immediately if A7 is wrong."*

### Conclusion: **DEFERRED-TO-PLAN-02**

A7 (parser-override re-entrancy under a nested `duckdb_connect`+`duckdb_disconnect`) will be tested empirically by Plan 02 as soon as it threads `db_handle` into `OverrideContext` and converts `rewrite_create` / `rewrite_drop_or_alter` / `rewrite_to_native_sql` to open per-call `ConnGuard`s. If the re-entrancy is unsafe (e.g., a hidden internal lock contention from `ConnectionManager::AddConnection` re-entering a lock held by `Parser::ParseQuery`), Plan 02's very first run of an existing parser-override sqllogictest will deadlock and surface the issue with a complete in-context backtrace. That is a strictly better signal than a contrived spike probe on the current baseline.

The risk of deferral is bounded:

- The standalone library evidence in RESEARCH §3.3 / §6.5 already argues `connections_lock` is per-`ConnectionManager` and does not gate the caller's existing connection's parse step (`duckdb.cpp:276187`).
- `Parser::ParseQuery` does not hold any global lock during `parser_override` invocation (no evidence to the contrary in the amalgamation).
- The empirical falsification path in Plan 02 is immediate and unmissable.

If Plan 02 observes a deadlock, it has the option of returning a `checkpoint:decision` to revisit the fix shape — e.g., move the connect to a different lifecycle point, or adopt the `plan_function` route mentioned in RESEARCH §9.1. No production code is being committed in Plan 01 based on the A7 assumption.

---

## Summary

| Spike | Outcome | Implication |
|-------|---------|-------------|
| A4 | **CONFIRMED** — `DBInstanceCache::GetInstanceInternal` busy-spin (99.4 % CPU; tight `ldr/cmn/b.ne` loop; structural frame depth matches) | RESEARCH §2 root cause stands. The fix must release the extension-held `shared_ptr<DatabaseInstance>` (i.e., not own long-lived `duckdb_connection`s). |
| A6 | **`BindInfo`-DOES-NOT-EXPOSE-db_handle** (verified across `duckdb-1.10502.0` `vtab/`, `vscalar/`, `extension.rs`, `core/`) | Plan 03 must adopt **shape (a)**: introduce `CatalogHandle { db, catalog_table_present }` as the `extra_info` payload, with `ConnGuard` opened in each bind callback. |
| A7 | **RE-ENTRANCY-UNSAFE** (falsified empirically by Plan 02 — 43/47 sqllogictests fail with `duckdb_connect failed (rc=1)` from inside `parser_override` on DuckDB 1.5.2). Original DEFERRED-TO-PLAN-02 rationale preserved below. | Per-call ConnGuard from inside `parser_override` is not viable. Plan 02 hit T-65-05 stop-and-revisit; user chose Option A (defer catalog reads to bind/plan time). Reshape pending replan. |

No production source files were modified by this task (`git diff --stat src/ cpp/` empty post-spike).
