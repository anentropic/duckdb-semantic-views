// src/shim/mod.rs
// Phase 10: FFI declarations for C++ pragma functions.
// These are implemented in src/shim/shim.cpp and statically linked
// into the cdylib. They are NOT exported (internal link only).
//
// Two write paths exist:
// 1. semantic_views_register_shim: registers pragma_query_t callbacks —
//    transactional (DuckDB executes returned SQL in caller's transaction).
//    Called at load time from lib.rs. Declared in lib.rs (not here).
// 2. semantic_views_pragma_define / _drop: execute INSERT/DELETE directly
//    on a separate stored connection — NOT in the user's transaction.
//    Called from ddl/define.rs and ddl/drop.rs invoke.
//
// Only feature = "extension" compiles shim.cpp, so these declarations
// are also gated. Unit tests (default/bundled feature) never call them.

#[cfg(feature = "extension")]
pub mod ffi {
    use libduckdb_sys::duckdb_connection;

    extern "C" {
        /// Write a semantic view definition to `semantic_layer._definitions`
        /// using a pre-stored separate connection.
        ///
        /// Returns 0 on success, -1 on error.
        ///
        /// # Safety
        /// `conn` must be a valid `duckdb_connection`. `name` and `json` must
        /// be valid null-terminated C strings for the duration of the call.
        pub fn semantic_views_pragma_define(
            conn: duckdb_connection,
            name: *const std::ffi::c_char,
            json: *const std::ffi::c_char,
        ) -> i32;

        /// Delete a semantic view definition from `semantic_layer._definitions`
        /// using a pre-stored separate connection.
        ///
        /// Returns 0 on success, -1 on error.
        ///
        /// # Safety
        /// `conn` must be a valid `duckdb_connection`. `name` must be a valid
        /// null-terminated C string for the duration of the call.
        pub fn semantic_views_pragma_drop(
            conn: duckdb_connection,
            name: *const std::ffi::c_char,
        ) -> i32;
    }
}
