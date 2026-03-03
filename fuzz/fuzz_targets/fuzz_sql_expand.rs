#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::SemanticViewDefinition;

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    def: SemanticViewDefinition,
    dim_names: Vec<String>,
    metric_names: Vec<String>,
}

fuzz_target!(|input: FuzzInput| {
    let req = QueryRequest {
        dimensions: input.dim_names,
        metrics: input.metric_names,
    };
    if let Ok(sql) = expand("fuzz_view", &input.def, &req) {
        // Successful expansion must produce non-empty SQL
        assert!(!sql.is_empty());
        // Basic validity: starts with expected CTE prefix
        assert!(sql.starts_with("WITH"));
    }
    // Errors are fine -- expand() returning Err is expected for invalid combos
});
