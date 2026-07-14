//! Fan-trap / chasm-trap detection during expansion.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::phase31_fan_trap_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::minimal_def;
use crate::model::{Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef};

fn fan_trap_three_table_def() -> SemanticViewDefinition {
    SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "li".to_string(),
                table: "line_items".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "c".to_string(),
                table: "customers".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
        ],
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "o.region".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            },
            Dimension {
                name: "status".to_string(),
                expr: "li.status".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Dimension {
                name: "segment".to_string(),
                expr: "c.segment".to_string(),
                source_table: Some("c".to_string()),
                ..Default::default()
            },
        ],
        metrics: vec![
            Metric {
                name: "revenue".to_string(),
                expr: "SUM(li.extended_price)".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Metric {
                name: "order_count".to_string(),
                expr: "COUNT(*)".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            },
        ],
        joins: vec![
            Join {
                table: "o".to_string(),
                from_alias: "li".to_string(),
                fk_columns: vec!["order_id".to_string()],
                ref_columns: vec!["id".to_string()],
                name: Some("li_to_order".to_string()),
                cardinality: Cardinality::ManyToOne,
                ..Default::default()
            },
            Join {
                table: "c".to_string(),
                from_alias: "o".to_string(),
                fk_columns: vec!["customer_id".to_string()],
                ref_columns: vec!["id".to_string()],
                name: Some("order_to_customer".to_string()),
                cardinality: Cardinality::ManyToOne,
                ..Default::default()
            },
        ],
        facts: vec![],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    }
}

#[test]
fn fan_trap_one_to_many_blocked() {
    let def = fan_trap_three_table_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("status")],
        metrics: vec![MetricName::new("order_count")],
    };
    let result = expand("sales", &def, &req);
    assert!(result.is_err(), "Fan trap must block the query");
    match result.unwrap_err() {
        ExpandError::FanTrap { detail } => {
            assert_eq!(detail.view_name, "sales");
            assert_eq!(detail.metric_name, "order_count");
            assert_eq!(detail.dimension_name, "status");
        }
        other => panic!("Expected FanTrap, got: {other}"),
    }
}

#[test]
fn fan_trap_many_to_one_safe() {
    let def = fan_trap_three_table_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("revenue")],
    };
    let result = expand("sales", &def, &req);
    assert!(
        result.is_ok(),
        "MANY TO ONE direction must be safe: {:?}",
        result.err()
    );
}

#[test]
fn fan_trap_one_to_one_safe() {
    let def = SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "d".to_string(),
                table: "details".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
        ],
        dimensions: vec![Dimension {
            name: "detail".to_string(),
            expr: "d.detail".to_string(),
            source_table: Some("d".to_string()),
            ..Default::default()
        }],
        metrics: vec![Metric {
            name: "cnt".to_string(),
            expr: "COUNT(*)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],
        joins: vec![Join {
            table: "d".to_string(),
            from_alias: "o".to_string(),
            fk_columns: vec!["detail_id".to_string()],
            ref_columns: vec!["id".to_string()],
            name: Some("order_to_detail".to_string()),
            cardinality: Cardinality::OneToOne,
            ..Default::default()
        }],
        facts: vec![],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    };
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("detail")],
        metrics: vec![MetricName::new("cnt")],
    };
    let result = expand("test", &def, &req);
    assert!(
        result.is_ok(),
        "ONE TO ONE must be safe: {:?}",
        result.err()
    );
}

#[test]
fn fan_trap_same_table_safe() {
    let def = fan_trap_three_table_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("status")],
        metrics: vec![MetricName::new("revenue")],
    };
    let result = expand("sales", &def, &req);
    assert!(
        result.is_ok(),
        "Same table must be safe: {:?}",
        result.err()
    );
}

#[test]
fn fan_trap_no_joins_safe() {
    let def = minimal_def("orders", "region", "region", "cnt", "COUNT(*)");
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("cnt")],
    };
    let result = expand("test", &def, &req);
    assert!(result.is_ok(), "No joins must be safe: {:?}", result.err());
}

#[test]
fn fan_trap_transitive_chain() {
    let mut def = fan_trap_three_table_def();
    def.metrics.push(Metric {
        name: "customer_count".to_string(),
        expr: "COUNT(DISTINCT c.id)".to_string(),
        source_table: Some("c".to_string()),
        ..Default::default()
    });
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("status")],
        metrics: vec![MetricName::new("customer_count")],
    };
    let result = expand("sales", &def, &req);
    assert!(
        result.is_err(),
        "Transitive chain fan trap must be detected"
    );
    match result.unwrap_err() {
        ExpandError::FanTrap { detail } => {
            assert_eq!(detail.metric_name, "customer_count");
            assert_eq!(detail.dimension_name, "status");
        }
        other => panic!("Expected FanTrap, got: {other}"),
    }
}

#[test]
fn fan_trap_derived_metric_blocked() {
    let mut def = fan_trap_three_table_def();
    def.metrics.push(Metric {
        name: "avg_order".to_string(),
        expr: "order_count / 1".to_string(),
        ..Default::default()
    });
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("status")],
        metrics: vec![MetricName::new("avg_order")],
    };
    let result = expand("sales", &def, &req);
    assert!(result.is_err(), "Derived metric fan trap must be detected");
    match result.unwrap_err() {
        ExpandError::FanTrap { detail } => {
            assert_eq!(detail.metric_name, "avg_order");
            assert_eq!(detail.dimension_name, "status");
        }
        other => panic!("Expected FanTrap, got: {other}"),
    }
}

#[test]
fn fan_trap_error_message_format() {
    let err = ExpandError::FanTrap {
        detail: Box::new(FanTrapError {
            view_name: "sales".to_string(),
            metric_name: "order_count".to_string(),
            metric_table: "o".to_string(),
            dimension_name: "status".to_string(),
            dimension_table: "li".to_string(),
            relationship_name: "li_to_order".to_string(),
        }),
    };
    let msg = format!("{err}");
    assert!(msg.contains("sales"), "Must contain view name");
    assert!(msg.contains("order_count"), "Must contain metric name");
    assert!(msg.contains("status"), "Must contain dimension name");
    assert!(
        msg.contains("li_to_order"),
        "Must contain relationship name"
    );
    assert!(
        msg.contains("fan trap detected"),
        "Must contain 'fan trap detected'"
    );
    assert!(
        msg.contains("many-to-one cardinality"),
        "Must describe the cardinality direction"
    );
}
