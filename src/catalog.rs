use std::{
    collections::HashMap,
    sync::{mpsc, Arc, RwLock},
    thread,
};

use duckdb::{Connection, Result};

use crate::model::SemanticViewDefinition;

/// Shared in-memory cache of semantic view definitions.
/// Key: view name. Value: raw JSON string of the definition.
pub type CatalogState = Arc<RwLock<HashMap<String, String>>>;

/// Create the `semantic_layer` schema and `_definitions` table if they do not exist,
/// then load all existing rows into a new [`CatalogState`].
///
/// This function is called once at extension load time. It is idempotent: safe to call
/// on every extension load regardless of whether the catalog already exists.
pub fn init_catalog(con: &Connection) -> Result<CatalogState> {
    con.execute_batch(
        "CREATE SCHEMA IF NOT EXISTS semantic_layer;
         CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
             name       VARCHAR PRIMARY KEY,
             definition VARCHAR
         );",
    )?;

    let mut map = HashMap::new();
    let mut stmt = con.prepare("SELECT name, definition FROM semantic_layer._definitions")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name, def) = row?;
        map.insert(name, def);
    }

    Ok(Arc::new(RwLock::new(map)))
}

/// Write a new semantic view definition to the catalog and update the in-memory cache.
///
/// Returns an error if:
/// - A view with `name` already exists (catalog PRIMARY KEY violation)
/// - The catalog write fails for any other reason
///
/// The `HashMap` is updated only on successful catalog write.
pub fn catalog_insert(
    con: &Connection,
    state: &CatalogState,
    name: &str,
    json: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate JSON before writing — fail fast, nothing written on invalid input
    SemanticViewDefinition::from_json(name, json).map_err(Box::<dyn std::error::Error>::from)?;

    // Check for duplicate before catalog write for a cleaner error message
    {
        let guard = state.read().unwrap();
        if guard.contains_key(name) {
            return Err(format!(
                "semantic view '{name}' already exists; call drop_semantic_view first"
            )
            .into());
        }
    }

    // Write to catalog first — error propagates via ? without touching HashMap
    con.execute(
        "INSERT INTO semantic_layer._definitions (name, definition) VALUES (?, ?)",
        duckdb::params![name, json],
    )?;

    // Update HashMap only on successful catalog write
    state
        .write()
        .unwrap()
        .insert(name.to_string(), json.to_string());
    Ok(())
}

/// Remove a semantic view definition from the catalog and the in-memory cache.
///
/// Returns an error if no view with `name` exists.
pub fn catalog_delete(
    con: &Connection,
    state: &CatalogState,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    {
        let guard = state.read().unwrap();
        if !guard.contains_key(name) {
            return Err(format!("semantic view '{name}' does not exist").into());
        }
    }

    let _ = con.execute(
        "DELETE FROM semantic_layer._definitions WHERE name = ?",
        duckdb::params![name],
    )?;

    // Update HashMap regardless of rows_affected — HashMap is source of truth.
    state.write().unwrap().remove(name);
    Ok(())
}

// ---------------------------------------------------------------------------
// Background catalog writer
// ---------------------------------------------------------------------------

