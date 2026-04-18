use serde::{Deserialize, Serialize};

/// A table alias entry for the `tables` DDL parameter.
/// Maps a short alias (e.g., `"o"`) to a physical table name (e.g., `"orders"`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct TableRef {
    pub alias: String,
    pub table: String,
    /// Primary key columns for this table (Phase 24: PK/FK model).
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pk_columns: Vec<String>,
    /// UNIQUE constraints on this table. Each inner Vec is one constraint's column list.
    /// A table can have zero or more UNIQUE constraints (composite allowed).
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_constraints: Vec<Vec<String>>,
    /// Optional human-readable comment for this table entry.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Informational synonyms (aliases) for this table entry.
    /// Old stored JSON without this field deserializes to empty Vec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
}

/// A named SQL column expression used as a dimension.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Optional human-readable comment for this dimension.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Informational synonyms (aliases) for this dimension.
    /// Old stored JSON without this field deserializes to empty Vec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
}

/// Sort order for NON ADDITIVE BY dimension ordering.
/// Default: Asc (matches Snowflake default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    /// Returns `true` when the variant is the default (`Asc`).
    /// Used by `serde(skip_serializing_if)` to omit the field from JSON
    /// when it matches the default, preserving backward-compatible output.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Asc)
    }
}

/// NULLS placement for NON ADDITIVE BY dimension ordering.
/// Default: Last (matches `DuckDB` ASC default and Snowflake ASC default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum NullsOrder {
    #[default]
    Last,
    First,
}

impl NullsOrder {
    /// Returns `true` when the variant is the default (`Last`).
    /// Used by `serde(skip_serializing_if)` to omit the field from JSON
    /// when it matches the default, preserving backward-compatible output.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Last)
    }
}

/// A dimension reference in a NON ADDITIVE BY clause.
/// Specifies which dimension(s) a metric is non-additive by,
/// with sort order and nulls placement for snapshot selection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct NonAdditiveDim {
    pub dimension: String,
    #[serde(default, skip_serializing_if = "SortOrder::is_default")]
    pub order: SortOrder,
    #[serde(default, skip_serializing_if = "NullsOrder::is_default")]
    pub nulls: NullsOrder,
}

/// Parsed window function specification for window metrics.
/// Stored alongside the raw expression for expansion-time rewriting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct WindowSpec {
    /// The window function name (e.g., "AVG", "LAG", "SUM")
    pub window_function: String,
    /// The metric name referenced inside the window function
    pub inner_metric: String,
    /// Additional arguments after the inner metric (e.g., "30" in LAG(metric, 30))
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    /// Dimensions to EXCLUDE from partitioning (PARTITION BY EXCLUDING semantics)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluding_dims: Vec<String>,
    /// Explicit partition dimensions (PARTITION BY semantics, mutually exclusive with `excluding_dims`)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partition_dims: Vec<String>,
    /// ORDER BY clause entries (dimension/expression + direction + nulls)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<WindowOrderBy>,
    /// Raw frame clause (e.g., "RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_clause: Option<String>,
}

/// An ORDER BY entry in a window function specification.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct WindowOrderBy {
    pub expr: String,
    #[serde(default, skip_serializing_if = "SortOrder::is_default")]
    pub order: SortOrder,
    #[serde(default, skip_serializing_if = "NullsOrder::is_default")]
    pub nulls: NullsOrder,
}

/// A named aggregation expression used as a metric.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Optional human-readable comment for this metric.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Informational synonyms (aliases) for this metric.
    /// Old stored JSON without this field deserializes to empty Vec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    /// Access modifier: PUBLIC (default, queryable) or PRIVATE (hidden from queries,
    /// usable only in derived metric expressions).
    /// Old stored JSON without this field deserializes as Public.
    #[serde(default, skip_serializing_if = "AccessModifier::is_default")]
    pub access: AccessModifier,
    /// Dimensions this metric is non-additive by (snapshot aggregation).
    /// When non-empty, expansion uses `ROW_NUMBER` CTE for snapshot selection.
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_additive_by: Vec<NonAdditiveDim>,
    /// Window function specification for window metrics.
    /// When Some, this metric uses a window function wrapping another metric.
    /// Old stored JSON without this field deserializes to None.
    /// Not serialized when None to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_spec: Option<WindowSpec>,
}

