/// Parse the 6 positional STRUCT/LIST arguments of `define_semantic_view` from a
/// [`DataChunkHandle`] into a [`SemanticViewDefinition`].
///
/// # Argument positions
///
/// | Position | DuckDB type | Field |
/// |----------|-------------|-------|
/// | 0 | `VARCHAR` | view name |
/// | 1 | `LIST(STRUCT(alias VARCHAR, table VARCHAR))` | tables |
/// | 2 | `LIST(STRUCT(from_table VARCHAR, to_table VARCHAR, join_columns LIST(STRUCT(from VARCHAR, to VARCHAR))))` | relationships |
/// | 3 | `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))` | dimensions |
/// | 4 | `LIST(STRUCT(name VARCHAR, expr VARCHAR, granularity VARCHAR))` | time_dimensions |
/// | 5 | `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))` | metrics |
///
/// # Safety
///
/// Caller must ensure `input` contains valid DuckDB vector data produced by the DuckDB
/// engine. Reads raw `duckdb_string_t` bytes from vector slices — undefined behavior if
/// the vectors are invalidated (e.g., after the chunk is freed).
use duckdb::core::DataChunkHandle;
use duckdb::types::DuckString;
use libduckdb_sys::duckdb_string_t;

use crate::model::{Dimension, Join, JoinColumn, Metric, SemanticViewDefinition, TableRef};

/// Result of parsing the 6 `define_semantic_view` positional arguments.
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

/// Read a `duckdb_string_t` value from a raw slice at a given index.
///
/// # Safety
///
/// The caller must ensure `data` points to a valid, live DuckDB string vector slice
/// and `i` is within bounds.
pub unsafe fn read_str(data: &[duckdb_string_t], i: usize) -> String {
    DuckString::new(&mut { data[i] }).as_str().to_string()
}

