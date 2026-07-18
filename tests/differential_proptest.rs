//! T-9 (code-review 2026-07-11): Rust-side randomized-schema differential
//! proptest for the core base-table aggregation path.
//!
//! For each case we generate a random single-table star-schema *shape* — a
//! random number of group-by dimension columns and a random set of aggregate
//! metrics over random value columns — fill it with random integer rows, then
//! for a random non-empty subset of dims + metrics compare the semantic-view
//! expansion (`expand`) against an independently hand-written `GROUP BY` query
//! over the same physical table.
//!
//! The two result sets are multiset-compared *inside `DuckDB`* via a symmetric
//! `EXCEPT ALL` difference, which is type-agnostic (no Rust-side row decoding)
//! and order-independent (both sides are projected in a canonical column order
//! keyed by output alias, so a column-ordering difference between the two
//! formulations is not a false diff).
//!
//! Scope: base-table metrics, single grain, integer data + exact aggregates
//! (`SUM`/`COUNT`/`MIN`/`MAX` — no floating point, so equality is exact).
//! Joins, semi-additive, window, wildcard, and USING paths are exercised by
//! the fixed-schema Python differential harness (`test/integration/
//! test_differential.py`, extended per T-1); this test adds schema + data +
//! query randomization for the core path and runs under plain `cargo test`.

use proptest::prelude::*;
use semantic_views::expand::{expand, DimensionName, MetricName, QueryRequest};
use semantic_views::model::{AccessModifier, Dimension, Metric, SemanticViewDefinition, TableRef};

/// An aggregate over a generated column (or `COUNT(*)`), rendered to SQL.
///
/// `CountCol` (`count(v{j})`) counts only non-NULL rows, so with NULL-bearing
/// value columns it diverges from `Count` (`count(*)`) — the two are now
/// differentially distinguished (previously only `count(*)` existed).
#[derive(Debug, Clone)]
enum Agg {
    Sum(usize),
    Count,
    CountCol(usize),
    Min(usize),
    Max(usize),
}

impl Agg {
    fn to_sql(&self) -> String {
        match self {
            Agg::Sum(j) => format!("sum(v{j})"),
            Agg::Count => "count(*)".to_string(),
            Agg::CountCol(j) => format!("count(v{j})"),
            Agg::Min(j) => format!("min(v{j})"),
            Agg::Max(j) => format!("max(v{j})"),
        }
    }
}

/// A generated schema shape plus its data. Columns are `d0..d{n_dims-1}`
/// (group-by dimensions) followed by `v0..v{n_vals-1}` (metric inputs), all
/// `INTEGER`. `rows` holds `n_dims + n_vals` cells per row; `None` is a SQL
/// `NULL` (exercises NULL group keys and NULL aggregate inputs).
#[derive(Debug, Clone)]
struct Schema {
    n_dims: usize,
    n_vals: usize,
    metric_aggs: Vec<Agg>,
    rows: Vec<Vec<Option<i64>>>,
}

/// A full test case: a schema plus the non-empty subset of dims and metrics to
/// query (indices into the schema's dimension list / `metric_aggs`).
#[derive(Debug, Clone)]
struct Case {
    schema: Schema,
    sel_dims: Vec<usize>,
    sel_metrics: Vec<usize>,
}

