//! GET_DDL scalar function: wraps [`crate::render_ddl::render_create_ddl`] as a
//! `VScalar` so that `SELECT GET_DDL('SEMANTIC_VIEW', 'name')` works inside DuckDB.
//!
//! The render logic itself lives in [`crate::render_ddl`] (always compiled, unit-tested
//! under `cargo test`). This module adds the extension-only VScalar registration.
//!
//! # Phase 65 Plan 05 Task 4 (Wave 3)
//!
//! `get_ddl` is registered via the C++ Catalog API path
//! (`sv_register_get_ddl` in `cpp/src/shim.cpp`). The legacy
//! [`GetDdlScalar`] `VScalar` impl below is retained and marked
//! `#[allow(dead_code)]` for the duration of Plan 05; the Wave 6 cleanup
//! commit deletes it together with the other dead VTab/VScalar carcasses.
//! All live invocations of `SELECT GET_DDL(...)` route through
//! [`sv_get_ddl_exec_rust`] below.

use duckdb::core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId};
use duckdb::types::DuckString;
use duckdb::vscalar::{ScalarFunctionSignature, VScalar};
use duckdb::vtab::arrow::WritableVector;
use libduckdb_sys::duckdb_string_t;

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
    use crate::ddl::read_ffi::{probe_catalog_table_present, publish_owned_buffer, write_err};
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if conn.is_null() {
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

        let reader = CatalogReader::new(conn, probe_catalog_table_present(conn));
        let json = match reader.lookup(name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("semantic view '{name}' does not exist"),
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

#[allow(dead_code)]
pub struct GetDdlScalar;

impl VScalar for GetDdlScalar {
    type State = CatalogReader;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            let len = input.len();
            let type_vec = input.flat_vector(0);
            let name_vec = input.flat_vector(1);
            let types = type_vec.as_slice_with_len::<duckdb_string_t>(len);
            let names = name_vec.as_slice_with_len::<duckdb_string_t>(len);
            let out_vec = output.flat_vector();

            for i in 0..len {
                let obj_type = DuckString::new(&mut { types[i] }).as_str().to_string();
                let name = DuckString::new(&mut { names[i] }).as_str().to_string();

                if !obj_type.eq_ignore_ascii_case("SEMANTIC_VIEW") {
                    return Err(format!(
                        "GET_DDL: unsupported object type '{}'. Only 'SEMANTIC_VIEW' is supported.",
                        obj_type
                    )
                    .into());
                }

                let json = state
                    .lookup(&name)
                    .map_err(Box::<dyn std::error::Error>::from)?
                    .ok_or_else(|| format!("semantic view '{}' does not exist", name))?;
                let def: SemanticViewDefinition = serde_json::from_str(&json)?;
                let ddl =
                    render_create_ddl(&name, &def).map_err(|e| -> Box<dyn std::error::Error> {
                        format!("GET_DDL error: {e}").into()
                    })?;
                out_vec.insert(i, ddl.as_str());
            }
            Ok(())
        }))
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // object_type
                LogicalTypeHandle::from(LogicalTypeId::Varchar), // name
            ],
            LogicalTypeHandle::from(LogicalTypeId::Varchar), // return: DDL string
        )]
    }
}
