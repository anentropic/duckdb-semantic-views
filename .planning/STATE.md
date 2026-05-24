---
gsd_state_version: 1.0
milestone: v0.10.0
milestone_name: Connection-Lifecycle & Catalog-Context Fixes
status: executing
stopped_at: Phase 65 Plan 05 complete (read-path migration wave; all 17 read-side functions on C++ Catalog API with per-call Connection bind; H2 query_conn DELETED; 17 legacy VTab/VScalar carcasses purged; LIFE-02 satisfied; LIFE-01 watchdog tests still RED pending Plan 06 H1 retirement)
last_updated: "2026-05-24T18:00:00.000Z"
last_activity: 2026-05-24 -- Plan 65-05 complete; 53/53 just test-sql; 843/843 cargo test --lib; 6/6 ADBC; 3/3 multi-DB; new test_concurrent_reads_per_call_conn.py PASS (80 reads in 0.02s); 5/8 test_readonly_load.py watchdog still RED (Plan 06 H1 retirement)
progress:
  total_phases: 2
  completed_phases: 0
  total_plans: 6
  completed_plans: 5
  percent: 83
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-21)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand
**Current focus:** Phase 65 — overridecontext-connection-teardown

## Current Position

Phase: 65 (overridecontext-connection-teardown) — EXECUTING
Plan: 5 of 6 complete (Plan 06 next)
Plans landed: 65-01 (ConnGuard + watchdog tests), 65-02 (sv_register_table_function C++ Catalog API shim, partial — reverted to v0.9.0 OverrideContext shape by Plan 03), 65-03 (parser_override slimming wave; conn_guard deleted; resolve_pk_from_catalog deleted; metadata-via-SQL via json_merge_patch on caller's connection), 65-04 (ALTER + CREATE FROM YAML FILE architecture wave; sv_register_table_function introduced from scratch ~250 LOC C++; __sv_compute_create_from_yaml helper TF with per-call Connection(*context.db) read of the YAML file; pure-SQL json_merge_patch UPDATE for ALTER SET/UNSET COMMENT; sv_compute_create_from_yaml_rust FFI bridge with catch_unwind + sv_free_buffer ownership), 65-05 (read-path migration wave; all 17 read-side functions on C++ Catalog API with per-call Connection(*context.db) bind; H2 query_conn allocation DELETED from init_extension; 17 legacy duckdb-rs VTab/VScalar struct + impl blocks purged atomically ~2,632 LOC across 13 files; src/type_cache.rs unbounded HashMap cache landed unused as deferred optimisation; sv_logical_type_from_c_type_id bridges C-API ↔ C++ enum-value mismatch; new test_concurrent_reads_per_call_conn.py PASSES 80 reads in 0.02s; LIFE-02 satisfied end-to-end; LIFE-01 watchdog tests still RED 5/8 pending Plan 06 H1 retirement)
Next plan: 65-06 lifecycle close-out (retire H1 catalog_conn at src/lib.rs:441; add structural Rust guard test asserting init_extension allocates no long-lived duckdb_connection; verify 5 currently-RED watchdog tests flip green; close LIFE-01 + LIFE-04 ledgers; if any watchdog test stays red after H1 retirement, file as Phase 67 follow-up per D-22)
Last activity: 2026-05-24 -- Plan 65-05 complete

## Performance Metrics

**Velocity:**

- Total plans completed: 19 (v0.7.0) + 4 (v0.8.0 phases 58–61, retroactive)
- Average duration: --
- Total execution time: 0 hours

## Accumulated Context

### Roadmap Evolution

- Phase 64 added: Fix CREATE SEMANTIC VIEW quoted identifier handling (downstream bug; quoted FQN stored verbatim, lookup by short name fails, expansion re-quotes producing triple quotes). Folded into v0.9.0 — milestone reopened pre-tag since maintainer squash-merge had not yet happened.
- v0.9.1 opened 2026-05-21 as a two-phase patch milestone; reframed 2026-05-23 to v0.10.0 after the B-prime architecture for Phase 65 was empirically eliminated (EXEC-TIME-RC1 spike). New architectural premise: preserve `parser_override` (only DuckDB v1.5.2 mechanism that delivers transactional DDL), eliminate the catalog reads that drive the need for a connection inside parser_override (drop PK auto-inference, move metadata to SQL expressions in INSERT, fold existence checks into ON CONFLICT, defer type inference to read-side bind callbacks), and use the rewrite-to-UPDATE-with-TF-subquery pattern (ALTER-RC0) for ALTER and CREATE FROM YAML FILE. Read-path callbacks migrate to C++ Catalog API registration (READ-BIND-RC0) so they gain ClientContext for per-call Connection. Both long-lived connections retire. Phase 66 scope re-evaluation pending Phase 65 re-plan (the H2 catalog-search-path divergence may dissolve once query_conn is retired). See `.planning/phases/65-overridecontext-connection-teardown/65-BPRIME-ARCHIVE-NOTE.md` for the full pivot rationale.

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
- [Phase ?]: [Phase 65 P02 replanned]: A2 spike returned A2-DEADLOCK — context.Query from inside sv_plan_function self-deadlocks on ClientContext::context_lock (lldb backtrace at 65-02-SPIKES.md)
- [Phase ?]: [Phase 65 P02 replanned]: A6-bind spike returned BIND-THREAD-RC1 — duckdb_connect from ListSemanticViewsVTab::bind also returns rc=1, generalising D-10 to the bind thread; Plan 03's shape (a) is empirically invalidated
- [Phase ?]: [Phase 65 P02 replanned]: HALTED at Task 1 checkpoint:decision per USER_HARD_CONSTRAINT (saved as feedback-transactional-ddl-non-negotiable) — A1/A3 forbidden (regress transactional DDL); only escalate is the live option
- [Phase 65 P03]: parser_override slimming complete — OverrideContext reverted to v0.9.0 shape; conn_guard.rs deleted; resolve_pk_from_catalog deleted; metadata capture moved to SQL via json_merge_patch on caller's connection; D-06 hard error for FK→PK-less target with actionable v0.10.0 CHANGELOG message
- [Phase 65 P03]: D-06 check extended to cover BOTH implicit REFERENCES (empty ref_columns) AND explicit REFERENCES(cols) — superset of plan's literal check; CARD-03 still fires for column-mismatch failures
- [Phase 65 P03]: phase29/phase30/phase39 sqllogictest FACT DATA_TYPE expectations updated to (empty); Plan 05's read-side bind probe will restore populated types and tests will need re-update
- [Phase 65 P03]: H1 catalog_conn at src/lib.rs:386-410 still allocated but unused by parser_override CREATE path; Plan 06 retires the allocation
- [Phase 65 P03]: D-21 transactional invariant intact — test_adbc_transactions.py 6/6 PASS; D-03 watchdog tests still TimeoutError (expected per 65-01-SUMMARY — flip green at Plan 06 only)
- [Phase 65 P04]: A1 resolved empirically — DuckDB v1.5.2 json_merge_patch honors RFC-7396 null-as-delete (Wave 0 sqllogictest spike). ALTER UNSET COMMENT therefore uses constant patch literal `{"comment":null}` with no helper TF
- [Phase 65 P04]: A2 honored — sv_register_table_function introduced from scratch (NOT a revert of a partial Plan 02 commit; that commit was self-reverted at end of spike). ~250 LOC new C++ in shim.cpp + 71-line new shim.hpp; within RESEARCH §5.4 budget
- [Phase 65 P04]: A7 honored — only the 3 ALTER variants present in HEAD (RENAME TO, SET COMMENT, UNSET COMMENT) were migrated; the 8 enumerated additional variants are NOT implemented (Snowflake non-features)
- [Phase 65 P04]: D-09 superseded — json_set / json_remove are NOT in DuckDB v1.5.2; use json_merge_patch instead. JSON patch construction uses serde_json::to_string for internal-quote escaping
- [Phase 65 P04]: __sv_compute_create_from_yaml helper TF returns metadata-less JSON; outer INSERT wraps with json_merge_patch + json_object('created_on',..., 'database_name',..., 'schema_name',...) on caller's conn so D-21 transactional contract preserved (matches Plan 03 inline CREATE byte-for-byte)
- [Phase 65 P04]: rewrite_yaml_file_create no longer reads files in Rust; the file read happens inside the helper TF's bind callback via Connection probe(*context.db) + read_text() with the path escaped before embedding
- [Phase 65 P04]: parser_override has ZERO remaining OverrideContext-catalog consumers (rewrite_drop / rewrite_alter_rename / rewrite_alter_comment / emit_native_create_sql all retain ctx.catalog.exists for "does not exist" wording -- Plan 06 retires); H1 catalog_conn at src/lib.rs:386-410 is truly unused by every parser_override path after Plan 04
- [Phase 65 P04]: D-21 verified end-to-end: test_adbc_transactions.py 6/6 PASS, test_create_from_yaml_v010.py T7 BEGIN+CREATE+ROLLBACK leaves _definitions empty, 65_alter_comment_merge_patch.test B5 BEGIN+ALTER+ROLLBACK restores pre-tx comment
- [Phase 65 P04]: Cross-database ALTER and CREATE FROM YAML FILE (ATTACH 'db2'; ALTER db2.v) is out-of-scope for v0.10.0 — the v0.9.0 extension only initializes semantic_layer._definitions on the LOAD database; Phase 66 follow-up territory
- [Phase 65 P05]: Bridge mechanism is reinterpret_cast<duckdb_connection>(Connection*) of a stack-allocated `Connection probe(*context.db)` — same cast `duckdb_connect` itself does (duckdb.cpp:266432-266447). BORROW contract: Rust dispatcher never calls duckdb_disconnect; C++ scope ~Connection() handles teardown. Empirically validated by Wave 0 spike (commit 2db2b9b), reused identically across all 17 migrations.
- [Phase 65 P05]: Wave 6 streaming model uses C++ MaterializedQueryResult inside SemanticViewGlobalState — ColumnDataCollection owns blocks independently of the producing Connection (per duckdb.hpp:18801-18813), so the per-call init_global Connection drops safely before any exec call.
- [Phase 65 P05]: TWO per-call Connections per semantic_view(...) invocation (bind + init_global) — both drop before any exec call; no shared mutable state contention; verified by test_concurrent_reads_per_call_conn.py (80 reads in 0.02s under 8 threads).
- [Phase 65 P05]: Named LIST(VARCHAR) parameter registrations (Wave 5 + Wave 6) require hand-built TableFunction construction because the generic sv_register_table_function shim doesn't accept named_parameters spec — TECH-DEBT 1 (v0.10.1 refactor opportunity), non-blocking.
- [Phase 65 P05]: Type cache (src/type_cache.rs) introduced but NOT consumed by migrated dispatchers — LIMIT-0 probe is sub-millisecond on existing test surface; module + unit tests stay in tree for telemetry-driven future adoption. Deferred optimisation, not TECH-DEBT.
- [Phase 65 P05]: C-API ↔ C++ enum-value mismatch caught and resolved via sv_logical_type_from_c_type_id (e.g., DECIMAL is 19 vs 21; LIST is 24 vs 101) — would have silently mis-typed every column. Highest-impact Batch 2 discovery. Single source of truth for the conversion (mirrors duckdb-rs's LogicalTypeId::from(u32)).
- [Phase 65 P05]: 5 helpers from the dead Rust VTab path retired (value_raw_ptr, extract_list_strings, LogicalTypeOwned, type_from_duckdb_type_u32, declare_output_type) — C++ side now owns LIST flattening (sv_serialise_string_list) + LogicalType declaration (sv_logical_type_from_c_type_id). Removes the duckdb-rs Value transmute that relied on repr(Rust) layout assumptions.
- [Phase 65 P05]: 5/8 test_readonly_load.py watchdog tests still RED post-Batch-3 (same failure shape as pre-Batch-3) — LIFE-01 has TWO contributors (H1 catalog_conn + H2 query_conn); Plan 05 retired H2 only, so DuckDB DBInstanceCache busy-spin on RW↔RO reopen persists. Plan 06 retires H1 → expected to flip 8/8 green. If any test stays red after Plan 06, file as Phase 67 follow-up per D-22.
- [Phase 65 P05]: test_multi_db_isolation.py 3/3 PASS confirms cross-database catalog/search-path resolution works through the per-call Connection model — preliminary EXPAND-CTX-01 finding: root cause may dissolve after Plan 06, Phase 66 may become test-scaffolding + release-prep only.

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
| Phase 65 P02 (replanned, halted) | ~30min | 2 of 6 tasks (Wave-0 spikes only) | 1 file (SPIKES.md only — both spikes reverted to disk-empty before commit) |
| Phase 65 P03 | 1h | 3 tasks | 9 files |
| Phase 65 P04 | 2h | 4 tasks | 10 files |
| Phase 65 P05 | ~10h (3 batches) | 6 tasks | 19 files (17 read-side sources + lib.rs + 2 test files) |

## Session Continuity

Last session: 2026-05-24T18:00:00.000Z
Stopped at: Phase 65 Plan 05 complete (5 of 6 plans done; Plan 06 next -- lifecycle close-out / H1 catalog_conn retirement + structural guard test + flip 5 RED watchdog tests green)
Resume file: .planning/phases/65-overridecontext-connection-teardown/65-05-SUMMARY.md
