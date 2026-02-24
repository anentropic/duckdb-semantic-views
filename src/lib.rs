use duckdb::{duckdb_entrypoint_c_api, Connection, Result};
use std::error::Error;

/// Extension entry point — called by DuckDB when the extension is loaded.
///
/// Phase 1: loads cleanly with no functions registered.
/// Phase 2+ will register scalar/table functions here.
#[duckdb_entrypoint_c_api()]
pub unsafe fn extension_entrypoint(con: Connection) -> Result<(), Box<dyn Error>> {
    let _ = con;
    Ok(())
}