impl Metric {
    /// Returns true if this metric is a window function metric.
    #[must_use]
    pub fn is_window(&self) -> bool {
        self.window_spec.is_some()
    }
}

/// A named raw SQL column expression — a pre-aggregation fact, scoped to a table alias.
/// Added in Phase 11 for the FACTS clause of CREATE SEMANTIC VIEW.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Fact {
    pub name: String,
    pub expr: String,
    /// Which table alias this fact is scoped to.
    #[serde(default)]
    pub source_table: Option<String>,
    /// Optional output type for this fact, used by SHOW FACTS `data_type` column.
    /// Populated at define time via type inference when possible.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default)]
    pub output_type: Option<String>,
    /// Optional human-readable comment for this fact.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Informational synonyms (aliases) for this fact.
    /// Old stored JSON without this field deserializes to empty Vec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synonyms: Vec<String>,
    /// Access modifier: PUBLIC (default, queryable) or PRIVATE (hidden from queries,
    /// usable only in derived metric expressions).
    /// Old stored JSON without this field deserializes as Public.
    #[serde(default, skip_serializing_if = "AccessModifier::is_default")]
    pub access: AccessModifier,
}

/// A column-pair relationship entry for composite or single FK declarations.
/// Used in the `relationships` DDL parameter's `join_columns` field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct JoinColumn {
    pub from: String,
    pub to: String,
}

/// Cardinality of a relationship between two tables.
///
/// Inferred from PK/UNIQUE constraints at define time (Phase 33).
/// `ManyToOne`: FK columns on the from-side table are bare (no PK/UNIQUE match).
/// `OneToOne`: FK columns on the from-side table match a PK or UNIQUE constraint.
/// Defaults to `ManyToOne` when deserialized from JSON without this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Cardinality {
    #[default]
    ManyToOne,
    OneToOne,
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

/// Access modifier for facts and metrics.
/// Default is Public -- private items cannot be queried directly
/// but can be referenced by derived metric expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum AccessModifier {
    #[default]
    Public,
    Private,
}

impl AccessModifier {
    /// Returns `true` when the variant is the default (`Public`).
    /// Used by `serde(skip_serializing_if)` to omit the field from JSON
    /// when it matches the default, preserving backward-compatible output.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Public)
    }
}

/// A JOIN relationship between the base table and another source table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Phase 33: Resolved referenced columns on the target table.
    /// Populated during inference: either the target's PK or the explicit UNIQUE columns.
    /// Used by `synthesize_on_clause` to generate ON clause.
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_columns: Vec<String>,
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
/// Optional fields: `joins` (defaults to []), `facts` (defaults to []).
/// Note: `deny_unknown_fields` is intentionally NOT set — old stored JSON with extra
/// fields (e.g., from future schema changes) must still load without error.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    pub joins: Vec<Join>,
    #[serde(default)]
    pub facts: Vec<Fact>,
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
    /// ISO 8601 timestamp of when this semantic view was created.
    /// Captured at define time via `DuckDB` `now()`.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default)]
    pub created_on: Option<String>,
    /// Database name from the connection context at define time.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default)]
    pub database_name: Option<String>,
    /// Schema name from the connection context at define time.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default)]
    pub schema_name: Option<String>,
    /// View-level comment describing the purpose of this semantic view.
    /// Old stored JSON without this field deserializes to None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl SemanticViewDefinition {
    /// Build a mapping from table alias to actual table name.
    ///
    /// Used by SHOW/DESCRIBE `VTabs` to resolve the stored alias (e.g. `"o"`)
    /// to the real table name (e.g. `"orders"`).
    #[must_use]
    pub fn alias_to_table_map(&self) -> std::collections::HashMap<String, String> {
        self.tables
            .iter()
            .map(|t| (t.alias.clone(), t.table.clone()))
            .collect()
    }

    /// Iterate over inferred column types as (name, `duckdb_type`) pairs.
    ///
    /// Zips `column_type_names` and `column_types_inferred`, which are parallel
    /// vectors populated by DDL-time LIMIT 0 inference. Returns an empty
    /// iterator if inference did not run (both vecs empty).
    ///
    /// This method makes the parallel-vector invariant explicit: callers get
    /// typed pairs rather than indexing two vecs manually.
    pub fn inferred_types(&self) -> impl Iterator<Item = (&str, u32)> {
        self.column_type_names
            .iter()
            .map(String::as_str)
            .zip(self.column_types_inferred.iter().copied())
    }
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

