---
gsd_state_version: 1.0
milestone: v0.9.1
milestone_name: Connection-Lifecycle & Catalog-Context Fixes
status: executing
stopped_at: "Completed 65-01-PLAN.md (scaffolding + Wave-0 spikes). Next: /gsd:execute-plan 65 02."
last_updated: "2026-05-22T15:09:13.612Z"
last_activity: 2026-05-22 -- Phase 65 planning complete
progress:
  total_phases: 2
  completed_phases: 0
  total_plans: 4
  completed_plans: 2
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-21)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand
**Current focus:** Phase 65 — overridecontext-connection-teardown

## Current Position

Phase: 65 (overridecontext-connection-teardown) — EXECUTING
Plan: 2 of 4
Status: Ready to execute
Progress: [███░░░░░░░] 25%
Last activity: 2026-05-22 -- Phase 65 planning complete

## Performance Metrics

**Velocity:**

- Total plans completed: 19 (v0.7.0) + 4 (v0.8.0 phases 58–61, retroactive)
- Average duration: --
- Total execution time: 0 hours

## Accumulated Context

### Roadmap Evolution

- Phase 64 added: Fix CREATE SEMANTIC VIEW quoted identifier handling (downstream bug; quoted FQN stored verbatim, lookup by short name fails, expansion re-quotes producing triple quotes). Folded into v0.9.0 — milestone reopened pre-tag since maintainer squash-merge had not yet happened.
- v0.9.1 opened 2026-05-21: two-phase patch milestone for connection-lifecycle / catalog-context regressions. Phase 65 covers in-process RW→RO reopen hang (LIFE-01..04, from Phase 63 deferred-items). Phase 66 covers cross-connection expansion qualification across all paths (EXPAND-CTX-01..03, from `_notes/error_with_adbc.md`) plus release prep (REL-01, REL-02) as its closing plan, matching the v0.9.0 Phase 64 pattern.

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [v0.7.0 roadmap]: Two independent tracks -- YAML (51-53) and Materialization (54-55), converging at YAML Export (56) and Introspection (57)
- [v0.7.0 roadmap]: serde_yaml_ng 0.10 selected as YAML dependency (serde_yaml archived, serde_yml has RUSTSEC advisory)
- [v0.7.0 roadmap]: Semi-additive and window metrics unconditionally excluded from materialization routing
- [v0.7.0 roadmap]: Re-aggregation for subset matches deferred to v2 (MAT-F01) -- exact match only in v0.7.0
- [v0.7.0 roadmap]: YAML export (Phase 56) placed after materialization model (Phase 54) so materializations appear in YAML output
- [Phase 51]: yaml_serde 0.10 added as unconditional dependency (not feature-gated), matching serde_json treatment
- [Phase 51]: PartialEq derived on all 10 model structs -- all fields are PartialEq-safe (no f32/f64)
- [Phase 51]: YAML_SIZE_CAP (1 MiB) is sanity guard, not security boundary -- trust assumption documented in code
- [Phase 55]: Routing placed after step 3 (name resolution) in expand() with internal semi-additive/window exclusion checks
- [Phase 55]: HashSet exact-match with to_ascii_lowercase() for case-insensitive materialization matching
- [Phase 56]: Field stripping via clone + clear + skip_serializing_if for YAML export (not a separate export struct)
- [Phase 56]: Bare name extraction via rsplit('.') for FQN support in READ_YAML_FROM_SEMANTIC_VIEW
- [Phase 57]: find_routing_materialization_name duplicates resolution logic rather than changing expand() return type
- [Phase 57]: Feature-gated re-export with #[allow(dead_code)] for extension-only cross-module access
- [Phase 62]: Phase 62 Plan 01: pre-stage all behavioural test slots (B1-B19) as halt no-ops + skip-guarded staged tests so suite stays green between waves. Pin ParserExtensionParseResult layout via static_assert before Plans 02-03 production changes land.
- [Phase 62]: Phase 62 Plan 02: Drop for OverrideContext leaks the inner duckdb_connection by design (Q2 destruction-order: ~DBConfig fires AFTER ~DatabaseInstance resets connection_manager, so calling duckdb_disconnect would UAF). Bounded leak — one Connection per DB ever opened, ~few KB each. Matches v0.8.0 baseline.
- [Phase 62]: Phase 62 Plan 03: parse_function reintroduced as error-reporting layer. parser_override owns success path (transactional rewrite + re-parse); error branches return rc=2 to defer to default parser, which fails on the unrecognised prefix and triggers sv_parse_stub which returns DISPLAY_EXTENSION_ERROR with error_location for caret rendering. sv_parse_function_rust uses rewrite_to_native_sql (catalog-aware) when ctx_ptr is non-null so DROP/ALTER catalog errors are reproduced with caret. sql_throwing helper deleted; write_error_to_buffer is now live. Resolves TECH-DEBT 22.
- [Phase 62]: TECH-DEBT 20 (bounded LRU eviction) and 22 (FALLBACK_OVERRIDE drops DISPLAY_EXTENSION_ERROR) marked resolved; caret rendering restored via parse_function
- [Phase 63]: Phase 63 Plan 01: pass catalog_table_present=true from sv_make_override_context — keeps src/parse.rs UNCHANGED in spirit; routes DDL writes through DuckDB native read-only error (RO-05). Fresh-RO DDL pre-checks may surface catalog errors instead of read-only — covered by RO-05 'or the closest equivalent' wording per RESEARCH §3 Q5.
- [Phase 63]: Phase 63 Plan 02: added readonly_load.test entry to test/sql/TEST_LIST (Rule 3 — required for fixture to actually run; runner gates on TEST_LIST membership, not directory scan). Skipped just ci per plan §Task 4 (docs-check defers to Plan 04 after Plan 03 lands).
- [Phase 63]: Phase 63 Plan 03: bootstrap-in-subprocess pattern adopted in examples/readonly_load.py to sidestep Phase 62 OverrideContext in-process RW->RO hang (verified by stuck CPU spin in first draft). Mirrors test_readonly_load.py::bootstrap_in_subprocess. Reflects real production split between bootstrap (build/CI) and read-only query (analytics worker).
- [Phase 63]: Phase 63 Plan 04: version bump to 0.9.0 (Cargo.toml + description.yml); description.yml repo.ref intentionally unchanged (just release on main owns it post-tag). Tag/squash-merge OUT OF SCOPE — flagged for maintainer in 63-04-SUMMARY.
- [Phase 64]: Phase 64 Plan 01: ident helper lives in own leaf module (src/ident.rs) — lets src/expand/resolution.rs::quote_table_ref depend on it without parse.rs → expand reverse direction; String error type matches existing convention; empty quoted "" rejected (Snowflake-aligned); find_identifier_end returns input.len() on unterminated quote so callers don't need Option/Result wrap
- [Phase 64]: Defensive normalize_view_name shadow at emit_native_create_sql entry is UNCONDITIONAL — hardens catalog boundary against future regressions
- [Phase 64]: ALTER RENAME normalises BOTH source-name and RENAME TO target-name slots; without this the target slot stored the raw quoted FQN
- [Phase 64]: Use sqllogictest block-form statement error (---- separator + substring), not inline regex — runner does not support inline form
- [Phase 64]: Tracked fuzz seeds live in fuzz/seeds/<target>/ (gitignore only excludes fuzz/corpus/)
- [Phase ?]: [Phase 65 P01]: Spike A4 CONFIRMED — DBInstanceCache::GetInstanceInternal busy-spin diagnosis stands (verbatim lldb backtrace captured at 65-01-SPIKES.md)
- [Phase ?]: [Phase 65 P01]: Spike A6 — BindInfo does NOT expose duckdb_database in duckdb-rs 1.10502.0 → Plan 03 must adopt shape (a) CatalogHandle threaded via extra_info
- [Phase ?]: [Phase 65 P01]: Spike A7 DEFERRED-TO-PLAN-02 (acceptable per plan guidance); Plan 02 first parser_override sqllogictest will deadlock if re-entrancy unsafe — strictly better falsification than contrived spike on baseline
- [Phase ?]: [Phase 65 P01]: ConnGuard module declared without #[cfg(feature=extension)] gate so null-drop test runs under default bundled feature; inner FFI body remains gated
- [Phase ?]: [Phase 65 P01]: ConnGuard is Send but deliberately NOT Sync (per-scope ownership); not Clone/Copy

