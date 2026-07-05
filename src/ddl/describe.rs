use std::collections::HashMap;

use crate::catalog::CatalogReader;
use crate::model::{AccessModifier, SemanticViewDefinition};

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 3 (Wave 2) — sv_describe_semantic_view_bind_rust
// ---------------------------------------------------------------------------
// FFI dispatcher for the migrated describe_semantic_view(view_name) TF.
// 5-column VARCHAR (object_kind, object_name, parent_entity, property,
// property_value). Same bridge + borrow contract as Wave 0 spike.

/// # Safety
///
/// `conn` is a borrowed handle; `name_ptr` must point to `name_len` UTF-8 bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_describe_semantic_view_bind_rust(
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
        BorrowedConnection,
    };
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        if name_ptr.is_null() {
            write_err(error_buf, error_buf_len, "view name pointer is null");
            return 1_u8;
        }
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                write_err(error_buf, error_buf_len, "view name is not valid UTF-8");
                return 1_u8;
            }
        };
        let reader = CatalogReader::new(&borrowed, probe_catalog_table_present(&borrowed));
        let json = match reader.lookup(&name) {
            Ok(Some(j)) => j,
            Ok(None) => {
                write_err(
                    error_buf,
                    error_buf_len,
                    &crate::catalog::view_not_found_msg(&name),
                );
                return 1_u8;
            }
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
        let def = match SemanticViewDefinition::from_json(&name, &json) {
            Ok(d) => d,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e.to_string());
                return 1_u8;
            }
        };
        let alias_map = def.alias_to_table_map();
        let base_table = def.base_table().to_string();

        let mut internal: Vec<DescribeRow> = Vec::new();
        if let Some(ref comment) = def.comment {
            internal.push(DescribeRow {
                object_kind: String::new(),
                object_name: String::new(),
                parent_entity: String::new(),
                property: "COMMENT".to_string(),
                property_value: comment.clone(),
            });
        }
        collect_table_rows(&def, &mut internal);
        collect_relationship_rows(&def, &alias_map, &mut internal);
        collect_fact_rows(&def, &base_table, &alias_map, &mut internal);
        collect_dimension_rows(&def, &base_table, &alias_map, &mut internal);
        collect_metric_rows(&def, &base_table, &alias_map, &mut internal);
        collect_materialization_rows(&def, &mut internal);

        let mut rows: Vec<Vec<String>> = Vec::with_capacity(internal.len());
        for r in internal {
            rows.push(vec![
                r.object_kind,
                r.object_name,
                r.parent_entity,
                r.property,
                r.property_value,
            ]);
        }
        let buf = match serialize_varchar_rows(&rows) {
            Ok(b) => b,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };
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
                "internal error: panic inside sv_describe_semantic_view_bind_rust",
            );
            2
        }
    }
}

/// A single property row in the DESCRIBE output.
///
/// Each row represents one property of one object in the semantic view.
/// Output schema: `(object_kind, object_name, parent_entity, property, property_value)`.
struct DescribeRow {
    object_kind: String,
    object_name: String,
    parent_entity: String,
    property: String,
    property_value: String,
}

// Phase 65 Plan 05 Batch 3: legacy `DescribeBindData` + `DescribeInitData`
// retired with the H2 query_conn allocation. `DescribeRow` remains because
// `sv_describe_semantic_view_bind_rust` (above) still calls the
// `collect_*` helpers in this file to assemble property rows for the
// C++ Catalog API path's wire format.

