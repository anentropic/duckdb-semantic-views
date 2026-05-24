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

} // extern "C"
