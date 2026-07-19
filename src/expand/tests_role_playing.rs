//! Role-playing (USING) scoped-alias resolution.
//!
//! Extracted from `sql_gen.rs`'s `mod tests::phase32_role_playing_tests` (§6.2 move 6,
//! code-review 2026-07-11) — behaviour-named files replace the phase-named
//! archaeology. `use super::*` resolves against `crate::expand`'s re-exports.

use super::*;
use crate::expand::test_helpers::TestFixtureExt;
use crate::model::{Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef};

fn flights_airports_def() -> SemanticViewDefinition {
    SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "f".to_string(),
                table: "flights".to_string(),
                pk_columns: vec!["flight_id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "a".to_string(),
                table: "airports".to_string(),
                pk_columns: vec!["airport_code".to_string()],
                ..Default::default()
            },
        ],
        dimensions: vec![
            Dimension {
                name: "city".to_string(),
                expr: "a.city".to_string(),
                source_table: Some("a".to_string()),
                ..Default::default()
            },
            Dimension {
                name: "country".to_string(),
                expr: "a.country".to_string(),
                source_table: Some("a".to_string()),
                ..Default::default()
            },
            Dimension {
                name: "carrier".to_string(),
                expr: "f.carrier".to_string(),
                source_table: Some("f".to_string()),
                ..Default::default()
            },
        ],
        metrics: vec![
            Metric {
                name: "departure_count".to_string(),
                expr: "COUNT(*)".to_string(),
                source_table: Some("f".to_string()),
                using_relationships: vec!["dep_airport".to_string()],
                ..Default::default()
            },
            Metric {
                name: "arrival_count".to_string(),
                expr: "COUNT(*)".to_string(),
                source_table: Some("f".to_string()),
                using_relationships: vec!["arr_airport".to_string()],
                ..Default::default()
            },
            Metric {
                name: "total_flights".to_string(),
                expr: "departure_count + arrival_count".to_string(),
                ..Default::default()
            },
        ],
        joins: vec![
            Join {
                table: "a".to_string(),
                from_alias: "f".to_string(),
                fk_columns: vec!["departure_code".to_string()],
                ref_columns: vec!["airport_code".to_string()],
                name: Some("dep_airport".to_string()),
                cardinality: Cardinality::ManyToOne,
                ..Default::default()
            },
            Join {
                table: "a".to_string(),
                from_alias: "f".to_string(),
                fk_columns: vec!["arrival_code".to_string()],
                ref_columns: vec!["airport_code".to_string()],
                name: Some("arr_airport".to_string()),
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

/// `flights_airports_def` extended with a `regions` table that `airports`
/// references — a DESCENDANT of the role-playing target `a` (EXP-4).
fn flights_airports_regions_def() -> SemanticViewDefinition {
    let mut def = flights_airports_def();
    def.tables.push(TableRef {
        alias: "r".to_string(),
        table: "regions".to_string(),
        pk_columns: vec!["region_id".to_string()],
        ..Default::default()
    });
    def.dimensions.push(Dimension {
        name: "region_name".to_string(),
        expr: "r.region_name".to_string(),
        source_table: Some("r".to_string()),
        ..Default::default()
    });
    def.joins.push(Join {
        table: "r".to_string(),
        from_alias: "a".to_string(),
        fk_columns: vec!["region_id".to_string()],
        ref_columns: vec!["region_id".to_string()],
        name: Some("airport_region".to_string()),
        cardinality: Cardinality::ManyToOne,
        ..Default::default()
    });
    def
}

#[test]
fn descendant_of_role_playing_table_errors_ambiguous() {
    // EXP-4 (code-review 2026-07-18): `region_name` is on `r`, a descendant of
    // the role-playing table `a` (flights reach `a` via BOTH dep_airport and
    // arr_airport). `r` therefore hangs off whichever airport instance the
    // join resolver picks first (departure), regardless of the queried metric's
    // USING -- a silent, declaration-order-dependent wrong grouping. Reaching a
    // table only through a role-playing table is ambiguous and must error, just
    // as a dimension directly on `a` does.
    let def = flights_airports_regions_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region_name")],
        metrics: vec![MetricName::new("arrival_count")],
    };
    let err = expand("test_flights", &def, &req).unwrap_err();
    match err {
        ExpandError::AmbiguousDescendantPath {
            view_name,
            dimension_name,
            dimension_table,
            role_playing_table,
            available_relationships,
        } => {
            assert_eq!(view_name, "test_flights");
            assert_eq!(dimension_name, "region_name");
            assert_eq!(dimension_table, "r");
            assert_eq!(role_playing_table, "a");
            assert!(
                available_relationships.contains(&"dep_airport".to_string())
                    && available_relationships.contains(&"arr_airport".to_string()),
                "both airport relationships must be listed: {available_relationships:?}"
            );
        }
        other => panic!("Expected AmbiguousDescendantPath, got: {other}"),
    }
}

#[test]
fn descendant_through_single_relationship_still_resolves() {
    // Guard against over-rejection: when the intermediate table is reached by a
    // SINGLE relationship (not role-playing), a dimension on its descendant is
    // unambiguous and must still expand. Here only `dep_airport` connects
    // flights to airports, so `region_name` on `r` has one join path.
    let mut def = flights_airports_regions_def();
    // Drop the second (arr) relationship so `a` is no longer role-playing.
    def.joins
        .retain(|j| j.name.as_deref() != Some("arr_airport"));
    def.metrics
        .retain(|m| m.name != "arrival_count" && m.name != "total_flights");
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("region_name")],
        metrics: vec![MetricName::new("departure_count")],
    };
    let sql = expand("test_flights", &def, &req).expect("single-path descendant must resolve");
    assert!(sql.contains("region_name"), "SQL: {sql}");
}

