use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogReader;
use crate::ddl::describe::format_json_array;
use crate::model::SemanticViewDefinition;

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 2 (Wave 1) — sv_show_semantic_metrics_all_bind_rust
// ---------------------------------------------------------------------------
// FFI dispatcher for the migrated show_semantic_metrics_all() TF. Same
// bridge mechanism + borrow contract as the Wave 0 spike. 8-column VARCHAR.

/// # Safety
///
/// `conn` is a borrowed handle (see `src/ddl/list.rs` file-level docs).
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_show_semantic_metrics_all_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use crate::ddl::read_ffi::{
        probe_catalog_table_present, publish_owned_buffer, serialize_varchar_rows, write_err,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if conn.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        let reader = CatalogReader::new(conn, probe_catalog_table_present(conn));
        let entries = match reader.list_all() {
            Ok(e) => e,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let mut rows: Vec<Vec<String>> = Vec::new();
        for (name, json) in &entries {
            for r in collect_metrics(name, json) {
                rows.push(vec![
                    r.database_name,
                    r.schema_name,
                    r.semantic_view_name,
                    r.table_name,
                    r.name,
                    r.data_type,
                    r.synonyms,
                    r.comment,
                ]);
            }
        }
        rows.sort_by(|a, b| a[2].cmp(&b[2]).then_with(|| a[4].cmp(&b[4])));
        let buf = serialize_varchar_rows(&rows);
        publish_owned_buffer(buf, out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            use crate::ddl::read_ffi::write_err;
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_show_semantic_metrics_all_bind_rust",
            );
            2
        }
    }
}

/// A single row in the SHOW SEMANTIC METRICS output.
///
/// 8 Snowflake-aligned columns: database_name, schema_name, semantic_view_name,
/// table_name, name, data_type, synonyms, comment.
struct ShowMetricRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    table_name: String,
    name: String,
    data_type: String,
    synonyms: String,
    comment: String,
}

/// Bind-time data: pre-collected metric rows.
pub struct ShowMetricsBindData {
    rows: Vec<ShowMetricRow>,
}

// SAFETY: all fields are owned `Vec<ShowMetricRow>` (String fields), which is `Send + Sync`.
unsafe impl Send for ShowMetricsBindData {}
unsafe impl Sync for ShowMetricsBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct ShowMetricsInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ShowMetricsInitData {}
unsafe impl Sync for ShowMetricsInitData {}

