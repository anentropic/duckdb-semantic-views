// C++ helper declarations for the DuckDB semantic_views extension.
//
// Phase 65 (v0.10.0) Plan 04 (A2 resolution): introduces a reusable C-callable
// `sv_register_table_function` shim that wraps the C++ Catalog API table-function
// registration pattern proven by `65-READ-PATH-SPIKE.md`. Bind callbacks
// registered via this path receive a native `ClientContext &` (not the
// duckdb-rs `BindInfo` wrapper which marshals `ClientContext` away), so they
// can open per-call `Connection(*context.db)` for catalog reads and YAML
// parsing without needing a long-lived extension-owned `duckdb_connection`.
//
// This header is consumed both by C++ translation units inside the shim
// (e.g. `shim.cpp` itself, future helper-TF translation units) and by Rust
// via the existing `extern "C"` declarations in `src/lib.rs` (Plan 05 will
// add Rust-side registration calls).
//
// IMPORTANT: Keep the signatures here in sync with the `extern "C"` block in
// `cpp/src/shim.cpp`. Mismatches surface as undefined symbols at link time.

#pragma once

#include "parser_extension_compat.hpp"

extern "C" {

// Phase 62 (v0.8.0) entry — registers the parser_override + parse_function
// + plan_function hooks. Bundles the catalog connection + is_file_backed flag
// into an OverrideContext owned by the C++ SemanticViewsParserInfo.
bool sv_register_parser_hooks(duckdb_database db_handle,
                              duckdb_connection catalog_conn,
                              bool is_file_backed);

// Phase 65 Plan 04 (A2 resolution) — register a table function via the C++
// Catalog API so its bind callback receives a native `ClientContext &`.
//
// Parameters:
//   db_handle   — the DuckDB C API database handle (unwrapped internally
//                 to a `DatabaseInstance &` for the registration call).
//   name        — UTF-8 NUL-terminated table-function name to register.
//   arg_types   — pointer to `arg_count` `duckdb::LogicalType` values. May
//                 be null when `arg_count == 0`. The values are copied
//                 into the constructed `TableFunction`; the caller can
//                 free the underlying storage after the call returns.
//   arg_count   — number of entries in `arg_types`.
//   bind_cb     — `unique_ptr<FunctionData>(ClientContext &, ...)` callback
//                 invoked during binding; must be non-null.
//   exec_cb     — `void(ClientContext &, TableFunctionInput &, DataChunk &)`
//                 callback invoked during execution; must be non-null.
//   init_cb     — `unique_ptr<LocalTableFunctionState>(ExecutionContext &, ...)`
//                 callback invoked once per execution to construct local
//                 state. May be null when the table function does not
//                 require per-execution local state.
//
// Returns true on success. On failure (registration throws, db handle
// invalid, or any callback null besides the documented optional init_cb),
// logs to stderr and returns false. Uses `OnCreateConflict::ALTER_ON_CONFLICT`
// internally so extension reload does not trip on a duplicate name.
//
// Thread safety: the registration call uses `Catalog::GetSystemCatalog(db)`
// + `CatalogTransaction::GetSystemTransaction(db)`, the same pattern the
// read-path spike (`65-READ-PATH-SPIKE.md`) validated. Safe to call from
// extension init.
bool sv_register_table_function(
    duckdb_database db_handle,
    const char *name,
    const duckdb::LogicalType *arg_types,
    size_t arg_count,
    duckdb::table_function_bind_t bind_cb,
    duckdb::table_function_t exec_cb,
    duckdb::table_function_init_local_t init_cb);

// Phase 65 Plan 05 (Task 2 Step A) — register a scalar function via the C++
// Catalog API, the scalar sibling of sv_register_table_function. Used by
// Task 4 / Wave 4 (get_ddl + read_yaml_from_semantic_view migrations).
//
// Parameters:
//   db_handle   — DuckDB C API database handle (unwrapped to
//                 `DatabaseInstance &` internally).
//   name        — UTF-8 NUL-terminated scalar-function name.
//   arg_types   — pointer to `arg_count` `duckdb::LogicalType` values; may
//                 be null when `arg_count == 0`.
//   arg_count   — number of entries in `arg_types`.
//   return_type — `duckdb::LogicalType` of the scalar's return value.
//   exec_cb     — `void(DataChunk &args, ExpressionState &state, Vector &result)`
//                 callback invoked at execution time; must be non-null.
//
// Returns true on success. Uses `OnCreateConflict::ALTER_ON_CONFLICT` so
// extension reload does not trip on a duplicate name. Logs to stderr on
// failure and returns false.
bool sv_register_scalar_function(
    duckdb_database db_handle,
    const char *name,
    const duckdb::LogicalType *arg_types,
    size_t arg_count,
    duckdb::LogicalType return_type,
    duckdb::scalar_function_t exec_cb);

// Phase 65 Plan 05 (Task 1 / Wave 0 bridge spike) — register the read-side
// `list_semantic_views()` table function via the C++ Catalog API. The bind
// callback opens a per-call `Connection(*context.db)` and bridges to the
// Rust dispatcher (`sv_list_semantic_views_bind_rust`) which performs the
// catalog read on the per-call connection. The bridge mechanism (cast of
// the C++ `Connection *` to `duckdb_connection` — confirmed by reading
// `duckdb.cpp:266432-266447` where `duckdb_connect` does
// `reinterpret_cast<duckdb_connection>(new Connection(...))`) is the
// load-bearing primitive that the remaining 16 read-side migrations will
// reuse. See `65-05-SPIKE-SUMMARY.md` for the LOC extrapolation.
//
// This entry point is the registration helper; the bind/function/init
// callbacks themselves live inside `shim.cpp` (file-static) and are
// passed into `sv_register_table_function` by name.
bool sv_register_list_semantic_views(duckdb_database db_handle);

// Phase 65 Plan 05 Task 2 (Wave 1) — register the migrated
// `list_terse_semantic_views()` table function via the C++ Catalog API.
// 5-column subset of list_semantic_views; same bridge mechanism.
bool sv_register_list_terse_semantic_views(duckdb_database db_handle);

// Phase 65 Plan 05 Task 2 (Wave 1) — register the migrated zero-arg "_all"
// TFs via the C++ Catalog API. All emit homogeneous VARCHAR rows; column
// counts and names match the legacy duckdb-rs registrations.
bool sv_register_show_semantic_dimensions_all(duckdb_database db_handle);
bool sv_register_show_semantic_metrics_all(duckdb_database db_handle);
bool sv_register_show_semantic_facts_all(duckdb_database db_handle);
bool sv_register_show_semantic_materializations_all(duckdb_database db_handle);

// Phase 65 Plan 05 Task 3 (Wave 2) — register the migrated single-arg /
// two-arg TFs via the C++ Catalog API. All take a VARCHAR view-name
// argument (the dimensions_for_metric variant additionally takes a metric
// name). The dimensions_for_metric variant returns 3 VARCHAR + 1 BOOLEAN;
// the rest return homogeneous VARCHAR rows.
bool sv_register_show_columns_in_semantic_view(duckdb_database db_handle);
bool sv_register_describe_semantic_view(duckdb_database db_handle);
bool sv_register_show_semantic_dimensions(duckdb_database db_handle);
bool sv_register_show_semantic_metrics(duckdb_database db_handle);
bool sv_register_show_semantic_facts(duckdb_database db_handle);
bool sv_register_show_semantic_materializations(duckdb_database db_handle);
bool sv_register_show_semantic_dimensions_for_metric(duckdb_database db_handle);

} // extern "C"
