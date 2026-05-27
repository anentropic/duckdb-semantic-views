# Phase 65: OverrideContext Connection Teardown — Research

**Researched:** 2026-05-23
**Domain:** DuckDB extension lifecycle / parser_override / C++ Catalog API table-function registration / read-elimination architecture
**Confidence:** HIGH for verified-from-source items (file paths, line ranges, current code state, JSON schema, ALTER variants present today, registration counts); MEDIUM for some upstream-DuckDB call-shape claims (cross-checked against spike evidence + amalgamation snippets quoted in CONTEXT); LOW only for a single CONTEXT-stated mechanism (`json_set` / `json_remove`) which DuckDB does NOT ship — see §1.3 and §4 for the correction the planner must adopt.

## Summary

The architecture is locked by `65-CONTEXT.md` and the spike trio (`OPTION-B`, `READ-PATH`, `ALTER-REWRITE`). Plans 03–06 are mechanically straightforward in concept but have several implementation-level surprises that the planner needs to surface explicitly before tasks are written:

1. **The current code state is a Plan 02 partial commit, NOT v0.9.0.** Commits `0d2c0b7` and `f9caafe` already mutated `OverrideContext` to the `db_handle + flags` shape and wired `ConnGuard` into every `rewrite_*` site. Plan 03's "revert" is a literal `git revert` (or hand-rolled equivalent) of those two commits — there is NO new code to write for D-01; the shape comes back from history. The lines in the locked CONTEXT (`src/parse.rs:67-87`, `:1788-2100`, `:2513-2554`) refer to the *current* (Plan 02 partial) state, which after the revert returns to the v0.9.0 shape documented in §2 below.
2. **D-09's `json_set` / `json_remove` shapes are NOT available in DuckDB v1.5.2.** DuckDB JSON extension has `json_extract`, `json_extract_string`, `json_transform`, `json_merge_patch`, `json_keys`, `json_array`, `json_object`, etc. — but no `json_set` or `json_remove`. The planner MUST replace D-09's example rewrites with `json_merge_patch`-based equivalents (see §4.1). This is the only place where CONTEXT decisions cannot be implemented as written; planner's discretion (per CONTEXT's open Discretion item on JSON path strings) covers this correction.
3. **The ALTER variant inventory in CONTEXT's `<specifics>` table is aspirational, not current.** Only 3 ALTER variants exist in `src/parse.rs::rewrite_alter` today (RENAME TO, SET COMMENT, UNSET COMMENT). The 8 other variants listed (MAKE PRIVATE/PUBLIC, SET TAG, ADD SYNONYMS, ADD DIMENSION/METRIC/FACT, DROP DIMENSION/METRIC/FACT/RELATIONSHIP, ADD RELATIONSHIP) are **new surface** that Plan 04 would have to introduce both at the parser layer (extend `rewrite_alter`) and at the rewriter layer. **Planner decision needed:** does Plan 04 actually ship all 11, or does it ship the rewrite-mechanism for the existing 3 (lifecycle goal) + a clean extensibility hook for future plans to add the other 8? Per D-22 (bounded scope) and D-24 (no time pressure), recommendation is: implement the 3 existing variants under the new mechanism in Plan 04; flag the other 8 as a v0.10.x or v0.11 follow-up (see §5).
4. **The read-side inventory in CONTEXT is 12 but the actual registration count is 17** (15 table funcs + 2 scalars). The CONTEXT count folds `show_X` and `show_X_all` siblings into one "logical" entry. See §3 — the planner needs the actual 17 to scope Plan 05 correctly.

**Primary recommendation:** plan Plan 03 as a near-pure revert of commits `0d2c0b7`+`f9caafe` plus three additive changes (delete `src/conn_guard.rs`, delete `src/ddl/define.rs::resolve_pk_from_catalog`+caller, restructure `enrich_definition_for_create` so metadata capture moves to SQL expressions in the INSERT). Plan 04 lives or dies on `json_merge_patch` ergonomics — adopt that mechanism for ALL ALTER variants regardless of whether the variant needs catalog reads (the helper-TF path applies only when catalog access is unavoidable). Plan 05 migrates all 17 read-side registrations in any order, with `semantic_view` last (highest blast radius). Plan 06 deletes the H1 `catalog_conn` allocation in `src/lib.rs:386-410` (the H2 `query_conn` retired in Plan 05 already), adds the structural guard test, and extends the watchdog suite per D-03b.

## User Constraints

(Copied verbatim from `65-CONTEXT.md` to preserve provenance for the planner.)

### Locked Decisions

All decisions D-01 through D-24 from CONTEXT.md are locked. The most load-bearing ones:

- **D-01** Hard-revert Plan 02 partial commits (`0d2c0b7` + `f9caafe`). `OverrideContext` returns to v0.9.0 shape; `INTENTIONAL LEAK` comment temporarily restored (Plan 06 retires H1 entirely so the comment becomes moot).
- **D-02** Delete `src/conn_guard.rs` entirely. No Rust consumer materializes under read-elimination.
- **D-03 / D-03b** Plan 01 watchdog tests (B1..B4 + B11) kept intact; Plan 06 adds 4 new post-reopen variants (semantic_view SELECT, describe, SHOW DIMENSIONS, get_ddl).
- **D-04** Keep `sv_register_table_function` C++ Catalog API shim from Plan 02 partial. Surviving infrastructure for Plans 04 and 05.
- **D-05** Delete `src/ddl/define.rs::resolve_pk_from_catalog` and its caller at `define.rs:105`.
- **D-06** Hard error at CREATE / ALTER for missing PK + FK reference, with the actionable message shape stated in CONTEXT §<decisions>.
- **D-07** Existing persisted definitions written under v0.9.0 with inferred PKs continue to load + query + introspect on v0.10.0 (validation triggers only on re-CREATE/ALTER).
- **D-09** Pure-SQL `json_set`/`json_remove` UPDATE rewrites for trivial ALTER variants — **mechanism correction required: see §1.3 + §4.1, DuckDB has no `json_set` or `json_remove`; use `json_merge_patch` and a regenerate-via-helper-TF for removals**.
- **D-10** Helper TFs for ALTER variants needing catalog reads or YAML parsing. `__sv_compute_<op>` naming convention (D-12); one helper per variant that needs one.
- **D-11** `CREATE FROM YAML FILE` uses `__sv_compute_create_from_yaml(path, opts)` helper-TF.
- **D-13** Phase 60 two-statement race-guard pattern carries forward unchanged.
- **D-14** Migrate all read-side table-function callbacks to the C++ Catalog API shim. **The actual count is 15 table funcs + 2 scalars = 17**, not 12 as CONTEXT states (CONTEXT folds the `_all` siblings together).
- **D-15** Read-path migration may be incremental; H2 `query_conn` retirement is the final atomic step in Plan 05.
- **D-16** Type inference defers to read-side bind on demand.
- **D-17** No persisted type cache in `_definitions`.
- **D-18** 4-plan structure (03 = parser_override slimming, 04 = ALTER architecture, 05 = read-path migration, 06 = lifecycle close-out).
- **D-19** Plan numbering continues from 03 (01 + 02-PARTIAL landed).
- **D-21** Transactional DDL semantics non-negotiable (Phase 58 ADBC tests stay green throughout).
- **D-22** Bounded scope + signal surfacing.
- **D-23** Root-cause over symptom hacks.
- **D-24** No time pressure.

### Claude's Discretion (from CONTEXT)

- Process-local type-inference cache shape (D-16). Recommended: see §6.2.
- Exact JSON path strings for D-09. **Verdict:** DuckDB has no `json_set` — use `json_merge_patch` ; see §4.1.
- Whether Plan 04 helper-TF lives in a new `cpp/src/alter_helpers.cpp` or extends `cpp/src/shim.cpp`. Recommended: see §5.4.
- Structural-guard-test mechanism. Recommended: see §7.
- Test layout for Plan 04 ALTER coverage. Recommended: see §10.

### Deferred Ideas (OUT OF SCOPE — Phase 66 or later)

- EXPAND-CTX-01..03 verification (qualify_and_quote_table_ref wiring across non-main expand paths).
- ADBC query-test harness (`test/integration/test_adbc_queries.py`).
- CHANGELOG `## [0.10.0]` section, Cargo.toml + description.yml version bump.
- `_notes/error_with_adbc.md` cleanup.
- TECH-DEBT #19 / #21 / #23 / #24 — carry-overs unchanged.
- RO→RW reverse direction hang (B4) — expected to flip green as side-effect of H1+H2 retirement; if not, Phase 66 follow-up.
- 8 future ALTER variants beyond RENAME/SET COMMENT/UNSET COMMENT (MAKE PRIVATE/PUBLIC, SET TAG, ADD SYNONYMS, ADD DIMENSION/METRIC/FACT, DROP DIMENSION/METRIC/FACT/RELATIONSHIP, ADD RELATIONSHIP). These don't exist in code today; CONTEXT's table treats them as in-scope but per D-22 the planner should defer them.

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| LIFE-01 | After RW close+drop, `duckdb.connect(path, read_only=True)` returns within 5s in same process | §9 (H1 + H2 retirement removes both `shared_ptr<DatabaseInstance>` holders the extension owned); §11 (B1/B2 watchdog tests + 4 new D-03b variants are the empirical evidence) |
| LIFE-02 | Fix is deterministic teardown OR access-mode detect+error (root cause path mandatory per D-23) | §9 + §1.1 (the read-elimination architecture IS the deterministic-teardown path: per-call connections in C++ bind callbacks, no long-lived state) |
| LIFE-03 | `test/integration/test_readonly_load.py` gains in-process bootstrap-then-RO scenario under watchdog | Plan 01 already landed B1/B2/B3/B4/B11; Plan 06 adds the 4 D-03b variants on top (§11) |
| LIFE-04 | Phase 63 `deferred-items.md` updated with resolution + v0.10.0 pointer | Plan 06 owns this — pure docs edit, no code |

---

## 1. Implementation Map per Plan

### 1.1 Plan 03 — parser_override slimming wave

