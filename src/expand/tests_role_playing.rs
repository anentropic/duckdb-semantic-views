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
        resolution_schema_name: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        resolution_schema_name: None,
        comment: None,
    };
    let req = QueryRequest {
        where_clause: None,
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
        where_clause: None,
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
        resolution_schema_name: None,
        comment: None,
    };
    let req = QueryRequest {
        where_clause: None,
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
        resolution_schema_name: None,
        comment: None,
    };
    let req = QueryRequest {
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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

// ---------------------------------------------------------------------------
// EXP-10 (code-review 2026-08-03): a `where_clause` member on a role-playing
// table must not bind silently to the first-declared relationship.
//
// Role-playing ambiguity is checked for queried DIMENSIONS (`find_using_context`,
// Phase 32) and, on the facts path, for queried dims and facts (EXP-5) — but a
// `where_clause` member's table is in neither set. It rides
// `resolve_joins_pkfk`'s `fact_source_tables` parameter, whose loop (unlike the
// dimension loop) never consults `role_playing_bare_aliases`, so the bare alias
// joins via `tree_parent` — the first-declared edge.
//
// Only a queried dimension's *expression* is rewritten to a scoped alias, so a
// where-member has no way to name its role; per EXP-4/EXP-5 the correct posture
// is to fail loudly rather than pick an edge by declaration order.
// ---------------------------------------------------------------------------

#[test]
fn where_clause_on_role_playing_table_is_rejected() {
    let def = flights_airports_def();
    let req = QueryRequest {
        where_clause: Some("city = 'NYC'".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("departure_count")],
    };
    let result = expand("flights_sv", &def, &req);
    // The specific variant, not merely "an error": a bare `is_err()` would go
    // green on any unrelated failure and stop guarding EXP-10.
    match result {
        Err(ExpandError::AmbiguousWhereClausePath {
            view_name,
            member_name,
            member_table,
            role_playing_table,
            available_relationships,
        }) => {
            assert_eq!(view_name, "flights_sv");
            assert_eq!(member_name, "city");
            assert_eq!(member_table, "a");
            assert_eq!(role_playing_table, "a");
            // The error must name BOTH roles -- that list is what tells the
            // user how to resolve the ambiguity.
            assert!(
                available_relationships.contains(&"dep_airport".to_string())
                    && available_relationships.contains(&"arr_airport".to_string()),
                "error must list both roles, got: {available_relationships:?}"
            );
        }
        Err(other) => panic!("expected AmbiguousWhereClausePath, got: {other}"),
        Ok(sql) => panic!(
            "a where_clause member on a role-playing table must not bind to the \
             first-declared relationship silently. Emitted SQL:\n{sql}"
        ),
    }
}

/// The wrong-number shape the rejection exists to prevent: the metric's
/// `USING (arr_airport)` scopes its join to `a__arr_airport`, while the
/// predicate's `a` is joined via the first-declared `dep_airport`. Filtering
/// on departure city while counting arrivals is a silently wrong answer, not a
/// different-but-defensible one.
#[test]
fn where_clause_role_playing_does_not_filter_through_the_other_role() {
    let def = flights_airports_def();
    let req = QueryRequest {
        where_clause: Some("city = 'NYC'".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("arrival_count")],
    };
    let result = expand("flights_sv", &def, &req);
    match result {
        Err(ExpandError::AmbiguousWhereClausePath {
            member_name,
            role_playing_table,
            ..
        }) => {
            assert_eq!(member_name, "city");
            assert_eq!(role_playing_table, "a");
        }
        Err(other) => panic!(
            "expected AmbiguousWhereClausePath -- a different error means this \
             case is no longer guarding the wrong-role filter: {other}"
        ),
        Ok(sql) => panic!(
            "expected a role-playing ambiguity error; emitted SQL instead (the \
             predicate binds to the dep_airport instance while the metric uses \
             arr_airport):\n{sql}"
        ),
    }
}

/// Control: the same predicate on a NON-role-playing table's member stays
/// legal. Without this the fix above could be "reject every where_clause on a
/// joined table" and still look green.
#[test]
fn where_clause_on_non_role_playing_table_still_allowed() {
    let def = flights_airports_def();
    let req = QueryRequest {
        where_clause: Some("carrier = 'AA'".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("departure_count")],
    };
    let result = expand("flights_sv", &def, &req);
    assert!(
        result.is_ok(),
        "a where_clause on the base table's own dimension is unambiguous: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// EXP-31 (code-review 2026-08-08): the role-playing twin of EXP-27. A
// `where_clause` member whose FACT CHAIN reaches a ROLE-PLAYING table had that
// table joined with no ambiguity check at all.
//
// EXP-10 (above) closed the case where the member is *declared* on the
// role-playing table. #207 (EXP-23) then taught `resolve_where_clause` to
// contribute the tables a member reaches THROUGH its fact references to
// `source_tables`, so the join resolver joins those too — and
// `check_where_clause_role_playing_path` still read only `member.table`. The
// reached table therefore rode `fact_source_tables` straight to `tree_parent`,
// the first-declared relationship: exactly the silent, declaration-order-
// dependent binding EXP-10 exists to prevent, reached one hop further along.
//
// Same blind spot #207 opened for fan traps (EXP-27) at a different fence,
// which is why both now walk `WhereMember::fact_tables` alongside
// `WhereMember::table`.
// ---------------------------------------------------------------------------

/// `flights_airports_def` plus a fact on the ROLE-PLAYING table `a` and a fact
/// on the base whose expression reaches it, so a predicate naming the base
/// fact pulls `a` in without ever naming it.
///
/// A fact on `a` is not itself illegal: `check_fact_role_playing_path` rejects
/// one when it is *queried*, and a `where_clause` member is not queried.
fn flights_airports_fact_chain_def() -> SemanticViewDefinition {
    flights_airports_def()
        // A plain metric with no USING, so nothing else in the query supplies a
        // role and the ambiguity is the member's alone.
        .with_metric("flight_count", "COUNT(*)", Some("f"))
        .with_fact("ap_elev", "a.elevation", "a")
        .with_fact("f_elev", "ap_elev + 1", "f")
}

/// The FACT branch: the predicate names a fact on the base whose expression
/// reaches a fact on the role-playing table.
#[test]
fn exp31_where_member_fact_chain_to_a_role_playing_table_is_rejected() {
    let def = flights_airports_fact_chain_def();
    let req = QueryRequest {
        where_clause: Some("f_elev > 0".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("flight_count")],
    };
    match expand("flights_sv", &def, &req) {
        Err(ExpandError::AmbiguousWhereClausePath {
            member_name,
            member_table,
            role_playing_table,
            available_relationships,
            ..
        }) => {
            assert_eq!(member_name, "f_elev");
            assert_eq!(
                member_table, "a",
                "the error must name the table joined on the member's behalf; \
                 the table the member is declared on is unambiguous"
            );
            assert_eq!(role_playing_table, "a");
            assert_eq!(
                available_relationships,
                vec!["dep_airport".to_string(), "arr_airport".to_string()],
                "the message must list the roles the user has to choose between"
            );
        }
        Err(other) => panic!(
            "expected AmbiguousWhereClausePath — a different error means this \
             case stopped guarding EXP-31: {other}"
        ),
        Ok(sql) => panic!(
            "EXP-31: the role-playing table was joined for a where_clause fact \
             chain with no ambiguity check, so the predicate binds to the \
             first-declared relationship (dep_airport) silently.\n\
             emitted SQL:\n{sql}"
        ),
    }
}

/// The DIMENSION branch: TECH-DEBT #54 made predicate dimensions' expressions
/// fact-inlined too, so a dimension on the base reaching the same fact takes
/// the identical unchecked path.
#[test]
fn exp31_where_member_dimension_reaching_a_role_playing_fact_is_rejected() {
    let def = flights_airports_fact_chain_def().with_dimension(
        "high_altitude",
        "CASE WHEN ap_elev > 5000 THEN 'hi' ELSE 'lo' END",
        Some("f"),
    );
    let req = QueryRequest {
        where_clause: Some("high_altitude = 'hi'".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("flight_count")],
    };
    match expand("flights_sv", &def, &req) {
        Err(ExpandError::AmbiguousWhereClausePath {
            member_name,
            member_table,
            ..
        }) => {
            assert_eq!(member_name, "high_altitude");
            assert_eq!(member_table, "a");
        }
        Err(other) => panic!("expected AmbiguousWhereClausePath, got: {other}"),
        Ok(sql) => panic!(
            "EXP-31 (dimension branch): a where_clause DIMENSION whose \
             expression reaches a fact on the role-playing table joined it \
             unchecked.\nemitted SQL:\n{sql}"
        ),
    }
}

/// Transitivity: the reached set walks the whole chain, so a two-hop chain
/// that only arrives at the role-playing table at its far end is rejected too.
/// A check that looked at the directly-named fact alone would miss this.
#[test]
fn exp31_transitive_fact_chain_to_a_role_playing_table_is_rejected() {
    let def = flights_airports_fact_chain_def().with_fact("f_elev2", "f_elev * 2", "f");
    let req = QueryRequest {
        where_clause: Some("f_elev2 > 0".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("flight_count")],
    };
    match expand("flights_sv", &def, &req) {
        Err(ExpandError::AmbiguousWhereClausePath { member_name, .. }) => {
            assert_eq!(member_name, "f_elev2");
        }
        Err(other) => panic!("expected AmbiguousWhereClausePath, got: {other}"),
        Ok(sql) => panic!(
            "EXP-31 (transitive): `f_elev2 -> f_elev -> ap_elev@a` reached the \
             role-playing table through two hops unchecked.\n\
             emitted SQL:\n{sql}"
        ),
    }
}

/// CONTROL — a fact chain reaching a NON-role-playing table must still expand.
/// Without this the fix could be "reject every where_clause fact chain" and
/// still look green. The same shape as the tests above with one relationship
/// instead of two, so nothing but the role-playing multi-edge differs.
#[test]
fn exp31_control_fact_chain_to_a_non_role_playing_table_still_expands() {
    let def = SemanticViewDefinition::default()
        .with_table("f", "exp31_flights", &["flight_id"])
        .with_table("a", "exp31_airports", &["airport_code"])
        .with_metric("flight_count", "COUNT(*)", Some("f"))
        .with_fact("ap_elev", "a.elevation", "a")
        .with_fact("f_elev", "ap_elev + 1", "f")
        .with_pkfk_join(
            "dep_airport",
            "f",
            "a",
            &["departure_code"],
            &["airport_code"],
        );
    let req = QueryRequest {
        where_clause: Some("f_elev > 0".to_string()),
        facts: vec![],
        dimensions: vec![],
        metrics: vec![MetricName::new("flight_count")],
    };
    let sql = expand("flights_sv", &def, &req)
        .expect("one relationship to the target means no role to disambiguate");
    assert!(
        sql.contains("exp31_airports"),
        "the table reached through the fact chain must still be joined: {sql}"
    );
}
