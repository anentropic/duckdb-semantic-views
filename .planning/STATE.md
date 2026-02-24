# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-23)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.
**Current focus:** Phase 1 — Scaffold

## Current Position

Phase: 1 of 5 (Scaffold)
Plan: 3 of 3 in current phase
Status: In progress
Last activity: 2026-02-24 — Completed plan 01-02 (CI Workflows and LOAD smoke test)

Progress: [███░░░░░░░] 30%

## Performance Metrics

**Velocity:**
- Total plans completed: 3
- Average duration: 3 min
- Total execution time: 9 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-scaffold | 3 | 9 min | 3 min |

**Recent Trend:**
- Last 5 plans: 01-01 (4 min), 01-03 (1 min), 01-02 (4 min)
- Trend: Phase 1 scaffold complete

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

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
Stopped at: Completed 01-01-PLAN.md (Rust Extension Scaffold)
Resume file: None
