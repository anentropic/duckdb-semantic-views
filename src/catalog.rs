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

/// Create the `semantic_layer` schema and `_definitions` table if they do not exist,
/// then load all existing rows into a new [`CatalogState`].
///
/// If `db_path` points to a file-backed database, this also reads the sidecar file
/// (written by `invoke` during define/drop) and merges its contents into the table
/// and `HashMap`.  The sidecar is the source of truth for cross-restart persistence
/// because `invoke` cannot execute `DuckDB` SQL (deadlock) but can write plain files.
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

    // Merge sidecar data (sidecar wins on conflict — it reflects the latest state
    // from the most recent session).
    if db_path != ":memory:" {
        let sidecar = read_sidecar(db_path);
        if !sidecar.is_empty() {
            // Replace table contents with merged state.
            // Sidecar is authoritative: it was written atomically at each
            // define/drop, so it reflects the final session state.
            map = sidecar;
            sync_table_from_map(con, &map)?;
        }
    }

    Ok(Arc::new(RwLock::new(map)))
}

/// Replace `semantic_layer._definitions` contents with the given map.
///
/// Called during `init_catalog` to sync sidecar data into the `DuckDB` table.
fn sync_table_from_map(con: &Connection, map: &HashMap<String, String>) -> Result<()> {
    con.execute_batch("DELETE FROM semantic_layer._definitions")?;
    let mut stmt =
        con.prepare("INSERT INTO semantic_layer._definitions (name, definition) VALUES (?, ?)")?;
    for (name, def) in map {
        stmt.execute(duckdb::params![name, def])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sidecar file persistence
//
// DuckDB holds internal execution locks during scalar function `invoke`.  Any
// SQL executed from within invoke — whether on the same connection, a cloned
// connection (`try_clone`), or a separate `Connection::open(path)` — deadlocks
// or blocks on file-level locks.
//
// The sidecar approach avoids DuckDB SQL entirely during invoke: the HashMap is
// serialized as JSON to a companion file (`<db_path>.semantic_views`) using
// plain filesystem I/O, which is not subject to DuckDB locks.
//
// On the next extension load, `init_catalog` reads the sidecar and syncs its
// contents into the DuckDB table, making definitions queryable via SQL and
// ensuring they survive subsequent restarts even if the sidecar is lost.
// ---------------------------------------------------------------------------

/// Derive the sidecar file path from the database path.
///
/// For `/path/to/mydb.duckdb`, returns `/path/to/mydb.duckdb.semantic_views`.
fn sidecar_path(db_path: &str) -> PathBuf {
    let mut p = PathBuf::from(db_path);
    let ext = match p.extension() {
        Some(e) => format!("{}.semantic_views", e.to_string_lossy()),
        None => "semantic_views".to_string(),
    };
    p.set_extension(ext);
    p
}

/// Read definitions from the sidecar file.
///
/// Returns an empty map if the file does not exist or cannot be parsed.
fn read_sidecar(db_path: &str) -> HashMap<String, String> {
    let path = sidecar_path(db_path);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Atomically write the current catalog state to the sidecar file.
///
/// Writes to a temporary file first, then renames — this is atomic on POSIX
/// systems and prevents partial writes from corrupting the sidecar.
pub fn write_sidecar(
    db_path: &str,
    state: &CatalogState,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let path = sidecar_path(db_path);
    let tmp = path.with_extension("tmp");
    let guard = state.read().unwrap();
    let json = serde_json::to_string(&*guard)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Write a new semantic view definition to the in-memory catalog.
///
/// Returns an error if:
/// - The JSON is invalid
/// - A view with `name` already exists
///
/// For file-backed databases, the caller is responsible for calling
/// [`write_sidecar`] after this function to persist the change.
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
                "semantic view '{name}' already exists; call drop_semantic_view first"
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
///
/// For file-backed databases, the caller is responsible for calling
/// [`write_sidecar`] after this function to persist the change.
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
        let tmpfile = "/tmp/test_pragma_rust_check.duckdb";
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
    fn sidecar_path_derivation() {
        assert_eq!(
            sidecar_path("/tmp/test.duckdb"),
            PathBuf::from("/tmp/test.duckdb.semantic_views")
        );
        assert_eq!(
            sidecar_path("/tmp/test.db"),
            PathBuf::from("/tmp/test.db.semantic_views")
        );
        assert_eq!(
            sidecar_path("/tmp/test"),
            PathBuf::from("/tmp/test.semantic_views")
        );
    }

    #[test]
    fn sidecar_round_trip() {
        let db_path = "/tmp/test_sidecar_roundtrip.duckdb";
        let sidecar = sidecar_path(db_path);
        // Clean up
        let _ = std::fs::remove_file(&sidecar);
        let _ = std::fs::remove_file(sidecar.with_extension("tmp"));

        let con = in_memory_con();
        let state = init_catalog(&con, ":memory:").unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&state, "orders", json).unwrap();

        // Write sidecar
        write_sidecar(db_path, &state).unwrap();

        // Read it back
        let loaded = read_sidecar(db_path);
        assert_eq!(loaded.get("orders").map(String::as_str), Some(json));

        // Clean up
        let _ = std::fs::remove_file(&sidecar);
    }

    #[test]
    fn init_catalog_loads_from_sidecar() {
        let db_path = "/tmp/test_init_sidecar.duckdb";
        let sidecar = sidecar_path(db_path);
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));
        let _ = std::fs::remove_file(&sidecar);

        // Simulate a previous session: write a sidecar with one definition
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        let mut prev = HashMap::new();
        prev.insert("orders".to_string(), json.to_string());
        let sidecar_json = serde_json::to_string(&prev).unwrap();
        std::fs::write(&sidecar, sidecar_json).unwrap();

        // Open a file-backed DB and init_catalog — should pick up the sidecar
        let con = Connection::open(db_path).expect("open file-backed DB");
        let state = init_catalog(&con, db_path).unwrap();
        assert!(state.read().unwrap().contains_key("orders"));

        // Verify the table was also synced
        let mut stmt = con
            .prepare("SELECT name FROM semantic_layer._definitions")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(names.contains(&"orders".to_string()));

        // Clean up
        drop(stmt);
        drop(con);
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{db_path}.wal"));
        let _ = std::fs::remove_file(&sidecar);
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
}
