use serde::{Deserialize, Serialize};

/// A table alias entry for the `tables` DDL parameter.
/// Maps a short alias (e.g., `"o"`) to a physical table name (e.g., `"orders"`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct TableRef {
    pub alias: String,
    pub table: String,
    /// Primary key columns for this table (Phase 24: PK/FK model).
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pk_columns: Vec<String>,
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
    /// Optional user-declared output type for this dimension column.
    /// When set, the generated SQL wraps the expression in `CAST(expr AS <type>)`
    /// AND declares the output column as this type in `bind()`.
    /// If None, the inferred or fallback type is used.
    #[serde(default)]
    pub output_type: Option<String>,
}

/// A named aggregation expression used as a metric.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Metric {
    pub name: String,
    pub expr: String,
    /// Optional source table — declares which join table this metric comes from.
    /// If `None`, the metric is assumed to come from the base table.
    #[serde(default)]
    pub source_table: Option<String>,
    /// Optional user-declared output type for this metric column.
    /// When set, the generated SQL wraps the expression in `CAST(expr AS <type>)`
    /// AND declares the output column as this type in `bind()`.
    /// If None, the inferred or fallback type is used.
    #[serde(default)]
    pub output_type: Option<String>,
    /// Phase 32: Named relationships that this metric traverses.
    /// When non-empty, the expansion engine uses these relationship names
    /// to resolve which join path to follow (role-playing dimensions).
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub using_relationships: Vec<String>,
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

/// A named drill-down path through declared dimensions.
/// Added in Phase 29 for the HIERARCHIES clause of CREATE SEMANTIC VIEW.
///
/// Example: `geo AS (country, state, city)` defines a hierarchy named "geo"
/// whose levels are existing dimension names from coarsest to finest granularity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Hierarchy {
    pub name: String,
    pub levels: Vec<String>,
}

/// A column-pair relationship entry for composite or single FK declarations.
/// Used in the `relationships` DDL parameter's `join_columns` field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct JoinColumn {
    pub from: String,
    pub to: String,
}

/// Cardinality of a relationship between two tables.
///
/// Declared after `REFERENCES <alias>` in the RELATIONSHIPS clause.
/// Defaults to `ManyToOne` when omitted (most common FK pattern).
/// Old stored JSON without a `cardinality` field deserializes as `ManyToOne`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Cardinality {
    #[default]
    ManyToOne,
    OneToOne,
    OneToMany,
}

impl Cardinality {
    /// Returns `true` when the variant is the default (`ManyToOne`).
    /// Used by `serde(skip_serializing_if)` to omit the field from JSON
    /// when it matches the default, preserving backward-compatible output.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::ManyToOne)
    }
}

/// A JOIN relationship between the base table and another source table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Phase 24: The source table alias from which FK columns are defined.
    /// In `order_to_customer AS o(customer_id) REFERENCES c`, this is `"o"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_alias: String,
    /// Phase 24: FK column names from the source alias (`from_alias`) side.
    /// In `order_to_customer AS o(customer_id) REFERENCES c`, this is `["customer_id"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fk_columns: Vec<String>,
    /// Phase 24: Optional relationship name for multi-table FK declarations.
    /// In `order_to_customer AS o(customer_id) REFERENCES c`, this is `Some("order_to_customer")`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Phase 31: Cardinality of this relationship.
    /// Defaults to `ManyToOne` when omitted in DDL (most common FK pattern).
    /// Old stored JSON without this field deserializes as `ManyToOne`.
    /// Not serialized when `ManyToOne` to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Cardinality::is_default")]
    pub cardinality: Cardinality,
}

/// Top-level definition of a semantic view.
///
/// Stored as JSON in `semantic_layer._definitions`.
/// Required fields: `base_table`, `dimensions`, `metrics`.
/// Optional fields: `filters` (defaults to []), `joins` (defaults to []), `facts` (defaults to []).
/// Note: `deny_unknown_fields` is intentionally NOT set — old stored JSON with extra
/// fields (e.g., from future schema changes) must still load without error.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Phase 29: named drill-down paths through dimensions.
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hierarchies: Vec<Hierarchy>,
    /// Column names from DDL-time LIMIT 0 inference, parallel to `column_types_inferred`.
    /// Populated by `create_semantic_view` `invoke()`. Empty = no inference ran.
    /// Used by `bind()` to build a name→type map for subquery column lookups.
    #[serde(default)]
    pub column_type_names: Vec<String>,
    /// DDL-time inferred column types stored as `ffi::duckdb_type` (u32) values.
    /// Populated by `create_semantic_view` `invoke()` after running LIMIT 0.
    /// Parallel to `column_type_names`: `column_type_names[i]` ↔ `column_types_inferred[i]`.
    /// Empty vec = no inference ran (in-memory DB or inference failed) → VARCHAR fallback.
    /// `bind()` builds a name→type `HashMap` from both vecs to look up requested columns by name.
    #[serde(default)]
    pub column_types_inferred: Vec<u32>,
}

