pub mod body_parser;
pub mod catalog;
pub mod errors;
pub mod expand;
pub mod ffi_util;
pub mod graph;
pub mod ident;
pub mod model;
pub mod parse;
#[cfg(feature = "extension")]
pub mod query;
pub mod render_ddl;
pub mod render_yaml;
pub mod util;

/// Minimum `DuckDB` version this extension declares compatibility with, passed to
/// `duckdb_rs_extension_api_init` at load time.
///
/// This must stay in lockstep with the `DuckDB` version the crate is built and
/// tested against. The three sources of truth — this constant, the
/// `.duckdb-version` file, and the pinned `libduckdb-sys` version in
/// `Cargo.toml` — are asserted consistent by `tests::duckdb_version_pins_agree`.
pub const MINIMUM_DUCKDB_VERSION: &str = "v1.5.4";

/// Test helpers for integration tests.
///
/// Exposes low-level FFI utilities needed by `tests/output_proptest.rs` which
/// runs under the default `bundled` feature (NOT the `extension` feature).
/// The `extension` feature enables `duckdb/loadable-extension` which replaces
/// all C API calls with stubs — incompatible with `Connection::open_in_memory()`.
///
/// These helpers mirror the functions in `query::table_function` but are compiled
/// under the `bundled` (default) feature so that integration tests can access them.
#[cfg(not(feature = "extension"))]
#[allow(clippy::pedantic, clippy::doc_markdown)]
pub mod test_helpers {
    use libduckdb_sys as ffi;
    use std::ffi::{CStr, CString};