/// Parse the 6 positional `define_semantic_view` arguments from a [`DataChunkHandle`].
///
/// Processes row at index `row`. Each call handles one VScalar invocation row.
///
/// # Safety
///
/// See module-level safety note.
pub unsafe fn parse_define_args(
    input: &mut DataChunkHandle,
    row: usize,
) -> Result<ParsedDefineArgs, String> {
    // ------------------------------------------------------------------
    // Column 0: name (VARCHAR)
    // ------------------------------------------------------------------
    let name_col = input.flat_vector(0);
    let names = name_col.as_slice_with_len::<duckdb_string_t>(input.len());
    let name = read_str(names, row);

    // ------------------------------------------------------------------
    // Column 1: tables LIST(STRUCT(alias VARCHAR, table VARCHAR))
    // ------------------------------------------------------------------
    let tables_list = input.list_vector(1);
    let total_tables = tables_list.len();
    let mut tables: Vec<TableRef> = Vec::new();
    if total_tables > 0 {
        let struct_child = tables_list.struct_child(total_tables);
        let alias_fv = struct_child.child(0, total_tables);
        let table_fv = struct_child.child(1, total_tables);
        let alias_data = alias_fv.as_slice_with_len::<duckdb_string_t>(total_tables);
        let table_data = table_fv.as_slice_with_len::<duckdb_string_t>(total_tables);
        let (offset, len) = tables_list.get_entry(row);
        for i in offset..offset + len {
            let alias = read_str(alias_data, i);
            let tbl = read_str(table_data, i);
            tables.push(TableRef { alias, table: tbl });
        }
    }

    // ------------------------------------------------------------------
    // Column 2: relationships LIST(STRUCT(from_table, to_table, join_columns LIST(STRUCT(from, to))))
    // ------------------------------------------------------------------
    let rel_list = input.list_vector(2);
    let total_rels = rel_list.len();
    let mut joins: Vec<Join> = Vec::new();
    if total_rels > 0 {
        let rel_struct = rel_list.struct_child(total_rels);
        // Field 0: from_table VARCHAR, Field 1: to_table VARCHAR, Field 2: join_columns LIST(STRUCT)
        let from_table_fv = rel_struct.child(0, total_rels);
        let to_table_fv = rel_struct.child(1, total_rels);
        let from_table_data = from_table_fv.as_slice_with_len::<duckdb_string_t>(total_rels);
        let to_table_data = to_table_fv.as_slice_with_len::<duckdb_string_t>(total_rels);

        // Field 2: nested join_columns list
        let jc_list = rel_struct.list_vector_child(2);
        let total_jc = jc_list.len();

        let (rel_offset, rel_len) = rel_list.get_entry(row);
        for rel_i in rel_offset..rel_offset + rel_len {
            let to_table = read_str(to_table_data, rel_i);
            // from_table is stored for reference (used in context, join.table = to_table)
            let _from_table = read_str(from_table_data, rel_i);

            // Read nested join_columns for this relationship entry
            let mut join_columns: Vec<JoinColumn> = Vec::new();
            if total_jc > 0 {
                let jc_struct = jc_list.struct_child(total_jc);
                let jc_from_fv = jc_struct.child(0, total_jc);
                let jc_to_fv = jc_struct.child(1, total_jc);
                let jc_from_data = jc_from_fv.as_slice_with_len::<duckdb_string_t>(total_jc);
                let jc_to_data = jc_to_fv.as_slice_with_len::<duckdb_string_t>(total_jc);
                let (jc_offset, jc_len) = jc_list.get_entry(rel_i);
                for jc_i in jc_offset..jc_offset + jc_len {
                    let from = read_str(jc_from_data, jc_i);
                    let to = read_str(jc_to_data, jc_i);
                    join_columns.push(JoinColumn { from, to });
                }
            }

            joins.push(Join {
                table: to_table,
                on: String::new(),
                from_cols: vec![],
                join_columns,
            });
        }
    }

    // ------------------------------------------------------------------
    // Column 3: dimensions LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))
    // ------------------------------------------------------------------
    let dim_list = input.list_vector(3);
    let total_dims = dim_list.len();
    let mut dimensions: Vec<Dimension> = Vec::new();
    if total_dims > 0 {
        let dim_struct = dim_list.struct_child(total_dims);
        let name_fv = dim_struct.child(0, total_dims);
        let expr_fv = dim_struct.child(1, total_dims);
        let st_fv = dim_struct.child(2, total_dims);
        let name_data = name_fv.as_slice_with_len::<duckdb_string_t>(total_dims);
        let expr_data = expr_fv.as_slice_with_len::<duckdb_string_t>(total_dims);
        let st_data = st_fv.as_slice_with_len::<duckdb_string_t>(total_dims);
        let (offset, len) = dim_list.get_entry(row);
        for i in offset..offset + len {
            let dim_name = read_str(name_data, i);
            let dim_expr = read_str(expr_data, i);
            let source_table_str = read_str(st_data, i);
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
            });
        }
    }

    // ------------------------------------------------------------------
    // Column 4: time_dimensions LIST(STRUCT(name VARCHAR, expr VARCHAR, granularity VARCHAR))
    // ------------------------------------------------------------------
    let tdim_list = input.list_vector(4);
    let total_tdims = tdim_list.len();
    if total_tdims > 0 {
        let tdim_struct = tdim_list.struct_child(total_tdims);
        let name_fv = tdim_struct.child(0, total_tdims);
        let expr_fv = tdim_struct.child(1, total_tdims);
        let gran_fv = tdim_struct.child(2, total_tdims);
        let name_data = name_fv.as_slice_with_len::<duckdb_string_t>(total_tdims);
        let expr_data = expr_fv.as_slice_with_len::<duckdb_string_t>(total_tdims);
        let gran_data = gran_fv.as_slice_with_len::<duckdb_string_t>(total_tdims);
        let (offset, len) = tdim_list.get_entry(row);
        for i in offset..offset + len {
            let dim_name = read_str(name_data, i);
            let dim_expr = read_str(expr_data, i);
            let gran = read_str(gran_data, i);
            validate_granularity(&gran)?;
            dimensions.push(Dimension {
                name: dim_name,
                expr: dim_expr,
                source_table: None,
                dim_type: Some("time".to_string()),
                granularity: Some(gran),
            });
        }
    }

    // ------------------------------------------------------------------
    // Column 5: metrics LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))
    // ------------------------------------------------------------------
    let met_list = input.list_vector(5);
    let total_mets = met_list.len();
    let mut metrics: Vec<Metric> = Vec::new();
    if total_mets > 0 {
        let met_struct = met_list.struct_child(total_mets);
        let name_fv = met_struct.child(0, total_mets);
        let expr_fv = met_struct.child(1, total_mets);
        let st_fv = met_struct.child(2, total_mets);
        let name_data = name_fv.as_slice_with_len::<duckdb_string_t>(total_mets);
        let expr_data = expr_fv.as_slice_with_len::<duckdb_string_t>(total_mets);
        let st_data = st_fv.as_slice_with_len::<duckdb_string_t>(total_mets);
        let (offset, len) = met_list.get_entry(row);
        for i in offset..offset + len {
            let met_name = read_str(name_data, i);
            let met_expr = read_str(expr_data, i);
            let source_table_str = read_str(st_data, i);
            let source_table = if source_table_str.is_empty() {
                None
            } else {
                Some(source_table_str)
            };
            metrics.push(Metric {
                name: met_name,
                expr: met_expr,
                source_table,
            });
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
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----------------------------------------------------------------
    // Unit tests for pure helper functions that don't require
    // DataChunkHandle construction. The full parse_define_args function
    // is validated end-to-end via the integration tests in Plan 05
    // (make test with sqllogictest).
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
            // Granularity is case-sensitive — "Day" is not valid, user must write "day"
            let err = validate_granularity("Day").unwrap_err();
            assert!(
                err.contains("Day"),
                "Error must include the bad value: {err}"
            );
        }
    }
}
