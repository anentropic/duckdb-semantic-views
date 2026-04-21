#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::{Dimension, Metric, SemanticViewDefinition, TableRef};

#[derive(Debug, Arbitrary)]
struct NameFuzzInput {
    dim_names: Vec<String>,
    metric_names: Vec<String>,
}

fuzz_target!(|input: NameFuzzInput| {
    let def = fixed_definition();
    let req = QueryRequest {
        dimensions: input.dim_names.into_iter().map(Into::into).collect(),
        metrics: input.metric_names.into_iter().map(Into::into).collect(),
        facts: vec![],
    };
    if let Ok(sql) = expand("fuzz_view", &def, &req) {
        assert!(!sql.is_empty());
        assert!(sql.starts_with("WITH"));
    }
});

fn fixed_definition() -> SemanticViewDefinition {
    SemanticViewDefinition {
        tables: vec![TableRef {
            alias: "orders".to_string(),
            table: "orders".to_string(),
            ..Default::default()
        }],
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                ..Default::default()
            },
            Dimension {
                name: "month".to_string(),
                expr: "date_trunc('month', created_at)".to_string(),
                ..Default::default()
            },
        ],
        metrics: vec![
            Metric {
                name: "revenue".to_string(),
                expr: "sum(amount)".to_string(),
                ..Default::default()
            },
            Metric {
                name: "count".to_string(),
                expr: "count(*)".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