    /// Execute a SQL string via the DuckDB C API and return the result.
    ///
    /// # Safety
    ///
    /// `conn` must be a valid, non-null `duckdb_connection` handle.
    pub unsafe fn execute_sql_raw(
        conn: ffi::duckdb_connection,
        sql: &str,
    ) -> Result<ffi::duckdb_result, String> {
        let sql_cstr = CString::new(sql).map_err(|e| e.to_string())?;
        let mut result: ffi::duckdb_result = std::mem::zeroed();
        let rc = ffi::duckdb_query(conn, sql_cstr.as_ptr(), &mut result);
        if rc != ffi::DuckDBSuccess {
            let err_ptr = ffi::duckdb_result_error(&mut result);
            let err_msg = if err_ptr.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
            };
            ffi::duckdb_destroy_result(&mut result);
            return Err(err_msg);
        }
        Ok(result)
    }

    /// Typed value for test assertions.
    #[derive(Debug, PartialEq)]
    pub enum TestValue {
        Null,
        Bool(bool),
        I8(i8),
        I16(i16),
        I32(i32),
        I64(i64),
        U8(u8),
        U16(u16),
        U32(u32),
        U64(u64),
        F32(f32),
        F64(f64),
        I128(i128),
        Str(String),
        List(Vec<TestValue>),
    }

    /// Read a typed value from a DuckDB result chunk column/row using binary reads.
    ///
    /// This mirrors `read_typed_from_vector` in `query::table_function` but is
    /// compiled with the default (bundled) feature for integration test use.
    ///
    /// # Safety
    ///
    /// `chunk` must be a valid `duckdb_data_chunk`. `col_idx` and `row_idx` must be in bounds.
    /// `logical_type` must be valid for the column (may be null for non-complex types).
    #[allow(clippy::cast_possible_truncation)]
    pub unsafe fn read_typed_value(
        chunk: ffi::duckdb_data_chunk,
        col_idx: usize,
        row_idx: usize,
        type_id: u32,
        logical_type: ffi::duckdb_logical_type,
    ) -> TestValue {
        // SEC-04: Guard against out-of-range indices in test helpers.
        let row_count = ffi::duckdb_data_chunk_get_size(chunk) as usize;
        debug_assert!(
            row_idx < row_count,
            "read_typed_value: row_idx {row_idx} out of bounds (chunk has {row_count} rows)"
        );
        let col_count = ffi::duckdb_data_chunk_get_column_count(chunk) as usize;
        debug_assert!(
            col_idx < col_count,
            "read_typed_value: col_idx {col_idx} out of bounds (chunk has {col_count} columns)"
        );

        use ffi::{
            DUCKDB_TYPE_DUCKDB_TYPE_BIGINT as BIGINT, DUCKDB_TYPE_DUCKDB_TYPE_BOOLEAN as BOOLEAN,
            DUCKDB_TYPE_DUCKDB_TYPE_DATE as DATE, DUCKDB_TYPE_DUCKDB_TYPE_DECIMAL as DECIMAL,
            DUCKDB_TYPE_DUCKDB_TYPE_DOUBLE as DOUBLE, DUCKDB_TYPE_DUCKDB_TYPE_FLOAT as FLOAT,
            DUCKDB_TYPE_DUCKDB_TYPE_HUGEINT as HUGEINT, DUCKDB_TYPE_DUCKDB_TYPE_INTEGER as INTEGER,
            DUCKDB_TYPE_DUCKDB_TYPE_LIST as LIST, DUCKDB_TYPE_DUCKDB_TYPE_SMALLINT as SMALLINT,
            DUCKDB_TYPE_DUCKDB_TYPE_TIME as TIME, DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP as TIMESTAMP,
            DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_MS as TIMESTAMP_MS,
            DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_NS as TIMESTAMP_NS,
            DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_S as TIMESTAMP_S,
            DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_TZ as TIMESTAMP_TZ,
            DUCKDB_TYPE_DUCKDB_TYPE_TINYINT as TINYINT, DUCKDB_TYPE_DUCKDB_TYPE_UBIGINT as UBIGINT,
            DUCKDB_TYPE_DUCKDB_TYPE_UHUGEINT as UHUGEINT,
            DUCKDB_TYPE_DUCKDB_TYPE_UINTEGER as UINTEGER,
            DUCKDB_TYPE_DUCKDB_TYPE_USMALLINT as USMALLINT,
            DUCKDB_TYPE_DUCKDB_TYPE_UTINYINT as UTINYINT,
            DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR as VARCHAR,
        };

        let vector = ffi::duckdb_data_chunk_get_vector(chunk, col_idx as ffi::idx_t);

        // NULL check via validity mask.
        let validity = ffi::duckdb_vector_get_validity(vector);
        if !validity.is_null() {
            let entry = *validity.add(row_idx / 64);
            if entry & (1u64 << (row_idx % 64)) == 0 {
                return TestValue::Null;
            }
        }

        let data_ptr = ffi::duckdb_vector_get_data(vector);

        match type_id {
            BOOLEAN => TestValue::Bool(*data_ptr.cast::<u8>().add(row_idx) != 0),
            TINYINT => TestValue::I8(*data_ptr.cast::<i8>().add(row_idx)),
            SMALLINT => TestValue::I16(*data_ptr.cast::<i16>().add(row_idx)),
            INTEGER => TestValue::I32(*data_ptr.cast::<i32>().add(row_idx)),
            BIGINT => TestValue::I64(*data_ptr.cast::<i64>().add(row_idx)),
            UTINYINT => TestValue::U8(*data_ptr.cast::<u8>().add(row_idx)),
            USMALLINT => TestValue::U16(*data_ptr.cast::<u16>().add(row_idx)),
            UINTEGER => TestValue::U32(*data_ptr.cast::<u32>().add(row_idx)),
            UBIGINT => TestValue::U64(*data_ptr.cast::<u64>().add(row_idx)),
            FLOAT => TestValue::F32(*data_ptr.cast::<f32>().add(row_idx)),
            DOUBLE => TestValue::F64(*data_ptr.cast::<f64>().add(row_idx)),
            DATE => TestValue::I32(*data_ptr.cast::<i32>().add(row_idx)),
            TIMESTAMP | TIMESTAMP_S | TIMESTAMP_MS | TIMESTAMP_NS | TIMESTAMP_TZ | TIME => {
                TestValue::I64(*data_ptr.cast::<i64>().add(row_idx))
            }
            HUGEINT => TestValue::I64(*data_ptr.cast::<i128>().add(row_idx) as i64),
            UHUGEINT => TestValue::U64(*data_ptr.cast::<u128>().add(row_idx) as u64),
            DECIMAL => {
                if logical_type.is_null() {
                    return TestValue::Null;
                }
                let internal = ffi::duckdb_decimal_internal_type(logical_type) as u32;
                match internal {
                    SMALLINT => TestValue::I128(i128::from(*data_ptr.cast::<i16>().add(row_idx))),
                    INTEGER => TestValue::I128(i128::from(*data_ptr.cast::<i32>().add(row_idx))),
                    BIGINT => TestValue::I128(i128::from(*data_ptr.cast::<i64>().add(row_idx))),
                    _ => TestValue::I128(*data_ptr.cast::<i128>().add(row_idx)),
                }
            }
            LIST => {
                if logical_type.is_null() {
                    return TestValue::Null;
                }
                let entry = *data_ptr.cast::<ffi::duckdb_list_entry>().add(row_idx);
                let offset = entry.offset as usize;
                let length = entry.length as usize;
                let child_vec = ffi::duckdb_list_vector_get_child(vector);
                let child_lt = ffi::duckdb_list_type_child_type(logical_type);
                let child_type_id = ffi::duckdb_get_type_id(child_lt) as u32;
                ffi::duckdb_destroy_logical_type(&mut { child_lt });

                let mut elements = Vec::with_capacity(length);
                for i in 0..length {
                    let child_row = offset + i;
                    let child_validity = ffi::duckdb_vector_get_validity(child_vec);
                    if !child_validity.is_null() {
                        let centry = *child_validity.add(child_row / 64);
                        if centry & (1u64 << (child_row % 64)) == 0 {
                            elements.push(TestValue::Null);
                            continue;
                        }
                    }
                    let child_data = ffi::duckdb_vector_get_data(child_vec);
                    let elem = match child_type_id {
                        BOOLEAN => TestValue::Bool(*child_data.cast::<u8>().add(child_row) != 0),
                        INTEGER => TestValue::I32(*child_data.cast::<i32>().add(child_row)),
                        BIGINT => TestValue::I64(*child_data.cast::<i64>().add(child_row)),
                        DOUBLE => TestValue::F64(*child_data.cast::<f64>().add(child_row)),
                        _ => TestValue::Null,
                    };
                    elements.push(elem);
                }
                TestValue::List(elements)
            }
            VARCHAR => {
                // Read VARCHAR using the duckdb_string_t layout.
                let string_t_ptr = data_ptr.cast::<ffi::duckdb_string_t>().add(row_idx);
                let string_t = &*string_t_ptr;
                let len = string_t.value.inlined.length as usize;
                if len == 0 {
                    return TestValue::Str(String::new());
                }
                let bytes = if len <= 12 {
                    let p = string_t.value.inlined.inlined.as_ptr().cast::<u8>();
                    std::slice::from_raw_parts(p, len)
                } else {
                    let p = string_t.value.pointer.ptr.cast::<u8>();
                    if p.is_null() {
                        return TestValue::Str(String::new());
                    }
                    std::slice::from_raw_parts(p, len)
                };
                TestValue::Str(String::from_utf8_lossy(bytes).into_owned())
            }
            _ => TestValue::Null,
        }
    }

    /// A raw in-memory DuckDB database + connection pair.
    ///
    /// Automatically disconnects and closes on drop.
    pub struct RawDb {
        pub db: ffi::duckdb_database,
        pub conn: ffi::duckdb_connection,
    }

    impl RawDb {
        /// Open a new in-memory DuckDB database and connection via the C API.
        pub fn open_in_memory() -> Self {
            unsafe {
                let path = c":memory:";
                let mut db: ffi::duckdb_database = std::ptr::null_mut();
                let rc = ffi::duckdb_open(path.as_ptr(), &mut db);
                assert!(
                    rc == ffi::DuckDBSuccess,
                    "Failed to open in-memory DuckDB database"
                );
                let mut conn: ffi::duckdb_connection = std::ptr::null_mut();
                let rc = ffi::duckdb_connect(db, &mut conn);
                assert!(
                    rc == ffi::DuckDBSuccess,
                    "Failed to connect to in-memory DuckDB"
                );
                Self { db, conn }
            }
        }

        /// Execute a SQL string, panicking on error.
        ///
        /// # Safety
        ///
        /// `self.conn` must be a valid, open `duckdb_connection` handle.
        pub unsafe fn exec(&self, sql: &str) {
            execute_sql_raw(self.conn, sql)
                .unwrap_or_else(|e| panic!("SQL failed: {sql}\nError: {e}"));
        }
    }

    impl Drop for RawDb {
        fn drop(&mut self) {
            unsafe {
                if !self.conn.is_null() {
                    ffi::duckdb_disconnect(&mut self.conn);
                }
                if !self.db.is_null() {
                    ffi::duckdb_close(&mut self.db);
                }
            }
        }
    }
}

