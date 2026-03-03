/// Parse the 6 keyword/positional arguments of `create_semantic_view` from a
/// [`BindInfo`] into a [`SemanticViewDefinition`].
///
/// Supports both positional and keyword argument syntax:
///   - Positional: `create_semantic_view('name', [...tables], [...rels], [...dims], [...tdims], [...metrics])`
///   - Keyword: `create_semantic_view('name', tables := [...], dimensions := [...], metrics := [...])`
///   - Mixed: `create_semantic_view('name', tables := [...], dimensions := [...], metrics := [...])`
///
/// Named parameters are checked first; if absent, the corresponding positional
/// parameter is used as fallback.
///
/// # Argument mapping
///
/// | Named param        | Positional | DuckDB type |
/// |--------------------|------------|-------------|
/// | (always positional) | 0          | `VARCHAR` (view name) |
/// | `tables`           | 1          | `LIST(STRUCT(alias VARCHAR, table VARCHAR))` |
/// | `relationships`    | 2          | `LIST(STRUCT(from_table VARCHAR, to_table VARCHAR, join_columns LIST(STRUCT(from VARCHAR, to VARCHAR))))` |
/// | `dimensions`       | 3          | `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))` |
/// | `time_dimensions`  | 4          | `LIST(STRUCT(name VARCHAR, expr VARCHAR, granularity VARCHAR))` |
/// | `metrics`          | 5          | `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))` |
use duckdb::vtab::{BindInfo, Value};
use libduckdb_sys as ffi;
use std::ffi::CStr;
use std::os::raw::c_void;

use crate::model::{Dimension, Join, JoinColumn, Metric, SemanticViewDefinition, TableRef};
use crate::query::table_function::value_raw_ptr;

/// Result of parsing the 6 `create_semantic_view` arguments.
pub struct ParsedDefineArgs {
    pub name: String,
    pub def: SemanticViewDefinition,
}

/// Valid granularity values for time dimensions.
const VALID_GRANULARITIES: &[&str] = &["day", "week", "month", "year"];

/// Validate a granularity string.
///
/// Returns `Ok(())` if the value is one of `day`, `week`, `month`, `year`.
/// Returns `Err` with a descriptive message otherwise.
pub fn validate_granularity(granularity: &str) -> Result<(), String> {
    if VALID_GRANULARITIES.contains(&granularity) {
        Ok(())
    } else {
        Err(format!(
            "time_dimension has unsupported granularity '{}'; valid values: {}",
            granularity,
            VALID_GRANULARITIES.join(", ")
        ))
    }
}

// ---------------------------------------------------------------------------
// FFI helpers for extracting struct fields from duckdb_value handles
// ---------------------------------------------------------------------------

/// Extract a string from a `duckdb_value` using `duckdb_get_varchar`.
///
/// Handles null pointers by returning an empty string. Properly frees the
/// allocated C string via `duckdb_free`.
///
/// # Safety
///
/// `val` must be a valid `duckdb_value` handle.
unsafe fn extract_varchar(val: ffi::duckdb_value) -> String {
    let cstr = ffi::duckdb_get_varchar(val);
    if cstr.is_null() {
        return String::new();
    }
    let s = CStr::from_ptr(cstr).to_string_lossy().into_owned();
    ffi::duckdb_free(cstr.cast::<c_void>());
    s
}

/// Extract a struct child by positional index, read its VARCHAR value, and destroy it.
///
/// # Safety
///
/// `struct_val` must be a valid `duckdb_value` handle representing a STRUCT.
/// `index` must be a valid child index.
unsafe fn extract_struct_child_varchar(struct_val: ffi::duckdb_value, index: u64) -> String {
    let mut child_val = ffi::duckdb_get_struct_child(struct_val, index);
    let result = extract_varchar(child_val);
    ffi::duckdb_destroy_value(&mut child_val);
    result
}

/// Get the named parameter if present, otherwise fall back to the positional parameter.
fn get_param(bind: &BindInfo, name: &str, positional_idx: u64) -> Option<Value> {
    bind.get_named_parameter(name)
        .or_else(|| Some(bind.get_parameter(positional_idx)))
}

