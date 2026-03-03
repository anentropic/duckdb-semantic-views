use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;
use std::ffi::CString;

use crate::catalog::{catalog_insert, catalog_upsert, CatalogState};
use crate::ddl::parse_args::parse_define_args_from_bind;
use crate::expand::{expand, QueryRequest};

/// Shared state for `create_semantic_view` and `create_or_replace_semantic_view`.
///
/// `persist_conn` is `Some` for file-backed databases -- it is a separate
/// `duckdb_connection` created at init time and used to execute INSERT into
/// `semantic_layer._definitions` from within bind (avoids deadlock with
/// the main connection's execution lock). For in-memory databases,
/// `persist_conn` is `None` and the HashMap is the sole source of truth.
#[derive(Clone)]
pub struct DefineState {
    pub catalog: CatalogState,
    pub persist_conn: Option<ffi::duckdb_connection>,
    /// When true, uses INSERT OR REPLACE (upsert); when false, errors on duplicate.
    pub or_replace: bool,
    /// When true, silently succeeds (no-op) if the view already exists.
    /// Mutually exclusive with `or_replace` (or_replace takes precedence if both set).
    pub if_not_exists: bool,
}

// SAFETY: duckdb_connection is an opaque pointer managed by DuckDB.
// DuckDB handles concurrent access internally per connection.
unsafe impl Send for DefineState {}
unsafe impl Sync for DefineState {}

/// Persist a view definition to `semantic_layer._definitions` using the separate
/// persist_conn. This avoids deadlocking with the main connection's execution lock
/// (context_lock is non-reentrant; a second duckdb_connection has its own context).
///
/// Uses the Rust ffi::duckdb_query which goes through function pointers (loadable-
/// extension compatible) rather than direct symbol references.
///
/// Returns Ok(()) on success, Err on failure.
fn persist_define(conn: ffi::duckdb_connection, name: &str, json: &str) -> Result<(), String> {
    // Escape single quotes to prevent SQL injection / breakage
    let safe_name = name.replace('\'', "''");
    let safe_json = json.replace('\'', "''");
    let sql = format!(
        "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES ('{}', '{}')",
        safe_name, safe_json
    );
    let c_sql = CString::new(sql).map_err(|_| "SQL contains null byte".to_string())?;
    unsafe {
        let mut result: ffi::duckdb_result = std::mem::zeroed();
        let state = ffi::duckdb_query(conn, c_sql.as_ptr(), &mut result);
        let success = state == ffi::DuckDBSuccess;
        ffi::duckdb_destroy_result(&mut result);
        if success {
            Ok(())
        } else {
            Err(format!(
                "failed to persist semantic view '{name}' to catalog table"
            ))
        }
    }
}

/// Bind-time data for the DDL define table function.
///
/// Holds the view name (returned as the single result row).
pub struct DefineBindData {
    name: String,
}

// SAFETY: String is Send + Sync.
unsafe impl Send for DefineBindData {}
unsafe impl Sync for DefineBindData {}

/// Init data for the DDL define table function.
pub struct DefineInitData {
    done: AtomicBool,
}

// SAFETY: AtomicBool is Send + Sync.
unsafe impl Send for DefineInitData {}
unsafe impl Sync for DefineInitData {}

/// `create_semantic_view(name, tables, relationships, dimensions, time_dimensions, metrics)`
/// table function.
///
/// Inserts a new semantic view definition. Errors if the view already exists.
/// Use `create_or_replace_semantic_view` to overwrite an existing view.
///
/// Supports both positional and keyword argument syntax:
/// ```sql
/// -- Positional:
/// FROM create_semantic_view('name', [...tables], [...rels], [...dims], [...tdims], [...metrics])
/// -- Keyword:
/// FROM create_semantic_view('name', tables := [...], dimensions := [...], metrics := [...])
/// ```
pub struct DefineSemanticViewVTab;

