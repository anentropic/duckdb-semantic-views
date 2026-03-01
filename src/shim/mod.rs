// src/shim/mod.rs
// Phase 11: The C++ shim is a no-op stub. All DDL functionality is
// implemented in Rust (src/ddl/) using the duckdb-rs VScalar interface.
//
// The DuckDB C++ API (parser extensions, ExtensionLoader, PragmaFunction)
// and C API symbols (duckdb_query, duckdb_destroy_result) are not accessible
// via direct symbol references from a Python-DuckDB-loaded extension:
// - C++ symbols: compiled with -fvisibility=hidden in the Python bundle
// - C API symbols: accessed through function-pointer tables, not exported
//
// Rust duckdb-rs accesses C API functions through AtomicPtr indirection,
// which works correctly with the Python DuckDB extension loading mechanism.
//
// This module is kept for the extern "C" shim entry point declaration.

#[cfg(feature = "extension")]
pub mod ffi {
    // No FFI declarations needed — all persistence is done in Rust using
    // ffi::duckdb_query through the loadable-extension function pointer table.
}