/// A catalog write operation sent to the background writer thread.
enum CatalogOp {
    Insert {
        name: String,
        json: String,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    Delete {
        name: String,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
}

/// Handle to the background catalog writer thread.
///
/// The background thread owns a `Connection::open(db_path)` that is completely
/// separate from the host connection.  This lets it execute SQL while `DuckDB`
/// holds internal locks on the host connection during scalar function `invoke`.
///
/// Cloning this handle creates an additional `SyncSender` to the same channel.
/// The background thread exits when all senders are dropped (on extension unload).
#[derive(Clone)]
pub struct CatalogWriterHandle {
    sender: mpsc::SyncSender<CatalogOp>,
}

impl CatalogWriterHandle {
    /// Persist an INSERT to `semantic_layer._definitions`.
    ///
    /// Blocks until the background thread confirms the write is committed.
    pub fn insert(&self, name: &str, json: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if self
            .sender
            .send(CatalogOp::Insert {
                name: name.to_string(),
                json: json.to_string(),
                reply: reply_tx,
            })
            .is_err()
        {
            return Err("catalog writer thread has exited".into());
        }
        reply_rx
            .recv()
            .map_err(|_| -> Box<dyn std::error::Error> {
                "catalog writer thread has exited".into()
            })?
            .map_err(Into::into)
    }

    /// Persist a DELETE from `semantic_layer._definitions`.
    ///
    /// Blocks until the background thread confirms the write is committed.
    pub fn delete(&self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if self
            .sender
            .send(CatalogOp::Delete {
                name: name.to_string(),
                reply: reply_tx,
            })
            .is_err()
        {
            return Err("catalog writer thread has exited".into());
        }
        reply_rx
            .recv()
            .map_err(|_| -> Box<dyn std::error::Error> {
                "catalog writer thread has exited".into()
            })?
            .map_err(Into::into)
    }
}

/// Spawn a background thread to handle catalog writes to a file-backed `DuckDB`.
///
/// Returns `None` for in-memory databases — the in-memory [`CatalogState`] `HashMap`
/// is the sole source of truth for the session and no persistence is needed.
///
/// # Why a background thread?
///
/// `DuckDB` holds internal locks during scalar function `invoke`.  Any SQL executed
/// on the *same* database instance from within `invoke` (via `try_clone()` or
/// otherwise) will deadlock or spinlock waiting for those same locks.
///
/// The background thread opens its own `Connection::open(db_path)` — a completely
/// separate `DuckDB` connection via the WAL layer.  A file-backed `DuckDB` allows
/// concurrent connections: the parent query holds a read snapshot, and the
/// background connection runs its write transaction independently.
///
/// `invoke` sends the op over the channel and blocks on the reply, making the
/// write synchronous from the caller's perspective while avoiding lock contention.
#[must_use]
pub fn spawn_catalog_writer(db_path: &str) -> Option<CatalogWriterHandle> {
    if db_path == ":memory:" {
        return None;
    }

    let (sender, receiver) = mpsc::sync_channel::<CatalogOp>(128);
    let path = db_path.to_string();

    thread::spawn(move || {
        let Ok(con) = Connection::open(&path) else { return };
        // Ensure schema/table exist on the writer connection (idempotent — the
        // entrypoint already created them, but this guards against races).
        let _ = con.execute_batch(
            "CREATE SCHEMA IF NOT EXISTS semantic_layer;
             CREATE TABLE IF NOT EXISTS semantic_layer._definitions (
                 name       VARCHAR PRIMARY KEY,
                 definition VARCHAR
             );",
        );

        while let Ok(op) = receiver.recv() {
            match op {
                CatalogOp::Insert { name, json, reply } => {
                    let result = con
                        .execute(
                            "INSERT INTO semantic_layer._definitions \
                             (name, definition) VALUES (?, ?)",
                            duckdb::params![name, json],
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                    // Checkpoint after each write so the data is visible to any
                    // future connection that opens the same file (e.g. after restart).
                    // Ignore checkpoint failures — the INSERT is already committed.
                    let _ = con.execute_batch("CHECKPOINT");
                    let _ = reply.send(result);
                }
                CatalogOp::Delete { name, reply } => {
                    let result = con
                        .execute(
                            "DELETE FROM semantic_layer._definitions WHERE name = ?",
                            duckdb::params![name],
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                    let _ = con.execute_batch("CHECKPOINT");
                    let _ = reply.send(result);
                }
            }
        }
    });

    Some(CatalogWriterHandle { sender })
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
        let state = init_catalog(&con).unwrap();
        assert!(state.read().unwrap().is_empty());
        // Idempotent: second call must not error
        let state2 = init_catalog(&con).unwrap();
        assert!(state2.read().unwrap().is_empty());
    }

    #[test]
    fn insert_and_retrieve() {
        let con = in_memory_con();
        let state = init_catalog(&con).unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&con, &state, "orders", json).unwrap();
        let guard = state.read().unwrap();
        assert_eq!(guard.get("orders").map(String::as_str), Some(json));
    }

    #[test]
    fn duplicate_insert_is_error() {
        let con = in_memory_con();
        let state = init_catalog(&con).unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&con, &state, "orders", json).unwrap();
        let result = catalog_insert(&con, &state, "orders", json);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already exists"), "unexpected: {msg}");
    }

    #[test]
    fn delete_removes_from_hashmap_and_catalog() {
        let con = in_memory_con();
        let state = init_catalog(&con).unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&con, &state, "orders", json).unwrap();
        catalog_delete(&con, &state, "orders").unwrap();
        assert!(!state.read().unwrap().contains_key("orders"));
    }

    #[test]
    fn delete_nonexistent_is_error() {
        let con = in_memory_con();
        let state = init_catalog(&con).unwrap();
        let result = catalog_delete(&con, &state, "nonexistent");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("does not exist"), "unexpected: {msg}");
    }

    #[test]
    fn pragma_database_list_returns_file_path() {
        // Verify that PRAGMA database_list returns the file path for a file-backed DB.
        // This is used in lib.rs to resolve db_path for the background writer thread.
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
        // Verify that PRAGMA database_list returns no file path for in-memory DB.
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
        let con = in_memory_con();
        // First load: insert a row
        let state = init_catalog(&con).unwrap();
        let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
        catalog_insert(&con, &state, "orders", json).unwrap();
        // Second load: simulates restart — loads from catalog
        let state2 = init_catalog(&con).unwrap();
        assert!(state2.read().unwrap().contains_key("orders"));
    }
}
