pub mod catalog;
pub mod model;

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
    use std::sync::Arc;

    use duckdb::{duckdb_entrypoint_c_api, Connection, Result};
    use std::error::Error;

    use crate::{
        catalog::init_catalog,
        ddl::{
            define::{DefineSemanticView, DefineState},
            describe::DescribeSemanticViewVTab,
            drop::{DropSemanticView, DropState},
            list::ListSemanticViewsVTab,
        },
    };

    #[allow(clippy::unnecessary_wraps)]
    #[allow(clippy::needless_pass_by_value)]
    #[duckdb_entrypoint_c_api()]
    pub unsafe fn extension_entrypoint(con: Connection) -> Result<(), Box<dyn Error>> {
        // Resolve the host database file path by querying PRAGMA database_list.
        // This must happen before init_catalog so the path can be passed in.
        //
        // PRAGMA database_list returns (seq, name, file).
        // The main database name is NOT always "main" — when opened via the
        // Python DuckDB client, the name is derived from the filename stem
        // (e.g. "restart_test" for "restart_test.db").  We take the first
        // row with a non-empty file path, which is always the primary DB.
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

        // Initialize the catalog: creates schema/table if needed, loads existing
        // rows from the DuckDB table, and merges in any sidecar file data (for
        // file-backed databases).  The sidecar is written by invoke during
        // define/drop — it bridges the gap because invoke cannot execute SQL.
        let catalog_state = init_catalog(&con, &db_path)?;

        // Register scalar DDL mutation functions.
        // State carries the db_path so invoke can write the sidecar file.
        con.register_scalar_function_with_state::<DefineSemanticView>(
            "define_semantic_view",
            &DefineState {
                catalog: catalog_state.clone(),
                db_path: db_path.clone(),
            },
        )?;
        con.register_scalar_function_with_state::<DropSemanticView>(
            "drop_semantic_view",
            &DropState {
                catalog: catalog_state.clone(),
                db_path: db_path.clone(),
            },
        )?;

        // Register table DDL read functions.
        con.register_table_function_with_extra_info::<ListSemanticViewsVTab, _>(
            "list_semantic_views",
            &catalog_state,
        )?;
        con.register_table_function_with_extra_info::<DescribeSemanticViewVTab, _>(
            "describe_semantic_view",
            &catalog_state,
        )?;

        Ok(())
    }
}
