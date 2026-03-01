# Phase 10: pragma_query_t Catalog Persistence - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Replace the sidecar `.semantic_views` file with DuckDB-native table persistence. The C++ shim
registers a `pragma_query_t` callback that writes/deletes rows in `semantic_layer._definitions`
directly — making those writes participate in DuckDB's transaction system. No user-facing API
change: `define_semantic_view()` and `drop_semantic_view()` remain the public interface. The
sidecar file mechanism (read, write, sidecar_path) is fully deleted from the codebase.

</domain>

<decisions>
## Implementation Decisions

### Migration from v0.1.0 sidecar files
- `init_catalog` performs a one-time migration on first load: read the sidecar, insert any
  definitions not already in the table, then **delete the sidecar file**.
- After migration, the sidecar path is never referenced again — the file disappears cleanly.
- This migration is automatic; users take no action on upgrade.
- If the sidecar doesn't exist (new install or already migrated), init_catalog skips silently.

### Write failure behavior
- If the `pragma_query_t` write to `semantic_layer._definitions` fails, `define_semantic_view()`
  returns an error. The in-memory HashMap is also rolled back (insert is not committed).
- No silent degradation to in-memory-only state. Fail loudly so users can investigate.
- Error message should be specific: include the view name and the underlying DuckDB error.

### In-memory database handling
- In-memory databases (`:memory:`) do **not** use `pragma_query_t` for writes. The in-memory
  `_definitions` table already exists (created in `init_catalog`) and there is no cross-session
  persistence to worry about — the HashMap is the sole source of truth for the session.
- Keep the `:memory:` check to skip the pragma write path. This avoids unnecessary C++ FFI
  overhead for in-memory use.

### Sidecar removal scope
- ALL sidecar code is deleted: `sidecar_path()`, `read_sidecar()`, `write_sidecar()` functions,
  and all tests that reference sidecar behavior.
- `catalog.rs` tests covering the table-backed behavior remain and are expanded.
- `write_sidecar` imports in `ddl/define.rs` and `ddl/drop.rs` are removed.
- `DefineState` and `DropState` lose the `db_path` field (no longer needed for sidecar).

### pragma_query_t call boundary
- The C++ shim registers a `pragma_query_t` callback at load time
  (`semantic_views_register_shim` will have real logic in Phase 10).
- Rust calls the shim from `define_semantic_view` invoke via an `extern "C"` function:
  `semantic_views_pragma_define(name_ptr, json_ptr)` and `semantic_views_pragma_drop(name_ptr)`.
- The shim executes: `INSERT OR REPLACE INTO semantic_layer._definitions VALUES (?, ?)` or
  `DELETE FROM semantic_layer._definitions WHERE name = ?` via the `pragma_query_t` mechanism.

### Transaction semantics (PERSIST-02)
- Because the shim writes via DuckDB's `pragma_query_t`, the INSERT/DELETE automatically
  participates in the calling transaction.
- `BEGIN; SELECT define_semantic_view(...); ROLLBACK;` leaves the catalog table unchanged.
- The in-memory HashMap must also be rolled back. Since scalar function invoke happens inside
  the transaction, we need to handle the HashMap carefully:
  - **Option A**: Update HashMap optimistically (as today), rely on the next `init_catalog`
    load to sync from table if the DB is reopened. Acceptable because in-session HashMap
    and table can diverge only within a rolled-back transaction that ends the session.
  - **Option B**: Don't update HashMap until after pragma write succeeds.
  - **Claude's Discretion**: Choose the approach that keeps HashMap consistent with the
    committed table state. If pragma_query_t fails or transaction is rolled back,
    the HashMap should reflect the pre-define state.

### Claude's Discretion
- Exact C++ `pragma_query_t` API usage (DuckDB C API call signatures)
- Whether to use `UPSERT` (`INSERT OR REPLACE`) or `DELETE + INSERT` for idempotency in
  edge cases during migration
- Whether `semantic_views_pragma_define` / `_drop` FFI functions are synchronous or
  return an error code (prefer error code + propagate to Rust Result)
- Naming of the internal shim PRAGMA (not user-visible)

</decisions>

<specifics>
## Specific Ideas

- Success criterion 2 uses the phrasing `PRAGMA define_semantic_view_internal(...)` — this
  suggests the test goes through the raw PRAGMA callback directly, not via the scalar function
  wrapper. The planner should include a test that calls the PRAGMA directly to verify ROLLBACK.
- PERSIST-03: `grep -r "semantic_views"` on file paths must return nothing — the sidecar
  extension (`.semantic_views`) cannot appear anywhere in source code.

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `catalog.rs: init_catalog()` — already creates `semantic_layer._definitions` table; will keep
  this initialization, just add the one-time migration step then remove sidecar reading
- `catalog.rs: sync_table_from_map()` — can be reused for the migration step (populate table
  from sidecar contents), then removed after migration code is written
- `catalog.rs: catalog_insert()` / `catalog_delete()` — keep as-is for in-memory HashMap ops;
  pragma write is added alongside these calls in the DDL invoke handlers
- `src/shim/mod.rs` — currently empty placeholder; Phase 10 adds `extern "C"` declarations
  for the pragma FFI functions here
- `shim/shim.cpp` (Phase 8) — `semantic_views_register_shim` stub gains real pragma callback
  registration logic in Phase 10

### Established Patterns
- `:memory:` special-case check exists in `ddl/define.rs:70` and `ddl/drop.rs` — keep this
  pattern to gate pragma writes (file-backed only) while in-memory goes through HashMap only
- `DefineState` / `DropState` structs carry shared state into invoke; `db_path` field can be
  removed; no new fields needed since pragma is called via FFI (no Rust Connection required)
- Error propagation: invoke returns `Result<(), Box<dyn Error>>` — pragma FFI errors should
  map to this type

### Integration Points
- `lib.rs: semantic_views_register_shim(db_handle)` call (line 135) — this is where the shim
  gains its pragma callback registration; the shim needs the db_handle to call
  `duckdb_pragma_register` (or equivalent C API)
- `ddl/define.rs: invoke()` — after `catalog_insert(...)`, replace `write_sidecar(...)` call
  with the pragma FFI call
- `ddl/drop.rs: invoke()` — same replacement for `catalog_delete` + `write_sidecar`
- `catalog.rs: init_catalog()` — add one-time migration block, remove sidecar reading block

</code_context>

<deferred>
## Deferred Ideas

- None — discussion stayed within phase scope.

</deferred>

---

*Phase: 10-pragma-query-t-catalog-persistence*
*Context gathered: 2026-03-01*
