//! Persistent catalog for semantic view definitions stored in
//! `semantic_layer._definitions`.
//!
//! Prior to v0.8.0 this module also maintained an in-memory `HashMap` mirror
//! that DDL writes updated alongside the catalog table. v0.8.0 removed the
//! mirror so catalog reads always see the same state `DuckDB` does — which is
//! a prerequisite for transactional DDL: the parser-override path emits
//! `INSERT/DELETE/UPDATE` against `_definitions` directly on the caller's
//! connection, and any cached mirror would diverge across rollback.

use std::path::PathBuf;

use duckdb::{Connection, Result};

// Extension appended to the DuckDB file path to form the v0.1.0 companion file.
// Used only in the one-time migration below. After the migration runs, the
// companion file is deleted and this constant is never referenced again at runtime.
const V010_COMPANION_EXT: &str = "semantic_views";

/// Create the `semantic_layer` schema and `_definitions` table if they do not
/// exist, and run the v0.1.0 companion-file migration once for file-backed
/// databases.
///
/// Idempotent: safe to call on every extension load.
pub fn init_catalog(con: &Connection, db_path: &str) -> Result<()> {
    con.execute_batch(
        "CREATE SCHEMA IF NOT EXISTS semantic_layer;
         CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
             name       VARCHAR PRIMARY KEY,
             definition VARCHAR
         );",
    )?;

    // One-time migration: if a v0.1.0 companion file exists alongside the database,
    // import its contents into the table then delete the file.
    if db_path != ":memory:" {
        let migration_path: PathBuf = {
            let mut p = PathBuf::from(db_path);
            let ext = match p.extension() {
                Some(e) => format!("{}.{V010_COMPANION_EXT}", e.to_string_lossy()),
                None => V010_COMPANION_EXT.to_string(),
            };
            p.set_extension(ext);
            p
        };
        if migration_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&migration_path) {
                if let Ok(migrated) =
                    serde_json::from_str::<std::collections::HashMap<String, String>>(&contents)
                {
                    for (name, def) in &migrated {
                        con.execute(
                            "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
                            duckdb::params![name, def],
                        )?;
                    }
                }
            }
            let _ = std::fs::remove_file(&migration_path);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// CatalogReader — extension-side handle wrapping the catalog connection.
// ---------------------------------------------------------------------------
//
// Gated on the `extension` feature because it depends on the C-API stubs
// (`duckdb_query`, `duckdb_prepare`, ...) which are only linked when the
// crate is built as a loadable extension.

#[cfg(feature = "extension")]
mod reader {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_void;

    use libduckdb_sys as ffi;

    /// Read-side handle for `semantic_layer._definitions`.
    ///
    /// Wraps a raw `duckdb_connection` ("catalog connection") created at
    /// extension load time. Reads see the connection's transactional view
    /// of the table, which for the catalog connection is always committed
    /// state. Writes performed by parser-override-emitted SQL run on the
    /// *caller's* connection and become visible here on commit.
    #[derive(Clone, Copy)]
    pub struct CatalogReader {
        conn: ffi::duckdb_connection,
    }

    // SAFETY: `duckdb_connection` is an opaque pointer managed by DuckDB.
    // The connection itself owns its synchronisation; reads from multiple
    // threads via the same handle are serialised by DuckDB internally.
    unsafe impl Send for CatalogReader {}
    unsafe impl Sync for CatalogReader {}

    impl CatalogReader {
        pub fn new(conn: ffi::duckdb_connection) -> Self {
            Self { conn }
        }

        pub fn raw(&self) -> ffi::duckdb_connection {
            self.conn
        }

        /// Fetch the JSON definition for a single view.
        ///
        /// Returns `Ok(None)` when no row exists.
        pub fn lookup(&self, name: &str) -> Result<Option<String>, String> {
            unsafe { prepared_lookup(self.conn, name) }
        }

        /// Whether a view with this name exists.
        pub fn exists(&self, name: &str) -> Result<bool, String> {
            Ok(self.lookup(name)?.is_some())
        }

