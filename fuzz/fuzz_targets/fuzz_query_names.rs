#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::{Dimension, Metric, SemanticViewDefinition};

#[derive(Debug, Arbitrary)]
struct NameFuzzInput {
    dim_names: Vec<String>,
    metric_names: Vec<String>,
}

fuzz_target!(|input: NameFuzzInput| {
    let def = fixed_definition();
    let req = QueryRequest {
        dimensions: input.dim_names,
        metrics: input.metric_names,
    };
    if let Ok(sql) = expand("fuzz_view", &def, &req) {
        assert!(!sql.is_empty());
        assert!(sql.starts_with("WITH"));
    }
});

fn fixed_definition() -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        dimensions: vec![
            Dimension {
                name: "region".to_string(),
                expr: "region".to_string(),
                source_table: None,
            },
            Dimension {
                name: "month".to_string(),
                expr: "date_trunc('month', created_at)".to_string(),
                source_table: None,
            },
        ],
        metrics: vec![
            Metric {
                name: "revenue".to_string(),
                expr: "sum(amount)".to_string(),
                source_table: None,
            },
            Metric {
                name: "count".to_string(),
                expr: "count(*)".to_string(),
                source_table: None,
            },
        ],
        filters: vec!["status = 'active'".to_string()],
        joins: vec![],
    }
}
