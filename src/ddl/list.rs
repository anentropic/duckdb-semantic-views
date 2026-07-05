use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;

// ---------------------------------------------------------------------------
// list_semantic_views — Phase 65 Plan 05 Task 1 (Wave 0 bridge spike)
// ---------------------------------------------------------------------------
//
// The legacy `ListSemanticViewsVTab` below (`impl VTab`) was registered via
// duckdb-rs's `register_table_function_with_extra_info`, which marshals
// `ClientContext &` away (Plan 01 Spike A6). That registration path is
// RETIRED for `list_semantic_views`; the function is now registered via the
// C++ Catalog API path (`cpp/src/shim.cpp::sv_register_list_semantic_views`
// → `sv_register_table_function`). The bind callback opens a per-call
// `Connection(*context.db)` and bridges to Rust via
// `sv_list_semantic_views_bind_rust` (defined below).
//
// The bridge mechanism is a `reinterpret_cast` of the stack `Connection *`
// to `duckdb_connection` — confirmed by reading `duckdb.cpp:266432-266447`
// where `duckdb_connect` is literally
// `reinterpret_cast<duckdb_connection>(new Connection(...))`. The Rust
// dispatcher receives a BORROWED handle: it MUST NOT call
// `duckdb_disconnect` (would `delete` the stack-allocated Connection — UB).
// The C++ bind scope's `~Connection()` handles teardown.
//
// The legacy `ListSemanticViewsVTab` + `ListTerseSemanticViewsVTab` impl
// blocks were deleted in the Batch 3 cleanup commit (along with all 17
// other dead VTab/VScalar carcasses) together with the H2 query_conn
// allocation in `src/lib.rs::init_extension`.

/// FFI entry point: read the catalog and serialize all semantic views as a
/// length-prefixed binary buffer for the C++ bind callback to parse.
///
/// Called from `cpp/src/shim.cpp::sv_list_semantic_views_bind`. The C++
/// side passes a per-call `duckdb_connection` borrowed from a stack
/// `Connection probe(*context.db)`. This function wraps the handle in a
/// `CatalogReader`, performs the catalog read, and serializes the rows
/// into a flat binary buffer with the wire format:
///
///   u32 row_count (little-endian)
///   for each row:
///     for each of 6 columns:
///       u32 byte_len (little-endian)
///       byte_len bytes (UTF-8, NOT NUL-terminated)
///
/// The 6 columns match the v0.9.0 Rust VTab shape exactly:
/// (created_on, name, kind, database_name, schema_name, comment).
///
/// # Bridge lifecycle (critical)
///
/// The `conn` parameter is a BORROWED handle — the underlying C++
/// `Connection` is owned by a stack local in the C++ bind callback.
/// This function MUST NOT:
///   * call `duckdb_disconnect(conn)` (would `delete` a stack object — UB),
///   * stash the handle in long-lived storage (would dangle after bind),
///   * call functions that take ownership of the handle (none in the
///     CatalogReader path — `CatalogReader::new` only stores the raw
///     pointer, and the prepared-statement / query helpers in
///     `src/catalog.rs` operate on the handle without consuming it).
///
/// # Return codes
///
/// * `0` — success; `(out_ptr, out_len)` populated. Caller MUST release
///         via `sv_free_buffer(ptr, len)`.
/// * `1` — catalog read error (e.g. the `semantic_layer._definitions`
///         table is missing); `error_buf` populated.
/// * `2` — internal error (panic across FFI, serialization failure);
///         `error_buf` populated.
///
/// # Catalog-table-present probing
///
/// The original `init_extension` probes whether `semantic_layer._definitions`
/// exists once at LOAD time and passes the flag to `CatalogReader::new`.
/// Under the per-call connection model that probe can't be done at LOAD
/// time anymore (the per-call conn doesn't exist yet). We probe inline
/// here on every bind: cheap (single `information_schema.tables` lookup)
/// and correct against schema drift mid-session. The probe runs on the
/// same per-call connection so it shares the caller's catalog/search-path
/// view (matches the Phase 63 read-only short-circuit behavior).
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_list_semantic_views_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use crate::ddl::read_ffi::{
        probe_catalog_table_present, publish_owned_buffer, serialize_varchar_rows, write_err,
        BorrowedConnection,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Wrap the raw FFI handle in a BorrowedConnection at the boundary
        // (D-10 / WR-05). Everything below this point goes through &borrowed
        // or borrowed.as_raw(); the raw `conn` parameter is shadowed and
        // never used again.
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }

        // Probe whether semantic_layer._definitions exists on the caller's
        // connection. Cheap (single query); matches Phase 63's RO-load
        // short-circuit semantics so an attached read-only DB without a
        // bootstrapped catalog returns 0 rows instead of an error.
        let table_present = probe_catalog_table_present(&borrowed);

        // CatalogReader::new only stores the raw pointer extracted via
        // borrowed.as_raw() — no transfer of ownership.
        let reader = CatalogReader::new(&borrowed, table_present);
        let entries = match reader.list_all() {
            Ok(e) => e,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };

        // Reconstruct the 6-column rows exactly like the Rust VTab did
        // (ListBindData::rows) — sort by name so output ordering is
        // byte-identical to the v0.9.0 behavior.
        let mut rows: Vec<Vec<String>> = Vec::with_capacity(entries.len());
        for (name, json) in &entries {
            let def = SemanticViewDefinition::from_json(name, json).ok();
            let (created_on, database_name, schema_name, comment) = match &def {
                Some(d) => (
                    d.created_on.clone().unwrap_or_default(),
                    d.database_name.clone().unwrap_or_default(),
                    d.schema_name.clone().unwrap_or_default(),
                    d.comment.clone().unwrap_or_default(),
                ),
                None => (String::new(), String::new(), String::new(), String::new()),
            };
            rows.push(vec![
                created_on,
                name.clone(),
                "SEMANTIC_VIEW".to_string(),
                database_name,
                schema_name,
                comment,
            ]);
        }
        rows.sort_by(|a, b| a[1].cmp(&b[1]));

        // FF-6: the shared serializer returns an error (rather than clamping a
        // length to u32::MAX and desyncing the header from the payload) if a
        // cell or the row count overflows the wire format's u32 fields. The
        // previous inline copy used bare `as u32` casts. `publish_owned_buffer`
        // hands the heap-owned buffer to C++ under the both-or-drop contract;
        // the caller releases it via sv_free_buffer with the exact (ptr, len).
        let buf = match serialize_varchar_rows(&rows) {
            Ok(b) => b,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        publish_owned_buffer(buf, out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_list_semantic_views_bind_rust",
            );
            2
        }
    }
}

