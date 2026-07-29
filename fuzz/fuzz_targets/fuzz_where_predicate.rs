#![no_main]
//! Structural fuzzing of the pre-aggregation `where_clause` splice.
//!
//! The predicate is the one place where **arbitrary user text** is spliced
//! verbatim into generated SQL. Member references inside it are rewritten to
//! their declared expressions by `expr_tokens::inline_references` — the same
//! quote/literal-aware pass the derived-metric path uses — and the result is
//! emitted after `WHERE` on the base-anchored path, or inside a grain /
//! snapshot / aggregation CTE on the others.
//!
//! That splice is precisely the seam issue #145 came from (a quoted identifier
//! carrying an embedded quote corrupted the output), so the oracle is the same
//! structural one `fuzz_sql_expand` uses: **balanced input must yield balanced
//! output**. An odd bare `"` or an unclosed paren in the emitted SQL means the
//! splice tore a literal or an identifier apart.
//!
//! Errors are expected and fine — `expand()` rejecting a predicate that names a
//! metric, or a definition that makes no sense, is not a bug. Only a
//! *structurally corrupt success* is.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::SemanticViewDefinition;

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    def: SemanticViewDefinition,
    dim_names: Vec<String>,
    metric_names: Vec<String>,
    fact_names: Vec<String>,
    /// The raw predicate text, spliced into the generated SQL.
    predicate: String,
    /// Exercise the fact path (plain `WHERE`) as well as the metric paths.
    use_facts: bool,
}

/// Walk `s` with the SQL lexer's escape rules (`''` inside strings, `""` inside
/// quoted identifiers) and report whether quotes and parens close cleanly.
/// Shared shape with `fuzz_sql_expand`'s oracle — see TECH-DEBT #33 for why the
/// struct-domain targets use a structural oracle rather than executing SQL.
fn is_balanced(s: &str) -> bool {
    let bytes = s.as_bytes();
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

/// Every free-form fragment of the definition that reaches the output verbatim.
///
/// The precondition must cover ALL of them: an incomplete precondition is the
/// recurring bug class in these targets (TECH-DEBT #33), because an unbalanced
/// *input* fragment legitimately produces unbalanced output and would be
/// reported as a false crash.
fn def_fragments_balanced(def: &SemanticViewDefinition) -> bool {
    let mut frags: Vec<&str> = Vec::new();
    for t in &def.tables {
        frags.push(&t.alias);
        frags.push(&t.table);
    }
    for d in &def.dimensions {
        frags.push(&d.name);
        frags.push(&d.expr);
        if let Some(s) = d.source_table.as_deref() {
            frags.push(s);
        }
    }
    for m in &def.metrics {
        frags.push(&m.name);
        frags.push(&m.expr);
        if let Some(s) = m.source_table.as_deref() {
            frags.push(s);
        }
    }
    for f in &def.facts {
        frags.push(&f.name);
        frags.push(&f.expr);
        if let Some(s) = f.source_table.as_deref() {
            frags.push(s);
        }
    }
    frags.into_iter().all(is_balanced)
}

fuzz_target!(|input: FuzzInput| {
    let req = QueryRequest {
        dimensions: input.dim_names.into_iter().map(Into::into).collect(),
        metrics: if input.use_facts {
            vec![]
        } else {
            input.metric_names.into_iter().map(Into::into).collect()
        },
        facts: if input.use_facts {
            input.fact_names.into_iter().map(Into::into).collect()
        } else {
            vec![]
        },
        where_clause: Some(input.predicate.clone()),
    };

    // The predicate is spliced verbatim, so it joins the definition's fragments
    // in the precondition: an unbalanced predicate yields unbalanced SQL by
    // construction, which is the caller's error rather than a splice bug.
    let inputs_ok = def_fragments_balanced(&input.def) && is_balanced(&input.predicate);

    if let Ok(sql) = expand("fuzz_view", &input.def, &req) {
        assert!(!sql.is_empty(), "successful expansion produced empty SQL");
        if inputs_ok {
            assert!(
                is_balanced(&sql),
                "unbalanced quotes/parens after splicing predicate {:?}: {sql}",
                input.predicate
            );
        }
    }
});
