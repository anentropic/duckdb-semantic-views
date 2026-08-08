//! Structural guard: the parser hook must be the **last** entry in
//! `REGISTRATIONS` (LIFE-2, code-review 2026-08-08).
//!
//! `init_extension` registers everything in `REGISTRATIONS` order and returns
//! `Err` on the first failure. Nothing rolls back, so whatever was registered
//! before the failure stays live for the rest of the process. With the parser
//! hook first, a failure in any read-side registration left the extension
//! *half-active*: `sv_register_parser_hooks` had already installed
//! `parser_override` / `parse_function` **and** flipped
//! `allow_parser_override_extension` to `FALLBACK`
//! (`cpp/src/shim.cpp::sv_register_parser_hooks`), while the table functions
//! the hook's own rewrites call were missing. `SHOW SEMANTIC VIEWS` would then
//! rewrite to `SELECT * FROM list_semantic_views()` and fail on an unresolved
//! function name, and `CREATE SEMANTIC VIEW` would write catalog rows nothing
//! could read back — a worse end state than a LOAD that simply did nothing.
//!
//! The dependency runs one way only:
//!
//! * Every read-side registration is a pure DuckDB Catalog-API call
//!   (`sv_register_table_function_core` / `sv_register_scalar_function_set`
//!   build a `TableFunction` / `ScalarFunction` and hand it to
//!   `Catalog::GetSystemCatalog(db)`). None of them parses SQL, so none of
//!   them needs the parser hook.
//! * The parser hook's *output* does need them: `plan_rewrite` lowers
//!   `SHOW SEMANTIC VIEWS` to `list_semantic_views()`, `DESCRIBE SEMANTIC VIEW`
//!   to `describe_semantic_view(...)`, and `CREATE ... FROM YAML FILE` to
//!   `__sv_compute_create_from_yaml(...)`.
//!
//! So registering the hook last is the safe order, and it is what this test
//! pins. The window is not closed entirely — the catalog schema
//! (`init_catalog`) is still created before the loop runs — but after this
//! change a partial LOAD leaves semantic DDL *inert* (DuckDB's own
//! `syntax error at or near "SEMANTIC"`) rather than half-working.
//!
//! ## Known limitation
//!
//! This is a syntactic check over the `sv_registrations!` invocation in
//! `src/lib.rs`. It cannot see a dependency introduced *inside* a registration
//! wrapper on the C++ side; the one-way argument above has to be re-checked by
//! hand if a wrapper ever starts running SQL.

use syn::visit::Visit;
use syn::ItemMacro;

/// Collects the `sv_registrations!` invocation's token text from `src/lib.rs`.
struct MacroFinder {
    tokens: Option<String>,
}

impl<'ast> Visit<'ast> for MacroFinder {
    fn visit_item_macro(&mut self, m: &'ast ItemMacro) {
        if m.mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "sv_registrations")
        {
            self.tokens = Some(m.mac.tokens.to_string());
        }
        syn::visit::visit_item_macro(self, m);
    }
}

/// Pull the string literals out of the macro's token text, in order. The
/// invocation is a list of `("<label>", sv_register_<sym>)` pairs, so the
/// literals are exactly the human labels `init_extension` iterates.
fn labels_in_order(tokens: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = tokens.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut lit = String::new();
        for c in chars.by_ref() {
            if c == '"' {
                break;
            }
            lit.push(c);
        }
        out.push(lit);
    }
    out
}

#[test]
fn parser_hook_is_registered_last() {
    let src = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");
    let file: syn::File = syn::parse_str(&src).expect("parse src/lib.rs");

    let mut finder = MacroFinder { tokens: None };
    finder.visit_file(&file);
    let tokens = finder
        .tokens
        .expect("src/lib.rs must contain an `sv_registrations!` invocation");

    let labels = labels_in_order(&tokens);
    assert!(
        labels.len() > 1,
        "expected several registration labels, got {labels:?} — the scanner \
         is probably out of step with the macro's shape"
    );
    assert_eq!(
        labels.iter().filter(|l| *l == "parser hooks").count(),
        1,
        "expected exactly one \"parser hooks\" entry in REGISTRATIONS; got {labels:?}"
    );
    assert_eq!(
        labels.last().map(String::as_str),
        Some("parser hooks"),
        "LIFE-2: the parser hook must be the LAST entry in REGISTRATIONS. \
         `init_extension` returns Err on the first failing registration and \
         rolls nothing back, so registering the hook earlier leaves a failed \
         LOAD with parser_override installed and \
         allow_parser_override_extension='FALLBACK' while the read table \
         functions its own rewrites call are absent. Read-side registrations \
         are pure Catalog-API calls and never need the hook, so the hook goes \
         last. Order found: {labels:?}"
    );
}
