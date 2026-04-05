//! Shared test helpers for graph submodule tests.

use crate::model::{Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef};

/// Helper to build a minimal SemanticViewDefinition for testing.
pub(super) fn make_def(
    tables: Vec<(&str, &str, Vec<&str>)>,
    joins: Vec<(&str, &str, Vec<&str>)>,
    dims: Vec<(&str, Option<&str>)>,
    metrics: Vec<(&str, Option<&str>)>,
) -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: tables
            .first()
            .map(|(_, t, _)| t.to_string())
            .unwrap_or_default(),
        tables: tables
            .iter()
            .map(|(alias, table, pks)| TableRef {
                alias: alias.to_string(),
                table: table.to_string(),
                pk_columns: pks.iter().map(|s| s.to_string()).collect(),
                unique_constraints: vec![],
            })
            .collect(),
        joins: joins
            .iter()
            .map(|(from_alias, to_alias, fk_cols)| Join {
                table: to_alias.to_string(),
                from_alias: from_alias.to_string(),
                fk_columns: fk_cols.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            })
            .collect(),
        dimensions: dims
            .iter()
            .map(|(name, source)| Dimension {
                name: name.to_string(),
                expr: name.to_string(),
                source_table: source.map(|s| s.to_string()),
                output_type: None,
            })
            .collect(),
        metrics: metrics
            .iter()
            .map(|(name, source)| Metric {
                name: name.to_string(),
                expr: format!("sum({})", name),
                source_table: source.map(|s| s.to_string()),
                output_type: None,
                using_relationships: vec![],
            })
            .collect(),
        facts: vec![],

        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
    }
}

/// Helper to build a def with facts for testing.
pub(super) fn make_def_with_facts(
    tables: Vec<(&str, &str)>,
    facts: Vec<(&str, &str, &str)>,
) -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: tables
            .first()
            .map(|(_, t)| t.to_string())
            .unwrap_or_default(),
        tables: tables
            .iter()
            .map(|(alias, table)| TableRef {
                alias: alias.to_string(),
                table: table.to_string(),
                pk_columns: vec!["id".to_string()],
                unique_constraints: vec![],
            })
            .collect(),
        facts: facts
            .iter()
            .map(|(name, expr, source)| Fact {
                name: name.to_string(),
                expr: expr.to_string(),
                source_table: Some(source.to_string()),
                output_type: None,
            })
            .collect(),
        dimensions: vec![],
        metrics: vec![],
        joins: vec![],

        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
    }
}

/// Helper to build a def with base metrics and derived metrics for testing.
pub(super) fn make_def_with_derived_metrics(
    base_metrics: Vec<(&str, &str, &str)>, // (name, expr, source_table)
    derived_metrics: Vec<(&str, &str)>,    // (name, expr) -- source_table: None
) -> SemanticViewDefinition {
    let mut metrics = Vec::new();
    for (name, expr, source) in base_metrics {
        metrics.push(Metric {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: Some(source.to_string()),
            output_type: None,
            using_relationships: vec![],
        });
    }
    for (name, expr) in derived_metrics {
        metrics.push(Metric {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: None,
            output_type: None,
            using_relationships: vec![],
        });
    }
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        tables: vec![TableRef {
            alias: "o".to_string(),
            table: "orders".to_string(),
            pk_columns: vec!["id".to_string()],
            unique_constraints: vec![],
        }],
        metrics,
        dimensions: vec![],
        joins: vec![],
        facts: vec![],

        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
    }
}

/// Helper to build a definition with named (or unnamed) joins for diamond tests.
pub(super) fn make_def_with_named_joins(
    tables: Vec<(&str, &str, Vec<&str>)>,
    joins: Vec<(Option<&str>, &str, &str, Vec<&str>)>, // (name, from, to, fk_cols)
    metrics: Vec<(&str, Option<&str>, Vec<&str>)>,     // (name, source, using_rels)
) -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: tables
            .first()
            .map(|(_, t, _)| t.to_string())
            .unwrap_or_default(),
        tables: tables
            .iter()
            .map(|(alias, table, pks)| TableRef {
                alias: alias.to_string(),
                table: table.to_string(),
                pk_columns: pks.iter().map(|s| s.to_string()).collect(),
                unique_constraints: vec![],
            })
            .collect(),
        joins: joins
            .iter()
            .map(|(name, from_alias, to_alias, fk_cols)| Join {
                table: to_alias.to_string(),
                from_alias: from_alias.to_string(),
                fk_columns: fk_cols.iter().map(|s| s.to_string()).collect(),
                name: name.map(|n| n.to_string()),
                ..Default::default()
            })
            .collect(),
        dimensions: vec![],
        metrics: metrics
            .iter()
            .map(|(name, source, using_rels)| Metric {
                name: name.to_string(),
                expr: "COUNT(*)".to_string(),
                source_table: source.map(|s| s.to_string()),
                output_type: None,
                using_relationships: using_rels.iter().map(|s| s.to_string()).collect(),
            })
            .collect(),
        facts: vec![],

        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
    }
}
