"""Custom Pygments lexer for SQL grammar notation.

Highlights SQL grammar syntax diagrams with 4 token types:
  - Keywords (CREATE, SEMANTIC, VIEW, ...) -> Token.Keyword
  - Placeholders (<name>, <column>, ...) -> Token.Name.Variable.Class
  - Optional brackets [ ] and ellipsis ... -> Token.Name.Tag
  - String literals ('month', ...) -> Token.Literal.String
"""

from pygments.lexer import RegexLexer, words
from pygments.token import Token


class SqlGrammarLexer(RegexLexer):
    name = "sqlgrammar"
    aliases = ["sqlgrammar"]

    tokens = {
        "root": [
            # Whitespace
            (r"\s+", Token.Text),
            # String literals (single-quoted)
            (r"'[^']*'", Token.Literal.String),
            # Qualified placeholders: <alias>.<name>
            (r"<[a-zA-Z_][a-zA-Z0-9_]*>\.<[a-zA-Z_][a-zA-Z0-9_]*>", Token.Name.Variable.Class),
            # Simple placeholders: <name>
            (r"<[a-zA-Z_][a-zA-Z0-9_]*>", Token.Name.Variable.Class),
            # Optional brackets and ellipsis
            (r"[\[\]]", Token.Name.Tag),
            (r"\.\.\.", Token.Name.Tag),
            # SQL keywords (case-insensitive, word-boundary)
            (
                words(
                    (
                        "CREATE",
                        "OR",
                        "REPLACE",
                        "SEMANTIC",
                        "VIEW",
                        "IF",
                        "NOT",
                        "EXISTS",
                        "AS",
                        "TABLES",
                        "RELATIONSHIPS",
                        "FACTS",
                        "DIMENSIONS",
                        "METRICS",
                        "PRIMARY",
                        "KEY",
                        "UNIQUE",
                        "REFERENCES",
                        "USING",
                        "DROP",
                        "DESCRIBE",
                        "SHOW",
                        "SELECT",
                        "FROM",
                        "WHERE",
                        "ORDER",
                        "BY",
                        "LIMIT",
                    ),
                    prefix=r"\b",
                    suffix=r"\b",
                ),
                Token.Keyword,
            ),
            # Everything else (punctuation, identifiers, commas, parens, semicolons)
            (r"[^\s]+", Token.Text),
        ],
    }
