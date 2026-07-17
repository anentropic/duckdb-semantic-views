//! Shared error types for the semantic views parser pipeline.
//!
//! Extracted from `parse.rs` to break the parse <-> `body_parser` circular dependency.
//! Both `parse` and `body_parser` modules import from here.

/// Error from DDL validation with an optional byte offset into the original query.
///
/// The `position` field, when present, is a 0-based byte offset into the
/// original query string (before any trimming). `DuckDB` uses this to render
/// a caret (`^`) under the error location.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the original query string.
    pub position: Option<usize>,
}

impl ParseError {
    /// A validation error carrying **no** caret position.
    ///
    /// Used by the graph / semantic-validation layer (`graph::*`,
    /// [`crate::ddl::define::enrich_definition_for_create`]). Those validators
    /// receive a fully-built
    /// [`SemanticViewDefinition`](crate::model::SemanticViewDefinition) whose
    /// members hold owned names / expressions, not byte spans into the original
    /// DDL — and the original query text is no longer in scope by then — so a
    /// caret offset is genuinely not recoverable there. Many of these failures
    /// are also global/topological (a dependency cycle, an ambiguous join
    /// diamond) with no single offending token to point at.
    ///
    /// Emitting a typed `ParseError` (rather than a bare `String`) keeps one
    /// error type across the parse, graph, and query layers; `position` is
    /// honestly `None`, exactly as at the parse layer's own semantic-failure
    /// sites. See TECH-DEBT #31 for the boundary rationale and what threading
    /// real positions here would require.
    #[must_use]
    pub fn positionless(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            position: None,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `position` is caret-rendering metadata for DuckDB, not part of
        // the human-readable message, so `Display` emits the message verbatim.
        // This keeps the many existing `err.message` call sites and
        // `write!(.., "{}", err)` interchangeable.
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::ParseError;

    #[test]
    fn display_emits_message_without_position() {
        let err = ParseError {
            message: "unexpected token 'FOO'".to_string(),
            position: Some(42),
        };
        assert_eq!(err.to_string(), "unexpected token 'FOO'");
        // `Display` must agree with the `.message` field callers still read.
        assert_eq!(err.to_string(), err.message);
    }

    #[test]
    fn positionless_has_no_caret_and_displays_message() {
        let err = ParseError::positionless("duplicate metric name 'revenue'");
        assert_eq!(err.position, None);
        assert_eq!(err.to_string(), "duplicate metric name 'revenue'");
        // Accepts both &str and String.
        assert_eq!(
            ParseError::positionless(format!("cycle in {}", "facts")).message,
            "cycle in facts"
        );
    }

    #[test]
    fn usable_as_std_error() {
        fn as_dyn(e: &ParseError) -> &dyn std::error::Error {
            e
        }
        let err = ParseError {
            message: "boom".to_string(),
            position: None,
        };
        assert_eq!(as_dyn(&err).to_string(), "boom");
    }
}
