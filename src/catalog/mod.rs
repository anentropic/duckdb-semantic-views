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

use duckdb::Connection;

/// Write-side SQL builders (existence/collision guards) for `_definitions`.
pub(crate) mod writes;

// Extension appended to the DuckDB file path to form the v0.1.0 companion file.
// Used only in the one-time migration below. After the migration runs, the
// companion file is deleted and this constant is never referenced again at runtime.
const V010_COMPANION_EXT: &str = "semantic_views";

/// Schema holding the semantic-view catalog table.
pub const DEFINITIONS_SCHEMA: &str = "semantic_layer";
/// Bare (unqualified) name of the semantic-view catalog table.
pub const DEFINITIONS_TABLE_NAME: &str = "_definitions";
/// Fully-qualified catalog table where all semantic-view definitions are stored.
///
/// This is the single source of truth for the table reference; every SQL builder
/// that reads or writes definitions embeds this constant rather than the literal
/// string. The `DEFINITIONS_TABLE == DEFINITIONS_SCHEMA.DEFINITIONS_TABLE_NAME`
/// relationship is asserted by `tests::definitions_table_const_is_consistent`.
pub const DEFINITIONS_TABLE: &str = "semantic_layer._definitions";

/// Canonical "view does not exist" error wording, shared by every read-side DDL
/// command so the message stays identical across the surface. The SQL-side guard
/// selects in the sibling [`writes`] module intentionally inline an escaped copy
/// of this wording.
#[must_use]
pub fn view_not_found_msg(name: &str) -> String {
    format!("semantic view '{name}' does not exist")
}

/// Create the `semantic_layer` schema and `_definitions` table if they do not
/// exist, and run the v0.1.0 companion-file migration once for file-backed
/// databases.
///
/// Idempotent: safe to call on every extension load.
///
/// Phase 63 (v0.9.0): when `is_read_only=true`, skips the entire body —
/// the host DB is read-only so neither the schema/table CREATE nor the
/// companion-file migration (which INSERTs into the table) can run.
/// Reader-path code in `CatalogReader` short-circuits separately when
/// the table is genuinely absent. See 63-RESEARCH.md §3 Q2.
pub fn init_catalog(
    con: &Connection,
    db_path: &str,
    is_read_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_read_only {
        return Ok(());
    }
    // FF-10: `definition` is `NOT NULL`. A SQL-NULL definition is an
    // unrecoverable-looking state — readers treat a NULL definition as
    // "view does not exist" while the write-side existence guards see the row
    // as present, so a manually-tampered NULL row can neither be read nor
    // re-created. The constraint makes that state unrepresentable for new
    // catalogs (all writes always supply a definition).
    con.execute_batch(&format!(
        "CREATE SCHEMA IF NOT EXISTS {DEFINITIONS_SCHEMA};
         CREATE TABLE IF NOT EXISTS {DEFINITIONS_TABLE} (
             name       VARCHAR PRIMARY KEY,
             definition VARCHAR NOT NULL
         );"
    ))?;

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
            let contents = std::fs::read_to_string(&migration_path).map_err(|e| {
                format!(
                    "semantic_views: cannot read v0.1.0 companion file '{}': {e}. \
                     The file was left in place — fix its permissions (or move it \
                     away to skip migration) and re-LOAD.",
                    migration_path.display()
                )
            })?;
            let migrated: std::collections::HashMap<String, String> =
                serde_json::from_str(&contents).map_err(|e| {
                    format!(
                        "semantic_views: v0.1.0 companion file '{}' is not valid JSON: {e}. \
                         The file was left in place so its definitions are not lost — \
                         repair it (or move it away to skip migration) and re-LOAD.",
                        migration_path.display()
                    )
                })?;
            for (name, def) in &migrated {
                con.execute(
                    &format!(
                        "INSERT OR REPLACE INTO {DEFINITIONS_TABLE} (name, definition) VALUES (?, ?)"
                    ),
                    duckdb::params![name, def],
                )?;
            }
            // Delete ONLY after a fully successful import. Pre-fix the file was
            // removed even when unreadable or corrupt, permanently destroying
            // the user's pre-v0.2 definitions. A failed delete must also be an
            // error: if the file survives, every subsequent LOAD re-imports
            // this (now stale) snapshot over newer definitions via
            // INSERT OR REPLACE.
            std::fs::remove_file(&migration_path).map_err(|e| {
                format!(
                    "semantic_views: imported v0.1.0 companion file '{}' but could \
                     not delete it: {e}. Delete it manually before the next LOAD to \
                     avoid re-importing stale definitions.",
                    migration_path.display()
                )
            })?;
        }
    }

    // AR-4: one-time storage-format upgrade pass. Runs after the v0.1.0
    // companion import so freshly-imported rows are considered too. Only on
    // writable DBs (guarded by the is_read_only early-return above).
    upgrade_definitions_schema(con)?;

    Ok(())
}

