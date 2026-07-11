//! `GET_DDL` scalar function: wraps [`crate::render_ddl::render_create_ddl`] as a
//! C++ Catalog API scalar so that `SELECT GET_DDL('SEMANTIC_VIEW', 'name')`
//! works inside `DuckDB`.
//!
//! The render logic itself lives in [`crate::render_ddl`] (always compiled,
//! unit-tested under `cargo test`). This module adds the extension-only Rust
//! FFI dispatcher reached from `sv_register_get_ddl` in `cpp/src/shim.cpp`.
//!
//! # Phase 65 Plan 05 Task 4 (Wave 3) — Batch 3 final cleanup
//!
//! The legacy `GetDdlScalar` `VScalar` impl block was retired in the same
//! commit that deleted the H2 `query_conn` allocation; all live invocations
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
    crate::ddl::read_ffi::run_dispatcher(
        conn,
        out_ptr,
        out_len,
        error_buf,
        error_buf_len,
        "sv_get_ddl_exec_rust",
        |borrowed| unsafe { get_ddl(borrowed, type_ptr, type_len, name_ptr, name_len) },
    )
}

/// Body for [`sv_get_ddl_exec_rust`]: validate the object type, resolve the
/// view, and render its `CREATE OR REPLACE SEMANTIC VIEW` DDL.
///
/// # Safety
///
/// `type_ptr` / `name_ptr` must each be null or point to the matching number
/// of readable bytes.
#[cfg(feature = "extension")]
unsafe fn get_ddl(
    borrowed: &crate::ddl::read_ffi::BorrowedConnection,
    type_ptr: *const u8,
    type_len: usize,
    name_ptr: *const u8,
    name_len: usize,
) -> Result<Vec<u8>, String> {
    use crate::ddl::read_ffi::{probe_catalog_table_present, read_str_arg};

    let obj_type = read_str_arg(type_ptr, type_len, "object_type")?;
    let raw_name = read_str_arg(name_ptr, name_len, "view name")?;

    if !obj_type.eq_ignore_ascii_case("SEMANTIC_VIEW") {
        return Err(format!(
            "GET_DDL: unsupported object type '{obj_type}'. Only 'SEMANTIC_VIEW' is supported."
        ));
    }

    // C-2 (code-review 2026-07-11): normalize the requested name like every
    // other single-view read path (FF-4/PA-8 sweep). Lenient contract mirrors
    // read_yaml's `resolve_bare_name`: a name that does not parse as an
    // identifier is looked up verbatim and fails with the canonical message.
    let name = crate::ident::normalize_view_name(&raw_name).unwrap_or(raw_name);

    // FF-9: a probe-query failure is distinct from "no views" (propagated).
    let present = probe_catalog_table_present(borrowed)?;
    let reader = CatalogReader::new(borrowed, present);
    let json = reader
        .lookup(&name)?
        .ok_or_else(|| crate::catalog::view_not_found_msg(&name))?;
    // C-2: `from_json` for the canonical "invalid definition for semantic
    // view '<name>'" context on corrupt rows.
    let def = SemanticViewDefinition::from_json(&name, &json)?;
    render_create_ddl(&name, &def)
        .map(String::into_bytes)
        .map_err(|e| format!("GET_DDL error: {e}"))
}