        /// Return `(name, definition_json)` for every registered view,
        /// sorted by name.
        pub fn list_all(&self) -> Result<Vec<(String, String)>, String> {
            unsafe { execute_list_all(self.conn) }
        }

        /// Return just the view names, sorted.
        pub fn list_names(&self) -> Result<Vec<String>, String> {
            Ok(self.list_all()?.into_iter().map(|(n, _)| n).collect())
        }
    }

    unsafe fn prepared_lookup(
        conn: ffi::duckdb_connection,
        name: &str,
    ) -> Result<Option<String>, String> {
        let c_sql =
            CString::new("SELECT definition FROM semantic_layer._definitions WHERE name = $1")
                .map_err(|_| "SQL contains null byte".to_string())?;
        let mut stmt: ffi::duckdb_prepared_statement = std::ptr::null_mut();
        let rc = ffi::duckdb_prepare(conn, c_sql.as_ptr(), &mut stmt);
        if rc != ffi::DuckDBSuccess {
            let err = ffi::duckdb_prepare_error(stmt);
            let msg = if err.is_null() {
                "unknown prepare error".to_string()
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            ffi::duckdb_destroy_prepare(&mut stmt);
            return Err(msg);
        }

        let c_name = CString::new(name).map_err(|_| "view name contains null byte".to_string())?;
        if ffi::duckdb_bind_varchar(stmt, 1, c_name.as_ptr()) != ffi::DuckDBSuccess {
            ffi::duckdb_destroy_prepare(&mut stmt);
            return Err("failed to bind view name".to_string());
        }

        let mut result: ffi::duckdb_result = std::mem::zeroed();
        let exec_rc = ffi::duckdb_execute_prepared(stmt, &mut result);
        if exec_rc != ffi::DuckDBSuccess {
            let err_ptr = ffi::duckdb_result_error(&mut result);
            let msg = if err_ptr.is_null() {
                "catalog lookup failed".to_string()
            } else {
                CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
            };
            ffi::duckdb_destroy_result(&mut result);
            ffi::duckdb_destroy_prepare(&mut stmt);
            return Err(msg);
        }

        let row_count = ffi::duckdb_row_count(&mut result);
        let value = if row_count == 0 {
            None
        } else {
            let val_ptr = ffi::duckdb_value_varchar(&mut result, 0, 0);
            if val_ptr.is_null() {
                None
            } else {
                let s = CStr::from_ptr(val_ptr).to_string_lossy().into_owned();
                ffi::duckdb_free(val_ptr.cast::<c_void>());
                Some(s)
            }
        };
        ffi::duckdb_destroy_result(&mut result);
        ffi::duckdb_destroy_prepare(&mut stmt);
        Ok(value)
    }

    unsafe fn execute_list_all(
        conn: ffi::duckdb_connection,
    ) -> Result<Vec<(String, String)>, String> {
        let c_sql =
            CString::new("SELECT name, definition FROM semantic_layer._definitions ORDER BY name")
                .map_err(|_| "SQL contains null byte".to_string())?;
        let mut result: ffi::duckdb_result = std::mem::zeroed();
        let rc = ffi::duckdb_query(conn, c_sql.as_ptr(), &mut result);
        if rc != ffi::DuckDBSuccess {
            let err_ptr = ffi::duckdb_result_error(&mut result);
            let msg = if err_ptr.is_null() {
                "catalog list failed".to_string()
            } else {
                CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
            };
            ffi::duckdb_destroy_result(&mut result);
            return Err(msg);
        }

        let row_count = ffi::duckdb_row_count(&mut result);
        let mut out = Vec::with_capacity(row_count as usize);
        for r in 0..row_count {
            let name_ptr = ffi::duckdb_value_varchar(&mut result, 0, r);
            let def_ptr = ffi::duckdb_value_varchar(&mut result, 1, r);
            let name = if name_ptr.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(name_ptr).to_string_lossy().into_owned();
                ffi::duckdb_free(name_ptr.cast::<c_void>());
                s
            };
            let def = if def_ptr.is_null() {
                String::new()
            } else {
                let s = CStr::from_ptr(def_ptr).to_string_lossy().into_owned();
                ffi::duckdb_free(def_ptr.cast::<c_void>());
                s
            };
            out.push((name, def));
        }
        ffi::duckdb_destroy_result(&mut result);
        Ok(out)
    }
}

