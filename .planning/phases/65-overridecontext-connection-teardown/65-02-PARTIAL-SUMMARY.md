---
phase: 65-overridecontext-connection-teardown
plan: 02
status: partial
subsystem: parser_override
tags: [duckdb, rust, ffi, parser_override, lifecycle, a7-falsified]

# Dependency graph
requires:
  - phase: 65-overridecontext-connection-teardown
    plan: 01
    provides: "ConnGuard RAII type + watchdog tests + A4/A6 spike outcomes"
provides:
  - "OverrideContext { db_handle, catalog_table_present, is_file_backed } shape — db_handle plumbing through Rust+C++ FFI"
  - "sv_register_parser_hooks signature: (duckdb_database, bool, bool) — no longer takes duckdb_connection"
  - "Empirical A7 falsification evidence (43/47 sqllogictests fail with duckdb_connect rc=1 from inside parser_override on DuckDB 1.5.2)"
deferred:
  - "Per-call ConnGuard inside rewrite_* sites is RE-ENTRANCY-UNSAFE → catalog reads must move OUT of parser_override (user chose Option A: defer to bind/plan time)"
  - "Plan 03 (query_conn / H2 removal) — blocked on the bind/plan-time reshape because it shares the read-path architecture"
  - "Plan 04 (LIFE-04 ledger close + B13/B14 guards) — blocked on Plans 02A/03 completing"
affects: [65-02A, 65-03, 65-04, 66-overridecontext-and-adbc]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "OverrideContext now carries a non-owning duckdb_database pointer (was: a CatalogReader wrapping a duckdb_connection)"
    - "unsafe impl Send/Sync for OverrideContext (preserves thread-safety contract that the previous CatalogReader field provided via its auto-traits)"
    - "PHASE-65-GUARD marker comment inside OverrideContext struct body for Plan 04 B13 structural test"

key-files:
  created:
    - ".planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log"
  modified:
    - "src/parse.rs (+182 / -114) — OverrideContext field swap, Drop removed, 4× ConnGuard::open call sites (NOW KNOWN-BROKEN at runtime due to A7)"
    - "cpp/src/shim.cpp (+38 / -35) — sv_register_parser_hooks signature update, INTENTIONAL LEAK comment removed"
    - "cpp/src/shim.hpp (+3 / -3) — header declaration matches new signature"
    - "src/lib.rs (+10 / -10) — init_extension calls sv_register_parser_hooks(db_handle, catalog_table_present, is_file_backed); catalog_conn local retained pending Plan 03"

# Verification status
verification:
  cargo_build: "PASS"
  cargo_build_extension: "PASS"
  cargo_test_lib: "PASS (839 tests)"
  just_build: "PASS (extension binary produced)"
  just_test_sql: "FAIL — 43/47 fail (RUNTIME REGRESSION introduced by per-call ConnGuard from inside parser_override). Evidence: 65-02-A7-test-sql-evidence.log"
  baseline_in_process_tests: "Not re-run — they would also fail because the read path is now broken upstream of the lifecycle question they test"
---

## Plan 02 — PARTIAL (A7 stop-and-revisit triggered)

### What landed

| Commit  | Subject                                                                | Files                              |
|---------|------------------------------------------------------------------------|------------------------------------|
| `0d2c0b7` | feat(65-02): swap OverrideContext to db_handle + per-call ConnGuard    | `src/parse.rs`                     |
| `f9caafe` | feat(65-02): update sv_register_parser_hooks signature (db_handle + flags) | `cpp/src/shim.cpp`, `cpp/src/shim.hpp`, `src/lib.rs` |
| `656bae7` | docs(65-02): preserve A7 RE-ENTRANCY-UNSAFE empirical evidence         | `65-02-A7-test-sql-evidence.log`   |

Structural acceptance criteria all met:

- `OverrideContext { db_handle: duckdb_database, catalog_table_present: bool, is_file_backed: bool }` — no `catalog: CatalogReader`, no `conn: duckdb_connection`, no custom `Drop`.
- PHASE-65-GUARD marker comment present as first line inside the struct body (consumed by Plan 04 B13 structural test).
- `unsafe impl Send for OverrideContext {}` + `unsafe impl Sync for OverrideContext {}` with SAFETY comment cribbed from `src/catalog.rs:106-112`.
- `sv_register_parser_hooks(duckdb_database, bool, bool)` — C++ declaration, C++ definition, Rust `extern "C"`, and `init_extension` call site all match.
- No `INTENTIONAL LEAK` / `Phase 62 Q2` comments survive in `src/parse.rs`, `cpp/src/shim.cpp`, or `cpp/src/shim.hpp`.
- ≥4 `ConnGuard::open` call sites in `src/parse.rs` (sites: `emit_native_create_sql`, `rewrite_drop_or_alter`, `rewrite_yaml_file_create` enrichment, second enrichment site).
- `cargo build`, `cargo build --features extension --no-default-features`, `cargo test --lib`, `just build` all PASS.