impl SemanticViewDefinition {
    /// Maximum YAML input size (1 MiB). Sanity guard against oversized input.
    /// This is NOT a security boundary -- creating semantic views is a
    /// privileged operation guarded by warehouse auth. See trust assumption docs.
    pub const YAML_SIZE_CAP: usize = 1_048_576;

    /// Parse a YAML string into a typed semantic view definition.
    ///
    /// Returns an error if the YAML is syntactically invalid or missing
    /// required fields. The `name` parameter appears in the error message.
    pub fn from_yaml(name: &str, yaml: &str) -> Result<Self, String> {
        let def: Self = yaml_serde::from_str(yaml)
            .map_err(|e| format!("invalid YAML definition for semantic view '{name}': {e}"))?;
        Ok(def)
    }

    /// Parse YAML with a size cap check.
    ///
    /// Rejects input exceeding [`YAML_SIZE_CAP`] (1 MiB) before parsing.
    /// Returns an error including the actual size and the cap.
    pub fn from_yaml_with_size_cap(name: &str, yaml: &str) -> Result<Self, String> {
        if yaml.len() > Self::YAML_SIZE_CAP {
            return Err(format!(
                "YAML definition for semantic view '{name}' exceeds size limit \
                 ({} bytes > {} byte cap)",
                yaml.len(),
                Self::YAML_SIZE_CAP,
            ));
        }
        Self::from_yaml(name, yaml)
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

                joins: vec![],
                facts: vec![],

                column_type_names: vec![],
                column_types_inferred: vec![],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
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

    mod phase31_cardinality_tests {
        use super::*;

        #[test]
        fn cardinality_serde_roundtrip() {
            // Both variants serialize and deserialize correctly
            for (variant, expected_json) in [
                (Cardinality::ManyToOne, r#""ManyToOne""#),
                (Cardinality::OneToOne, r#""OneToOne""#),
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
                cardinality: Cardinality::OneToOne,
                ..Default::default()
            };
            let json = serde_json::to_string(&join).unwrap();
            assert!(json.contains(r#""cardinality":"OneToOne""#));
            let rt: Join = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.cardinality, Cardinality::OneToOne);
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
        fn old_json_with_one_to_many_is_rejected() {
            // Phase 33: OneToMany variant removed -- old JSON with it must fail
            let result = serde_json::from_str::<Cardinality>(r#""OneToMany""#);
            assert!(
                result.is_err(),
                "OneToMany should be an unknown variant after Phase 33"
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
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
                non_additive_by: vec![],
                window_spec: None,
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
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
                non_additive_by: vec![],
                window_spec: None,
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
                comment: None,
                synonyms: vec![],
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
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
                non_additive_by: vec![],
                window_spec: None,
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

                joins: vec![],
                facts: vec![],

                column_type_names: vec!["region".to_string(), "revenue".to_string()],
                column_types_inferred: vec![17u32, 20u32],
                created_on: None,
                database_name: None,
                schema_name: None,
                comment: None,
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

    mod phase33_model_tests {
        use super::*;

        #[test]
        fn table_ref_with_unique_constraints_roundtrip() {
            let tr = TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                unique_constraints: vec![
                    vec!["email".to_string()],
                    vec!["first_name".to_string(), "last_name".to_string()],
                ],
                comment: None,
                synonyms: vec![],
            };
            let json = serde_json::to_string(&tr).unwrap();
            assert!(json.contains("unique_constraints"));
            let rt: TableRef = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.unique_constraints.len(), 2);
            assert_eq!(rt.unique_constraints[0], vec!["email"]);
            assert_eq!(rt.unique_constraints[1], vec!["first_name", "last_name"]);
        }

        #[test]
        fn old_json_without_unique_constraints_deserializes() {
            // Backward compat: old JSON without unique_constraints field
            let json = r#"{"alias":"o","table":"orders","pk_columns":["id"]}"#;
            let tr: TableRef = serde_json::from_str(json).unwrap();
            assert!(
                tr.unique_constraints.is_empty(),
                "unique_constraints should default to [] for old JSON"
            );
        }

        #[test]
        fn join_with_ref_columns_roundtrip() {
            let join = Join {
                table: "c".to_string(),
                from_alias: "o".to_string(),
                fk_columns: vec!["customer_id".to_string()],
                ref_columns: vec!["id".to_string()],
                name: Some("o_to_c".to_string()),
                ..Default::default()
            };
            let json = serde_json::to_string(&join).unwrap();
            assert!(json.contains("ref_columns"));
            let rt: Join = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.ref_columns, vec!["id"]);
        }

        #[test]
        fn old_json_without_ref_columns_deserializes() {
            // Backward compat: old JSON without ref_columns field
            let json = r#"{"table":"customers","on":"a.id=b.id"}"#;
            let join: Join = serde_json::from_str(json).unwrap();
            assert!(
                join.ref_columns.is_empty(),
                "ref_columns should default to [] for old JSON"
            );
        }

        #[test]
        fn empty_unique_constraints_not_serialized() {
            let tr = TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                unique_constraints: vec![],
                comment: None,
                synonyms: vec![],
            };
            let json = serde_json::to_string(&tr).unwrap();
            assert!(
                !json.contains("unique_constraints"),
                "Empty unique_constraints should be omitted from JSON: {json}"
            );
        }

        #[test]
        fn empty_ref_columns_not_serialized() {
            let join = Join {
                table: "c".to_string(),
                from_alias: "o".to_string(),
                fk_columns: vec!["customer_id".to_string()],
                ref_columns: vec![],
                ..Default::default()
            };
            let json = serde_json::to_string(&join).unwrap();
            assert!(
                !json.contains("ref_columns"),
                "Empty ref_columns should be omitted from JSON: {json}"
            );
        }

        #[test]
        fn table_ref_without_pk_is_valid() {
            let tr = TableRef {
                alias: "f".to_string(),
                table: "fact_table".to_string(),
                pk_columns: vec![],
                unique_constraints: vec![],
                comment: None,
                synonyms: vec![],
            };
            assert_eq!(tr.alias, "f");
            assert_eq!(tr.table, "fact_table");
            assert!(tr.pk_columns.is_empty());
            // Roundtrip through JSON
            let json = serde_json::to_string(&tr).unwrap();
            let rt: TableRef = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.alias, "f");
            assert!(rt.pk_columns.is_empty());
        }
    }

    mod phase39_metadata_tests {
        use super::*;

        #[test]
        fn created_on_roundtrip() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                dimensions: vec![],
                metrics: vec![],
                created_on: Some("2026-04-01T12:00:00Z".to_string()),
                database_name: Some("mydb".to_string()),
                schema_name: Some("main".to_string()),
                ..Default::default()
            };
            let json = serde_json::to_string(&def).unwrap();
            let rt = SemanticViewDefinition::from_json("orders", &json).unwrap();
            assert_eq!(rt.created_on.as_deref(), Some("2026-04-01T12:00:00Z"));
            assert_eq!(rt.database_name.as_deref(), Some("mydb"));
            assert_eq!(rt.schema_name.as_deref(), Some("main"));
        }

        #[test]
        fn old_json_without_metadata_fields_deserializes() {
            let json = r#"{"base_table":"orders","dimensions":[],"metrics":[]}"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();
            assert!(
                def.created_on.is_none(),
                "created_on should default to None"
            );
            assert!(
                def.database_name.is_none(),
                "database_name should default to None"
            );
            assert!(
                def.schema_name.is_none(),
                "schema_name should default to None"
            );
        }

        #[test]
        fn fact_output_type_roundtrip() {
            let fact = Fact {
                name: "rev".to_string(),
                expr: "amount".to_string(),
                source_table: Some("orders".to_string()),
                output_type: Some("DECIMAL(10,2)".to_string()),
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
            };
            let json = serde_json::to_string(&fact).unwrap();
            let rt: Fact = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.output_type.as_deref(), Some("DECIMAL(10,2)"));
        }

        #[test]
        fn old_fact_json_without_output_type_deserializes() {
            let json = r#"{"name":"rev","expr":"amount","source_table":"orders"}"#;
            let fact: Fact = serde_json::from_str(json).unwrap();
            assert!(
                fact.output_type.is_none(),
                "output_type should default to None"
            );
        }
    }

    mod phase43_metadata_tests {
        use super::*;

        #[test]
        fn access_modifier_default_is_public() {
            assert_eq!(AccessModifier::default(), AccessModifier::Public);
        }

        #[test]
        fn access_modifier_is_default() {
            assert!(AccessModifier::Public.is_default());
            assert!(!AccessModifier::Private.is_default());
        }

        #[test]
        fn pre_v060_json_deserializes_with_defaults() {
            // Full v0.5.5 JSON blob with NO comment/synonyms/access fields
            let json = r#"{
                "base_table": "orders",
                "tables": [{"alias": "o", "table": "orders", "pk_columns": ["id"]}],
                "dimensions": [{"name": "region", "expr": "region", "source_table": "o"}],
                "metrics": [{"name": "revenue", "expr": "SUM(amount)", "source_table": "o", "using_relationships": ["rel1"]}],
                "facts": [{"name": "unit_price", "expr": "price / qty", "source_table": "o", "output_type": "DOUBLE"}],
                "joins": [{"table": "c", "from_alias": "o", "fk_columns": ["customer_id"]}],
                "column_type_names": ["region", "revenue"],
                "column_types_inferred": [17, 20],
                "created_on": "2026-04-01T12:00:00Z",
                "database_name": "mydb",
                "schema_name": "main"
            }"#;
            let def = SemanticViewDefinition::from_json("orders", json).unwrap();

            // View-level comment
            assert!(def.comment.is_none(), "view comment should default to None");

            // Table metadata
            assert!(
                def.tables[0].comment.is_none(),
                "table comment should default to None"
            );
            assert!(
                def.tables[0].synonyms.is_empty(),
                "table synonyms should default to []"
            );

            // Dimension metadata
            assert!(
                def.dimensions[0].comment.is_none(),
                "dim comment should default to None"
            );
            assert!(
                def.dimensions[0].synonyms.is_empty(),
                "dim synonyms should default to []"
            );

            // Metric metadata
            assert!(
                def.metrics[0].comment.is_none(),
                "metric comment should default to None"
            );
            assert!(
                def.metrics[0].synonyms.is_empty(),
                "metric synonyms should default to []"
            );
            assert_eq!(
                def.metrics[0].access,
                AccessModifier::Public,
                "metric access should default to Public"
            );

            // Fact metadata
            assert!(
                def.facts[0].comment.is_none(),
                "fact comment should default to None"
            );
            assert!(
                def.facts[0].synonyms.is_empty(),
                "fact synonyms should default to []"
            );
            assert_eq!(
                def.facts[0].access,
                AccessModifier::Public,
                "fact access should default to Public"
            );
        }

        #[test]
        fn metric_with_access_private_roundtrips() {
            let met = Metric {
                name: "internal_rev".to_string(),
                expr: "SUM(amount)".to_string(),
                source_table: None,
                output_type: None,
                using_relationships: vec![],
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Private,
                non_additive_by: vec![],
                window_spec: None,
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(
                json.contains(r#""access":"Private""#),
                "Private access must appear in JSON: {json}"
            );
            let rt: Metric = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.access, AccessModifier::Private);
        }

        #[test]
        fn metric_with_access_public_omits_field() {
            let met = Metric {
                name: "revenue".to_string(),
                expr: "SUM(amount)".to_string(),
                source_table: None,
                output_type: None,
                using_relationships: vec![],
                comment: None,
                synonyms: vec![],
                access: AccessModifier::Public,
                non_additive_by: vec![],
                window_spec: None,
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(
                !json.contains("access"),
                "Public access (default) should be omitted from JSON: {json}"
            );
            // Also verify empty synonyms omitted
            assert!(
                !json.contains("synonyms"),
                "Empty synonyms should be omitted from JSON: {json}"
            );
            // Also verify None comment omitted
            assert!(
                !json.contains("comment"),
                "None comment should be omitted from JSON: {json}"
            );
        }

        #[test]
        fn dimension_with_comment_and_synonyms_roundtrips() {
            let dim = Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                source_table: None,
                output_type: None,
                comment: Some("Geographic region".to_string()),
                synonyms: vec!["area".to_string(), "territory".to_string()],
            };
            let json = serde_json::to_string(&dim).unwrap();
            assert!(
                json.contains(r#""comment":"Geographic region""#),
                "comment in JSON: {json}"
            );
            assert!(
                json.contains(r#""synonyms":["area","territory"]"#),
                "synonyms in JSON: {json}"
            );
            let rt: Dimension = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.comment.as_deref(), Some("Geographic region"));
            assert_eq!(rt.synonyms, vec!["area", "territory"]);
        }

        #[test]
        fn table_ref_with_metadata_roundtrips() {
            let tr = TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                unique_constraints: vec![],
                comment: Some("Main orders table".to_string()),
                synonyms: vec!["order_facts".to_string()],
            };
            let json = serde_json::to_string(&tr).unwrap();
            assert!(
                json.contains(r#""comment":"Main orders table""#),
                "comment in JSON: {json}"
            );
            assert!(
                json.contains(r#""synonyms":["order_facts"]"#),
                "synonyms in JSON: {json}"
            );
            let rt: TableRef = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.comment.as_deref(), Some("Main orders table"));
            assert_eq!(rt.synonyms, vec!["order_facts"]);
        }

        #[test]
        fn fact_with_access_and_metadata_roundtrips() {
            let fact = Fact {
                name: "unit_price".to_string(),
                expr: "price / qty".to_string(),
                source_table: Some("o".to_string()),
                output_type: None,
                comment: Some("Price per unit".to_string()),
                synonyms: vec!["price_per_item".to_string()],
                access: AccessModifier::Private,
            };
            let json = serde_json::to_string(&fact).unwrap();
            assert!(
                json.contains(r#""access":"Private""#),
                "access in JSON: {json}"
            );
            assert!(
                json.contains(r#""comment":"Price per unit""#),
                "comment in JSON: {json}"
            );
            let rt: Fact = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.access, AccessModifier::Private);
            assert_eq!(rt.comment.as_deref(), Some("Price per unit"));
            assert_eq!(rt.synonyms, vec!["price_per_item"]);
        }

        #[test]
        fn view_level_comment_roundtrips() {
            let def = SemanticViewDefinition {
                base_table: "orders".to_string(),
                comment: Some("Revenue analytics view".to_string()),
                ..Default::default()
            };
            let json = serde_json::to_string(&def).unwrap();
            assert!(
                json.contains(r#""comment":"Revenue analytics view""#),
                "view comment in JSON: {json}"
            );
            let rt = SemanticViewDefinition::from_json("orders", &json).unwrap();
            assert_eq!(rt.comment.as_deref(), Some("Revenue analytics view"));
        }
    }

    mod phase47_non_additive_tests {
        use super::*;

        #[test]
        fn sort_order_default_is_asc() {
            assert_eq!(SortOrder::default(), SortOrder::Asc);
        }

        #[test]
        fn nulls_order_default_is_last() {
            assert_eq!(NullsOrder::default(), NullsOrder::Last);
        }

        #[test]
        fn sort_order_is_default() {
            assert!(SortOrder::Asc.is_default());
            assert!(!SortOrder::Desc.is_default());
        }

        #[test]
        fn nulls_order_is_default() {
            assert!(NullsOrder::Last.is_default());
            assert!(!NullsOrder::First.is_default());
        }

        #[test]
        fn non_additive_dim_with_defaults_skips_order_and_nulls() {
            let nad = NonAdditiveDim {
                dimension: "date_dim".to_string(),
                order: SortOrder::Asc,
                nulls: NullsOrder::Last,
            };
            let json = serde_json::to_string(&nad).unwrap();
            assert!(
                !json.contains("order"),
                "Default order (Asc) should be omitted: {json}"
            );
            assert!(
                !json.contains("nulls"),
                "Default nulls (Last) should be omitted: {json}"
            );
        }

        #[test]
        fn non_additive_dim_with_non_defaults_includes_fields() {
            let nad = NonAdditiveDim {
                dimension: "date_dim".to_string(),
                order: SortOrder::Desc,
                nulls: NullsOrder::First,
            };
            let json = serde_json::to_string(&nad).unwrap();
            assert!(
                json.contains(r#""order":"Desc""#),
                "Desc order should appear in JSON: {json}"
            );
            assert!(
                json.contains(r#""nulls":"First""#),
                "First nulls should appear in JSON: {json}"
            );
            // Roundtrip
            let rt: NonAdditiveDim = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.order, SortOrder::Desc);
            assert_eq!(rt.nulls, NullsOrder::First);
        }

        #[test]
        fn metric_without_non_additive_by_deserializes_with_empty_vec() {
            // Backward compat: pre-v0.6.0 JSON without non_additive_by field
            let json = r#"{"name":"revenue","expr":"SUM(amount)"}"#;
            let met: Metric = serde_json::from_str(json).unwrap();
            assert!(
                met.non_additive_by.is_empty(),
                "non_additive_by should default to [] for old JSON"
            );
        }

        #[test]
        fn metric_with_empty_non_additive_by_omits_field() {
            let met = Metric {
                name: "revenue".to_string(),
                expr: "SUM(amount)".to_string(),
                non_additive_by: vec![],
                ..Default::default()
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(
                !json.contains("non_additive_by"),
                "Empty non_additive_by should be omitted from JSON: {json}"
            );
        }

        #[test]
        fn metric_with_non_additive_by_roundtrips() {
            let met = Metric {
                name: "balance".to_string(),
                expr: "SUM(amount)".to_string(),
                source_table: Some("a".to_string()),
                non_additive_by: vec![
                    NonAdditiveDim {
                        dimension: "date_dim".to_string(),
                        order: SortOrder::Desc,
                        nulls: NullsOrder::First,
                    },
                    NonAdditiveDim {
                        dimension: "account".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    },
                ],
                ..Default::default()
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(
                json.contains("non_additive_by"),
                "non_additive_by with entries should appear in JSON: {json}"
            );
            let rt: Metric = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.non_additive_by.len(), 2);
            assert_eq!(rt.non_additive_by[0].dimension, "date_dim");
            assert_eq!(rt.non_additive_by[0].order, SortOrder::Desc);
            assert_eq!(rt.non_additive_by[0].nulls, NullsOrder::First);
            assert_eq!(rt.non_additive_by[1].dimension, "account");
            assert_eq!(rt.non_additive_by[1].order, SortOrder::Asc);
            assert_eq!(rt.non_additive_by[1].nulls, NullsOrder::Last);
        }
    }

    mod window_spec_tests {
        use super::*;

        #[test]
        fn window_spec_roundtrip_serde() {
            let ws = WindowSpec {
                window_function: "AVG".to_string(),
                inner_metric: "total_qty".to_string(),
                extra_args: vec![],
                excluding_dims: vec!["date_dim".to_string()],
                partition_dims: vec![],
                order_by: vec![WindowOrderBy {
                    expr: "date_dim".to_string(),
                    order: SortOrder::Asc,
                    nulls: NullsOrder::Last,
                }],
                frame_clause: None,
            };
            let json = serde_json::to_string(&ws).unwrap();
            let rt: WindowSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.window_function, "AVG");
            assert_eq!(rt.inner_metric, "total_qty");
            assert_eq!(rt.excluding_dims, vec!["date_dim"]);
            assert_eq!(rt.order_by.len(), 1);
            assert_eq!(rt.order_by[0].expr, "date_dim");
            assert!(rt.frame_clause.is_none());
        }

        #[test]
        fn metric_without_window_spec_deserializes_from_old_json() {
            // Backward compat: pre-Phase 48 JSON has no window_spec field
            let json = r#"{"name":"revenue","expr":"SUM(amount)"}"#;
            let met: Metric = serde_json::from_str(json).unwrap();
            assert!(met.window_spec.is_none());
            assert!(!met.is_window());
        }

        #[test]
        fn window_spec_full_roundtrip() {
            let ws = WindowSpec {
                window_function: "LAG".to_string(),
                inner_metric: "balance".to_string(),
                extra_args: vec!["30".to_string()],
                excluding_dims: vec!["region".to_string(), "status".to_string()],
                partition_dims: vec![],
                order_by: vec![
                    WindowOrderBy {
                        expr: "date_dim".to_string(),
                        order: SortOrder::Desc,
                        nulls: NullsOrder::First,
                    },
                    WindowOrderBy {
                        expr: "account".to_string(),
                        order: SortOrder::Asc,
                        nulls: NullsOrder::Last,
                    },
                ],
                frame_clause: Some(
                    "RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW".to_string(),
                ),
            };
            let json = serde_json::to_string(&ws).unwrap();
            let rt: WindowSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.window_function, "LAG");
            assert_eq!(rt.inner_metric, "balance");
            assert_eq!(rt.extra_args, vec!["30"]);
            assert_eq!(rt.excluding_dims, vec!["region", "status"]);
            assert_eq!(rt.order_by.len(), 2);
            assert_eq!(rt.order_by[0].expr, "date_dim");
            assert_eq!(rt.order_by[0].order, SortOrder::Desc);
            assert_eq!(rt.order_by[0].nulls, NullsOrder::First);
            assert_eq!(rt.order_by[1].expr, "account");
            assert_eq!(rt.order_by[1].order, SortOrder::Asc);
            assert_eq!(rt.order_by[1].nulls, NullsOrder::Last);
            assert_eq!(
                rt.frame_clause.as_deref(),
                Some("RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW")
            );
        }

        #[test]
        fn window_order_by_non_default_sort_nulls_roundtrips() {
            let wob = WindowOrderBy {
                expr: "ts".to_string(),
                order: SortOrder::Desc,
                nulls: NullsOrder::First,
            };
            let json = serde_json::to_string(&wob).unwrap();
            assert!(json.contains("\"order\":\"Desc\""));
            assert!(json.contains("\"nulls\":\"First\""));
            let rt: WindowOrderBy = serde_json::from_str(&json).unwrap();
            assert_eq!(rt.order, SortOrder::Desc);
            assert_eq!(rt.nulls, NullsOrder::First);
        }

        #[test]
        fn metric_with_empty_window_spec_omits_field() {
            let met = Metric {
                name: "revenue".to_string(),
                expr: "SUM(amount)".to_string(),
                window_spec: None,
                ..Default::default()
            };
            let json = serde_json::to_string(&met).unwrap();
            assert!(
                !json.contains("window_spec"),
                "None window_spec should be omitted from JSON: {json}"
            );
        }

        #[test]
        fn metric_is_window_returns_true_when_set() {
            let met = Metric {
                name: "avg_qty_7d".to_string(),
                expr: "AVG(total_qty) OVER (...)".to_string(),
                window_spec: Some(WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: "total_qty".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert!(met.is_window());
        }
    }
}
