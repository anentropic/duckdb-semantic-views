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
        // Structural oracle: the emitted SQL must have balanced parentheses
        // once single-quoted string literals are ignored, and every literal
        // must be closed. The fixed definition's identifiers are known-good
        // (no parens/quotes), so any imbalance is a real code-gen defect —
        // strictly stronger than the previous `starts_with("WITH")` check.
        assert!(
            parens_balanced_outside_quotes(&sql),
            "expand produced structurally-invalid SQL: {sql}"
        );
    }
});

/// True iff parentheses are balanced when single-quoted string literals are
/// skipped, the nesting depth never goes negative, and no literal is left open.
/// A doubled `''` inside a literal is an escaped quote (stays inside the
/// literal). `(`, `)`, and `'` are ASCII, so byte scanning is UTF-8-safe.
fn parens_balanced_outside_quotes(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if in_str => {
                if bytes.get(i + 1) == Some(&b'\'') {
                    // Escaped quote — consume both, remain inside the literal.
                    i += 2;
                    continue;
                }
                in_str = false;
            }
            b'\'' => in_str = true,
            b'(' if !in_str => depth += 1,
            b')' if !in_str => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    depth == 0 && !in_str
}

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
