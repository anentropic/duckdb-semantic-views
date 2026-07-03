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

/// TC-9 oracle (code-review 2026-07-02): quote and bracket balance. Walks
/// the text with the same escape rules the SQL lexer uses (`''` inside
/// strings, `""` inside quoted identifiers); parens are counted only in
/// live code. Unbalanced output for balanced input would be a generator bug
/// that the old "non-empty + starts with WITH" oracle waved through.
fn is_balanced(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut in_ident = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
        } else if in_ident {
            if b == b'"' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                in_ident = false;
            }
        } else {
            match b {
                b'\'' => in_string = true,
                b'"' => in_ident = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    !in_string && !in_ident && depth == 0
}

/// The balance oracle only holds when the *input* definition's verbatim
/// SQL fragments are themselves balanced — expansion copies expressions
/// through, and define-time validation (not exercised here) is what keeps
/// unbalanced fragments out of the real catalog.
fn def_fragments_balanced(def: &SemanticViewDefinition) -> bool {
    let mut frags: Vec<&str> = Vec::new();
    for d in &def.dimensions {
        frags.push(&d.expr);
    }
    for f in &def.facts {
        frags.push(&f.expr);
    }
    for m in &def.metrics {
        frags.push(&m.expr);
        for na in &m.non_additive_by {
            frags.push(&na.dimension);
        }
        if let Some(ws) = &m.window_spec {
            frags.push(&ws.inner_metric);
            frags.extend(ws.extra_args.iter().map(String::as_str));
            frags.extend(ws.excluding_dims.iter().map(String::as_str));
            frags.extend(ws.partition_dims.iter().map(String::as_str));
            for ob in &ws.order_by {
                frags.push(&ob.expr);
            }
            if let Some(fc) = &ws.frame_clause {
                frags.push(fc);
            }
        }
    }
    frags.into_iter().all(is_balanced)
}

fuzz_target!(|input: FuzzInput| {
    let req = QueryRequest {
        dimensions: input.dim_names.into_iter().map(Into::into).collect(),
        metrics: input.metric_names.into_iter().map(Into::into).collect(),
        facts: vec![],
    };
    let fragments_ok = def_fragments_balanced(&input.def);
    if let Ok(sql) = expand("fuzz_view", &input.def, &req) {
        // Successful expansion must produce non-empty SQL
        assert!(!sql.is_empty());
        // Basic validity: starts with expected CTE prefix
        assert!(sql.starts_with("WITH"));
        // Structural validity (TC-9): balanced quotes and parens whenever
        // the input fragments were balanced.
        if fragments_ok {
            assert!(
                is_balanced(&sql),
                "unbalanced quotes/parens in generated SQL: {sql}"
            );
        }
    }
    // Errors are fine -- expand() returning Err is expected for invalid combos
});
