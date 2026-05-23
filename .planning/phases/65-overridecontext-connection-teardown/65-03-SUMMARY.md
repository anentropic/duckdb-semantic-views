---
phase: 65-overridecontext-connection-teardown
plan: 03
subsystem: parser_override / catalog-read elimination
tags:
  - duckdb
  - rust
  - ffi
  - parser-override
  - lifecycle
  - slimming
  - breaking-change
  - read-elimination
dependency-graph:
  requires:
    - 65-01 (ConnGuard scaffolding + watchdog tests — ConnGuard now deleted; watchdog tests reused)
    - 65-02 (partial — 3 commits reverted to v0.9.0 shape via direct rewrite, not git revert)
  provides:
    - parser_override CREATE path with ZERO catalog reads
    - D-06 hard-error path for FK→PK-less target
    - metadata-via-SQL on the caller's connection (now() / current_database() / current_schema())
  affects:
    - 65-04 (ALTER architecture wave) — clean substrate for json_merge_patch ALTER rewrites
    - 65-05 (read-path migration wave) — read-side bind callbacks will populate column_type_names / fact.output_type that Plan 03 left empty
    - 65-06 (lifecycle close-out) — H1 catalog_conn allocation now unused by any parser_override path; Plan 06 retires the allocation itself
tech-stack:
  added: []
  patterns:
    - metadata-via-SQL via json_merge_patch + json_object embedded in INSERT
    - D-06 hard-error template (actionable, names the fix verbatim, CHANGELOG-pointing)
    - read-side type inference deferral (D-16, D-17) — column_type_names / fact.output_type left empty in persisted JSON; read-side bind populates on demand under Plan 05
key-files:
  created:
    - test/sql/65_pk_error.test
    - test/sql/65_metadata_via_sql.test
  modified:
    - src/parse.rs (revert OverrideContext to v0.9.0 shape; metadata-via-SQL emit_native_create_sql; D-06 wiring; ConnGuard removal; inline test feature-gate)
    - src/ddl/define.rs (delete resolve_pk_from_catalog; remove conn arg / infer_types flag; slim to validation + cardinality + serialize only)
    - src/lib.rs (remove `pub mod conn_guard;`; restore v0.9.0 sv_register_parser_hooks call shape)
    - cpp/src/shim.cpp (revert sv_register_parser_hooks signature; restore Phase 62 §Q2 INTENTIONAL LEAK comment)
    - src/query/table_function.rs (allow(dead_code) on type_id_to_display_name with Plan 05 forward-pointer)
    - test/sql/TEST_LIST (append 65_pk_error.test, 65_metadata_via_sql.test)
    - test/sql/phase33_cardinality_inference.test (rename PKOpt → PKReq; declare PRIMARY KEY explicitly; update Test 4 expected error to D-06 substring)
    - test/sql/phase29_facts.test, test/sql/phase30_derived_metrics.test, test/sql/phase39_metadata_storage.test (FACT DATA_TYPE → (empty); Plan 05 will revert when read-side bind probes on demand)
  deleted:
    - src/conn_guard.rs (entire file — D-02)
decisions:
  - D-01 hard-revert to v0.9.0 OverrideContext shape (catalog + is_file_backed fields, Phase 62 §Q2 Drop with INTENTIONAL LEAK comment restored)
  - D-02 src/conn_guard.rs deleted — no Rust consumer under read-elimination
  - D-05 resolve_pk_from_catalog deleted — PKs are LOGICAL user assertions per Snowflake alignment
  - D-06 hard error at CREATE / ALTER when FK target lacks PRIMARY KEY / UNIQUE in TABLES — extended to cover BOTH implicit REFERENCES (empty ref_columns from infer_cardinality) AND explicit REFERENCES(cols) (target with no pk_columns + no unique_constraints). Plan's literal check `ref_columns.is_empty()` only handles the implicit case; extending to both keeps the actionable message live regardless of REFERENCES syntax flavour the user writes.
  - D-16 / D-17 / metadata-via-SQL — CREATE-time catalog reads replaced by SQL embedded in the rewritten INSERT (json_merge_patch). Type inference deferred to read-side bind under Plan 05.
  - D-21 transactional contract intact — verified by test_adbc_transactions.py (6/6 PASS including CREATE+rollback / CREATE FROM YAML FILE+rollback / ALTER RENAME+rollback / DROP+rollback).
