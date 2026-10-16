//! Transforms for the read statements: `SHOW SEMANTIC ...`, `SHOW COLUMNS IN
//! SEMANTIC VIEW` and `DESCRIBE SEMANTIC VIEW`.
//!
//! Each lowers to the same query the parser override emits for it
//! ([`lower_show`], [`lower_name_only_read`]), so the two paths cannot drift
//! while both exist. Each is a query, so it can stand wherever `DuckDB`'s
//! grammar accepts one: a subquery, a CTE, a view body (TECH-DEBT #77).

use super::tree::{Kind, Node, Tree};
use crate::errors::ParseError;
use crate::parse::{lower_name_only_read, lower_show, DdlKind, ScopeName, ShowClauses};

/// The grammar rules for the read statements, in `DuckDB`'s PEG syntax.
///
/// Two shapes here work around what that syntax lacks:
///
/// - The scope name after `IN SCHEMA` / `IN DATABASE` is optional, so the
///   word after it may be the next clause instead. `LIMIT` and `FOR` are
///   reserved words, which a name cannot be, but `STARTS` is not, and the
///   grammar has no negative lookahead (`DuckDB`'s PEG reader ignores `!`). So
///   the bare scope followed by `STARTS WITH` is its own alternative, tried
///   first; the transform finds the `STARTS WITH` clause wherever it sits.
/// - `LIMIT` takes any single value, so a non-integer is our error with our
///   wording rather than a `LIMIT` modifier on the query `DuckDB` would bind.
pub(super) const RULES: &str = r"
SvShowStatement <- 'SHOW' SvShowWhat SvShowLike? SvShowIn? SvShowForMetric? SvShowStartsWith? SvShowLimit?
SvShowWhat <- SvShowTerse / SvShowViews / SvShowDimensions / SvShowMetrics / SvShowFacts / SvShowMaterializations
SvShowTerse <- 'TERSE' 'SEMANTIC' 'VIEWS'
SvShowViews <- 'SEMANTIC' 'VIEWS'
SvShowDimensions <- 'SEMANTIC' 'DIMENSIONS'
SvShowMetrics <- 'SEMANTIC' 'METRICS'
SvShowFacts <- 'SEMANTIC' 'FACTS'
SvShowMaterializations <- 'SEMANTIC' 'MATERIALIZATIONS'
SvShowLike <- 'LIKE' StringLiteral
SvShowIn <- 'IN' SvShowScope
SvShowScope <- SvShowInAccount / SvShowInSchema / SvShowInDatabase / SvShowInView
SvShowInAccount <- 'ACCOUNT'
SvShowInSchema <- 'SCHEMA' SvShowStartsWith / 'SCHEMA' SvScopeName?
SvShowInDatabase <- 'DATABASE' SvShowStartsWith / 'DATABASE' SvScopeName?
SvScopeName <- SvNamePart ('.' SvNamePart)*
SvShowInView <- SvViewName
SvShowForMetric <- 'FOR' 'METRIC' SvMemberName
SvShowStartsWith <- 'STARTS' 'WITH' StringLiteral
SvShowLimit <- 'LIMIT' SvShowLimitValue
SvShowLimitValue <- NumberLiteral / '-' NumberLiteral / ColId / StringLiteral
SvShowColumns <- 'SHOW' 'COLUMNS' 'IN' 'SEMANTIC' 'VIEW' SvViewName
SvDescribe <- SvDescribeWord 'SEMANTIC' 'VIEW' SvViewName
SvDescribeWord <- 'DESCRIBE' / 'DESC'
SvViewName <- SvNamePart ('.' SvNamePart)*
SvMemberName <- SvNamePart ('.' SvNamePart)*
SvNamePart <- ColId
";

/// The statement rules, each a query alternative of `DescribeStatement`.
pub(super) const QUERY_STATEMENTS: &[&str] = &["SvShowStatement", "SvShowColumns", "SvDescribe"];

/// Lower one matched read statement to SQL.
pub(super) fn transform(rule: &str, tree: &Tree) -> Result<String, ParseError> {
    let root = &tree.root;
    if rule.eq_ignore_ascii_case("SvDescribe") {
        let name = required(tree, root, "SvViewName")?;
        return Ok(lower_name_only_read(DdlKind::Describe, &name));
    }
    if rule.eq_ignore_ascii_case("SvShowColumns") {
        let name = required(tree, root, "SvViewName")?;
        return Ok(lower_name_only_read(DdlKind::ShowColumns, &name));
    }
    transform_show(tree)
}

