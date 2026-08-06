//! Differential proptest for metrics on a **child** of the base table —
//! the NULL-extension direction (EXP-21, code-review 2026-08-06).
//!
//! Both existing join harnesses ([`star_schema_proptest`],
//! [`multi_hop_join_proptest`]) root their tree at the "many"-most table, so
//! every generated join reaches from the base table UPWARDS to a parent. That
//! leaves one whole direction unrandomized: a base table LEFT JOINed DOWN to a
//! child, where a base row with no matching child rows survives the join as a
//! single **NULL-extended** row.
//!
//! That phantom row is what SG-8 exists for. `COUNT(*)` on the child is
//! rewritten to `COUNT(<child pk>)` so it does not count it — but the rewrite
//! matched only the literal `*`, so `COUNT(1)` sailed past every guard and
//! counted the phantom (EXP-21: `COUNT(1)` returned 2 next to `COUNT(*)`'s 1,
//! in one result row, with a single childless order). Any aggregate over a
//! CONSTANT has the same blind spot — the argument is never NULL, so nothing
//! distinguishes the phantom row from a real one.
//!
//! Shape: `o` (base, one row per id) with child `li` (`li.order_id REFERENCES
//! o.id`, `ManyToOne` from `li`). Foreign keys include NULL and dangling ids,
//! and the generator is deliberately biased so that **childless orders are
//! common** — they are the only rows that expose the defect.
//!
//! Scope: dimensions are drawn from the BASE table only. Grouping by a child
//! dimension puts childless parents in their own NULL group, whose oracle is a
//! different (and much less independent) formulation; that case is covered by
//! the fixed tests in `src/expand/tests_count_star_rewrite.rs`. What this
//! harness randomizes is the axis that had none: constant-argument aggregates
//! against real data with real childless parents.
//!
//! The oracle is formulated WITHOUT a join at all — each metric is a correlated
//! subquery over `li` — so it shares no structure with the expansion's
//! `FROM o LEFT JOIN li` + `CASE WHEN li.id IS NOT NULL` rewrite. A rewrite
//! that guards the wrong column, guards nothing, or guards too much shows up as
//! a non-zero multiset difference.

use proptest::prelude::*;
use semantic_views::expand::{expand, DimensionName, MetricName, QueryRequest};
use semantic_views::model::{
    AccessModifier, Cardinality, Dimension, Join, Metric, SemanticViewDefinition, TableRef,
};

/// Base rows `o` have ids `0..n_o`; child rows `li` carry a foreign key into
/// them. `None` is a SQL NULL throughout.
#[derive(Debug, Clone)]
struct Instance {
    /// Base rows: `region` for ids `0..n_o`.
    o_rows: Vec<Option<i64>>,
    /// Child rows: `(order_id, amount)`.
    li_rows: Vec<(Option<i64>, Option<i64>)>,
}

/// Queryable objects, by stable name.
const DIMS: [&str; 1] = ["region"];
/// `n_star` is the spelling SG-8 always handled; `n_one`, `s_one`, `n_str` and
/// `n_paren` are the constant-argument spellings it did not — `n_paren` is the
/// redundantly-parenthesized literal raised by review on #203, which the first
/// cut of the constant check still let through. `s_amt` is an ordinary column
/// aggregate — the control that must stay untouched by the rewrite.
const METS: [&str; 6] = ["n_star", "n_one", "s_one", "n_str", "n_paren", "s_amt"];

#[derive(Debug, Clone)]
struct Case {
    inst: Instance,
    sel_dims: Vec<usize>,
    sel_metrics: Vec<usize>,
}

fn arb_instance() -> impl Strategy<Value = Instance> {
    // 1..=4 base rows, 0..=6 child rows. Foreign keys are drawn from a range
    // WIDER than the base ids, so dangling references and childless orders
    // both occur often; `None` covers the NULL foreign key.
    (
        prop::collection::vec(prop::option::of(0i64..3), 1..5),
        prop::collection::vec(
            (prop::option::of(0i64..6), prop::option::of(-3i64..8)),
            0..7,
        ),
    )
        .prop_map(|(o_rows, li_rows)| Instance { o_rows, li_rows })
}

