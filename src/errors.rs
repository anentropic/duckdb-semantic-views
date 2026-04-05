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
