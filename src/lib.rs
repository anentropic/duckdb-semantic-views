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
        catalog::{init_catalog, spawn_catalog_writer},
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
        // Initialize the catalog: creates schema/table if needed, loads existing rows.
        let catalog_state = init_catalog(&con)?;

        // Resolve the host database file path by querying PRAGMA database_list.
        // The row where name = 'main' gives the host DB file path.
        // For in-memory databases, file is an empty string — we map that to ":memory:".
        let db_path: Arc<str> = {
            // PRAGMA database_list returns (seq INTEGER, name VARCHAR, file VARCHAR).
            // We find the row where name = 'main' and extract the file path.
            // For in-memory databases the file column is an empty string.
            let mut stmt = con.prepare("PRAGMA database_list")?;
            let path = stmt
                .query_map([], |row| {
                    let name: String = row.get(1)?;
                    let file: String = row.get(2)?;
                    Ok((name, file))
                })?
                .filter_map(Result::ok)
                .find(|(name, _)| name == "main")
                .map(|(_, file)| file)
                .unwrap_or_default();
            if path.is_empty() {
                Arc::from(":memory:")
            } else {
                Arc::from(path.as_str())
            }
        };

        // Spawn a background thread to handle catalog writes to the DuckDB file.
        //
        // Scalar function `invoke` cannot safely execute SQL against the host database:
        // DuckDB holds internal locks during query execution, and any SQL on the same
        // database instance (even via a cloned connection) will deadlock or spinlock.
        //
        // The background thread opens its own Connection::open(db_path) — a separate
        // file-level connection — and processes INSERT/DELETE ops via a channel.
        // `invoke` sends an op and blocks on the reply, making the write synchronous
        // without holding any DuckDB locks on the background thread's connection.
        //
        // Returns None for in-memory databases (no file to write; HashMap is the
        // sole source of truth for the session, which is correct behavior).
        let writer = spawn_catalog_writer(db_path.as_ref());

        // Register scalar DDL mutation functions.
        con.register_scalar_function_with_state::<DefineSemanticView>(
            "define_semantic_view",
            &DefineState {
                catalog: catalog_state.clone(),
                writer: writer.clone(),
            },
        )?;
        con.register_scalar_function_with_state::<DropSemanticView>(
            "drop_semantic_view",
            &DropState {
                catalog: catalog_state.clone(),
                writer: writer.clone(),
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