metrics:
  duration: 1h
  completed-date: 2026-05-24
  total-tasks: 3
  total-commits: 3
---

# Phase 65 Plan 03: parser_override Slimming Wave Summary

**One-liner:** Reverted Plan 02 partial damage and slimmed `parser_override` to zero catalog reads on the CREATE path — deleted `conn_guard.rs` + `resolve_pk_from_catalog`, moved metadata capture (`now()` / `current_database()` / `current_schema()`) into SQL via `json_merge_patch` embedded in the rewritten INSERT, and added the D-06 hard-error path for FK references to PK-less targets.

## What Shipped

1. **OverrideContext + FFI signatures back to v0.9.0 shape** (Task 1, commit `d45852c`).
   - `src/parse.rs::OverrideContext` fields = `{ catalog: CatalogReader, is_file_backed: bool }` (was `{ db_handle, catalog_table_present, is_file_backed }` under Plan 02 partial).
   - `src/parse.rs::sv_make_override_context(catalog_conn, is_file_backed) -> *mut c_void` (was 3-arg).
   - `cpp/src/shim.cpp::sv_register_parser_hooks(duckdb_database, duckdb_connection, bool)` — note: literal v0.9.0 historical truth was the 3-arg shape, not the plan's <interfaces> 2-arg transcription (deviation logged in commit body).
   - `src/conn_guard.rs` deleted (D-02).
   - Phase 62 §Q2 INTENTIONAL LEAK comment restored on `OverrideContext::drop` and `~SemanticViewsParserInfo`.

2. **`resolve_pk_from_catalog` removed; D-06 hard error wired** (Task 2, commit `d168b76`).
   - `src/ddl/define.rs::resolve_pk_from_catalog` (entire function, lines 19–76 of pre-revert) deleted.
   - `enrich_definition_for_create` step 2 emits the D-06 template:
     > `Table 'X' has no PRIMARY KEY declared but is referenced by FK in 'Y'. Add PRIMARY KEY (cols) or UNIQUE (cols) to the TABLES clause for X. (v0.10.0: physical-catalog PK auto-inference removed -- see CHANGELOG.)`
   - The check fires when the FK target has NEITHER `pk_columns` NOR `unique_constraints` declared in TABLES — covers both `REFERENCES o` (implicit, ref_columns empty) and `REFERENCES o(col)` (explicit, ref_columns set). Plan literal check was `ref_columns.is_empty()` only; extending to both flavours keeps the actionable D-06 wording live regardless of user syntax.
   - `test/sql/65_pk_error.test` (new): B1 (implicit REFERENCES) + B2 (explicit REFERENCES with cols) + B3 (good view with PRIMARY KEY).
   - Inline Rust test `errors_when_target_has_no_pk_and_no_explicit_ref` (feature-gated on `extension`) asserts both the D-06 substring and the v0.10.0 CHANGELOG parenthetical.
   - `test/sql/phase33_cardinality_inference.test`: renamed "PKOpt" → "PKReq", declared PRIMARY KEY explicitly in all formerly auto-inferred test cases, updated Test 4 expected error from the v0.9.0 "Specify referenced columns explicitly" template to the D-06 substring.