impl SemanticViewDefinition {
    /// Parse and validate a JSON string, returning a typed definition.
    ///
    /// Returns an error if the JSON is invalid or missing required fields.
    ///
    /// The `name` parameter is used only in the error message for context.
    pub fn from_json(name: &str, json: &str) -> Result<Self, String> {
        let def: Self = serde_json::from_str(json)
            .map_err(|e| format!("invalid definition for semantic view '{name}': {e}"))?;
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
                ..Default::default()
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
                    ..Default::default()
                }],
                dimensions: vec![],
                metrics: vec![],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                hierarchies: vec![],
                column_type_names: vec![],
                column_types_inferred: vec![],
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

    mod phase29_hierarchy_tests {
        use super::*;

        #[test]
        fn hierarchy_roundtrip_serialization() {
            let h = Hierarchy {
                name: "geo".to_string(),
                levels: vec![
                    "country".to_string(),
                    "state".to_string(),
                    "city".to_string(),
                ],
            };
            let json = serde_json::to_string(&h).unwrap();
            let rt: Hierarchy = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.name, "geo");
            assert_eq!(rt.levels, vec!["country", "state", "city"]);
        }

        #[test]
        fn definition_with_hierarchies_roundtrips() {
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "country", "expr": "country"}],
                "metrics": [],
                "hierarchies": [{"name": "geo", "levels": ["country", "state", "city"]}]
            }"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert_eq!(def.hierarchies.len(), 1);
            assert_eq!(def.hierarchies[0].name, "geo");
            assert_eq!(def.hierarchies[0].levels, vec!["country", "state", "city"]);

            // Roundtrip: serialize and re-parse
            let re_json = serde_json::to_string(&def).unwrap();
            let rt = SemanticViewDefinition::from_json("orders", &re_json).unwrap();
            assert_eq!(rt.hierarchies.len(), 1);
        }

        #[test]
        fn old_json_without_hierarchies_deserializes_with_empty_vec() {
            let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert!(
                def.hierarchies.is_empty(),
                "hierarchies should default to [] for old JSON without hierarchies field"
            );
        }

        #[test]
        fn definition_without_hierarchies_does_not_serialize_field() {
            // skip_serializing_if = "Vec::is_empty" means no hierarchies key in output
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![],
                metrics: vec![],
                ..Default::default()
            };
            let json = serde_json::to_string(&def).unwrap();
            assert!(
                !json.contains("hierarchies"),
                "Empty hierarchies should be omitted from JSON: {json}"
            );
        }
    }

    mod phase31_cardinality_tests {
        use super::*;

        #[test]
        fn cardinality_serde_roundtrip() {
            // All three variants serialize and deserialize correctly
            for (variant, expected_json) in [
                (Cardinality::ManyToOne, r#""ManyToOne""#),
                (Cardinality::OneToOne, r#""OneToOne""#),
                (Cardinality::OneToMany, r#""OneToMany""#),
            ] {
                let json = serde_json::to_string(&variant).unwrap();
                assert_eq!(json, expected_json);
                let rt: Cardinality = serde_json::from_str(&json).unwrap();
                assert_eq!(rt, variant);
            }
        }

        #[test]
        fn join_with_cardinality_roundtrip() {
            let join = Join {
                table: "customers".to_string(),
                from_alias: "o".to_string(),
                fk_columns: vec!["customer_id".to_string()],
                name: Some("order_to_customer".to_string()),
                cardinality: Cardinality::OneToMany,
                ..Default::default()
            };
            let json = serde_json::to_string(&join).unwrap();
            assert!(json.contains(r#""cardinality":"OneToMany""#));
            let rt: Join = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.cardinality, Cardinality::OneToMany);
        }

        #[test]
        fn old_json_without_cardinality_defaults_to_many_to_one() {
            // Backward compat: old JSON without cardinality field
            let json = r#"{"table":"customers","on":"a.id=b.id"}"#;
            let join: Join = serde_json::from_str(json).unwrap();
            assert_eq!(
                join.cardinality,
                Cardinality::ManyToOne,
                "Missing cardinality must default to ManyToOne"
            );
        }

        #[test]
        fn definition_with_cardinality_joins_roundtrips() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![],
                metrics: vec![],
                joins: vec![Join {
                    table: "c".to_string(),
                    from_alias: "o".to_string(),
                    fk_columns: vec!["customer_id".to_string()],
                    name: Some("order_to_customer".to_string()),
                    cardinality: Cardinality::OneToOne,
                    ..Default::default()
                }],
                ..Default::default()
            };
            let json = serde_json::to_string(&def).unwrap();
            let rt = SemanticViewDefinition::from_json("orders", &json).unwrap();
            assert_eq!(rt.joins.len(), 1);
            assert_eq!(rt.joins[0].cardinality, Cardinality::OneToOne);
        }
    }

    mod phase32_using_relationships_tests {
        use super::*;

        #[test]
        fn metric_with_using_relationships_roundtrips() {
            let met = Metric {
                name: "departure_count".to_string(),
                expr: "COUNT(*)".to_string(),
                source_table: Some("f".to_string()),
                output_type: None,
                using_relationships: vec!["dep_airport".to_string()],
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(json.contains("using_relationships"));
            let rt: Metric = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.using_relationships, vec!["dep_airport"]);
        }

        #[test]
        fn old_json_without_using_relationships_deserializes_with_empty_vec() {
            // Backward compat: Phase 30 definitions don't have using_relationships
            let json = r#"{"name":"revenue","expr":"SUM(amount)","source_table":"o"}"#;
            let met: Metric = serde_json::from_str(json).unwrap();
            assert!(
                met.using_relationships.is_empty(),
                "using_relationships should default to [] for old JSON"
            );
        }

        #[test]
        fn metric_with_empty_using_relationships_does_not_emit_field() {
            // skip_serializing_if = "Vec::is_empty" means no using_relationships key in output
            let met = Metric {
                name: "revenue".to_string(),
                expr: "SUM(amount)".to_string(),
                source_table: Some("o".to_string()),
                output_type: None,
                using_relationships: vec![],
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(
                !json.contains("using_relationships"),
                "Empty using_relationships should be omitted from JSON: {json}"
            );
        }
    }

    mod phase12_model_tests {
        use super::*;

        #[test]
        fn output_type_on_dimension_roundtrips() {
            let dim = Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                source_table: None,
                output_type: Some("BIGINT".to_string()),
            };
            let json = serde_json::to_string(&dim).unwrap();
            let rt: Dimension = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.output_type.as_deref(), Some("BIGINT"));
        }

        #[test]
        fn output_type_on_metric_roundtrips() {
            let met = Metric {
                name: "revenue".to_string(),
                expr: "sum(amount)".to_string(),
                source_table: None,
                output_type: Some("DOUBLE".to_string()),
                using_relationships: vec![],
            };
            let json = serde_json::to_string(&met).unwrap();
            let rt: Metric = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.output_type.as_deref(), Some("DOUBLE"));
        }

        #[test]
        fn column_types_inferred_roundtrips() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                tables: vec![],
                dimensions: vec![],
                metrics: vec![],
                filters: vec![],
                joins: vec![],
                facts: vec![],
                hierarchies: vec![],
                column_type_names: vec!["region".to_string(), "revenue".to_string()],
                column_types_inferred: vec![17u32, 20u32],
            };
            let json = serde_json::to_string(&def).unwrap();
            let rt: SemanticViewDefinition = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.column_type_names, vec!["region", "revenue"]);
            assert_eq!(rt.column_types_inferred, vec![17u32, 20u32]);
        }

        #[test]
        fn old_json_without_output_type_deserializes() {
            // Old JSON without output_type field — must deserialize to None
            let json = r#"{
                "base_table": "orders",
                "dimensions": [{"name": "region", "expr": "region"}],
                "metrics": [{"name": "revenue", "expr": "sum(amount)"}]
            }"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert!(
                def.dimensions[0].output_type.is_none(),
                "output_type should default to None"
            );
            assert!(
                def.metrics[0].output_type.is_none(),
                "output_type should default to None"
            );
        }

        #[test]
        fn old_json_without_column_types_inferred_deserializes() {
            // Old JSON without column_type_names or column_types_inferred — must succeed with empty vecs
            let json = r#"{"base_table": "orders", "dimensions": [], "metrics": []}"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert!(
                def.column_type_names.is_empty(),
                "column_type_names should default to []"
            );
            assert!(
                def.column_types_inferred.is_empty(),
                "column_types_inferred should default to []"
            );
        }
    }
}
