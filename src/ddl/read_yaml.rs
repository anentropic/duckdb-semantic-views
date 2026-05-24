//! `READ_YAML_FROM_SEMANTIC_VIEW` scalar function: wraps
//! [`crate::render_yaml::render_yaml_export`] as a `VScalar` so that
//! `SELECT READ_YAML_FROM_SEMANTIC_VIEW('name')` works inside `DuckDB`.
//!
//! The render logic itself lives in [`crate::render_yaml`] (always compiled,
//! unit-tested under `cargo test`). This module adds the extension-only
//! `VScalar` registration.
//!
//! # Phase 65 Plan 05 Task 4 (Wave 3)
//!
//! `read_yaml_from_semantic_view` is registered via the C++ Catalog API path
//! (`sv_register_read_yaml_from_semantic_view` in `cpp/src/shim.cpp`). The
//! legacy [`ReadYamlFromSemanticViewScalar`] `VScalar` impl below is
//! retained and marked `#[allow(dead_code)]` for the duration of Plan 05;
//! the Wave 6 cleanup commit deletes it together with the other dead
//! VTab/VScalar carcasses. All live invocations of
//! `SELECT READ_YAML_FROM_SEMANTIC_VIEW(...)` route through
//! [`sv_read_yaml_from_semantic_view_exec_rust`] below.

use duckdb::core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId};
use duckdb::types::DuckString;
use duckdb::vscalar::{ScalarFunctionSignature, VScalar};
use duckdb::vtab::arrow::WritableVector;
use libduckdb_sys::duckdb_string_t;

use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;
use crate::render_yaml::render_yaml_export;

/// Extract the bare view name from a potentially qualified name.
/// Supports: `"view_name"`, `"schema.view_name"`, `"database.schema.view_name"`.
fn resolve_bare_name(input: &str) -> &str {
    input.rsplit('.').next().unwrap_or(input)
}

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 4 (Wave 3) — sv_read_yaml_from_semantic_view_exec_rust
// ---------------------------------------------------------------------------
// FFI dispatcher for the migrated `read_yaml_from_semantic_view(name)`
// scalar. Invoked once per row by the C++ exec callback
// `sv_read_yaml_from_semantic_view_exec` in cpp/src/shim.cpp. Same per-call
// borrowed Connection contract as `sv_get_ddl_exec_rust` and the read-path
// bind dispatchers (see `src/ddl/read_ffi.rs` module docs).

/// # Safety
///
/// `conn` is a borrowed handle (do NOT disconnect). `name_ptr` must point
/// to `name_len` UTF-8 bytes (not NUL-terminated).
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_read_yaml_from_semantic_view_exec_rust(
    conn: libduckdb_sys::duckdb_connection,
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
        if name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "view name pointer is null");
            return 1_u8;
        }
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let raw_name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => {
                write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
                return 1_u8;
            }
        };
        let bare_name = resolve_bare_name(raw_name);

        let reader = CatalogReader::new(conn, probe_catalog_table_present(conn));
        let json = match reader.lookup(bare_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("semantic view '{bare_name}' does not exist"),
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
        let yaml = match render_yaml_export(&def) {
            Ok(s) => s,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };
        publish_owned_buffer(yaml.into_bytes(), out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            use crate::ddl::read_ffi::write_err;
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_read_yaml_from_semantic_view_exec_rust",
            );
            2
        }
    }
}

#[allow(dead_code)]
pub struct ReadYamlFromSemanticViewScalar;

impl VScalar for ReadYamlFromSemanticViewScalar {
    type State = CatalogReader;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            let len = input.len();
            let name_vec = input.flat_vector(0);
            let names = name_vec.as_slice_with_len::<duckdb_string_t>(len);
            let out_vec = output.flat_vector();

            for i in 0..len {
                let raw_name = DuckString::new(&mut { names[i] }).as_str().to_string();
                let bare_name = resolve_bare_name(&raw_name);

                let json = state
                    .lookup(bare_name)
                    .map_err(Box::<dyn std::error::Error>::from)?
                    .ok_or_else(|| format!("semantic view '{}' does not exist", bare_name))?;
                let def: SemanticViewDefinition = serde_json::from_str(&json)?;
                let yaml = render_yaml_export(&def)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                out_vec.insert(i, yaml.as_str());
            }
            Ok(())
        }))
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)], // name only (1 arg)
            LogicalTypeHandle::from(LogicalTypeId::Varchar),       // return: YAML string
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_bare_name_unqualified() {
        assert_eq!(resolve_bare_name("my_view"), "my_view");
    }

    #[test]
    fn resolve_bare_name_schema_qualified() {
        assert_eq!(resolve_bare_name("main.my_view"), "my_view");
    }

    #[test]
    fn resolve_bare_name_fully_qualified() {
        assert_eq!(resolve_bare_name("memory.main.my_view"), "my_view");
    }

    #[test]
    fn resolve_bare_name_empty() {
        assert_eq!(resolve_bare_name(""), "");
    }
}