/// DDL function implementations — only compiled when building the `DuckDB` extension.
/// The `ddl` module uses `duckdb::vscalar` and `duckdb::vtab`, which are only
/// available when the `extension` feature (and thus `vscalar` + `loadable-extension`)
/// is active.  Under `cargo test` (default `bundled` feature), this module is excluded.
#[cfg(feature = "extension")]
pub mod ddl;

/// Extension entry point — called by DuckDB when the extension is loaded.
///
/// Uses C_STRUCT ABI (semantic_views_init_c_api) for the DuckDB handshake.
/// After Rust-side initialization (catalog, DDL functions, query functions),
/// calls a C++ helper (sv_register_parser_hooks) to register parser extension
/// hooks that require C++ types (ParserExtension, DBConfig).
///
/// This is "Option A" from Phase 15: keep C_STRUCT entry, call C++ helper.
/// Option B (CPP entry via DUCKDB_CPP_EXTENSION_ENTRY) was tried first but
/// failed due to unresolved C++ symbols from -fvisibility=hidden in the host.
#[cfg(feature = "extension")]
mod extension {
    use std::sync::Arc;

    use duckdb::{Connection, Result};
    use libduckdb_sys as ffi;
    use std::error::Error;
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use crate::catalog::init_catalog;
    // Phase 65 Plan 05 (complete after Batch 3 cleanup): all 17 read-side
    // registrations now go through the C++ Catalog API path:
    //  - Wave 0: list_semantic_views (bridge spike)
    //  - Wave 1: list_terse + 4 "_all" variants
    //  - Wave 2: describe + show_columns + 4 single-view SHOW variants +
    //            show_dimensions_for_metric
    //  - Wave 3: get_ddl + read_yaml_from_semantic_view (scalars)
    //  - Wave 5: explain_semantic_view
    //  - Wave 6: semantic_view
    // The legacy duckdb-rs `register_table_function_with_extra_info` /
    // `register_scalar_function_with_state` registrations were deleted
    // together with the H2 query_conn allocation in the Plan 05 Batch 3
    // cleanup commit. Phase 65 Plan 06 retires H1 catalog_conn as well —
    // `init_extension` no longer allocates any long-lived extension-owned
    // `duckdb_connection`. `tests/no_long_lived_conn.rs` is the
    // structural guard that fails CI if any future change re-introduces
    // one inside `init_extension`.

