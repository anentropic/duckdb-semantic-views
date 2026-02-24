pub mod model;

use duckdb::{duckdb_entrypoint_c_api, Connection, Result};
use std::error::Error;

/// Extension entry point — called by `DuckDB` when the extension is loaded.
///
/// Phase 1: loads cleanly with no functions registered.
/// Phase 2+ will register scalar/table functions here.
///
/// # Safety
///
/// This function is called by `DuckDB` across an FFI boundary. The `con` parameter
/// is provided by `DuckDB` and is guaranteed to be a valid connection handle for
/// the duration of the call. The `#[duckdb_entrypoint_c_api]` macro handles the
/// unsafe C FFI bridging and panic-catching automatically.
#[duckdb_entrypoint_c_api()]
pub unsafe fn extension_entrypoint(_con: Connection) -> Result<(), Box<dyn Error>> {
    Ok(())
}
