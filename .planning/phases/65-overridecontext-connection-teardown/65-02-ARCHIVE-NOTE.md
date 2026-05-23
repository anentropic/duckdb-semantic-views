# 65-02-PLAN archived as stale B-prime

**Archived:** 2026-05-23
**Reason:** Plan 02's intent is fully superseded under the read-elimination architecture (Plans 03-06).

## Why skipped

The file `65-02-PLAN.md.archived-stale-bprime` (last updated in commit `cd8194f`, B-prime planning) carried two responsibilities:

1. **Three git reverts** of Plan-02-partial commits (`0d2c0b7`, `f9caafe`, `656bae7`) to restore the v0.9.0 OverrideContext + sv_register_parser_hooks shape.
2. **Wave 1 addition** of the `sv_register_table_function` C++ Catalog API shim infrastructure into `cpp/src/shim.cpp` + new `cpp/src/shim.hpp`.

Both responsibilities have moved:

- (1) **Plan 03** (`65-03-PLAN.md`) covers the revert via direct rewrite back to v0.9.0 shape (must_haves lock `OverrideContext { catalog: CatalogReader, is_file_backed: bool }`, `src/conn_guard.rs` deleted, `sv_register_parser_hooks(duckdb_connection catalog_conn, bool is_file_backed)` signature). Plan 03 declares `depends_on: []` and runs from the current Plan-02-partial baseline.
- (2) **Plan 04** (`65-04-PLAN.md`) owns the shim addition per locked **A2 resolution**: "introduces `sv_register_table_function(...)` from scratch (~150 LOC) following the 65-READ-PATH-SPIKE.md template; A2 resolution: NOT in HEAD". Verified at archive time: `grep sv_register_table_function cpp/src/shim.cpp` returns no matches; `cpp/src/shim.hpp` does not exist.

## Other staleness in 65-02-PLAN

- `tags:` included `bprime` (not refreshed during 2026-05-23 re-plan).
- Text references `milestone/v0.9.1` (milestone was reframed to `v0.10.0` on 2026-05-23 per `65-BPRIME-ARCHIVE-NOTE.md`).
- `wave: 1` declaration triggered a planner-index warning ("declared wave: 1 but depends_on DAG places it in wave 2").

## Resume path

`/gsd-execute-phase 65` will now correctly start at Plan 03 (the first incomplete plan after Plan 01).
