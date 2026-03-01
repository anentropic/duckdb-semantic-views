use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use duckdb::{Connection, Result};

use crate::model::SemanticViewDefinition;

/// Shared in-memory cache of semantic view definitions.
/// Key: view name. Value: raw JSON string of the definition.
pub type CatalogState = Arc<RwLock<HashMap<String, String>>>;

// Extension appended to the DuckDB file path to form the v0.1.0 companion file.
// Used only in the one-time migration below. After the migration runs, the
// companion file is deleted and this constant is never referenced again at runtime.
const V010_COMPANION_EXT: &str = "semantic_views";

/// Create the `semantic_layer` schema and `_definitions` table if they do not exist,
/// then load all existing rows into a new [`CatalogState`].
///
/// For file-backed databases, performs a one-time migration: if a v0.1.0 companion
/// file exists alongside the database, its contents are imported into the table
/// and the file is deleted. After the migration runs once, the companion file is
/// gone and this block is a no-op on subsequent loads.
///
/// This function is called once at extension load time. It is idempotent: safe to call
/// on every extension load regardless of whether the catalog already exists.
pub fn init_catalog(con: &Connection, db_path: &str) -> Result<CatalogState> {
    con.execute_batch(
        "CREATE SCHEMA IF NOT EXISTS semantic_layer;
         CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
             name       VARCHAR PRIMARY KEY,
             definition VARCHAR
         );",
    )?;

    // Read existing rows from the DuckDB table.
    let mut map = HashMap::new();
    let mut stmt = con.prepare("SELECT name, definition FROM semantic_layer._definitions")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name, def) = row?;
        map.insert(name, def);
    }

    // One-time migration: if a v0.1.0 companion file exists alongside the database,
    // import its contents into the table then delete the file.
    // After this migration runs once, the companion file is gone and this block
    // is a no-op on subsequent loads (file absent → skip silently).
    if db_path != ":memory:" {
        // Derive the companion file path: <db_path>.<ext>.<V010_COMPANION_EXT>
        // e.g. /path/to/mydb.duckdb → /path/to/mydb.duckdb.<companion>
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
            // Read v0.1.0 definitions from the companion file
            if let Ok(contents) = std::fs::read_to_string(&migration_path) {
                if let Ok(migrated) = serde_json::from_str::<HashMap<String, String>>(&contents) {
                    for (name, def) in &migrated {
                        // INSERT OR REPLACE: companion file wins on conflict (latest session state)
                        con.execute(
                            "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
                            duckdb::params![name, def],
                        )?;
                        // Also update the in-memory map
                        map.insert(name.clone(), def.clone());
                    }
                }
            }
            // Delete the companion file regardless of whether it had data.
            // Ignore errors (read-only filesystem, race condition, etc.)
            let _ = std::fs::remove_file(&migration_path);
        }
    }

    Ok(Arc::new(RwLock::new(map)))
}

/// Write a new semantic view definition to the in-memory catalog.
///
/// Returns an error if:
/// - The JSON is invalid
/// - A view with `name` already exists
pub fn catalog_insert(
    state: &CatalogState,
    name: &str,
    json: &str,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Validate JSON before writing — fail fast, nothing written on invalid input
    SemanticViewDefinition::from_json(name, json).map_err(Box::<dyn std::error::Error>::from)?;

    // Check for duplicate before modifying state
    {
        let guard = state.read().unwrap();
        if guard.contains_key(name) {
            return Err(format!(
                "semantic view '{name}' already exists; use CREATE OR REPLACE SEMANTIC VIEW to overwrite"
            )
            .into());
        }
    }

    state
        .write()
        .unwrap()
        .insert(name.to_string(), json.to_string());
    Ok(())
}

/// Remove a semantic view definition from the in-memory catalog.
///
/// Returns an error if no view with `name` exists.
pub fn catalog_delete(
    state: &CatalogState,
    name: &str,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    {
        let guard = state.read().unwrap();
        if !guard.contains_key(name) {
            return Err(format!("semantic view '{name}' does not exist").into());
        }
    }

    state.write().unwrap().remove(name);
    Ok(())
}

/// Write or overwrite a semantic view definition in the in-memory catalog.
///
/// Unlike `catalog_insert`, this does not error on duplicates — it replaces
/// any existing definition for `name`.
///
/// Returns an error if the JSON is invalid.
pub fn catalog_upsert(
    state: &CatalogState,
    name: &str,
    json: &str,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    SemanticViewDefinition::from_json(name, json).map_err(Box::<dyn std::error::Error>::from)?;
    state
        .write()
        .unwrap()
        .insert(name.to_string(), json.to_string());
    Ok(())
}

/// Remove a semantic view definition from the in-memory catalog if it exists.
///
/// Unlike `catalog_delete`, this silently succeeds when the view does not exist.
pub fn catalog_delete_if_exists(state: &CatalogState, name: &str) {
    state.write().unwrap().remove(name);
}

/// FFI-callable catalog mutation functions — called from the C++ parser hook scan function.
///
/// These functions are gated on the `extension` feature so they are not included in
/// standalone test binaries (which cannot use the loadable-extension C API stubs).
///
/// All functions take an opaque `*const CatalogState` pointer (the Rust Arc<RwLock<HashMap>>)
/// and return 0 on success, -1 on error.
#[cfg(feature = "extension")]
mod ffi_catalog {
    use super::*;
    use std::ffi::c_char;

    unsafe fn str_from_ptr<'a>(ptr: *const c_char) -> Option<&'a str> {
        if ptr.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(ptr).to_str().ok()
    }