    // C++ shim registration wrappers (defined in cpp/src/shim.cpp). Every one
    // shares the same ABI — `(db_handle, error_buf, error_buf_len) -> bool` —
    // so the `extern "C"` import block and the ordered `REGISTRATIONS` table
    // are generated together from a single list by `sv_registrations!`,
    // keeping them in sync (adding a table function is a one-line edit; ST-1).
    // On failure each wrapper snprintf's a diagnostic into `error_buf`,
    // surfaced through the ABI-stable channel by `decode_register_err_buf`.
    type RegisterFn = unsafe extern "C" fn(ffi::duckdb_database, *mut c_char, usize) -> bool;

    macro_rules! sv_registrations {
        ($( ($label:literal, $sym:ident) ),+ $(,)?) => {
            extern "C" {
                $(
                    fn $sym(
                        db_handle: ffi::duckdb_database,
                        error_buf: *mut c_char,
                        error_buf_len: usize,
                    ) -> bool;
                )+
            }

            /// Ordered `(human label, C++ registration wrapper)` table, iterated
            /// by `init_extension`. Order is only significant in that the parser
            /// hook (entry 0) must be registered before the table functions; the
            /// read-side registrations are mutually independent.
            const REGISTRATIONS: &[(&str, RegisterFn)] = &[ $( ($label, $sym) ),+ ];
        };
    }

