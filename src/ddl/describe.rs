use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use std::collections::HashMap;

use crate::catalog::CatalogState;
use crate::model::{AccessModifier, SemanticViewDefinition};

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

/// Bind-time data for `describe_semantic_view`: pre-collected property rows.
pub struct DescribeBindData {
    rows: Vec<DescribeRow>,
}

// SAFETY: all fields are owned `String` inside `Vec`, which is `Send + Sync`.
unsafe impl Send for DescribeBindData {}
unsafe impl Sync for DescribeBindData {}

/// Init data for `describe_semantic_view`: tracks whether rows have been emitted.
pub struct DescribeInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for DescribeInitData {}
unsafe impl Sync for DescribeInitData {}

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

    // Handle legacy definitions with empty tables vec.
    if def.tables.is_empty() {
        let obj_name = def.base_table.clone();
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
            object_name: obj_name,
            parent_entity: String::new(),
            property: "BASE_TABLE_NAME".to_string(),
            property_value: def.base_table.clone(),
        });
        return;
    }

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

/// Table function: Snowflake-aligned DESCRIBE SEMANTIC VIEW.
///
/// Returns property-per-row output with 5 VARCHAR columns:
///   `(object_kind, object_name, parent_entity, property, property_value)`
///
/// Takes one positional VARCHAR parameter: the view name.
pub struct DescribeSemanticViewVTab;

impl VTab for DescribeSemanticViewVTab {
    type BindData = DescribeBindData;
    type InitData = DescribeInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            // Declare 5 output columns — all VARCHAR.
            bind.add_result_column(
                "object_kind",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "object_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "parent_entity",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("property", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column(
                "property_value",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );

            // Read the name parameter.
            let name = bind.get_parameter(0).to_string();

            // Access the shared catalog state injected via extra_info.
            let state_ptr = bind.get_extra_info::<CatalogState>();
            let guard = unsafe {
                (*state_ptr)
                    .read()
                    .map_err(|_| Box::<dyn std::error::Error>::from("catalog lock poisoned"))?
            };

            let json_str = guard
                .get(&name)
                .ok_or_else(|| format!("semantic view '{name}' does not exist"))?;

            // Parse the stored JSON into the model.
            let def = SemanticViewDefinition::from_json(&name, json_str)?;

            // Build alias->table map and compute base table name.
            let alias_map = def.alias_to_table_map();
            let base_table = def
                .tables
                .first()
                .map(|t| t.table.clone())
                .unwrap_or_else(|| def.base_table.clone());

            // Collect rows in definition order.
            let mut rows = Vec::new();
            if let Some(ref comment) = def.comment {
                rows.push(DescribeRow {
                    object_kind: String::new(),
                    object_name: String::new(),
                    parent_entity: String::new(),
                    property: "COMMENT".to_string(),
                    property_value: comment.clone(),
                });
            }
            collect_table_rows(&def, &mut rows);
            collect_relationship_rows(&def, &alias_map, &mut rows);
            collect_fact_rows(&def, &base_table, &alias_map, &mut rows);
            collect_dimension_rows(&def, &base_table, &alias_map, &mut rows);
            collect_metric_rows(&def, &base_table, &alias_map, &mut rows);

            Ok(DescribeBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(DescribeInitData {
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

        let kind_vec = output.flat_vector(0);
        let name_vec = output.flat_vector(1);
        let parent_vec = output.flat_vector(2);
        let prop_vec = output.flat_vector(3);
        let val_vec = output.flat_vector(4);

        for (i, row) in bind_data.rows.iter().enumerate() {
            kind_vec.insert(i, row.object_kind.as_str());
            name_vec.insert(i, row.object_name.as_str());
            parent_vec.insert(i, row.parent_entity.as_str());
            prop_vec.insert(i, row.property.as_str());
            val_vec.insert(i, row.property_value.as_str());
        }

        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}

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
            base_table: "orders".to_string(),
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
