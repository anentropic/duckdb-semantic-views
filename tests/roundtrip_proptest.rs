//! RT-3 (code-review 2026-07-02): the parse ↔ render round-trip property.
//!
//! For every definition in the parser's CANONICAL form,
//!
//! ```text
//! parse_keyword_body(body_of(render_create_ddl(def))) == def
//! ```
//!
//! "Canonical form" means the definition's fields hold exactly what the body
//! parser stores: quoted identifiers RETAIN their quotes (`"my col"`),
//! bare identifiers are bare, expressions are trimmed, and `ref_columns`
//! is either empty (omitted in DDL) or an explicit list that differs from
//! the target's PRIMARY KEY (a PK-equal list is a render-side omission and
//! is re-populated by define-time inference, not by the parser).
//!
//! The generators deliberately cover the hostile shapes the DDL grammar has
//! regressed on before (TECH-DEBT #24/#25, WR-04, PA-1/2/3/6): quoted
//! identifiers with whitespace / dots / commas / uppercase / non-ASCII /
//! escaped quotes, unicode comment and synonym payloads, annotation
//! keywords inside string literals, USING lists, and NON ADDITIVE BY
//! modifiers.
//!
//! Window metrics are exercised by the fixpoint fuzz target
//! (`fuzz_render_roundtrip`) instead: their expression text is *rebuilt*
//! from `WindowSpec` at render time, so byte-equality of `expr` needs the
//! canonical formatter, not the parser.

use proptest::prelude::*;
use semantic_views::body_parser::parse_keyword_body;
use semantic_views::model::{
    AccessModifier, Dimension, Fact, Join, Metric, NonAdditiveDim, NullsOrder,
    SemanticViewDefinition, SortOrder, TableRef,
};
use semantic_views::render_ddl::render_create_ddl;

// ---------------------------------------------------------------------------
// Identifier / payload strategies (TC-3: quoted + unicode + keyword arms)
// ---------------------------------------------------------------------------

/// Bare identifier — safe to emit unquoted.
fn arb_bare_ident() -> impl Strategy<Value = String> {
    "[a-z_][a-z0-9_]{0,7}"
}

/// Content for a quoted identifier: whitespace, dots, commas, parens,
/// uppercase, keywords, and non-ASCII are all fair game. Embedded double
/// quotes are exercised via the dedicated arm below (escaping is applied
/// when the stored form is built).
fn arb_quoted_content() -> impl Strategy<Value = String> {
    prop_oneof![
        // hostile ASCII: keywords, whitespace, punctuation
        "[A-Za-z ,.()'_-]{1,12}",
        // non-ASCII
        "[a-zéàΩ東京☕ ]{1,8}",
        // annotation keywords that must stay inert inside quotes
        Just("PRIMARY KEY (id)".to_string()),
        Just("comment".to_string()),
        Just("AS".to_string()),
        // embedded double quote
        Just(r#"we"ird"#.to_string()),
    ]
    .prop_filter("quoted content must not be all-whitespace", |s| {
        !s.trim().is_empty()
    })
}

/// An identifier in STORED form: either bare, or quoted with `""` escaping
/// retained (exactly what the body parser keeps).
fn arb_stored_ident() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => arb_bare_ident().boxed(),
        2 => arb_quoted_content()
            .prop_map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
            .boxed(),
    ]
}

/// Unicode-bearing text payload for COMMENT / SYNONYMS.
fn arb_payload() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Za-z0-9 ,.()=_-]{0,16}",
        "[a-zéàçΩ東京☕' ]{0,10}",
        Just("the PRIMARY KEY (id) lives here".to_string()),
        Just("has -- and /* inside */".to_string()),
    ]
}

/// A canonical expression: composed so that no depth-0 comma or annotation
/// keyword appears outside quotes, and all quotes/parens are balanced.
fn arb_expr(aliases: Vec<String>) -> impl Strategy<Value = String> {
    let alias = prop::sample::select(aliases);
    let atom = prop_oneof![
        (alias, arb_bare_ident()).prop_map(|(a, c)| format!("{a}.{c}")),
        arb_bare_ident(),
        arb_payload().prop_map(|p| format!("'{}'", p.replace('\'', "''"))),
        "[0-9]{1,4}",
    ];
    atom.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            (
                inner.clone(),
                inner.clone(),
                prop::sample::select(vec!["+", "-", "*", "||"])
            )
                .prop_map(|(a, b, op)| format!("{a} {op} {b}")),
            inner.clone().prop_map(|e| format!("({e})")),
            (
                prop::sample::select(vec!["SUM", "AVG", "MIN", "MAX", "COUNT"]),
                inner
            )
                .prop_map(|(f, e)| format!("{f}({e})")),
        ]
    })
}