fn arb_schema() -> impl Strategy<Value = Schema> {
    (1usize..=3, 1usize..=3).prop_flat_map(|(n_dims, n_vals)| {
        let agg = prop_oneof![
            (0..n_vals).prop_map(Agg::Sum),
            Just(Agg::Count),
            (0..n_vals).prop_map(Agg::CountCol),
            (0..n_vals).prop_map(Agg::Min),
            (0..n_vals).prop_map(Agg::Max),
        ];
        let metrics = prop::collection::vec(agg, 1..=3);
        // Dimension cells: a small domain (0..3) so rows collide into real
        // groups, plus `None` (NULL) so NULL group keys are exercised.
        let dim_cell = prop_oneof![
            1 => Just(None),
            5 => (0i64..3).prop_map(Some),
        ];
        // Value cells: `None` (NULL, so SUM-over-all-NULL and COUNT(col) vs
        // COUNT(*) diverge), a small signed domain (exact sums / collisions),
        // and a large signed magnitude — both spanning negatives. The domain
        // stays within INT32 so it fits the `INTEGER` columns; SUM widens to
        // HUGEINT in DuckDB so no aggregate overflow arises.
        let val_cell = prop_oneof![
            1 => Just(None),
            2 => (-5i64..=5).prop_map(Some),
            2 => (-1_000_000_000i64..=1_000_000_000).prop_map(Some),
        ];
        let row = (
            prop::collection::vec(dim_cell, n_dims),
            prop::collection::vec(val_cell, n_vals),
        )
            .prop_map(|(mut cells, vals)| {
                cells.extend(vals);
                cells
            });
        // 0 rows is allowed — the empty-table path (global aggregate over no
        // rows, empty DISTINCT) was never differentially checked before.
        let rows = prop::collection::vec(row, 0..=25);
        (Just(n_dims), Just(n_vals), metrics, rows).prop_map(
            |(n_dims, n_vals, metric_aggs, rows)| Schema {
                n_dims,
                n_vals,
                metric_aggs,
                rows,
            },
        )
    })
}

fn arb_case() -> impl Strategy<Value = Case> {
    arb_schema().prop_flat_map(|schema| {
        let nd = schema.n_dims;
        let nm = schema.metric_aggs.len();
        // Either selection may be empty (dims-only → SELECT DISTINCT,
        // metrics-only → global aggregate), but not both — a fully-empty
        // request is invalid, so the at-least-one invariant is preserved by
        // the filter below.
        let dim_sel = prop::sample::subsequence((0..nd).collect::<Vec<_>>(), 0..=nd);
        let met_sel = prop::sample::subsequence((0..nm).collect::<Vec<_>>(), 0..=nm);
        (Just(schema), dim_sel, met_sel)
            .prop_filter(
                "at least one of dimensions/metrics must be selected",
                |(_, sel_dims, sel_metrics)| !sel_dims.is_empty() || !sel_metrics.is_empty(),
            )
            .prop_map(|(schema, sel_dims, sel_metrics)| Case {
                schema,
                sel_dims,
                sel_metrics,
            })
    })
}

/// Build the semantic-view definition for a generated schema: base table `t`,
/// one dimension per `d{i}` (expr == column), one metric per generated agg.
fn build_def(s: &Schema) -> SemanticViewDefinition {
    let dimensions = (0..s.n_dims)
        .map(|i| Dimension {
            name: format!("d{i}"),
            expr: format!("d{i}"),
            source_table: None,
            output_type: None,
            comment: None,
            synonyms: vec![],
        })
        .collect();
    let metrics = s
        .metric_aggs
        .iter()
        .enumerate()
        .map(|(i, agg)| Metric {
            name: format!("m{i}"),
            expr: agg.to_sql(),
            source_table: None,
            output_type: None,
            using_relationships: vec![],
            comment: None,
            synonyms: vec![],
            access: AccessModifier::Public,
            non_additive_by: vec![],
            window_spec: None,
        })
        .collect();
    SemanticViewDefinition {
        tables: vec![TableRef {
            alias: "t".to_string(),
            table: "t".to_string(),
            pk_columns: vec![],
            unique_constraints: vec![],
            comment: None,
            synonyms: vec![],
        }],
        dimensions,
        metrics,
        joins: vec![],
        facts: vec![],
        materializations: vec![],
        created_on: None,
        database_name: None,
        schema_name: None,
        comment: None,
    }
}

