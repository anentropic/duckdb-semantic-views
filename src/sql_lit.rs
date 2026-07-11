//! `SqlLit` — a string already `''`-escaped for embedding inside a
//! single-quoted SQL string literal.
//!
//! R-1 (code-review 2026-07-11): the write-side DDL rewrite emits catalog
//! SQL by interpolating user-supplied names/comments into single-quoted
//! literals. Escaping was carried by a naming convention (`name_escaped:
//! &str`) with a free `escape_sql_arg` / `unescape_sql_arg` pair, so nothing
//! at the type level stopped a call site from (a) forgetting to escape — an
//! injection into emitted DDL — or (b) escaping an already-escaped value
//! twice. This newtype makes both a compile error: a value can only reach an
//! emission helper by going through [`SqlLit::escape`] exactly once, and a
//! raw `&str` no longer type-checks where a `&SqlLit` is required.

// The only non-test constructor call sites are the `extension`-gated write
// emitters (`crate::parse::native_sql`); under a default, non-test build the
// type is unused (the guard builders that consume it are themselves
// `allow(dead_code)` there). Mirror the escaping helpers this replaced.
#![cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]

/// A string with single quotes already SQL-doubled (`'` → `''`), ready to be
/// embedded **between** the quotes of a single-quoted SQL string literal
/// (`'{lit}'`). Construct only via [`SqlLit::escape`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqlLit(String);

impl SqlLit {
    /// Escape a raw value for embedding in a single-quoted SQL literal:
    /// doubles every `'`. This is the single entry point, so a value that
    /// exists as a `SqlLit` has been escaped exactly once. Interpolate the
    /// result with `{sql_lit}` (the `Display` impl) to emit the escaped text
    /// between the caller-supplied quotes.
    #[must_use]
    pub(crate) fn escape(raw: &str) -> Self {
        Self(raw.replace('\'', "''"))
    }
}

impl std::fmt::Display for SqlLit {
    /// Writes the escaped inner text verbatim — i.e. what belongs between the
    /// surrounding single quotes. The caller supplies the quotes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn escape_doubles_single_quotes() {
        assert_eq!(SqlLit::escape("").to_string(), "");
        assert_eq!(SqlLit::escape("plain").to_string(), "plain");
        assert_eq!(SqlLit::escape("O'Brien").to_string(), "O''Brien");
        assert_eq!(SqlLit::escape("a'b'c").to_string(), "a''b''c");
        assert_eq!(SqlLit::escape("''").to_string(), "''''");
    }

    #[test]
    fn display_emits_escaped_text_verbatim() {
        assert_eq!(format!("'{}'", SqlLit::escape("O'Brien")), "'O''Brien'");
    }

    proptest! {
        /// Escaping is idempotent at the type level: a `SqlLit` embedded in a
        /// single-quoted literal never contains an unescaped lone `'`.
        #[test]
        fn escaped_text_has_no_lone_single_quote(s in ".*") {
            let esc = SqlLit::escape(&s);
            // Every `'` in the escaped form is part of a doubled `''` pair.
            let esc_str = esc.to_string();
            let bytes = esc_str.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    prop_assert!(
                        i + 1 < bytes.len() && bytes[i + 1] == b'\'',
                        "lone single quote at {i} in {esc_str:?}"
                    );
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }
    }
}