// ---------------------------------------------------------------------------
// Definition generator (canonical form)
// ---------------------------------------------------------------------------

/// Raw material for one table entry.
type TableSpec = (
    String,
    Vec<String>,
    Vec<Vec<String>>,
    Option<String>,
    Vec<String>,
);
/// Raw material for one join: (has_join, fk_columns, explicit ref_columns?).
type JoinSpec = (bool, Vec<String>, Option<Vec<String>>);
/// Raw material for one dimension/fact entry.
type EntrySpec = (usize, String, String, bool, Option<String>, Vec<String>);
/// Raw material for one metric: entry + (wants USING, wants NAB(order,nulls)).
type MetricSpec = (EntrySpec, bool, Option<(SortOrder, NullsOrder)>);

/// All cross-references (USING → declared relationship, NON ADDITIVE BY →
/// declared dimension) are resolved during assembly, so every generated
/// definition passes the parser's define-time reference checks by
/// construction.
#[allow(clippy::too_many_lines)]
fn arb_canonical_def() -> impl Strategy<Value = SemanticViewDefinition> {
    let table_spec = (
        arb_stored_ident(),
        prop::collection::vec(arb_stored_ident(), 0..=2),
        prop::collection::vec(prop::collection::vec(arb_stored_ident(), 1..=2), 0..=1),
        prop::option::of(arb_payload()),
        prop::collection::vec(arb_payload(), 0..=2),
    );
    let join_spec = (
        any::<bool>(),
        prop::collection::vec(arb_bare_ident(), 1..=2),
        prop::option::of(prop::collection::vec(arb_bare_ident(), 1..=2)),
    );
    let entry_spec = |aliases_max: usize| {
        (
            0..aliases_max,
            arb_stored_ident(),
            arb_expr(vec!["t0".to_string(), "t1".to_string(), "t2".to_string()]),
            any::<bool>(),
            prop::option::of(arb_payload()),
            prop::collection::vec(arb_payload(), 0..=2),
        )
    };
    let metric_spec = (
        entry_spec(3),
        any::<bool>(),
        prop::option::of((
            prop::sample::select(vec![SortOrder::Asc, SortOrder::Desc]),
            prop::sample::select(vec![NullsOrder::First, NullsOrder::Last]),
        )),
    );

    (
        prop::collection::vec(table_spec, 1..=3),
        prop::collection::vec(join_spec, 0..=2),
        prop::collection::vec(entry_spec(3), 0..=2), // dims
        prop::collection::vec(entry_spec(3), 0..=2), // facts
        prop::collection::vec(metric_spec, 0..=2),
    )
        .prop_map(
            |(table_specs, join_specs, dim_specs, fact_specs, metric_specs)| {
                let tables: Vec<TableRef> = table_specs
                    .into_iter()
                    .enumerate()
                    .map(
                        |(i, (table, pk_columns, unique_constraints, comment, synonyms)): (
                            usize,
                            TableSpec,
                        )| TableRef {
                            alias: format!("t{i}"),
                            table,
                            pk_columns,
                            unique_constraints,
                            comment,
                            synonyms,
                            ..Default::default()
                        },
                    )
                    .collect();
                let n_tables = tables.len();
                let base_pk = tables[0].pk_columns.clone();

                // One optional named relationship per non-base table.
                let joins: Vec<Join> = join_specs
                    .into_iter()
                    .take(n_tables.saturating_sub(1))
                    .enumerate()
                    .filter_map(|(i, (has, fk_columns, ref_columns)): (usize, JoinSpec)| {
                        if !has {
                            return None;
                        }
                        // Explicit ref_columns must differ from the target PK —
                        // a PK-equal list is omitted at render time (RT-1's
                        // compact form) and re-populated by define-time
                        // inference, not the parser.
                        let mut ref_columns = ref_columns.unwrap_or_default();
                        if !ref_columns.is_empty() && ref_columns == base_pk {
                            ref_columns.push("extra_ref".to_string());
                        }
                        Some(Join {
                            name: Some(format!("rel{i}")),
                            from_alias: format!("t{}", i + 1),
                            table: "t0".to_string(),
                            fk_columns,
                            ref_columns,
                            ..Default::default()
                        })
                    })
                    .collect();

                let dimensions: Vec<Dimension> = dim_specs
                    .into_iter()
                    .enumerate()
                    .map(
                        |(i, (alias_idx, name, expr, _private, comment, synonyms)): (
                            usize,
                            EntrySpec,
                        )| {
                            Dimension {
                                name: distinct_name(&name, i),
                                expr,
                                source_table: Some(format!("t{}", alias_idx % n_tables)),
                                comment,
                                synonyms,
                                ..Default::default()
                            }
                        },
                    )
                    .collect();

                let facts: Vec<Fact> = fact_specs
                    .into_iter()
                    .enumerate()
                    .map(
                        |(i, (alias_idx, name, expr, private, comment, synonyms)): (
                            usize,
                            EntrySpec,
                        )| Fact {
                            name: distinct_name(&name, i),
                            expr,
                            source_table: Some(format!("t{}", alias_idx % n_tables)),
                            access: if private {
                                AccessModifier::Private
                            } else {
                                AccessModifier::Public
                            },
                            comment,
                            synonyms,
                            ..Default::default()
                        },
                    )
                    .collect();

                let metrics: Vec<Metric> = metric_specs
                    .into_iter()
                    .enumerate()
                    .map(|(i, (entry, wants_using, nab)): (usize, MetricSpec)| {
                        let (alias_idx, name, expr, private, comment, synonyms) = entry;
                        Metric {
                            name: distinct_name(&name, i),
                            expr,
                            source_table: Some(format!("t{}", alias_idx % n_tables)),
                            // USING resolves to a relationship that actually
                            // exists, or is dropped.
                            using_relationships: if wants_using && !joins.is_empty() {
                                vec![joins[i % joins.len()].name.clone().unwrap()]
                            } else {
                                vec![]
                            },
                            // NON ADDITIVE BY references a declared dimension,
                            // or is dropped.
                            non_additive_by: match (nab, dimensions.first()) {
                                (Some((order, nulls)), Some(dim)) => vec![NonAdditiveDim {
                                    dimension: dim.name.clone(),
                                    order,
                                    nulls,
                                }],
                                _ => vec![],
                            },
                            access: if private {
                                AccessModifier::Private
                            } else {
                                AccessModifier::Public
                            },
                            comment,
                            synonyms,
                            ..Default::default()
                        }
                    })
                    .collect();

                SemanticViewDefinition {
                    tables,
                    joins,
                    dimensions,
                    facts,
                    metrics,
                    ..Default::default()
                }
            },
        )
}

