//! Qualified dimension/metric name resolution.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::qualified_name_resolution_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::{orders_view, TestFixtureExt};

#[test]
fn test_qualified_dimension_wrong_table_errors() {
    // SG-14: no fallback to "any dimension with that bare name".
    let def = orders_view();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("x.region")],
        metrics: vec![MetricName::new("total_revenue")],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match err {
        ExpandError::UnknownDimension { name, .. } => {
            assert_eq!(name, "x.region");
        }
        other => panic!("Expected UnknownDimension, got: {other}"),
    }
}

#[test]
fn test_qualified_metric_wrong_table_errors() {
    let def =
        orders_view()
            .clear_metrics()
            .with_metric("total_revenue", "sum(amount)", Some("orders"));
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("x.total_revenue")],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match err {
        ExpandError::UnknownMetric { name, .. } => {
            assert_eq!(name, "x.total_revenue");
        }
        other => panic!("Expected UnknownMetric, got: {other}"),
    }
}

#[test]
fn test_base_alias_qualification_matches_unqualified_declaration() {
    // A dimension declared without a source table is a base-table
    // item; qualifying the request with the base alias must resolve.
    let def = orders_view();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("orders.region")],
        metrics: vec![MetricName::new("total_revenue")],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(sql.contains("region AS \"region\""), "SQL: {sql}");
}

#[test]
fn test_bare_and_qualified_same_dimension_rejected_as_duplicate() {
    // SG-14: the duplicate check keys on the RESOLVED item, so
    // `region` and `orders.region` cannot emit the column twice.
    let def = orders_view();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![
            DimensionName::new("region"),
            DimensionName::new("orders.region"),
        ],
        metrics: vec![],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match err {
        ExpandError::DuplicateDimension { name, .. } => {
            assert_eq!(name, "orders.region");
        }
        other => panic!("Expected DuplicateDimension, got: {other}"),
    }
}

#[test]
fn test_bare_and_qualified_same_metric_rejected_as_duplicate() {
    let def =
        orders_view()
            .clear_metrics()
            .with_metric("total_revenue", "sum(amount)", Some("orders"));
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![],
        metrics: vec![
            MetricName::new("total_revenue"),
            MetricName::new("orders.total_revenue"),
        ],
    };
    let err = expand("orders", &def, &req).unwrap_err();
    match err {
        ExpandError::DuplicateMetric { name, .. } => {
            assert_eq!(name, "orders.total_revenue");
        }
        other => panic!("Expected DuplicateMetric, got: {other}"),
    }
}