/// One-time `schema_version` upgrade pass over `_definitions` (AR-4).
///
/// For every stored row still below [`crate::model::CURRENT_SCHEMA_VERSION`]:
/// stamp it to the current version **iff** it can be positively verified as
/// current-format (parses cleanly and every relationship carries `fk_columns`).
/// Rows that fail to parse, or whose relationships lack FK metadata (legacy
/// pre-Phase-24 encodings), are left untouched at version 0 — the fan-trap
/// safety check (`expand::fan_trap`) then hard-errors on read rather than
/// silently trusting them. Non-destructive: no row is deleted or rewritten
/// beyond the version stamp.
///
/// Idempotent: rows already at the current version are skipped, so subsequent
/// loads are no-ops. The `schema_version` integer is inlined from a
/// compile-time constant (no user input), so the `json_object` embed is safe.
fn upgrade_definitions_schema(con: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let rows: Vec<(String, String)> = {
        let mut stmt = con.prepare(&format!("SELECT name, definition FROM {DEFINITIONS_TABLE}"))?;
        let mapped =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };

    for (name, json) in rows {
        if crate::model::SemanticViewDefinition::stored_schema_version(&json)
            >= crate::model::CURRENT_SCHEMA_VERSION
        {
            continue; // already current
        }
        // Only stamp rows we can positively verify as current-format. A parse
        // failure or missing FK metadata => un-upgradeable legacy row; leave it
        // at version 0 so reads hard-error rather than silently under-checking.
        let upgradeable = crate::model::SemanticViewDefinition::from_json(&name, &json)
            .is_ok_and(|def| !def.has_incomplete_relationships());
        if !upgradeable {
            continue;
        }
        con.execute(
            &format!(
                "UPDATE {DEFINITIONS_TABLE} \
                 SET definition = json_merge_patch(definition::JSON, \
                     json_object('schema_version', {version}))::VARCHAR \
                 WHERE name = ?",
                version = crate::model::CURRENT_SCHEMA_VERSION
            ),
            duckdb::params![name],
        )?;
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
    use std::marker::PhantomData;
    use std::os::raw::c_void;

    use libduckdb_sys as ffi;

    use crate::catalog::DEFINITIONS_TABLE;
    use crate::ddl::read_ffi::BorrowedConnection;

    /// Read-side handle for `semantic_layer._definitions`.
    ///
    /// Wraps a raw `duckdb_connection` ("catalog connection") created at
    /// extension load time. Reads see the connection's transactional view
    /// of the table, which for the catalog connection is always committed
    /// state. Writes performed by parser-override-emitted SQL run on the
    /// *caller's* connection and become visible here on commit.
    ///
    /// The `'a` lifetime ties the reader to the `&'a BorrowedConnection` it
    /// was constructed from (Phase 65.1 WR-05 follow-up to Copilot PR #35
    /// review): the underlying raw handle is owned by the caller's stack
    /// `Connection probe(*context.db)`, so allowing a `CatalogReader` to
    /// escape that scope would be use-after-free. `PhantomData<&'a
    /// BorrowedConnection>` enforces this at compile time. Because
    /// `BorrowedConnection` is neither `Send` nor `Sync` (it wraps a raw
    /// pointer), `&'a BorrowedConnection` is neither either, so
    /// `CatalogReader<'a>` inherits the !Send / !Sync auto-traits without
    /// any `unsafe impl` claiming otherwise.
    pub struct CatalogReader<'a> {
        conn: ffi::duckdb_connection,
        // Phase 63 (v0.9.0): when false (only possible on a read-only host
        // DB whose semantic_layer._definitions table was never created),
        // reader methods short-circuit to "empty" / "not found" without
        // hitting the DB. On a writable host this is always true (the
        // table is CREATE'd in init_catalog).
        catalog_table_present: bool,
        _borrow: PhantomData<&'a BorrowedConnection>,
    }

    impl<'a> CatalogReader<'a> {
        /// Construct a `CatalogReader` from a borrowed connection (D-10 / WR-05).
        ///
        /// The handle is extracted via `borrowed.as_raw()` and stored as a raw
        /// pointer in the reader. `CatalogReader` never disconnects — the
        /// underlying lifetime is owned by the caller's stack `Connection`
        /// (the BORROW contract documented at module scope of
        /// `src/ddl/read_ffi.rs`). The `'a` lifetime parameter prevents safe
        /// code from moving the reader past that scope.
        pub fn new(borrowed: &'a BorrowedConnection, catalog_table_present: bool) -> Self {
            Self {
                conn: borrowed.as_raw(),
                catalog_table_present,
                _borrow: PhantomData,
            }
        }

        pub fn raw(&self) -> ffi::duckdb_connection {
            self.conn
        }

        /// Fetch the JSON definition for a single view.
        ///
        /// Returns `Ok(None)` when no row exists.
        ///
        /// Phase 63: when `catalog_table_present=false` (read-only host DB
        /// without a bootstrapped `_definitions` table), short-circuits to
        /// `Ok(None)` BEFORE the unsafe FFI call. Callers see the existing
        /// "semantic view '<name>' does not exist" error path. See
        /// 63-RESEARCH.md §3 Q4.
        pub fn lookup(&self, name: &str) -> Result<Option<String>, String> {
            if !self.catalog_table_present {
                return Ok(None);
            }
            unsafe { prepared_lookup(self.conn, name) }
        }

        /// Whether a view with this name exists.
        pub fn exists(&self, name: &str) -> Result<bool, String> {
            Ok(self.lookup(name)?.is_some())
        }

        /// Return `(name, definition_json)` for every registered view,
        /// sorted by name.
        ///
        /// Phase 63: short-circuits to `Ok(Vec::new())` when
        /// `catalog_table_present=false`.
        pub fn list_all(&self) -> Result<Vec<(String, String)>, String> {
            if !self.catalog_table_present {
                return Ok(Vec::new());
            }
            unsafe { execute_list_all(self.conn) }
        }

        /// Return just the view names, sorted. Used by error-path suggestion
        /// helpers ("did you mean ...?"); avoids reading the full JSON
        /// definition column that `list_all` would pull.
        ///
        /// Phase 63: short-circuits to `Ok(Vec::new())` when
        /// `catalog_table_present=false`.
        pub fn list_names(&self) -> Result<Vec<String>, String> {
            if !self.catalog_table_present {
                return Ok(Vec::new());
            }
            unsafe { execute_list_names(self.conn) }
        }
    }

    /// RAII guard for `duckdb_prepared_statement`. Drops the statement on
    /// scope exit even when the body short-circuits via `?`. Pre-v0.8.0
    /// every error path repeated `duckdb_destroy_prepare` by hand.
    struct PreparedStmt {
        ptr: ffi::duckdb_prepared_statement,
    }

    impl PreparedStmt {
        unsafe fn prepare(conn: ffi::duckdb_connection, sql: &CStr) -> Result<Self, String> {
            let mut ptr: ffi::duckdb_prepared_statement = std::ptr::null_mut();
            let rc = ffi::duckdb_prepare(conn, sql.as_ptr(), &mut ptr);
            if rc != ffi::DuckDBSuccess {
                let err = ffi::duckdb_prepare_error(ptr);
                let msg = if err.is_null() {
                    "unknown prepare error".to_string()
                } else {
                    CStr::from_ptr(err).to_string_lossy().into_owned()
                };
                ffi::duckdb_destroy_prepare(&mut ptr);
                return Err(msg);
            }
            Ok(Self { ptr })
        }

        fn raw(&self) -> ffi::duckdb_prepared_statement {
            self.ptr
        }
    }

    impl Drop for PreparedStmt {
        fn drop(&mut self) {
            unsafe { ffi::duckdb_destroy_prepare(&mut self.ptr) };
        }
    }

    /// RAII guard for `duckdb_result`. Like `PreparedStmt`, removes the
    /// per-error-path `duckdb_destroy_result` calls.
    struct QueryResult {
        inner: ffi::duckdb_result,
    }

    impl QueryResult {
        fn zeroed() -> Self {
            Self {
                inner: unsafe { std::mem::zeroed() },
            }
        }

        fn raw_mut(&mut self) -> *mut ffi::duckdb_result {
            std::ptr::addr_of_mut!(self.inner)
        }
    }

    impl Drop for QueryResult {
        fn drop(&mut self) {
            unsafe { ffi::duckdb_destroy_result(self.raw_mut()) };
        }
    }

    unsafe fn read_column_string(
        result: *mut ffi::duckdb_result,
        col: ffi::idx_t,
        row: ffi::idx_t,
    ) -> Option<String> {
        let ptr = ffi::duckdb_value_varchar(result, col, row);
        if ptr.is_null() {
            return None;
        }
        let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        ffi::duckdb_free(ptr.cast::<c_void>());
        Some(s)
    }

    unsafe fn result_error_message(result: *mut ffi::duckdb_result, fallback: &str) -> String {
        let err_ptr = ffi::duckdb_result_error(result);
        if err_ptr.is_null() {
            fallback.to_string()
        } else {
            CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
        }
    }

    unsafe fn prepared_lookup(
        conn: ffi::duckdb_connection,
        name: &str,
    ) -> Result<Option<String>, String> {
        let c_sql = CString::new(format!(
            "SELECT definition FROM {DEFINITIONS_TABLE} WHERE name = $1"
        ))
        .map_err(|_| "SQL contains null byte".to_string())?;
        let stmt = PreparedStmt::prepare(conn, &c_sql)?;

        let c_name = CString::new(name).map_err(|_| "view name contains null byte".to_string())?;
        if ffi::duckdb_bind_varchar(stmt.raw(), 1, c_name.as_ptr()) != ffi::DuckDBSuccess {
            return Err("failed to bind view name".to_string());
        }

        let mut result = QueryResult::zeroed();
        let exec_rc = ffi::duckdb_execute_prepared(stmt.raw(), result.raw_mut());
        if exec_rc != ffi::DuckDBSuccess {
            return Err(result_error_message(
                result.raw_mut(),
                "catalog lookup failed",
            ));
        }

        let row_count = ffi::duckdb_row_count(result.raw_mut());
        if row_count == 0 {
            Ok(None)
        } else {
            Ok(read_column_string(result.raw_mut(), 0, 0))
        }
    }

    unsafe fn execute_list_all(
        conn: ffi::duckdb_connection,
    ) -> Result<Vec<(String, String)>, String> {
        let c_sql = CString::new(format!(
            "SELECT name, definition FROM {DEFINITIONS_TABLE} ORDER BY name"
        ))
        .map_err(|_| "SQL contains null byte".to_string())?;
        let mut result = QueryResult::zeroed();
        let rc = ffi::duckdb_query(conn, c_sql.as_ptr(), result.raw_mut());
        if rc != ffi::DuckDBSuccess {
            return Err(result_error_message(
                result.raw_mut(),
                "catalog list failed",
            ));
        }

        let row_count = ffi::duckdb_row_count(result.raw_mut());
        let mut out = Vec::with_capacity(row_count as usize);
        for r in 0..row_count {
            let name = read_column_string(result.raw_mut(), 0, r).unwrap_or_default();
            let def = read_column_string(result.raw_mut(), 1, r).unwrap_or_default();
            out.push((name, def));
        }
        Ok(out)
    }

    /// Names-only counterpart to `execute_list_all`. Skips the `definition`
    /// column so error-path suggestion lookups don't pay for the JSON blobs.
    unsafe fn execute_list_names(conn: ffi::duckdb_connection) -> Result<Vec<String>, String> {
        let c_sql = CString::new(format!(
            "SELECT name FROM {DEFINITIONS_TABLE} ORDER BY name"
        ))
        .map_err(|_| "SQL contains null byte".to_string())?;
        let mut result = QueryResult::zeroed();
        let rc = ffi::duckdb_query(conn, c_sql.as_ptr(), result.raw_mut());
        if rc != ffi::DuckDBSuccess {
            return Err(result_error_message(
                result.raw_mut(),
                "catalog list failed",
            ));
        }

        let row_count = ffi::duckdb_row_count(result.raw_mut());
        let mut out = Vec::with_capacity(row_count as usize);
        for r in 0..row_count {
            let name = read_column_string(result.raw_mut(), 0, r).unwrap_or_default();
            out.push(name);
        }
        Ok(out)
    }
}

