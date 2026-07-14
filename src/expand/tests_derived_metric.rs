//! Derived-metric (metric-of-metrics) inlining.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::phase30_derived_metric_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::facts::{inline_derived_metrics, toposort_facts};
use crate::expand::test_helpers::{minimal_def, TestFixtureExt};
use crate::model::{
    AccessModifier, Dimension, Fact, Join, Metric, SemanticViewDefinition, TableRef,
};

#[test]
fn inline_derived_one_base_one_derived() {
    let metrics = vec![
        Metric {
            name: "revenue".to_string(),
            expr: "SUM(amount)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        },
        Metric {
            name: "cost".to_string(),
            expr: "SUM(unit_cost)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        },
        Metric {
            name: "profit".to_string(),
            expr: "revenue - cost".to_string(),
            ..Default::default()
        },
    ];
    let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
        .unwrap()
        .exprs;
    assert_eq!(
        resolved.get("profit").unwrap(),
        "(SUM(amount)) - (SUM(unit_cost))"
    );
}

#[test]
fn inline_derived_stacked() {
    let metrics = vec![
        Metric {
            name: "revenue".to_string(),
            expr: "SUM(amount)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        },
        Metric {
            name: "cost".to_string(),
            expr: "SUM(unit_cost)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        },
        Metric {
            name: "profit".to_string(),
            expr: "revenue - cost".to_string(),
            ..Default::default()
        },
        Metric {
            name: "margin".to_string(),
            expr: "profit / revenue * 100".to_string(),
            ..Default::default()
        },
    ];
    let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
        .unwrap()
        .exprs;
    assert_eq!(
        resolved.get("profit").unwrap(),
        "(SUM(amount)) - (SUM(unit_cost))"
    );
    assert_eq!(
        resolved.get("margin").unwrap(),
        "((SUM(amount)) - (SUM(unit_cost))) / (SUM(amount)) * 100"
    );
}

#[test]
fn inline_derived_with_facts() {
    let metrics = vec![
        Metric {
            name: "revenue".to_string(),
            expr: "SUM(net_price)".to_string(),
            source_table: Some("li".to_string()),
            ..Default::default()
        },
        Metric {
            name: "double_rev".to_string(),
            expr: "revenue * 2".to_string(),
            ..Default::default()
        },
    ];
    let facts = vec![Fact {
        name: "net_price".to_string(),
        expr: "extended_price * (1 - discount)".to_string(),
        source_table: Some("li".to_string()),
        output_type: None,
        comment: None,
        synonyms: vec![],
        access: AccessModifier::Public,
    }];
    let topo_order = toposort_facts(&facts).unwrap();
    let resolved = inline_derived_metrics(&metrics, &facts, &topo_order, &[])
        .unwrap()
        .exprs;
    assert_eq!(
        resolved.get("revenue").unwrap(),
        "SUM((extended_price * (1 - discount)))"
    );
    assert_eq!(
        resolved.get("double_rev").unwrap(),
        "(SUM((extended_price * (1 - discount)))) * 2"
    );
}

#[test]
fn inline_derived_parenthesization_prevents_precedence_error() {
    let metrics = vec![
        Metric {
            name: "a".to_string(),
            expr: "SUM(x)".to_string(),
            source_table: Some("t".to_string()),
            ..Default::default()
        },
        Metric {
            name: "b".to_string(),
            expr: "SUM(y)".to_string(),
            source_table: Some("t".to_string()),
            ..Default::default()
        },
        Metric {
            name: "profit".to_string(),
            expr: "a - b".to_string(),
            ..Default::default()
        },
        Metric {
            name: "margin".to_string(),
            expr: "profit / a".to_string(),
            ..Default::default()
        },
    ];
    let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
        .unwrap()
        .exprs;
    assert_eq!(
        resolved.get("margin").unwrap(),
        "((SUM(x)) - (SUM(y))) / (SUM(x))"
    );
}

#[test]
fn inline_derived_word_boundary_safety() {
    let metrics = vec![
        Metric {
            name: "revenue".to_string(),
            expr: "SUM(amount)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        },
        Metric {
            name: "revenue_total".to_string(),
            expr: "SUM(total)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        },
        Metric {
            name: "derived".to_string(),
            expr: "revenue + revenue_total".to_string(),
            ..Default::default()
        },
    ];
    let resolved = inline_derived_metrics(&metrics, &[], &[], &[])
        .unwrap()
        .exprs;
    assert_eq!(
        resolved.get("derived").unwrap(),
        "(SUM(amount)) + (SUM(total))"
    );
}

#[test]
fn expand_derived_metric_correct_sql() {
    let def = minimal_def("orders", "region", "region", "revenue", "SUM(amount)")
        .with_metric("cost", "SUM(unit_cost)", Some("o"))
        .with_metric("profit", "revenue - cost", None);
    // Fix revenue source_table to match original
    let mut def = def;
    def.metrics[0].source_table = Some("o".to_string());
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("profit")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        sql.contains("(SUM(amount)) - (SUM(unit_cost)) AS \"profit\""),
        "Derived metric must expand to inlined expression: {sql}"
    );
    assert!(
        sql.contains("GROUP BY\n    1"),
        "GROUP BY should reference only the dimension: {sql}"
    );
}

#[test]
fn expand_derived_only_no_base_metrics_requested() {
    let def = SemanticViewDefinition {
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
        ],
        dimensions: vec![Dimension {
            name: "region".to_string(),
            expr: "o.region".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],
        metrics: vec![
            Metric {
                name: "revenue".to_string(),
                expr: "SUM(li.amount)".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Metric {
                name: "cost".to_string(),
                expr: "SUM(li.unit_cost)".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Metric {
                name: "profit".to_string(),
                expr: "revenue - cost".to_string(),
                ..Default::default()
            },
        ],
        joins: vec![Join {
            table: "o".to_string(),
            from_alias: "li".to_string(),
            fk_columns: vec!["order_id".to_string()],
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
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("profit")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"line_items\" AS \"li\""),
        "JOIN to li must be included for derived metric referencing li-based metrics: {sql}"
    );
    assert!(
        sql.contains("(SUM(li.amount)) - (SUM(li.unit_cost)) AS \"profit\""),
        "Derived metric expression must be inlined: {sql}"
    );
}

#[test]
fn resolve_joins_includes_transitive_deps_from_derived() {
    let def = SemanticViewDefinition {
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
        ],
        dimensions: vec![Dimension {
            name: "region".to_string(),
            expr: "o.region".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],
        metrics: vec![
            Metric {
                name: "revenue".to_string(),
                expr: "SUM(li.amount)".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Metric {
                name: "order_count".to_string(),
                expr: "COUNT(DISTINCT o.id)".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            },
            Metric {
                name: "avg_order_value".to_string(),
                expr: "revenue / order_count".to_string(),
                ..Default::default()
            },
        ],
        joins: vec![Join {
            table: "o".to_string(),
            from_alias: "li".to_string(),
            fk_columns: vec!["order_id".to_string()],
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
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("avg_order_value")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"line_items\" AS \"li\""),
        "JOIN to li must be included for derived metric avg_order_value: {sql}"
    );
}

#[test]
fn expand_derived_metric_with_facts_chain() {
    let def = SemanticViewDefinition {
        tables: vec![],
        dimensions: vec![],
        metrics: vec![
            Metric {
                name: "revenue".to_string(),
                expr: "SUM(net_price)".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Metric {
                name: "cost".to_string(),
                expr: "SUM(unit_cost)".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
            Metric {
                name: "profit".to_string(),
                expr: "revenue - cost".to_string(),
                ..Default::default()
            },
        ],
        joins: vec![],
        facts: vec![Fact {
            name: "net_price".to_string(),
            expr: "extended_price * (1 - discount)".to_string(),
            source_table: Some("li".to_string()),
            output_type: None,
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
        }],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    };
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("profit")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        sql.contains("(SUM((extended_price * (1 - discount)))) - (SUM(unit_cost)) AS \"profit\""),
        "Fact->base->derived chain must resolve correctly: {sql}"
    );
}
