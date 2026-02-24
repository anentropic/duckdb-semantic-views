use serde::{Deserialize, Serialize};

/// A named SQL column expression used as a dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub name: String,
    pub expr: String,
}

/// A named aggregation expression used as a metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub expr: String,
}

/// A JOIN relationship between the base table and another source table.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Returns an error if the JSON is invalid or missing required fields.
    /// The `name` parameter is used only in the error message for context.
    pub fn from_json(name: &str, json: &str) -> Result<Self, String> {
        serde_json::from_str(json)
            .map_err(|e| format!("invalid definition for semantic view '{name}': {e}"))
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
}
