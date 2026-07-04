//! GET_DDL scalar function: wraps [`crate::render_ddl::render_create_ddl`] as a
//! C++ Catalog API scalar so that `SELECT GET_DDL('SEMANTIC_VIEW', 'name')`
//! works inside DuckDB.
//!
//! The render logic itself lives in [`crate::render_ddl`] (always compiled,
//! unit-tested under `cargo test`). This module adds the extension-only Rust
//! FFI dispatcher reached from `sv_register_get_ddl` in `cpp/src/shim.cpp`.
//!
//! # Phase 65 Plan 05 Task 4 (Wave 3) — Batch 3 final cleanup
//!
//! The legacy `GetDdlScalar` `VScalar` impl block was retired in the same
//! commit that deleted the H2 query_conn allocation; all live invocations
//! of `SELECT GET_DDL(...)` now route through [`sv_get_ddl_exec_rust`] below.

use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;
use crate::render_ddl::render_create_ddl;

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 4 (Wave 3) — sv_get_ddl_exec_rust
// ---------------------------------------------------------------------------
// FFI dispatcher for the migrated `get_ddl(object_type, name)` scalar.
// Invoked once per row by the C++ exec callback `sv_get_ddl_exec` in
// cpp/src/shim.cpp. The caller (C++ side) opens a per-call
// `Connection probe(*state.GetContext().db)` and passes it as a borrowed
// `duckdb_connection` — the same borrow contract as the read-path bind
// dispatchers (see `src/ddl/read_ffi.rs` module docs). The Rust side MUST
// NOT call `duckdb_disconnect`; teardown is the C++ scope's responsibility.

/// # Safety
///
/// `conn` is a borrowed handle (do NOT disconnect). `type_ptr` and `name_ptr`
/// must each point to the corresponding number of UTF-8 bytes (not
/// NUL-terminated).
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_get_ddl_exec_rust(
    conn: libduckdb_sys::duckdb_connection,
    type_ptr: *const u8,
    type_len: usize,
    name_ptr: *const u8,
    name_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use crate::ddl::read_ffi::{
        probe_catalog_table_present, publish_owned_buffer, write_err, BorrowedConnection,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        if type_ptr.is_null() || name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "argument pointer is null");
            return 1_u8;
        }
        let type_bytes = std::slice::from_raw_parts(type_ptr, type_len);
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let obj_type = match std::str::from_utf8(type_bytes) {
            Ok(s) => s,
            Err(_) => {
                write_err(error_buf, error_buf_len, "object_type is not valid UTF-8");
                return 1_u8;
            }
        };
        let name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => {
                write_err(error_buf, error_buf_len, "name is not valid UTF-8");
                return 1_u8;
            }
        };

        if !obj_type.eq_ignore_ascii_case("SEMANTIC_VIEW") {
            write_err(
                error_buf,
                error_buf_len,
                &format!(
                    "GET_DDL: unsupported object type '{obj_type}'. Only 'SEMANTIC_VIEW' is supported."
                ),
            );
            return 1_u8;
        }

        let reader = CatalogReader::new(&borrowed, probe_catalog_table_present(&borrowed));
        let json = match reader.lookup(name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &crate::catalog::view_not_found_msg(name),
                );
                return 1_u8;
            }
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let def: SemanticViewDefinition = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };
        let ddl = match render_create_ddl(name, &def) {
            Ok(s) => s,
            Err(e) => {
                write_err(error_buf, error_buf_len, &format!("GET_DDL error: {e}"));
                return 1_u8;
            }
        };
        publish_owned_buffer(ddl.into_bytes(), out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            use crate::ddl::read_ffi::write_err;
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_get_ddl_exec_rust",
            );
            2
        }
    }
}

// Legacy `GetDdlScalar` (duckdb-rs `VScalar` impl) RETIRED — Phase 65
// Plan 05 Batch 3. The C++ Catalog API path
// (`sv_register_get_ddl` → `sv_get_ddl_exec_rust`) is the sole
// registration target.
