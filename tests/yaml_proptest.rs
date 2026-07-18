use proptest::prelude::*;
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Fact, Join, Materialization, Metric, NonAdditiveDim,
    NullsOrder, SemanticViewDefinition, SortOrder, TableRef, WindowOrderBy, WindowSpec,
};

// ---------------------------------------------------------------------------
// Proptest strategies for model types
// ---------------------------------------------------------------------------

/// Generate an arbitrary non-empty identifier. The alphabet includes quoted,
/// unicode, keyword, and whitespace-bearing arms (TC-3, code-review
/// 2026-07-02 — the previous [a-z][a-z0-9_]* alphabet systematically missed
/// the shapes behind the UTF-8 and quoting regressions; YAML must round-trip
/// them all as plain string scalars).
fn arb_name() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "[a-z][a-z0-9_]{0,19}".boxed(),
        1 => "[A-Za-zéàçΩ東京☕][A-Za-zéàçΩ東京☕ _.-]{0,10}".boxed(),
        1 => "\"[a-zA-Z ,.()]{1,10}\"".boxed(),
        1 => prop::sample::select(vec![
            "SELECT".to_string(),
            "primary key".to_string(),
            "wéird name".to_string(),
        ]).boxed(),
        // YAML-hostile scalars: bare forms a naive serializer would emit
        // unquoted and re-read as null / bool / number / mapping / comment
        // rather than as the original string. A correct serializer must quote
        // them so they round-trip as plain string scalars; if any of these
        // breaks the round-trip it is a real serializer bug, not a test bug.
        2 => prop::sample::select(vec![
            "null", "~", "no", "on", "yes", "true", "false", "123", "1.5",
            "-0", "a: b", "x #y", " padded ", "line\nbreak",
        ]).prop_map(str::to_string).boxed(),
        1 => Just("has \"embedded\" quote".to_string()).boxed(),
    ]
}

/// Free-text payload for COMMENT / SYNONYMS fields. Reuses `arb_name`'s
/// alphabet (including the YAML-hostile scalars) so those optional fields
/// actually exercise the round-trip instead of being hardcoded empty.
fn arb_payload() -> impl Strategy<Value = String> {
    arb_name()
}

/// Generate an arbitrary SQL-like expression.
fn arb_expr() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_name(),
        arb_name().prop_map(|n| format!("SUM({n})")),
        arb_name().prop_map(|n| format!("COUNT({n})")),
        arb_name().prop_map(|n| format!("AVG({n})")),
        arb_name().prop_map(|n| format!("concat({n}, ' – ☕')")),
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
        proptest::collection::vec(proptest::collection::vec(arb_name(), 1..=2), 0..=2),
        proptest::option::of(arb_payload()),
        proptest::collection::vec(arb_payload(), 0..=2),
    )
        .prop_map(
            |(alias, table, pk_columns, unique_constraints, comment, synonyms)| TableRef {
                alias,
                table,
                pk_columns,
                unique_constraints,
                comment,
                synonyms,
            },
        )
}

fn arb_dimension() -> impl Strategy<Value = Dimension> {
    (
        arb_name(),
        arb_expr(),
        proptest::option::of(arb_name()),
        proptest::option::of(arb_payload()),
        proptest::collection::vec(arb_payload(), 0..=2),
    )
        .prop_map(|(name, expr, source_table, comment, synonyms)| Dimension {
            name,
            expr,
            source_table,
            output_type: None,
            comment,
            synonyms,
        })
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
        proptest::collection::vec(arb_name(), 0..=2),
        proptest::collection::vec(arb_name(), 0..=2),
        proptest::option::of(arb_payload()),
    )
        .prop_map(
            |(
                window_function,
                inner_metric,
                excluding_dims,
                order_by,
                extra_args,
                partition_dims,
                frame_clause,
            )| WindowSpec {
                window_function,
                inner_metric,
                extra_args,
                excluding_dims,
                partition_dims,
                order_by,
                frame_clause,
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
        proptest::option::of(arb_payload()),
        proptest::collection::vec(arb_payload(), 0..=2),
    )
        .prop_map(
            |(
                name,
                expr,
                source_table,
                access,
                non_additive_by,
                window_spec,
                comment,
                synonyms,
            )| {
                Metric {
                    name,
                    expr,
                    source_table,
                    output_type: None,
                    using_relationships: vec![],
                    comment,
                    synonyms,
                    access,
                    non_additive_by,
                    window_spec,
                }
            },
        )
}

fn arb_fact() -> impl Strategy<Value = Fact> {
    (
        arb_name(),
        arb_expr(),
        proptest::option::of(arb_name()),
        arb_access(),
        proptest::option::of(arb_payload()),
        proptest::collection::vec(arb_payload(), 0..=2),
    )
        .prop_map(
            |(name, expr, source_table, access, comment, synonyms)| Fact {
                name,
                expr,
                source_table,
                output_type: None,
                comment,
                synonyms,
                access,
            },
        )
}

fn arb_join() -> impl Strategy<Value = Join> {
    (
        arb_name(),
        arb_name(),
        proptest::collection::vec(arb_name(), 0..=2),
        arb_cardinality(),
        proptest::collection::vec(arb_name(), 0..=2),
        proptest::option::of(arb_name()),
    )
        .prop_map(
            |(table, from_alias, fk_columns, cardinality, ref_columns, name)| Join {
                table,
                from_alias,
                fk_columns,
                ref_columns,
                name,
                cardinality,
            },
        )
}

fn arb_materialization() -> impl Strategy<Value = Materialization> {
    (
        arb_name(),
        arb_name(),
        proptest::collection::vec(arb_name(), 0..=3),
        proptest::collection::vec(arb_name(), 0..=3),
    )
        .prop_map(|(name, table, dimensions, metrics)| Materialization {
            name,
            table,
            dimensions,
            metrics,
        })
}

fn arb_definition() -> impl Strategy<Value = SemanticViewDefinition> {
    (
        proptest::collection::vec(arb_table_ref(), 0..=2),
        proptest::collection::vec(arb_dimension(), 1..=3),
        proptest::collection::vec(arb_metric(), 1..=3),
        proptest::collection::vec(arb_join(), 0..=2),
        proptest::collection::vec(arb_fact(), 0..=2),
        proptest::option::of("[a-z ]{1,30}"),
        proptest::collection::vec(arb_materialization(), 0..=2),
    )
        .prop_map(
            |(tables, dimensions, metrics, joins, facts, comment, materializations)| {
                SemanticViewDefinition {
                    tables,
                    dimensions,
                    metrics,
                    joins,
                    facts,
                    materializations,
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

    #[test]
    fn materialization_json_roundtrip(mat in arb_materialization()) {
        let json_str = serde_json::to_string(&mat).expect("JSON serialize");
        let roundtripped: Materialization = serde_json::from_str(&json_str).expect("JSON deserialize");
        prop_assert_eq!(mat, roundtripped);
    }

    #[test]
    fn yaml_export_roundtrip(def in arb_definition()) {
        let yaml_str = semantic_views::render_yaml::render_yaml_export(&def)
            .expect("YAML export should succeed");
        let reimported = SemanticViewDefinition::from_yaml("proptest", &yaml_str)
            .expect("Re-import should succeed");

        // Strip internal fields from original for comparison
        let mut expected = def.clone();
        expected.created_on = None;
        expected.database_name = None;
        expected.schema_name = None;

        prop_assert_eq!(expected, reimported);
    }
}