/// Format column names as a JSON array: `["col1","col2"]`.
/// Matches Snowflake format: no spaces after commas.
pub(crate) fn format_json_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// Collect TABLE property rows from the definition.
///
/// Each table alias emits: `BASE_TABLE_DATABASE_NAME`, `BASE_TABLE_SCHEMA_NAME`,
/// `BASE_TABLE_NAME`, and optionally `PRIMARY_KEY` (only when non-empty).
fn collect_table_rows(def: &SemanticViewDefinition, rows: &mut Vec<DescribeRow>) {
    let db_name = def.database_name.clone().unwrap_or_default();
    let sch_name = def.schema_name.clone().unwrap_or_default();

    for table in &def.tables {
        let obj_name = table.table.clone();

        rows.push(DescribeRow {
            object_kind: "TABLE".to_string(),
            object_name: obj_name.clone(),
            parent_entity: String::new(),
            property: "BASE_TABLE_DATABASE_NAME".to_string(),
            property_value: db_name.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "TABLE".to_string(),
            object_name: obj_name.clone(),
            parent_entity: String::new(),
            property: "BASE_TABLE_SCHEMA_NAME".to_string(),
            property_value: sch_name.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "TABLE".to_string(),
            object_name: obj_name.clone(),
            parent_entity: String::new(),
            property: "BASE_TABLE_NAME".to_string(),
            property_value: table.table.clone(),
        });
        if !table.pk_columns.is_empty() {
            rows.push(DescribeRow {
                object_kind: "TABLE".to_string(),
                object_name: obj_name.clone(),
                parent_entity: String::new(),
                property: "PRIMARY_KEY".to_string(),
                property_value: format_json_array(&table.pk_columns),
            });
        }
        if let Some(ref comment) = table.comment {
            rows.push(DescribeRow {
                object_kind: "TABLE".to_string(),
                object_name: obj_name.clone(),
                parent_entity: String::new(),
                property: "COMMENT".to_string(),
                property_value: comment.clone(),
            });
        }
        if !table.synonyms.is_empty() {
            rows.push(DescribeRow {
                object_kind: "TABLE".to_string(),
                object_name: obj_name,
                parent_entity: String::new(),
                property: "SYNONYMS".to_string(),
                property_value: format_json_array(&table.synonyms),
            });
        }
    }
}

/// Collect RELATIONSHIP property rows from the definition.
///
/// Each named join emits: `TABLE`, `REF_TABLE`, `FOREIGN_KEY`, `REF_KEY`.
/// Unnamed/legacy joins are skipped.
fn collect_relationship_rows(
    def: &SemanticViewDefinition,
    alias_map: &HashMap<String, String>,
    rows: &mut Vec<DescribeRow>,
) {
    for join in &def.joins {
        let rel_name = match &join.name {
            Some(n) => n.clone(),
            None => continue, // skip unnamed legacy joins
        };
        if join.from_alias.is_empty() {
            continue; // skip legacy joins without from_alias
        }
        let from_table = alias_map
            .get(&join.from_alias)
            .cloned()
            .unwrap_or_else(|| join.from_alias.clone());
        let ref_table = alias_map
            .get(&join.table)
            .cloned()
            .unwrap_or_else(|| join.table.clone());

        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_table.clone(),
            property: "TABLE".to_string(),
            property_value: from_table.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_table.clone(),
            property: "REF_TABLE".to_string(),
            property_value: ref_table,
        });
        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_table.clone(),
            property: "FOREIGN_KEY".to_string(),
            property_value: format_json_array(&join.fk_columns),
        });
        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name,
            parent_entity: from_table,
            property: "REF_KEY".to_string(),
            property_value: format_json_array(&join.ref_columns),
        });
    }
}

/// Collect FACT property rows from the definition.
///
/// Each fact emits: `TABLE`, `EXPRESSION`, `DATA_TYPE`.
fn collect_fact_rows(
    def: &SemanticViewDefinition,
    base_table: &str,
    alias_map: &HashMap<String, String>,
    rows: &mut Vec<DescribeRow>,
) {
    for fact in &def.facts {
        let parent = fact
            .source_table
            .as_ref()
            .and_then(|a| alias_map.get(a).cloned())
            .unwrap_or_else(|| base_table.to_string());

        rows.push(DescribeRow {
            object_kind: "FACT".to_string(),
            object_name: fact.name.clone(),
            parent_entity: parent.clone(),
            property: "TABLE".to_string(),
            property_value: parent.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "FACT".to_string(),
            object_name: fact.name.clone(),
            parent_entity: parent.clone(),
            property: "EXPRESSION".to_string(),
            property_value: fact.expr.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "FACT".to_string(),
            object_name: fact.name.clone(),
            parent_entity: parent.clone(),
            property: "DATA_TYPE".to_string(),
            property_value: fact.output_type.clone().unwrap_or_default(),
        });
        if let Some(ref comment) = fact.comment {
            rows.push(DescribeRow {
                object_kind: "FACT".to_string(),
                object_name: fact.name.clone(),
                parent_entity: parent.clone(),
                property: "COMMENT".to_string(),
                property_value: comment.clone(),
            });
        }
        if !fact.synonyms.is_empty() {
            rows.push(DescribeRow {
                object_kind: "FACT".to_string(),
                object_name: fact.name.clone(),
                parent_entity: parent.clone(),
                property: "SYNONYMS".to_string(),
                property_value: format_json_array(&fact.synonyms),
            });
        }
        rows.push(DescribeRow {
            object_kind: "FACT".to_string(),
            object_name: fact.name.clone(),
            parent_entity: parent,
            property: "ACCESS_MODIFIER".to_string(),
            property_value: match fact.access {
                AccessModifier::Public => "PUBLIC".to_string(),
                AccessModifier::Private => "PRIVATE".to_string(),
            },
        });
    }
}