**End state:** `parser_override` has zero catalog reads. H1 `catalog_conn` is still allocated (Plan 06 retires) but is unused by any code path. `just test-sql` is green again (regression from Plan 02 partial's 4/47 fixed).

**Tasks (suggested grouping for planner):**

1. **Hard-revert two commits** (D-01):
   - `git revert 0d2c0b7` — restores `OverrideContext` to v0.9.0 shape (CatalogReader field + INTENTIONAL LEAK Drop impl).
   - `git revert f9caafe` — restores `sv_register_parser_hooks` to v0.9.0 signature (`duckdb_connection, bool is_file_backed`) in both `cpp/src/shim.cpp` and the `extern "C"` block in `src/lib.rs:498-507` neighborhood.
   - **Order:** revert `f9caafe` (the C++ shim) BEFORE `0d2c0b7` (Rust). Reverting Rust first would leave the C++ shim trying to call a non-existent `sv_make_override_context(duckdb_database, bool, bool)` signature.
   - **Conflict expected** with surviving Plan 02 commit that landed `sv_register_table_function` — the planner needs to keep that infrastructure (D-04) while reverting only the parser_override-side changes.
   - Verification after revert: `cargo build --features extension --no-default-features` clean; `just build` succeeds; `just test-sql` returns to v0.9.0 47/47 PASS.

2. **Delete `src/conn_guard.rs`** (D-02): 196 LOC file removed. `pub mod conn_guard;` line in `src/lib.rs:3` removed.

3. **Delete `src/ddl/define.rs::resolve_pk_from_catalog`** (D-05): lines 19-76 (the function). Caller at line 105 (inside `enrich_definition_for_create`) becomes `// PK auto-inference removed — see D-05/D-06`. Inside `src/parse.rs::infer_cardinality` the comment at `parse.rs:1518-1523` ("At bind time, resolve_pk_from_catalog will attempt to fill in pk_columns…") becomes incorrect — replace with the D-06 hard-error path. Test at `src/parse.rs:3860-3874` (`skips_when_target_has_no_pk_and_no_explicit_ref`) becomes incorrect — replace with a test that asserts the D-06 error fires (see §1.1.5 below).

4. **Restructure `enrich_definition_for_create`** so metadata moves to SQL (the load-bearing part of "parser_override has zero catalog reads"):
   - Today (`src/ddl/define.rs:138-161`) it issues `SELECT strftime(now(), '...'), current_database(), current_schema()` via `execute_sql_raw(conn, ...)` and populates `def.created_on / def.database_name / def.schema_name` BEFORE serializing. This is exactly what consumes the catalog connection at parser_override time and must move.
   - After change: `enrich_definition_for_create` no longer touches `conn`. Metadata fields are left as `None` in the serialized JSON (or in-fact removed from the `SemanticViewDefinition` struct — see decision point below). The generated INSERT becomes (in `emit_native_create_sql`):
     ```sql
     INSERT INTO semantic_layer._definitions (name, definition)
     SELECT
       CASE WHEN EXISTS (...) THEN error(...) ELSE '<name_escaped>' END,
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
   - This runs on the caller's connection so the `now()` / `current_database()` / `current_schema()` resolve in their context.
   - **Decision point for planner:** also remove the `created_on / database_name / schema_name` fields from the `SemanticViewDefinition` Rust struct? Or just leave them None at serialize time and let `json_merge_patch` patch them in? The latter is less invasive (no model.rs change, no migration concerns for stored JSON) and is recommended.
   - Type inference (`define.rs:163-219`, the LIMIT 0 probe + per-column type lookup) ALSO consumes `conn`. **D-16 says defer this to read-side bind.** So Plan 03 removes this block ENTIRELY from `enrich_definition_for_create`. After removal: `column_type_names` / `column_types_inferred` fields stay None / empty in stored JSON; read-path bind callbacks (under Plan 05) re-probe and cache process-locally.
   - Fact type inference (`define.rs:221-260`, `typeof(expr) FROM <table> LIMIT 1`) ALSO consumes `conn`. Same fate: remove from CREATE-time, defer to read-side. Fact `output_type` stays None at CREATE; SHOW SEMANTIC FACTS / DESCRIBE bind callback computes it lazily.

5. **Delete second per-call ConnGuard in `emit_native_create_sql`** (`src/parse.rs:1940-1969` and `:1971-1994`): both blocks become irrelevant once `enrich_definition_for_create` no longer takes a `conn` parameter. The existence pre-check goes too — D-13 says fold into ON CONFLICT semantics. So the existence pre-check block (`parse.rs:1940-1969`) is deleted; `emit_native_create_sql` becomes a pure-Rust function (no FFI) that just serializes the JSON and emits the INSERT shape above with the embedded `json_merge_patch` for metadata.
   - **Subtle:** the IF NOT EXISTS branch today emits `SELECT '...' WHERE 1 = 0` (a 0-row sentinel) for the "already exists" case. Without the pre-check, that branch can't tell whether the row exists at parser_override time. **Replacement:** use `INSERT OR IGNORE` which silently absorbs PK conflict and emits the same 0-row schema via `RETURNING name AS view_name` (already in the current code at `parse.rs:2017-2022`). The CONTEXT.md's "fold existence checks into INSERT … ON CONFLICT" decision is exactly this — the IF NOT EXISTS pre-check disappears.
   - **Plain CREATE (not OR REPLACE, not IF NOT EXISTS):** the existing CASE+error() path at `parse.rs:2024-2035` is retained verbatim — it raises the friendly "already exists" message at execution time via `error()` on the caller's connection (no parser_override catalog read needed). Good news: this code already works without a pre-check on the parser_override side.

6. **Restructure `rewrite_drop_or_alter` / `rewrite_drop` / `rewrite_alter_rename` / `rewrite_alter_comment`** so they have NO catalog access:
   - DROP today (`parse.rs:2200-2249`) does `catalog.exists(&name)` and chooses between silent-no-op SELECT (IF EXISTS) vs error vs DELETE-with-race-guard. The race-guard pattern from D-13 ALREADY handles the missing case on the caller's connection via the `SELECT CASE WHEN NOT EXISTS THEN error() …; DELETE … RETURNING` two-statement sequence. So `parse.rs:2200-2249` collapses to: always emit the two-statement form for plain DROP, and emit just the `DELETE … RETURNING` (without the guard SELECT) for `DROP IF EXISTS`. No catalog read needed.
   - ALTER RENAME today (`parse.rs:2252-2311`) does TWO catalog reads — `catalog.exists(&old_name)` and `catalog.exists(&new_name)`. The first becomes the race-guard pattern (same as DROP). The second (checking destination collision) is harder — there's no built-in race-guard for "destination must not exist". Workaround: rely on the `name VARCHAR PRIMARY KEY` constraint on `_definitions` to raise a PK violation when the new name collides. The error message is generic ("Constraint Error: Duplicate key") but it's correct. To preserve the friendly message ("semantic view 'X' already exists"), wrap with another CASE+error() pattern that runs on the caller's connection:
     ```sql
     SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM _definitions WHERE name = '<old>')
                 THEN error('semantic view ''<old>'' was concurrently dropped')
                 WHEN EXISTS     (SELECT 1 FROM _definitions WHERE name = '<new>')
                 THEN error('semantic view ''<new>'' already exists')
                 ELSE TRUE
            END;
     UPDATE _definitions SET name = '<new>' WHERE name = '<old>' RETURNING ...
     ```
   - ALTER SET/UNSET COMMENT today (`parse.rs:2314-2383`) does `catalog.lookup(&name)`, deserializes the JSON, mutates `def.comment`, re-serializes, and emits an UPDATE with the new JSON. This is the case D-09 was designed for — replace the lookup+mutate+reserialize with a pure-SQL `json_merge_patch` rewrite. See §4.1 for the exact shape.

7. **`rewrite_yaml_file_create` connection use** (`parse.rs:2051-2148`): today this opens a per-call ConnGuard to run `SELECT content FROM read_text('<path>')` on the catalog connection. **Migration:** rather than executing `read_text` from the parser_override side, embed `read_text(...)` directly into the INSERT shape OR move the whole YAML-read-and-parse into a helper TF (D-11's `__sv_compute_create_from_yaml`). The latter is what CONTEXT mandates and is cleaner because it ALSO removes the YAML parsing + cardinality inference from parser_override (today at `parse.rs:2130-2146`). **Plan 04 owns this migration**, not Plan 03. Plan 03 leaves `rewrite_yaml_file_create` calling its ConnGuard temporarily — the function still works after Plan 03's revert because conn_guard.rs is gone but parse.rs imports `crate::conn_guard::ConnGuard`. **Sequencing problem:** can Plan 03 delete `conn_guard.rs` if `rewrite_yaml_file_create` still uses it? Two options for the planner:
   - **(a)** Plan 03 reverts to v0.9.0 shape where `rewrite_yaml_file_create` uses the OverrideContext's CatalogReader (no ConnGuard) — natural under D-01's revert. The CatalogReader connection that gets used here is the long-lived `catalog_conn` (H1), so reads happen via the long-lived handle until Plan 04 migrates YAML CREATE to the helper-TF path. This matches the locked plan ordering.
   - **(b)** Plan 03 keeps a stub ConnGuard for `rewrite_yaml_file_create` only, deletes it in Plan 04 when YAML migration lands. Messier — option (a) is cleaner.
   - **Recommendation:** option (a). Plan 03 reverts cleanly to v0.9.0 (where `rewrite_yaml_file_create` consumed the OverrideContext-owned conn for read_text), conn_guard.rs is deleted. Plan 04 then deletes the v0.9.0 read_text-via-CatalogReader path when it lands the `__sv_compute_create_from_yaml` helper TF.

**Lines/files affected in Plan 03:**

| File | Lines | Change |
|------|-------|--------|
| `src/parse.rs` | 67-87 | Revert to v0.9.0 OverrideContext shape (§2) |
| `src/parse.rs` | ~1518-1523 | Replace "defer to bind time" comment with D-06 hard-error |
| `src/parse.rs` | 1788-1840 (`rewrite_drop_or_alter`) | Remove ConnGuard::open; rewrite_drop/alter_* take no catalog arg |
| `src/parse.rs` | 1899-2038 (`emit_native_create_sql`) | Drop both ConnGuard blocks; embed metadata via SQL expressions |
| `src/parse.rs` | 2040-2148 (`rewrite_yaml_file_create`) | Revert to v0.9.0 CatalogReader-via-OverrideContext shape |
| `src/parse.rs` | 2200-2249 (`rewrite_drop`) | Remove `catalog: &CatalogReader` arg; emit race-guard-based DELETE only |
| `src/parse.rs` | 2252-2311 (`rewrite_alter_rename`) | Remove catalog arg; combined CASE+error+UPDATE per §1.1.6 |
| `src/parse.rs` | 2314-2383 (`rewrite_alter_comment`) | Use `json_merge_patch` (§4.1) instead of lookup+mutate+reserialize |
| `src/parse.rs` | 2513-2557 (`sv_make_override_context`) | Revert signature to v0.9.0 `(duckdb_connection, bool)` |
| `src/parse.rs` | 3860-3874 | Replace `skips_when_target_has_no_pk_and_no_explicit_ref` test with D-06 error assertion |
| `src/conn_guard.rs` | 1-196 (whole file) | DELETE |
| `src/lib.rs` | 3 | `pub mod conn_guard;` line removed |
| `src/lib.rs` | 386-422 | Restore v0.9.0 `sv_register_parser_hooks(catalog_conn, is_file_backed)` call shape |
| `src/ddl/define.rs` | 19-76 | DELETE `resolve_pk_from_catalog` function |
| `src/ddl/define.rs` | 98-263 | Remove `conn: ffi::duckdb_connection` parameter; remove §1 PK lookup, §5 metadata, §6 type inference, §7 fact typing. Function reduces to validation only (graph checks at §4) and JSON serialization. |
| `cpp/src/shim.cpp` | 39-104 (extern "C" block) | Revert `sv_make_override_context` declaration to v0.9.0 |
| `cpp/src/shim.cpp` | 159-181 (`SemanticViewsParserInfo`) | Restore "intentional leak" comment (Phase 62 §Q2) |
| `cpp/src/shim.cpp` | 353-407 (`sv_register_parser_hooks`) | Revert signature; build OverrideContext with v0.9.0 args |

### 1.2 Plan 04 — ALTER architecture wave

**End state:** all 3 currently-existing ALTER variants (RENAME, SET COMMENT, UNSET COMMENT) ride pure-SQL rewrites with `json_merge_patch`; `CREATE FROM YAML FILE` uses the `__sv_compute_create_from_yaml(path, opts)` helper TF; conn_guard.rs is moot (already deleted in Plan 03); the v0.9.0 read_text-via-CatalogReader path inside `rewrite_yaml_file_create` is replaced by the helper-TF approach.

**The 3 trivial ALTER variants** (RENAME, SET/UNSET COMMENT) actually do NOT need helper TFs once Plan 03 has landed:
- **RENAME** is already a pure UPDATE of the `name` column (no JSON touch). Plan 03's restructure handles it (§1.1.6).
- **SET COMMENT** = `UPDATE _definitions SET definition = json_merge_patch(definition::JSON, '{"comment": "<new>"}'::JSON)::VARCHAR WHERE name = ?` — pure SQL.
- **UNSET COMMENT** = `UPDATE _definitions SET definition = json_merge_patch(definition::JSON, '{"comment": null}'::JSON)::VARCHAR WHERE name = ?` — `json_merge_patch` per the RFC-7396 spec interprets a `null` value as "remove this key" (verify with a sqllogictest at task-start).

**Open empirical question for Plan 04 (Wave-0 spike):** does DuckDB v1.5.2's `json_merge_patch` follow RFC-7396 strictly (null-as-delete)? If not, UNSET COMMENT needs a different shape — possibly `regexp_replace` on the JSON string, or a helper TF. **Recommended Plan 04 first task:** one-statement sqllogictest spike: `SELECT json_merge_patch('{"a":1,"b":2}'::JSON, '{"b":null}'::JSON);` — expected output `{"a":1}` if RFC-7396-compliant.

**Helper TF in Plan 04 is only needed for one path: `CREATE FROM YAML FILE` (D-11).** Sketch:

```cpp
// cpp/src/shim.cpp — Plan 04 addition (or new alter_helpers.cpp if size pressure)
struct CreateFromYamlBindData : TableFunctionData {
  string file_path;
  string view_name;
  string comment;     // optional view-level comment
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
    // 1. read_text the YAML file
    // 2. call into Rust to parse YAML → SemanticViewDefinition → enrich → serialize
    //    (Rust FFI helper exposed for this; reuses model::from_yaml_with_size_cap)
    // 3. populate bd->new_def
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("new_def");
    return std::move(bd);
}
```

Outer parser_override emits:
```sql
INSERT INTO semantic_layer._definitions (name, definition)
SELECT '<name>',
       new_def