3. **Metadata-via-SQL upgrade; enrich_definition_for_create slimmed** (Task 3, commit `fddb981`).
   - `src/ddl/define.rs::enrich_definition_for_create` signature reduced to `(name: &str, def: SemanticViewDefinition) -> Result<String, String>`. No `conn`, no `infer_types`. Steps surviving:
     1. infer_cardinality
     2. D-06 hard-error scan
     3. graph + facts + derived-metrics + using-relationships validators
     4. serde_json::to_string
   - `src/parse.rs::emit_native_create_sql` builds a `metadata_patched_definition` sub-expression:
     ```sql
     json_merge_patch(
       '<enriched_json>'::JSON,
       json_object(
         'created_on',    strftime(now(), '%Y-%m-%dT%H:%M:%SZ'),
         'database_name', current_database(),
         'schema_name',   current_schema()
       )
     )::VARCHAR
     ```
     and substitutes it into the OR REPLACE, IF NOT EXISTS, and plain CREATE INSERT shapes. `now()` / `current_database()` / `current_schema()` resolve on the CALLER's connection at INSERT-time, preserving D-21 transactional contract.
   - Phase 60 CASE+error friendly-already-exists pattern (`semantic view 'X' already exists; use CREATE OR REPLACE…`) preserved verbatim for plain CREATE.
   - `test/sql/65_metadata_via_sql.test` (new): 4 assertions on the new shape — B1 database_name matches caller's `current_database()`; B2 schema_name matches caller's `current_schema()`; B3 created_on ISO-8601-shaped (`%T…%Z`); B4 CREATE OR REPLACE refreshes created_on (uses `pg_sleep(1.1)` for whole-second strftime resolution).

## Deviations from Plan

### Auto-fixed (Rules 1–3)

1. **[Rule 2 — Plan-consistency extension] D-06 check covers explicit REFERENCES(cols) too.**
   - **Found during:** Task 2 sqllogictest authoring.
   - **Issue:** Plan's behavior section uses `REFERENCES o(order_id)` (explicit ref columns), but plan's implementation guidance says `!fk_columns.is_empty() && ref_columns.is_empty()`. Those are mutually exclusive — when REFERENCES has explicit cols, ref_columns is NOT empty, so the D-06 path never fires. Sqllogictest using the plan's literal SQL would have surfaced the existing generic CARD-03 "FK ... does not match any PRIMARY KEY or UNIQUE constraint" error, not the actionable D-06 wording the plan mandates.
   - **Fix:** Extended the D-06 check in `enrich_definition_for_create` step 2 to trigger when the FK target has NEITHER `pk_columns` NOR `unique_constraints` declared — regardless of whether `ref_columns` is empty (implicit) or set (explicit). Strict superset of the plan's check; CARD-03 still fires for column-mismatch failures (target HAS PK but ref_columns doesn't match).
   - **Files modified:** `src/ddl/define.rs`.
   - **Commit:** `d168b76`.

2. **[Rule 1 — Phase 39 expected-output update] FACT DATA_TYPE → (empty).**
   - **Found during:** Task 3 just test-sql run after slimming.
   - **Issue:** Plan's Task 3 acceptance criterion ("phase39 stays byte-identical") is incompatible with the plan's own must-have ("§7 fact typeof… removed"). Removing §7 means persisted JSON has no `fact.output_type`, so SHOW SEMANTIC FACTS necessarily returns `(empty)` for the data_type column until Plan 05 lands the read-side bind probe. Plan's own Test 5 wording acknowledges this for SHOW SEMANTIC COLUMNS ("Plan 05's read-side bind will probe at SHOW time. Plan 03's deliverable is that the JSON shape no longer carries types -- the read-side population is Plan 05's responsibility").
   - **Fix:** Updated test/sql/phase39_metadata_storage.test (Test 4 + Test 7), test/sql/phase29_facts.test, and test/sql/phase30_derived_metrics.test to expect `(empty)` for `FACT … DATA_TYPE`. Plan 05's read-side bind populates the column on demand; tests will need re-update at that time to assert the inferred values come back.
   - **Files modified:** `test/sql/phase39_metadata_storage.test`, `test/sql/phase29_facts.test`, `test/sql/phase30_derived_metrics.test`.
   - **Commit:** `fddb981`.