/// Collect DIMENSION property rows from the definition.
///
/// Each dimension emits: `TABLE`, `EXPRESSION`, `DATA_TYPE`.
fn collect_dimension_rows(
    def: &SemanticViewDefinition,
    base_table: &str,
    alias_map: &HashMap<String, String>,
    rows: &mut Vec<DescribeRow>,
) {
    for dim in &def.dimensions {
        let parent = dim
            .source_table
            .as_ref()
            .and_then(|a| alias_map.get(a).cloned())
            .unwrap_or_else(|| base_table.to_string());

        rows.push(DescribeRow {
            object_kind: "DIMENSION".to_string(),
            object_name: dim.name.clone(),
            parent_entity: parent.clone(),
            property: "TABLE".to_string(),
            property_value: parent.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "DIMENSION".to_string(),
            object_name: dim.name.clone(),
            parent_entity: parent.clone(),
            property: "EXPRESSION".to_string(),
            property_value: dim.expr.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "DIMENSION".to_string(),
            object_name: dim.name.clone(),
            parent_entity: parent.clone(),
            property: "DATA_TYPE".to_string(),
            property_value: dim.output_type.clone().unwrap_or_default(),
        });
        if let Some(ref comment) = dim.comment {
            rows.push(DescribeRow {
                object_kind: "DIMENSION".to_string(),
                object_name: dim.name.clone(),
                parent_entity: parent.clone(),
                property: "COMMENT".to_string(),
                property_value: comment.clone(),
            });
        }
        if !dim.synonyms.is_empty() {
            rows.push(DescribeRow {
                object_kind: "DIMENSION".to_string(),
                object_name: dim.name.clone(),
                parent_entity: parent,
                property: "SYNONYMS".to_string(),
                property_value: format_json_array(&dim.synonyms),
            });
        }
    }
}

