//! Shared test helpers for expand submodule tests.
//!
//! Provides builder functions for common SemanticViewDefinition fixtures,
//! following the pattern established in `graph/test_helpers.rs`.

use crate::model::{Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef};

/// Base orders view: single table, 2 dimensions, 2 metrics.
///
/// - base_table: "orders"
/// - dimensions: region, status
/// - metrics: total_revenue = sum(amount), order_count = count(*)
pub(super) fn orders_view() -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        tables: vec![],
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                source_table: None,
                output_type: None,
            },
            Dimension {
                name: "status".to_string(),
                expr: "status".to_string(),
                source_table: None,
                output_type: None,
            },
        ],
        metrics: vec![
            Metric {
                name: "total_revenue".to_string(),
                expr: "sum(amount)".to_string(),
                source_table: None,
                output_type: None,
                using_relationships: vec![],
            },
            Metric {
                name: "order_count".to_string(),
                expr: "count(*)".to_string(),
                source_table: None,
                output_type: None,
                using_relationships: vec![],
            },
        ],
        joins: vec![],
        facts: vec![],
        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
    }
}

/// Minimal single-dim, single-metric view for focused tests.
///
/// - base_table: configurable
/// - dimensions: one with given name/expr
/// - metrics: one with given name/expr
pub(super) fn minimal_def(
    base_table: &str,
    dim_name: &str,
    dim_expr: &str,
    metric_name: &str,
    metric_expr: &str,
) -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: base_table.to_string(),
        tables: vec![],
        dimensions: vec![Dimension {
            name: dim_name.to_string(),
            expr: dim_expr.to_string(),
            source_table: None,
            output_type: None,
        }],
        metrics: vec![Metric {
            name: metric_name.to_string(),
            expr: metric_expr.to_string(),
            source_table: None,
            output_type: None,
            using_relationships: vec![],
        }],
        joins: vec![],
        facts: vec![],
        column_type_names: vec![],
        column_types_inferred: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
    }
}

/// Extension trait for test fixture mutations.
///
/// Allows builder-style chaining: `orders_view().with_dimension(...).with_join(...)`
pub(super) trait TestFixtureExt {
    fn with_base_table(self, base_table: &str) -> Self;
    fn with_dimension(self, name: &str, expr: &str, source_table: Option<&str>) -> Self;
    fn with_metric(self, name: &str, expr: &str, source_table: Option<&str>) -> Self;
    fn with_join_on(self, table: &str, on: &str) -> Self;
    fn with_table(self, alias: &str, table: &str, pk_columns: &[&str]) -> Self;
    fn with_fact(self, name: &str, expr: &str, source_table: &str) -> Self;
    fn with_using_relationship(self, metric_name: &str, relationships: &[&str]) -> Self;
    fn clear_dimensions(self) -> Self;
    fn clear_metrics(self) -> Self;
}

impl TestFixtureExt for SemanticViewDefinition {
    fn with_base_table(mut self, base_table: &str) -> Self {
        self.base_table = base_table.to_string();
        self
    }

    fn with_dimension(mut self, name: &str, expr: &str, source_table: Option<&str>) -> Self {
        self.dimensions.push(Dimension {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: source_table.map(|s| s.to_string()),
            output_type: None,
        });
        self
    }

    fn with_metric(mut self, name: &str, expr: &str, source_table: Option<&str>) -> Self {
        self.metrics.push(Metric {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: source_table.map(|s| s.to_string()),
            output_type: None,
            using_relationships: vec![],
        });
        self
    }

    fn with_join_on(mut self, table: &str, on: &str) -> Self {
        self.joins.push(Join {
            table: table.to_string(),
            on: on.to_string(),
            from_cols: vec![],
            join_columns: vec![],
            ..Default::default()
        });
        self
    }

    fn with_table(mut self, alias: &str, table: &str, pk_columns: &[&str]) -> Self {
        self.tables.push(TableRef {
            alias: alias.to_string(),
            table: table.to_string(),
            pk_columns: pk_columns.iter().map(|s| s.to_string()).collect(),
            unique_constraints: vec![],
        });
        self
    }

    fn with_fact(mut self, name: &str, expr: &str, source_table: &str) -> Self {
        self.facts.push(Fact {
            name: name.to_string(),
            expr: expr.to_string(),
            source_table: Some(source_table.to_string()),
            output_type: None,
        });
        self
    }

    fn with_using_relationship(mut self, metric_name: &str, relationships: &[&str]) -> Self {
        if let Some(m) = self.metrics.iter_mut().find(|m| m.name == metric_name) {
            m.using_relationships = relationships.iter().map(|s| s.to_string()).collect();
        }
        self
    }

    fn clear_dimensions(mut self) -> Self {
        self.dimensions.clear();
        self
    }

    fn clear_metrics(mut self) -> Self {
        self.metrics.clear();
        self
    }
}
