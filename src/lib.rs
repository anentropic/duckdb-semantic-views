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

    // The duckdb_entrypoint_c_api macro calls this function via `?` and requires
    // the Result return type; `con: Connection` is taken by value because the macro
    // generates the FFI bridge that transfers ownership of the connection handle.
    // Both allow attributes suppress false-positive pedantic lints caused by the
    // macro's required function signature.
    #[allow(clippy::unnecessary_wraps)]
    #[allow(clippy::needless_pass_by_value)]
    #[duckdb_entrypoint_c_api()]
    pub unsafe fn extension_entrypoint(con: Connection) -> Result<(), Box<dyn Error>> {
        // Initialize the catalog: creates schema/table if needed, loads existing rows.
        let catalog_state = init_catalog(&con)?;

        // Resolve the host database file path by querying PRAGMA database_list.
        // This returns rows: (seq INTEGER, name VARCHAR, file VARCHAR).
        // The row where name = 'main' gives the host DB file path.
        // For in-memory databases, file is an empty string — we map that to
        // ":memory:" to preserve the existing sentinel behavior (writes to a
        // second ":memory:" connection are ephemeral, but in-memory DBs cannot
        // survive restart anyway, so no regression).
        //
        // When db_path is a real file path, `invoke` in define_semantic_view
        // and drop_semantic_view will open Connection::open(db_path) and write
        // catalog entries to the actual host database file, satisfying DDL-05.
        let db_path: Arc<str> = {
            let mut stmt =
                con.prepare("SELECT file FROM pragma_database_list() WHERE name = 'main'")?;
            let path = stmt
                .query_row([], |row| row.get::<_, String>(0))
                .unwrap_or_default(); // returns "" for :memory: or on error
            if path.is_empty() {
                Arc::from(":memory:")
            } else {
                Arc::from(path.as_str())
            }
        };

        // Register scalar DDL mutation functions.
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
