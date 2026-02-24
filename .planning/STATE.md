# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-23)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.
**Current focus:** Phase 2 — Storage and DDL

## Current Position

Phase: 2 of 5 (Storage and DDL)
Plan: 4 of 4 in current phase
Status: Phase complete
Last activity: 2026-02-24 — Completed plan 02-04 (DDL-05 Gap Closure)

Progress: [██████░░░░] 60%

## Performance Metrics

**Velocity:**
- Total plans completed: 5
- Average duration: 5 min
- Total execution time: 32 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-scaffold | 3 | 9 min | 3 min |
| 02-storage-and-ddl | 4 | 35 min | 9 min |

**Recent Trend:**
- Last 5 plans: 01-01 (4 min), 02-01 (18 min), 02-02 (5 min), 02-03 (7 min), 02-04 (5 min)
- Trend: Phase 2 fully complete with DDL-05 gap closure; sidecar persistence enables cross-restart survival

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [02-04]: sidecar-persistence: invoke cannot execute DuckDB SQL (execution locks deadlock); sidecar file (<db>.semantic_views) written with plain fs I/O bridges the gap; init_catalog reads sidecar on next load and syncs into DuckDB table
- [02-04]: pragma-database-list-path: entrypoint queries PRAGMA database_list to resolve the host DB file path; takes first row with non-empty file (not filtered by name='main' because Python DuckDB names DBs by filename stem)
- [02-04]: atomic-rename-write: sidecar writes use write-to-tmp-then-rename pattern for POSIX atomicity
- [02-03]: init_catalog-before-write: any code path that opens a fresh Connection::open() must call init_catalog() before catalog writes — the fresh connection starts with no schema/table
- [02-03]: HashMap is truth for catalog_delete: removed rows_affected == 0 check; ephemeral :memory: DB DELETE always returns 0 rows; contains_key() guard is authoritative
- [02-03]: serde_json serializes JSON object keys alphabetically (expr before name) — integration test expected values must match this order
- [02-03]: test-sql recipe delegates to make test_debug (Python duckdb_sqllogictest runner); no standalone DuckDB CLI available locally; SQLLogicTest format in test/sql/ picked up automatically
- [02-02]: Connection::path() not available in duckdb-rs 1.4.4 — scalar invoke opens Connection::open(":memory:") as sentinel; catalog writes from invoke go to a separate ephemeral DB; HashMap state is always correct; integration tests must verify via list/describe, not semantic_layer._definitions
- [02-02]: DDL module gated behind #[cfg(feature = "extension")] — duckdb::vscalar and duckdb::vtab not available under bundled default feature; entire src/ddl/ excluded from cargo test compilation
- [02-02]: register_table_function_with_extra_info requires two type params <T, E> — use ::<VTabType, _> turbofish; Rust infers E from the extra_info value
- [02-02]: #[allow(clippy::needless_pass_by_value)] on extension_entrypoint — duckdb_entrypoint_c_api macro requires con: Connection by value for FFI bridge ownership transfer
- [02-01]: Cargo feature split for testable DuckDB extensions: `default=["duckdb/bundled"]` enables `cargo test` with Connection::open_in_memory(); `extension=["duckdb/loadable-extension","duckdb/vscalar"]` used by Makefile for cdylib builds with --no-default-features --features extension
- [02-01]: duckdb/loadable-extension replaces all C API calls with function-pointer stubs initialized by DuckDB at load time — standalone test binaries cannot use these stubs; bundled feature resolves this without workspace restructuring
- [02-01]: Write-catalog-first pattern: catalog_insert/catalog_delete write to semantic_layer._definitions before updating HashMap; error propagates via ? preventing HashMap/catalog drift
- [02-01]: serde deny_unknown_fields on SemanticViewDefinition: unknown JSON fields return parse error immediately at define time
- [02-01]: #[allow(clippy::unnecessary_wraps)] required on extension_entrypoint — duckdb_entrypoint_c_api macro calls it via ? requiring Result return type
- [Init]: v0.1 uses function-based DDL (`define_semantic_view`, `drop_semantic_view`) not native `CREATE SEMANTIC VIEW` — parser hooks not available in DuckDB C API from Rust
- [Init]: Expansion-only scope for v0.1; no pre-aggregation; DuckDB is the execution engine
- [Init]: SQL expressions stored as opaque strings in the definition JSON; DuckDB validates them at execution time (avoids sqlparser-rs dialect gap)
- [Init]: Persistence via a plain DuckDB table (`semantic_layer._definitions`) in the user's `.duckdb` file; in-memory HashMap reconstructed from it at load time
- [01-03]: Use steps.build.outcome (not steps.build.conclusion) in version monitor — conclusion is always success when continue-on-error: true; outcome reflects actual result
- [01-03]: Breakage PR tags @copilot for automated fix; version-bump PR does not — signals human/bot attention only when build is broken
- [01-02]: Arch names for exclude_archs verified from extension-ci-tools/config/distribution_matrix.json — plan examples were incomplete; actual matrix has linux_arm64_musl, windows_arm64, windows_amd64_mingw not listed in RESEARCH.md
- [01-02]: PullRequestCI excludes all non-linux_amd64 targets; MainDistributionPipeline excludes only musl, windows variants (arm64/mingw), and WASM — keeping 5 target platforms
- [01-01]: duckdb_entrypoint_c_api is re-exported from the duckdb crate — no separate duckdb_loadable_macros dep needed in Cargo.toml; accessed as duckdb::duckdb_entrypoint_c_api
- [01-01]: workspace.lints.clippy pedantic requires { level = "deny", priority = -1 } for individual lint overrides to take precedence; lint_groups_priority clippy lint enforces this pattern

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 1 risk]: `duckdb-rs` vtab/replacement-scan API coverage must be verified by prototype in Phase 1. If gaps exist, raw `libduckdb-sys` FFI wrappers must be scoped before Phase 3 planning.
- [Phase 4 risk]: Re-entrant query execution in the vtab bind phase may not be allowed by DuckDB. Output schema must be inferred from definition metadata if SQL re-execution is blocked. Prototype needed early in Phase 4.

## Session Continuity

Last session: 2026-02-24
Stopped at: Completed 02-04-PLAN.md (DDL-05 Gap Closure) — Phase 2 fully complete with all 5 DDL requirements met
Resume file: None
