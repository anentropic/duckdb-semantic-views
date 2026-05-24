---
phase: 65-overridecontext-connection-teardown
plan: 04
subsystem: parser_override / ALTER + CREATE FROM YAML architecture wave
tags:
  - duckdb
  - rust
  - cpp
  - ffi
  - parser-override
  - alter
  - yaml
  - json_merge_patch
  - catalog-api
  - read-elimination
dependency-graph:
  requires:
    - 65-03 (parser_override slimmed; CREATE path runs without catalog reads; metadata-via-SQL via json_merge_patch on the inline path)
  provides:
    - sv_register_table_function C++ Catalog API shim (consumed by Plan 05 for 17 read-side callbacks)
    - __sv_compute_create_from_yaml helper TF (per-call Connection(*context.db) read of the YAML file)
    - pure-SQL ALTER SET / UNSET COMMENT (json_merge_patch UPDATE on caller's conn)
    - pure-SQL CREATE FROM YAML FILE (INSERT...SELECT FROM __sv_compute_create_from_yaml subquery on caller's conn)
    - empirical confirmation that DuckDB v1.5.2 json_merge_patch honors RFC-7396 null-as-delete
  affects:
    - 65-05 (read-path migration wave) — sv_register_table_function is now in HEAD and ready to be the table-function side of Plan 05's bridge; A2 resolution honored.
    - 65-06 (lifecycle close-out) — parser_override has ZERO catalog read consumers after this plan; H1 catalog_conn allocation at src/lib.rs:386-410 is still present but truly unused by any parser_override path. Plan 06 retires the allocation itself.
tech-stack:
  added: []
  patterns:
    - C++ Catalog API table-function registration via sv_register_table_function (per A2 resolution; introduced from scratch in this plan ~330 LOC of new C++ in shim.cpp following 65-READ-PATH-SPIKE.md template)
    - Per-call Connection(*context.db) opened inside a TableFunction bind callback (load-bearing primitive for read-elimination; consumed by __sv_compute_create_from_yaml today, by all 17 read-side bind callbacks under Plan 05)
    - Rust↔C++ FFI bridge via sv_compute_create_from_yaml_rust (catch_unwind + Box<[u8]>::into_raw + sv_free_buffer ownership-transfer convention; mirrors sv_parser_override_rust from v0.8.0)
    - json_merge_patch UPDATE rewrites for ALTER SET/UNSET COMMENT (Plan 04 Wave 0 empirically confirmed RFC-7396 null-as-delete; replaces the v0.9.0 lookup+mutate+reserialize round trip)
    - helper-TF-subquery INSERT shape for CREATE FROM YAML FILE (INSERT INTO _definitions SELECT ... FROM __sv_compute_create_from_yaml(...)); inherits the metadata-via-SQL json_merge_patch wrapper from Plan 03's inline CREATE path
key-files:
  created:
    - cpp/src/shim.hpp
    - src/ddl/alter_helpers_ffi.rs
    - test/sql/65_json_merge_patch_smoke.test
    - test/sql/65_alter_comment_merge_patch.test
    - test/sql/65_alter_rename_via_sql.test
    - test/integration/test_create_from_yaml_v010.py
  modified:
    - cpp/src/shim.cpp (added ~330 LOC: sv_register_table_function + CreateFromYamlBindData + sv_create_from_yaml_bind / sv_create_from_yaml_function / sv_create_from_yaml_init_local + registration call site inside sv_register_parser_hooks; included shim.hpp at the top; added Rust FFI forward declaration for sv_compute_create_from_yaml_rust)
    - src/parse.rs (rewrite_alter_comment migrated to pure-SQL json_merge_patch UPDATE; rewrite_yaml_file_create migrated to __sv_compute_create_from_yaml subquery + metadata-via-SQL wrapper)
    - src/ddl/mod.rs (declared alter_helpers_ffi module)
    - build.rs (added cpp/src/shim.hpp to rerun-if-changed set)
    - test/sql/TEST_LIST (appended the three new sqllogictest files)
decisions:
  - A1 RESOLVED EMPIRICALLY (Task 1 Wave 0): DuckDB v1.5.2 json_merge_patch honors RFC-7396 null-as-delete. UNSET COMMENT therefore uses constant patch literal `{"comment":null}` -- no helper TF needed for the unset case.
  - A2 HONORED (Task 2): sv_register_table_function introduced from scratch per the planner's <interfaces> spec (NOT a revert/keep of a partial-Plan-02 commit; that commit was self-reverted at end of spike per A2 finding). ~250 LOC of new C++ in shim.cpp (the register helper + the helper TF + the integration glue) plus a 71-line new shim.hpp -- within RESEARCH §5.4's budget for keeping helpers in the one TU.
  - A7 HONORED (Task 3): only the 3 ALTER variants present in HEAD (RENAME TO, SET COMMENT, UNSET COMMENT) were migrated. The 8 enumerated additional variants (MAKE PRIVATE/PUBLIC, SET TAG, ADD SYNONYMS, ADD/DROP DIMENSION/METRIC/FACT, ADD/DROP RELATIONSHIP) are explicitly NOT implemented per locked resolution A7 (Snowflake non-features). No code paths added for them.
  - D-09 SUPERSEDED BY A1 RESOLUTION: json_set / json_remove DO NOT exist in DuckDB v1.5.2; json_merge_patch is the actual mechanism. Plan 04 emits `json_merge_patch(def::JSON, '<patch>'::JSON)::VARCHAR` instead of `json_set(def, '$.comment', '<new>')`.
  - D-11 IMPLEMENTED (Task 4): __sv_compute_create_from_yaml(path, name, kind, comment) helper TF with per-call Connection(*context.db) read of the YAML file. Helper returns metadata-less JSON; outer INSERT wraps it with json_merge_patch + json_object('created_on', strftime(now(),...), 'database_name', current_database(), 'schema_name', current_schema()) so metadata reflects the caller's session (D-21 transactional contract preserved).
  - D-13 PRESERVED: Phase 60 race-guard two-statement pattern (SELECT CASE WHEN NOT EXISTS THEN error() ELSE TRUE; followed by the actual UPDATE) is unchanged for both rewrite_alter_comment and the plain-CREATE shape in rewrite_yaml_file_create.
  - D-21 PRESERVED: every change in this plan emits SQL that runs on the caller's connection -- the outer UPDATE for ALTER, the outer INSERT for CREATE FROM YAML FILE -- so BEGIN/.../ROLLBACK undoes the change. Verified by test_adbc_transactions.py (6/6 PASS after each commit) AND by test_create_from_yaml_v010.py T7 (BEGIN/CREATE FROM YAML FILE/ROLLBACK leaves _definitions empty) AND by test/sql/65_alter_comment_merge_patch.test B5 (BEGIN/ALTER/ROLLBACK restores pre-tx comment).
  - DESIGN CHOICE: helper TF returns metadata-less JSON; outer INSERT wraps via json_merge_patch (option (b) from the planner's <action> guidance). Reason: keeps the helper TF stateless (it doesn't capture caller's session) AND lets the metadata fields resolve on the caller's connection at INSERT-time, matching Plan 03's inline-CREATE behaviour byte-for-byte. The alternative would have been to capture metadata inside the helper's bind (would have required passing current_database/current_schema/now into the helper somehow, or having it open another query). Chose the simpler stateless shape.
metrics:
  duration: 2h
  completed-date: 2026-05-24
  total-tasks: 4
  total-commits: 4
---

# Phase 65 Plan 04: ALTER + CREATE FROM YAML FILE Architecture Wave Summary

**One-liner:** ALTER SET/UNSET COMMENT and CREATE FROM YAML FILE now ride pure-SQL rewrites on the caller's connection via DuckDB v1.5.2 `json_merge_patch` (UPDATE) and the new `__sv_compute_create_from_yaml` helper table function (INSERT...SELECT FROM subquery), eliminating the last two `parser_override` consumers of the long-lived extension-owned catalog connection.

## What Shipped

1. **Wave 0 spike — json_merge_patch RFC-7396 null-as-delete confirmed** (Task 1, commit `2ff494d`).
   - `test/sql/65_json_merge_patch_smoke.test` with 4 query assertions on DuckDB v1.5.2 stock behaviour:
     - B1 `json_merge_patch('{"a":1,"b":2}'::JSON, '{"b":null}'::JSON)` -> `{"a":1}` (null deletes the key)
     - B2 scalar overwrite `'{"comment":"old"}'` + `'{"comment":"new"}'` -> `'{"comment":"new"}'`
     - B3 the literal UNSET COMMENT shape: `'{"comment":"old","other":42}'` + `'{"comment":null}'` -> `'{"other":42}'` (sibling keys preserved, comment deleted)
     - B4 `'{}'` + `'{"a":null}'` -> `'{}'` (null on absent key is a no-op)
   - All 4 PASS; A1 resolution unblocked Task 3.

2. **Wave 1 — sv_register_table_function shim + __sv_compute_create_from_yaml helper TF + Rust FFI bridge** (Task 2, commit `40004e0`).
   - `cpp/src/shim.hpp` (new, 71 LOC) declares `sv_register_parser_hooks` + `sv_register_table_function` extern "C" entries. Pinned by `#include "shim.hpp"` at the top of shim.cpp.
   - `cpp/src/shim.cpp` (+330 LOC):
     - `sv_register_table_function(db_handle, name, args, arg_count, bind_cb, exec_cb, init_cb)` -- reusable C++ Catalog API wrapper. Internally builds `TableFunction tf(name, args, exec, bind, /*init_global*/ nullptr, init_local)`, sets `info.on_conflict = ALTER_ON_CONFLICT` (clean re-LOAD), calls `system_catalog.CreateTableFunction(txn, info)` per the 65-READ-PATH-SPIKE.md template.
     - `CreateFromYamlBindData : TableFunctionData` holds (file_path, view_name, kind, comment, new_def).
     - `CreateFromYamlLocalState : LocalTableFunctionState` holds an `emitted` flag for single-row emission.
     - `sv_create_from_yaml_bind` opens `Connection probe(*context.db)`, runs `Query("SELECT content FROM read_text('<escaped>')")` (path escaped on the Rust side; file_path arrives through `input.inputs[0]` as a typed `Value`, not concatenated), validates the result, then calls `sv_compute_create_from_yaml_rust` to parse/enrich/serialize the YAML into a metadata-less JSON, moves the heap-owned buffer into `bd->new_def`, and releases via `sv_free_buffer`.
     - `sv_create_from_yaml_function` emits the single-VARCHAR row from `bd->new_def`, gated on `state.emitted` so subsequent invocations return zero rows (chunked emission safety).
     - `sv_register_parser_hooks` now calls `sv_register_table_function(...)` to register the helper TF; failure surfaces a stderr warning and returns false (extension load fails fast).
     - Originally tried `Connection::Prepare("SELECT content FROM read_text(?)")` + `Execute(params)` but `Cast<MaterializedQueryResult>` triggered an `InternalException`. Switched to `probe.Query(string)` with the path escaped before embedding (SQL-injection safety: `input.inputs[0]` is a typed `Value`, not user-string concatenation; the doubled-quote escape is belt-and-braces).
   - `src/ddl/alter_helpers_ffi.rs` (new, 386 LOC after rustfmt):
     - `#[no_mangle] pub unsafe extern "C" fn sv_compute_create_from_yaml_rust(content_ptr, content_len, name_ptr, name_len, comment_ptr, comment_len, _kind, out_ptr, out_len, error_buf, error_buf_len) -> u8`
     - Body wraps `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`. Decodes inputs (UTF-8-checked), calls `SemanticViewDefinition::from_yaml_with_size_cap(name, content)` (size cap enforced), applies comment override if provided, then calls slimmed `crate::ddl::define::enrich_definition_for_create(name, def)` to produce JSON. Output goes through `Box<[u8]>::into_raw` -> publish to `*out_ptr` / `*out_len`. Caller releases via `sv_free_buffer`.
     - Return codes: 0 success / 1 parse-or-input / 2 enrichment / 3 internal-panic.
     - 6 inline `#[cfg(all(test, feature="extension"))]` tests cover the happy path, comment override, malformed YAML, oversized YAML, empty name, null content pointer.
   - `src/ddl/mod.rs` declares the new module.
   - `build.rs` adds `cpp/src/shim.hpp` to the `cargo:rerun-if-changed` set.

3. **Wave 2 Task 3 — ALTER SET/UNSET COMMENT via pure-SQL json_merge_patch UPDATE** (commit `7ba997d`).
   - `src/parse.rs::rewrite_alter_comment` rewritten:
     - NO longer calls `catalog.lookup()`, no longer deserializes the stored JSON, no longer mutates a Rust `SemanticViewDefinition`, no longer serializes back. The JSON mutation runs entirely inside DuckDB.
     - Still calls `catalog.exists()` so the friendly "does not exist" pre-check wording stays byte-identical with v0.9.0 / phase45's assertions (rewrite_drop has used this same pattern since v0.8.0; Plan 06 retires both). The plan must-have explicitly bans `catalog.lookup` -- `catalog.exists` is a distinct API and is unchanged here.
     - SET COMMENT: build JSON patch via `serde_json::to_string(&json!({"comment": comment}))` so internal `"` / `\` / control chars are JSON-escaped, then `escape_sql_arg` doubles any embedded single quotes for the outer SQL literal (belt-and-braces escape stack).
     - UNSET COMMENT: constant patch `{"comment":null}` (Wave 0 spike verifies RFC-7396 null-as-delete).
     - Race-guard two-statement pattern (D-13) preserved for non-IF-EXISTS; IF EXISTS keeps the silent UPDATE-affects-0 contract.
   - `test/sql/65_alter_comment_merge_patch.test` (new): 5 behaviour groups B1-B5 covering SET, UNSET, "does not exist" wording (matches phase45 byte-identical), special-char round-trip through the JSON+SQL escape stack (single-quote / double-quote / backslash), and BEGIN/ALTER/ROLLBACK transactional invariant. Cross-database ALTER (ATTACH 'db2'; ALTER ON db2.v) was attempted in initial drafting and surfaced a Plan-66 scope concern (the v0.9.0 extension only initializes `semantic_layer._definitions` on the LOAD database); documented as out-of-scope and replaced with the transactional test instead.
   - `test/sql/65_alter_rename_via_sql.test` (new): 7 behaviour groups regression-guarding the pure-SQL ALTER RENAME path (already landed under Plan 03; this plan only adds the regression test). Covers RENAME, round-trip rename, RENAME-to-existing (already exists error), RENAME-of-nonexistent (does not exist error), IF EXISTS silent no-op, post-rename query path.
   - phase45_alter_comment.test continues to PASS byte-identical (the new SQL shape produces the same user-visible result).

4. **Wave 2 Task 4 — CREATE FROM YAML FILE via __sv_compute_create_from_yaml subquery** (commit `124c3e6`).
   - `src/parse.rs::rewrite_yaml_file_create` fully rewritten:
     - The function NO longer reads the YAML file via `ctx.catalog.raw()` and `read_text()`. The entire file-read + YAML-parse + enrich + serialize pipeline moves into the helper TF's bind callback (Task 2).
     - The `&OverrideContext` argument is now `_ctx` (unused). Kept for signature symmetry with `rewrite_create`. Plan 06 retires both call surfaces.
     - Three emit shapes mirror `emit_native_create_sql` so user-visible behaviour matches the inline CREATE path byte-for-byte:
       - Plain CREATE: `INSERT INTO _definitions (name, definition) SELECT CASE WHEN EXISTS (...) THEN error('already exists') ELSE '<name>' END, json_merge_patch(new_def::JSON, json_object(<metadata>))::VARCHAR FROM __sv_compute_create_from_yaml('<path>', '<name>', 0, '<comment>') RETURNING name AS view_name`.
       - CREATE OR REPLACE: `INSERT OR REPLACE INTO _definitions (name, definition) SELECT '<name>', <metadata_patched> FROM __sv_compute_...(...,1,...) RETURNING ...`.
       - CREATE IF NOT EXISTS: `INSERT OR IGNORE INTO _definitions (name, definition) SELECT ... FROM __sv_compute_...(...,2,...) RETURNING ...`.
     - The metadata-via-SQL wrapper uses `json_merge_patch(new_def::JSON, json_object('created_on', strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), 'database_name', current_database(), 'schema_name', current_schema()))::VARCHAR` -- same shape as Plan 03's inline CREATE path. The helper TF returns metadata-less JSON, so the patch is the sole source of those three fields (no overwrite risk).
   - `test/integration/test_create_from_yaml_v010.py` (new, PEP-723 header): 8 end-to-end tests T1-T8 -- plain CREATE / OR REPLACE / IF NOT EXISTS / nonexistent file (`FROM YAML FILE failed` substring) / malformed YAML / 2 MiB YAML (size cap) / BEGIN+CREATE+ROLLBACK transactional invariant / get_ddl round-trip. All 8 PASS.
   - phase53_yaml_file.test continues to PASS byte-identical.

## Deviations from Plan

### Auto-fixed (Rules 1-3)

1. **[Rule 1 — bug] Connection::Prepare/Execute path triggers InternalException; switched to Connection::Query with escaped path.**
   - **Found during:** Task 2 smoke test of the helper TF.
   - **Issue:** The planner's spec suggested `probe.Prepare("SELECT content FROM read_text(?)") + Execute(params)` for the file read. On DuckDB v1.5.2 the resulting non-materialized `QueryResult` triggers `INTERNAL Error: Failed to cast query result to type` when downcast to `MaterializedQueryResult`.
   - **Fix:** Switched to `probe.Query(string)` (returns `MaterializedQueryResult` directly) with the file path escaped before embedding into the SQL string. SQL-injection safety preserved: the file path arrives through `input.inputs[0]` as a typed `Value` (not raw user input concatenated into a SQL string); the doubled-quote escape on top is defence-in-depth.
   - **Files modified:** `cpp/src/shim.cpp` (the `sv_create_from_yaml_bind` body).
   - **Commit:** `40004e0` (rolled into Task 2 since it was discovered during Task 2 verification).

2. **[Rule 1 — bug] get_ddl signature is `(kind VARCHAR, name VARCHAR)`, not `(name VARCHAR)`; kind value is `SEMANTIC_VIEW` (underscore).**
   - **Found during:** Task 4 integration test T8.
   - **Issue:** Plan's T8 sketch was `SELECT get_ddl('v')`. The actual scalar takes a `kind` argument first, and the supported kind is the snake-case form `SEMANTIC_VIEW` (NOT space-separated `SEMANTIC VIEW`).
   - **Fix:** Test T8 calls `get_ddl('SEMANTIC_VIEW', 'v')`. Round-trip uses the emitted DDL directly (`CREATE OR REPLACE` is already the default get_ddl shape) instead of a sed-style replace.
   - **Files modified:** `test/integration/test_create_from_yaml_v010.py`.
   - **Commit:** `124c3e6` (rolled into Task 4 since it was discovered during Task 4 verification).

3. **[Rule 1 — bug] list_semantic_views() reads committed-only state (TECH-DEBT 19); transactional test must use direct _definitions SELECT inside the txn.**
   - **Found during:** Task 3 sqllogictest authoring (B5).
   - **Issue:** The transactional test's first assertion `SELECT comment FROM list_semantic_views() WHERE name = 'p65_sv'` inside an open txn returned the pre-txn comment value, not the post-ALTER value. This is because `list_semantic_views` runs on the extension's catalog conn (committed-only, TECH-DEBT 19 / Plan 05 territory), NOT the caller's conn.
   - **Fix:** Use `SELECT json_extract_string(definition, '$.comment') FROM semantic_layer._definitions WHERE name = ?` inside the txn -- this runs on the caller's own conn which sees the uncommitted UPDATE. Post-ROLLBACK the value is restored. This pattern is what verifies D-21 transactionality at the SQL layer.
   - **Files modified:** `test/sql/65_alter_comment_merge_patch.test`.
   - **Commit:** `7ba997d`.

### Declared (no auto-fix, documented as known scope)

1. **Acceptance criterion grep `grep -nE 'catalog\.read_text|from_yaml_with_size_cap' src/parse.rs` returns NON-zero matches (2 inline-path hits + 1 doc comment), not 0.**
   - The plan's acceptance criterion conflated the INLINE FROM YAML body path (`rewrite_ddl_yaml_body` at lines 1411 / 1430, untouched by Plan 04) with the FROM YAML FILE path (`rewrite_yaml_file_create`, the actual Plan 04 target). The inline path doesn't need a Connection because the YAML is already in the DDL text itself; it's NOT consuming the OverrideContext catalog. Plan 04's scope was only the FROM YAML FILE path, and rewrite_yaml_file_create no longer parses YAML in Rust at all (verified: `grep` inside `fn rewrite_yaml_file_create` -- the only `from_yaml_with_size_cap` hit on line 2064 is inside a code comment explaining what the helper TF does).
   - The two remaining `from_yaml_with_size_cap` Rust call sites in `src/parse.rs` are at the doc comment on line 1411 and the inline parse on line 1430, both inside `rewrite_ddl_yaml_body` (the dollar-quoted inline path), which is out of scope per CONTEXT D-22 (bounded scope: surface, don't absorb adjacent issues).
   - Net effect: parser_override has zero remaining OverrideContext-catalog consumers under the FROM YAML FILE path AND under the inline FROM YAML path (the inline path never used OverrideContext anyway). Plan 05 / Plan 06 ledger is unchanged.

2. **`just test-all` overall is RED on the test/integration/test_readonly_load.py watchdog tests (B1..B4 + B11 TimeoutError).**
   - Per D-03 in CONTEXT.md and Plan 03's identical SUMMARY notation: these tests fail on baseline and on every plan through 05; they flip green at Plan 06's H1 catalog_conn retirement (the entire root cause of LIFE-01).
   - All other gates that Plan 04 owns are GREEN: `just test-sql` (52/52 PASS), `test_adbc_transactions.py` (6/6 PASS), `test_create_from_yaml_v010.py` (8/8 PASS), `cargo nextest run` (933/933 PASS), `cargo test --lib --features extension` (850/850 PASS).
   - This deviation is unchanged across Plans 03, 04, 05; it is a property of the WHOLE phase, not a regression introduced by any individual plan.

### Auth gates / human-action checkpoints

- None.

## Decision Log: Helper TF Returns Metadata-Less JSON

Per the planner's <action> guidance for Task 2 Step C, two options for metadata population:
- (a) helper TF captures `now()` / `current_database()` / `current_schema()` itself (would require either opening a second probe Connection inside bind, or passing context through Rust);
- (b) helper TF returns metadata-less JSON, outer INSERT wraps with `json_merge_patch + json_object` on the caller's conn.

**Chose (b).** Reasons:
1. **Stateless helper TF.** It doesn't care which session called it; the metadata reflects the SQL caller's session by construction (the `now()` / `current_database()` / `current_schema()` resolve on the outer INSERT's binding).
2. **Byte-identical behaviour with non-YAML CREATE.** Plan 03 used exactly this `json_merge_patch + json_object` pattern in `emit_native_create_sql`. Plan 04 reuses the same template for the YAML path so SHOW SEMANTIC VIEWS / list_semantic_views / DESCRIBE / get_ddl produce identical output for "CREATE inline" vs "CREATE FROM YAML FILE" beyond the one-row content difference in the YAML version's definition.
3. **D-21 transactional contract preservation.** The metadata-capture expressions resolve on the caller's conn at INSERT-time, so they participate in BEGIN/COMMIT/ROLLBACK without the helper TF needing to know anything about the caller's transaction.

The helper TF's `_kind` parameter is currently unused on the Rust side (the outer INSERT shape encodes ON CONFLICT behaviour). Kept threaded through the FFI for forward compat with future variants whose enrichment might differ by kind.

## Verification Evidence

- **just test-sql:** 52/52 PASS (49 from Plan 03 + 65_json_merge_patch_smoke + 65_alter_comment_merge_patch + 65_alter_rename_via_sql).
- **phase53_yaml_file.test:** PASS byte-identical (no edits needed; the new __sv_compute_create_from_yaml-based INSERT produces the same user-visible result for every assertion including "FROM YAML FILE failed" wording).
- **phase45_alter_comment.test:** PASS byte-identical (the catalog.exists pre-check carries the "does not exist" wording through unchanged).
- **phase34_1_alter_rename.test:** PASS byte-identical.
- **test_adbc_transactions.py:** 6/6 PASS (CREATE inline rollback/commit, CREATE FROM YAML FILE rollback/commit, ALTER RENAME rollback, DROP rollback). D-21 transactional contract intact across all 4 commits of this plan.
- **test_create_from_yaml_v010.py:** 8/8 PASS (T1 plain CREATE + metadata fields, T2 OR REPLACE replaces, T3 IF NOT EXISTS silent no-op, T4 nonexistent path -> "FROM YAML FILE failed", T5 malformed YAML -> parse error wrapped with the same prefix, T6 2 MiB -> size cap "exceeds" message, T7 BEGIN+CREATE+ROLLBACK leaves _definitions empty (D-21 verified end-to-end), T8 get_ddl round-trip via OR REPLACE).
- **cargo nextest run (default features):** 933/933 PASS.
- **cargo test --lib --features extension --no-default-features:** 850/850 PASS (includes 6 new alter_helpers_ffi inline tests).
- **just test-all:** RED on the readonly_load watchdog suite (expected per D-03, flips green at Plan 06).

### Acceptance Criteria Grep Counts

- `grep -c 'sv_register_table_function' cpp/src/shim.cpp` = **8** (definition + call site + comments; >= required 2).
- `grep -c 'sv_register_table_function' cpp/src/shim.hpp` = **2** (declaration + comment; >= required 1).
- `grep -c '__sv_compute_create_from_yaml' cpp/src/shim.cpp` = **6** (registration + bind/exec/init + comments; >= required 1).
- `grep -c '__sv_compute_create_from_yaml' src/parse.rs` = **2** (rewrite_yaml_file_create emits the helper-TF subquery + doc comment; >= required 1).
- `grep -c 'sv_compute_create_from_yaml_rust' src/ddl/alter_helpers_ffi.rs` = **4** (definition + 3 doc references; >= required 1).
- `grep -c 'catch_unwind' src/ddl/alter_helpers_ffi.rs` = **2** (in the function body + in doc comment; >= required 1).
- `grep -c 'json_merge_patch' src/parse.rs` = **5** (rewrite_alter_comment SET path + UNSET path + rewrite_yaml_file_create metadata wrapper + emit_native_create_sql metadata wrapper + comments; >= required 1).

## TECH-DEBT Surfaced

- **No new TECH-DEBT entries from Plan 04.** The committed-state-only read behaviour for `list_semantic_views` inside an open txn (TECH-DEBT 19 carry-over) was surfaced during Task 3 test authoring but is unchanged by this plan; the workaround (use direct `_definitions` SELECT inside the txn to verify the caller's-conn view) is documented inline in the test.
- **Cross-database ALTER and CREATE FROM YAML FILE (ATTACH 'db2'; ALTER db2.v).** The v0.9.0 extension only initializes `semantic_layer._definitions` on the LOAD database, so creating a semantic view inside an attached DB raises "schema 'semantic_layer' does not exist". This was attempted in Task 3 sqllogictest authoring and ruled out of Plan 04 scope. Documented as Phase 66 follow-up territory; NOT a Plan 04 regression (the pre-Plan-04 v0.9.0 behaviour was identical).

## Forward Pointers

- **Plan 05 (read-path migration wave):** `sv_register_table_function` is now in HEAD and ready to be consumed by the 17 read-side bind callbacks (per A2/A3 resolutions). The C++ Catalog API + per-call Connection(*context.db) primitive is empirically proven end-to-end through the YAML helper TF -- Plan 05 can re-use the exact same bind+exec+init shape. Plan 05's final commit retires H2 `query_conn` at src/lib.rs:498-507.
- **Plan 06 (lifecycle close-out):** parser_override has ZERO remaining OverrideContext-catalog consumers after Plan 04. `ctx.catalog.exists` remains the only Rust-side catalog access (rewrite_drop + rewrite_alter_rename + rewrite_alter_comment + emit_native_create_sql); Plan 06 either retires it entirely or replaces with an embedded `EXISTS (SELECT 1 FROM _definitions WHERE name = ?)` SQL pre-check. The H1 `catalog_conn` allocation at `src/lib.rs:386-410` is still PRESENT but is now truly unused by every parser_override write path. Plan 06 retires the allocation itself + the OverrideContext fields that wrap it.
- **Plan 06 read-side data-type inference (D-16):** Plan 03 left the persisted JSON with empty `column_type_names` and `fact.output_type`. Plan 05's read-side bind callbacks (using `sv_register_table_function` from Plan 04) will populate these on demand via the LIMIT 0 probe + per-fact typeof inference. After Plan 05, the phase29 / phase30 / phase39 test expectations revert to v0.9.0-style populated types.

## Task Commits

1. **Task 1 (Wave 0): test(65-04)** — `2ff494d` — json_merge_patch RFC-7396 null-as-delete sqllogictest spike (4 assertions PASS).
2. **Task 2 (Wave 1): feat(65-04)** — `40004e0` — sv_register_table_function C++ shim + __sv_compute_create_from_yaml helper TF + Rust FFI bridge + cpp/src/shim.hpp + alter_helpers_ffi module.
3. **Task 3 (Wave 2a): feat(65-04)** — `7ba997d` — ALTER SET/UNSET COMMENT migrated to pure-SQL json_merge_patch UPDATE; regression tests for SET/UNSET/RENAME.
4. **Task 4 (Wave 2b): feat(65-04)** — `124c3e6` — CREATE FROM YAML FILE migrated to __sv_compute_create_from_yaml subquery + metadata-via-SQL wrapper; 8-test integration suite.

## Self-Check: PASSED

- File `cpp/src/shim.hpp` -> FOUND.
- File `src/ddl/alter_helpers_ffi.rs` -> FOUND.
- File `test/sql/65_json_merge_patch_smoke.test` -> FOUND.
- File `test/sql/65_alter_comment_merge_patch.test` -> FOUND.
- File `test/sql/65_alter_rename_via_sql.test` -> FOUND.
- File `test/integration/test_create_from_yaml_v010.py` -> FOUND.
- File `.planning/phases/65-overridecontext-connection-teardown/65-04-SUMMARY.md` -> FOUND (this file).
- Commit `2ff494d` -> FOUND in git log.
- Commit `40004e0` -> FOUND in git log.
- Commit `7ba997d` -> FOUND in git log.
- Commit `124c3e6` -> FOUND in git log.
- `just test-sql` -> 52/52 PASS verified.
- `test_adbc_transactions.py` -> 6/6 PASS verified.
- `test_create_from_yaml_v010.py` -> 8/8 PASS verified.
- `cargo nextest run` -> 933/933 PASS verified.
- `cargo test --lib --features extension` -> 850/850 PASS verified.
- TEST_LIST contains all three new sqllogictest paths -> verified by inspection.
- `sv_register_table_function` defined in shim.cpp + declared in shim.hpp -> verified by grep.
- `__sv_compute_create_from_yaml` registered in shim.cpp + emitted by src/parse.rs -> verified by grep.
- `sv_compute_create_from_yaml_rust` defined in src/ddl/alter_helpers_ffi.rs -> verified by grep.
- `catch_unwind` wraps the Rust FFI body -> verified by grep.
- `json_merge_patch` emitted by src/parse.rs for both ALTER COMMENT and YAML metadata wrapper -> verified by grep.