#[cfg(feature = "extension")]
pub use reader::CatalogReader;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "extension"))]
    use duckdb::Connection;

    #[test]
    fn definitions_table_const_is_consistent() {
        assert_eq!(
            DEFINITIONS_TABLE,
            format!("{DEFINITIONS_SCHEMA}.{DEFINITIONS_TABLE_NAME}"),
            "DEFINITIONS_TABLE must equal DEFINITIONS_SCHEMA.DEFINITIONS_TABLE_NAME"
        );
    }

    #[test]
    fn view_not_found_msg_wording() {
        assert_eq!(
            view_not_found_msg("sales"),
            "semantic view 'sales' does not exist"
        );
    }

    // In-memory `Connection` requires the bundled DuckDB API; the `extension`
    // feature swaps in `loadable-extension` stubs that error at runtime with
    // "DuckDB API not initialized or DuckDB feature omitted". Tests that need
    // an actual in-process DB are gated `not(feature = "extension")`; the
    // CatalogReader short-circuit tests (which never touch the DB) below
    // run under the `extension` feature.
    #[cfg(not(feature = "extension"))]
    fn in_memory_con() -> Connection {
        Connection::open_in_memory().expect("in-memory DuckDB")
    }

    // TEMPORARY: smoke test for v0.8.0 race-guard SQL shape.
    // CTE+DML+RETURNING is NOT supported in DuckDB v1.5.2 ("Parser Error:
    // A CTE needs a SELECT"), so we emit two statements separated by `;`:
    // a guard SELECT that raises via error() if the row is missing, then
    // the DELETE/UPDATE itself. Both statements run on the caller's
    // connection; they share one snapshot only within an explicit caller
    // transaction — under autocommit each auto-commits separately (FF-1 /
    // TECH-DEBT #27). This single-connection smoke test exercises the SQL
    // shape, not the concurrency window.
    #[cfg(not(feature = "extension"))]
    #[test]
    fn two_statement_guard_then_dml_smoke() {
        let con = in_memory_con();
        con.execute_batch(
            "CREATE TABLE t (name VARCHAR PRIMARY KEY, val INTEGER); \
             INSERT INTO t VALUES ('a', 1), ('b', 2), ('c', 3);",
        )
        .unwrap();

        // 1. Guard fires when row missing.
        let err = con
            .execute_batch(
                "SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM t WHERE name = 'nonexistent') \
                              THEN error('not found') \
                              ELSE TRUE END; \
                 DELETE FROM t WHERE name = 'nonexistent' RETURNING name;",
            )
            .err()
            .expect("missing-row guard must error");
        assert!(format!("{err}").contains("not found"), "unexpected: {err}");

        // Row 'a' must NOT have been deleted (guard aborted before DELETE).
        let count: i64 = con
            .query_row("SELECT count(*) FROM t WHERE name = 'a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // 2. Guard passes; DELETE runs and the user-visible result is the
        // RETURNING from the last statement.
        let mut stmt = con
            .prepare(
                "SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM t WHERE name = 'a') \
                              THEN error('not found') \
                              ELSE TRUE END; \
                 DELETE FROM t WHERE name = 'a' RETURNING name AS view_name;",
            )
            .expect("multi-statement parse");
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(names, vec!["a".to_string()]);

        // 3. UPDATE variant.
        let mut stmt = con
            .prepare(
                "SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM t WHERE name = 'b') \
                              THEN error('not found') \
                              ELSE TRUE END; \
                 UPDATE t SET val = 99 WHERE name = 'b' RETURNING name AS view_name;",
            )
            .expect("UPDATE guard parse");
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(names, vec!["b".to_string()]);
    }

    // AR-4: the schema_version upgrade pass stamps verifiable current-format
    // rows and leaves un-upgradeable legacy rows at version 0.
    #[cfg(not(feature = "extension"))]
    #[test]
    fn upgrade_stamps_complete_rows_and_skips_incomplete() {
        use crate::model::{SemanticViewDefinition, CURRENT_SCHEMA_VERSION};
        let con = in_memory_con();
        init_catalog(&con, ":memory:", false).unwrap();

        // Complete row: every relationship carries fk_columns, no schema_version.
        let complete = r#"{"tables":[{"alias":"o","table":"orders","pk_columns":["id"]},
            {"alias":"c","table":"customers","pk_columns":["id"]}],
            "dimensions":[],"metrics":[],
            "joins":[{"table":"c","from_alias":"o","fk_columns":["cid"],"ref_columns":["id"]}]}"#;
        // Incomplete row: a relationship in the legacy `on`-only encoding (no fk_columns).
        let incomplete = r#"{"tables":[{"alias":"o","table":"orders"}],
            "dimensions":[],"metrics":[],
            "joins":[{"table":"c","on":"o.cid = c.id"}]}"#;
        // A no-join single-table view: trivially complete, should be stamped.
        let single = r#"{"tables":[{"alias":"o","table":"orders"}],"dimensions":[],"metrics":[]}"#;

        for (name, def) in [
            ("complete_v", complete),
            ("incomplete_v", incomplete),
            ("single_v", single),
        ] {
            con.execute(
                "INSERT INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
                duckdb::params![name, def],
            )
            .unwrap();
        }

        // Re-run init_catalog: triggers the one-time upgrade pass.
        init_catalog(&con, ":memory:", false).unwrap();

        let stored = |name: &str| -> String {
            con.query_row(
                "SELECT definition FROM semantic_layer._definitions WHERE name = ?",
                duckdb::params![name],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(
            SemanticViewDefinition::stored_schema_version(&stored("complete_v")),
            CURRENT_SCHEMA_VERSION,
            "complete row must be stamped current"
        );
        assert_eq!(
            SemanticViewDefinition::stored_schema_version(&stored("single_v")),
            CURRENT_SCHEMA_VERSION,
            "no-join view is trivially complete and must be stamped"
        );
        assert_eq!(
            SemanticViewDefinition::stored_schema_version(&stored("incomplete_v")),
            0,
            "un-upgradeable legacy row must be left at version 0"
        );

        // Idempotent: a second pass changes nothing and does not error.
        init_catalog(&con, ":memory:", false).unwrap();
        assert_eq!(
            SemanticViewDefinition::stored_schema_version(&stored("complete_v")),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[cfg(not(feature = "extension"))]
    #[test]
    fn init_catalog_creates_schema_and_table() {
        let con = in_memory_con();
        init_catalog(&con, ":memory:", false).unwrap();
        // Idempotent: second call must not error
        init_catalog(&con, ":memory:", false).unwrap();

        let count: i64 = con
            .query_row(
                "SELECT count(*) FROM semantic_layer._definitions",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[cfg(not(feature = "extension"))]
    #[test]
    fn migration_corrupt_companion_file_errors_and_survives() {
        // MS-2 (code-review 2026-07-02): pre-fix, an unreadable or corrupt
        // v0.1.0 companion file was silently DELETED without importing —
        // permanent loss of pre-v0.2 definitions. Post-fix init_catalog must
        // error, and the file must be left in place for the user to repair.
        let tmp = std::env::temp_dir();
        let db_path_buf = tmp.join("test_ms2_corrupt.duckdb");
        let db_path = db_path_buf.to_str().expect("temp dir is UTF-8");
        let companion = tmp.join("test_ms2_corrupt.duckdb.semantic_views");
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(&companion);

        std::fs::write(&companion, "{not valid json").unwrap();
        let con = Connection::open(db_path).expect("open file-backed DB");
        let err = init_catalog(&con, db_path, false)
            .expect_err("corrupt companion file must fail the load");
        assert!(
            err.to_string().contains("not valid JSON"),
            "error should name the problem, got: {err}"
        );
        assert!(
            companion.exists(),
            "corrupt companion file must be left in place, not deleted"
        );

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(&companion);
    }

    #[cfg(not(feature = "extension"))]
    #[test]
    fn migration_valid_companion_file_imports_then_deletes() {
        let tmp = std::env::temp_dir();
        let db_path_buf = tmp.join("test_ms2_valid.duckdb");
        let db_path = db_path_buf.to_str().expect("temp dir is UTF-8");
        let companion = tmp.join("test_ms2_valid.duckdb.semantic_views");
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(&companion);

        std::fs::write(
            &companion,
            r#"{"orders": "{\"base_table\":\"orders\",\"dimensions\":[],\"metrics\":[]}"}"#,
        )
        .unwrap();
        let con = Connection::open(db_path).expect("open file-backed DB");
        init_catalog(&con, db_path, false).expect("valid companion file imports cleanly");

        let count: i64 = con
            .query_row(
                "SELECT count(*) FROM semantic_layer._definitions WHERE name = 'orders'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "companion definition should be imported");
        assert!(
            !companion.exists(),
            "companion file should be deleted after a successful import"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[cfg(not(feature = "extension"))]
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

    #[cfg(not(feature = "extension"))]
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

    #[cfg(not(feature = "extension"))]
    #[test]
    fn persist_02_rollback_leaves_catalog_unchanged() {
        let tmp = std::env::temp_dir();
        let db_path_buf = tmp.join("test_persist02_rollback.duckdb");
        let db_path = db_path_buf.to_str().expect("temp dir is UTF-8");
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));

        let con = Connection::open(db_path).expect("open file-backed DB");
        init_catalog(&con, db_path, false).unwrap();

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

    /// TC-6: restart persistence. Open a file-backed DB, persist a definition,
    /// close the connection entirely, reopen from disk, run `init_catalog`
    /// against the now-populated catalog, and confirm the definition survives.
    ///
    /// This is the open→persist→drop→reopen→lookup coverage the Makefile /
    /// `_excluded` test headers claimed "verified via cargo test Rust
    /// integration tests" — before this test, only ROLLBACK (persist_02) was
    /// exercised, not durability across a fresh connection. The key invariant
    /// is that `init_catalog` on an existing catalog is idempotent: it must
    /// `CREATE ... IF NOT EXISTS` and leave stored rows intact, not re-create
    /// or truncate the table.
    #[cfg(not(feature = "extension"))]
    #[test]
    fn tc6_restart_persistence_survives_reopen() {
        // Unique per-invocation filename (pid + nanos) so concurrent `cargo test`
        // runs — in this binary or another process — cannot race on the same
        // DB/WAL paths and flake via cross-deletion/reuse.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = std::env::temp_dir();
        let db_path_buf = tmp.join(format!(
            "test_tc6_restart_persistence_{}_{nanos}.duckdb",
            std::process::id()
        ));
        let db_path = db_path_buf.to_str().expect("temp dir is UTF-8");
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));

        let json = r#"{"schema_version":1,"base_table":"sales","dimensions":[],"metrics":[]}"#;

        // --- Session 1: create the catalog and persist a definition. ---
        {
            let con = Connection::open(db_path).expect("open file-backed DB (session 1)");
            init_catalog(&con, db_path, false).unwrap();
            con.execute(
                "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
                duckdb::params!["sales", json],
            )
            .unwrap();
            // Force the write to the file so a fresh open sees it.
            con.execute_batch("CHECKPOINT;").unwrap();
            drop(con);
        }

        // --- Session 2: reopen from disk; init_catalog must not clobber. ---
        {
            let con = Connection::open(db_path).expect("reopen file-backed DB (session 2)");
            // Re-running init_catalog against a populated catalog is the exact
            // path taken on every LOAD of an existing database; it must be a
            // no-op for stored rows.
            init_catalog(&con, db_path, false).unwrap();

            let stored: String = con
                .query_row(
                    "SELECT definition FROM semantic_layer._definitions WHERE name = 'sales'",
                    [],
                    |row| row.get(0),
                )
                .expect("definition must survive reopen + init_catalog");
            assert_eq!(
                stored, json,
                "reopened definition must be byte-identical to what was persisted"
            );

            let count: i64 = con
                .query_row(
                    "SELECT count(*) FROM semantic_layer._definitions",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "init_catalog on reopen must not add or drop rows");
            drop(con);
        }

        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));
    }

    // -----------------------------------------------------------------
    // Phase 63 (v0.9.0): read-only LOAD support — init_catalog
    // short-circuit + CatalogReader::{lookup,list_all,list_names}
    // short-circuits when catalog_table_present=false. See
    // 63-RESEARCH.md §3 Q2/Q3/Q4.
    // -----------------------------------------------------------------

    #[cfg(not(feature = "extension"))]
    #[test]
    fn init_catalog_skips_writes_on_readonly() {
        // Phase 63 (RO-01): is_read_only=true must early-return without
        // creating the schema. Verified by checking information_schema.
        let con = in_memory_con();
        init_catalog(&con, ":memory:", true).unwrap();
        let count: i64 = con
            .query_row(
                "SELECT count(*) FROM information_schema.schemata WHERE schema_name = 'semantic_layer'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "init_catalog with is_read_only=true must NOT create the semantic_layer schema"
        );
    }

    #[cfg(not(feature = "extension"))]
    #[test]
    fn init_catalog_writes_when_writable() {
        // Sibling: writable path still creates the schema + table.
        let con = in_memory_con();
        init_catalog(&con, ":memory:", false).unwrap();
        let count: i64 = con
            .query_row(
                "SELECT count(*) FROM semantic_layer._definitions",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "writable init_catalog must create empty _definitions table"
        );
    }

    #[cfg(feature = "extension")]
    #[test]
    fn lookup_returns_none_when_table_missing() {
        // Phase 63 (RO-04): catalog_table_present=false must short-circuit
        // BEFORE hitting the DB. Use std::ptr::null_mut() for the conn —
        // the short-circuit must return Ok(None) before any FFI call, so
        // the null pointer is never dereferenced.
        use crate::catalog::CatalogReader;
        use crate::ddl::read_ffi::BorrowedConnection;
        let borrowed = unsafe { BorrowedConnection::new(std::ptr::null_mut()) };
        let reader = CatalogReader::new(&borrowed, false);
        let result = reader.lookup("any_view");
        assert!(
            matches!(result, Ok(None)),
            "expected Ok(None), got: {:?}",
            result
        );
    }

    #[cfg(feature = "extension")]
    #[test]
    fn list_all_returns_empty_when_table_missing() {
        // Phase 63 (RO-03): catalog_table_present=false must short-circuit.
        use crate::catalog::CatalogReader;
        use crate::ddl::read_ffi::BorrowedConnection;
        let borrowed = unsafe { BorrowedConnection::new(std::ptr::null_mut()) };
        let reader = CatalogReader::new(&borrowed, false);
        let result = reader.list_all();
        assert!(
            matches!(result, Ok(ref v) if v.is_empty()),
            "expected Ok(empty), got: {:?}",
            result
        );
    }

    #[cfg(feature = "extension")]
    #[test]
    fn list_names_returns_empty_when_table_missing() {
        // Phase 63 (RO-03): catalog_table_present=false must short-circuit.
        use crate::catalog::CatalogReader;
        use crate::ddl::read_ffi::BorrowedConnection;
        let borrowed = unsafe { BorrowedConnection::new(std::ptr::null_mut()) };
        let reader = CatalogReader::new(&borrowed, false);
        let result = reader.list_names();
        assert!(
            matches!(result, Ok(ref v) if v.is_empty()),
            "expected Ok(empty), got: {:?}",
            result
        );
    }
}