/// Helper: declare the 8-column output schema for metrics.
fn bind_output_columns(bind: &BindInfo) {
    bind.add_result_column(
        "database_name",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column(
        "schema_name",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column(
        "semantic_view_name",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column(
        "table_name",
        LogicalTypeHandle::from(LogicalTypeId::Varchar),
    );
    bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("data_type", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("synonyms", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("comment", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}

/// Helper: collect metric rows for a single view.
fn collect_metrics(view_name: &str, json: &str) -> Vec<ShowMetricRow> {
    let Ok(def) = SemanticViewDefinition::from_json(view_name, json) else {
        return Vec::new();
    };
    let db_name = def.database_name.clone().unwrap_or_default();
    let sch_name = def.schema_name.clone().unwrap_or_default();
    let alias_map = def.alias_to_table_map();
    def.metrics
        .iter()
        .map(|m| {
            let table_name = m
                .source_table
                .as_ref()
                .and_then(|a| alias_map.get(a).cloned())
                .unwrap_or_default();
            ShowMetricRow {
                database_name: db_name.clone(),
                schema_name: sch_name.clone(),
                semantic_view_name: view_name.to_string(),
                table_name,
                name: m.name.clone(),
                data_type: m.output_type.clone().unwrap_or_default(),
                synonyms: format_json_array(&m.synonyms),
                comment: m.comment.clone().unwrap_or_default(),
            }
        })
        .collect()
}

/// Helper: emit rows from bind data into the output chunk.
fn emit_rows(
    func: &TableFunctionInfo<
        impl VTab<BindData = ShowMetricsBindData, InitData = ShowMetricsInitData>,
    >,
    output: &mut DataChunkHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let init_data = func.get_init_data();
    if init_data.done.swap(true, Ordering::Relaxed) {
        output.set_len(0);
        return Ok(());
    }

    let bind_data = func.get_bind_data();
    let n = bind_data.rows.len();

    let db_name_vec = output.flat_vector(0);
    let sch_name_vec = output.flat_vector(1);
    let sv_vec = output.flat_vector(2);
    let table_vec = output.flat_vector(3);
    let name_vec = output.flat_vector(4);
    let type_vec = output.flat_vector(5);
    let syn_vec = output.flat_vector(6);
    let cmt_vec = output.flat_vector(7);

    for (i, row) in bind_data.rows.iter().enumerate() {
        db_name_vec.insert(i, row.database_name.as_str());
        sch_name_vec.insert(i, row.schema_name.as_str());
        sv_vec.insert(i, row.semantic_view_name.as_str());
        table_vec.insert(i, row.table_name.as_str());
        name_vec.insert(i, row.name.as_str());
        type_vec.insert(i, row.data_type.as_str());
        syn_vec.insert(i, row.synonyms.as_str());
        cmt_vec.insert(i, row.comment.as_str());
    }
    output.set_len(n);
    Ok(())
}

// ---------------------------------------------------------------------------
// Single-view form: show_semantic_metrics('view_name')
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 3 (Wave 2) — sv_show_semantic_metrics_bind_rust
// ---------------------------------------------------------------------------
// Single-view variant. 8 VARCHAR cols (same shape as the _all variant).

/// # Safety
///
/// `conn` is a borrowed handle; `name_ptr` must point to `name_len` UTF-8 bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_show_semantic_metrics_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    name_ptr: *const u8,
    name_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use crate::ddl::read_ffi::{
        probe_catalog_table_present, publish_owned_buffer, serialize_varchar_rows, write_err,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if conn.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        if name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "view name pointer is null");
            return 1_u8;
        }
        let view_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s.to_string(),
            Err(_) => {
                write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
                return 1_u8;
            }
        };
        let reader = CatalogReader::new(conn, probe_catalog_table_present(conn));
        let json = match reader.lookup(&view_name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &format!("semantic view '{view_name}' does not exist"),
                );
                return 1_u8;
            }
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let mut internal = collect_metrics(&view_name, &json);
        internal.sort_by(|a, b| a.name.cmp(&b.name));
        let mut rows: Vec<Vec<String>> = Vec::with_capacity(internal.len());
        for r in internal {
            rows.push(vec![
                r.database_name,
                r.schema_name,
                r.semantic_view_name,
                r.table_name,
                r.name,
                r.data_type,
                r.synonyms,
                r.comment,
            ]);
        }
        let buf = serialize_varchar_rows(&rows);
        publish_owned_buffer(buf, out_ptr, out_len);
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            use crate::ddl::read_ffi::write_err;
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_show_semantic_metrics_bind_rust",
            );
            2
        }
    }
}

/// Table function: `SHOW SEMANTIC METRICS IN view_name`
///
/// Takes one VARCHAR parameter (view name). Returns metrics for that view.
///
/// Phase 65 Plan 05 Task 3 (Wave 2): registration retired in favor of the
/// C++ Catalog API path (`sv_register_show_semantic_metrics`).
#[allow(dead_code)]
pub struct ShowSemanticMetricsVTab;

impl VTab for ShowSemanticMetricsVTab {
    type BindData = ShowMetricsBindData;
    type InitData = ShowMetricsInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind_output_columns(bind);

            let view_name = bind.get_parameter(0).to_string();
            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let json = reader
                .lookup(&view_name)
                .map_err(Box::<dyn std::error::Error>::from)?
                .ok_or_else(|| format!("semantic view '{view_name}' does not exist"))?;

            let mut rows = collect_metrics(&view_name, &json);
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ShowMetricsBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowMetricsInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        emit_rows(func, output)
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}

// ---------------------------------------------------------------------------
// Cross-view form: show_semantic_metrics_all()
// ---------------------------------------------------------------------------

/// Table function: `SHOW SEMANTIC METRICS` (no IN clause)
///
/// Takes no parameters. Returns metrics across all registered semantic views.
///
/// Phase 65 Plan 05 Task 2 (Wave 1): registration retired in favor of the
/// C++ Catalog API path (`sv_register_show_semantic_metrics_all`).
#[allow(dead_code)]
pub struct ShowSemanticMetricsAllVTab;

impl VTab for ShowSemanticMetricsAllVTab {
    type BindData = ShowMetricsBindData;
    type InitData = ShowMetricsInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind_output_columns(bind);

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader
                .list_all()
                .map_err(Box::<dyn std::error::Error>::from)?;

            let mut rows = Vec::new();
            for (name, json) in &entries {
                rows.extend(collect_metrics(name, json));
            }
            rows.sort_by(|a, b| {
                a.semantic_view_name
                    .cmp(&b.semantic_view_name)
                    .then_with(|| a.name.cmp(&b.name))
            });

            Ok(ShowMetricsBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ShowMetricsInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        emit_rows(func, output)
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}