fn arb_case() -> impl Strategy<Value = Case> {
    (
        arb_instance(),
        prop::collection::vec(any::<bool>(), DIMS.len()),
        prop::collection::vec(any::<bool>(), METS.len()),
    )
        .prop_map(|(inst, dim_mask, met_mask)| {
            let sel_dims: Vec<usize> = dim_mask
                .iter()
                .enumerate()
                .filter_map(|(i, &b)| b.then_some(i))
                .collect();
            let mut sel_metrics: Vec<usize> = met_mask
                .iter()
                .enumerate()
                .filter_map(|(i, &b)| b.then_some(i))
                .collect();
            // A request with no metrics is a different code path (dims-only
            // DISTINCT); this harness is about the aggregates.
            if sel_metrics.is_empty() {
                sel_metrics.push(1); // n_one — the metric EXP-21 was about.
            }
            Case {
                inst,
                sel_dims,
                sel_metrics,
            }
        })
}

fn build_def() -> SemanticViewDefinition {
    let tables = vec![
        // The BASE table is the parent here — the direction neither other
        // join harness generates.
        TableRef {
            alias: "o".to_string(),
            table: "o".to_string(),
            pk_columns: vec!["id".to_string()],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        },
        TableRef {
            alias: "li".to_string(),
            table: "li".to_string(),
            pk_columns: vec!["id".to_string()],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        },
    ];
    let dimensions = vec![Dimension {
        name: "region".to_string(),
        expr: "o.region".to_string(),
        source_table: Some("o".to_string()),
        output_type: None,
        comment: None,
        synonyms: vec![],
        is_filter: false,
    }];
    let base_metric = |name: &str, expr: &str, source: Option<&str>| Metric {
        name: name.to_string(),
        expr: expr.to_string(),
        source_table: source.map(str::to_string),
        output_type: None,
        using_relationships: vec![],
        comment: None,
        synonyms: vec![],
        access: AccessModifier::Public,
        non_additive_by: vec![],
        window_spec: None,
    };
    let metrics = vec![
        base_metric("n_star", "COUNT(*)", Some("li")),
        base_metric("n_one", "COUNT(1)", Some("li")),
        base_metric("s_one", "SUM(1)", Some("li")),
        base_metric("n_str", "COUNT('x')", Some("li")),
        base_metric("n_paren", "COUNT((1))", Some("li")),
        base_metric("s_amt", "SUM(li.amount)", Some("li")),
    ];
    let joins = vec![Join {
        from_alias: "li".to_string(),
        table: "o".to_string(),
        fk_columns: vec!["order_id".to_string()],
        ref_columns: vec!["id".to_string()],
        name: Some("li_o".to_string()),
        cardinality: Cardinality::ManyToOne,
    }];
    SemanticViewDefinition {
        tables,
        dimensions,
        metrics,
        joins,
        facts: vec![],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        resolution_schema_name: None,
        comment: None,
    }
}