// Phase 65.1 Plan 03a (IN-06 / D-26): the module-local duplicates of
// `probe_catalog_table_present` and `write_err` were DELETED. The canonical
// definitions in `src/ddl/read_ffi.rs` are imported at each call site.
// Single source of truth + the BorrowedConnection migration on the
// canonical version flows to all 17 read-side dispatchers.

// ---------------------------------------------------------------------------
// Legacy Rust VTab `ListSemanticViewsVTab` RETIRED — Phase 65 Plan 05 Batch 3
// (cleanup commit). The C++ Catalog API path above is the sole registration
// target; no duckdb-rs `register_table_function_with_extra_info` call is
// reachable from `src/lib.rs::init_extension`. The legacy struct + impl block
// + `ListBindData` + `ListInitData` were deleted together with the H2
// query_conn allocation that fed `CatalogReader` to the bind callback's
// `get_extra_info::<CatalogReader>()`.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// list_terse_semantic_views — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
//
// Same bridge mechanism as `list_semantic_views` (see Wave 0 spike docs at
// the top of this file). Bind callback opens a per-call
// `Connection(*context.db)` and calls `sv_list_terse_semantic_views_bind_rust`.
// Borrow contract: dispatcher MUST NOT call `duckdb_disconnect` on the
// borrowed handle.

/// FFI dispatcher for the migrated `list_terse_semantic_views()` table
/// function — 5-column subset of `list_semantic_views()` (no `comment`).
///
/// Wire format (length-prefixed binary, LE):
///   u32 row_count
///   for each row:
///     for each of 5 cols: u32 byte_len | bytes (UTF-8)
///
/// Columns: (created_on, name, kind, database_name, schema_name).
///
/// # Safety
///
/// `conn` is a borrowed handle (see file-level docs). Caller must release
/// the returned buffer via `sv_free_buffer(*out_ptr, *out_len)`.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_list_terse_semantic_views_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use crate::ddl::read_ffi::{
        probe_catalog_table_present, publish_owned_buffer, serialize_varchar_rows, write_err,
        BorrowedConnection,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }

        let table_present = probe_catalog_table_present(&borrowed);
        let reader = CatalogReader::new(&borrowed, table_present);
        let entries = match reader.list_all() {
            Ok(e) => e,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };

        let mut rows: Vec<Vec<String>> = Vec::with_capacity(entries.len());
        for (name, json) in &entries {
            let def = SemanticViewDefinition::from_json(name, json).ok();
            let (created_on, database_name, schema_name) = match &def {
                Some(d) => (
                    d.created_on.clone().unwrap_or_default(),
                    d.database_name.clone().unwrap_or_default(),
                    d.schema_name.clone().unwrap_or_default(),
                ),
                None => (String::new(), String::new(), String::new()),
            };
            rows.push(vec![
                created_on,
                name.clone(),
                "SEMANTIC_VIEW".to_string(),
                database_name,
                schema_name,
            ]);
        }
        rows.sort_by(|a, b| a[1].cmp(&b[1]));

        let buf = match serialize_varchar_rows(&rows) {
            Ok(b) => b,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        publish_owned_buffer(buf, out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            use crate::ddl::read_ffi::write_err;
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_list_terse_semantic_views_bind_rust",
            );
            2
        }
    }
}

// ---------------------------------------------------------------------------
// SHOW TERSE SEMANTIC VIEWS — Legacy VTab RETIRED Phase 65 Plan 05 Batch 3.
// ListTerseSemanticViewsVTab + ListTerseRow + ListTerseBindData +
// ListTerseInitData were deleted together with the H2 query_conn allocation.
// The C++ Catalog API path is the sole registration target.
// ---------------------------------------------------------------------------