fn transform_show(tree: &Tree) -> Result<String, ParseError> {
    let root = &tree.root;
    let kind = show_kind(root)?;
    let entity = match kind {
        DdlKind::Show | DdlKind::ShowTerse => "VIEWS",
        DdlKind::ShowDimensions => "DIMENSIONS",
        DdlKind::ShowMetrics => "METRICS",
        DdlKind::ShowMaterializations => "MATERIALIZATIONS",
        _ => "FACTS",
    };

    let like_pattern = root.find("SvShowLike").map(string_value);
    let starts_with_clauses = root.find_all("SvShowStartsWith");
    if let Some(second) = starts_with_clauses.get(1) {
        return Err(ParseError {
            message: "STARTS WITH given twice".to_string(),
            position: second.offset(),
        });
    }
    let starts_with = starts_with_clauses.first().map(|n| string_value(n));

    let (in_view, in_schema, in_database) = match root.find("SvShowScope") {
        Some(scope) => show_scope(tree, scope, kind)?,
        None => (None, None, None),
    };

    let mut for_metric = None;
    if let Some(metric) = root.find("SvShowForMetric") {
        if kind != DdlKind::ShowDimensions {
            return Err(ParseError {
                message: format!(
                    "FOR METRIC is only valid for SHOW SEMANTIC DIMENSIONS, not SHOW SEMANTIC {entity}"
                ),
                position: metric.offset(),
            });
        }
        let name = metric.find("SvMemberName").unwrap_or(metric);
        for_metric = Some(name_text(tree, name));
    }

    let mut limit = None;
    if let Some(node) = root.find("SvShowLimit") {
        let token = node
            .find("SvShowLimitValue")
            .map(|v| name_text(tree, v))
            .unwrap_or_default();
        let n: u64 = token.parse().map_err(|_| ParseError {
            message: format!("LIMIT must be a non-negative integer, got: '{token}'"),
            position: node.offset(),
        })?;
        limit = Some(n);
    }

    let clauses = ShowClauses {
        like_pattern,
        in_view: in_view.as_deref(),
        in_schema,
        in_database,
        for_metric: for_metric.as_deref(),
        starts_with,
        limit,
    };
    lower_show(kind, &clauses, root.offset().unwrap_or(0))
}

/// What an `IN` clause names: a view, or a schema and / or database scope.
/// `IN ACCOUNT` names nothing: it is an unfiltered listing.
type Scope = (Option<String>, Option<ScopeName>, Option<ScopeName>);

fn show_scope(tree: &Tree, scope: &Node, kind: DdlKind) -> Result<Scope, ParseError> {
    if let Some(view) = scope.find("SvShowInView") {
        // VIEWS has no view-name slot: there `IN` is always a scope.
        if matches!(kind, DdlKind::Show | DdlKind::ShowTerse) {
            return Err(ParseError {
                message: "SHOW SEMANTIC VIEWS requires a scope: IN {ACCOUNT | DATABASE [db] | SCHEMA [[db.]schema]}"
                    .to_string(),
                position: view.offset(),
            });
        }
        return Ok((Some(name_text(tree, view)), None, None));
    }
    if let Some(schema) = scope.find("SvShowInSchema") {
        let Some(name) = schema.find("SvScopeName") else {
            return Ok((None, Some(ScopeName::Current), None));
        };
        let mut parts = name_parts(name);
        return match parts.len() {
            1 => Ok((None, parts.pop().map(ScopeName::Named), None)),
            // The schema is the last part; the database qualifies it.
            2 => {
                let schema = parts.pop().map(ScopeName::Named);
                Ok((None, schema, parts.pop().map(ScopeName::Named)))
            }
            _ => Err(ParseError {
                message: format!(
                    "Invalid schema name '{}': expected <schema> or <database>.<schema>",
                    name_text(tree, name)
                ),
                position: name.offset(),
            }),
        };
    }
    if let Some(database) = scope.find("SvShowInDatabase") {
        let Some(name) = database.find("SvScopeName") else {
            return Ok((None, None, Some(ScopeName::Current)));
        };
        let mut parts = name_parts(name);
        if parts.len() != 1 {
            return Err(ParseError {
                message: format!(
                    "Invalid database name '{}': expected a single <database>",
                    name_text(tree, name)
                ),
                position: name.offset(),
            });
        }
        return Ok((None, None, parts.pop().map(ScopeName::Named)));
    }
    Ok((None, None, None))
}