/// Make a stored-form identifier distinct by index while preserving its
/// quoting shape (suffix goes INSIDE the closing quote for quoted names).
fn distinct_name(name: &str, i: usize) -> String {
    if let Some(stripped) = name.strip_suffix('"') {
        format!("{stripped}_{i}\"")
    } else {
        format!("{name}_{i}")
    }
}

// ---------------------------------------------------------------------------
// The property
// ---------------------------------------------------------------------------

/// Strip the fixed rendered header for the bare-safe test view name and
/// return the `AS ...` body.
fn body_of(ddl: &str) -> &str {
    ddl.strip_prefix("CREATE OR REPLACE SEMANTIC VIEW rt_view ")
        .expect("rendered header shape")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// parse(render(def)) == def for canonical definitions.
    #[test]
    fn parse_render_roundtrip_is_identity(def in arb_canonical_def()) {
        // At least one of DIMENSIONS/METRICS is required by the grammar.
        prop_assume!(!def.dimensions.is_empty() || !def.metrics.is_empty());

        let rendered = render_create_ddl("rt_view", &def)
            .expect("canonical definitions must render");
        let body = body_of(&rendered);
        let reparsed = parse_keyword_body(body, 0).unwrap_or_else(|e| {
            panic!(
                "rendered DDL failed to re-parse: {}\n--- rendered ---\n{rendered}",
                e.message
            )
        });
        prop_assert_eq!(&reparsed.tables, &def.tables, "tables drift\n{}", rendered);
        prop_assert_eq!(
            &reparsed.relationships,
            &def.joins,
            "relationships drift\n{}",
            rendered
        );
        prop_assert_eq!(&reparsed.facts, &def.facts, "facts drift\n{}", rendered);
        prop_assert_eq!(
            &reparsed.dimensions,
            &def.dimensions,
            "dimensions drift\n{}",
            rendered
        );
        prop_assert_eq!(&reparsed.metrics, &def.metrics, "metrics drift\n{}", rendered);
        prop_assert_eq!(
            &reparsed.materializations,
            &def.materializations,
            "materializations drift\n{}",
            rendered
        );
    }
}
