use serde::{Deserialize, Serialize};

/// A table alias entry for the `tables` DDL parameter.
/// Maps a short alias (e.g., `"o"`) to a physical table name (e.g., `"orders"`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct TableRef {
    pub alias: String,
    pub table: String,
}

/// A named SQL column expression used as a dimension.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Dimension {
    pub name: String,
    pub expr: String,
    /// Optional source table — declares which join table this dimension comes from.
    /// If `None`, the dimension is assumed to come from the base table.
    #[serde(default)]
    pub source_table: Option<String>,
    /// Optional dimension type. Only `"time"` is supported in v0.2.0.
    /// Serde rename required because `type` is a Rust keyword.
    #[serde(default, rename = "type")]
    pub dim_type: Option<String>,
    /// Required when `dim_type` is `Some("time")`.
    /// Valid values: `"day"`, `"week"`, `"month"`, `"year"`.
    #[serde(default)]
    pub granularity: Option<String>,
}

/// A named aggregation expression used as a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Metric {
    pub name: String,
    pub expr: String,
    /// Optional source table — declares which join table this metric comes from.
    /// If `None`, the metric is assumed to come from the base table.
    #[serde(default)]
    pub source_table: Option<String>,
}

/// A named raw SQL column expression — a pre-aggregation fact, scoped to a table alias.
/// Added in Phase 11 for the FACTS clause of CREATE SEMANTIC VIEW.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Fact {
    pub name: String,
    pub expr: String,
    /// Which table alias this fact is scoped to.
    #[serde(default)]
    pub source_table: Option<String>,
}

/// A column-pair relationship entry for composite or single FK declarations.
/// Used in the `relationships` DDL parameter's `join_columns` field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct JoinColumn {
    pub from: String,
    pub to: String,
}

/// A JOIN relationship between the base table and another source table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Join {
    pub table: String,
    /// Legacy field (Phase 10 and earlier): raw SQL ON clause.
    /// Kept for backward compat with stored JSON. Not written by Phase 11 DDL.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,
    /// New field (Phase 11): FK column names from this table to the base table.
    /// Set by CREATE SEMANTIC VIEW RELATIONSHIPS clause.
    #[serde(default)]
    pub from_cols: Vec<String>,
    /// Phase 11.1: column-pair FK declarations. Replaces `from_cols` for new definitions.
    /// Old stored JSON without this field deserializes with empty Vec.
    #[serde(default)]
    pub join_columns: Vec<JoinColumn>,
}

/// Top-level definition of a semantic view.
///
/// Stored as JSON in `semantic_layer._definitions`.
/// Required fields: `base_table`, `dimensions`, `metrics`.
/// Optional fields: `filters` (defaults to []), `joins` (defaults to []), `facts` (defaults to []).
/// Note: `deny_unknown_fields` is intentionally NOT set — old stored JSON with extra
/// fields (e.g., from future schema changes) must still load without error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SemanticViewDefinition {
    pub base_table: String,
    /// Phase 11.1: table alias registry for multi-table views.
    /// Old stored JSON without this field deserializes with empty Vec.
    #[serde(default)]
    pub tables: Vec<TableRef>,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default)]
    pub joins: Vec<Join>,
    #[serde(default)]
    pub facts: Vec<Fact>,
}

impl SemanticViewDefinition {
    /// Parse and validate a JSON string, returning a typed definition.
    ///
    /// Returns an error if the JSON is invalid, missing required fields, or contains
    /// invalid time dimension declarations (unknown type, missing granularity, or
    /// unsupported granularity value).
    ///
    /// The `name` parameter is used only in the error message for context.
    pub fn from_json(name: &str, json: &str) -> Result<Self, String> {
        const VALID_GRANULARITIES: &[&str] = &["day", "week", "month", "year"];

        let def: Self = serde_json::from_str(json)
            .map_err(|e| format!("invalid definition for semantic view '{name}': {e}"))?;

        for dim in &def.dimensions {
            if let Some(ref dt) = dim.dim_type {
                if dt != "time" {
                    return Err(format!(
                        "dimension '{}' has unknown type '{}'; only 'time' is supported",
                        dim.name, dt
                    ));
                }
                match &dim.granularity {
                    None => {
                        return Err(format!(
                            "dimension '{}' declares type 'time' but is missing required 'granularity' field",
                            dim.name
                        ));
                    }
                    Some(g) if !VALID_GRANULARITIES.contains(&g.as_str()) => {
                        return Err(format!(
                            "dimension '{}' has unsupported granularity '{}'; valid values: {}",
                            dim.name,
                            g,
                            VALID_GRANULARITIES.join(", ")
                        ));
                    }
                    _ => {}
                }
            }
        }

        Ok(def)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_definition_roundtrips() {
        let json = r#"{
            "base_table": "orders",
            "dimensions": [{"name": "region", "expr": "region"}],
            "metrics": [{"name": "revenue", "expr": "sum(amount)"}]
        }"#;
        let def = SemanticViewDefinition::from_json("orders", json).unwrap();
        assert_eq!(def.base_table, "orders");
        assert_eq!(def.dimensions.len(), 1);
        assert_eq!(def.metrics.len(), 1);
        assert!(def.filters.is_empty());
        assert!(def.joins.is_empty());
    }