/// Collect METRIC and DERIVED_METRIC property rows from the definition.
///
/// Metrics with `source_table: Some(...)` emit as METRIC (TABLE, EXPRESSION, DATA_TYPE).
/// Metrics with `source_table: None` emit as DERIVED_METRIC (EXPRESSION, DATA_TYPE only).
fn collect_metric_rows(
    def: &SemanticViewDefinition,
    base_table: &str,
    alias_map: &HashMap<String, String>,
    rows: &mut Vec<DescribeRow>,
) {
    for metric in &def.metrics {
        let is_derived = metric.source_table.is_none();
        let object_kind = if is_derived {
            "DERIVED_METRIC"
        } else {
            "METRIC"
        };
        let parent = if is_derived {
            String::new()
        } else {
            metric
                .source_table
                .as_ref()
                .and_then(|a| alias_map.get(a).cloned())
                .unwrap_or_else(|| base_table.to_string())
        };

        if !is_derived {
            rows.push(DescribeRow {
                object_kind: object_kind.to_string(),
                object_name: metric.name.clone(),
                parent_entity: parent.clone(),
                property: "TABLE".to_string(),
                property_value: parent.clone(),
            });
        }
        rows.push(DescribeRow {
            object_kind: object_kind.to_string(),
            object_name: metric.name.clone(),
            parent_entity: parent.clone(),
            property: "EXPRESSION".to_string(),
            property_value: metric.expr.clone(),
        });
        rows.push(DescribeRow {
            object_kind: object_kind.to_string(),
            object_name: metric.name.clone(),
            parent_entity: parent.clone(),
            property: "DATA_TYPE".to_string(),
            property_value: metric.output_type.clone().unwrap_or_default(),
        });
        if let Some(ref comment) = metric.comment {
            rows.push(DescribeRow {
                object_kind: object_kind.to_string(),
                object_name: metric.name.clone(),
                parent_entity: parent.clone(),
                property: "COMMENT".to_string(),
                property_value: comment.clone(),
            });
        }
        if !metric.synonyms.is_empty() {
            rows.push(DescribeRow {
                object_kind: object_kind.to_string(),
                object_name: metric.name.clone(),
                parent_entity: parent.clone(),
                property: "SYNONYMS".to_string(),
                property_value: format_json_array(&metric.synonyms),
            });
        }
        rows.push(DescribeRow {
            object_kind: object_kind.to_string(),
            object_name: metric.name.clone(),
            parent_entity: parent.clone(),
            property: "ACCESS_MODIFIER".to_string(),
            property_value: match metric.access {
                AccessModifier::Public => "PUBLIC".to_string(),
                AccessModifier::Private => "PRIVATE".to_string(),
            },
        });
        if !metric.non_additive_by.is_empty() {
            let na_value = metric
                .non_additive_by
                .iter()
                .map(|na| {
                    let mut s = na.dimension.clone();
                    match na.order {
                        crate::model::SortOrder::Desc => s.push_str(" DESC"),
                        crate::model::SortOrder::Asc => {}
                    }
                    match na.nulls {
                        crate::model::NullsOrder::First => s.push_str(" NULLS FIRST"),
                        crate::model::NullsOrder::Last => {}
                    }
                    s
                })
                .collect::<Vec<_>>()
                .join(", ");
            rows.push(DescribeRow {
                object_kind: object_kind.to_string(),
                object_name: metric.name.clone(),
                parent_entity: parent.clone(),
                property: "NON_ADDITIVE_BY".to_string(),
                property_value: na_value,
            });
        }
        if let Some(ref ws) = metric.window_spec {
            let mut ws_value = format!("{}({})", ws.window_function, ws.inner_metric);
            if !ws.extra_args.is_empty() {
                // Rewrite to include extra args: e.g., LAG(metric, 30)
                ws_value = format!(
                    "{}({}, {})",
                    ws.window_function,
                    ws.inner_metric,
                    ws.extra_args.join(", ")
                );
            }
            ws_value.push_str(" OVER (");
            let has_partition = if !ws.excluding_dims.is_empty() {
                ws_value.push_str("PARTITION BY EXCLUDING ");
                ws_value.push_str(&ws.excluding_dims.join(", "));
                true
            } else if !ws.partition_dims.is_empty() {
                ws_value.push_str("PARTITION BY ");
                ws_value.push_str(&ws.partition_dims.join(", "));
                true
            } else {
                false
            };
            if !ws.order_by.is_empty() {
                if has_partition {
                    ws_value.push(' ');
                }
                ws_value.push_str("ORDER BY ");
                let ob_strs: Vec<String> = ws
                    .order_by
                    .iter()
                    .map(|ob| {
                        let mut s = ob.expr.clone();
                        match ob.order {
                            crate::model::SortOrder::Desc => s.push_str(" DESC"),
                            crate::model::SortOrder::Asc => {}
                        }
                        match ob.nulls {
                            crate::model::NullsOrder::First => s.push_str(" NULLS FIRST"),
                            crate::model::NullsOrder::Last => {}
                        }
                        s
                    })
                    .collect();
                ws_value.push_str(&ob_strs.join(", "));
            }
            ws_value.push(')');
            rows.push(DescribeRow {
                object_kind: object_kind.to_string(),
                object_name: metric.name.clone(),
                parent_entity: parent,
                property: "WINDOW_SPEC".to_string(),
                property_value: ws_value,
            });
        }
    }
}

