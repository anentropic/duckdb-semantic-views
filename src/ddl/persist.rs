//! Shared persistence utilities for DDL operations.
//!
//! Provides parameterized query execution via the DuckDB C API prepared
//! statement interface, replacing string interpolation in persistence functions.

use libduckdb_sys as ffi;
use std::ffi::CString;

/// Execute a parameterized query with VARCHAR bindings via the DuckDB C API.
///
/// Uses `duckdb_prepare` + `duckdb_bind_varchar` + `duckdb_execute_prepared`
/// to avoid SQL injection risks from string interpolation.
///
/// Parameters use positional `$1`, `$2`, ... placeholders in the SQL string.
/// The `params` slice is bound in order (1-based indexing).
///
/// # Safety
///
/// `conn` must be a valid, open `duckdb_connection`.
pub(crate) unsafe fn execute_parameterized(
    conn: ffi::duckdb_connection,
    sql: &str,
    params: &[&str],
) -> Result<(), String> {
    let c_sql = CString::new(sql).map_err(|_| "SQL contains null byte".to_string())?;
    let mut stmt: ffi::duckdb_prepared_statement = std::ptr::null_mut();

    let rc = ffi::duckdb_prepare(conn, c_sql.as_ptr(), &mut stmt);
    if rc != ffi::DuckDBSuccess {
        let err = ffi::duckdb_prepare_error(stmt);
        let msg = if err.is_null() {
            "unknown prepare error".to_string()
        } else {
            std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
        };
        ffi::duckdb_destroy_prepare(&mut stmt);
        return Err(msg);
    }

    for (i, param) in params.iter().enumerate() {
        let c_param =
            CString::new(*param).map_err(|_| format!("parameter {} contains null byte", i + 1))?;
        let rc = ffi::duckdb_bind_varchar(stmt, (i + 1) as ffi::idx_t, c_param.as_ptr());
        if rc != ffi::DuckDBSuccess {
            ffi::duckdb_destroy_prepare(&mut stmt);
            return Err(format!("failed to bind parameter {}", i + 1));
        }
    }

    let mut result: ffi::duckdb_result = std::mem::zeroed();
    let rc = ffi::duckdb_execute_prepared(stmt, &mut result);
    let success = rc == ffi::DuckDBSuccess;
    ffi::duckdb_destroy_result(&mut result);
    ffi::duckdb_destroy_prepare(&mut stmt);

    if success {
        Ok(())
    } else {
        Err("prepared statement execution failed".to_string())
    }
}