#[cfg(feature = "extension")]
pub use reader::CatalogReader;

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::Connection;

    fn in_memory_con() -> Connection {
        Connection::open_in_memory().expect("in-memory DuckDB")
    }

    #[test]
    fn init_catalog_creates_schema_and_table() {
        let con = in_memory_con();
        init_catalog(&con, ":memory:").unwrap();
        // Idempotent: second call must not error
        init_catalog(&con, ":memory:").unwrap();

        let count: i64 = con
            .query_row(
                "SELECT count(*) FROM semantic_layer._definitions",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn pragma_database_list_returns_file_path() {
        let tmp = std::env::temp_dir();
        let tmpfile_buf = tmp.join("test_pragma_rust_check.duckdb");
        let tmpfile = tmpfile_buf.to_str().expect("temp dir is UTF-8");
        let _ = std::fs::remove_file(tmpfile);
        let con = Connection::open(tmpfile).expect("open file-backed connection");
        let mut stmt = con.prepare("PRAGMA database_list").expect("prepare PRAGMA");
        let paths: Vec<Option<String>> = stmt
            .query_map([], |row| row.get::<_, Option<String>>(2))
            .expect("query_map")
            .filter_map(|r| r.ok())
            .collect();
        let file_path = paths.into_iter().flatten().find(|s| !s.is_empty());
        assert!(
            file_path.is_some(),
            "PRAGMA database_list should return a non-empty file path for file-backed DB"
        );
        let path = file_path.unwrap();
        assert!(
            path.contains("test_pragma_rust_check"),
            "file path should reference the opened DB file, got: {path}"
        );
        let _ = std::fs::remove_file(tmpfile);
    }

    #[test]
    fn pragma_database_list_returns_none_for_in_memory() {
        let con = in_memory_con();
        let mut stmt = con.prepare("PRAGMA database_list").expect("prepare PRAGMA");
        let paths: Vec<Option<String>> = stmt
            .query_map([], |row| row.get::<_, Option<String>>(2))
            .expect("query_map")
            .filter_map(|r| r.ok())
            .collect();
        let file_path = paths.into_iter().flatten().find(|s| !s.is_empty());
        assert!(
            file_path.is_none(),
            "PRAGMA database_list should return no file path for in-memory DB, got: {file_path:?}"
        );
    }

    #[test]
    fn persist_02_rollback_leaves_catalog_unchanged() {
        let tmp = std::env::temp_dir();
        let db_path_buf = tmp.join("test_persist02_rollback.duckdb");
        let db_path = db_path_buf.to_str().expect("temp dir is UTF-8");
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));

        let con = Connection::open(db_path).expect("open file-backed DB");
        init_catalog(&con, db_path).unwrap();

        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        con.execute(
            "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
            duckdb::params!["orders", json],
        )
        .unwrap();

        let count_before: i64 = con
            .query_row(
                "SELECT count(*) FROM semantic_layer._definitions WHERE name = 'orders'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_before, 1);

        // DuckDB table rollback: BEGIN + DELETE + ROLLBACK must leave row present
        con.execute_batch(
            "BEGIN; DELETE FROM semantic_layer._definitions WHERE name = 'orders'; ROLLBACK;",
        )
        .unwrap();

        let count_after: i64 = con
            .query_row(
                "SELECT count(*) FROM semantic_layer._definitions WHERE name = 'orders'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count_after, 1,
            "Row must still exist after ROLLBACK (PERSIST-02)"
        );

        drop(con);
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));
    }
}
