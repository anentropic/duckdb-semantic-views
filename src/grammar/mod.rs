//! Semantic-view syntax as productions of `DuckDB`'s own grammar (`DuckDB` 2.0).
//!
//! `DuckDB` 2.0 parses with a PEG grammar that an extension can extend. This
//! module owns our part of it: the rule text ([`changes`]) and the transforms
//! that turn a matched rule into SQL ([`transform`]). The C++ shim only
//! bridges: it registers the rules `DuckDB` gets from [`changes`], and when one
//! of the statement rules matches it serialises the parse subtree (see
//! [`tree`]), calls [`transform`] through `sv_grammar_transform_rust`, and
//! parses the SQL that comes back with the builtin parser.
//!
//! The parser override still runs first and still owns every statement that
//! starts with our syntax. The grammar takes what the override cannot reach,
//! such as SHOW inside a subquery, and everything when overrides are off
//! (`SET allow_parser_override_extension = 'DEFAULT'`), which is how the DDL
//! corpus is run against both while they coexist (`.planning/PEG-PARSER-EPIC.md`,
//! Phase 3).

mod show;
pub(crate) mod tree;

#[cfg(feature = "extension")]
pub(crate) mod ffi;

use crate::errors::ParseError;
use std::fmt::Write;
use tree::Tree;

/// What a statement rule's transform returns, which decides where `DuckDB`
/// may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// One query (a `QueryNode`): an alternative of `DescribeStatement`, so it
    /// also stands as a subquery.
    Query,
}

impl Shape {
    fn wire_name(self) -> &'static str {
        match self {
            Self::Query => "query",
        }
    }
}

/// The grammar changes to register, one per line:
///
/// - `RULE\t<definition>\t<shape>`: add a rule. `<shape>` is empty for a
///   helper rule, or names the transform's result for a statement rule.
/// - `PREPEND\t<rule>\t<choice>`: make `<choice>` the first alternative of
///   an existing `DuckDB` rule.
pub(crate) fn changes() -> String {
    let mut out = String::new();
    let statements: Vec<(&str, Shape)> = show::QUERY_STATEMENTS
        .iter()
        .map(|rule| (*rule, Shape::Query))
        .collect();
    for definition in show::RULES.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let name = definition.split_whitespace().next().unwrap_or_default();
        let shape = statements
            .iter()
            .find(|(rule, _)| *rule == name)
            .map_or("", |(_, shape)| shape.wire_name());
        let _ = writeln!(out, "RULE\t{definition}\t{shape}");
    }
    for (rule, shape) in &statements {
        let target = match shape {
            Shape::Query => "DescribeStatement",
        };
        let _ = writeln!(out, "PREPEND\t{target}\t{rule}");
    }
    out
}

/// Lower one matched statement rule to SQL.
pub(crate) fn transform(tree: &Tree) -> Result<String, ParseError> {
    let rule = tree.root.rule.as_str();
    if show::QUERY_STATEMENTS
        .iter()
        .any(|r| r.eq_ignore_ascii_case(rule))
    {
        return show::transform(rule, tree);
    }
    Err(ParseError {
        message: format!("semantic_views grammar: no transform for rule '{rule}'"),
        position: tree.root.offset(),
    })
}

#[cfg(test)]
mod tests {
    use super::changes;

    #[test]
    fn every_statement_rule_is_defined_and_prepended() {
        let text = changes();
        for rule in super::show::QUERY_STATEMENTS {
            assert!(
                text.lines()
                    .any(|l| l.starts_with(&format!("RULE\t{rule} <-")) && l.ends_with("\tquery")),
                "{rule} is not defined as a query rule"
            );
            assert!(text.contains(&format!("PREPEND\tDescribeStatement\t{rule}\n")));
        }
    }

    #[test]
    fn helper_rules_have_no_shape() {
        let text = changes();
        let line = text
            .lines()
            .find(|l| l.starts_with("RULE\tSvViewName <-"))
            .unwrap();
        assert!(line.ends_with('\t'));
    }
}
