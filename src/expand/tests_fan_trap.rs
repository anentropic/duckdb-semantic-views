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
        resolution_schema_name: None,
        comment: None,
    }
}

#[test]
fn fan_trap_one_to_many_blocked() {
    let def = fan_trap_three_table_def();
    let req = QueryRequest {
        where_clause: None,
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
        where_clause: None,
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
        resolution_schema_name: None,
        comment: None,
    };
    let req = QueryRequest {
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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
        where_clause: None,
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

#[test]
fn cyclic_relationships_are_rejected_by_expand() {
    // #141 (fuzz_sql_expand OOM): relationships forming a cycle (a -> b via r1,
    // b -> a via r2) are parser-reachable and previously drove
    // `check_fan_traps`' JoinTree parent-walk into an infinite loop, allocating
    // until OOM. d48abee made the walks terminate and left the Ok/Err outcome
    // deliberately unspecified — so `expand` returned Ok, and the fence CERTIFIED
    // the cyclic definition (EXP-15): `fanning_edge_on_path` sees the forward
    // edge `(a, b)` and calls the hop safe without considering the reverse
    // `ManyToOne` edge `(b, a)` that fans.
    //
    // EXP-15 specifies the outcome: `build_relationship_graph` re-runs the
    // CREATE-time cycle check, so this is now `UncheckableDefinition`.
    //
    // Note what this test no longer proves: the fence short-circuits BEFORE the
    // JoinTree parent-walk, so it is not the #141 termination guard any more.
    // That guard's direct coverage is
    // `graph::join_tree::tests::walks_terminate_on_cyclic_parent_map`, which
    // exercises the walk on a cyclic parent map without going through the fence.
    let def = SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "a".to_string(),
                table: "ta".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "b".to_string(),
                table: "tb".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
        ],
        dimensions: vec![Dimension {
            name: "d".to_string(),
            expr: "a.c".to_string(),
            source_table: Some("a".to_string()),
            ..Default::default()
        }],
        metrics: vec![Metric {
            name: "m".to_string(),
            expr: "sum(b.v)".to_string(),
            source_table: Some("b".to_string()),
            ..Default::default()
        }],
        joins: vec![
            Join {
                from_alias: "a".to_string(),
                table: "b".to_string(),
                fk_columns: vec!["bid".to_string()],
                ref_columns: vec!["id".to_string()],
                name: Some("r1".to_string()),
                cardinality: Cardinality::ManyToOne,
            },
            Join {
                from_alias: "b".to_string(),
                table: "a".to_string(),
                fk_columns: vec!["aid".to_string()],
                ref_columns: vec!["id".to_string()],
                name: Some("r2".to_string()),
                cardinality: Cardinality::ManyToOne,
            },
        ],
        ..Default::default()
    };
    let req = QueryRequest {
        where_clause: None,
        dimensions: vec!["d".into()],
        metrics: vec!["m".into()],
        facts: vec![],
    };
    match expand("v", &def, &req) {
        Err(ExpandError::UncheckableDefinition { view_name, reason }) => {
            assert_eq!(view_name, "v");
            assert!(
                reason.contains("cycle"),
                "reason should name the cycle: {reason}"
            );
        }
        other => panic!("Expected UncheckableDefinition, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// EXP-9 (code-review 2026-08-03): the fence must check the NON ADDITIVE BY
// dimension's OWN table, not just the queried dimensions.
//
// EXP-3 made active semi-additive metrics subject to the metric x dimension and
// metric x metric checks, but the table joined *because of the un-queried NA
// dimension itself* is exempt from every check. `collect_na_dim_source_tables`
// joins it into the snapshot CTE exactly as a queried dimension's table is
// joined, so when that join fans, the RANK runs over duplicated source rows —
// and ties across the fanned copies of one row are indistinguishable from ties
// across distinct rows, so the CTE cannot dedupe them (silent double-count).
//
// `snapshot_cte_anchor` does not rescue this shape: it returns None when the
// metric is already at the root grain, leaving the base-anchored fanned join.
// ---------------------------------------------------------------------------

/// Root `o` (orders) with child `li` (line_items) on the many side, a metric at
/// the ROOT grain, and its `NON ADDITIVE BY` dimension on the child.
fn semi_additive_na_dim_fans_def() -> SemanticViewDefinition {
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
        ],
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "o.region".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            },
            Dimension {
                name: "ship_ts".to_string(),
                expr: "li.ship_ts".to_string(),
                source_table: Some("li".to_string()),
                ..Default::default()
            },
        ],
        metrics: vec![Metric {
            name: "balance_at".to_string(),
            expr: "SUM(o.balance)".to_string(),
            source_table: Some("o".to_string()),
            non_additive_by: vec![crate::model::NonAdditiveDim {
                dimension: "ship_ts".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        joins: vec![Join {
            table: "o".to_string(),
            from_alias: "li".to_string(),
            fk_columns: vec!["order_id".to_string()],
            ref_columns: vec!["id".to_string()],
            name: Some("li_to_order".to_string()),
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
    }
}

#[test]
fn semi_additive_na_dim_on_fanning_table_blocked() {
    let def = semi_additive_na_dim_fans_def();
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("balance_at")],
    };
    let result = expand("sales", &def, &req);
    // Assert the specific variant AND its payload: a bare `is_err()` would go
    // green on any unrelated failure (a mistyped metric name resolving to
    // `UnknownMetric`, say) and quietly stop guarding EXP-9.
    match result {
        Err(ExpandError::FanTrap { detail }) => {
            assert_eq!(detail.view_name, "sales");
            assert_eq!(detail.metric_name, "balance_at");
            // The NA dimension is named as the fanning participant even though
            // it was never queried -- that is the whole point of EXP-9.
            assert_eq!(detail.dimension_name, "ship_ts");
            assert_eq!(detail.dimension_table, "li");
            assert_eq!(detail.relationship_name, "li_to_order");
        }
        Err(other) => panic!("expected FanTrap naming the NA dimension, got: {other}"),
        Ok(sql) => panic!(
            "the NA dimension's table fans the metric's grain inside the snapshot \
             CTE -- RANK ties across the duplicated rows double-count the metric. \
             Emitted SQL instead:\n{sql}"
        ),
    }
}

/// The asymmetry that makes EXP-9 unmistakable: *querying* the same dimension
/// makes the metric regular and the fence already rejects the identical join.
/// The un-queried case must not be the permissive one.
#[test]
fn semi_additive_na_dim_queried_is_already_blocked() {
    let def = semi_additive_na_dim_fans_def();
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("ship_ts")],
        metrics: vec![MetricName::new("balance_at")],
    };
    let result = expand("sales", &def, &req);
    match result {
        Err(ExpandError::FanTrap { detail }) => {
            assert_eq!(detail.metric_name, "balance_at");
            assert_eq!(detail.dimension_name, "ship_ts");
        }
        Err(other) => panic!(
            "control: the queried dimension on the fanning child must still be a \
             FanTrap, got: {other}"
        ),
        Ok(sql) => panic!("control: this has always been a FanTrap; got SQL:\n{sql}"),
    }
}

/// Control: an NA dimension on a table that does NOT fan the metric's grain
/// stays legal, so the fix cannot degenerate into "reject every NA dimension
/// on another table".
#[test]
fn semi_additive_na_dim_on_non_fanning_table_allowed() {
    let mut def = semi_additive_na_dim_fans_def();
    // Flip the edge: `o` now references `li`, so reaching `li` from `o` crosses
    // the many-to-one edge forwards -- one `li` row per `o` row, no fan-out.
    def.joins = vec![Join {
        table: "li".to_string(),
        from_alias: "o".to_string(),
        fk_columns: vec!["ship_id".to_string()],
        ref_columns: vec!["id".to_string()],
        name: Some("order_to_shipment".to_string()),
        cardinality: Cardinality::ManyToOne,
        ..Default::default()
    }];
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("balance_at")],
    };
    let result = expand("sales", &def, &req);
    assert!(
        result.is_ok(),
        "a many-to-one NA-dim join does not fan the metric: {:?}",
        result.err()
    );
}