    sv_registrations![
        ("parser hooks", sv_register_parser_hooks),
        ("list_semantic_views", sv_register_list_semantic_views),
        (
            "list_terse_semantic_views",
            sv_register_list_terse_semantic_views
        ),
        (
            "show_columns_in_semantic_view",
            sv_register_show_columns_in_semantic_view
        ),
        ("describe_semantic_view", sv_register_describe_semantic_view),
        (
            "show_semantic_dimensions",
            sv_register_show_semantic_dimensions
        ),
        (
            "show_semantic_dimensions_all",
            sv_register_show_semantic_dimensions_all
        ),
        (
            "show_semantic_dimensions_for_metric",
            sv_register_show_semantic_dimensions_for_metric
        ),
        ("show_semantic_metrics", sv_register_show_semantic_metrics),
        (
            "show_semantic_metrics_all",
            sv_register_show_semantic_metrics_all
        ),
        ("show_semantic_facts", sv_register_show_semantic_facts),
        (
            "show_semantic_facts_all",
            sv_register_show_semantic_facts_all
        ),
        (
            "show_semantic_materializations",
            sv_register_show_semantic_materializations
        ),
        (
            "show_semantic_materializations_all",
            sv_register_show_semantic_materializations_all
        ),
        ("get_ddl", sv_register_get_ddl),
        (
            "read_yaml_from_semantic_view",
            sv_register_read_yaml_from_semantic_view
        ),
        ("semantic_view", sv_register_semantic_view),
        ("explain_semantic_view", sv_register_explain_semantic_view),
    ];

    /// Decode a `[0u8; 1024]` registration error buffer into an owned `String`,
    /// trimming at the first NUL. Returns `"(no error text)"` if the buffer
    /// is empty so the caller never emits a misleading bare-colon suffix.
    ///
    /// Phase 65.1 Plan 02b (WR-02 D-08/D-09): paired with the snprintf-into-
    /// `error_buf` convention on the C++ side of every `sv_register_*` helper.
    fn decode_register_err_buf(buf: &[u8]) -> String {
        // SAFETY: `buf` is a valid byte slice owned by the caller; the C side
        // wrote a NUL-terminated string via snprintf, truncating within
        // `buf.len()`. `CStr::from_ptr` reads up to (but not including) the
        // terminating NUL.
        let msg = unsafe { CStr::from_ptr(buf.as_ptr().cast::<c_char>()) }
            .to_string_lossy()
            .into_owned();
        if msg.is_empty() {
            "(no error text)".to_string()
        } else {
            msg
        }
    }