fn show_kind(root: &Node) -> Result<DdlKind, ParseError> {
    let kinds = [
        ("SvShowTerse", DdlKind::ShowTerse),
        ("SvShowViews", DdlKind::Show),
        ("SvShowDimensions", DdlKind::ShowDimensions),
        ("SvShowMetrics", DdlKind::ShowMetrics),
        ("SvShowFacts", DdlKind::ShowFacts),
        ("SvShowMaterializations", DdlKind::ShowMaterializations),
    ];
    kinds
        .iter()
        .find(|(rule, _)| root.find(rule).is_some())
        .map(|(_, kind)| *kind)
        .ok_or_else(|| internal(root, "SHOW statement without an object kind"))
}

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

/// The parts of a dotted name, each unquoted.
fn name_parts(node: &Node) -> Vec<String> {
    node.find_all("SvNamePart")
        .into_iter()
        .map(|part| {
            part.first_leaf(Kind::Identifier)
                .or_else(|| part.first_leaf(Kind::Keyword))
                .map(|leaf| leaf.value.clone())
                .unwrap_or_default()
        })
        .collect()
}

fn required(tree: &Tree, root: &Node, rule: &str) -> Result<String, ParseError> {
    root.find(rule)
        .map(|n| name_text(tree, n))
        .ok_or_else(|| internal(root, &format!("missing {rule}")))
}

fn internal(node: &Node, what: &str) -> ParseError {
    ParseError {
        message: format!("semantic_views grammar: {what} in {}", node.rule),
        position: node.offset(),
    }
}

#[cfg(test)]
mod tests {
    use super::transform;
    use crate::grammar::tree::{Kind, Node, Token, Tree};

    /// A node spanning the first occurrence of `text` in `sql`.
    fn n(sql: &str, rule: &str, text: &str, kind: Kind, value: &str, children: Vec<Node>) -> Node {
        let offset = sql
            .find(text)
            .unwrap_or_else(|| panic!("{text} not in {sql}"));
        Node {
            rule: rule.to_string(),
            kind,
            span: Some((offset, text.len())),
            value: value.to_string(),
            children,
        }
    }

    fn at(rule: &str, offset: usize, len: usize, kind: Kind, value: &str) -> Node {
        Node {
            rule: rule.to_string(),
            kind,
            span: Some((offset, len)),
            value: value.to_string(),
            children: Vec::new(),
        }
    }

    /// The statement's tokens: words split on whitespace, with `.` split off
    /// as its own token.
    fn tree(sql: &str, root: Node) -> Tree {
        let mut tokens = Vec::new();
        let mut start = None;
        for (i, c) in sql.char_indices().chain(std::iter::once((sql.len(), ' '))) {
            let boundary = c.is_whitespace() || c == '.';
            if boundary {
                if let Some(s) = start.take() {
                    tokens.push(Token {
                        offset: s,
                        len: i - s,
                        text: sql[s..i].to_string(),
                    });
                }
                if c == '.' {
                    tokens.push(Token {
                        offset: i,
                        len: 1,
                        text: ".".to_string(),
                    });
                }
            } else if start.is_none() {
                start = Some(i);
            }
        }
        Tree { root, tokens }
    }

    fn root(sql: &str, rule: &str, children: Vec<Node>) -> Node {
        n(sql, rule, sql, Kind::List, "", children)
    }

    const L: Kind = Kind::List;

