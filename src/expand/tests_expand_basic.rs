//! Basic single/multi dimension + metric expansion.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::phase11_1_expand_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::model::TableRef;

fn def_with_join_columns() -> crate::model::SemanticViewDefinition {
    crate::model::SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                ..Default::default()
            },
            TableRef {
                alias: "c".to_string(),
                table: "customers".to_string(),
                ..Default::default()
            },
        ],
        dimensions: vec![
            crate::model::Dimension {
                name: "region".to_string(),
                expr: "o.region".to_string(),
                source_table: Some("o".to_string()),

                ..Default::default()
            },
            crate::model::Dimension {
                name: "tier".to_string(),
                expr: "c.tier".to_string(),
                source_table: Some("c".to_string()),

                ..Default::default()
            },
        ],
        metrics: vec![crate::model::Metric {
            name: "revenue".to_string(),
            expr: "sum(o.amount)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],

        joins: vec![crate::model::Join {
            // Modern (Phase 24) FK encoding: source alias `o`, target
            // alias `c`, with fk/ref columns so the fan-trap safety
            // check can build the relationship graph (SG-7 / AR-4).
            table: "c".to_string(),
            from_alias: "o".to_string(),
            fk_columns: vec!["customer_id".to_string()],
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        }],
        facts: vec![],
        materializations: vec![],

        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    }
}

#[test]
fn table_qualified_dimension_lookup_with_matching_source_table() {
    let def = def_with_join_columns();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("o.region")],
        metrics: vec![],
    };
    let sql = expand("sales_view", &def, &req).unwrap();
    assert!(
        sql.contains("o.region"),
        "Must include the dimension expr: {sql}"
    );
    assert!(
        sql.contains("AS \"region\""),
        "Must alias as bare name: {sql}"
    );
}

#[test]
fn bare_dimension_name_still_resolves() {
    let def = def_with_join_columns();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![],
    };
    let result = expand("sales_view", &def, &req);
    assert!(
        result.is_ok(),
        "Bare name lookup must succeed: {:?}",
        result.err()
    );
}

#[test]
fn table_qualified_unknown_dimension_returns_error() {
    let def = def_with_join_columns();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("o.nosuch")],
        metrics: vec![],
    };
    let result = expand("sales_view", &def, &req);
    match result {
        Err(ExpandError::UnknownDimension { name, .. }) => {
            let _ = name;
        }
        other => panic!("Expected UnknownDimension error, got: {:?}", other),
    }
}

#[test]
fn table_qualified_metric_lookup_with_matching_source_table() {
    let def = def_with_join_columns();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("o.revenue")],
    };
    let sql = expand("sales_view", &def, &req).unwrap();
    assert!(
        sql.contains("sum(o.amount)"),
        "Must include metric expr: {sql}"
    );
}
