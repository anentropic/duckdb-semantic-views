# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-23)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.
**Current focus:** Phase 2 — Storage and DDL

## Current Position

Phase: 2 of 5 (Storage and DDL)
Plan: 2 of 3 in current phase
Status: In progress
Last activity: 2026-02-24 — Completed plan 02-02 (DDL Function Implementations)

Progress: [█████░░░░░] 50%

## Performance Metrics

**Velocity:**
- Total plans completed: 4
- Average duration: 5 min
- Total execution time: 27 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-scaffold | 3 | 9 min | 3 min |
| 02-storage-and-ddl | 2 | 23 min | 11.5 min |

**Recent Trend:**
- Last 5 plans: 01-03 (1 min), 01-02 (4 min), 01-01 (4 min), 02-01 (18 min), 02-02 (5 min)
- Trend: Phase 2 plan 2 complete; all four DDL functions registered in extension entrypoint

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

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
Stopped at: Completed 02-02-PLAN.md (DDL Function Implementations)
Resume file: None
