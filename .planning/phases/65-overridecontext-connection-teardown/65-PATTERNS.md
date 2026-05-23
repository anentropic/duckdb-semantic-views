# Phase 65: OverrideContext Connection Teardown — Pattern Map

**Mapped:** 2026-05-23
**Files analyzed:** ~30 (4 plans × ~6-12 files each)
**Analogs found:** 26 / 30 (4 files have no prior analog and adopt patterns from spike code in `65-READ-PATH-SPIKE.md` / `65-ALTER-REWRITE-SPIKE.md`)

> **Scope note.** Plans 03 / 04 / 05 / 06 share the same architectural backbone (revert → ALTER architecture → read-path migration → lifecycle close-out), so this PATTERNS.md groups by **plan** rather than by file. For each plan we list every file the planner must touch and pin it to the closest analog with concrete excerpts (file path + line range + verbatim snippet).
>
> Every analog cited is read-only context for the planner; this file does NOT modify any source.

---

## File Classification

| Plan | File | Role | Data Flow | Closest Analog | Match |
|------|------|------|-----------|----------------|-------|
| 03 | `src/parse.rs::OverrideContext` (lines 67-87) | parser hook struct | bind-time state | `git show 0d2c0b7^:src/parse.rs:39-72` (v0.9.0 shape) | exact (revert) |
| 03 | `src/parse.rs::sv_make_override_context` (2513-2557) | FFI entry | extension-load setup | `git show 0d2c0b7^:src/parse.rs::sv_make_override_context` | exact (revert) |
| 03 | `src/parse.rs::rewrite_drop` (2200-2249) | catalog rewriter | request-response | itself pre-Plan-02 (existing race-guard pattern at lines 2243-2248 stays) | exact |
| 03 | `src/parse.rs::rewrite_alter_rename` (2252-2311) | catalog rewriter | request-response | itself (race-guard at 2304) | exact |
| 03 | `src/parse.rs::rewrite_alter_comment` (2314-2383) | catalog rewriter | request-response | itself (lookup+mutate today) → Plan 04 `json_merge_patch` shape | role-match (mechanism upgrade) |
| 03 | `src/parse.rs::emit_native_create_sql` (1899-2038) | catalog rewriter | request-response | self (lines 2024-2035 CASE+error pattern survives; lines 1940-1994 ConnGuard blocks delete) | exact |
| 03 | `src/parse.rs::rewrite_yaml_file_create` (2051-2148) | catalog rewriter | file-I/O | `git show 0d2c0b7^:src/parse.rs::rewrite_yaml_file_create` (v0.9.0 CatalogReader-via-OverrideContext shape) | exact (revert) |
| 03 | `src/conn_guard.rs` (whole file) | utility / RAII | **DELETE** | `src/catalog.rs::PreparedStmt` (176-230) for RAII shape that was inspiration | n/a (deletion) |
| 03 | `src/ddl/define.rs::resolve_pk_from_catalog` (19-76) | catalog reader | **DELETE** | n/a (whole function deleted per D-05) | n/a |
| 03 | `src/ddl/define.rs::enrich_definition_for_create` (98-263) | catalog reader | request-response | self — strip §1 PK lookup, §5 metadata, §6 type infer, §7 fact typing; keep §2-§4 validation + §final serialize | partial (heavy slimming) |
| 03 | `cpp/src/shim.cpp::sv_register_parser_hooks` (353-407) | FFI shim | extension-load setup | `git show 0d2c0b7^:cpp/src/shim.cpp::sv_register_parser_hooks` (v0.9.0) | exact (revert) |
| 03 | `cpp/src/shim.cpp::SemanticViewsParserInfo` (159-181) | C++ owning struct | extension-load setup | v0.9.0 shape with INTENTIONAL LEAK comment | exact (revert) |
| 03 | `src/lib.rs::init_extension` (386-422) | extension init | extension-load setup | self — restore v0.9.0 `sv_register_parser_hooks(catalog_conn, is_file_backed)` call shape | exact (revert) |
| 03 | `test/sql/65_pk_error.test` (new) | sqllogictest | test | `test/sql/phase45_alter_comment.test` (negative-test shape) | role-match |
| 03 | `test/sql/65_metadata_via_sql.test` (new) | sqllogictest | test | `test/sql/phase39_metadata_storage.test` (metadata column assertions) | role-match |
| 04 | `cpp/src/shim.cpp::sv_register_table_function` (new — INTRODUCE per A2 resolution) | FFI shim | extension-load setup | `65-READ-PATH-SPIKE.md` lines 21-31 (the registration block) | template |
| 04 | `cpp/src/shim.cpp::__sv_compute_create_from_yaml` (new) | helper table function | request-response (bind reads file) | `65-ALTER-REWRITE-SPIKE.md` §"UPDATE-with-TF-subquery binds" + `65-READ-PATH-SPIKE.md` lines 35-39 (bind callback shape with `Connection probe(*context.db)`) | template |
| 04 | `src/parse.rs::rewrite_yaml_file_create` (2051-2148, second pass) | catalog rewriter | file-I/O | self (post-Plan-03 v0.9.0 shape) — replace `read_text` on CatalogReader with `INSERT INTO _definitions … SELECT new_def FROM __sv_compute_create_from_yaml(…)` outer shape | role-match |
| 04 | `src/parse.rs::rewrite_alter_comment` (2314-2383) | catalog rewriter | request-response | self — adopt `json_merge_patch` SQL shape (D-09 corrected by A1) | mechanism upgrade |
| 04 | new Rust FFI helper exported via `#[no_mangle] extern "C" fn sv_compute_create_from_yaml_rust(…)` | FFI bridge | request-response | `src/parse.rs::sv_parser_override_rust` (lines ~2620+) for FFI ptr+len convention + `sv_free_buffer` ownership transfer | role-match |
| 04 | `test/sql/65_json_merge_patch_smoke.test` (new — Wave 0 spike) | sqllogictest | test | `test/sql/phase45_alter_comment.test` (one-statement smoke shape) | role-match |
| 04 | `test/sql/65_alter_comment_merge_patch.test` (new) | sqllogictest | test | `test/sql/phase45_alter_comment.test` | exact role |
| 04 | `test/integration/test_create_from_yaml_v010.py` (new) | python integration | file-I/O | `test/integration/test_readonly_load.py` (PEP-723 + tempdir + open_writable pattern) | role-match |
| 05 | `cpp/src/shim.cpp::sv_register_table_function` (extension) | FFI shim | extension-load setup | itself from Plan 04 + `65-READ-PATH-SPIKE.md` lines 21-31 | template (extend) |
| 05 | new `cpp/src/shim.cpp::sv_register_scalar_function` (~30 LOC) | FFI shim | extension-load setup | `sv_register_table_function` sibling + DuckDB system_catalog scalar-function registration pattern | role-match |
| 05 | 17 read-side bind callbacks (`src/ddl/list.rs`, `describe.rs`, `show_*.rs`, `get_ddl.rs`, `read_yaml.rs`, `src/query/table_function.rs`, `src/query/explain.rs`) | bind-callback refactor | request-response | `src/ddl/list.rs::bind` (51-100) — current shape consuming `CatalogReader` from `bind.get_extra_info`; Plan 05 rewrites these to receive per-call `duckdb_connection` from the C++ shim | partial (registration mechanism swap) |
| 05 | new Rust `extern "C"` callback dispatchers (one per VTab) | FFI bridge | request-response | `src/parse.rs::sv_parser_override_rust` (FFI ptr+len + catch_unwind) for safety pattern | role-match |
| 05 | new process-local type-inference cache module (Rust) | utility | request-response | `src/catalog.rs::PreparedStmt` (cache-with-OnceLock shape lives in catalog already) — actual recommended shape: `OnceLock<RwLock<HashMap<(String, u64), Arc<InferredTypes>>>>` (RESEARCH §6.2) | new (no analog) |
| 05 | `src/lib.rs:498-507` (H2 `query_conn`) — final commit of Plan 05 | extension init | **DELETE** | self — delete the allocation block | n/a |
| 05 | `src/query/table_function.rs::try_infer_schema` (call sites at 600, 661) | catalog reader | request-response | self — function body unchanged; move call site from CREATE-time (`define.rs:179-188`) to bind time | exact reuse |
| 05 | `test/integration/test_concurrent_reads_per_call_conn.py` (new) | python integration | request-response | `test/integration/test_concurrent_ddl.py` (concurrent connections pattern) | role-match |
| 06 | `src/lib.rs:386-410` (H1 `catalog_conn` + `catalog_table_present` probe + `catalog_reader` local) | extension init | **DELETE** | self — delete the block; final lifecycle close-out | n/a |
| 06 | `src/parse.rs::OverrideContext` (final slim) | parser hook struct | bind-time state | self (Plan 03's post-revert v0.9.0 shape, now slimmed to drop `CatalogReader`/`Drop` impl; carries only `is_file_backed` or retired entirely) | partial |
| 06 | `cpp/src/shim.cpp::sv_register_parser_hooks` (signature update) | FFI shim | extension-load setup | self (post-Plan-03 v0.9.0 form) → drop `catalog_table_present` arg | partial |
| 06 | `tests/no_long_lived_conn.rs` (new) | structural rust test | test | RESEARCH §7.2 syn-AST-scan sketch (lines 572-609 of 65-RESEARCH.md) | template |
| 06 | `test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_semantic_view_select` (new) | python integration | request-response | `test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_fresh` (B1, lines 425-461) | exact role |
| 06 | `test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_describe` (new) | python integration | request-response | same B1 template + `describe_semantic_view` call | exact role |
| 06 | `test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_show_dimensions` (new) | python integration | request-response | same B1 template + `SHOW SEMANTIC DIMENSIONS` call | exact role |
| 06 | `test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_get_ddl` (new) | python integration | request-response | same B1 template + `get_ddl('v')` call | exact role |
| 06 | `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` (close LIFE-04) | ledger doc | docs | self (existing entry lines 62+; mark resolved with forward pointer to v0.10.0) | exact reuse |

---

## Pattern Assignments

### Plan 03 — parser_override slimming wave

#### `src/parse.rs::OverrideContext` (lines 67-87) — revert to v0.9.0

**Analog:** `git show 0d2c0b7^:src/parse.rs` lines 39-72. Current (Plan 02 partial) shape:

Current (`src/parse.rs:66-72`):
```rust
#[cfg(feature = "extension")]
pub struct OverrideContext {
    // PHASE-65-GUARD: do not reintroduce duckdb_connection or CatalogReader field here.
    pub db_handle: libduckdb_sys::duckdb_database,
    pub catalog_table_present: bool,
    pub is_file_backed: bool,
}
```

**Target (revert):**
```rust
#[cfg(feature = "extension")]
pub struct OverrideContext {
    pub catalog: crate::catalog::CatalogReader,
    pub is_file_backed: bool,
}

#[cfg(feature = "extension")]
impl Drop for OverrideContext {
    fn drop(&mut self) {
        // Phase 62 Q2 — INTENTIONAL LEAK of self.catalog.conn (the duckdb_connection).
        // [full comment from 65-RESEARCH.md §2]
    }
}
```

Plan 06 later re-slims this (retires the `CatalogReader` field once H1 is gone), but for Plan 03 the revert is byte-for-byte the v0.9.0 shape.

#### `src/parse.rs::sv_make_override_context` (2513-2557) — revert signature

**Analog:** `git show 0d2c0b7^:src/parse.rs::sv_make_override_context`.

**Current signature** (`src/parse.rs:2543-2547`):
```rust
pub unsafe extern "C" fn sv_make_override_context(
    db: libduckdb_sys::duckdb_database,
    catalog_table_present: bool,
    is_file_backed: bool,
) -> *mut std::ffi::c_void
```

**Target (revert):**
```rust
pub unsafe extern "C" fn sv_make_override_context(
    catalog_conn: libduckdb_sys::duckdb_connection,
    is_file_backed: bool,
) -> *mut std::ffi::c_void
```

#### `src/parse.rs::rewrite_drop` (2200-2249) — keep race-guard, drop ConnGuard caller

**Analog (self):** the function body already contains the correct race-guard pattern. Plan 03's revert removes the `catalog: &CatalogReader` arg by virtue of going back to passing a `CatalogReader` from `OverrideContext`, not opening a fresh `ConnGuard` per call.

**Race-guard pattern to preserve** (lines 2243-2248):
```rust
let guard = race_guard_select(name_escaped);
Ok(Some(format!(
    "{guard}; \
     DELETE FROM semantic_layer._definitions WHERE name = '{name_escaped}' \
     RETURNING name AS view_name"
)))
```

**`race_guard_select` shape** (`src/parse.rs:2190-2197`) — D-13 carries this forward unchanged:
```rust
fn race_guard_select(name_escaped: &str) -> String {
    format!(
        "SELECT CASE WHEN NOT EXISTS \
                   (SELECT 1 FROM semantic_layer._definitions WHERE name = '{name_escaped}') \
                THEN error('semantic view ''{name_escaped}'' was concurrently dropped') \
                ELSE TRUE END"
    )
}
```

#### `src/parse.rs::rewrite_alter_rename` (2252-2311) — keep dual-CASE pattern

**Analog (self):** lines 2301-2310 already emit the right shape with the race guard. Plan 03's revert restores the CatalogReader-based pre-check that runs on the OverrideContext's connection.

RESEARCH §1.1.6 also flags an alternate "combined CASE+error" pattern that folds both old-name-missing AND new-name-collision into one race guard:
```sql
SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM _definitions WHERE name = '<old>')
            THEN error('semantic view ''<old>'' was concurrently dropped')
            WHEN EXISTS     (SELECT 1 FROM _definitions WHERE name = '<new>')
            THEN error('semantic view ''<new>'' already exists')
            ELSE TRUE
       END;
UPDATE _definitions SET name = '<new>' WHERE name = '<old>' RETURNING ...
```

Planner picks (existing dual-pre-check vs combined CASE) based on simplicity. The v0.9.0 baseline used the dual-pre-check; Plan 03 reverts to that.

#### `src/parse.rs::rewrite_alter_comment` (2314-2383) — Plan 03 reverts; Plan 04 upgrades to `json_merge_patch`

**Plan 03 (revert step):** restore v0.9.0 lookup+mutate+reserialize shape using `catalog.lookup()` from OverrideContext.

**Current lookup+mutate body** (lines 2322-2358):
```rust
let json_str = catalog.lookup(&name).map_err(|e| ParseError {
    message: format!("catalog lookup failed: {e}"),
    position: None,
})?;
// ... deserialize, mutate def.comment, serialize ...
let new_json = serde_json::to_string(&def).map_err(|e| ParseError {
    message: format!("failed to serialize updated definition: {e}"),
    position: None,
})?;
```

**Plan 04 target (per D-09 + A1 resolution):** replace with pure SQL — no catalog read, no Rust-side serialize.
```sql
-- SET COMMENT
UPDATE semantic_layer._definitions
   SET definition = json_merge_patch(definition::JSON, '{"comment":"<escaped_new>"}'::JSON)::VARCHAR
 WHERE name = '<name_escaped>'
RETURNING name, 'comment set'::VARCHAR AS status;

-- UNSET COMMENT (RFC-7396 null-as-delete — Plan 04 Wave 0 spike verifies)
UPDATE semantic_layer._definitions
   SET definition = json_merge_patch(definition::JSON, '{"comment":null}'::JSON)::VARCHAR
 WHERE name = '<name_escaped>'
RETURNING name, 'comment unset'::VARCHAR AS status;
```

Wrap in the same `race_guard_select` two-statement pattern for plain ALTER (non-IF-EXISTS).

**Security note** (RESEARCH §11): the `<escaped_new>` must be JSON-escaped via `serde_json::to_string`, NOT manual string concat. The outer SQL literal then uses `escape_sql_arg` on top.

#### `src/parse.rs::emit_native_create_sql` (1899-2038) — drop ConnGuard blocks, keep CASE+error

**Existing CASE+error pattern (lines 2024-2035)** stays verbatim — this is the friendly "already exists" message path that runs on the caller's connection:
```rust
format!(
    "INSERT INTO semantic_layer._definitions (name, definition) \
     SELECT \
       CASE WHEN EXISTS (SELECT 1 FROM semantic_layer._definitions \
                         WHERE name = '{name_escaped}') \
            THEN error('semantic view ''{name_escaped}'' already exists; \
                        use CREATE OR REPLACE SEMANTIC VIEW to overwrite') \
            ELSE '{name_escaped}' \
       END, \
       '{enriched_escaped}' \
     RETURNING name AS view_name"
)
```

**Drop:** the two ConnGuard blocks at lines 1940-1969 and 1971-1994. Replace with a single call into `enrich_definition_for_create` that no longer takes a `conn` parameter (since type inference and metadata move to read-side / SQL respectively).

**RESEARCH §1.1.4 metadata-via-SQL upgrade:** instead of populating `created_on`/`database_name`/`schema_name` on the Rust struct, embed them in the INSERT via `json_merge_patch`:
```sql
INSERT INTO semantic_layer._definitions (name, definition)
SELECT
  '<name_escaped>',
  json_merge_patch(
    '<enriched_json_minus_metadata>'::JSON,
    json_object(
      'created_on', strftime(now(), '%Y-%m-%dT%H:%M:%SZ'),
      'database_name', current_database(),
      'schema_name', current_schema()
    )
  )::VARCHAR
RETURNING name AS view_name
```

The `now()` / `current_database()` / `current_schema()` resolve on the caller's connection — exactly what we want for transactional DDL.

#### `src/ddl/define.rs::enrich_definition_for_create` (98-263) — slim heavily

**Analog (self):** the function decomposes into 7 numbered steps in the docstring (lines 81-95). Plan 03 keeps §2 (cardinality inference), §3 (FK→ref-column check), §4 (graph validations) and the final `serde_json::to_string`. **Drop** §1 (`resolve_pk_from_catalog` call at line 105), §5 (metadata at 137-161), §6 (LIMIT 0 probe at 163-219), §7 (fact `typeof` at 221-260). After slimming the function takes only `(name: &str, def: SemanticViewDefinition)` — no `conn` arg, no `infer_types` arg.

**Excerpt of what survives** (lines 107-135):
```rust
// 2. Re-run cardinality inference now that catalog PKs are resolved.
crate::parse::infer_cardinality(&def.tables, &mut def.joins).map_err(|e| e.message)?;

// 3. Catch joins that still have FK columns but no resolved ref_columns.
for join in &def.joins {
    if !join.fk_columns.is_empty() && join.ref_columns.is_empty() {
        // ... D-06 hard error path replaces older soft-error message
    }
}

// 4. Graph validations.
crate::graph::validate_graph(&def)?;
crate::graph::validate_facts(&def)?;
crate::graph::validate_derived_metrics(&def)?;
crate::graph::validate_using_relationships(&def)?;
```

**D-06 error message** (per CONTEXT.md): replace the message at lines 119-124 with:
> `Table 'X' has no PRIMARY KEY declared but is referenced by FK in 'Y'. Add PRIMARY KEY (cols) or UNIQUE (cols) to the TABLES clause for X. (v0.10.0: physical-catalog PK auto-inference removed — see CHANGELOG.)`

#### `src/conn_guard.rs` — DELETE entire file

**Analog (deletion):** the RAII shape inside this file was patterned on `src/catalog.rs::PreparedStmt` (header comment at line 14-15 explicitly says so). Nothing under read-elimination consumes `ConnGuard`, so the file goes; the analog `PreparedStmt` stays alive for its own legitimate uses inside `CatalogReader`.

#### `cpp/src/shim.cpp::sv_register_parser_hooks` (353-407) — revert signature

**Current** (`cpp/src/shim.cpp:354-356`):
```cpp
bool sv_register_parser_hooks(duckdb_database db_handle,
                              bool catalog_table_present,
                              bool is_file_backed) {
```

**Target (v0.9.0 revert):**
```cpp
bool sv_register_parser_hooks(duckdb_connection catalog_conn,
                              bool is_file_backed) {
```

Inside the body: rebuild OverrideContext via `sv_make_override_context(catalog_conn, is_file_backed)` (the reverted 2-arg signature).

#### `cpp/src/shim.cpp::SemanticViewsParserInfo` (159-181) — restore INTENTIONAL LEAK comment

**Current** has Phase 65 comments saying the leak is moot (lines 173-179). **Target:** restore the v0.9.0 comment text per RESEARCH §2 (the INTENTIONAL LEAK rationale tied to `~DatabaseInstance` resetting `connection_manager` before `~SemanticViewsParserInfo`). Plan 06 deletes the comment entirely when H1 retires.

#### `src/lib.rs::init_extension` (386-422) — restore v0.9.0 call shape

**Target:**
```rust
let mut catalog_conn: ffi::duckdb_connection = ptr::null_mut();
let rc = unsafe { ffi::duckdb_connect(db_handle, &mut catalog_conn) };
if rc != ffi::DuckDBSuccess {
    return Err("Failed to create catalog connection".into());
}
// ... catalog_reader construction stays ...
let is_file_backed = db_path.as_ref() != ":memory:";
if !unsafe { sv_register_parser_hooks(catalog_conn, is_file_backed) } {  // ← 2-arg
    return Err("Failed to register parser hooks via C++ helper".into());
}
```

Note: `catalog_table_present` probe (lines 392-406) becomes a no-op consumer that builds CatalogReader for the read-side registrations (425-495 unchanged); the parser hook no longer receives the flag. Plan 06 deletes both the probe and the catalog_conn allocation.

#### `test/sql/65_pk_error.test` (new) — D-06 error message regression

**Analog:** `test/sql/phase45_alter_comment.test` (negative-test sqllogictest shape) and `test/sql/phase21_error_reporting.test`. Excerpt of error-assertion convention from `phase21_error_reporting.test`-style sqllogictest:
```
statement error
CREATE SEMANTIC VIEW v_bad AS TABLES (
  o AS orders,  -- no PRIMARY KEY
  i AS items REFERENCES o(order_id)  -- FK target lacks PK
) DIMENSIONS (...) METRICS (...)
----
Table 'o' has no PRIMARY KEY declared but is referenced by FK in 'i'
```

#### `test/sql/65_metadata_via_sql.test` (new) — metadata via SQL

**Analog:** `test/sql/phase39_metadata_storage.test` already asserts `created_on`/`database_name`/`schema_name` values; Plan 03's metadata-via-SQL upgrade must keep those assertions green byte-identical (the user-visible behavior is unchanged; only the mechanism of capture moves from Rust-side `execute_sql_raw` to SQL expressions inside the INSERT).

---

### Plan 04 — ALTER architecture wave

#### `cpp/src/shim.cpp::sv_register_table_function` (new — A2 resolution: INTRODUCE FROM SCRATCH)

**Analog (template):** `65-READ-PATH-SPIKE.md` lines 21-31 — the verbatim spike registration block that proved the C++ Catalog API path works:

```cpp
TableFunction tf("__sv_read_path_spike", {},
                 sv_read_path_spike_function,
                 sv_read_path_spike_bind,
                 sv_read_path_spike_init);
CreateTableFunctionInfo info(tf);
info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
auto &system_catalog = Catalog::GetSystemCatalog(db);
auto txn = CatalogTransaction::GetSystemTransaction(db);
system_catalog.CreateTableFunction(txn, info);
```

Plan 04 wraps this into a reusable `sv_register_table_function(db, name, args, bind_cb, exec_cb, init_cb)` shim. The spike proved (`READ-BIND-RC0`) that bind callbacks registered via this path receive a usable `ClientContext &` and that `Connection probe(*context.db)` opens cleanly with zero deadlock across 3 successive bind invocations.

#### `cpp/src/shim.cpp::__sv_compute_create_from_yaml` (new helper TF)

**Analog (template):** `65-RESEARCH.md` §1.2 sketch (lines 177-205):

```cpp
struct CreateFromYamlBindData : TableFunctionData {
  string file_path;
  string view_name;
  string comment;
  int    kind;        // 0=CREATE, 1=OR REPLACE, 2=IF NOT EXISTS
  string new_def;     // populated by bind
};

static unique_ptr<FunctionData> sv_create_from_yaml_bind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    auto bd = make_uniq<CreateFromYamlBindData>();
    bd->file_path = input.inputs[0].GetValue<string>();
    bd->view_name = input.inputs[1].GetValue<string>();
    bd->kind      = input.inputs[2].GetValue<int>();
    bd->comment   = input.inputs[3].GetValue<string>();

    Connection probe(*context.db);
    // 1. read_text the YAML file via probe.Query("SELECT content FROM read_text(?)")
    // 2. call into Rust to parse YAML → SemanticViewDefinition → enrich → serialize
    //    (Rust FFI helper exposed for this; reuses model::from_yaml_with_size_cap)
    // 3. populate bd->new_def
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("new_def");
    return std::move(bd);
}
```

**Connection-from-bind pattern:** RESEARCH §1.2 + `65-READ-PATH-SPIKE.md` line 37 — `Connection probe(*context.db);` constructed inside bind succeeds across all 3 invocations; dtor at end-of-scope completes without `connections_lock` deadlock. **This is the load-bearing primitive for both Plan 04 and Plan 05.**

**Outer INSERT shape** (RESEARCH §1.2):
```sql
INSERT INTO semantic_layer._definitions (name, definition)
SELECT '<name>',
       new_def
FROM __sv_compute_create_from_yaml('<path>', '<name>', <kind>, '<comment>')
ON CONFLICT (name) DO UPDATE SET definition = excluded.definition  -- for OR REPLACE
RETURNING name AS view_name
```

#### `src/parse.rs::rewrite_yaml_file_create` (2051-2148) — second-pass rewrite

**Analog (self):** Plan 03 reverts to v0.9.0 CatalogReader+read_text shape. Plan 04 replaces that with the outer INSERT-with-TF-subquery shape above. The sentinel-parsing prologue (lines 2062-2087) stays unchanged; the `read_text` path (2089-2098) and the `from_yaml_with_size_cap` parse path (2104-2146) move into the C++ bind callback (which dispatches into Rust via the FFI helper).

#### New Rust FFI helper for the YAML helper-TF bridge

**Analog:** `src/parse.rs::sv_parser_override_rust` (the existing ptr+len + `sv_free_buffer` ownership-transfer pattern). Same shape:

```rust
#[no_mangle]
pub unsafe extern "C" fn sv_compute_create_from_yaml_rust(
    path_ptr: *const c_char, path_len: usize,
    name_ptr: *const c_char, name_len: usize,
    comment_ptr: *const c_char, comment_len: usize,
    kind: u8,
    out_ptr: *mut *mut c_char,
    out_len: *mut usize,
    error_buf: *mut c_char,
    error_buf_len: usize,
) -> u8 {
    // catch_unwind, parse YAML via model::from_yaml_with_size_cap,
    // call slimmed enrich_definition_for_create, serialize, transfer
    // ownership of bytes to caller — caller releases via sv_free_buffer
}
```

#### Test files

**Analog for `test/sql/65_json_merge_patch_smoke.test` (Wave 0):** single-statement sqllogictest shape from `test/sql/peg_compat.test`:
```
require semantic_views

query T
SELECT json_merge_patch('{"a":1,"b":2}'::JSON, '{"b":null}'::JSON);
----
{"a":1}
```

**Analog for `test/sql/65_alter_comment_merge_patch.test`:** `test/sql/phase45_alter_comment.test` (the existing ALT-01 / ALT-02 cases). Same setup; new assertions only differ in the internal SQL shape (which Plan 04 emits via `json_merge_patch`). User-visible behavior is unchanged.

**Analog for `test/integration/test_create_from_yaml_v010.py`:** `test/integration/test_readonly_load.py` for PEP-723 metadata + tempdir + open_writable pattern. Excerpt of PEP-723 header convention:
```python
# /// script
# requires-python = ">=3.11"
# dependencies = ["duckdb==1.5.2"]
# ///
```

---

### Plan 05 — read-path migration wave

#### `cpp/src/shim.cpp::sv_register_table_function` (extend) + new `sv_register_scalar_function`

**Analog (template):** `65-READ-PATH-SPIKE.md` lines 21-31 for the table-function side (introduced by Plan 04). The scalar side uses `system_catalog.CreateFunction(txn, ScalarFunctionSet)` per RESEARCH §1.3 — same `system_catalog` + `txn` plumbing, different `Info` subtype. Planner confirms the scalar-side shape against postgres_scanner or another community extension during Plan 05 Wave 0.

#### 17 read-side bind-callback refactors

**Current shape** (`src/ddl/list.rs:51-100` — representative for the 6 table-function-with-extra-info siblings):
```rust
impl VTab for ListSemanticViewsVTab {
    type BindData = ListBindData;
    type InitData = ListInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind.add_result_column("created_on", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            // ... 5 more columns ...

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader.list_all().map_err(...)?;
            // ... build rows from entries ...
            Ok(ListBindData { rows })
        }))
    }
}
```

**Target shape (under C++ Catalog API + bridge):** the bind callback receives a per-call `duckdb_connection` from the C++ side (the `Connection(*context.db)` opened in the C++ bind), wraps a `CatalogReader::new(conn, true)` locally, calls the same `reader.list_all()`, then drops the connection at end of bind scope. The function body is byte-identical; only the source of `conn` changes.

**Per-call CatalogReader construction analog:** `src/parse.rs:1947`:
```rust
let catalog = crate::catalog::CatalogReader::new(guard.raw(), ctx.catalog_table_present);
```
(That code is being deleted in Plan 03, but the **shape** — `CatalogReader::new(conn, present)` per call — survives as the canonical "wrap a freshly-acquired connection in a CatalogReader" pattern Plan 05's bind callbacks will adopt.)

#### Rust↔C++ bridge callback dispatcher (Plan 05 Wave 0 spike)

**Analog:** `src/parse.rs::sv_parser_override_rust` for the FFI safety pattern (catch_unwind on Rust side, owned-buffer transfer via `sv_free_buffer`).

**Wave 0 spike per A3 resolution:** confirm Rust callbacks are reachable from a C++ bind callback registered via `sv_register_table_function`, and that `Connection(*context.db)` (or a `duckdb_connection` derived from it) is usable from the bridged Rust side. RESEARCH §1.3 Option A is the only viable path; Options B and C are ruled out.

#### Process-local type-inference cache (new module)

**No direct analog** — RESEARCH §6.2 specifies the shape from scratch:
```rust
static TYPE_CACHE: OnceLock<RwLock<HashMap<(String, u64), Arc<InferredTypes>>>>
    = OnceLock::new();
```

Key is `(view_name, schema_fingerprint)` where the fingerprint hashes the relevant fields of `SemanticViewDefinition` (table refs, dimensions, metrics, facts). ALTER invalidates naturally via JSON change → fingerprint change → cache miss → re-probe.

**Anti-pattern from TECH-DEBT 20:** must NOT be a bounded LRU. Phase 62 retired the LRU explicitly; do not re-introduce.

#### `src/query/table_function.rs::try_infer_schema` — reuse verbatim

**Analog (self):** `src/query/table_function.rs:892` already implements the LIMIT 0 probe. Call sites move from `define.rs:179-188` (CREATE-time) to the read-side bind callback (RESEARCH §6.2 sketch). The function body is unchanged.

#### `src/lib.rs:498-507` H2 retirement — final commit of Plan 05

**Excerpt (delete):**
```rust
// Create a NEW connection for the semantic_view table function.
let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
if rc != ffi::DuckDBSuccess {
    return Err("Failed to create query connection for semantic_view".into());
}
let query_state = QueryState {
    catalog: catalog_reader,
    conn: query_conn,
};
```

The `QueryState` struct (`src/query/table_function.rs:33-37`) also retires.

#### `test/integration/test_concurrent_reads_per_call_conn.py` (new)

**Analog:** `test/integration/test_concurrent_ddl.py` for the concurrent-Python-threads pattern. Same threading model; new assertion is "8 parallel SHOW SEMANTIC DIMENSIONS calls each open their own per-call connection without contention".

---

### Plan 06 — lifecycle close-out wave

#### `src/lib.rs:386-410` H1 retirement

**Excerpt (delete):**
```rust
let mut catalog_conn: ffi::duckdb_connection = ptr::null_mut();
let rc = unsafe { ffi::duckdb_connect(db_handle, &mut catalog_conn) };
if rc != ffi::DuckDBSuccess {
    return Err("Failed to create catalog connection".into());
}
// catalog_table_present probe (392-406) also deletes
let catalog_reader = crate::catalog::CatalogReader::new(catalog_conn, catalog_table_present);
```

After deletion, `sv_register_parser_hooks(db_handle, is_file_backed)` (Plan 03's reverted signature, minus catalog_table_present per RESEARCH §1.4).

#### `src/parse.rs::OverrideContext` — final slim

**Analog (self):** Plan 03's post-revert v0.9.0 shape. Plan 06 removes the `CatalogReader` field, the `Drop` impl, and the `catalog_table_present` field (the latter no longer plumbed in from Rust). Decision point per RESEARCH §1.4 Step 2: if both fields die, retire `OverrideContext` and `SemanticViewsParserInfo`'s `rust_state` field entirely. The parser-override hook then just needs `ParserExtensionInfo` with no Rust-side data.

#### `tests/no_long_lived_conn.rs` (new structural guard)

**Analog (template):** RESEARCH §7.2 (lines 572-609 of 65-RESEARCH.md). Verbatim sketch using `syn`:

```rust
use syn::{visit::Visit, ItemFn, ExprCall};

#[test]
fn init_extension_has_no_duckdb_connect_call() {
    let src = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");
    let file: syn::File = syn::parse_str(&src).expect("parse src/lib.rs");

    struct Finder { in_init_extension: bool, found: bool }
    impl<'ast> Visit<'ast> for Finder {
        fn visit_item_fn(&mut self, f: &'ast ItemFn) {
            let was = self.in_init_extension;
            if f.sig.ident == "init_extension" { self.in_init_extension = true; }
            syn::visit::visit_item_fn(self, f);
            self.in_init_extension = was;
        }
        fn visit_expr_call(&mut self, c: &'ast ExprCall) {
            if self.in_init_extension {
                if let syn::Expr::Path(p) = &*c.func {
                    if p.path.segments.last().map(|s| s.ident.to_string()).as_deref()
                        == Some("duckdb_connect") {
                        self.found = true;
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
    }

    let mut f = Finder { in_init_extension: false, found: false };
    f.visit_file(&file);
    assert!(!f.found, "init_extension contains a duckdb_connect call site. ...");
}
```

**Analog for build-graph integration:** any existing `tests/<name>.rs` (`tests/output_proptest.rs` referenced in `src/lib.rs:18` is the structural template — a top-level `tests/` integration test that compiles standalone).

#### 4 D-03b post-reopen tests in `test/integration/test_readonly_load.py`

**Analog:** `test_in_process_bootstrap_then_readonly_fresh` (B1, lines 425-461). The 4 new tests share the same prologue (bootstrap with `open_writable`, CREATE TABLE + CREATE SEMANTIC VIEW, close, gc, RO reopen under `_connect_with_watchdog`), differing only in the post-reopen call.

**B1 prologue verbatim** (lines 435-449) — the exact template to clone:
```python
with tempfile.TemporaryDirectory() as tmp:
    db = str(Path(tmp) / "fresh.duckdb")
    w = open_writable(db)
    w.execute("CREATE TABLE t (i INT)")
    w.execute(_minimal_create_sql("v"))
    w.close()
    del w
    gc.collect()

    ro, elapsed = _connect_with_watchdog(
        db,
        watchdog_seconds=5.0,
        read_only=True,
        config=_connect_config(),
    )
```

**Per-test divergence (RESEARCH §8):**
```python
# D-03b #1 — semantic_view() SELECT
rows = ro.execute(
    "SELECT * FROM semantic_view('v', dimensions := ['i'], metrics := ['s']) ORDER BY i"
).fetchall()

# D-03b #2 — describe_semantic_view()
rows = ro.execute("SELECT * FROM describe_semantic_view('v')").fetchall()

# D-03b #3 — SHOW SEMANTIC DIMENSIONS
rows = ro.execute("SHOW SEMANTIC DIMENSIONS FROM v").fetchall()

# D-03b #4 — get_ddl()
ddl = ro.execute("SELECT get_ddl('v')").fetchone()[0]
```

**run_test registration site:** lines 645-649 — append the 4 new tests to the `run_test(...)` call list inside `main()`.

#### LIFE-04 ledger close in `deferred-items.md`

**Analog (self):** `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` already has the entry "In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)" at line 62+. Plan 06 marks it resolved with a forward pointer to v0.10.0 / Phase 65. Pure docs edit.

---

## Shared Patterns

### Connection-from-bind (load-bearing for Plans 04 + 05)

**Source:** `65-READ-PATH-SPIKE.md` line 37 + RESEARCH §1.2/§1.3.
**Apply to:** every helper-TF bind callback (Plan 04 `__sv_compute_create_from_yaml`) and every read-side TF/scalar bind (Plan 05's 17 callbacks).

```cpp
static unique_ptr<FunctionData> sv_<name>_bind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    auto bd = make_uniq<MyBindData>();
    Connection probe(*context.db);  // ← the load-bearing primitive
    auto result = probe.Query("SELECT ...");
    // populate bd, return_types, names
    return std::move(bd);
}
```

**Empirical evidence:** `READ-BIND-RC0` (3/3 bind invocations succeed without deadlock).
**Anti-pattern:** `duckdb_connect(db_handle)` C-API path (`BIND-THREAD-RC1` = rc=1 every time).

### Race-guard two-statement pattern (D-13 unchanged)

**Source:** `src/parse.rs::race_guard_select` (lines 2190-2197); existing call sites in `rewrite_drop`, `rewrite_alter_rename`, `rewrite_alter_comment`.
**Apply to:** every DROP / ALTER rewrite in Plans 03 and 04. Plan 04's `json_merge_patch`-based ALTER COMMENT keeps this guard.

```rust
let guard = race_guard_select(name_escaped);
format!("{guard}; <DELETE-or-UPDATE-with-RETURNING>")
```

Pinned by `src/parse.rs::tests::race_guard_select_emits_not_exists_and_error` (2950).

### FFI ptr+len + sv_free_buffer ownership transfer

**Source:** `src/parse.rs::sv_parser_override_rust` + `cpp/src/shim.cpp::SvOwnedBuffer` (lines 128-157).
**Apply to:** every new Rust FFI helper that returns a heap-owned UTF-8 string to C++ (Plan 04's `sv_compute_create_from_yaml_rust`, any Plan 05 bridge dispatchers).

Conventions:
- Rust returns `(ptr, len)` pair; buffer is NOT NUL-terminated; caller reads exactly `len` bytes.
- C++ wraps with `SvOwnedBuffer` (RAII) so the buffer releases even if downstream `Parser::ParseQuery` throws.
- Release via `sv_free_buffer(ptr, len)` — exact pair Rust returned.

### `catch_unwind` on every FFI boundary

**Source:** `src/parse.rs::sv_make_override_context` (lines 2548-2555) — `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { ... }))`.
**Apply to:** every `#[no_mangle] extern "C" fn` in Plans 04 and 05. Panics across FFI boundaries are UB.

### `escape_sql_arg` for SQL literal escaping

**Source:** `src/parse.rs:2170-2172`.
**Apply to:** every embedded VARCHAR in Plan 03/04's emitted SQL (view names, comment values, JSON literals, file paths). The pair `escape_sql_arg` / `unescape_sql_arg` is the canonical doubling-quote convention.

**JSON-specific:** when embedding JSON inside a single-quoted SQL literal (e.g. the `json_merge_patch('{...}'::JSON, ...)` shape), the JSON itself must be built via `serde_json::to_string` (handles internal `"` escaping); then `escape_sql_arg` doubles any embedded single quotes for the outer SQL literal.

### Watchdog test pattern (Plan 06's 4 new tests)

**Source:** `test/integration/test_readonly_load.py::_connect_with_watchdog` + B1's `test_in_process_bootstrap_then_readonly_fresh` (425-461).
**Apply to:** every D-03b test. The watchdog wraps the RO reopen at a 5 s budget so a regression fails fast rather than hanging the test runner. The 4 new tests must also wrap the post-reopen call in an analogous watchdog (or assert elapsed < 5.0 on the reopen and rely on the per-test assertion to surface any post-reopen hang quickly).

### `cargo test` / `just test-sql` / `just test-all` gates

**Source:** `CLAUDE.md` Quality Gate section (top of file).
**Apply to:** every plan-end verification. Per CLAUDE.md, **a phase verification that only runs `cargo test` is incomplete** — sqllogictest covers integration paths Rust tests do not. Each Plan ends with `just test-all` green; Plan 06 additionally runs `just ci` before phase complete.

---

## No Analog Found

Files with no close match in the codebase (planner uses RESEARCH.md sketches + spike code as starting templates):

| File | Plan | Reason |
|------|------|--------|
| `cpp/src/shim.cpp::sv_register_table_function` (new) | 04 | A2 resolution: not in HEAD. Template = `65-READ-PATH-SPIKE.md` lines 21-31. |
| Process-local type-inference cache module (Rust) | 05 | First read-side cache in the extension; pattern from RESEARCH §6.2. Phase 61's bounded-LRU prior art is explicit anti-pattern. |
| Rust↔C++ bridge callback dispatchers (one per VTab) | 05 | New FFI surface for routing C++ bind callbacks to Rust function bodies. Plan 05 Wave 0 spike chooses the bridge mechanism. |
| `tests/no_long_lived_conn.rs` (new structural guard) | 06 | First syn-based AST scan in the project. Template = RESEARCH §7.2. |

---

## Metadata

**Analog search scope:**
- `src/` (full tree — parse.rs, lib.rs, conn_guard.rs, catalog.rs, ddl/*, query/*)
- `cpp/src/shim.cpp`
- `test/sql/` (47 sqllogictest files)
- `test/integration/test_readonly_load.py`, `test_concurrent_ddl.py`, `test_adbc_transactions.py`
- `.planning/phases/65-overridecontext-connection-teardown/65-READ-PATH-SPIKE.md`
- `.planning/phases/65-overridecontext-connection-teardown/65-ALTER-REWRITE-SPIKE.md`
- `.planning/phases/65-overridecontext-connection-teardown/65-OPTION-B-SPIKE.md`
- `.planning/phases/62-caret-restoration-lru-removal/` (`OverrideContext` attachment to `SemanticViewsParserInfo`)
- `.planning/milestones/v0.8.0-phases/58-transactional-ddl/` (parser_override rewrite-to-native-SQL pattern)
- `.planning/milestones/v0.8.0-phases/60-race-guards-validation-hardening/` (race-guard two-statement pattern)
- `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md`

**Files scanned:** ~50
**Pattern extraction date:** 2026-05-23

**Key patterns identified:**
- Plan 03 is **dominated by revert operations** — every parser_override file change has a v0.9.0 git-history analog. The planner can `git revert 0d2c0b7` + `git revert f9caafe` and let the diff land 90% of Plan 03 mechanically. Only the metadata-via-SQL upgrade in `emit_native_create_sql` and the D-06 hard-error replacement of `resolve_pk_from_catalog`'s caller are net-new code.
- Plan 04 is **smaller than Plan 03 once A7 is honored** — only 3 ALTER variants (RENAME / SET COMMENT / UNSET COMMENT) get the json_merge_patch treatment, plus CREATE FROM YAML FILE via the new helper TF. The 8 other variants previously enumerated in CONTEXT's `<specifics>` table are dropped as non-features (Snowflake parity).
- Plan 05 has the **highest blast radius** — 17 read-side bind callbacks plus the Rust↔C++ bridge. Migration is incremental within the plan; the final commit retires H2.
- Plan 06 is **the smallest by LOC but the highest in stakes** — the 4 D-03b tests + structural guard test are the LIFE-01 / criterion-3 / criterion-4 acceptance evidence. Watchdog tests (B1..B4 + B11) must flip green here.

**Cross-cutting invariants every plan honors:**
- D-21 transactional DDL non-negotiable — `test/integration/test_adbc_transactions.py` stays green throughout.
- D-22 bounded scope — adjacent issues surface as TECH-DEBT / Phase 66 items, not silently absorbed.
- D-23 root-cause — no access-mode-mismatch detect-and-error fallback.
- D-24 no time pressure — extend scope before contracting it.

**Ready for Planning.** Pattern mapping complete. Planner can reference analog patterns + concrete excerpts in each Plan 03 / 04 / 05 / 06 PLAN.md.
