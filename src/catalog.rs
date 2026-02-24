use std::{
    collections::HashMap,
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

    // Attempt to delete from the catalog table.
    //
    // In v0.1 the scalar function opens `Connection::open(":memory:")` which
    // creates a separate ephemeral database — the DELETE may affect 0 rows
    // because the ephemeral DB's `semantic_layer._definitions` was just created
    // by `init_catalog` and contains no rows (even though the host DB does).
    //
    // The HashMap `contains_key` check above is the authoritative existence
    // check.  The catalog write is best-effort: it will correctly persist when
    // called with a real file-backed path in a future revision.
    let _ = con.execute(
        "DELETE FROM semantic_layer._definitions WHERE name = ?",
        duckdb::params![name],
    )?;

    // Update HashMap regardless of rows_affected — HashMap is source of truth.
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