FROM __sv_compute_create_from_yaml('<path>', '<name>', <kind>, '<comment>')
ON CONFLICT (name) DO UPDATE SET definition = excluded.definition  -- for OR REPLACE
RETURNING name AS view_name
```

(or `INSERT OR IGNORE` for IF NOT EXISTS — fold per Plan 03's existence-check elimination decision.)

**Lines/files affected in Plan 04:**

| File | Change |
|------|--------|
| `src/parse.rs` `rewrite_yaml_file_create` (`:2051-2148`) | Replace v0.9.0 read_text-via-CatalogReader path with rewrite to the `__sv_compute_create_from_yaml` TF subquery shape above. No catalog access from parser_override. |
| `src/parse.rs` `rewrite_alter_comment` (`:2314-2383`) | Replace lookup+mutate+reserialize with `json_merge_patch` UPDATE shape (Plan 03 may already have done this; Plan 04 verifies + extends tests) |
| `cpp/src/shim.cpp` OR new `cpp/src/alter_helpers.cpp` | Add `__sv_compute_create_from_yaml` TF registration via `sv_register_table_function` (the Plan 02 partial infrastructure) |
| `src/` new FFI helper | Rust function exposed via `#[no_mangle] extern "C"` that takes (path, name, kind, comment) → returns owned JSON string ptr+len. Called from the C++ bind. Reuses `SemanticViewDefinition::from_yaml_with_size_cap`, `infer_cardinality`, and the slimmed `enrich_definition_for_create`. |
| `test/sql/65_alter_*.test` and/or `test/integration/test_alter_yaml_file.py` | New sqllogictests for SET COMMENT / UNSET COMMENT under json_merge_patch; new integration tests for CREATE FROM YAML FILE through the helper-TF path |

**Cardinality of work:** Plan 04 is the smallest of the four plans IF the future 8 ALTER variants are deferred. Recommendation (per D-22): defer them — surface as TECH-DEBT entries or a v0.10.x follow-up.

### 1.3 Plan 05 — read-path wave

**End state:** 15 table funcs + 2 scalars (17 total) registered via `sv_register_table_function` (C++ Catalog API) instead of duckdb-rs's `register_table_function_with_extra_info` / `register_scalar_function_with_state`; each bind callback opens per-call `Connection(*context.db)`; type inference defers to bind time; H2 `query_conn` (`src/lib.rs:498-507`) deleted in the final commit of Plan 05.

**See §3 for the per-callback inventory and bind-callback shape.**

**Sequencing within Plan 05** (per CONTEXT §<specifics>, refined here):
1. **Wave 1 — simplest callbacks first** (no bind args except varargs): `list_semantic_views`, `list_terse_semantic_views`, `show_semantic_dimensions_all`, `show_semantic_metrics_all`, `show_semantic_facts_all`, `show_semantic_materializations_all`. Pure full-table scans of `_definitions`. (6 funcs)
2. **Wave 2 — name-arg callbacks**: `describe_semantic_view`, `show_columns_in_semantic_view`, `show_semantic_dimensions`, `show_semantic_metrics`, `show_semantic_facts`, `show_semantic_materializations`. Take one view-name VARCHAR; lookup-by-PK in `_definitions`. (6 funcs)
3. **Wave 3 — two-arg callbacks**: `show_semantic_dimensions_for_metric` (view-name + metric-name). (1 func)
4. **Wave 4 — scalars**: `get_ddl`, `read_yaml_from_semantic_view`. (2 funcs). Note: scalars do NOT have a bind/init/exec triple in the same shape as table funcs — they have a single `ScalarFunction` registration. The C++ API for scalar registration is `system_catalog.CreateFunction(txn, ScalarFunctionSet)`. **Verify whether `sv_register_table_function` infrastructure handles scalars too** OR add a sibling `sv_register_scalar_function` shim. **Recommended:** read `65-READ-PATH-SPIKE.md` setup code — it registered a table function only. Plan 05 needs an analogous scalar registration helper. The pattern is structurally identical; planner adds ~30 LOC of C++ for the scalar variant.
5. **Wave 5 — `explain_semantic_view`**: similar to `semantic_view` exec path; varargs. Bind needs to look up the view definition and compute the expansion plan. (1 func)
6. **Wave 6 — `semantic_view` (main expansion path)**: highest blast radius — migrate last. Once green, the final commit deletes the H2 `query_conn` allocation. (1 func)

**Critical mechanism detail for Plan 05:** the existing 17 read-side functions are implemented as Rust types implementing duckdb-rs's `VTab` / `ScalarFunctionSet` traits. Their `bind` / `init` / `func` callbacks receive `BindInfo` / `FunctionInfo` (the duckdb-rs wrappers) which do NOT expose `ClientContext`. Plan 05 needs to:

- **Option A** (recommended per `65-READ-PATH-SPIKE.md` interpretation §1): move registration off duckdb-rs and into the C++ shim. Keep the Rust function bodies. Add FFI surface so the C++ bind callback can call back into Rust with `ClientContext` (as an opaque `duckdb_connection` opened via `Connection(*context.db)` then converted to a `duckdb_connection` C-API handle via internal pointer manipulation, OR by exposing the raw `Connection` to Rust through a new FFI type).

  - **Sub-question:** how does Rust code consume the C++-opened `Connection`? The current Rust bind callbacks expect `duckdb_connection` (C-API handle). The C++ `Connection(*context.db)` is a C++ object. **Bridge:** the C++ shim opens `Connection probe(*context.db);` then calls `duckdb_connect` semantics in the bridge to wrap it, OR (simpler) the C++ shim wraps `Connection`'s internal `duckdb_connection` pointer back into a `duckdb_connection`. Need a small spike during Wave 0 to validate the chosen bridge.

- **Option B** (CONTEXT § canonical_refs prior-art option 3): keep duckdb-rs registration but inject `db_handle` into bind callbacks via a stashed `OnceLock<usize>`. Re-uses the `BIND-THREAD-RC1`-failed C-API path with a workaround. **Rejected** — `BIND-THREAD-RC1` empirically failed; CONTEXT D-23 (root-cause over symptom hacks) forbids this.

- **Option C** (CONTEXT § canonical_refs option 2): wait for duckdb-rs upstream to expose `ClientContext &` via `BindInfo`. Out of project control.

**Verdict: Option A is the only viable path.** Plan 05's first task is a spike to choose the Rust↔C++ bridge mechanism.

**Lines/files affected in Plan 05:**

| File | Change |
|------|--------|
| `cpp/src/shim.cpp` | Extend `sv_register_table_function` to handle the 15 read-side table functions; add `sv_register_scalar_function` for the 2 scalars. Bind/exec callbacks call back into Rust via new FFI surface. |
| `src/ddl/list.rs`, `describe.rs`, `show_*.rs`, `get_ddl.rs`, `read_yaml.rs` | Refactor bind/exec impls to be callable from C++ — likely a new `pub extern "C" fn sv_<vtab>_bind(...)` per VTab. Or expose a single dispatch function keyed on a vtab-id enum. |
| `src/query/table_function.rs` (`semantic_view`) | Same — migrate registration; keep bind/exec logic but route via the C++ shim. Type inference at bind time (D-16). |
| `src/query/explain.rs` | Same for `explain_semantic_view`. |
| `src/lib.rs:425-518` | Remove all 17 `register_table_function_with_extra_info` / `register_scalar_function_with_state` calls. Replace with a single `sv_register_read_side_functions(db_handle)` C++ FFI call. |
| `src/lib.rs:498-507` | **FINAL COMMIT of Plan 05:** delete H2 `query_conn` allocation. |
| `src/catalog.rs::CatalogReader` | The `CatalogReader` struct (`src/catalog.rs:97+`) wraps a `duckdb_connection`. Plan 05 either retires it entirely (the bind callbacks construct a fresh one from the per-call connection) OR refactors it to take a connection-borrow at method call time. Recommended: keep the struct (it has useful caching of `catalog_table_present` short-circuits) but make `new` take a `duckdb_connection` per-call instead of holding it long-lived. |

