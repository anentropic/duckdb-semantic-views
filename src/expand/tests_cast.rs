//! output_type CAST-wrapping of dimensions/metrics.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::phase12_cast_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::TestFixtureExt;
use crate::model::{Dimension, SemanticViewDefinition};

#[test]
fn output_type_on_metric_emits_cast() {
    let mut def = SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_metric("revenue", "sum(amount)", None);
    def.metrics[0].output_type = Some("BIGINT".to_string());
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("revenue")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("CAST(sum(amount) AS BIGINT)"),
        "output_type BIGINT must generate CAST wrapper: {sql}"
    );
}

#[test]
fn output_type_on_dimension_emits_cast() {
    let mut def = SemanticViewDefinition::default().with_table("orders", "orders", &[]);
    def.dimensions.push(Dimension {
        name: "region_id".to_string(),
        expr: "region_id".to_string(),
        output_type: Some("INTEGER".to_string()),
        ..Default::default()
    });
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region_id")],
        metrics: vec![],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        sql.contains("CAST(region_id AS INTEGER)"),
        "output_type INTEGER on dimension must generate CAST wrapper: {sql}"
    );
}

#[test]
fn no_output_type_no_cast() {
    let def = SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_metric("revenue", "sum(amount)", None);
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("revenue")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(
        !sql.contains("CAST(sum(amount) AS"),
        "No output_type must not generate CAST: {sql}"
    );
    assert!(
        sql.contains("sum(amount) AS"),
        "Bare expr must be present: {sql}"
    );
}
