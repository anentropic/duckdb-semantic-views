pub mod catalog;
pub mod expand;
pub mod model;
#[cfg(feature = "extension")]
pub mod query;

/// DDL function implementations — only compiled when building the `DuckDB` extension.
/// The `ddl` module uses `duckdb::vscalar` and `duckdb::vtab`, which are only
/// available when the `extension` feature (and thus `vscalar` + `loadable-extension`)
/// is active.  Under `cargo test` (default `bundled` feature), this module is excluded.
#[cfg(feature = "extension")]
pub mod ddl;

/// Extension entry point — called by `DuckDB` when the extension is loaded.
///
/// Initializes the catalog (`semantic_layer._definitions` schema and table),
/// then registers all four DDL functions on the connection.
///
/// # Safety
///
/// This function is called by `DuckDB` across an FFI boundary. The `con` parameter
/// is provided by `DuckDB` and is guaranteed to be a valid connection handle for
/// the duration of the call. The `#[duckdb_entrypoint_c_api]` macro handles the
/// unsafe C FFI bridging and panic-catching automatically.
#[cfg(feature = "extension")]
mod extension {
    use std::ptr;
    use std::sync::Arc;

    use duckdb::{Connection, Result};
    use libduckdb_sys as ffi;
    use std::error::Error;

    use crate::{
        catalog::init_catalog,
        ddl::{describe::DescribeSemanticViewVTab, list::ListSemanticViewsVTab},
        query::explain::ExplainSemanticViewVTab,
        query::table_function::{QueryState, SemanticViewVTab},
    };

    // Extern C declaration for the C++ shim entry point.
    // Compiled and linked when `--features extension` is active (see build.rs).
    // Phase 10: registers pragma_query_t callbacks.
    // Phase 11: also registers parser hooks for CREATE/DROP SEMANTIC VIEW DDL.
    //   Updated signature: accepts catalog_raw_ptr and persist_conn for the parser hook.
    unsafe extern "C" {
        fn semantic_views_register_shim(
            db_instance_ptr: *mut std::ffi::c_void,
            catalog_raw_ptr: *const std::ffi::c_void,
            persist_conn: ffi::duckdb_connection,
        );
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
        // This connection is passed to the C++ parser hook scan function, which uses it
        // to write to semantic_layer._definitions without deadlocking the main connection's
        // execution lock (context_lock is non-reentrant; a second duckdb_connection has its
        // own context).
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

        // Register table DDL read functions (list_semantic_views, describe_semantic_view).
        // Note: define_semantic_view and drop_semantic_view scalar functions are removed
        // in Phase 11 — native CREATE/DROP SEMANTIC VIEW DDL replaces them via the
        // C++ parser hook registered below.
        con.register_table_function_with_extra_info::<ListSemanticViewsVTab, _>(
            "list_semantic_views",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<DescribeSemanticViewVTab, _>(
            "describe_semantic_view",
            &catalog_state,
        )?;

        // Create a NEW connection for the semantic_query table function.
        // The host connection may hold execution locks during query processing.
        // A separate connection avoids lock conflicts when executing the expanded
        // SQL from within the table function.
        let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
        let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
        if rc != ffi::DuckDBSuccess {
            return Err("Failed to create query connection for semantic_query".into());
        }

        // Register the semantic_query table function.
        let query_state = QueryState {
            catalog: catalog_state.clone(),
            conn: query_conn,
        };
        con.register_table_function_with_extra_info::<SemanticViewVTab, _>(
            "semantic_query",
            &query_state,
        )?;

        // Register the explain_semantic_view table function (shares the same
        // QueryState for catalog access and SQL execution).
        con.register_table_function_with_extra_info::<ExplainSemanticViewVTab, _>(
            "explain_semantic_view",
            &query_state,
        )?;

        // Call C++ shim to register pragma callbacks (Phase 10) and parser hooks (Phase 11).
        // Safety: db_handle is a valid duckdb_database for the extension lifetime.
        //         The shim does not outlive the database instance.
        //         catalog_raw is a non-owning raw pointer — the Arc's refcount is elevated
        //         because catalog_state is cloned into QueryState above, keeping it alive.
        let catalog_raw = Arc::as_ptr(&catalog_state) as *const std::ffi::c_void;
        let raw_persist_conn = persist_conn.unwrap_or(std::ptr::null_mut());
        unsafe {
            semantic_views_register_shim(db_handle.cast(), catalog_raw, raw_persist_conn);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Manual FFI entrypoint (replaces #[duckdb_entrypoint_c_api()] macro)
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

    /// FFI entrypoint called by DuckDB when the extension is loaded.
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
        let init_result = semantic_views_init_c_api_internal(info, access);

        if let Err(x) = init_result {
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
            return false;
        }

        init_result.unwrap()
    }
}