### 1.4 Plan 06 — lifecycle close-out wave

**End state:** H1 retired; structural guard test in CI; 4 new D-03b post-reopen tests added to `test/integration/test_readonly_load.py`; B1..B4 + B11 + 4 new tests all green; LIFE-04 ledger entry closed.

**Tasks:**

1. **Delete H1 `catalog_conn` allocation** (`src/lib.rs:386-410`). The variable becomes unused — the `catalog_reader` it was feeding is also unused since Plan 05 moved everyone off it. Cleanup cascade: `catalog_reader` local variable, `catalog_table_present` probe (was still feeding `sv_register_parser_hooks` per the v0.9.0 shape Plan 03 restored). **Sub-question:** does `sv_register_parser_hooks` still need `catalog_table_present`? At v0.9.0 it gated reader-path short-circuits when RO host DB didn't have `_definitions`. Under read-elimination, reader-path callbacks (Plan 05) open per-call connections; their bind can do the same `information_schema.tables` probe directly. **Recommendation:** Plan 06 deletes the `catalog_table_present` arg from `sv_register_parser_hooks` entirely (it has no consumer); each reader-path bind that needs the flag computes it locally on its per-call connection.

2. **Restore + then-delete `INTENTIONAL LEAK` comment** (`src/parse.rs::OverrideContext::Drop` and `cpp/src/shim.cpp::~SemanticViewsParserInfo`): Plan 03 restored the v0.9.0 leak comment. Plan 06 deletes the leak entirely because there's no longer a `duckdb_connection` field in `OverrideContext` (no — Plan 03 reverts to v0.9.0 which DOES have one). **Resequencing:** Plan 06 must change `OverrideContext` again — remove the `CatalogReader` field and the `Drop` impl — because H1 is now gone. The CONTEXT D-01 phrasing ("temporarily restored, retired by Plan 06") covers this. So Plan 06's parse.rs change: re-slim `OverrideContext` to carry only `is_file_backed` (and possibly nothing — see below). The `db_handle` is no longer needed because nothing under parser_override needs a connection.
   - **What does OverrideContext carry post-Plan-06?** Probably just `is_file_backed` (still used to gate whether facts/columns have inferred types persisted in JSON — though if Plan 03 already removed CREATE-time type inference, `is_file_backed` becomes dead too). **Decision point:** if both `catalog_table_present` and `is_file_backed` are dead post-Plan-06, retire `OverrideContext` AND `SemanticViewsParserInfo`'s `rust_state` field entirely. The parser-override hook then just needs `ParserExtensionInfo` with no Rust-side data. Cleaner end state.

3. **Structural guard test** (success criterion 4). See §7 for design. Recommendation: a single Rust integration test under `tests/no_long_lived_conn.rs` that uses `syn` to parse `src/lib.rs::init_extension` and asserts no `duckdb_connect` call site remains. ~50 LOC of test code. `syn` is already a Cargo dependency (verify via Cargo.toml) — if not, low-cost addition.

4. **Extend post-reopen integration coverage** (D-03b). See §8 for sketches of the 4 new tests.

5. **Update `deferred-items.md`** at `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md`. Mark the "In-process RW→RO reopen of the same DB hangs" entry as resolved with a forward pointer to Phase 65 / v0.10.0. (Plain docs edit.)

**Lines/files affected in Plan 06:**

| File | Change |
|------|--------|
| `src/lib.rs:386-410` | DELETE H1 `catalog_conn` allocation + `catalog_table_present` probe + `catalog_reader` local |
| `src/lib.rs:420` | `sv_register_parser_hooks(db_handle, is_file_backed)` — drop the `catalog_table_present` arg |
| `src/parse.rs:67-87` | Final slim of `OverrideContext` — retire `CatalogReader` field, `Drop` impl, and `catalog_table_present` (decision pending: may retire whole struct) |
| `src/parse.rs:2513-2557` | Slim `sv_make_override_context` signature accordingly |
| `cpp/src/shim.cpp:159-181` | Delete the v0.9.0 leak comment from `~SemanticViewsParserInfo` |
| `cpp/src/shim.cpp:353-407` | Update `sv_register_parser_hooks` signature |
| `tests/no_long_lived_conn.rs` (new) | Structural guard test (§7) |
| `test/integration/test_readonly_load.py` | Add 4 D-03b tests (§8) |
| `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` | LIFE-04 ledger update |

---

## 2. v0.9.0 OverrideContext Shape (for Plan 03's Revert)

**Exact pre-`0d2c0b7` shape**, lifted from `git show 0d2c0b7^:src/parse.rs` lines 39-72:

```rust
//
// CREATE/DROP/ALTER need to know whether a view exists (and for SET/UNSET
// COMMENT, what its current JSON definition is) before emitting native SQL
// with friendly errors. The parser_override callback runs in a context
// without access to the caller's catalog, so we stash a dedicated
// `CatalogReader` (populated at extension load) and hand it to the C++ shim
// as an opaque `Box<OverrideContext>`. The shim attaches the boxed pointer
// to its `SemanticViewsParserInfo` (the `parser_info` value DuckDB passes
// back into the override callback for every parse). Lifetime is tied to the
// `DBConfig`, so destruction happens on DB unload.

/// Catalog handle plus an `is_file_backed` flag that gates DDL-time
/// type inference. `LIMIT 0` probes used for type inference depend on
/// user tables having been committed; for in-memory DBs we follow the
/// v0.7.1 behaviour and skip inference entirely.
///
/// Owned by the C++ shim as `Box<OverrideContext>` (one per
/// `SemanticViewsParserInfo`, i.e. one per extension-LOAD-per-DB).
#[cfg(feature = "extension")]
pub struct OverrideContext {
    pub catalog: crate::catalog::CatalogReader,
    pub is_file_backed: bool,
}

#[cfg(feature = "extension")]
impl Drop for OverrideContext {
    fn drop(&mut self) {
        // Phase 62 Q2 — INTENTIONAL LEAK of self.catalog.conn (the duckdb_connection).
        //
        // ~SemanticViewsParserInfo (and therefore Drop for OverrideContext) fires
        // during ~DBConfig, AFTER ~DatabaseInstance has already reset
        // connection_manager (duckdb.cpp:276819). Calling duckdb_disconnect here
        // would invoke ~Connection() → ConnectionManager::RemoveConnection() on
        // the destroyed manager — use-after-free.
        //
        // The leak is bounded at ONE duckdb_connection per DB ever opened in this
        // process (a few KB each). This matches v0.8.0 commit 680a967 which shipped
        // successfully with the same leak.
        //
        // See: .planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md §Q2.
    }
}
```

**FFI signature pre-`0d2c0b7`** (`sv_make_override_context`):
```rust
pub unsafe extern "C" fn sv_make_override_context(
    catalog_conn: libduckdb_sys::duckdb_connection,
    is_file_backed: bool,
) -> *mut std::ffi::c_void
```

**`sv_register_parser_hooks` C++ shim signature pre-`f9caafe`** (`cpp/src/shim.cpp`):
```cpp
bool sv_register_parser_hooks(duckdb_connection catalog_conn,
                              bool is_file_backed)
```

**Plan 03 reverts to exactly these shapes.** Confirmation: `git show 0d2c0b7^:cpp/src/shim.cpp` ≅ `git show 6ee18fe:cpp/src/shim.cpp` (the last v0.9.0 commit). [VERIFIED: git history]

---

## 3. Read-Side Callback Inventory (Plan 05)

The actual count is **15 table functions + 2 scalar functions = 17**, not 12 as CONTEXT.md states (CONTEXT folds the `_X` / `_X_all` sibling pairs together for brevity). [VERIFIED: grep `register_table_function_with_extra_info|register_scalar_function_with_state` in `src/lib.rs`]

| # | Function | Type | Source file | Registration site | Arg shape | Notes |
|---|----------|------|-------------|-------------------|-----------|-------|
| 1 | `list_semantic_views` | TF | `src/ddl/list.rs` (`ListSemanticViewsVTab`) | `src/lib.rs:425` | () | Full-table scan of `_definitions` |
| 2 | `list_terse_semantic_views` | TF | `src/ddl/list.rs` (`ListTerseSemanticViewsVTab`) | `src/lib.rs:429` | () | Terse-format scan |
| 3 | `show_columns_in_semantic_view` | TF | `src/ddl/show_columns.rs` | `src/lib.rs:433` | (VARCHAR) view name | Lookup-by-PK |
| 4 | `describe_semantic_view` | TF | `src/ddl/describe.rs` | `src/lib.rs:437` | (VARCHAR) | Lookup-by-PK; this is the post-reopen variant D-03b mandates |
| 5 | `show_semantic_dimensions` | TF | `src/ddl/show_dims.rs` (`ShowSemanticDimensionsVTab`) | `src/lib.rs:443` | (VARCHAR) | D-03b post-reopen target (the SHOW representative) |
| 6 | `show_semantic_dimensions_all` | TF | `src/ddl/show_dims.rs` (`ShowSemanticDimensionsAllVTab`) | `src/lib.rs:447` | () | Full-table |
| 7 | `show_semantic_dimensions_for_metric` | TF | `src/ddl/show_dims_for_metric.rs` | `src/lib.rs:453` | (VARCHAR, VARCHAR) | Two-arg lookup |
| 8 | `show_semantic_metrics` | TF | `src/ddl/show_metrics.rs` (`ShowSemanticMetricsVTab`) | `src/lib.rs:459` | (VARCHAR) | Lookup-by-PK |
| 9 | `show_semantic_metrics_all` | TF | `src/ddl/show_metrics.rs` (`ShowSemanticMetricsAllVTab`) | `src/lib.rs:463` | () | Full-table |
| 10 | `show_semantic_facts` | TF | `src/ddl/show_facts.rs` (`ShowSemanticFactsVTab`) | `src/lib.rs:469` | (VARCHAR) | Lookup-by-PK |
| 11 | `show_semantic_facts_all` | TF | `src/ddl/show_facts.rs` (`ShowSemanticFactsAllVTab`) | `src/lib.rs:473` | () | Full-table |
| 12 | `show_semantic_materializations` | TF | `src/ddl/show_materializations.rs` (`ShowSemanticMaterializationsVTab`) | `src/lib.rs:479` | (VARCHAR) | Lookup-by-PK |
| 13 | `show_semantic_materializations_all` | TF | `src/ddl/show_materializations.rs` (`ShowSemanticMaterializationsAllVTab`) | `src/lib.rs:483` | () | Full-table |
| 14 | `get_ddl` | **Scalar** | `src/ddl/get_ddl.rs` (`GetDdlScalar`) | `src/lib.rs:489` | (VARCHAR) view name | D-03b post-reopen target |
| 15 | `read_yaml_from_semantic_view` | **Scalar** | `src/ddl/read_yaml.rs` (`ReadYamlFromSemanticViewScalar`) | `src/lib.rs:492` | (VARCHAR) | YAML round-trip |
| 16 | `semantic_view` | TF | `src/query/table_function.rs` (`SemanticViewVTab`) | `src/lib.rs:509` | varargs (name + dimensions + metrics + facts) | **Highest blast radius — D-03b post-reopen target — migrate LAST** |
| 17 | `explain_semantic_view` | TF | `src/query/explain.rs` (`ExplainSemanticViewVTab`) | `src/lib.rs:515` | varargs | Mirror of `semantic_view` exec path |

