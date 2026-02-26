---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: in_progress
last_updated: "2026-02-26T15:57:35Z"
progress:
  total_phases: 7
  completed_phases: 6
  total_plans: 17
  completed_plans: 17
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-23)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand — the extension handles expansion, DuckDB handles execution.
**Current focus:** Phase 7 verification and formal closure

## Current Position

Phase: 7 of 7 (Verification & Formal Closure)
Plan: 1 of 2 in current phase (07-01 complete)
Status: Plan 07-01 complete. TECH-DEBT.md created with full v0.1 inventory.
Last activity: 2026-02-26 — Completed plan 07-01 (TECH-DEBT.md)

Progress: [█████████░] 94% (phases 1-6 complete, phase 7 in progress)

## Performance Metrics

**Velocity:**
- Total plans completed: 13
- Average duration: 7 min
- Total execution time: 87 min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-scaffold | 3 | 9 min | 3 min |
| 02-storage-and-ddl | 4 | 35 min | 9 min |
| 03-expansion-engine | 3 | 13 min | 4 min |
| 04-query-interface | 3 | 53 min | 18 min |
| 05-hardening-and-docs | 2 | 6 min | 3 min |
| 06-tech-debt-cleanup | 1 | 3 min | 3 min |

**Recent Trend:**
- Last 5 plans: 04-03 (29 min), 05-01 (3 min), 05-02 (3 min), 06-01 (3 min), 07-01 (2 min)
- Trend: Documentation-only plans complete quickly

*Updated after each plan completion*
| Phase 06 P01 | 3min | 2 tasks | 4 files |
| Phase 07 P01 | 2min | 1 task | 1 file |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [07-01]: tech-debt-at-root: TECH-DEBT.md placed at repo root alongside MAINTAINER.md for contributor visibility (not in .planning/)
- [06-01]: infer-schema-returns-names-only: infer_schema_or_default returns Vec<String> only; types discarded at caller via _types (try_infer_schema unchanged)
- [06-01]: case-conditional-cleanup: CASE-based conditional drop in SQLLogicTest section 10 (DuckDB evaluates CASE lazily)
- [06-01]: temp-dir-pattern: all Rust tests creating temp files use std::env::temp_dir() for sandbox portability
- [05-02]: python-audience-first-tone: All Rust concepts in MAINTAINER.md explained with Python analogies as inline footnotes, not standalone sections
- [05-02]: single-source-doc: MAINTAINER.md is self-contained with no 'see also' chains for essential workflows
- [05-02]: feature-flag-explainer: Dedicated subsection explaining bundled vs extension feature split since it is the #1 source of confusion
- [05-01]: conditional-arbitrary-derive: Arbitrary derive gated behind feature flag (#[cfg_attr(feature = "arbitrary", ...)]) to avoid impacting default/extension builds
- [05-01]: fuzz-crate-default-features: fuzz crate depends on default feature (duckdb/bundled) not extension; exercises pure Rust logic (model parsing, expand())
- [05-01]: separate-corpus-job: commit-corpus CI job runs after all fuzz matrix jobs to avoid parallel push race condition
- [05-01]: corpus-via-pr: corpus updates submitted as PR (peter-evans/create-pull-request) not direct push; consistent with DuckDB version monitor pattern
- [04-03]: varchar-output-columns: all semantic_query output columns declared as VARCHAR; avoids type mismatch panics when writing string data to typed output vectors
- [04-03]: varchar-cast-wrapper: expanded SQL wrapped in SELECT CAST(...AS VARCHAR) subquery; ensures all result chunk vectors contain duckdb_string_t data for uniform reading
- [04-03]: direct-string-t-decode: read duckdb_string_t inline/pointer union directly from vector memory; avoids reliance on C API helper functions in loadable-extension stubs
- [04-03]: unqualified-join-expressions: dimension/metric expressions must use unqualified column names because CTE flattens all tables into _base namespace
- [04-03]: python-ducklake-test: DuckLake integration test uses Python script instead of SQLLogicTest; runner cannot install DuckDB extensions dynamically
- [04-02]: pub-crate-ffi-helpers: promoted execute_sql_raw and extract_list_strings to pub(crate) for reuse by explain module; avoids code duplication across query table functions
- [04-02]: collect-explain-lines-helper: extracted EXPLAIN plan collection into separate unsafe fn; keeps bind() under clippy pedantic 100-line limit
- [04-02]: graceful-explain-fallback: if EXPLAIN execution fails (tables not created), output shows '-- (not available -- {error})' instead of hard error
- [04-01]: manual-ffi-entrypoint: replaced #[duckdb_entrypoint_c_api] macro with hand-written FFI entrypoint to capture raw duckdb_database handle; enables duckdb_connect for independent query connection
- [04-01]: independent-query-connection: semantic_query uses a separate duckdb_connection via duckdb_connect; avoids lock conflicts with host during expanded SQL execution
- [04-01]: limit0-schema-inference: bind() executes expanded SQL LIMIT 0 on independent connection to discover column types; falls back to VARCHAR dims / DOUBLE metrics if inference fails
- [04-01]: varchar-string-materialization: func() reads all result values as VARCHAR via duckdb_value_varchar; DuckDB handles implicit casting to declared output types
- [04-01]: empty-request-replaces-empty-metrics: EmptyMetrics replaced with EmptyRequest — triggered when both dims and metrics empty; dimensions-only is now valid (SELECT DISTINCT)
- [03-03]: lib-crate-type-for-integration-tests: added "lib" to crate-type alongside cdylib so integration tests in tests/ can link against the crate; cdylib alone only produces a dynamic library for FFI consumers
- [03-03]: proptest-subsequence-strategy: arb_query_request() uses proptest::sample::subsequence to generate valid dimension/metric subsets from definitions; guarantees all generated requests reference valid names
- [03-03]: proptest-default-config-256-cases: default proptest config (256 cases per property) is sufficient for the dimension/metric subset combinatorial space
- [03-02]: levenshtein-threshold-3: fuzzy "did you mean" suggestions use strsim Levenshtein distance with threshold <= 3; balances helpfulness vs false positives
- [03-02]: on-clause-substring-matching: transitive join dependency detection uses substring check on ON clause; sufficient heuristic for v0.1 where users declare joins in dependency order
- [03-02]: fixed-point-join-resolution: resolve_joins uses a fixed-point loop for transitive convergence; handles arbitrary chain depth
- [03-02]: source-table-join-pruning: only joins whose table matches a source_table from requested dims/metrics are included; replaces Plan 01 "include all joins" behavior
- [03-01]: all-joins-included: Plan 01 includes all declared joins in the base CTE; join pruning based on source_table deferred to Plan 02
- [03-01]: case-insensitive-names: dimension/metric name lookup uses eq_ignore_ascii_case for DuckDB compatibility; definition name "Region" matches request "region"
- [03-01]: expressions-verbatim: dimension expr, metric expr, filter strings, and join ON clauses are emitted as raw SQL; only engine-generated identifiers (table names, aliases, CTE name) are double-quoted
- [03-01]: fuzzy-suggestions-deferred: ExpandError suggestion field is None for Plan 01; strsim-based fuzzy matching added in Plan 02
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

Last session: 2026-02-26
Stopped at: Completed 07-01-PLAN.md (TECH-DEBT.md creation)
Resume file: None
