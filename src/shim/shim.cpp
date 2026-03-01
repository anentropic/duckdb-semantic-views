// src/shim/shim.cpp
// Phase 11 no-op C++ shim.
//
// All DDL functionality is implemented in Rust (src/ddl/define.rs,
// src/ddl/drop.rs) using the duckdb-rs VScalar interface. The shim
// exists only to satisfy the extern "C" declaration in lib.rs.
//
// Context: The DuckDB C++ API (parser extensions, ExtensionLoader, etc.)
// and even the C API symbols (duckdb_query, duckdb_destroy_result) are
// not directly accessible as symbols from a Python-DuckDB-loaded extension:
// - C++ symbols are compiled with -fvisibility=hidden in the Python bundle
// - C API symbols are accessed through function-pointer tables (not exported
//   as global symbols), so direct symbol references from C++ also fail
//
// Rust duckdb-rs accesses C API functions through AtomicPtr (function pointer
// indirection), which works correctly. C++ code cannot use the same mechanism.
//
// Therefore, all implementation that needs to call DuckDB must be in Rust.
//
// IMPORTANT: Do NOT call any duckdb_* C API functions from this file.
// Even C API functions (duckdb_query, duckdb_destroy_result) are not exported
// as direct symbols from the Python DuckDB bundle and will cause dlopen failure.

#include "shim.h"

// No-op stub — all registration and persistence is handled in Rust.
// The duckdb_connection parameter is unused; cast suppresses unused-var warning.
void semantic_views_register_shim(
    void* db_instance_ptr,
    const void* catalog_raw_ptr,
    duckdb_connection persist_conn_param
) {
    (void)db_instance_ptr;
    (void)catalog_raw_ptr;
    (void)persist_conn_param;
}
