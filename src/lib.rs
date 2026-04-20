pub mod body_parser;
pub mod catalog;
pub mod errors;
pub mod expand;
pub mod graph;
pub mod model;
pub mod parse;
#[cfg(feature = "extension")]
pub mod query;
pub mod render_ddl;
pub mod render_yaml;
pub mod util;

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
    use std::ptr;
    use std::sync::Arc;

    use duckdb::{Connection, Result};
    use libduckdb_sys as ffi;
    use std::error::Error;

    use crate::{
        catalog::init_catalog,
        ddl::{
            alter::{
                AlterCommentState, AlterRenameState, AlterRenameVTab, AlterSetCommentVTab,
                AlterUnsetCommentVTab,
            },
            define::{DefineFromJsonVTab, DefineState},
            describe::DescribeSemanticViewVTab,
            drop::{DropSemanticViewVTab, DropState},
            get_ddl::GetDdlScalar,
            list::{ListSemanticViewsVTab, ListTerseSemanticViewsVTab},
            show_columns::ShowColumnsInSemanticViewVTab,
            show_dims::{ShowSemanticDimensionsAllVTab, ShowSemanticDimensionsVTab},
            show_dims_for_metric::ShowDimensionsForMetricVTab,
            show_facts::{ShowSemanticFactsAllVTab, ShowSemanticFactsVTab},
            show_metrics::{ShowSemanticMetricsAllVTab, ShowSemanticMetricsVTab},
        },
        query::explain::ExplainSemanticViewVTab,
        query::table_function::{QueryState, SemanticViewVTab},
    };

    // C++ helper for parser hook registration (defined in cpp/src/shim.cpp)
    extern "C" {
        fn sv_register_parser_hooks(
            db_handle: ffi::duckdb_database,
            ddl_conn: ffi::duckdb_connection,
        ) -> bool;
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

        // Initialize the catalog.
        let catalog_state = init_catalog(con, &db_path)?;

        // Create a separate connection for DDL persistence.
        // Only created for file-backed databases — in-memory DBs use HashMap only.
        // This connection is used by define_semantic_view / drop_semantic_view via FFI
        // to write to semantic_layer._definitions without deadlocking the main
        // connection's execution lock (context_lock is non-reentrant; a second
        // duckdb_connection has its own context).
        let persist_conn: Option<ffi::duckdb_connection> = if db_path.as_ref() != ":memory:" {
            let mut conn: ffi::duckdb_connection = ptr::null_mut();
            let rc = unsafe { ffi::duckdb_connect(db_handle, &mut conn) };
            if rc != ffi::DuckDBSuccess {
                return Err("Failed to create persist connection for DDL writes".into());
            }
            Some(conn)
        } else {
            None
        };

        // Register create_semantic_view_from_json(name, json) -- target for native DDL.
        // The native DDL path (CREATE SEMANTIC VIEW ... AS ...) rewrites to a call to
        // these _from_json table functions. Three variants cover the 3 CREATE forms.
        // Create a dedicated connection for catalog queries (duckdb_constraints() lookups).
        // Always created for both file-backed and in-memory databases.
        let mut catalog_conn: ffi::duckdb_connection = ptr::null_mut();
        let rc = unsafe { ffi::duckdb_connect(db_handle, &mut catalog_conn) };
        if rc != ffi::DuckDBSuccess {
            return Err("Failed to create catalog connection for PK resolution".into());
        }

        let define_state = DefineState {
            catalog: catalog_state.clone(),
            persist_conn,
            catalog_conn,
            or_replace: false,
            if_not_exists: false,
        };
        con.register_table_function_with_extra_info::<DefineFromJsonVTab, _>(
            "create_semantic_view_from_json",
            &define_state,
        )?;

        let define_or_replace_state = DefineState {
            catalog: catalog_state.clone(),
            persist_conn,
            catalog_conn,
            or_replace: true,
            if_not_exists: false,
        };
        con.register_table_function_with_extra_info::<DefineFromJsonVTab, _>(
            "create_or_replace_semantic_view_from_json",
            &define_or_replace_state,
        )?;

        let define_if_not_exists_state = DefineState {
            catalog: catalog_state.clone(),
            persist_conn,
            catalog_conn,
            or_replace: false,
            if_not_exists: true,
        };
        con.register_table_function_with_extra_info::<DefineFromJsonVTab, _>(
            "create_semantic_view_if_not_exists_from_json",
            &define_if_not_exists_state,
        )?;

        // Register drop_semantic_view(name) table function.
        let drop_state = DropState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: false,
        };
        con.register_table_function_with_extra_info::<DropSemanticViewVTab, _>(
            "drop_semantic_view",
            &drop_state,
        )?;

        // Register drop_semantic_view_if_exists(name) table function.
        let drop_if_exists_state = DropState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: true,
        };
        con.register_table_function_with_extra_info::<DropSemanticViewVTab, _>(
            "drop_semantic_view_if_exists",
            &drop_if_exists_state,
        )?;

        // Register alter_semantic_view_rename(old, new) table function.
        let alter_state = AlterRenameState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: false,
        };
        con.register_table_function_with_extra_info::<AlterRenameVTab, _>(
            "alter_semantic_view_rename",
            &alter_state,
        )?;

        // Register alter_semantic_view_rename_if_exists(old, new) table function.
        let alter_if_exists_state = AlterRenameState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: true,
        };
        con.register_table_function_with_extra_info::<AlterRenameVTab, _>(
            "alter_semantic_view_rename_if_exists",
            &alter_if_exists_state,
        )?;

        // Register alter_semantic_view_set_comment(name, comment) table function.
        let alter_set_comment_state = AlterCommentState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: false,
        };
        con.register_table_function_with_extra_info::<AlterSetCommentVTab, _>(
            "alter_semantic_view_set_comment",
            &alter_set_comment_state,
        )?;

        // Register alter_semantic_view_set_comment_if_exists(name, comment) table function.
        let alter_set_comment_if_exists_state = AlterCommentState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: true,
        };
        con.register_table_function_with_extra_info::<AlterSetCommentVTab, _>(
            "alter_semantic_view_set_comment_if_exists",
            &alter_set_comment_if_exists_state,
        )?;

        // Register alter_semantic_view_unset_comment(name) table function.
        let alter_unset_comment_state = AlterCommentState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: false,
        };
        con.register_table_function_with_extra_info::<AlterUnsetCommentVTab, _>(
            "alter_semantic_view_unset_comment",
            &alter_unset_comment_state,
        )?;

        // Register alter_semantic_view_unset_comment_if_exists(name) table function.
        let alter_unset_comment_if_exists_state = AlterCommentState {
            catalog: catalog_state.clone(),
            persist_conn,
            if_exists: true,
        };
        con.register_table_function_with_extra_info::<AlterUnsetCommentVTab, _>(
            "alter_semantic_view_unset_comment_if_exists",
            &alter_unset_comment_if_exists_state,
        )?;

        // Register table DDL read functions (list_semantic_views, describe_semantic_view).
        con.register_table_function_with_extra_info::<ListSemanticViewsVTab, _>(
            "list_semantic_views",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<ListTerseSemanticViewsVTab, _>(
            "list_terse_semantic_views",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<ShowColumnsInSemanticViewVTab, _>(
            "show_columns_in_semantic_view",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<DescribeSemanticViewVTab, _>(
            "describe_semantic_view",
            &catalog_state,
        )?;

        // SHOW SEMANTIC DIMENSIONS
        con.register_table_function_with_extra_info::<ShowSemanticDimensionsVTab, _>(
            "show_semantic_dimensions",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<ShowSemanticDimensionsAllVTab, _>(
            "show_semantic_dimensions_all",
            &catalog_state,
        )?;

        // SHOW SEMANTIC DIMENSIONS FOR METRIC
        con.register_table_function_with_extra_info::<ShowDimensionsForMetricVTab, _>(
            "show_semantic_dimensions_for_metric",
            &catalog_state,
        )?;

        // SHOW SEMANTIC METRICS
        con.register_table_function_with_extra_info::<ShowSemanticMetricsVTab, _>(
            "show_semantic_metrics",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<ShowSemanticMetricsAllVTab, _>(
            "show_semantic_metrics_all",
            &catalog_state,
        )?;

        // SHOW SEMANTIC FACTS
        con.register_table_function_with_extra_info::<ShowSemanticFactsVTab, _>(
            "show_semantic_facts",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<ShowSemanticFactsAllVTab, _>(
            "show_semantic_facts_all",
            &catalog_state,
        )?;

        // Register GET_DDL scalar function.
        con.register_scalar_function_with_state::<GetDdlScalar>("get_ddl", &catalog_state)?;

        // Create a NEW connection for the semantic_view table function.
        let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
        let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
        if rc != ffi::DuckDBSuccess {
            return Err("Failed to create query connection for semantic_view".into());
        }

        // Register the semantic_view table function.
        let query_state = QueryState {
            catalog: catalog_state.clone(),
            conn: query_conn,
        };
        con.register_table_function_with_extra_info::<SemanticViewVTab, _>(
            "semantic_view",
            &query_state,
        )?;

        // Register the explain_semantic_view table function.
        con.register_table_function_with_extra_info::<ExplainSemanticViewVTab, _>(
            "explain_semantic_view",
            &query_state,
        )?;

        // Create a dedicated DDL connection for the parser hook path.
        // This connection is ALWAYS created (even for in-memory databases) because
        // the native DDL path rewrites to `SELECT * FROM create_semantic_view(...)`
        // which needs a separate connection to avoid deadlocking the ClientContext
        // lock held during plan/bind. This is distinct from persist_conn (which
        // writes to the _definitions catalog table for file-backed databases).
        let mut ddl_conn: ffi::duckdb_connection = ptr::null_mut();
        let rc = unsafe { ffi::duckdb_connect(db_handle, &mut ddl_conn) };
        if rc != ffi::DuckDBSuccess {
            return Err("Failed to create DDL connection for parser hook".into());
        }

        // Register parser hooks via C++ helper.
        // The C++ shim extracts DatabaseInstance& from the duckdb_database handle
        // and registers ParserExtension hooks on DBConfig. This requires C++ types
        // that are only available via the duckdb.hpp amalgamation header.
        if !unsafe { sv_register_parser_hooks(db_handle, ddl_conn) } {
            return Err("Failed to register parser hooks via C++ helper".into());
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

    const MINIMUM_DUCKDB_VERSION: &str = "v1.4.4";

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
            ffi::duckdb_rs_extension_api_init(info, access, MINIMUM_DUCKDB_VERSION).unwrap();

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

        match init_result {
            Ok(Ok(val)) => val,
            Ok(Err(x)) => {
                let error_c_string = std::ffi::CString::new(x.to_string());
                match error_c_string {
                    Ok(e) => {
                        (*access).set_error.unwrap()(info, e.as_ptr());
                    }
                    Err(_e) => {
                        let error_alloc_failure = c"An error occurred but the extension failed to allocate memory for an error string";
                        (*access).set_error.unwrap()(info, error_alloc_failure.as_ptr());
                    }
                }
                false
            }
            Err(_panic) => {
                let panic_msg =
                    c"Extension init panicked unexpectedly. This is a bug in semantic_views.";
                (*access).set_error.unwrap()(info, panic_msg.as_ptr());
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Guard that `duckdb_value` (used by `value_raw_ptr` transmute in
    /// `query/table_function.rs`) remains pointer-sized. The full layout
    /// check against `duckdb::vtab::Value` requires the `extension` feature
    /// (which enables `duckdb/vscalar`) and is validated during extension
    /// builds via `just build`.
    #[test]
    fn duckdb_value_is_pointer_sized() {
        use libduckdb_sys as ffi;
        assert_eq!(
            std::mem::size_of::<ffi::duckdb_value>(),
            std::mem::size_of::<*mut std::ffi::c_void>(),
            "duckdb_value is no longer pointer-sized -- value_raw_ptr transmute may be broken"
        );
        assert_eq!(
            std::mem::align_of::<ffi::duckdb_value>(),
            std::mem::align_of::<*mut std::ffi::c_void>(),
            "duckdb_value alignment changed -- value_raw_ptr transmute may be broken"
        );
    }
}