/// Parse `create_semantic_view` arguments from a VTab [`BindInfo`].
///
/// This is the primary parse path used by the VTab `bind()` implementation.
/// It reads arguments from either named parameters or positional parameters.
///
/// # Errors
///
/// Returns `Err` if:
/// - A time dimension has an invalid granularity value
pub fn parse_define_args_from_bind(bind: &BindInfo) -> Result<ParsedDefineArgs, String> {
    // ------------------------------------------------------------------
    // Param 0: name (VARCHAR) -- always positional
    // ------------------------------------------------------------------
    let name = bind.get_parameter(0).to_string();

    // ------------------------------------------------------------------
    // Param 1: tables LIST(STRUCT(alias:0, table:1))
    // ------------------------------------------------------------------
    let mut tables: Vec<TableRef> = Vec::new();
    if let Some(ref val) = get_param(bind, "tables", 1) {
        unsafe {
            let val_ptr = value_raw_ptr(val);
            let size = ffi::duckdb_get_list_size(val_ptr);
            for i in 0..size {
                let mut child = ffi::duckdb_get_list_child(val_ptr, i);
                let alias = extract_struct_child_varchar(child, 0); // alias
                let table = extract_struct_child_varchar(child, 1); // table
                ffi::duckdb_destroy_value(&mut child);
                tables.push(TableRef { alias, table });
            }
        }
    }

    // ------------------------------------------------------------------
    // Param 2: relationships LIST(STRUCT(from_table:0, to_table:1, join_columns:2))
    //   where join_columns is LIST(STRUCT(from:0, to:1))
    // ------------------------------------------------------------------
    let mut joins: Vec<Join> = Vec::new();
    if let Some(ref val) = get_param(bind, "relationships", 2) {
        unsafe {
            let val_ptr = value_raw_ptr(val);
            let size = ffi::duckdb_get_list_size(val_ptr);
            for i in 0..size {
                let mut rel_child = ffi::duckdb_get_list_child(val_ptr, i);
                let _from_alias = extract_struct_child_varchar(rel_child, 0); // from_table
                let to_alias = extract_struct_child_varchar(rel_child, 1); // to_table

                // Resolve to_alias to the physical table name via the tables vec.
                let to_table = tables
                    .iter()
                    .find(|t| t.alias.eq_ignore_ascii_case(&to_alias))
                    .map(|t| t.table.clone())
                    .unwrap_or(to_alias);

                // Extract join_columns: struct child at index 2 is a LIST(STRUCT(from, to))
                let mut jc_val = ffi::duckdb_get_struct_child(rel_child, 2);
                let jc_size = ffi::duckdb_get_list_size(jc_val);
                let mut join_columns: Vec<JoinColumn> = Vec::new();
                for j in 0..jc_size {
                    let mut jc_child = ffi::duckdb_get_list_child(jc_val, j);
                    let from = extract_struct_child_varchar(jc_child, 0); // from
                    let to = extract_struct_child_varchar(jc_child, 1); // to
                    ffi::duckdb_destroy_value(&mut jc_child);
                    join_columns.push(JoinColumn { from, to });
                }
                ffi::duckdb_destroy_value(&mut jc_val);
                ffi::duckdb_destroy_value(&mut rel_child);

                joins.push(Join {
                    table: to_table,
                    on: String::new(),
                    from_cols: vec![],
                    join_columns,
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Param 3: dimensions LIST(STRUCT(name:0, expr:1, source_table:2))
    // ------------------------------------------------------------------
    let mut dimensions: Vec<Dimension> = Vec::new();
    if let Some(ref val) = get_param(bind, "dimensions", 3) {
        unsafe {
            let val_ptr = value_raw_ptr(val);
            let size = ffi::duckdb_get_list_size(val_ptr);
            for i in 0..size {
                let mut child = ffi::duckdb_get_list_child(val_ptr, i);
                let dim_name = extract_struct_child_varchar(child, 0); // name
                let dim_expr = extract_struct_child_varchar(child, 1); // expr
                let source_table_str = extract_struct_child_varchar(child, 2); // source_table
                ffi::duckdb_destroy_value(&mut child);
                let source_table = if source_table_str.is_empty() {
                    None
                } else {
                    Some(source_table_str)
                };
                dimensions.push(Dimension {
                    name: dim_name,
                    expr: dim_expr,
                    source_table,
                    dim_type: None,
                    granularity: None,
                    output_type: None,
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Param 4: time_dimensions LIST(STRUCT(name:0, expr:1, granularity:2))
    // ------------------------------------------------------------------
    if let Some(ref val) = get_param(bind, "time_dimensions", 4) {
        unsafe {
            let val_ptr = value_raw_ptr(val);
            let size = ffi::duckdb_get_list_size(val_ptr);
            for i in 0..size {
                let mut child = ffi::duckdb_get_list_child(val_ptr, i);
                let dim_name = extract_struct_child_varchar(child, 0); // name
                let dim_expr = extract_struct_child_varchar(child, 1); // expr
                let gran = extract_struct_child_varchar(child, 2); // granularity
                ffi::duckdb_destroy_value(&mut child);
                validate_granularity(&gran)?;
                dimensions.push(Dimension {
                    name: dim_name,
                    expr: dim_expr,
                    source_table: None,
                    dim_type: Some("time".to_string()),
                    granularity: Some(gran),
                    output_type: None,
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Param 5: metrics LIST(STRUCT(name:0, expr:1, source_table:2))
    // ------------------------------------------------------------------
    let mut metrics: Vec<Metric> = Vec::new();
    if let Some(ref val) = get_param(bind, "metrics", 5) {
        unsafe {
            let val_ptr = value_raw_ptr(val);
            let size = ffi::duckdb_get_list_size(val_ptr);
            for i in 0..size {
                let mut child = ffi::duckdb_get_list_child(val_ptr, i);
                let met_name = extract_struct_child_varchar(child, 0); // name
                let met_expr = extract_struct_child_varchar(child, 1); // expr
                let source_table_str = extract_struct_child_varchar(child, 2); // source_table
                ffi::duckdb_destroy_value(&mut child);
                let source_table = if source_table_str.is_empty() {
                    None
                } else {
                    Some(source_table_str)
                };
                metrics.push(Metric {
                    name: met_name,
                    expr: met_expr,
                    source_table,
                    output_type: None,
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Assemble SemanticViewDefinition
    // ------------------------------------------------------------------
    let base_table = tables.first().map(|t| t.table.clone()).unwrap_or_default();

    Ok(ParsedDefineArgs {
        name,
        def: SemanticViewDefinition {
            base_table,
            tables,
            dimensions,
            metrics,
            filters: vec![],
            joins,
            facts: vec![],
            column_type_names: vec![],
            column_types_inferred: vec![],
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----------------------------------------------------------------
    // Unit tests for pure helper functions that don't require
    // BindInfo construction. The full parse_define_args_from_bind function
    // is validated end-to-end via the integration tests (sqllogictest).
    // ----------------------------------------------------------------

    mod granularity_validation_tests {
        use super::*;

        #[test]
        fn valid_granularities_are_accepted() {
            for gran in ["day", "week", "month", "year"] {
                assert!(
                    validate_granularity(gran).is_ok(),
                    "granularity '{gran}' should be accepted"
                );
            }
        }

        #[test]
        fn invalid_granularity_quarter_rejected() {
            let err = validate_granularity("quarter").unwrap_err();
            assert!(
                err.contains("quarter"),
                "Error must mention 'quarter': {err}"
            );
            assert!(
                err.contains("day, week, month, year"),
                "Error must list valid values: {err}"
            );
        }

        #[test]
        fn empty_granularity_rejected() {
            let err = validate_granularity("").unwrap_err();
            assert!(
                err.contains("unsupported granularity"),
                "Error must mention unsupported: {err}"
            );
        }

        #[test]
        fn mixed_case_granularity_rejected() {
            // Granularity is case-sensitive -- "Day" is not valid, user must write "day"
            let err = validate_granularity("Day").unwrap_err();
            assert!(
                err.contains("Day"),
                "Error must include the bad value: {err}"
            );
        }
    }
}
