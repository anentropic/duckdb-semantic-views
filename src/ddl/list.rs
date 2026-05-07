use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;

/// A single row in the SHOW SEMANTIC VIEWS output.
struct ListRow {
    created_on: String,
    name: String,
    kind: String,
    database_name: String,
    schema_name: String,
    comment: String,
}

/// Bind-time snapshot of all registered semantic views.
///
/// Populated once at bind time by reading the in-memory catalog.
/// Stored as a `Vec<ListRow>` — one entry per view with 6 Snowflake-aligned columns:
/// created_on, name, kind, database_name, schema_name, comment.
pub struct ListBindData {
    rows: Vec<ListRow>,
}

// SAFETY: `Vec<ListRow>` contains only `String` fields, which are `Send + Sync`.
unsafe impl Send for ListBindData {}
unsafe impl Sync for ListBindData {}

/// Init data for `list_semantic_views`: tracks whether rows have been emitted.
pub struct ListInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ListInitData {}
unsafe impl Sync for ListInitData {}

/// Table function that returns 6 columns: `(created_on VARCHAR, name VARCHAR,
/// kind VARCHAR, database_name VARCHAR, schema_name VARCHAR, comment VARCHAR)` — one row per
/// registered semantic view.
///
/// Takes no parameters.  State is injected via
/// `register_table_function_with_extra_info`.
pub struct ListSemanticViewsVTab;

impl VTab for ListSemanticViewsVTab {
    type BindData = ListBindData;
    type InitData = ListInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind.add_result_column(
                "created_on",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column("kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column(
                "database_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "schema_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("comment", LogicalTypeHandle::from(LogicalTypeId::Varchar));

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader
                .list_all()
                .map_err(Box::<dyn std::error::Error>::from)?;

            let mut rows = Vec::with_capacity(entries.len());
            for (name, json) in &entries {
                let def = SemanticViewDefinition::from_json(name, json).ok();
                let (created_on, database_name, schema_name, comment) = match &def {
                    Some(d) => (
                        d.created_on.clone().unwrap_or_default(),
                        d.database_name.clone().unwrap_or_default(),
                        d.schema_name.clone().unwrap_or_default(),
                        d.comment.clone().unwrap_or_default(),
                    ),
                    None => (String::new(), String::new(), String::new(), String::new()),
                };
                rows.push(ListRow {
                    created_on,
                    name: name.clone(),
                    kind: "SEMANTIC_VIEW".to_string(),
                    database_name,
                    schema_name,
                    comment,
                });
            }
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ListBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ListInitData {
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
        let n = bind_data.rows.len();

        let created_on_vec = output.flat_vector(0);
        let name_vec = output.flat_vector(1);
        let kind_vec = output.flat_vector(2);
        let db_name_vec = output.flat_vector(3);
        let sch_name_vec = output.flat_vector(4);
        let comment_vec = output.flat_vector(5);
        for (i, row) in bind_data.rows.iter().enumerate() {
            created_on_vec.insert(i, row.created_on.as_str());
            name_vec.insert(i, row.name.as_str());
            kind_vec.insert(i, row.kind.as_str());
            db_name_vec.insert(i, row.database_name.as_str());
            sch_name_vec.insert(i, row.schema_name.as_str());
            comment_vec.insert(i, row.comment.as_str());
        }
        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}

// ---------------------------------------------------------------------------
// SHOW TERSE SEMANTIC VIEWS — 5-column subset (no comment)
// ---------------------------------------------------------------------------

/// A single row in the SHOW TERSE SEMANTIC VIEWS output.
struct ListTerseRow {
    created_on: String,
    name: String,
    kind: String,
    database_name: String,
    schema_name: String,
}

/// Bind-time snapshot for terse listing.
pub struct ListTerseBindData {
    rows: Vec<ListTerseRow>,
}

// SAFETY: `Vec<ListTerseRow>` contains only `String` fields, which are `Send + Sync`.
unsafe impl Send for ListTerseBindData {}
unsafe impl Sync for ListTerseBindData {}

/// Init data for `list_terse_semantic_views`.
pub struct ListTerseInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ListTerseInitData {}
unsafe impl Sync for ListTerseInitData {}

/// Table function that returns 5 columns: `(created_on VARCHAR, name VARCHAR,
/// kind VARCHAR, database_name VARCHAR, schema_name VARCHAR)` — one row per
/// registered semantic view. No comment column (terse mode).
pub struct ListTerseSemanticViewsVTab;

impl VTab for ListTerseSemanticViewsVTab {
    type BindData = ListTerseBindData;
    type InitData = ListTerseInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind.add_result_column(
                "created_on",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column("kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column(
                "database_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "schema_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader
                .list_all()
                .map_err(Box::<dyn std::error::Error>::from)?;

            let mut rows = Vec::with_capacity(entries.len());
            for (name, json) in &entries {
                let def = SemanticViewDefinition::from_json(name, json).ok();
                let (created_on, database_name, schema_name) = match &def {
                    Some(d) => (
                        d.created_on.clone().unwrap_or_default(),
                        d.database_name.clone().unwrap_or_default(),
                        d.schema_name.clone().unwrap_or_default(),
                    ),
                    None => (String::new(), String::new(), String::new()),
                };
                rows.push(ListTerseRow {
                    created_on,
                    name: name.clone(),
                    kind: "SEMANTIC_VIEW".to_string(),
                    database_name,
                    schema_name,
                });
            }
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ListTerseBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ListTerseInitData {
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
        let n = bind_data.rows.len();

        let created_on_vec = output.flat_vector(0);
        let name_vec = output.flat_vector(1);
        let kind_vec = output.flat_vector(2);
        let db_name_vec = output.flat_vector(3);
        let sch_name_vec = output.flat_vector(4);
        for (i, row) in bind_data.rows.iter().enumerate() {
            created_on_vec.insert(i, row.created_on.as_str());
            name_vec.insert(i, row.name.as_str());
            kind_vec.insert(i, row.kind.as_str());
            db_name_vec.insert(i, row.database_name.as_str());
            sch_name_vec.insert(i, row.schema_name.as_str());
        }
        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}
