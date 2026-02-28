---
phase: 02-storage-and-ddl
verified: 2026-02-24T22:50:00Z
status: passed
score: 10/10 must-haves verified
re_verification:
  previous_status: gaps_found
  previous_score: 9/10
  gaps_closed:
    - "Definitions survive a DuckDB restart: after closing and reopening a file-backed DB, registered views are still present (DDL-05)"
  gaps_remaining: []
  regressions: []
---

# Phase 2: Storage and DDL Verification Report

**Phase Goal:** Users can register, inspect, and remove semantic view definitions, and those definitions survive a DuckDB restart
**Verified:** 2026-02-24T22:50:00Z
**Status:** passed
**Re-verification:** Yes — after gap closure for DDL-05 (sidecar persistence)

## Gap Closure Summary

The single gap from initial verification was DDL-05: `define_semantic_view` and `drop_semantic_view` invoked `Connection::open(":memory:")` inside their `invoke` callbacks, writing catalog changes to an ephemeral database that was discarded at end of call. The host database's `semantic_layer._definitions` table received no rows, so definitions were lost on restart.

The gap was closed via three commits:

- `1e24914`: Resolve host DB path in the extension entrypoint via `PRAGMA database_list` (filters to first row with non-empty file path, handles Python DuckDB stem-naming)
- `824c60e`: Sidecar persistence — `write_sidecar` / `read_sidecar` in `catalog.rs`; `init_catalog` merges sidecar into the DuckDB table on load; `define.rs` and `drop.rs` call `write_sidecar` after each mutation
- `13d4819`: Restart integration test made idempotent (drops view after verification so sidecar is clean for subsequent runs)

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `SELECT define_semantic_view('orders', '{...}')` registers the definition and returns a confirmation string | VERIFIED | `src/ddl/define.rs` VScalar implementation; SQLLogicTest section 1 returns `"Semantic view 'customers' registered successfully"`; `semantic_layer._definitions` table written via sidecar sync on next load |
| 2 | `SELECT drop_semantic_view('orders')` removes the definition; subsequent `describe_semantic_view('orders')` errors | VERIFIED | `src/ddl/drop.rs` VScalar; SQLLogicTest sections 6-7 pass: drop returns confirmation, list no longer shows view, describe returns "does not exist" error |
| 3 | `FROM list_semantic_views()` returns one row per registered view | VERIFIED | `src/ddl/list.rs` VTab; SQLLogicTest sections 2, 8, 9: two-view list matches expected rows; zero-view count after cleanup equals 0 |
| 4 | `FROM describe_semantic_view('orders')` returns structured fields: name, dimensions, metrics, base table, filters | VERIFIED | `src/ddl/describe.rs` VTab; SQLLogicTest sections 3-4: 6 VARCHAR columns (name, base_table, dimensions, metrics, filters, joins) returned correctly; unknown name produces "does not exist" error |
| 5 | After closing and reopening the DuckDB file, all previously registered semantic views are available | VERIFIED | SQLLogicTest section 10: `load __TEST_DIR__/restart_test.db` + `define_semantic_view` + `restart` + `FROM list_semantic_views()` — view present after restart; `write_sidecar` called in `define.rs:71`; `init_catalog` reads sidecar and syncs table on load; `init_catalog_loads_from_sidecar` unit test also passes |

**Score:** 10/10 truths verified

---

## Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/model.rs` | SemanticViewDefinition with serde deserialization | VERIFIED | `from_json` validates required fields; 5 unit tests all pass |
| `src/catalog.rs` | CatalogState, init_catalog (with sidecar merge), catalog_insert, catalog_delete, write_sidecar | VERIFIED | All functions present; `init_catalog` takes `db_path` param and merges sidecar when path is not `:memory:`; atomic write via tmp+rename; 11 unit tests all pass |
| `src/ddl/define.rs` | DefineSemanticView VScalar calling catalog_insert then write_sidecar | VERIFIED | Lines 65-72: `catalog_insert` then `write_sidecar` if `db_path != ":memory:"`; `DefineState` carries both `catalog` and `db_path` |
| `src/ddl/drop.rs` | DropSemanticView VScalar calling catalog_delete then write_sidecar | VERIFIED | Lines 50-55: `catalog_delete` then `write_sidecar` if `db_path != ":memory:"`; `DropState` carries both |
| `src/ddl/list.rs` | ListSemanticViewsVTab returning (name, base_table) per registered view | VERIFIED | Two-column VTab; bind-time catalog snapshot |
| `src/ddl/describe.rs` | DescribeSemanticViewVTab returning 6-column row for named view | VERIFIED | Six VARCHAR columns; error on unknown name |
| `src/lib.rs` | Entrypoint resolves host DB path via PRAGMA database_list, passes to init_catalog and both state structs | VERIFIED | Lines 51-63: `PRAGMA database_list` query filtering non-empty file; `db_path` passed to `init_catalog`, `DefineState`, `DropState` |
| `test/sql/phase2_ddl.test` | SQLLogicTest covering DDL-01..05 including restart section | VERIFIED | 10 sections; section 10 uses `load`/`restart` cycle; both `phase2_ddl.test` and `semantic_views.test` pass with SUCCESS |

---

## Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `lib.rs` entrypoint | host DB path | `PRAGMA database_list` | WIRED | Lines 52-57: `con.prepare("PRAGMA database_list")`, filter `!file.is_empty()`, fallback `:memory:` |
| `lib.rs` entrypoint | `init_catalog` | `db_path` Arc passed as second arg | WIRED | Line 69: `init_catalog(&con, &db_path)` |
| `lib.rs` entrypoint | `DefineState` / `DropState` | `db_path.clone()` in state structs | WIRED | Lines 75-85: `db_path: db_path.clone()` in both state structs |
| `src/ddl/define.rs` invoke | `write_sidecar` | Called after `catalog_insert` for file-backed DB | WIRED | Lines 70-72: guard `!= ":memory:"` then `write_sidecar(&state.db_path, &state.catalog)` |
| `src/ddl/drop.rs` invoke | `write_sidecar` | Called after `catalog_delete` for file-backed DB | WIRED | Lines 53-55: same guard pattern |
| `src/catalog.rs` | sidecar file | `sidecar_path(db_path)` derivation + atomic write | WIRED | Lines 94-129: `<db_path>.semantic_views`; write to `.tmp` then rename |
| `init_catalog` | sidecar merge | `read_sidecar` when `db_path != ":memory:"` | WIRED | Lines 47-55: reads sidecar, merges into map, calls `sync_table_from_map` |
| `init_catalog` | `semantic_layer._definitions` | `sync_table_from_map` on sidecar load | WIRED | Lines 64-71: DELETE + INSERT for each entry; table stays authoritative after first post-restart load |

---

## Requirements Coverage

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| DDL-01 | 02-01, 02-02, 02-03 | User can register a semantic view via `SELECT define_semantic_view('name', '{definition_json}')` | SATISFIED | VScalar in `define.rs`; SQLLogicTest sections 1, 4 pass (registration + duplicate-error) |
| DDL-02 | 02-01, 02-02, 02-03 | User can remove a semantic view via `SELECT drop_semantic_view('name')` | SATISFIED | VScalar in `drop.rs`; SQLLogicTest sections 6, 7 pass (removal + not-found-error) |
| DDL-03 | 02-01, 02-02, 02-03 | User can list all registered semantic views via `FROM list_semantic_views()` | SATISFIED | VTab in `list.rs`; SQLLogicTest sections 2, 8, 9 pass (row counts and column values) |
| DDL-04 | 02-01, 02-02, 02-03 | User can inspect a semantic view definition via `FROM describe_semantic_view('name')` | SATISFIED | VTab in `describe.rs`; SQLLogicTest sections 3, 5 pass (6-column row + unknown-name error) |
| DDL-05 | 02-04 | Semantic view definitions persist across DuckDB restarts | SATISFIED | Sidecar mechanism: `write_sidecar` called in `define.rs` and `drop.rs` invoke; `init_catalog` merges sidecar on load; SQLLogicTest section 10 `restart` cycle passes; `init_catalog_loads_from_sidecar` unit test passes |

**Note on catalog table name:** REQUIREMENTS.md specifies `_semantic_views_catalog`. The implementation uses `semantic_layer._definitions` (schema-qualified). This was an intentional design decision confirmed in `02-CONTEXT.md` — the schema-qualified name is cleaner and consistent with DuckDB conventions. Not a defect.

---

## Anti-Patterns Scan

| File | Line | Pattern | Severity | Notes |
|------|------|---------|----------|-------|
| — | — | — | — | No `TODO`, `FIXME`, `unimplemented!`, `todo!`, placeholder comments, empty return bodies, or console-only handlers found anywhere in `src/` |

The `:memory:` sentinel that appeared as a warning in the previous verification is now correctly confined to:

- In-memory test connections (correct — they cannot persist)
- The fallback branch in `lib.rs` when `PRAGMA database_list` returns no file path (correct — actually in-memory)
- Guard conditions in `define.rs` and `drop.rs` (`!= ":memory:"`) that skip sidecar writes for in-memory sessions (correct design)

---

## Human Verification Required

None. All five success criteria are verifiable programmatically:

- DDL-01 through DDL-04: SQLLogicTest passes confirm actual extension LOAD + SQL round-trips
- DDL-05: SQLLogicTest section 10 executes a real `load`/`restart` cycle against a file-backed database — not simulated

---

## Test Results (Actual Execution)

```
cargo test — 16 tests passed, 0 failed
  model::tests::valid_definition_roundtrips                        ok
  model::tests::missing_base_table_is_error                        ok
  model::tests::invalid_json_is_error                              ok
  model::tests::unknown_fields_are_rejected                        ok
  model::tests::optional_fields_default_to_empty                   ok
  catalog::tests::init_catalog_creates_schema_and_table            ok
  catalog::tests::insert_and_retrieve                              ok
  catalog::tests::duplicate_insert_is_error                        ok
  catalog::tests::delete_removes_from_hashmap                      ok
  catalog::tests::delete_nonexistent_is_error                      ok
  catalog::tests::pragma_database_list_returns_file_path           ok
  catalog::tests::pragma_database_list_returns_none_for_in_memory  ok
  catalog::tests::sidecar_path_derivation                          ok
  catalog::tests::sidecar_round_trip                               ok
  catalog::tests::init_catalog_loads_from_sidecar                  ok
  catalog::tests::init_catalog_loads_existing_rows                 ok

cargo clippy -- -D warnings                                        0 violations
cargo clippy --no-default-features --features extension -- -D warnings  0 violations

just test-sql (SQLLogicTest):
  [1/2] test/sql/phase2_ddl.test   SUCCESS
  [2/2] test/sql/semantic_views.test  SUCCESS
```

---

_Verified: 2026-02-24T22:50:00Z_
_Verifier: Claude (gsd-verifier)_