### Pending Todos

- [ ] Investigate WASM build strategy -- `.planning/todos/pending/2026-03-19-investigate-wasm-build-strategy.md`
- [ ] Explore dbt semantic layer integration -- `.planning/todos/pending/2026-03-19-explore-dbt-semantic-layer-integration-via-duckdb.md`
- [ ] Pre-aggregation materializations -- `.planning/todos/pending/2026-03-19-pre-aggregation-materializations-with-query-driven-suggestions.md`
- [ ] Test hardening — large-schema stress and concurrent access tests -- `.planning/todos/pending/2026-04-04-test-hardening-stress-and-concurrency.md`
- [ ] Remove obsolete pre-0.5.5 backwards-compatibility shims -- `.planning/todos/pending/2026-05-15-remove-obsolete-pre-0-5-5-backwards-compatibility-shims.md`

### Blockers/Concerns

- serde_yaml_ng anchor bomb handling needs verification (may need manual size cap before parse)
- Dollar-quote behavior in parser hook needs integration test (parser hook fires before DuckDB parser)
- Materialization table existence: define-time vs query-time validation TBD

### Quick Tasks Completed

| # | Description | Date | Commit |
|---|-------------|------|--------|
| 260318-fzu | Remove HIERARCHIES syntax | 2026-03-18 | 72fb69d |
| 260320-ekj | Fix Windows CI per-process sqllogictest | 2026-03-20 | fc8d582 |
| 260321-i40 | Custom Pygments lexer for docs | 2026-03-21 | fb672de |
| 260322-1zx | Make PRIMARY KEY optional via catalog lookup | 2026-03-22 | d09e4cc |
| 260322-s2y | LIKE/STARTS WITH/LIMIT on SHOW VIEWS | 2026-03-22 | 285c3bc |
| 260329-frb | Sync DuckDBVersionMonitor | 2026-03-29 | eef265b |
| 260331-ta2 | Release recipe for CE registry | 2026-03-31 | 0390bab |
| 260412-v5h | Generate complete CHANGELOG.md | 2026-04-12 | d42d240 |
| 260430-vdz | Fix parser hook to skip leading SQL comments (-- and /* */) before prefix matching | 2026-04-30 | edf5196 |
| Phase 51 P01 | 20min | 2 tasks | 6 files |
| Phase 55 P01 | 18min | 2 tasks | 6 files |
| Phase 56 P01 | 25min | 2 tasks | 8 files |
| Phase 57 P01 | 95min | 3 tasks | 11 files |
| Phase 62 P01 | 30 min | 3 tasks | 13 files |
| Phase 62 P02 | 25min | 3 tasks | 3 files |
| Phase 62 P03 | 18min | 3 tasks | 2 files |
| Phase 62 P04 | 30m | 4 tasks | 13 files |
| Phase 63 P01 | 25min | 3 tasks | 4 files |
| Phase 63 P02 | 10min | 4 tasks | 4 files |
| Phase 63 P03 | 25min | 6 tasks | 7 files |
| Phase 63 P04 | 16min | 3 tasks | 4 files |
| Phase 64 P01 | 4m | 2 tasks | 2 files |
| Phase 64 P02 | 7 | 2 tasks | 2 files |
| Phase 64 P03 | 8 | 2 tasks | 1 files |
| Phase 64 P04 | 6 | 3 tasks | 12 files |
| Phase 65 P01 | 31min | 3 tasks | 5 files |

## Session Continuity

Last session: 2026-05-21T17:27:55.171Z
Stopped at: Completed 65-01-PLAN.md (scaffolding + Wave-0 spikes). Next: /gsd:execute-plan 65 02.
Resume file: None