impl VTab for DefineSemanticViewVTab {
    type BindData = DefineBindData;
    type InitData = DefineInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // Declare output schema: single VARCHAR column with the view name.
        bind.add_result_column("view_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));

        // Parse all 6 arguments (name + 5 LIST(STRUCT) params).
        let mut parsed =
            parse_define_args_from_bind(bind).map_err(|e| Box::<dyn std::error::Error>::from(e))?;

        // Access the DefineState from extra_info.
        let state_ptr = bind.get_extra_info::<DefineState>();
        let state = unsafe { &*state_ptr };

        // DDL-time type inference (file-backed databases only).
        // Runs LIMIT 0 on the expanded SQL via the persist connection.
        // The persist connection has its own context -- safe from bind's execution lock.
        if let Some(conn) = state.persist_conn {
            let req_all = QueryRequest {
                dimensions: parsed
                    .def
                    .dimensions
                    .iter()
                    .map(|d| d.name.clone())
                    .collect(),
                metrics: parsed.def.metrics.iter().map(|m| m.name.clone()).collect(),
                granularity_overrides: std::collections::HashMap::new(),
            };
            if let Ok(expanded_for_inference) = expand(&parsed.name, &parsed.def, &req_all) {
                let limit0_sql = format!("{expanded_for_inference} LIMIT 0");
                if let Some((names, types)) =
                    unsafe { crate::query::table_function::try_infer_schema(conn, &limit0_sql) }
                {
                    parsed.def.column_type_names = names;
                    parsed.def.column_types_inferred = types
                        .iter()
                        .map(|t| crate::query::table_function::normalize_type_id(*t as u32))
                        .collect();
                }
            }
        }

        let json = serde_json::to_string(&parsed.def)
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;

        // 1. Persist to DuckDB table FIRST (file-backed databases only).
        if let Some(conn) = state.persist_conn {
            persist_define(conn, &parsed.name, &json)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
        }

        // 2. Update in-memory catalog.
        if state.or_replace {
            catalog_upsert(&state.catalog, &parsed.name, &json)?;
        } else if state.if_not_exists {
            match catalog_insert(&state.catalog, &parsed.name, &json) {
                Ok(()) => {}
                Err(e) if e.to_string().contains("already exists") => {
                    // Silently succeed -- view exists, no replacement needed.
                }
                Err(e) => return Err(e),
            }
        } else {
            catalog_insert(&state.catalog, &parsed.name, &json)?;
        }

        Ok(DefineBindData { name: parsed.name })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(DefineInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        if init_data.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let bind_data = func.get_bind_data();
        let name_vec = output.flat_vector(0);
        name_vec.insert(0, bind_data.name.as_str());
        output.set_len(1);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        // Positional parameter 0: view name (VARCHAR)
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }

    fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
        let varchar = || LogicalTypeHandle::from(LogicalTypeId::Varchar);

        let tables_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("alias", varchar()),
            ("table", varchar()),
        ]));
        let col_pair_struct =
            LogicalTypeHandle::struct_type(&[("from", varchar()), ("to", varchar())]);
        let join_columns_type = LogicalTypeHandle::list(&col_pair_struct);
        let relationships_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("from_table", varchar()),
            ("to_table", varchar()),
            ("join_columns", join_columns_type),
        ]));
        let dimensions_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("name", varchar()),
            ("expr", varchar()),
            ("source_table", varchar()),
        ]));
        let time_dimensions_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("name", varchar()),
            ("expr", varchar()),
            ("granularity", varchar()),
        ]));
        let metrics_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
            ("name", varchar()),
            ("expr", varchar()),
            ("source_table", varchar()),
        ]));

        Some(vec![
            ("tables".to_string(), tables_type),
            ("relationships".to_string(), relationships_type),
            ("dimensions".to_string(), dimensions_type),
            ("time_dimensions".to_string(), time_dimensions_type),
            ("metrics".to_string(), metrics_type),
        ])
    }
}