**State consumed today (all 17):** the long-lived `catalog_reader` (wrapping H1 `catalog_conn`) for everything except #16 and #17 which use `query_state` (wrapping H2 `query_conn`). Plan 05 migrates everything off both handles.

**Common bind-callback pattern under the new mechanism:**
```cpp
static unique_ptr<FunctionData> sv_<name>_bind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    auto bd = make_uniq<MyBindData>();
    // Open per-call connection
    Connection probe(*context.db);
    auto raw_conn = /* extract duckdb_connection from Connection — see bridge spike */;
    // Call back into Rust with raw_conn + input args + return_types/names refs
    sv_<vtab>_bind_rust(raw_conn, /* args */, /* return-shape pointers */);
    return std::move(bd);
}
```

Each Rust side keeps its existing logic (catalog lookup, JSON deserialize, schema declaration) but takes the per-call connection instead of a long-lived one.

---

## 4. ALTER Variant Inventory + Classification

### 4.1 Current state (today's parse.rs)

Only **3 ALTER variants** are implemented in `src/parse.rs::rewrite_alter` (lines 651-710):

| Variant | Current emit (parse.rs:679-703) | Catalog read today | Mechanism after read-elimination |
|---------|----------------------------------|--------------------|----------------------------------|
| `ALTER SEMANTIC VIEW v RENAME TO w` | `SELECT * FROM alter_semantic_view_rename('v', 'w')` → ultimately `UPDATE … SET name = ?` | `catalog.exists(old)` + `catalog.exists(new)` (2 reads) | Pure SQL: race-guard CASE+error+UPDATE per §1.1.6 |
| `ALTER SEMANTIC VIEW v SET COMMENT = '...'` | `SELECT * FROM alter_semantic_view_set_comment('v', 'comment')` → lookup+mutate+reserialize+UPDATE | `catalog.lookup(name)` (1 read, then JSON deserialize+reserialize) | Pure SQL: `UPDATE _definitions SET definition = json_merge_patch(definition::JSON, '{"comment":"..."}'::JSON)::VARCHAR WHERE name = ?` |
| `ALTER SEMANTIC VIEW v UNSET COMMENT` | Same lookup+mutate pattern | Same | Pure SQL: `UPDATE _definitions SET definition = json_merge_patch(definition::JSON, '{"comment":null}'::JSON)::VARCHAR WHERE name = ?` (assuming RFC-7396 null-as-delete — verify Wave 0) |

**All 3 are pure-SQL rewrites after Plan 03 / Plan 04.** None need helper TFs.

### 4.2 Future-additional ALTER variants (CONTEXT mentions but NOT in code today)

