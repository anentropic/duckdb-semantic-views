// src/shim/shim.h
// extern "C" boundary between the C++ shim and Rust.
// Must be includable from both C++ (shim.cpp) and plain C.
#pragma once

#include "duckdb.h"

#ifdef __cplusplus
extern "C" {
#endif

// Called from Rust init_extension to wire up C++ hooks.
// Phase 10: registers pragma_query_t callbacks for define/drop.
// Phase 11: also registers parser_extension hooks for CREATE/DROP SEMANTIC VIEW DDL.
// db_instance_ptr is the raw duckdb_database pointer (cast chain via capi_internal.hpp).
// catalog_raw_ptr is an opaque pointer to the Rust CatalogState (Arc<RwLock<HashMap>>).
// persist_conn is the pre-created separate DuckDB connection for DDL persistence writes.
void semantic_views_register_shim(
    void* db_instance_ptr,
    const void* catalog_raw_ptr,
    duckdb_connection persist_conn
);

// Called from the C++ parser hook scan function to persist a view definition.
// conn is the separate persist_conn (NOT the main connection — separate context, no deadlock).
// Returns 0 on success, -1 on error.
int32_t semantic_views_pragma_define(
    duckdb_connection conn,
    const char* name,
    const char* json
);

// Called from the C++ parser hook scan function to delete a view definition.
// Returns 0 on success, -1 on error.
int32_t semantic_views_pragma_drop(
    duckdb_connection conn,
    const char* name
);

// ---------------------------------------------------------------------------
// Catalog mutation FFI — implemented in Rust (src/catalog.rs ffi_catalog).
// Called from the C++ parser hook scan function to update the in-memory catalog.
// catalog_ptr is an opaque pointer to the Rust CatalogState (Arc<RwLock<HashMap>>).
// All functions return 0 on success, -1 on error.
// ---------------------------------------------------------------------------

/// Insert a new semantic view. Returns -1 if view already exists or json is invalid.
int32_t semantic_views_catalog_insert(
    const void* catalog_ptr,
    const char* name,
    const char* json
);

/// Upsert a semantic view (insert or replace). Returns -1 if json is invalid.
int32_t semantic_views_catalog_upsert(
    const void* catalog_ptr,
    const char* name,
    const char* json
);

/// Delete a semantic view. Returns -1 if view does not exist.
int32_t semantic_views_catalog_delete(
    const void* catalog_ptr,
    const char* name
);

/// Delete a semantic view if it exists. Silently succeeds if absent. Returns 0 always.
int32_t semantic_views_catalog_delete_if_exists(
    const void* catalog_ptr,
    const char* name
);

#ifdef __cplusplus
}
#endif
