use proptest::prelude::*;
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Fact, Join, JoinColumn, Metric, NonAdditiveDim,
    NullsOrder, SemanticViewDefinition, SortOrder, TableRef, WindowOrderBy, WindowSpec,
};

// ---------------------------------------------------------------------------
// Proptest strategies for model types
// ---------------------------------------------------------------------------

/// Generate an arbitrary non-empty string (1..=20 alphanumeric chars).
fn arb_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,19}".prop_map(|s| s.to_string())
}

/// Generate an arbitrary SQL-like expression.
fn arb_expr() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_name(),
        arb_name().prop_map(|n| format!("SUM({n})")),
        arb_name().prop_map(|n| format!("COUNT({n})")),
        arb_name().prop_map(|n| format!("AVG({n})")),
    ]
}

fn arb_sort_order() -> impl Strategy<Value = SortOrder> {
    prop_oneof![Just(SortOrder::Asc), Just(SortOrder::Desc),]
}

fn arb_nulls_order() -> impl Strategy<Value = NullsOrder> {
    prop_oneof![Just(NullsOrder::Last), Just(NullsOrder::First),]
}

fn arb_access() -> impl Strategy<Value = AccessModifier> {
    prop_oneof![Just(AccessModifier::Public), Just(AccessModifier::Private),]
}

fn arb_cardinality() -> impl Strategy<Value = Cardinality> {
    prop_oneof![Just(Cardinality::ManyToOne), Just(Cardinality::OneToOne),]
}

fn arb_table_ref() -> impl Strategy<Value = TableRef> {
    (
        arb_name(),
        arb_name(),
        proptest::collection::vec(arb_name(), 0..=2),
    )
        .prop_map(|(alias, table, pk_columns)| TableRef {
            alias,
            table,
            pk_columns,
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        })
}

fn arb_dimension() -> impl Strategy<Value = Dimension> {
    (arb_name(), arb_expr(), proptest::option::of(arb_name())).prop_map(
        |(name, expr, source_table)| Dimension {
            name,
            expr,
            source_table,
            output_type: None,
            comment: None,
            synonyms: vec![],
        },
    )
}

fn arb_non_additive_dim() -> impl Strategy<Value = NonAdditiveDim> {
    (arb_name(), arb_sort_order(), arb_nulls_order()).prop_map(|(dimension, order, nulls)| {
        NonAdditiveDim {
            dimension,
            order,
            nulls,
        }
    })
}

fn arb_window_order_by() -> impl Strategy<Value = WindowOrderBy> {
    (arb_name(), arb_sort_order(), arb_nulls_order())
        .prop_map(|(expr, order, nulls)| WindowOrderBy { expr, order, nulls })
}

fn arb_window_spec() -> impl Strategy<Value = WindowSpec> {
    (
        arb_name(),
        arb_name(),
        proptest::collection::vec(arb_name(), 0..=1),
        proptest::collection::vec(arb_window_order_by(), 0..=2),
    )
        .prop_map(
            |(window_function, inner_metric, excluding_dims, order_by)| WindowSpec {
                window_function,
                inner_metric,
                extra_args: vec![],
                excluding_dims,
                partition_dims: vec![],
                order_by,
                frame_clause: None,
            },
        )
}

fn arb_metric() -> impl Strategy<Value = Metric> {
    (
        arb_name(),
        arb_expr(),
        proptest::option::of(arb_name()),
        arb_access(),
        proptest::collection::vec(arb_non_additive_dim(), 0..=1),
        proptest::option::of(arb_window_spec()),
    )
        .prop_map(
            |(name, expr, source_table, access, non_additive_by, window_spec)| Metric {
                name,
                expr,
                source_table,
                output_type: None,
                using_relationships: vec![],
                comment: None,
                synonyms: vec![],
                access,
                non_additive_by,
                window_spec,
            },
        )
}

fn arb_fact() -> impl Strategy<Value = Fact> {
    (
        arb_name(),
        arb_expr(),
        proptest::option::of(arb_name()),
        arb_access(),
    )
        .prop_map(|(name, expr, source_table, access)| Fact {
            name,
            expr,
            source_table,
            output_type: None,
            comment: None,
            synonyms: vec![],
            access,
        })
}

fn arb_join_column() -> impl Strategy<Value = JoinColumn> {
    (arb_name(), arb_name()).prop_map(|(from, to)| JoinColumn { from, to })
}

fn arb_join() -> impl Strategy<Value = Join> {
    (
        arb_name(),
        arb_name(),
        proptest::collection::vec(arb_name(), 0..=2),
        proptest::collection::vec(arb_join_column(), 0..=2),
        arb_cardinality(),
    )
        .prop_map(
            |(table, from_alias, fk_columns, join_columns, cardinality)| Join {
                table,
                on: String::new(),
                from_cols: vec![],
                join_columns,
                from_alias,
                fk_columns,
                ref_columns: vec![],
                name: None,
                cardinality,
            },
        )
}

fn arb_definition() -> impl Strategy<Value = SemanticViewDefinition> {
    (
        arb_name(),
        proptest::collection::vec(arb_table_ref(), 0..=2),
        proptest::collection::vec(arb_dimension(), 1..=3),
        proptest::collection::vec(arb_metric(), 1..=3),
        proptest::collection::vec(arb_join(), 0..=2),
        proptest::collection::vec(arb_fact(), 0..=2),
        proptest::option::of("[a-z ]{1,30}"),
    )
        .prop_map(
            |(base_table, tables, dimensions, metrics, joins, facts, comment)| {
                SemanticViewDefinition {
                    base_table,
                    tables,
                    dimensions,
                    metrics,
                    joins,
                    facts,
                    column_type_names: vec![],
                    column_types_inferred: vec![],
                    created_on: None,
                    database_name: None,
                    schema_name: None,
                    comment,
                }
            },
        )
}

// ---------------------------------------------------------------------------
// Property-based test
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn yaml_json_roundtrip_equivalence(def in arb_definition()) {
        // Serialize the arbitrary struct to both JSON and YAML
        let json_str = serde_json::to_string(&def).expect("JSON serialize");
        let yaml_str = yaml_serde::to_string(&def).expect("YAML serialize");

        // Deserialize both
        let from_json = SemanticViewDefinition::from_json("proptest", &json_str)
            .expect("JSON deserialize");
        let from_yaml = SemanticViewDefinition::from_yaml("proptest", &yaml_str)
            .expect("YAML deserialize");

        // Assert structural equality
        prop_assert_eq!(from_json, from_yaml);
    }
}
