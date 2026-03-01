use serde::{Deserialize, Serialize};

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

/// A JOIN relationship between the base table and another source table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Join {
    pub table: String,
    pub on: String,
}

/// Top-level definition of a semantic view.
///
/// Stored as JSON in `semantic_layer._definitions`.
/// Required fields: `base_table`, `dimensions`, `metrics`.
/// Optional fields: `filters` (defaults to []), `joins` (defaults to []).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[serde(deny_unknown_fields)]
pub struct SemanticViewDefinition {
    pub base_table: String,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default)]
    pub joins: Vec<Join>,
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
    fn unknown_fields_are_rejected() {
        let json = r#"{"base_table": "t", "dimensions": [], "metrics": [], "extra": 1}"#;
        assert!(SemanticViewDefinition::from_json("test", json).is_err());
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
}
