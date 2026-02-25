use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};
use libduckdb_sys as ffi;

use crate::expand::{expand, suggest_closest, QueryRequest};
use crate::model::SemanticViewDefinition;

use super::error::QueryError;
use super::table_function::{
    execute_sql_raw, extract_list_strings, read_varchar_from_vector, QueryState,
};

// ---------------------------------------------------------------------------
// BindData / InitData
// ---------------------------------------------------------------------------

/// Data computed at bind time: all output lines to return as rows.
pub struct ExplainBindData {
    /// Each entry is one line of the three-part EXPLAIN output.
    lines: Vec<String>,
}

// SAFETY: `Vec<String>` is `Send + Sync`.
unsafe impl Send for ExplainBindData {}
unsafe impl Sync for ExplainBindData {}

/// State tracked during function execution.
pub struct ExplainInitData {
    /// Signals when all rows have been emitted.
    done: AtomicBool,
    /// Current row index for iteration.
    row_index: AtomicUsize,
}

// SAFETY: Atomic types are `Send + Sync`.
unsafe impl Send for ExplainInitData {}
unsafe impl Sync for ExplainInitData {}

// ---------------------------------------------------------------------------
// EXPLAIN plan extraction
// ---------------------------------------------------------------------------

/// Execute `EXPLAIN {sql}` and return the plan as lines of text.
///
/// If the EXPLAIN fails (e.g., referenced tables do not exist), returns
/// a single fallback line with the error message.
///
/// # Safety
///
/// `conn` must be a valid, non-null `duckdb_connection` handle.
#[allow(clippy::cast_possible_truncation)]
unsafe fn collect_explain_lines(conn: ffi::duckdb_connection, sql: &str) -> Vec<String> {
    let explain_sql = format!("EXPLAIN {sql}");
    let mut lines = Vec::new();

    match execute_sql_raw(conn, &explain_sql) {
        Ok(mut result) => {
            let col_count = ffi::duckdb_column_count(&raw mut result) as usize;
            let chunk_count = ffi::duckdb_result_chunk_count(result) as usize;

            for chunk_idx in 0..chunk_count {
                let chunk = ffi::duckdb_result_get_chunk(result, chunk_idx as ffi::idx_t);
                if chunk.is_null() {
                    continue;
                }
                let row_count = ffi::duckdb_data_chunk_get_size(chunk) as usize;

                for row_idx in 0..row_count {
                    for col_idx in 0..col_count {
                        let s = read_varchar_from_vector(chunk, col_idx, row_idx);
                        if !s.is_empty() {
                            for plan_line in s.lines() {
                                lines.push(plan_line.to_string());
                            }
                        }
                    }
                }

                ffi::duckdb_destroy_data_chunk(&mut { chunk });
            }

            ffi::duckdb_destroy_result(&raw mut result);
        }
        Err(err) => {
            lines.push(format!("-- (not available -- {err})"));
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// VTab implementation
// ---------------------------------------------------------------------------

/// The `explain_semantic_view` table function.
///
/// Returns formatted EXPLAIN output with three parts:
/// 1. Metadata header (view name, dimensions, metrics)
/// 2. Pretty-printed expanded SQL
/// 3. `DuckDB` EXPLAIN plan (or graceful fallback)
///
/// Usage:
/// ```sql
/// FROM explain_semantic_view('my_view', dimensions := ['region'], metrics := ['total_revenue'])
/// ```
pub struct ExplainSemanticViewVTab;

impl VTab for ExplainSemanticViewVTab {
    type BindData = ExplainBindData;
    type InitData = ExplainInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        // 1. Extract parameters (same as semantic_query).
        let view_name = bind.get_parameter(0).to_string();

        let dimensions = match bind.get_named_parameter("dimensions") {
            Some(ref val) => unsafe { extract_list_strings(val) },
            None => vec![],
        };
        let metrics = match bind.get_named_parameter("metrics") {
            Some(ref val) => unsafe { extract_list_strings(val) },
            None => vec![],
        };

        // 2. Validate: at least one dimension or metric.
        if dimensions.is_empty() && metrics.is_empty() {
            return Err(Box::new(QueryError::EmptyRequest {
                view_name: view_name.clone(),
            }));
        }

        // 3. Look up view definition in the catalog.
        let state_ptr = bind.get_extra_info::<QueryState>();
        let state = unsafe { &*state_ptr };
        let catalog_guard = state.catalog.read().expect("catalog RwLock poisoned");

        let json_str = if let Some(j) = catalog_guard.get(&view_name) {
            j.clone()
        } else {
            let available: Vec<String> = catalog_guard.keys().cloned().collect();
            let suggestion = suggest_closest(&view_name, &available);
            return Err(Box::new(QueryError::ViewNotFound {
                name: view_name,
                suggestion,
                available,
            }));
        };
        drop(catalog_guard);

        // 4. Parse definition and expand.
        let def = SemanticViewDefinition::from_json(&view_name, &json_str)
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        let req = QueryRequest {
            dimensions: dimensions.clone(),
            metrics: metrics.clone(),
        };
        let expanded_sql = expand(&view_name, &def, &req)
            .map_err(|e| -> Box<dyn std::error::Error> { Box::new(QueryError::from(e)) })?;

        // 5. Build the three-part output.
        let mut lines: Vec<String> = Vec::new();

        // Part 1: Metadata header.
        lines.push(format!("-- Semantic View: {view_name}"));
        lines.push(format!("-- Dimensions: {}", dimensions.join(", ")));
        lines.push(format!("-- Metrics: {}", metrics.join(", ")));
        lines.push(String::new());

        // Part 2: Expanded SQL.
        lines.push("-- Expanded SQL:".to_string());
        for sql_line in expanded_sql.lines() {
            lines.push(sql_line.to_string());
        }
        lines.push(String::new());

        // Part 3: DuckDB EXPLAIN plan.
        lines.push("-- DuckDB Plan:".to_string());
        let explain_lines = unsafe { collect_explain_lines(state.conn, &expanded_sql) };
        lines.extend(explain_lines);

        // 6. Declare output column.
        bind.add_result_column(
            "explain_output",
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        );

        Ok(ExplainBindData { lines })
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ExplainInitData {
            done: AtomicBool::new(false),
            row_index: AtomicUsize::new(0),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();

        // If already done, signal completion.
        if init_data.done.load(Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let bind_data = func.get_bind_data();
        let total_lines = bind_data.lines.len();
        let start = init_data.row_index.load(Ordering::Relaxed);

        if start >= total_lines {
            init_data.done.store(true, Ordering::Relaxed);
            output.set_len(0);
            return Ok(());
        }

        // Emit lines in chunks. DuckDB standard chunk size is 2048.
        let chunk_size = 2048;
        let end = (start + chunk_size).min(total_lines);
        let count = end - start;

        let out_vec = output.flat_vector(0);
        for (i, line) in bind_data.lines[start..end].iter().enumerate() {
            out_vec.insert(i, line.as_str());
        }
        output.set_len(count);

        init_data.row_index.store(end, Ordering::Relaxed);
        if end >= total_lines {
            init_data.done.store(true, Ordering::Relaxed);
        }

        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        // Positional parameter: view_name (VARCHAR)
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }

    fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
        Some(vec![
            (
                "dimensions".to_string(),
                LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar)),
            ),
            (
                "metrics".to_string(),
                LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar)),
            ),
        ])
    }
}