/// The error payload reports the source table **as declared**, not folded.
///
/// Copilot review on PR #189: the EXP-9 branch initially put the lowercased
/// lookup key in `FanTrapError.dimension_table`, while the queried-dimension
/// branch reports `dim.source_table` verbatim. Path and cardinality lookups
/// still use the folded key — only the message changed — so this pins the two
/// apart with a mixed-case alias, which the lowercase-aliased tests above
/// cannot distinguish.
#[test]
fn semi_additive_na_dim_fan_trap_reports_declared_table_spelling() {
    let mut def = semi_additive_na_dim_fans_def();
    // Re-declare the child table under a mixed-case alias, wiring every
    // reference to it (the dimension's source, its expression, and the
    // relationship's from side) to the new spelling.
    def.tables[1].alias = "LiNe".to_string();
    def.dimensions[1].source_table = Some("LiNe".to_string());
    def.dimensions[1].expr = "LiNe.ship_ts".to_string();
    def.joins[0].from_alias = "LiNe".to_string();

    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("region")],
        metrics: vec![MetricName::new("balance_at")],
    };
    match expand("sales", &def, &req) {
        Err(ExpandError::FanTrap { detail }) => assert_eq!(
            detail.dimension_table, "LiNe",
            "the error must echo the declared spelling, not the folded lookup key"
        ),
        Err(other) => panic!("expected FanTrap, got: {other}"),
        Ok(sql) => panic!("expected FanTrap; got SQL:\n{sql}"),
    }
}

