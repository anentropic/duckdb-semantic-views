//! `READ_YAML_FROM_SEMANTIC_VIEW` scalar function: wraps
//! [`crate::render_yaml::render_yaml_export`] as a C++ Catalog API scalar so
//! that `SELECT READ_YAML_FROM_SEMANTIC_VIEW('name')` works inside `DuckDB`.
//!
//! The render logic itself lives in [`crate::render_yaml`] (always compiled,
//! unit-tested under `cargo test`). This module adds the extension-only Rust
//! FFI dispatcher reached from `sv_register_read_yaml_from_semantic_view` in
//! `cpp/src/shim.cpp`.
//!
//! # Phase 65 Plan 05 Task 4 (Wave 3) — Batch 3 final cleanup
//!
//! The legacy `ReadYamlFromSemanticViewScalar` `VScalar` impl block was
//! retired in the same commit that deleted the H2 query_conn allocation; all
//! live invocations of `SELECT READ_YAML_FROM_SEMANTIC_VIEW(...)` now route
//! through [`sv_read_yaml_from_semantic_view_exec_rust`] below.

use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;
use crate::render_yaml::render_yaml_export;

/// Extract the bare view name from a potentially qualified name.
/// Supports: `"view_name"`, `"schema.view_name"`, `"database.schema.view_name"`.
///
/// Delegates to [`crate::ident::normalize_view_name`] (PA-10, code-review
/// 2026-07-02): the previous naive `rsplit('.')` split inside quoted parts,
/// so `"a.b"` resolved to `b"` instead of `a.b`. Falls back to the input
/// verbatim when it does not parse as an identifier (legacy behaviour for
/// malformed names — the lookup then fails with "does not exist").
fn resolve_bare_name(input: &str) -> String {
    crate::ident::normalize_view_name(input).unwrap_or_else(|_| input.to_string())
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

        // FF-9: surface a probe-query failure as an error distinct from "no
        // views" instead of silently folding it into absence.
        let present = match probe_catalog_table_present(&borrowed) {
            Ok(p) => p,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let reader = CatalogReader::new(&borrowed, present);
        let json = match reader.lookup(&bare_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &crate::catalog::view_not_found_msg(&bare_name),
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

    #[test]
    fn resolve_bare_name_quoted_dot_not_split() {
        // PA-10: the old rsplit('.') split inside the quoted part.
        assert_eq!(resolve_bare_name("\"a.b\""), "a.b");
        assert_eq!(resolve_bare_name("main.\"my view\""), "my view");
    }

    #[test]
    fn resolve_bare_name_folds_unquoted() {
        // PA-8 consistency: unquoted references fold to lowercase like
        // every other lookup path.
        assert_eq!(resolve_bare_name("MyView"), "myview");
        assert_eq!(resolve_bare_name("\"MyView\""), "MyView");
    }
}