### What broke (the stop-and-revisit)

`just test-sql` regressed from `47/47 PASS` (baseline) to `4/47 PASS` (post-refactor). Every test that reaches a `ConnGuard::open` call from inside `parser_override` fails with:

```
Parser Error: catalog connection failed: duckdb_connect failed (rc=1)
```

The 4 passing tests (`error_caret_create.test`, `error_caret_drop.test`, `error_caret_multiline.test`, `error_caret_unicode.test`) exercise near-miss / invalid-syntax paths that never reach `ConnGuard::open`. This precisely scopes the failure to opening a fresh `duckdb_connection` from inside `parser_override` on the bundled DuckDB 1.5.2.

This **falsifies** RESEARCH §3.3 / §6.5's standalone-library argument that `connections_lock` is per-`ConnectionManager` and does not gate the caller's existing connection's parse step. On DuckDB 1.5.2 in the `--features extension` build, `duckdb_connect` from the parse thread returns rc=1 (the value of the inner `connections_lock` / `DBInstanceCache` interaction was investigated only to the extent of observing the rc=1 — deeper instrumentation deferred).

Plan 02's threat model anticipated this exact outcome (T-65-05, disposition "mitigate via first build+test cycle surfaces the deadlock and triggers stop-and-revisit"). The executor returned a `checkpoint:decision` with four options (A: defer catalog reads to bind/plan time; C: re-cache + deterministic teardown; D: documented limitation; revert). The user chose **Option A**.

### Why the commits stayed in place

The `db_handle` plumbing through Rust + C++ FFI, the `sv_register_parser_hooks` signature update, and the removal of `INTENTIONAL LEAK` comments are **reusable foundation** for the Option A reshape. Whatever the bind/plan-time architecture looks like, `OverrideContext` will continue to need `db_handle` (it is the only way the parse-side caret-rendering and the new bind/plan-time catalog reads can converge on the same data source). Reverting would discard ~150 LOC of cleanly-tested FFI work and re-introduce the `Phase 62 Q2 INTENTIONAL LEAK` rationale.

The 4× `ConnGuard::open` call sites inside `parse.rs::rewrite_*` are the known-broken surface. They MUST be removed (or replaced) in the reshape; until then `just test-sql` is red. The replanning work below owns this rollback / replacement.

### What needs to happen next (replan input)

The reshape should produce a new **Plan 02A** (and likely re-scope Plans 03/04). Inputs for the planner:

1. **Move catalog reads OUT of `parser_override`**. The `parser_override` callback returns a validated parse tree (or an error) but does NOT touch the catalog. CONTEXT.md D-07.1 phrasing: "don't cache a connection — open/close per DDL invocation".

2. **Use the `parse_function` / `plan_function` route**. The Phase 62 architecture introduced `parse_function` as the error-reporting hook. `parse_function` runs OUTSIDE the parser lock and has access to a `ClientContext&` (which can be converted to `duckdb_connection` for `CatalogReader` use). Promoting `parse_function` from "error reporting only" to "success-path catalog reads + native SQL emission" is the structural reshape Option A names.

3. **`SemanticViewParseData` becomes the carrier**. The parser_override callback stuffs the raw query (or a partially-validated form) into `SemanticViewParseData`; `parse_function` reads it back, opens its own connection from `ClientContext`, performs the existence / enrichment / native-SQL emission, and hands the rewritten query to the planner. The exact shape of `SemanticViewParseData` needs to be designed.

4. **Plans 03 and 04 likely shift**. Plan 03's `query_conn` (H2) removal partly overlaps with the read-path rewiring; if the bind callbacks for the 14 read-side table functions + 2 scalars can also use `ClientContext`-derived connections (Plan 01 Spike A6 said `BindInfo` does NOT expose `duckdb_database` — but `TableFunctionInfo` and `FunctionInfo` might expose a `ClientContext` getter; this needs verification). Plan 04 stays roughly the same shape (LIFE-04 ledger + structural guards) but the B13 grep targets change.

5. **Phase 62 caret rendering stays intact**. The reshape MUST NOT regress the 4 caret tests that currently pass. The `parser_override` -> error-path branch (where the parse fails) is the caret rendering surface; it does not touch the catalog and is unaffected by the reshape.

### Evidence pointers

- Full sqllogictest failure log: `.planning/phases/65-overridecontext-connection-teardown/65-02-A7-test-sql-evidence.log`
- Updated SPIKES A7 entry: `.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md#A7`
- Structural commits: `0d2c0b7`, `f9caafe` on `milestone/v0.9.1`

## Self-Check: PARTIAL (stop-and-revisit triggered per T-65-05; awaiting replan via /gsd:plan-phase 65)