    /// Insert a new semantic view into the in-memory catalog.
    /// Returns 0 on success, -1 if pointer null / json invalid / duplicate.
    #[no_mangle]
    pub unsafe extern "C" fn semantic_views_catalog_insert(
        catalog_ptr: *const CatalogState,
        name_ptr: *const c_char,
        json_ptr: *const c_char,
    ) -> i32 {
        let Some(state) = catalog_ptr.as_ref() else {
            return -1;
        };
        let (Some(name), Some(json)) = (str_from_ptr(name_ptr), str_from_ptr(json_ptr)) else {
            return -1;
        };
        catalog_insert(state, name, json).map(|_| 0).unwrap_or(-1)
    }

    /// Upsert a semantic view in the in-memory catalog.
    /// Returns 0 on success, -1 if pointer null or json invalid.
    #[no_mangle]
    pub unsafe extern "C" fn semantic_views_catalog_upsert(
        catalog_ptr: *const CatalogState,
        name_ptr: *const c_char,
        json_ptr: *const c_char,
    ) -> i32 {
        let Some(state) = catalog_ptr.as_ref() else {
            return -1;
        };
        let (Some(name), Some(json)) = (str_from_ptr(name_ptr), str_from_ptr(json_ptr)) else {
            return -1;
        };
        catalog_upsert(state, name, json).map(|_| 0).unwrap_or(-1)
    }

    /// Delete a semantic view from the in-memory catalog.
    /// Returns 0 on success, -1 if not found.
    #[no_mangle]
    pub unsafe extern "C" fn semantic_views_catalog_delete(
        catalog_ptr: *const CatalogState,
        name_ptr: *const c_char,
    ) -> i32 {
        let Some(state) = catalog_ptr.as_ref() else {
            return -1;
        };
        let Some(name) = str_from_ptr(name_ptr) else {
            return -1;
        };
        catalog_delete(state, name).map(|_| 0).unwrap_or(-1)
    }

    /// Delete a semantic view if it exists; silently succeeds if absent.
    /// Returns 0 always (unless null pointer, which returns -1).
    #[no_mangle]
    pub unsafe extern "C" fn semantic_views_catalog_delete_if_exists(
        catalog_ptr: *const CatalogState,
        name_ptr: *const c_char,
    ) -> i32 {
        let Some(state) = catalog_ptr.as_ref() else {
            return -1;
        };
        let Some(name) = str_from_ptr(name_ptr) else {
            return -1;
        };
        catalog_delete_if_exists(state, name);
        0
    }
}

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
        let state = init_catalog(&con, ":memory:").unwrap();
        assert!(state.read().unwrap().is_empty());
        // Idempotent: second call must not error
        let state2 = init_catalog(&con, ":memory:").unwrap();
        assert!(state2.read().unwrap().is_empty());
    }

    #[test]
    fn insert_and_retrieve() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&state, "orders", json).unwrap();
        let guard = state.read().unwrap();
        assert_eq!(guard.get("orders").map(String::as_str), Some(json));
    }

    #[test]
    fn duplicate_insert_is_error() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&state, "orders", json).unwrap();
        let result = catalog_insert(&state, "orders", json);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already exists"), "unexpected: {msg}");
    }

    #[test]
    fn delete_removes_from_hashmap() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&state, "orders", json).unwrap();
        catalog_delete(&state, "orders").unwrap();
        assert!(!state.read().unwrap().contains_key("orders"));
    }

    #[test]
    fn delete_nonexistent_is_error() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let result = catalog_delete(&state, "nonexistent");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("does not exist"), "unexpected: {msg}");
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
    fn init_catalog_loads_existing_rows() {
        // Simulate data already in the DuckDB table (no sidecar).
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        // Write directly to the table (simulating a previous entrypoint sync).
        con.execute(
            "INSERT INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
            duckdb::params!["orders", json],
        )
        .unwrap();
        // Second load: simulates restart — loads from catalog
        let state2 = init_catalog(&con, ":memory:").unwrap();
        assert!(state2.read().unwrap().contains_key("orders"));
        drop(state);
    }

    #[test]
    fn upsert_inserts_when_absent() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_upsert(&state, "orders", json).unwrap();
        let guard = state.read().unwrap();
        assert_eq!(guard.get("orders").map(String::as_str), Some(json));
    }

    #[test]
    fn upsert_replaces_when_present() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json1 = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        let json2 = r#"{"base_table":"orders","dimensions":[{"name":"region","expr":"region"}],"metrics":[]}"#;
        catalog_upsert(&state, "orders", json1).unwrap();
        catalog_upsert(&state, "orders", json2).unwrap();
        let guard = state.read().unwrap();
        assert_eq!(guard.get("orders").map(String::as_str), Some(json2));
    }

    #[test]
    fn upsert_rejects_invalid_json() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let result = catalog_upsert(&state, "orders", "{invalid json}");
        assert!(result.is_err());
        // Catalog must remain unchanged
        assert!(!state.read().unwrap().contains_key("orders"));
    }

    #[test]
    fn delete_if_exists_removes() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&state, "orders", json).unwrap();
        catalog_delete_if_exists(&state, "orders");
        assert!(!state.read().unwrap().contains_key("orders"));
    }

    #[test]
    fn delete_if_exists_silent_when_absent() {
        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        // Should not panic or error
        catalog_delete_if_exists(&state, "nonexistent");
        assert!(state.read().unwrap().is_empty());
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
