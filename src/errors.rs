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