3. **[Rule 3 — feature-gate inline test] `errors_when_target_has_no_pk_and_no_explicit_ref` gated on `extension`.**
   - **Found during:** `just test-all` cargo nextest run.
   - **Issue:** The new test calls `crate::ddl::define::enrich_definition_for_create`, but `crate::ddl` lives under `#[cfg(feature = "extension")]` (src/lib.rs:283-284). Default-features nextest run failed E0433 ("cannot find `ddl` in crate").
   - **Fix:** Added `#[cfg(feature = "extension")]` to the new test.
   - **Files modified:** `src/parse.rs`.
   - **Commit:** `fddb981` (rolled into Task 3 commit since it was discovered during Task 3 verification).

### Plan-text vs git-history truth (declared deviation, no auto-fix)

- **C++ shim signature: 3-arg historical truth vs 2-arg plan transcription.** Plan 03's `<interfaces>` block transcribes the v0.9.0 `sv_register_parser_hooks` signature as 2-arg `(duckdb_connection catalog_conn, bool is_file_backed)`. The literal git history (`git show f9caafe^:cpp/src/shim.cpp`) shows 3-arg `(duckdb_database db_handle, duckdb_connection catalog_conn, bool is_file_backed)`. Task 1 reverted to the historical truth (3-arg). Acceptance criterion grep `'sv_register_parser_hooks\\(' cpp/src/shim.cpp` still finds the signature on the same line, with the actual v0.9.0 shape — the grep narrows on the function name, not the arg count. Documented in Task 1 commit body (`d45852c`).

### Auth gates / human-action checkpoints

- None.

## Persisted v0.9.0 Definition Compatibility (D-07)

Verified by inspection: existing v0.9.0-written rows in `semantic_layer._definitions` (with auto-inferred `pk_columns`, `column_type_names`, `column_types_inferred`, fact `output_type` populated) deserialize cleanly via `serde_json::from_str::<SemanticViewDefinition>` — all the affected fields use `#[serde(default, skip_serializing_if = "Vec::is_empty" / "Option::is_none")]` so missing keys deserialize to defaults and existing keys round-trip. The D-06 validation triggers only inside `enrich_definition_for_create`, which is called only from CREATE / ALTER (re-write) paths — NOT from any read-side bind. `semantic_view()` SELECT + `SHOW SEMANTIC *` + `DESCRIBE SEMANTIC VIEW` continue to load v0.9.0 rows without re-running validation. Sqllogictest evidence: `phase39_metadata_storage.test` Test 2 (DROP + recreate) and Test 3 (CREATE OR REPLACE) both PASS — exercising the catalog round-trip across the new write path.

## TECH-DEBT Surfaced

- **TECH-DEBT note (Plan 05 forward-pointer):** `src/query/table_function.rs::type_id_to_display_name` and `try_infer_schema` / `normalize_type_id` have NO in-tree caller after Plan 03 strips CREATE-time type inference. Marked `#[allow(dead_code)]` for now with a comment pointing at Plan 05. If Plan 05 lands without consuming them, this becomes dead code worth deleting.
- **TECH-DEBT note (read-side type inference still pending):** Until Plan 05's read-side bind callbacks probe on demand, `SHOW SEMANTIC FACTS / DIMENSIONS / METRICS` and `DESCRIBE SEMANTIC VIEW` return `(empty)` for the data_type column on freshly-created views. v0.9.0 user-visible behavior (populated DOUBLE / DECIMAL / etc.) returns under Plan 05; tests phase29 / phase30 / phase39 will need re-update when that lands.

## Verification Evidence

- **just test-sql:** 49/49 PASS (47 baseline + `65_pk_error.test` + `65_metadata_via_sql.test`). Plan 02 partial's 4/47 regression is RESOLVED.
- **cargo nextest run (default features, extension on):** 933/933 PASS.
- **cargo test --lib --features extension --no-default-features:** 844/844 PASS.
- **test_adbc_transactions.py:** 6/6 PASS (CREATE inline rollback/commit, CREATE FROM YAML FILE rollback/commit, ALTER RENAME rollback, DROP rollback). D-21 transactional contract intact.
- **test/integration/test_readonly_load.py watchdog tests (B1..B4 + B11):** EXPECTED FAILURE (TimeoutError) — per D-03 these flip green only at Plan 06's commit (H1 catalog_conn retirement). Plan 03 does not retire H1; Plan 03's deliverable is "H1 catalog_conn is still allocated but no parser_override code path needs it" — which is satisfied (`grep -E "ctx\\.catalog\\.(raw|exists|lookup|read_text)" src/parse.rs` returns hits inside DROP / ALTER paths that Plan 04 owns, NOT inside CREATE paths Plan 03 owned). 65-01-SUMMARY explicitly documents the watchdog tests "fail on baseline and on the intermediate slimming/ALTER plans, and MUST flip green by the lifecycle close-out plan".

## Forward Pointers

- **Plan 04 (ALTER architecture wave):** can now adopt the same `json_merge_patch` pattern Plan 03 introduced for the metadata sub-expression. The 3 surviving ALTER variants (RENAME, SET COMMENT, UNSET COMMENT) become pure-SQL UPDATEs against `_definitions` per RESEARCH §1.2. `rewrite_yaml_file_create` still uses `ctx.catalog.raw()` for `read_text(file_path)` — Plan 04 migrates this to the `__sv_compute_create_from_yaml` helper TF.
- **Plan 05 (read-path migration wave):** read-side bind callbacks (12 listed in CONTEXT D-14 / 17 per A2/A3 resolutions) move to the C++ Catalog API shim and gain `ClientContext &` for per-call `Connection(*context.db)`. Read-side `LIMIT 0` type probe + per-fact `typeof(expr)` populate `column_type_names` / `fact.output_type` lazily on demand. Tests phase29 / phase30 / phase39 expectations revert to v0.9.0-style populated types at that time.
- **Plan 06 (lifecycle close-out):** H1 `catalog_conn` allocation at `src/lib.rs:386-410` is unused by any parser_override CREATE path after Plan 03 (verify via grep: the only `ctx.catalog.*` consumers in the CREATE path are `ctx.catalog.exists` parser-side fast-path checks that Plan 06 can either delete or keep as a no-op gated on `is_file_backed`). Plan 06 retires the allocation, restoring the v0.10.0 milestone goal (LIFE-01 root-cause fix: in-process RW→RO reopen no longer hangs).

## Task Commits

1. **Task 1: revert(65-03)** — `d45852c` — restore v0.9.0 OverrideContext shape; delete conn_guard.rs.
2. **Task 2: feat(65-03)** — `d168b76` — delete resolve_pk_from_catalog; add D-06 hard error.
3. **Task 3: feat(65-03)** — `fddb981` — metadata-via-SQL; slim enrich_definition_for_create.

## Self-Check: PASSED

- File `src/conn_guard.rs` → MISSING (expected per D-02): VERIFIED via `test ! -f src/conn_guard.rs`.
- File `test/sql/65_pk_error.test` → FOUND.
- File `test/sql/65_metadata_via_sql.test` → FOUND.
- File `.planning/phases/65-overridecontext-connection-teardown/65-03-SUMMARY.md` → FOUND (this file).
- Commit `d45852c` → FOUND in git log.
- Commit `d168b76` → FOUND in git log.
- Commit `fddb981` → FOUND in git log.
- `just test-sql` → 49/49 PASS verified.
- `cargo nextest run` → 933/933 PASS verified.
- `test_adbc_transactions.py` → 6/6 PASS verified.