    #[test]
    fn missing_base_table_is_error() {
        let json = r#"{"dimensions": [], "metrics": []}"#;
        assert!(SemanticViewDefinition::from_json("test", json).is_err());
    }

    #[test]
    fn invalid_json_is_error() {
        assert!(SemanticViewDefinition::from_json("test", "{not json}").is_err());
    }

    #[test]
    fn optional_fields_default_to_empty() {
        let json = r#"{"base_table": "t", "dimensions": [], "metrics": []}"#;
        let def = SemanticViewDefinition::from_json("test", json).unwrap();
        assert!(def.filters.is_empty());
        assert!(def.joins.is_empty());
    }

    #[test]
    fn old_json_without_source_table_deserializes() {
        // Backward compat: Phase 2 definitions don't have source_table.
        let json = r#"{
            "base_table": "orders",
            "dimensions": [{"name": "region", "expr": "region"}],
            "metrics": [{"name": "revenue", "expr": "sum(amount)"}]
        }"#;
        let def = SemanticViewDefinition::from_json("orders", json).unwrap();
        assert!(def.dimensions[0].source_table.is_none());
        assert!(def.metrics[0].source_table.is_none());
    }

    #[test]
    fn json_with_source_table_deserializes() {
        let json = r#"{
            "base_table": "orders",
            "dimensions": [{"name": "customer_name", "expr": "customers.name", "source_table": "customers"}],
            "metrics": [{"name": "revenue", "expr": "sum(amount)", "source_table": "line_items"}]
        }"#;
        let def = SemanticViewDefinition::from_json("orders", json).unwrap();
        assert_eq!(def.dimensions[0].source_table.as_deref(), Some("customers"));
        assert_eq!(def.metrics[0].source_table.as_deref(), Some("line_items"));
    }

    mod time_dimension_tests {
        use super::*;

        #[test]
        fn time_dimension_roundtrip() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "order_date", "expr": "order_date", "type": "time", "granularity": "month"}],
                "metrics": [{"name": "revenue", "expr": "sum(amount)"}]
            }"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert_eq!(def.dimensions[0].dim_type.as_deref(), Some("time"));
            assert_eq!(def.dimensions[0].granularity.as_deref(), Some("month"));
        }

        #[test]
        fn old_json_without_type_deserializes() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "region", "expr": "region"}],
                "metrics": []
            }"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert!(def.dimensions[0].dim_type.is_none());
            assert!(def.dimensions[0].granularity.is_none());
        }

        #[test]
        fn time_dimension_missing_granularity_error() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "order_date", "expr": "order_date", "type": "time"}],
                "metrics": []
            }"#;
            let err = SemanticViewDefinition::from_json("orders", json).unwrap_err();
            assert!(
                err.contains("missing required 'granularity' field"),
                "Got: {err}"
            );
            assert!(err.contains("order_date"), "Got: {err}");
        }

        #[test]
        fn time_dimension_unknown_type_error() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "order_date", "expr": "order_date", "type": "date", "granularity": "month"}],
                "metrics": []
            }"#;
            let err = SemanticViewDefinition::from_json("orders", json).unwrap_err();
            assert!(err.contains("unknown type 'date'"), "Got: {err}");
            assert!(err.contains("only 'time' is supported"), "Got: {err}");
        }

        #[test]
        fn time_dimension_unsupported_granularity_error() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "order_date", "expr": "order_date", "type": "time", "granularity": "quarter"}],
                "metrics": []
            }"#;
            let err = SemanticViewDefinition::from_json("orders", json).unwrap_err();
            assert!(err.contains("'quarter'"), "Got: {err}");
            assert!(err.contains("day, week, month, year"), "Got: {err}");
        }

        #[test]
        fn all_supported_granularities_accepted() {
            for gran in ["day", "week", "month", "year"] {
                let json = format!(
                    r#"{{
                        "base_table": "orders",
                        "dimensions": [{{"name": "order_date", "expr": "order_date", "type": "time", "granularity": "{gran}"}}],
                        "metrics": []
                    }}"#
                );
                SemanticViewDefinition::from_json("orders", &json)
                    .unwrap_or_else(|e| panic!("granularity '{gran}' rejected: {e}"));
            }
        }
    }

    mod phase11_model_tests {
        use super::*;

        #[test]
        fn fact_roundtrip() {
            // Fact with source_table
            let json = r#"{"name":"rev","expr":"amount","source_table":"orders"}"#;
            let fact: Fact = serde_json::from_str(json).unwrap();
            assert_eq!(fact.name, "rev");
            assert_eq!(fact.expr, "amount");
            assert_eq!(fact.source_table.as_deref(), Some("orders"));

            // Fact without source_table — defaults to None
            let json2 = r#"{"name":"total","expr":"price * qty"}"#;
            let fact2: Fact = serde_json::from_str(json2).unwrap();
            assert_eq!(fact2.name, "total");
            assert!(fact2.source_table.is_none());
        }

        #[test]
        fn join_old_format_backwards_compat() {
            // Old Join with `on` field (Phase 10 and earlier format)
            let json = r#"{"table":"customers","on":"a.id=b.id"}"#;
            let join: Join = serde_json::from_str(json).unwrap();
            assert_eq!(join.table, "customers");
            assert_eq!(join.on, "a.id=b.id");
            assert!(join.from_cols.is_empty(), "from_cols should default to []");
        }

        #[test]
        fn join_new_format() {
            // New Join with `from_cols` (Phase 11 format)
            let json = r#"{"table":"customers","from_cols":["customer_id"]}"#;
            let join: Join = serde_json::from_str(json).unwrap();
            assert_eq!(join.table, "customers");
            assert_eq!(join.on, "", "on should default to empty string");
            assert_eq!(join.from_cols, vec!["customer_id"]);
        }

        #[test]
        fn definition_with_facts() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [],
                "metrics": [],
                "facts": [{"name":"unit_price","expr":"amount / qty","source_table":"orders"}]
            }"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert_eq!(def.facts.len(), 1);
            assert_eq!(def.facts[0].name, "unit_price");
            assert_eq!(def.facts[0].expr, "amount / qty");
            assert_eq!(def.facts[0].source_table.as_deref(), Some("orders"));
        }

        #[test]
        fn definition_without_facts_defaults_empty() {
            let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert!(def.facts.is_empty(), "facts should default to []");
        }

        #[test]
        fn unknown_fields_are_allowed() {
            // deny_unknown_fields removed — old stored JSON with extra fields must load
            let json = r#"{"base_table": "t", "dimensions": [], "metrics": [], "extra": 1}"#;
            assert!(
                SemanticViewDefinition::from_json("test", json).is_ok(),
                "unknown fields must not cause rejection after deny_unknown_fields removal"
            );
        }
    }

    mod phase11_1_model_tests {
        use super::*;

        #[test]
        fn table_ref_roundtrip() {
            let tr = TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
            };
            let json = serde_json::to_string(&tr).unwrap();
            assert_eq!(json, r#"{"alias":"o","table":"orders"}"#);
            let rt: TableRef = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.alias, "o");
            assert_eq!(rt.table, "orders");
        }

        #[test]
        fn join_column_roundtrip() {
            let jc = JoinColumn {
                from: "customer_id".to_string(),
                to: "id".to_string(),
            };
            let json = serde_json::to_string(&jc).unwrap();
            assert_eq!(json, r#"{"from":"customer_id","to":"id"}"#);
            let rt: JoinColumn = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.from, "customer_id");
            assert_eq!(rt.to, "id");
        }

        #[test]
        fn join_old_on_format_backwards_compat_with_join_columns_default() {
            // Old Join with only `on` field — join_columns must default to []
            let json = r#"{"table":"customers","on":"a.id=b.id"}"#;
            let join: Join = serde_json::from_str(json).unwrap();
            assert_eq!(join.table, "customers");
            assert_eq!(join.on, "a.id=b.id");
            assert!(
                join.join_columns.is_empty(),
                "join_columns should default to [] for old JSON"
            );
        }

        #[test]
        fn join_new_format_with_join_columns() {
            let json = r#"{"table":"customers","join_columns":[{"from":"customer_id","to":"id"}]}"#;
            let join: Join = serde_json::from_str(json).unwrap();
            assert_eq!(join.table, "customers");
            assert_eq!(join.on, "", "on should default to empty string");
            assert_eq!(join.join_columns.len(), 1);
            assert_eq!(join.join_columns[0].from, "customer_id");
            assert_eq!(join.join_columns[0].to, "id");
        }

        #[test]
        fn semantic_view_definition_with_tables_roundtrip() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![TableRef {
                    alias: "o".to_string(),
                    table: "orders".to_string(),
                }],
                dimensions: vec![],
                metrics: vec![],
                filters: vec![],
                joins: vec![],
                facts: vec![],
            };
            let json = serde_json::to_string(&def).unwrap();
            assert!(
                json.contains(r#""tables":[{"alias":"o","table":"orders"}]"#),
                "tables field must appear in serialized JSON: {json}"
            );
            let rt: SemanticViewDefinition = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.tables.len(), 1);
            assert_eq!(rt.tables[0].alias, "o");
            assert_eq!(rt.tables[0].table, "orders");
        }

        #[test]
        fn old_definition_without_tables_deserializes_with_empty_vec() {
            // Old stored JSON without `tables` field — must load with tables: []
            let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
            let def: SemanticViewDefinition = serde_json::from_str(json).unwrap();
            assert!(
                def.tables.is_empty(),
                "tables should default to [] for old JSON without tables field"
            );
        }
    }
}
