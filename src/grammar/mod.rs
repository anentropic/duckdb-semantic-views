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

mod create;
mod show;
pub(crate) mod tree;
mod write;

#[cfg(feature = "extension")]
pub(crate) mod ffi;

use crate::errors::ParseError;
use crate::parse::RewriteAction;
use std::fmt::Write;
use tree::{Kind, Node, Tree};

/// What a statement rule's transform returns, which decides where `DuckDB`
/// may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// One query (a `QueryNode`): an alternative of `DescribeStatement`, so it
    /// also stands as a subquery.
    Query,
    /// A statement (a `SQLStatement`, several as one `MultiStatement`): an
    /// alternative of `Statement`, so it stands only at the top level.
    Statement,
}

impl Shape {
    fn wire_name(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Statement => "statement",
        }
    }

    /// The `DuckDB` rule a statement rule of this shape is an alternative of.
    fn target(self) -> &'static str {
        match self {
            Self::Query => "DescribeStatement",
            Self::Statement => "Statement",
        }
    }
}

/// The grammar changes to register, one per line:
///
/// - `RULE\t<definition>\t<shape>`: add a rule. `<shape>` is empty for a
///   helper rule, or names the transform's result for a statement rule.
/// - `ANY_TOKEN\t<rule>`: match `<rule>` with `DuckDB`'s matcher for any one
///   token short of the statement's end, instead of its body.
/// - `PREPEND\t<rule>\t<choice>`: make `<choice>` the first alternative of
///   an existing `DuckDB` rule.
pub(crate) fn changes() -> String {
    let mut out = String::new();
    let statements: Vec<(&str, Shape)> = show::QUERY_STATEMENTS
        .iter()
        .map(|rule| (*rule, Shape::Query))
        .chain(
            write::STATEMENTS
                .iter()
                .chain(create::STATEMENTS)
                .map(|rule| (*rule, Shape::Statement)),
        )
        .collect();
    let definitions = show::RULES
        .lines()
        .chain(write::RULES.lines())
        .chain(create::RULES.lines());
    for definition in definitions.map(str::trim).filter(|l| !l.is_empty()) {
        let name = definition.split_whitespace().next().unwrap_or_default();
        let shape = statements
            .iter()
            .find(|(rule, _)| *rule == name)
            .map_or("", |(_, shape)| shape.wire_name());
        let _ = writeln!(out, "RULE\t{definition}\t{shape}");
    }
    for rule in create::ANY_TOKEN {
        let _ = writeln!(out, "ANY_TOKEN\t{rule}");
    }
    for (rule, shape) in &statements {
        let _ = writeln!(out, "PREPEND\t{}\t{rule}", shape.target());
    }
    out
}

/// Plan one matched statement rule: the same [`RewriteAction`] the parser
/// override plans for the statement, which `crate::parse::lower_action` turns
/// into SQL.
#[cfg_attr(not(feature = "extension"), allow(dead_code))]
pub(crate) fn transform(tree: &Tree) -> Result<RewriteAction, ParseError> {
    let rule = tree.root.rule.as_str();
    let is = |rules: &[&str]| rules.iter().any(|r| r.eq_ignore_ascii_case(rule));
    if is(show::QUERY_STATEMENTS) {
        return show::transform(rule, tree).map(RewriteAction::Passthrough);
    }
    if is(write::STATEMENTS) {
        return write::transform(rule, tree);
    }
    if is(create::STATEMENTS) {
        return create::transform(tree);
    }
    Err(ParseError {
        message: format!("semantic_views grammar: no transform for rule '{rule}'"),
        position: tree.root.offset(),
    })
}

// --- helpers the statement modules share ---

/// A string literal's decoded contents.
fn string_value(node: &Node) -> String {
    node.first_leaf(Kind::String)
        .map(|n| n.value.clone())
        .unwrap_or_default()
}

/// A name exactly as written, quotes and qualifiers included, with no
/// whitespace: the read table functions fold and resolve it themselves (FF-4).
fn name_text(tree: &Tree, node: &Node) -> String {
    tree.tokens_in(node).map(|t| t.text.as_str()).collect()
}

fn internal(node: &Node, what: &str) -> ParseError {
    ParseError {
        message: format!("semantic_views grammar: {what} in {}", node.rule),
        position: node.offset(),
    }
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
        for rule in super::create::ANY_TOKEN {
            let at = text.find(&format!("ANY_TOKEN\t{rule}\n")).unwrap();
            let defined = text.find(&format!("RULE\t{rule} <-")).unwrap();
            assert!(defined < at, "{rule} must be defined before its override");
        }
        for rule in super::write::STATEMENTS
            .iter()
            .chain(super::create::STATEMENTS)
        {
            assert!(
                text.lines()
                    .any(|l| l.starts_with(&format!("RULE\t{rule} <-"))
                        && l.ends_with("\tstatement")),
                "{rule} is not defined as a statement rule"
            );
            assert!(text.contains(&format!("PREPEND\tStatement\t{rule}\n")));
        }
    }

    #[test]
    fn no_rule_is_defined_twice() {
        let text = changes();
        let mut names: Vec<&str> = text
            .lines()
            .filter_map(|l| l.strip_prefix("RULE\t"))
            .map(|l| l.split_whitespace().next().unwrap())
            .collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
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