/// Create the physical base table `t` and insert the generated rows.
fn make_db(s: &Schema) -> duckdb::Connection {
    let conn = duckdb::Connection::open_in_memory().expect("in-memory DuckDB");
    let mut cols: Vec<String> = (0..s.n_dims).map(|i| format!("d{i} INTEGER")).collect();
    cols.extend((0..s.n_vals).map(|j| format!("v{j} INTEGER")));
    conn.execute_batch(&format!("CREATE TABLE t ({});", cols.join(", ")))
        .expect("create table t");
    let values: Vec<String> = s
        .rows
        .iter()
        .map(|r| {
            format!(
                "({})",
                r.iter()
                    .map(|c| c.map_or_else(|| "NULL".to_string(), |v| v.to_string()))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect();
    if !values.is_empty() {
        conn.execute_batch(&format!("INSERT INTO t VALUES {};", values.join(",")))
            .expect("insert rows");
    }
    conn
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// The semantic-view expansion of a random core query returns exactly the
    /// same rows (as a multiset) as an independently hand-written GROUP BY over
    /// the same physical table.
    #[test]
    fn expansion_matches_handwritten_group_by(case in arb_case()) {
        let def = build_def(&case.schema);

        let req = QueryRequest {
            dimensions: case
                .sel_dims
                .iter()
                .map(|i| DimensionName::new(format!("d{i}")))
                .collect(),
            metrics: case
                .sel_metrics
                .iter()
                .map(|i| MetricName::new(format!("m{i}")))
                .collect(),
            facts: vec![],
        };

        let expanded = expand("t_diff", &def, &req)
            .expect("expand must succeed for a core base-table definition");

        // Independent oracle: a plain GROUP BY over the same table, aliasing
        // each output column by the same name the expansion uses.
        let dim_items: Vec<String> = case.sel_dims.iter().map(|i| format!("d{i} AS d{i}")).collect();
        let met_items: Vec<String> = case
            .sel_metrics
            .iter()
            .map(|i| format!("{} AS m{i}", case.schema.metric_aggs[*i].to_sql()))
            .collect();
        let select_items = dim_items
            .iter()
            .chain(met_items.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        // Metrics-only requests are a single global-aggregate row (no GROUP
        // BY). Otherwise GROUP BY the selected dimensions by ordinal — they are
        // projected first, so positions 1..=sel_dims.len(). Dims-only is a
        // GROUP BY over all selected dims, multiset-equal to the expansion's
        // SELECT DISTINCT.
        let oracle = if case.sel_dims.is_empty() {
            format!("SELECT {select_items} FROM t")
        } else {
            let group_by = (1..=case.sel_dims.len())
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("SELECT {select_items} FROM t GROUP BY {group_by}")
        };

        // Canonical projection (columns sorted by output name) so a column
        // ORDER difference between the two formulations is not a false diff.
        let mut proj_cols: Vec<String> = case
            .sel_dims
            .iter()
            .map(|i| format!("d{i}"))
            .chain(case.sel_metrics.iter().map(|i| format!("m{i}")))
            .collect();
        proj_cols.sort();
        let proj = proj_cols.join(", ");

        // Symmetric multiset difference inside DuckDB: 0 iff the two result
        // sets are equal as multisets.
        let cmp = format!(
            "SELECT \
               (SELECT count(*) FROM (SELECT {proj} FROM ({expanded}) qa \
                                      EXCEPT ALL \
                                      SELECT {proj} FROM ({oracle}) qb) e1) \
             + (SELECT count(*) FROM (SELECT {proj} FROM ({oracle}) qc \
                                      EXCEPT ALL \
                                      SELECT {proj} FROM ({expanded}) qd) e2) AS diff"
        );

        let conn = make_db(&case.schema);
        let diff: i64 = conn.query_row(&cmp, [], |r| r.get(0)).unwrap_or_else(|e| {
            panic!("differential comparison query failed: {e}\n--- expanded:\n{expanded}\n--- oracle:\n{oracle}")
        });

        prop_assert_eq!(
            diff, 0,
            "semantic-view expansion disagrees with hand-written GROUP BY \
             (symmetric multiset diff = {})\n--- expanded:\n{}\n--- oracle:\n{}",
            diff, expanded, oracle
        );
    }
}