#[test]
fn using_metric_generates_scoped_join_alias() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![MetricName::new("departure_count")],
    };
    let sql = expand("test_flights", &def, &req).unwrap();
    assert!(
        sql.contains("a__dep_airport"),
        "Scoped alias a__dep_airport must appear: {sql}"
    );
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
        "LEFT JOIN with scoped alias must appear: {sql}"
    );
}

#[test]
fn two_using_metrics_generate_two_scoped_joins() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("carrier")],
        metrics: vec![
            MetricName::new("departure_count"),
            MetricName::new("arrival_count"),
        ],
    };
    let sql = expand("test_flights", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
        "dep_airport scoped JOIN must appear: {sql}"
    );
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
        "arr_airport scoped JOIN must appear: {sql}"
    );
}

#[test]
fn dimension_rewritten_to_scoped_alias() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![MetricName::new("departure_count")],
    };
    let sql = expand("test_flights", &def, &req).unwrap();
    assert!(
        sql.contains("a__dep_airport.city"),
        "Dimension must be rewritten to scoped alias: {sql}"
    );
}

#[test]
fn ambiguous_dimension_without_using_produces_error() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![],
    };
    let result = expand("test_flights", &def, &req);
    assert!(result.is_err(), "Ambiguous dimension must produce error");
    match result.unwrap_err() {
        ExpandError::AmbiguousPath {
            view_name,
            dimension_name,
            dimension_table,
            available_relationships,
        } => {
            assert_eq!(view_name, "test_flights");
            assert_eq!(dimension_name, "city");
            assert_eq!(dimension_table, "a");
            assert!(available_relationships.contains(&"dep_airport".to_string()));
            assert!(available_relationships.contains(&"arr_airport".to_string()));
        }
        other => panic!("Expected AmbiguousPath, got: {other}"),
    }
}

#[test]
fn ambiguous_path_error_lists_relationships() {
    let err = ExpandError::AmbiguousPath {
        view_name: "test_flights".to_string(),
        dimension_name: "city".to_string(),
        dimension_table: "a".to_string(),
        available_relationships: vec!["dep_airport".to_string(), "arr_airport".to_string()],
    };
    let msg = format!("{err}");
    assert!(msg.contains("test_flights"));
    assert!(msg.contains("city"));
    assert!(msg.contains("ambiguous"));
    assert!(msg.contains("dep_airport"));
    assert!(msg.contains("arr_airport"));
}

#[test]
fn non_ambiguous_single_relationship_works_without_using() {
    let mut def = SemanticViewDefinition::default()
        .with_table("orders", "orders", &[])
        .with_table("o", "orders", &["id"])
        .with_table("c", "customers", &["id"])
        .with_dimension("customer_name", "c.name", Some("c"))
        .with_metric("revenue", "SUM(o.amount)", Some("o"));
    def.joins.push(Join {
        table: "c".to_string(),
        from_alias: "o".to_string(),
        fk_columns: vec!["customer_id".to_string()],
        name: Some("order_to_customer".to_string()),
        ..Default::default()
    });
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("customer_name")],
        metrics: vec![MetricName::new("revenue")],
    };
    let result = expand("test", &def, &req);
    assert!(
        result.is_ok(),
        "Single relationship must work without USING: {:?}",
        result.err()
    );
}

#[test]
fn base_table_dimension_works_unchanged() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("carrier")],
        metrics: vec![MetricName::new("departure_count")],
    };
    let sql = expand("test_flights", &def, &req).unwrap();
    assert!(
        sql.contains("f.carrier AS \"carrier\""),
        "Base table dimension must appear unchanged: {sql}"
    );
}

