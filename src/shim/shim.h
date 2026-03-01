// src/shim/shim.h
// extern "C" boundary between the C++ shim and Rust.
// Must be includable from both C++ (shim.cpp) and plain C.
//
// Phase 11: The shim is a no-op stub. All DDL functionality is implemented
// in Rust using the duckdb-rs VScalar interface, which accesses DuckDB
// through function-pointer tables (loadable-extension compatible).
#pragma once

#include "duckdb.h"

#ifdef __cplusplus
extern "C" {
#endif

// Called from Rust init_extension — no-op stub.
// Signature must match the extern "C" declaration in src/lib.rs.
void semantic_views_register_shim(
    void* db_instance_ptr,
    const void* catalog_raw_ptr,
    duckdb_connection persist_conn
);

#ifdef __cplusplus
}
#endif