    /// Core initialization logic, called with both the high-level Connection and
    /// the raw database handle (extracted by the manual FFI entrypoint below).
    fn init_extension(
        con: &Connection,
        db_handle: ffi::duckdb_database,
    ) -> Result<(), Box<dyn Error>> {
        // Resolve the host database file path by querying PRAGMA database_list.
        let db_path: Arc<str> = {
            let mut stmt = con.prepare("PRAGMA database_list")?;
            let path = stmt
                .query_map([], |row| row.get::<_, String>(2))?
                .filter_map(Result::ok)
                .find(|file| !file.is_empty())
                .unwrap_or_default();
            if path.is_empty() {
                Arc::from(":memory:")
            } else {
                Arc::from(path.as_str())
            }
        };

        // Phase 63: Detect read-only access mode for THIS database.
        // AccessModeSetting::GetSetting (duckdb.cpp:301163-301167) calls
        // StringUtil::Lower(EnumUtil::ToString(AccessMode)), so the value is
        // lowercased: "read_only" / "read_write" / "automatic" / "undefined".
        // We match case-insensitively to be future-proof across DuckDB minor
        // bumps. Fail-open: if the setting is renamed/removed in a future
        // version, treat as writable; init_catalog will then surface DuckDB's
        // own read-only error from CREATE SCHEMA, which is strictly better
        // than a silent miss.
        let is_read_only: bool = con
            .query_row("SELECT current_setting('access_mode')", [], |row| {
                row.get::<_, String>(0)
            })
            .map(|s| s.eq_ignore_ascii_case("read_only"))
            .unwrap_or(false);

        // Initialize the persistent catalog (schema + table + companion-file migration).
        init_catalog(con, &db_path, is_read_only)?;

        // Phase 65 Plan 06: H1 catalog_conn allocation RETIRED. The
        // parser_override path is now pure-SQL on the caller's connection
        // — existence checks use a `SELECT CASE WHEN NOT EXISTS THEN
        // error() ELSE TRUE END; <DML>` two-statement guard that runs
        // snapshot-consistent with the DML. No long-lived extension-owned
        // `duckdb_connection` is allocated in `init_extension` after this
        // plan; `tests/no_long_lived_conn.rs` is the structural guard
        // that fails CI if anyone re-introduces one.

        // Register the parser_override hook and every read-side table function
        // / scalar via the C++ Catalog-API wrappers. `REGISTRATIONS` is ordered
        // so the parser hook (entry 0) runs before the table functions (the
        // one ordering constraint; the read-side registrations are independent).
        // Each wrapper writes a diagnostic into `error_buf` on failure, decoded
        // here so ADBC/JDBC/Python callers see the underlying DuckDB error.
        for &(label, register) in REGISTRATIONS {
            let mut error_buf = [0u8; 1024];
            if !unsafe {
                register(
                    db_handle,
                    error_buf.as_mut_ptr().cast::<c_char>(),
                    error_buf.len(),
                )
            } {
                return Err(format!(
                    "Failed to register {label}: {}",
                    decode_register_err_buf(&error_buf)
                )
                .into());
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Manual FFI entrypoint (C_STRUCT ABI)
    //
    // We write the entrypoint by hand to capture the raw duckdb_database handle
    // BEFORE it is wrapped in a Connection. This avoids unsafe pointer arithmetic
    // to extract private fields from Connection.
    //
    // The implementation mirrors the code generated by `duckdb_entrypoint_c_api`
    // in duckdb-loadable-macros 0.1.14, with the addition of passing `db_handle`
    // to `init_extension`.
    // -----------------------------------------------------------------------

    /// Internal entrypoint with error handling.
    ///
    /// # Safety
    ///
    /// Called by the extern "C" entrypoint below. `info` and `access` must be
    /// valid pointers provided by DuckDB.
    unsafe fn semantic_views_init_c_api_internal(
        info: ffi::duckdb_extension_info,
        access: *const ffi::duckdb_extension_access,
    ) -> std::result::Result<bool, Box<dyn Error>> {
        let have_api_struct =
            ffi::duckdb_rs_extension_api_init(info, access, crate::MINIMUM_DUCKDB_VERSION).unwrap();

        if !have_api_struct {
            return Ok(false);
        }

        // Get the raw database handle BEFORE wrapping in Connection.
        let db_handle: ffi::duckdb_database = *(*access).get_database.unwrap()(info);

        // Create a Connection from the database handle (same as the macro does).
        let connection = Connection::open_from_raw(db_handle.cast())?;

        // Call our init with both the Connection and the raw db handle.
        init_extension(&connection, db_handle)?;

        Ok(true)
    }

    /// FFI entrypoint called by DuckDB when the extension is loaded (C_STRUCT ABI).
    ///
    /// # Safety
    ///
    /// This is an extern "C" function called across the FFI boundary by DuckDB.
    /// `info` and `access` are guaranteed valid by DuckDB for the call duration.
    #[no_mangle]
    pub unsafe extern "C" fn semantic_views_init_c_api(
        info: ffi::duckdb_extension_info,
        access: *const ffi::duckdb_extension_access,
    ) -> bool {
        let init_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            semantic_views_init_c_api_internal(info, access)
        }));

        // FF-7 (code-review 2026-07-02): these arms run AFTER catch_unwind
        // has returned, so a panic here (e.g. `.unwrap()` on a null
        // `set_error`) would unwind out of this extern "C" fn — a guaranteed
        // process abort in the host. A null callback instead degrades to a
        // message-less failed load.
        match init_result {
            Ok(Ok(val)) => val,
            Ok(Err(x)) => {
                if let Some(set_error) = (*access).set_error {
                    let error_c_string = std::ffi::CString::new(x.to_string());
                    match error_c_string {
                        Ok(e) => {
                            set_error(info, e.as_ptr());
                        }
                        Err(_e) => {
                            let error_alloc_failure = c"An error occurred but the extension failed to allocate memory for an error string";
                            set_error(info, error_alloc_failure.as_ptr());
                        }
                    }
                }
                false
            }
            Err(_panic) => {
                if let Some(set_error) = (*access).set_error {
                    let panic_msg =
                        c"Extension init panicked unexpectedly. This is a bug in semantic_views.";
                    set_error(info, panic_msg.as_ptr());
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Machine-guards the three `DuckDB` version sources of truth against drift
    /// (AR-8): `MINIMUM_DUCKDB_VERSION`, the `.duckdb-version` file, and the
    /// pinned `libduckdb-sys` version in `Cargo.toml` must all describe the
    /// same `DuckDB` release.
    #[test]
    fn duckdb_version_pins_agree() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");

        // `.duckdb-version` is the canonical human-readable version, e.g. "v1.5.4".
        let version_file =
            std::fs::read_to_string(std::path::Path::new(manifest_dir).join(".duckdb-version"))
                .expect("read .duckdb-version");
        let canonical = version_file.trim();

        // The version declared to the loader must match it verbatim.
        assert_eq!(
            super::MINIMUM_DUCKDB_VERSION,
            canonical,
            "MINIMUM_DUCKDB_VERSION disagrees with .duckdb-version"
        );

        // The libduckdb-sys pin encodes the same release as
        // 1.<major*10000 + minor*100 + patch>.0 (e.g. v1.5.4 -> =1.10504.0).
        let mut parts = canonical
            .strip_prefix('v')
            .unwrap_or(canonical)
            .split('.')
            .map(|p| p.parse::<u32>().expect("numeric version component"));
        let (major, minor, patch) = (
            parts.next().expect("major"),
            parts.next().expect("minor"),
            parts.next().expect("patch"),
        );
        let expected_encoded = major * 10000 + minor * 100 + patch;

        let cargo_toml =
            std::fs::read_to_string(std::path::Path::new(manifest_dir).join("Cargo.toml"))
                .expect("read Cargo.toml");
        let pin_line = cargo_toml
            .lines()
            .find(|l| l.trim_start().starts_with("libduckdb-sys"))
            .expect("libduckdb-sys pin line in Cargo.toml");
        let encoded: u32 = pin_line
            .split('"')
            .find(|s| s.starts_with("=1."))
            .and_then(|pin| pin.trim_start_matches("=1.").split('.').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("could not parse libduckdb-sys pin from: {pin_line}"));
        assert_eq!(
            encoded, expected_encoded,
            "libduckdb-sys pin =1.{encoded}.0 disagrees with .duckdb-version {canonical} \
             (expected =1.{expected_encoded}.0)"
        );
    }

    // -----------------------------------------------------------------
    // Phase 63 (v0.9.0): pin DuckDB's `current_setting('access_mode')`
    // contract so that future DuckDB minor bumps that change the
    // rendering surface as a CI failure rather than a silent miss in
    // production. See src/lib.rs::init_extension Phase 63 detection
    // block and 63-RESEARCH.md §3 Q1.
    // -----------------------------------------------------------------

    #[cfg(not(feature = "extension"))]
    #[test]
    fn access_mode_lowercased_on_readonly_open() {
        use duckdb::{AccessMode, Config, Connection};

        // Pin DuckDB's contract: current_setting('access_mode') returns
        // the lowercased enum form ("read_only"), not "READ_ONLY".
        // If a future DuckDB version changes this rendering, this test
        // catches it at CI bump time rather than in production.
        let tmp = std::env::temp_dir().join("phase63_access_mode_pin.duckdb");
        let _ = std::fs::remove_file(&tmp);
        // Bootstrap an empty file with valid header bytes by opening
        // writable then closing.
        {
            let con = Connection::open(&tmp).expect("open writable");
            con.execute_batch("SELECT 1").unwrap();
        }
        let cfg = Config::default()
            .access_mode(AccessMode::ReadOnly)
            .expect("set access_mode");
        let con = Connection::open_with_flags(&tmp, cfg).expect("open read-only");
        let mode: String = con
            .query_row("SELECT current_setting('access_mode')", [], |r| r.get(0))
            .expect("query access_mode");
        assert_eq!(
            mode.to_ascii_lowercase(),
            "read_only",
            "Phase 63: current_setting('access_mode') must return 'read_only' (lowercased) for read-only DBs; got: {mode:?}"
        );
        drop(con);
        let _ = std::fs::remove_file(&tmp);
    }

    #[cfg(not(feature = "extension"))]
    #[test]
    fn access_mode_writable_returns_automatic_or_read_write() {
        use duckdb::Connection;

        // Sibling test: confirm writable connections do NOT match "read_only".
        let con = Connection::open_in_memory().expect("in-memory");
        let mode: String = con
            .query_row("SELECT current_setting('access_mode')", [], |r| r.get(0))
            .expect("query access_mode");
        assert!(
            !mode.eq_ignore_ascii_case("read_only"),
            "Phase 63: in-memory DB must NOT report read_only; got: {mode:?}"
        );
    }
}