#[test]
fn fan_trap_detection_works_with_using_paths() {
    let def = SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "f".to_string(),
                table: "flights".to_string(),
                pk_columns: vec!["flight_id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "a".to_string(),
                table: "airports".to_string(),
                pk_columns: vec!["airport_code".to_string()],
                ..Default::default()
            },
        ],
        dimensions: vec![Dimension {
            name: "carrier".to_string(),
            expr: "f.carrier".to_string(),
            source_table: Some("f".to_string()),
            ..Default::default()
        }],
        metrics: vec![Metric {
            name: "airport_count".to_string(),
            expr: "COUNT(*)".to_string(),
            source_table: Some("a".to_string()),
            ..Default::default()
        }],
        joins: vec![Join {
            table: "a".to_string(),
            from_alias: "f".to_string(),
            fk_columns: vec!["dep_airport_code".to_string()],
            ref_columns: vec!["airport_code".to_string()],
            name: Some("dep_flights".to_string()),
            cardinality: Cardinality::ManyToOne,
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
        dimensions: vec![DimensionName::new("carrier")],
        metrics: vec![MetricName::new("airport_count")],
    };
    let result = expand("test", &def, &req);
    assert!(result.is_err(), "Fan trap must still be detected");
    match result.unwrap_err() {
        ExpandError::FanTrap { .. } => {}
        other => panic!("Expected FanTrap, got: {other}"),
    }
}

#[test]
fn derived_metric_with_two_using_resolves_both_joins() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("carrier")],
        metrics: vec![MetricName::new("total_flights")],
    };
    let sql = expand("test_flights", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a__dep_airport\""),
        "Derived metric must resolve dep_airport join: {sql}"
    );
    assert!(
        sql.contains("LEFT JOIN \"airports\" AS \"a__arr_airport\""),
        "Derived metric must resolve arr_airport join: {sql}"
    );
}

#[test]
fn metric_using_from_base_table_no_unnecessary_join() {
    let def = SemanticViewDefinition {
        tables: vec![TableRef {
            alias: "o".to_string(),
            table: "orders".to_string(),
            pk_columns: vec!["id".to_string()],
            ..Default::default()
        }],
        dimensions: vec![Dimension {
            name: "region".to_string(),
            expr: "o.region".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],
        metrics: vec![Metric {
            name: "cnt".to_string(),
            expr: "COUNT(*)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],
        joins: vec![],
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
        metrics: vec![MetricName::new("cnt")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        !sql.contains("JOIN"),
        "No JOIN needed when everything is on base table: {sql}"
    );
}

#[test]
fn backward_compat_no_using_expands_as_before() {
    let def = SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
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
        dimensions: vec![Dimension {
            name: "customer_name".to_string(),
            expr: "c.name".to_string(),
            source_table: Some("c".to_string()),
            ..Default::default()
        }],
        metrics: vec![Metric {
            name: "revenue".to_string(),
            expr: "SUM(o.amount)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }],
        joins: vec![Join {
            table: "c".to_string(),
            from_alias: "o".to_string(),
            fk_columns: vec!["customer_id".to_string()],
            name: Some("order_to_customer".to_string()),
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
        dimensions: vec![DimensionName::new("customer_name")],
        metrics: vec![MetricName::new("revenue")],
    };
    let sql = expand("test", &def, &req).unwrap();
    assert!(
        sql.contains("LEFT JOIN \"customers\" AS \"c\""),
        "Non-USING definition must use bare alias: {sql}"
    );
    assert!(
        sql.contains("c.name AS"),
        "Dimension expr must use bare alias: {sql}"
    );
}

#[test]
fn ambiguous_dimension_with_derived_metric_using_both_paths() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![MetricName::new("total_flights")],
    };
    let result = expand("test_flights", &def, &req);
    assert!(
        result.is_err(),
        "City dimension must be ambiguous when derived metric uses both paths"
    );
    match result.unwrap_err() {
        ExpandError::AmbiguousPath { .. } => {}
        other => panic!("Expected AmbiguousPath, got: {other}"),
    }
}

#[test]
fn scoped_join_on_clause_uses_correct_fk_pk() {
    let def = flights_airports_def();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec![DimensionName::new("city")],
        metrics: vec![MetricName::new("departure_count")],
    };
    let sql = expand("test_flights", &def, &req).unwrap();
    assert!(
        sql.contains("\"f\".\"departure_code\" = \"a__dep_airport\".\"airport_code\""),
        "Scoped JOIN ON clause must use correct FK/PK: {sql}"
    );
}
