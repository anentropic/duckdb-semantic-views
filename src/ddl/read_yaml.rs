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
//! retired in the same commit that deleted the H2 `query_conn` allocation; all
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
    crate::ddl::read_ffi::run_dispatcher(
        conn,
        out_ptr,
        out_len,
        error_buf,
        error_buf_len,
        "sv_read_yaml_from_semantic_view_exec_rust",
        |borrowed| unsafe { read_yaml_export(borrowed, name_ptr, name_len) },
    )
}

/// Body for [`sv_read_yaml_from_semantic_view_exec_rust`]: resolve the view
/// and render its YAML export.
///
/// # Safety
///
/// `name_ptr` must be null or point to `name_len` readable bytes.
#[cfg(feature = "extension")]
unsafe fn read_yaml_export(
    borrowed: &crate::ddl::read_ffi::BorrowedConnection,
    name_ptr: *const u8,
    name_len: usize,
) -> Result<Vec<u8>, String> {
    use crate::ddl::read_ffi::{probe_catalog_table_present, read_str_arg};

    let raw_name = read_str_arg(name_ptr, name_len, "view name")?;
    let bare_name = resolve_bare_name(&raw_name);

    // FF-9: a probe-query failure is distinct from "no views" (propagated).
    let present = probe_catalog_table_present(borrowed)?;
    let reader = CatalogReader::new(borrowed, present);
    let json = reader
        .lookup(&bare_name)?
        .ok_or_else(|| crate::catalog::view_not_found_msg(&bare_name))?;
    // C-2 (code-review 2026-07-11): `from_json` for the canonical
    // "invalid definition for semantic view '<name>'" context on corrupt rows.
    let def = SemanticViewDefinition::from_json(&bare_name, &json)?;
    render_yaml_export(&def).map(String::into_bytes)
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
    fn resolve_bare_name_folds_to_lowercase() {
        // View-name lookup folds to lowercase the same way `normalize_view_name`
        // and every other lookup path does — for quoted names too. Under
        // DuckDB's identifier rule (and this project's documented view-name
        // normalization) quoting only lets a name carry special characters; it
        // does NOT preserve case. Stored view names are lowercase, so a request
        // written `"MyView"` must resolve to `myview` to find the view.
        assert_eq!(resolve_bare_name("MyView"), "myview");
        assert_eq!(resolve_bare_name("\"MyView\""), "myview");
    }
}