/// Collect MATERIALIZATION property rows from the definition.
///
/// Each materialization emits three rows: TABLE, DIMENSIONS, METRICS.
/// When `def.materializations` is empty, no rows are added.
fn collect_materialization_rows(def: &SemanticViewDefinition, rows: &mut Vec<DescribeRow>) {
    for mat in &def.materializations {
        rows.push(DescribeRow {
            object_kind: "MATERIALIZATION".to_string(),
            object_name: mat.name.clone(),
            parent_entity: String::new(),
            property: "TABLE".to_string(),
            property_value: mat.table.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "MATERIALIZATION".to_string(),
            object_name: mat.name.clone(),
            parent_entity: String::new(),
            property: "DIMENSIONS".to_string(),
            property_value: format_json_array(&mat.dimensions),
        });
        rows.push(DescribeRow {
            object_kind: "MATERIALIZATION".to_string(),
            object_name: mat.name.clone(),
            parent_entity: String::new(),
            property: "METRICS".to_string(),
            property_value: format_json_array(&mat.metrics),
        });
    }
}

// Legacy `DescribeSemanticViewVTab` (duckdb-rs `VTab` impl block) RETIRED —
// Phase 65 Plan 05 Batch 3. The C++ Catalog API path
// (`sv_register_describe_semantic_view` → `sv_describe_semantic_view_bind_rust`
// above) is the sole registration target.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_json_array_single() {
        assert_eq!(format_json_array(&["id".to_string()]), r#"["id"]"#);
    }

    #[test]
    fn format_json_array_multiple() {
        let cols = vec!["first_name".to_string(), "last_name".to_string()];
        assert_eq!(format_json_array(&cols), r#"["first_name","last_name"]"#);
    }

    #[test]
    fn format_json_array_empty() {
        let cols: Vec<String> = vec![];
        assert_eq!(format_json_array(&cols), "[]");
    }

    #[test]
    fn window_spec_property_row_emitted() {
        use crate::model::{
            AccessModifier, Dimension, Metric, NullsOrder, SortOrder, TableRef, WindowOrderBy,
            WindowSpec,
        };
        let def = SemanticViewDefinition {
            tables: vec![TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            }],
            dimensions: vec![Dimension {
                name: "region".to_string(),
                expr: "o.region".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            }],
            metrics: vec![
                Metric {
                    name: "total_qty".to_string(),
                    expr: "SUM(o.qty)".to_string(),
                    source_table: Some("o".to_string()),
                    access: AccessModifier::Public,
                    ..Default::default()
                },
                Metric {
                    name: "avg_qty".to_string(),
                    expr: "AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY region)"
                        .to_string(),
                    source_table: Some("o".to_string()),
                    access: AccessModifier::Public,
                    window_spec: Some(WindowSpec {
                        window_function: "AVG".to_string(),
                        inner_metric: "total_qty".to_string(),
                        extra_args: vec![],
                        excluding_dims: vec!["region".to_string()],
                        partition_dims: vec![],
                        order_by: vec![WindowOrderBy {
                            expr: "region".to_string(),
                            order: SortOrder::Desc,
                            nulls: NullsOrder::First,
                        }],
                        frame_clause: None,
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let alias_map = def.alias_to_table_map();
        let mut rows = Vec::new();
        collect_metric_rows(&def, "orders", &alias_map, &mut rows);

        // Find the WINDOW_SPEC row
        let ws_row = rows
            .iter()
            .find(|r| r.property == "WINDOW_SPEC")
            .expect("Should have WINDOW_SPEC row");
        assert_eq!(ws_row.object_name, "avg_qty");
        assert!(
            ws_row
                .property_value
                .contains("AVG(total_qty) OVER (PARTITION BY EXCLUDING region"),
            "WINDOW_SPEC value should contain parsed spec: {}",
            ws_row.property_value
        );

        // Regular metric should NOT have WINDOW_SPEC
        let total_rows: Vec<&DescribeRow> = rows
            .iter()
            .filter(|r| r.object_name == "total_qty" && r.property == "WINDOW_SPEC")
            .collect();
        assert!(
            total_rows.is_empty(),
            "Regular metric should not have WINDOW_SPEC row"
        );
    }
}