// ---------------------------------------------------------------------------
// EXP-12 (code-review 2026-08-03): a QUOTED window inner-metric reference must
// behave exactly like its unquoted spelling.
//
// Four sites resolve `WindowSpec::inner_metric`: the CREATE-time validator
// (`body_parser`), the fan-trap fence's `metric_grain`, the per-grain
// planner's `window_cte_anchor`, and the emitter (`expand::window`). Only the
// emitter used the canonical identifier key; the other three compared raw
// spellings, so `"total"` did not match a metric stored as `total`.
//
// The CREATE-side strictness masked the rest: a definition with a quoted
// reference could not be stored, so the fence never saw one. Migrating CREATE
// alone would have opened a silent inflation path -- the fence would lose the
// inner aggregate's grain, `RootGrainFanTrap` would not fire, and the
// base-anchored `__sv_agg` CTE would compute the inner aggregate over a fanned
// join while the emitter happily resolved the same reference. So all four move
// together, and the invariant tested here is the one that ties them: the
// quoted and unquoted spellings must produce byte-identical outcomes.
// ---------------------------------------------------------------------------

/// Base `b` with parent `p` (b references p, so p is the "one" side and sits
/// ABOVE the root grain). A window metric on `b` whose inner aggregate lives on
/// `p` — the shape whose grain the fence must see through the reference.
fn window_inner_ref_def(inner_ref: &str) -> SemanticViewDefinition {
    SemanticViewDefinition {
        tables: vec![
            TableRef {
                alias: "b".to_string(),
                table: "base".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
            TableRef {
                alias: "p".to_string(),
                table: "parent".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            },
        ],
        dimensions: vec![Dimension {
            name: "seg".to_string(),
            expr: "p.seg".to_string(),
            source_table: Some("p".to_string()),
            ..Default::default()
        }],
        metrics: vec![
            Metric {
                name: "total".to_string(),
                expr: "SUM(p.amount)".to_string(),
                source_table: Some("p".to_string()),
                ..Default::default()
            },
            Metric {
                name: "w".to_string(),
                expr: "AVG(total)".to_string(),
                source_table: Some("b".to_string()),
                window_spec: Some(crate::model::WindowSpec {
                    window_function: "AVG".to_string(),
                    inner_metric: inner_ref.to_string(),
                    extra_args: vec![],
                    excluding_dims: vec![],
                    partition_dims: vec!["seg".to_string()],
                    order_by: vec![],
                    frame_clause: None,
                }),
                ..Default::default()
            },
        ],
        joins: vec![Join {
            table: "p".to_string(),
            from_alias: "b".to_string(),
            fk_columns: vec!["parent_id".to_string()],
            ref_columns: vec!["id".to_string()],
            name: Some("b_to_p".to_string()),
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
    }
}

/// The EXP-12 invariant: quoting the inner-metric reference changes nothing.
///
/// Asserting equality of the two OUTCOMES (rather than pinning one specific
/// answer) is deliberate — it holds whether the shape is answered by anchoring
/// the CTE at the inner aggregate's grain or rejected by the fence, and it
/// keeps holding if that decision is ever revisited. What it forbids is the
/// asymmetry: the unquoted spelling seeing the inner grain while the quoted one
/// silently does not.
#[test]
fn window_quoted_inner_metric_reference_behaves_like_unquoted() {
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("seg")],
        metrics: vec![MetricName::new("w")],
    };
    let plain = expand("wq", &window_inner_ref_def("total"), &req);
    let quoted = expand("wq", &window_inner_ref_def("\"total\""), &req);

    match (&plain, &quoted) {
        (Ok(a), Ok(b)) => assert_eq!(
            a, b,
            "quoted and unquoted inner-metric references must emit identical SQL"
        ),
        (Err(a), Err(b)) => assert_eq!(
            a.to_string(),
            b.to_string(),
            "quoted and unquoted inner-metric references must fail identically"
        ),
        (Ok(sql), Err(e)) => panic!(
            "unquoted resolved but quoted did not -- the quoted reference lost \
             the inner aggregate's grain.\nunquoted SQL:\n{sql}\nquoted error: {e}"
        ),
        (Err(e), Ok(sql)) => {
            panic!("quoted resolved but unquoted did not.\nunquoted error: {e}\nquoted SQL:\n{sql}")
        }
    }
}

/// The same invariant with the reference's CASE varied inside the quotes.
/// DuckDB folds case whether or not a name is quoted, so `"TOTAL"` is `total`.
#[test]
fn window_quoted_case_varied_inner_metric_reference_behaves_like_unquoted() {
    let req = QueryRequest {
        where_clause: None,
        facts: vec![],
        dimensions: vec![DimensionName::new("seg")],
        metrics: vec![MetricName::new("w")],
    };
    let plain = expand("wq", &window_inner_ref_def("total"), &req);
    let quoted = expand("wq", &window_inner_ref_def("\"TOTAL\""), &req);
    assert_eq!(
        plain.as_ref().map_err(std::string::ToString::to_string),
        quoted.as_ref().map_err(std::string::ToString::to_string),
        "case inside quotes is folded too, so the outcomes must match"
    );
}

/// The fence-side half stated directly: the inner aggregate's table is part of
/// the window metric's grain no matter how the reference is spelled. This is
/// what would have been lost had the CREATE-side check migrated alone.
#[test]
fn window_quoted_inner_metric_contributes_its_grain_to_the_fence() {
    for reference in ["total", "\"total\"", "\"Total\""] {
        let def = window_inner_ref_def(reference);
        let met = def
            .metrics
            .iter()
            .find(|m| m.name == "w")
            .expect("window metric");
        let grains = crate::expand::fan_trap::metric_grain(met, &def).anchored("b");
        assert!(
            grains.iter().any(|t| t == "p"),
            "inner aggregate's table `p` must be in the grain set for reference \
             {reference}, got {grains:?}"
        );
    }
}
