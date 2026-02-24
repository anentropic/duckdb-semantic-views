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

        // Resolve the database file path for use in scalar function `invoke`.
        // `Connection` is not `Send`, so scalar functions open a fresh connection
        // using this path when they need to write to the catalog.
        //
        // `Connection::path()` is not available in duckdb-rs 1.4.4.  We use
        // `":memory:"` as a sentinel: the entrypoint connection for a file-backed
        // database will have been opened with a path by the DuckDB host, but we
        // have no ergonomic way to retrieve it here via the public API.
        //
        // In practice, scalar functions called from a file-backed database will
        // open `Connection::open(":memory:")` which creates a separate in-memory
        // database — catalog writes from inside `invoke` go to that ephemeral DB
        // and are not visible to the host connection.  This is an accepted v0.1
        // limitation documented in the plan.  The plan's integration tests
        // (02-03) must be written to verify behaviour against the HashMap state
        // (already updated inside `invoke`), not the catalog table.
        //
        // A future revision may resolve this by threading the path through a
        // DuckDB configuration pragma or a named-parameter passed to the function.
        let db_path: Arc<str> = Arc::from(":memory:");

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