    #[test]
    fn show_dimensions_for_metric_lowers_to_the_metric_function() {
        let sql = "SHOW SEMANTIC DIMENSIONS IN v FOR METRIC m LIMIT 3";
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(
                    sql,
                    "SvShowDimensions",
                    "SEMANTIC DIMENSIONS",
                    L,
                    "",
                    vec![],
                ),
                n(
                    sql,
                    "SvShowScope",
                    "v",
                    L,
                    "",
                    vec![n(sql, "SvShowInView", "v", L, "", vec![])],
                ),
                n(
                    sql,
                    "SvShowForMetric",
                    "FOR METRIC m",
                    L,
                    "",
                    vec![n(sql, "SvMemberName", " m", L, "", vec![])],
                ),
                n(
                    sql,
                    "SvShowLimit",
                    "LIMIT 3",
                    L,
                    "",
                    vec![n(sql, "SvShowLimitValue", "3", Kind::Number, "3", vec![])],
                ),
            ],
        );
        assert_eq!(
            transform("SvShowStatement", &tree(sql, r)).unwrap(),
            "SELECT * FROM show_semantic_dimensions_for_metric('v', 'm') LIMIT 3"
        );
    }

    #[test]
    fn show_views_in_a_view_name_is_an_error() {
        let sql = "SHOW SEMANTIC VIEWS IN v";
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowViews", "SEMANTIC VIEWS", L, "", vec![]),
                n(
                    sql,
                    "SvShowScope",
                    " v",
                    L,
                    "",
                    vec![n(sql, "SvShowInView", " v", L, "", vec![])],
                ),
            ],
        );
        let err = transform("SvShowStatement", &tree(sql, r)).unwrap_err();
        assert!(
            err.message
                .starts_with("SHOW SEMANTIC VIEWS requires a scope"),
            "{}",
            err.message
        );
        assert_eq!(err.position, sql.find(" v"));
    }

    #[test]
    fn for_metric_on_another_object_kind_is_an_error() {
        let sql = "SHOW SEMANTIC METRICS IN v FOR METRIC m";
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowMetrics", "SEMANTIC METRICS", L, "", vec![]),
                n(sql, "SvShowForMetric", "FOR METRIC m", L, "", vec![]),
            ],
        );
        let err = transform("SvShowStatement", &tree(sql, r)).unwrap_err();
        assert_eq!(
            err.message,
            "FOR METRIC is only valid for SHOW SEMANTIC DIMENSIONS, not SHOW SEMANTIC METRICS"
        );
        assert_eq!(err.position, sql.find("FOR"));
    }

    #[test]
    fn for_metric_without_a_view_is_an_error() {
        let sql = "SHOW SEMANTIC DIMENSIONS FOR METRIC m";
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(
                    sql,
                    "SvShowDimensions",
                    "SEMANTIC DIMENSIONS",
                    L,
                    "",
                    vec![],
                ),
                n(
                    sql,
                    "SvShowForMetric",
                    "FOR METRIC m",
                    L,
                    "",
                    vec![n(sql, "SvMemberName", " m", L, "", vec![])],
                ),
            ],
        );
        let err = transform("SvShowStatement", &tree(sql, r)).unwrap_err();
        assert_eq!(err.message, "FOR METRIC requires IN view_name");
    }

    #[test]
    fn a_negative_limit_is_an_error_with_the_value_as_written() {
        let sql = "SHOW SEMANTIC VIEWS LIMIT -1";
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowViews", "SEMANTIC VIEWS", L, "", vec![]),
                n(
                    sql,
                    "SvShowLimit",
                    "LIMIT -1",
                    L,
                    "",
                    vec![n(sql, "SvShowLimitValue", "-1", L, "", vec![])],
                ),
            ],
        );
        let err = transform("SvShowStatement", &tree(sql, r)).unwrap_err();
        assert_eq!(
            err.message,
            "LIMIT must be a non-negative integer, got: '-1'"
        );
    }

    #[test]
    fn starts_with_twice_is_an_error() {
        let sql = "SHOW SEMANTIC VIEWS IN SCHEMA STARTS WITH 'a' STARTS WITH 'b'";
        let second = sql.rfind("STARTS").unwrap();
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowViews", "SEMANTIC VIEWS", L, "", vec![]),
                n(
                    sql,
                    "SvShowScope",
                    "SCHEMA STARTS WITH 'a'",
                    L,
                    "",
                    vec![n(
                        sql,
                        "SvShowInSchema",
                        "SCHEMA STARTS WITH 'a'",
                        L,
                        "",
                        vec![n(
                            sql,
                            "SvShowStartsWith",
                            "STARTS WITH 'a'",
                            L,
                            "",
                            vec![n(sql, "", "'a'", Kind::String, "a", vec![])],
                        )],
                    )],
                ),
                Node {
                    children: vec![at("", second + 12, 3, Kind::String, "b")],
                    ..at("SvShowStartsWith", second, 15, L, "")
                },
            ],
        );
        let err = transform("SvShowStatement", &tree(sql, r)).unwrap_err();
        assert_eq!(err.message, "STARTS WITH given twice");
        assert_eq!(err.position, Some(second));
    }

    #[test]
    fn a_bare_schema_scope_then_starts_with_keeps_both() {
        let sql = "SHOW SEMANTIC VIEWS IN SCHEMA STARTS WITH 'a'";
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowViews", "SEMANTIC VIEWS", L, "", vec![]),
                n(
                    sql,
                    "SvShowScope",
                    "SCHEMA STARTS WITH 'a'",
                    L,
                    "",
                    vec![n(
                        sql,
                        "SvShowInSchema",
                        "SCHEMA STARTS WITH 'a'",
                        L,
                        "",
                        vec![n(
                            sql,
                            "SvShowStartsWith",
                            "STARTS WITH 'a'",
                            L,
                            "",
                            vec![n(sql, "", "'a'", Kind::String, "a", vec![])],
                        )],
                    )],
                ),
            ],
        );
        assert_eq!(
            transform("SvShowStatement", &tree(sql, r)).unwrap(),
            "SELECT * FROM list_semantic_views() WHERE starts_with(name, 'a') \
             AND lower(schema_name) = lower(current_schema())"
        );
    }

    #[test]
    fn a_two_part_schema_scope_filters_on_database_and_schema() {
        let sql = "SHOW SEMANTIC VIEWS IN SCHEMA db.s";
        let s_at = sql.rfind('s').unwrap();
        let part = |offset: usize, name: &str| Node {
            children: vec![at("", offset, name.len(), Kind::Identifier, name)],
            ..at("SvNamePart", offset, name.len(), L, "")
        };
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowViews", "SEMANTIC VIEWS", L, "", vec![]),
                n(
                    sql,
                    "SvShowScope",
                    "SCHEMA db.s",
                    L,
                    "",
                    vec![n(
                        sql,
                        "SvShowInSchema",
                        "SCHEMA db.s",
                        L,
                        "",
                        vec![n(
                            sql,
                            "SvScopeName",
                            "db.s",
                            L,
                            "",
                            vec![part(sql.find("db").unwrap(), "db"), part(s_at, "s")],
                        )],
                    )],
                ),
            ],
        );
        assert_eq!(
            transform("SvShowStatement", &tree(sql, r)).unwrap(),
            "SELECT * FROM list_semantic_views() WHERE lower(schema_name) = lower('s') \
             AND lower(database_name) = lower('db')"
        );
    }

    #[test]
    fn a_three_part_schema_scope_is_an_error() {
        let sql = "SHOW SEMANTIC VIEWS IN SCHEMA a.b.c";
        let part = |name: &str| Node {
            children: vec![n(
                sql,
                "",
                &format!(".{name}"),
                Kind::Identifier,
                name,
                vec![],
            )],
            ..n(sql, "SvNamePart", &format!(".{name}"), L, "", vec![])
        };
        let first = Node {
            children: vec![n(sql, "", " a", Kind::Identifier, "a", vec![])],
            ..n(sql, "SvNamePart", " a", L, "", vec![])
        };
        let r = root(
            sql,
            "SvShowStatement",
            vec![
                n(sql, "SvShowViews", "SEMANTIC VIEWS", L, "", vec![]),
                n(
                    sql,
                    "SvShowScope",
                    "SCHEMA a.b.c",
                    L,
                    "",
                    vec![n(
                        sql,
                        "SvShowInSchema",
                        "SCHEMA a.b.c",
                        L,
                        "",
                        vec![n(
                            sql,
                            "SvScopeName",
                            "a.b.c",
                            L,
                            "",
                            vec![first, part("b"), part("c")],
                        )],
                    )],
                ),
            ],
        );
        let err = transform("SvShowStatement", &tree(sql, r)).unwrap_err();
        assert_eq!(
            err.message,
            "Invalid schema name 'a.b.c': expected <schema> or <database>.<schema>"
        );
    }

    #[test]
    fn describe_passes_the_name_as_written() {
        let sql = "DESCRIBE SEMANTIC VIEW \"My\".v";
        let r = root(
            sql,
            "SvDescribe",
            vec![n(sql, "SvViewName", "\"My\".v", L, "", vec![])],
        );
        assert_eq!(
            transform("SvDescribe", &tree(sql, r)).unwrap(),
            "SELECT * FROM describe_semantic_view('\"My\".v')"
        );
    }

    #[test]
    fn show_columns_lowers_to_its_function() {
        let sql = "SHOW COLUMNS IN SEMANTIC VIEW v";
        let r = root(
            sql,
            "SvShowColumns",
            vec![n(sql, "SvViewName", " v", L, "", vec![])],
        );
        assert_eq!(
            transform("SvShowColumns", &tree(sql, r)).unwrap(),
            "SELECT * FROM show_columns_in_semantic_view('v')"
        );
    }
}