fn make_db(inst: &Instance) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    conn.execute_batch(
        "CREATE TABLE o (id INTEGER, region INTEGER); \
         CREATE TABLE li (id INTEGER, order_id INTEGER, amount INTEGER);",
    )
    .expect("create tables");

    let cell = |c: &Option<i64>| c.map_or_else(|| "NULL".to_string(), |v| v.to_string());

    if !inst.o_rows.is_empty() {
        let values: Vec<String> = inst
            .o_rows
            .iter()
            .enumerate()
            .map(|(i, region)| format!("({i},{})", cell(region)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO o VALUES {};", values.join(",")))
            .expect("insert o rows");
    }
    if !inst.li_rows.is_empty() {
        let values: Vec<String> = inst
            .li_rows
            .iter()
            .enumerate()
            .map(|(i, (order_id, amount))| format!("({i},{},{})", cell(order_id), cell(amount)))
            .collect();
        conn.execute_batch(&format!("INSERT INTO li VALUES {};", values.join(",")))
            .expect("insert li rows");
    }
    conn
}

/// The oracle: every metric as a correlated subquery over `li`, so the
/// formulation shares nothing with the expansion's LEFT JOIN + PK guard.
///
/// `count(*)` in the subquery returns 0 for a childless order and `sum(...)`
/// returns NULL, which is exactly the empty-group semantics each aggregate
/// should have had over its own rows — the property the phantom NULL-extended
/// row destroys.
fn oracle_sql(case: &Case) -> String {
    let per_order = |i: usize| -> String {
        match METS[i] {
            "n_star" | "n_one" | "n_str" | "n_paren" => {
                "(SELECT count(*) FROM li WHERE li.order_id = o.id)".to_string()
            }
            "s_one" => "(SELECT sum(1) FROM li WHERE li.order_id = o.id)".to_string(),
            "s_amt" => "(SELECT sum(li.amount) FROM li WHERE li.order_id = o.id)".to_string(),
            other => unreachable!("unexpected metric {other}"),
        }
    };
    let met_items: Vec<String> = case
        .sel_metrics
        .iter()
        .map(|&i| format!("sum({}) AS {}", per_order(i), METS[i]))
        .collect();
    let dim_items: Vec<String> = case
        .sel_dims
        .iter()
        .map(|&i| format!("o.{} AS {}", DIMS[i], DIMS[i]))
        .collect();
    let select_items = dim_items
        .iter()
        .chain(met_items.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if case.sel_dims.is_empty() {
        format!("SELECT {select_items} FROM o")
    } else {
        let group: Vec<String> = (1..=case.sel_dims.len()).map(|n| n.to_string()).collect();
        format!("SELECT {select_items} FROM o GROUP BY {}", group.join(", "))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn child_grain_constant_aggregates_match_the_oracle(case in arb_case()) {
        let def = build_def();
        let req = QueryRequest {
            where_clause: None,
            dimensions: case
                .sel_dims
                .iter()
                .map(|&i| DimensionName::new(DIMS[i]))
                .collect(),
            metrics: case
                .sel_metrics
                .iter()
                .map(|&i| MetricName::new(METS[i]))
                .collect(),
            facts: vec![],
        };

        let expanded = match expand("child", &def, &req) {
            Ok(sql) => sql,
            Err(e) => {
                prop_assert!(false, "child-grain aggregate query unexpectedly rejected: {e}");
                unreachable!()
            }
        };
        let oracle = oracle_sql(&case);

        // Canonical projection (columns sorted by name) so a column-order
        // difference between the two formulations is not a false diff.
        let mut proj_cols: Vec<String> = case
            .sel_dims
            .iter()
            .map(|&i| DIMS[i].to_string())
            .chain(case.sel_metrics.iter().map(|&i| METS[i].to_string()))
            .collect();
        proj_cols.sort();
        let proj = proj_cols.join(", ");

        let cmp = format!(
            "SELECT \
               (SELECT count(*) FROM (SELECT {proj} FROM ({expanded}) qa \
                                      EXCEPT ALL \
                                      SELECT {proj} FROM ({oracle}) qb) e1) \
             + (SELECT count(*) FROM (SELECT {proj} FROM ({oracle}) qc \
                                      EXCEPT ALL \
                                      SELECT {proj} FROM ({expanded}) qd) e2) AS diff"
        );

        let conn = make_db(&case.inst);
        let diff: i64 = conn
            .query_row(&cmp, [], |r| r.get(0))
            .unwrap_or_else(|e| panic!("comparison failed: {e}\nEXPANDED:\n{expanded}\nORACLE:\n{oracle}"));
        prop_assert_eq!(
            diff, 0,
            "expansion disagrees with the oracle\nEXPANDED:\n{}\nORACLE:\n{}",
            expanded, oracle
        );
    }
}

/// Anti-vacuity guard, in the spirit of the PBT-6 harnesses: the generator must
/// actually produce the shape the property is about — at least one childless
/// base row, which is the only row that can expose EXP-21. Without a childless
/// order every metric spelling agrees and the property proves nothing.
#[test]
fn generator_produces_childless_base_rows() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let mut saw_childless = false;
    for _ in 0..256 {
        let case = arb_case().new_tree(&mut runner).unwrap().current();
        let n_o = case.inst.o_rows.len() as i64;
        let childless = (0..n_o).any(|id| {
            !case
                .inst
                .li_rows
                .iter()
                .any(|(order_id, _)| *order_id == Some(id))
        });
        if childless {
            saw_childless = true;
            break;
        }
    }
    assert!(
        saw_childless,
        "the generator never produced a childless base row, so the property is vacuous"
    );
}

/// Companion guard: the constant-argument metrics must be reachable in a
/// request, not merely declared. A mask that never selects them would make the
/// numeric property green without ever testing the EXP-21 rewrite.
#[test]
fn generator_selects_constant_argument_metrics() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let mut seen = [false; METS.len()];
    for _ in 0..256 {
        let case = arb_case().new_tree(&mut runner).unwrap().current();
        for &i in &case.sel_metrics {
            seen[i] = true;
        }
    }
    for (i, was_seen) in seen.iter().enumerate() {
        assert!(
            *was_seen,
            "metric {} is never selected by the generator",
            METS[i]
        );
    }
}
