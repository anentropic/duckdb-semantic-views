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
// Phase 11 will add parser_extension hooks for CREATE SEMANTIC VIEW DDL.
// db_instance_ptr is the raw duckdb_database pointer (cast chain via capi_internal.hpp).
void semantic_views_register_shim(void* db_instance_ptr);

// Called from Rust define_semantic_view invoke to persist a view definition.
// conn is the separate persist_conn created at init time (NOT the main connection).
// Using a separate connection avoids deadlock with the main connection's execution lock.
// Returns 0 on success, -1 on error.
int32_t semantic_views_pragma_define(
    duckdb_connection conn,
    const char* name,
    const char* json
);

// Called from Rust drop_semantic_view invoke to delete a view definition.
// Returns 0 on success, -1 on error.
int32_t semantic_views_pragma_drop(
    duckdb_connection conn,
    const char* name
);

#ifdef __cplusplus
}
#endif