CONTEXT.md `<specifics>` lists 8 more variants (MAKE PRIVATE/PUBLIC, SET TAG, ADD SYNONYMS, ADD DIMENSION/METRIC/FACT, DROP DIMENSION/METRIC/FACT/RELATIONSHIP, ADD RELATIONSHIP). These would require both:
- **Parser layer extension** in `rewrite_alter` (`src/parse.rs:651-710`): each new variant needs its own keyword arm + arg extraction. Order of ~50-100 LOC per variant.
- **Helper-TF or pure-SQL rewrite shape** depending on whether catalog reads / type inference / FK validation is needed (CONTEXT classifies but the binding to actual code doesn't exist yet).

**Per D-22 (bounded scope) recommendation:** defer all 8 to a v0.10.x or v0.11 follow-up. Phase 65 ships the lifecycle fix and the read-elimination architecture; it does NOT need to ship new ALTER surface. Mark in CHANGELOG: "ALTER SET COMMENT / UNSET COMMENT / RENAME now ride pure-SQL JSON rewrites; future ALTER variants will adopt the helper-TF pattern established in Plan 04."

### 4.3 v0.6.0 ALTER test coverage (planner extends, doesn't rewrite)

- `test/sql/error_caret_alter.test` — caret rendering for ALTER syntax errors
- `test/sql/phase34_1_alter_rename.test` — ALTER RENAME TO end-to-end
- `test/sql/phase45_alter_comment.test` — ALTER SET/UNSET COMMENT end-to-end
- Plus inline tests in `src/parse.rs` (lines ~4448-5293 — assertions about emitted SQL shape)

**Planner action for Plan 04:** add new sqllogictests under `test/sql/65_alter_*.test` for the new pure-SQL rewrite shapes. Existing v0.6.0 tests should continue to pass byte-identical at the result level (the user-visible behavior — COMMENT got set, view got renamed — is unchanged; only the internal SQL shape differs).

---

## 5. C++ Catalog API Table-Function Registration (Plans 04 + 05)

### 5.1 The surviving `sv_register_table_function` from Plan 02 partial

**To be confirmed by the planner via `git log cpp/src/shim.cpp` against the Plan 02 partial commits.** The CONTEXT D-04 states "Plan 02's commits that touch ONLY this shim (not OverrideContext / parser_override) stay." Need to identify which commit(s) added `sv_register_table_function` and confirm they survive Plan 03's revert.

`grep -n "sv_register_table_function" cpp/src/shim.cpp` from current HEAD returns no matches (the function isn't in HEAD). **This means the Plan 02 "surviving infrastructure" referenced in CONTEXT D-04 was NEVER actually committed** — only the OverrideContext-side `0d2c0b7` + `f9caafe` landed, then halted. The C++ Catalog API registration shim was sketched in the spike files (`65-READ-PATH-SPIKE.md`, `65-ALTER-REWRITE-SPIKE.md`) but reverted before commit per the SPIKE.md "self-check" sections (every spike says `git diff --stat cpp/src/shim.cpp ... returns empty after git checkout`). [VERIFIED: `grep -c sv_register_table_function cpp/src/shim.cpp` → 0]

**Implication for the planner:** D-04's "keep the surviving Plan 02 infrastructure" is actually "write the infrastructure from scratch in Plan 04 (or as a Plan 04 prerequisite spike), using the spike code as a template." This is a tighter scope than CONTEXT suggests — Plan 04 introduces the entire `sv_register_table_function` machinery; Plan 05 consumes and extends it. Confirm with the planner.

### 5.2 Idiomatic shape (from `65-READ-PATH-SPIKE.md` + `65-ALTER-REWRITE-SPIKE.md`)

```cpp
// Pattern 1 — Table function with bind callback that can open a per-call Connection
static unique_ptr<FunctionData> sv_<name>_bind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    auto bd = make_uniq<MyBindData>();
    // Read inputs
    bd->arg = input.inputs[0].GetValue<string>();
    // Per-call connection — the critical mechanism
    Connection probe(*context.db);
    auto result = probe.Query("SELECT ...");
    // populate bd, return_types, names
    return std::move(bd);
}

static void sv_<name>_function(
    ClientContext &context, TableFunctionInput &data_p, DataChunk &output) {
    // emit rows from bind data
}

static void sv_register_<name>(DatabaseInstance &db) {
    TableFunction tf("<name>", {LogicalType::VARCHAR /* arg types */},
                     sv_<name>_function, sv_<name>_bind);
    CreateTableFunctionInfo info(tf);
    info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
    auto &system_catalog = Catalog::GetSystemCatalog(db);
    auto txn = CatalogTransaction::GetSystemTransaction(db);
    system_catalog.CreateTableFunction(txn, info);
}
```

### 5.3 Idiomatic shapes in community extensions (cited from training + planner can verify with Context7)

[ASSUMED] httpfs, iceberg, ducklake, and postgres_scanner all use the same `system_catalog.CreateTableFunction(txn, info)` pattern. The `Connection(*context.db)` per-call mechanism is most cleanly demonstrated in the postgres_scanner extension's bind callbacks, where it opens a Connection to query DuckDB-side metadata before reaching out to the remote Postgres instance.

**Planner action:** during Plan 05 Wave 0, do a quick read of one community extension's source (postgres_scanner or iceberg) to confirm the registration shape matches the spike code, AND to find canonical patterns for scalar-function registration (which the spike files don't cover). DuckDB v1.5.2 scalar function registration uses `system_catalog.CreateFunction(txn, ScalarFunctionSet)` — the same Catalog API entry, different `Info` subtype.

### 5.4 File-layout recommendation

CONTEXT discretion item: "Whether Plan 04's helper-TF family lives in a new `cpp/src/alter_helpers.cpp` translation unit or extends `cpp/src/shim.cpp` directly."

**Recommendation:** keep helper TFs in `cpp/src/shim.cpp` for Plans 04 and 05; refactor into separate translation units (`cpp/src/read_funcs.cpp`, `cpp/src/alter_helpers.cpp`) in a follow-up if/when shim.cpp exceeds ~1500 LOC. Today shim.cpp is 407 LOC; adding Plan 04 (~1 helper TF + 30 LOC scalar shim) and Plan 05 (~17 registration calls + dispatch) lands it at ~800-1000 LOC — still manageable in a single TU.

---

## 6. Type-Inference Deferral (D-16 + D-17)

### 6.1 Today's CREATE-time probe (Plan 03 removes)

`src/ddl/define.rs:163-219` (column types via LIMIT 0) and `:221-260` (fact types via `typeof(expr)`). Both currently execute via `crate::query::table_function::try_infer_schema(conn, &limit0_sql)` (in `src/query/table_function.rs`). The function is generic and re-usable from the read-side bind.

`is_file_backed` flag (currently on OverrideContext, today at parse.rs:71) gates whether the probe runs at all — in-memory DBs skip it because the temporary tables may not be committed yet. **Post-D-16 this gate moves to the read-side:** bind callback decides per-invocation whether to probe (based on something like `Connection(*context.db).is_file_backed()` or just unconditionally try and tolerate "table doesn't exist" errors).

### 6.2 Bind-time replacement (recommended shape)

```rust
// In each read-side bind callback that needs types (describe, show_columns, semantic_view, ...):
fn bind_with_type_inference(
    conn: duckdb_connection,        // per-call connection from C++
    view_name: &str,
    def: &SemanticViewDefinition,
) -> /* bind data */ {
    // Check process-local cache first
    if let Some(cached) = TYPE_CACHE.get(&(view_name, def.fingerprint())) {
        return cached;
    }
    // Not cached — probe
    let limit0_sql = format!("{} LIMIT 0", expand::expand(view_name, def, &full_request)?);
    let result = unsafe { try_infer_schema(conn, &limit0_sql) };
    TYPE_CACHE.insert((view_name, def.fingerprint()), result.clone());
    result
}
```

**Cache shape recommendation:** `OnceLock<RwLock<HashMap<(String, u64), Arc<InferredTypes>>>>` keyed on `(view_name, schema_fingerprint)` where `schema_fingerprint` is a hash of the relevant fields of `SemanticViewDefinition` (table refs, dimensions, metrics, facts). This handles the DDL-between-SHOWs invalidation case naturally: if ALTER changes the view, its JSON changes, fingerprint changes, cache misses, re-probes.

**Why not unbounded LRU?** TECH-DEBT #20 was the v0.8.0 LRU silent-eviction error class. Avoid re-introducing an LRU. A simple HashMap unbounded for the life of the extension is fine — entries are tiny (column-name + type-id pairs), and the keyspace is bounded by `number_of_views × number_of_distinct_definitions_in_history` per process. Phase 62 retired the LRU explicitly; don't bring it back.

**Why process-local, not connection-local?** Multiple connections (e.g. concurrent reads) hit the same view definition; sharing the cache eliminates duplicate probes. The TECH-DEBT #19 commit-visibility caveat applies (a cache hit may serve stale data if another transaction has just committed an ALTER); same trade-off Phase 60 documented, no worse here.

**Mechanism for `is_file_backed` gate at bind time:** the C++ `Connection(*context.db)` constructor gives access to `DatabaseInstance::config().options.access_mode` — easy enough to read from the bind side. OR just unconditionally try the LIMIT 0 probe and silently swallow any error (today's CREATE-time code at `define.rs:179-188` already has this `if let Ok(...)` shape).

### 6.3 No persisted type cache (D-17)

The `column_type_names` / `column_types_inferred` fields on `SemanticViewDefinition` (`model.rs:381-392`) stay in the struct but are populated only at read time (in-memory in the cache), never serialized to `_definitions.definition`. **Implementation:** `#[serde(skip_serializing_if = "Vec::is_empty")]` already on those fields; Plan 03 stops populating them at serialization time, so they always serialize as absent.

---

## 7. Structural Guard Test Design (Plan 06)

Goal (success criterion 4): "Structural Rust unit test fails CI if anyone re-introduces a long-lived native handle in `init_extension`."

### 7.1 Options surveyed

| Approach | Mechanism | Cost | Maintenance |
|----------|-----------|------|------------|
| **A. `syn` AST scan** | Test parses `src/lib.rs` with `syn` crate, walks `init_extension` body, fails if it finds `duckdb_connect` call site | ~50 LOC; need `syn` as dev-dep | Robust to whitespace / refactor; survives minor edits |
| **B. `build.rs` grep** | `build.rs` greps `src/lib.rs` for `duckdb_connect\(` inside the `init_extension` block; emits `compile_error!` if found | ~30 LOC of build.rs | Fragile to block-boundary detection |
| **C. Cargo-level grep test** | A `tests/no_long_lived_conn.rs` that runs `grep -E "duckdb_connect\(" src/lib.rs` via `std::process::Command` and asserts no match | ~10 LOC | Brittle; depends on shell + grep availability across platforms (Windows CI breaks) |
| **D. Symbol-table inspection** | Use `nm` or `objdump` on the built cdylib to count exported `duckdb_connect` call sites — but they're all inside the binary, not exported, so not visible | N/A | Not feasible |
| **E. `#[deny(unused_imports)]` trick** | Move `duckdb_connect` import behind a feature flag that's off in normal builds; the structural-guard test enables a `forbid_long_lived_conn` feature that flips it to a `compile_error!` | ~20 LOC + Cargo.toml feature | Awkward UX; mixes feature flags with API safety |

### 7.2 Recommendation: Option A (syn-based AST scan)

**Rationale:**
- Cross-platform (no shell dependency)
- Robust to formatting (parses AST, not text)
- Localized — checks `init_extension` specifically, not "no `duckdb_connect` anywhere" (because `ConnGuard::open` calls it as part of the per-call mechanism in Plan 05's bind callbacks)
- Cargo already pulls `syn` transitively (verify: many proc-macro deps depend on it); if not, adding `syn = "2"` as a dev-dep is ~0 cost
- ~50 LOC of test code is reviewable and well-scoped

**Sketch:**

```rust
// tests/no_long_lived_conn.rs
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
                    if p.path.segments.last().map(|s| s.ident.to_string()).as_deref() == Some("duckdb_connect") {
                        self.found = true;
                    }
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
    }

    let mut f = Finder { in_init_extension: false, found: false };
    f.visit_file(&file);
    assert!(!f.found,
        "init_extension contains a duckdb_connect call site. \
         Phase 65 retired long-lived extension-owned duckdb_connection handles. \
         If a new connection is genuinely needed, open it via a per-call \
         Connection(*context.db) inside a bind/exec callback instead.");
}
```

---

## 8. D-03b Post-Reopen Test Sketches (Plan 06)

Each test follows the B1/B2 pattern: bootstrap in subprocess (or via `open_writable`+close+gc), then reopen RO via `_connect_with_watchdog`, then exercise a post-reopen call also under watchdog (the call should return quickly because no long-lived state from the writer remains).

```python
def test_in_process_bootstrap_then_readonly_semantic_view_select():
    """D-03b #1 (LIFE-01 / criterion 3): exercise semantic_view() SELECT on RO reopened conn."""
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "sv_select.duckdb")
        w = open_writable(db)
        w.execute("CREATE TABLE t (i INT, j INT); INSERT INTO t VALUES (1,10),(2,20)")
        w.execute("CREATE SEMANTIC VIEW v TABLES (t PRIMARY KEY (i)) DIMENSIONS (i AS t.i) METRICS (s AS SUM(j))")
        w.close(); del w; gc.collect()

        ro, elapsed = _connect_with_watchdog(db, watchdog_seconds=5.0, read_only=True, config=_connect_config())
        try:
            assert elapsed < 5.0
            ro.execute("LOAD semantic_views")
            rows = ro.execute(
                "SELECT * FROM semantic_view('v', dimensions := ['i'], metrics := ['s']) ORDER BY i"
            ).fetchall()
            assert rows == [(1, 10), (2, 20)], rows
        finally:
            ro.close()


def test_in_process_bootstrap_then_readonly_describe():
    """D-03b #2: exercise describe_semantic_view() on RO reopened conn."""
    # bootstrap + close + RO reopen as above; then:
    rows = ro.execute("SELECT * FROM describe_semantic_view('v')").fetchall()
    assert len(rows) > 0, "describe returned empty"


def test_in_process_bootstrap_then_readonly_show_dimensions():
    """D-03b #3: exercise SHOW SEMANTIC DIMENSIONS on RO reopened conn (representative SHOW)."""
    # bootstrap + close + RO reopen; then:
    rows = ro.execute("SHOW SEMANTIC DIMENSIONS FROM v").fetchall()
    assert any('i' in str(r) for r in rows)


def test_in_process_bootstrap_then_readonly_get_ddl():
    """D-03b #4: exercise get_ddl() round-trip on RO reopened conn."""
    # bootstrap + close + RO reopen; then:
    ddl = ro.execute("SELECT get_ddl('v')").fetchone()[0]
    assert "CREATE OR REPLACE SEMANTIC VIEW" in ddl
    assert "v" in ddl
```

**All four tests fail on v0.9.0 baseline** (the initial `_connect_with_watchdog` already hangs because of H1; even if Plan 01's watchdog catches the hang, the test fails its `< 5.0s` assertion). **All four pass after Plan 06** (both H1 and H2 are retired; the RO reopen returns quickly; the read-side bind callbacks open per-call connections that succeed on the RO conn).

---

## 9. H1 + H2 Retirement Map (Plans 03/05/06)

| Handle | Allocation site | Current consumers | Retire plan |
|--------|-----------------|-------------------|------------|
| **H1** `catalog_conn` | `src/lib.rs:386-387` (`duckdb_connect(db_handle, &mut catalog_conn)`) | `catalog_reader` (used by all 17 read-side registrations as `extra_info`); `sv_register_parser_hooks` (passed to make `OverrideContext::catalog`) | Plan 03 makes parser_override stop using it (slimming); Plan 05 moves read-side off via per-call connections; **Plan 06 deletes the allocation itself** |
| **H2** `query_conn` | `src/lib.rs:498-499` (`duckdb_connect(db_handle, &mut query_conn)`) | `query_state` (used by `semantic_view` + `explain_semantic_view` registrations) | **Plan 05's final commit deletes the allocation.** No other plan touches it. |

**Key:** Plan 05 cannot retire H1 because parser_override (the `OverrideContext::catalog` path that Plan 03 reverts to) still uses it through Plan 04 (specifically `rewrite_yaml_file_create` keeps using the OverrideContext's CatalogReader until Plan 04's helper-TF migration). H1's allocation site removal is therefore correctly placed in Plan 06.

---

## 10. Validation Architecture (Nyquist)

Per `.planning/config.json` `workflow.nyquist_validation: true`. Plan-by-plan evidence types:

### Test Framework
| Property | Value |
|----------|-------|
| Framework | `cargo test` (Rust unit + proptest); `sqllogictest` via `just test-sql`; Python integration via `uv run` on files under `test/integration/` |
| Config file | `Cargo.toml`, `justfile`, no per-test framework config |
| Quick run command | `cargo test --lib` |
| Full suite command | `just test-all` |

### Phase Requirements → Test Map (anchored to D-04, D-05, D-13, etc.)

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| LIFE-01 | In-process RW→close→RO reopen returns < 5s | Python integration | `uv run test/integration/test_readonly_load.py::test_in_process_bootstrap_then_readonly_fresh` | ✅ (Plan 01) |
| LIFE-01 (variants) | Same with existing-bootstrap / LOAD-only / RW→RO→RW direction | Python integration | `..._existing`, `..._load_only_then_readonly`, `test_in_process_readonly_then_readwrite` | ✅ (Plan 01) |
| LIFE-01 (D-03b) | Post-reopen `semantic_view()` SELECT under watchdog | Python integration | `..._readonly_semantic_view_select` | ❌ Plan 06 W0 |
| LIFE-01 (D-03b) | Post-reopen `describe_semantic_view()` | Python integration | `..._readonly_describe` | ❌ Plan 06 W0 |
| LIFE-01 (D-03b) | Post-reopen `SHOW SEMANTIC DIMENSIONS` | Python integration | `..._readonly_show_dimensions` | ❌ Plan 06 W0 |
| LIFE-01 (D-03b) | Post-reopen `get_ddl()` | Python integration | `..._readonly_get_ddl` | ❌ Plan 06 W0 |
| LIFE-02 | Repeated bootstrap+close 50× — no busy-spin | Python integration | `test_repeated_load_close_no_busy_spin` | ✅ (Plan 01 B11) |
| LIFE-02 (structural) | `init_extension` has no `duckdb_connect` call | Rust unit | `cargo test --test no_long_lived_conn` | ❌ Plan 06 W0 |
| LIFE-04 | `deferred-items.md` entry marked resolved | Manual docs check | grep ledger | ❌ Plan 06 W0 |
| D-05 (PK auto-removal) | Missing-PK + FK CREATE raises actionable error | sqllogictest | `just test-sql test/sql/65_pk_error.test` | ❌ Plan 03 W0 |
| D-05 | Existing v0.9.0 stored defs continue to load + query | sqllogictest | extend `test/sql/phase42_persistence.test` or new | ❌ Plan 03 W0 |
| D-09 (json_merge_patch SET COMMENT) | ALTER SET COMMENT updates JSON via json_merge_patch | sqllogictest | extend `test/sql/phase45_alter_comment.test` | ⚠️ extend |
| D-09 (UNSET COMMENT null-as-delete) | RFC-7396 verification | sqllogictest | new `test/sql/65_json_merge_patch_smoke.test` | ❌ Plan 04 W0 |
| D-09/D-13 race-guard | DROP/ALTER race-guard SQL still works after slimming | sqllogictest | extend existing `phase45_alter_comment.test` + `phase4*_*.test` | ✅ exists |
| D-11 (CREATE FROM YAML FILE via helper TF) | YAML file → semantic view via `__sv_compute_create_from_yaml` | Python integration (file IO) | new `test/integration/test_create_from_yaml_v010.py` | ❌ Plan 04 W0 |
| D-14 (read-path migration parity) | All 17 read-side functions return same shape pre/post migration | sqllogictest | existing `phase4*_*.test`, `phase39_metadata.test` etc. | ✅ exists; full pass required |
| D-15 (H2 retirement) | After H2 removed, `semantic_view()` still returns rows on attached DBs (`db2.main.v`) | Python integration | `test_multi_db_isolation.py` (existing) | ✅ exists |
| D-16 (type inference on demand) | `data_type` populated on first DESCRIBE call post-Plan-05 | sqllogictest | extend `test/sql/phase41_describe.test` | ⚠️ extend |
| D-21 (transactional DDL) | ADBC tests stay green | Python integration | `uv run test/integration/test_adbc_transactions.py` | ✅ exists |

### Sampling Rate
- **Per task commit:** `cargo test --lib` (~30 s)
- **Per wave merge:** `just test-all` (~3-5 min) — every wave that touches Rust must pass
- **Phase gate:** `just ci` green on `milestone/v0.10.0` before `/gsd-verify-work`

### Wave 0 Gaps (per plan)

**Plan 03 W0:**
- [ ] `test/sql/65_pk_error.test` — D-06 actionable error message assertion
- [ ] `test/sql/65_metadata_via_sql.test` — verify `now()` / `current_database()` / `current_schema()` populate JSON correctly via the embedded `json_merge_patch` shape
- [ ] Rust test in `src/parse.rs` mod tests for new D-06 error path

**Plan 04 W0:**
- [ ] `test/sql/65_json_merge_patch_smoke.test` — RFC-7396 null-as-delete verification (single statement, very fast); if it fails the UNSET COMMENT mechanism needs redesign
- [ ] Plan 04's first task must be this spike (~5 min); planner halts and replans if it fails
- [ ] `test/integration/test_create_from_yaml_v010.py` — CREATE FROM YAML FILE through `__sv_compute_create_from_yaml`

**Plan 05 W0:**
- [ ] Bridge spike — choose Rust↔C++ connection bridge mechanism. Single sqllogictest exercising one migrated callback end-to-end (e.g. `list_semantic_views`).
- [ ] `test/integration/test_concurrent_reads_per_call_conn.py` — exercise 8 parallel `SHOW SEMANTIC DIMENSIONS` calls against the same DB; verify each opens its own per-call connection without contention.

**Plan 06 W0:**
- [ ] `tests/no_long_lived_conn.rs` — structural guard test (§7)
- [ ] 4 D-03b post-reopen tests (§8)

---

## 11. Security Domain

Per `.planning/config.json`, security_enforcement is not explicitly disabled; treat as enabled.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No auth surface — extension trusts the DuckDB process |
| V3 Session Management | no | DuckDB connections are the session boundary; no extension-level session state |
| V4 Access Control | no | RO mode enforcement is DuckDB's responsibility; D-21 ADBC tests verify the extension doesn't bypass it |
| V5 Input Validation | yes | `parser_override` parses user SQL; validation already enforced via `body_parser`, `find_identifier_end`, `normalize_view_name`. Plan 03's revert restores v0.9.0 hardening. Plan 04's new SQL shapes (`json_merge_patch` with user-supplied comment) MUST continue to use `escape_sql_arg` for embedded strings. |
| V6 Cryptography | no | No crypto |
| V7 Error handling | yes | New D-06 error message must not leak catalog internals — the message template in CONTEXT.md is appropriate (lists user-actionable remediation) |
| V8 Data protection | no | No PII in `_definitions`; storage is alongside user DB |
| V12 Files | yes | `CREATE FROM YAML FILE` reads arbitrary paths via `read_text` — Plan 04 must preserve `YAML_SIZE_CAP` (1 MiB) sanity guard from Phase 51 and continue to delegate path resolution to DuckDB's `read_text` (which honors `enable_external_access` etc.) |

### Known Threat Patterns for parser_override + json_merge_patch path

| Pattern | STRIDE | Mitigation |
|---------|--------|------------|
| SQL injection via view name in ALTER COMMENT | T | `escape_sql_arg` (current code at `parse.rs:2170-2172`) — must apply to BOTH the WHERE clause name AND the comment value embedded in the `json_merge_patch` JSON literal. Planner verifies in Plan 03/04 |
| JSON injection via comment content | T | `escape_sql_arg` doubles single quotes; the JSON literal `'{"comment":"<user_content>"}'` is single-quoted, so SQL injection guards apply. JSON-level injection (a `"` in the comment) is escaped by serializing via `serde_json::to_string` (current code) — Plan 04 must continue to use `serde_json` to build the JSON literal, NOT manual concatenation |
| Path traversal in CREATE FROM YAML FILE | T | DuckDB's `read_text` honors path safety settings; the spike confirms (`65-ALTER-REWRITE-SPIKE.md` doesn't address this directly, but the existing v0.9.0 code already delegates and is fine). No new surface |
| Resource exhaustion via large YAML | D | YAML_SIZE_CAP (`model.rs:467` = 1 MiB) — Plan 04 helper TF must call `from_yaml_with_size_cap` (already does) |
| Memory unsafety in new FFI bridge | T | Plan 05's Rust↔C++ bridge needs `catch_unwind` on Rust side, try/catch on C++ side. Spike code already shows this pattern in the bind callback |

---

## 12. Risks and Known-Falsified Alternatives

**These are spike-verified dead ends. Do NOT re-propose under any circumstance.**

- **`parse_function` + `plan_function` for write-path DDL.** Falsified by `Binder::Bind(ExtensionStatement &)` analysis in `65-03-SUMMARY.md` and confirmed by `65-EXEC-TIME-SPIKE.md` (`EXEC-TIME-RC1` self-deadlock on `ClientContext::context_lock`). The binder builds a `LogicalGet` from `parse_result.function`, NOT an `InsertStatement` from re-parsing `native_sql`. The only DuckDB v1.5.2 mechanism that delivers transactional CREATE/DROP/ALTER on the caller's transaction is `parser_override` returning `vector<unique_ptr<SQLStatement>>` from `Parser::ParseQuery(native_sql)`.

- **`context.Query(native_sql)` from any extension callback.** Self-deadlocks on `ClientContext::context_lock` whether called from `parse_function`, `plan_function`, or `TableFunction.func` (exec time). Three independent spikes (`A2-DEADLOCK`, `EXEC-TIME-RC1`, A7 implicit) confirm. The `Connection(*context.db)` mechanism in the spike-verified architecture explicitly opens a FRESH `ClientContext` to sidestep this lock.

- **`duckdb_connect(db_handle)` (C-API) from any callback that has `ClientContext`.** Returns rc=1 (DuckDBError) on every lifecycle phase tested (`D-10` parse thread, `BIND-THREAD-RC1` bind thread, `PLAN-THREAD-RC1` plan thread). Probably because the `reinterpret_cast<DatabaseWrapper *>(db_handle->internal_ptr)` yields a stale/wrong pointer relative to `context.db`. The C++ direct path `Connection(*context.db)` succeeds at the same call site. **Implication:** the new bind callbacks in Plan 05 cannot use the C-API to open connections; they must use the C++ direct path and bridge to Rust afterward.

- **Detect access-mode mismatch on reopen and surface an error.** Explicitly forbidden by D-23 (root-cause over symptom hacks) and D-01 (PRE-BPRIME). Even if it were technically achievable, it would not satisfy the LIFE-02 "deterministic teardown" wording.

- **Re-introducing a long-lived `duckdb_connection` in `init_extension` for any reason.** Forbidden by success criterion 4 and enforced by the Plan 06 structural guard test.

- **Per-call C-API `ConnGuard` inside `parser_override`.** This was the PRE-BPRIME architecture; killed by D-10 / `BIND-THREAD-RC1` (the C-API `duckdb_connect` from the parse/bind thread returns rc=1). The Plan 02 partial commits implementing this returned `Parser Error: catalog connection failed: duckdb_connect failed (rc=1)` on every parser_override sqllogictest reaching catalog-read paths (43 of 47 tests failed) — see `65-02-A7-test-sql-evidence.log`. Plan 03's revert removes these commits.

- **`json_set` / `json_remove` in DuckDB v1.5.2.** Not implemented. Plan 04 uses `json_merge_patch` instead — see §4.1.

- **LRU cache for type inference at read-side.** Phase 62 retired the v0.8.0 LRU (TECH-DEBT #20) because of silent-eviction error class. Plan 05's process-local cache (§6.2) must be unbounded HashMap or `OnceLock` per session — NOT a bounded LRU.

---

## 13. DuckDB Upstream Code-Level Anchors

**Required reading for Plan 04 and 05 planner / executors:**

- `cpp/include/duckdb.cpp:369065-369085` — `Binder::Bind(ExtensionStatement &)`. Internalize WHY `parse_function` + `plan_function` cannot deliver transactional DDL. **Do not re-propose paths through here for the write path.**
- `cpp/include/duckdb.cpp:370905-370936` — `BindUpdateSet::PlanSubqueries`. Confirms UPDATE-with-TF-subquery binds. **Foundation for Plan 04's `__sv_compute_create_from_yaml` shape.**
- `cpp/include/duckdb.cpp:266432-266447` — `duckdb_connect` C-API implementation. Helps explain why the C-API path fails (`reinterpret_cast` to `DatabaseWrapper *`) but the C++ direct `Connection(*context.db)` succeeds.
- `cpp/include/duckdb.cpp:276819` — `~DatabaseInstance` resetting `connection_manager`. Relevant to understanding why the v0.8.0 `INTENTIONAL LEAK` was actually necessary at the time, and why retiring the connection entirely (Plan 06) eliminates the problem rather than fixing the leak.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| `CREATE / DROP / ALTER SEMANTIC VIEW` | DuckDB binder + `parser_override` rewrite | Rust (`src/parse.rs`) | `parser_override` returns native SQL; DuckDB binds on caller's conn (D-21 transactional). Rust does the rewrite mechanically. |
| ALTER variant JSON mutation | DuckDB SQL (`json_merge_patch`) | none | Avoids round-trip through Rust to mutate JSON; runs on caller's conn |
| `CREATE FROM YAML FILE` parsing + enrichment | Helper TF bind callback (C++ + Rust FFI) | Rust (`SemanticViewDefinition::from_yaml_with_size_cap`) | YAML parse needs per-call `Connection(*context.db)` for type inference; helper TF is the registered surface |
| Read-side TF execution (`list`, `describe`, `show_*`, `semantic_view`, `explain_*`) | C++ Catalog API TF bind + Rust function body | Rust (`src/ddl/*`) | Bind callback receives `ClientContext`, opens per-call connection, calls into Rust |
| `get_ddl` / `read_yaml_from_semantic_view` (scalars) | C++ Catalog API ScalarFunction + Rust body | Rust | Same pattern as TFs, scalar registration variant |
| Metadata capture (`created_on`, `database_name`, `schema_name`) | DuckDB SQL (`now()`, `current_database()`, `current_schema()`) | none | Was Rust+C-API in v0.9.0; moves to SQL expressions in INSERT (Plan 03) |
| Type inference (LIMIT 0 probe) | Read-side bind callback (lazy) | Process-local cache | Was CREATE-time in v0.9.0; moves to read-time + cached (D-16) |
| PK auto-inference from `duckdb_constraints()` | **RETIRED** | — | D-05 — Snowflake-aligned: PKs are logical assertions, not physical imports |
| Race-guard for DROP/ALTER concurrent dropper | DuckDB SQL (two-statement guard from Phase 60) | — | Unchanged (D-13) |
| OverrideContext / catalog connection in parser_override | **RETIRED** | — | The whole point of Phase 65 |

---

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` (cargo test + sqllogictest + DuckLake CI tests) — Phase 65 verification incomplete without this. All four plans must keep it green.
- **Pre-push gate:** `just ci` (adds lint + fmt + cargo-deny + fuzz target compile). Plan 06 verifies before claiming phase complete.
- **Rust toolchain:** pinned in `rust-toolchain.toml`; Dependabot auto-bumps. No version assumptions in plans.
- **Milestone branch:** all work on `milestone/v0.10.0`; verify `git branch --show-current` before any commit (per MEMORY.md feedback entries).
- **No parallel builds:** `feedback-no-parallel-builds` — all `cargo` / `just build` foreground, one at a time.
- **No background GSD agents:** `feedback-no-background-agents` — never `run_in_background` an executor/verifier.
- **Tail caveat:** `feedback-no-tail-on-long-commands` — use `cmd > $TMPDIR/x.log; tail -N $TMPDIR/x.log` not bare `cmd | tail -N`.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | DuckDB v1.5.2 `json_merge_patch` implements RFC-7396 (null-as-delete semantics) for UNSET COMMENT | §4.1 | Medium — Plan 04 W0 spike verifies in 5 minutes; if false, UNSET COMMENT needs a regenerate-from-helper-TF path. The CONTEXT D-09 commitment to "trivial pure-SQL rewrite" for UNSET COMMENT degrades to "trivial helper-TF" — not a redesign, just an upgrade in mechanism complexity. |
| A2 | The Plan 02 partial's `sv_register_table_function` was reverted before commit and is NOT in HEAD | §5.1 | Medium — Plan 04 will need to introduce the infrastructure from scratch (using spike code as template). Increases Plan 04 scope by ~150 LOC of C++. Surfaced here so the planner doesn't allocate "kept infrastructure" to a task that doesn't exist. |
| A3 | The Rust↔C++ bridge for read-side bind callbacks (Plan 05) can extract a `duckdb_connection` from a C++ `Connection(*context.db)` via internal pointer manipulation OR by wrapping `Connection`'s `context` field directly | §1.3, §5 | Medium — if this bridge is harder than expected, Plan 05 may need to rewrite read-side bodies in C++ rather than keeping them in Rust. The spike code in `65-READ-PATH-SPIKE.md` is purely C++; it never exercises the bridge. **Recommendation: Plan 05's first task is the bridge spike.** |
| A4 | DuckDB v1.5.2 supports scalar function registration via `system_catalog.CreateFunction(txn, ScalarFunctionSet)` | §5.3 | Low — well-established API used by many community extensions. Worst case: scalar registration uses a slightly different Info subtype than table-function registration; ~30 LOC difference. |
| A5 | `OnceLock<RwLock<HashMap<...>>>` for the process-local type cache is sufficient — no need for finer-grained invalidation | §6.2 | Low — cache key includes `schema_fingerprint` so ALTER invalidates naturally. The TECH-DEBT #19 committed-state caveat means stale reads CAN occur cross-transaction, but no worse than v0.9.0 |
| A6 | Community extensions (postgres_scanner, iceberg, ducklake, httpfs) use canonical `Connection(*context.db)` per-call pattern for catalog reads from bind callbacks | §5.3 | Low — `[ASSUMED]` based on training data and the spike's "no new mechanism" framing. Planner verifies with one quick read during Plan 05 Wave 0. |
| A7 | The 8 future ALTER variants (MAKE PRIVATE/PUBLIC, SET TAG, ADD SYNONYMS, ADD DIMENSION/METRIC/FACT, DROP DIMENSION/METRIC/FACT/RELATIONSHIP, ADD RELATIONSHIP) are out of scope for Phase 65 per D-22 | §4.2 | Low — CONTEXT D-22 supports this scoping; the table in CONTEXT `<specifics>` is informational. But if the user actually wanted them in scope, Plan 04 doubles in size. **Planner should re-confirm with user during /gsd-discuss-phase follow-up if uncertain.** |

---

## Sources

### Primary (HIGH confidence)
- `.planning/phases/65-overridecontext-connection-teardown/65-CONTEXT.md` — locked architecture (this phase)
- `.planning/phases/65-overridecontext-connection-teardown/65-BPRIME-ARCHIVE-NOTE.md` — pivot rationale
- `.planning/phases/65-overridecontext-connection-teardown/65-OPTION-B-SPIKE.md` — `Connection(*context.db)` works from plan thread (PLAN-THREAD-RC0 + RC1 split)
- `.planning/phases/65-overridecontext-connection-teardown/65-READ-PATH-SPIKE.md` — bind-thread C++ direct path works (READ-BIND-RC0)
- `.planning/phases/65-overridecontext-connection-teardown/65-EXEC-TIME-SPIKE.md` — `context.Query` from exec time self-deadlocks (EXEC-TIME-RC1)
- `.planning/phases/65-overridecontext-connection-teardown/65-ALTER-REWRITE-SPIKE.md` — UPDATE-with-TF-subquery viable on DuckDB v1.5.2 (ALTER-RC0)
- `.planning/phases/65-overridecontext-connection-teardown/65-02-SPIKES.md` — A2-DEADLOCK + A7-RE-ENTRANCY-UNSAFE
- `src/parse.rs`, `src/lib.rs`, `src/ddl/define.rs`, `cpp/src/shim.cpp` — direct code reads (HEAD = Plan 02 partial state)
- `git show 0d2c0b7^:src/parse.rs` — verified v0.9.0 `OverrideContext` shape (§2)
- `git show 0d2c0b7^:cpp/src/shim.cpp` — verified v0.9.0 `sv_register_parser_hooks` signature
- `src/catalog.rs:35-42` — verified `_definitions` schema (`name VARCHAR PK, definition VARCHAR`)
- `src/model.rs:357-410` — verified `SemanticViewDefinition` field layout

### Secondary (MEDIUM confidence)
- `https://duckdb.org/docs/current/data/json/json_functions` — JSON function inventory; confirmed no `json_set` / `json_remove`
- `https://duckdb.org/docs/current/data/json/creating_json` — `json_merge_patch` exists; full RFC-7396 behavior unverified (Plan 04 W0 spike addresses)

### Tertiary (LOW confidence — `[ASSUMED]`)
- Community extension patterns (postgres_scanner, iceberg, ducklake) — `[ASSUMED]` based on training data; planner verifies during Plan 05 Wave 0

## Assumption Resolutions (locked 2026-05-23 during plan-phase)

- **A1 — `json_merge_patch` RFC-7396 semantics:** Plan 04 Wave 0 = 5-min sqllogictest spike confirming null-as-delete. Confirmed by user during plan-phase.
- **A3 — Rust↔C++ bridge:** Plan 05 Wave 0 = spike confirming Rust callbacks reachable from C++ bind via `sv_register_table_function` and that `Connection(*context.db)` is usable from the bridged callback. Confirmed by user during plan-phase.
- **A7 — 8 unimplemented ALTER variants:** **Out of scope and dropped — not deferred.** User rationale (locked 2026-05-23): "they are non-features — Snowflake doesn't have them." Per CLAUDE.md the project tracks Snowflake semantic-views syntax/behavior. Plan 04 migrates only the 3 existing ALTER variants (RENAME TO, SET COMMENT, UNSET COMMENT) plus CREATE FROM YAML FILE. The CONTEXT.md `<specifics>` ALTER table and D-09/D-10 enumeration of 8 additional variants are superseded by this resolution.

## RESEARCH COMPLETE

The four plans differ in scope, dependency, and blast radius as follows. **Plan 03 (slimming)** is mostly a revert + delete operation (~700 LOC of net deletion) with one architectural change (metadata via SQL expressions embedded in INSERT). Blast radius: medium — touches every parser_override rewrite path but the test surface is well-covered by existing sqllogictests. **Plan 04 (ALTER architecture)** is the smallest plan if the 8 future ALTER variants are deferred (3 existing variants only, plus the `__sv_compute_create_from_yaml` helper TF — ~200-300 LOC of new code). Blast radius: small — only touches CREATE FROM YAML FILE and the 3 existing ALTER variants. **Plan 05 (read-path)** is the largest plan: 17 read-side registrations to migrate, plus the Rust↔C++ bridge mechanism, plus type-inference deferral. Blast radius: highest — every SHOW/DESCRIBE/`semantic_view()` user-visible call path goes through this. Migration is incremental within the plan (suggested 6-wave sequence in §1.3). The final commit retires H2 `query_conn`. **Plan 06 (lifecycle close-out)** is the smallest plan by LOC (~150 LOC total: H1 deletion, OverrideContext slim, structural guard test, 4 watchdog test additions, ledger update). Blast radius: low — but high-stakes because the watchdog tests are the LIFE-01 acceptance evidence. Dependencies: Plan 03 must land before Plans 04 + 05; Plan 04 and Plan 05 are independent and could run in parallel (but per D-24 no time pressure, sequential is fine); Plan 06 depends on both 04 and 05 retiring their respective use of H1 / H2 first.

Sources:
- [DuckDB JSON Processing Functions](https://duckdb.org/docs/current/data/json/json_functions)
- [DuckDB Creating JSON](https://duckdb.org/docs/current/data/json/creating_json)
