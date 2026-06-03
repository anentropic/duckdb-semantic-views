//! Shared string utilities for fuzzy matching and word-boundary replacement.
//!
//! Extracted from `expand.rs` to break the expand <-> graph circular dependency.
//! Both `expand` and `graph` modules import from here.

/// Suggest the closest matching name from `available` using Levenshtein distance.
///
/// Returns `Some(name)` (with original casing) if the best match has an edit
/// distance of 3 or fewer characters. Returns `None` if no candidate is close
/// enough. Both the query and candidates are lowercased for comparison.
#[must_use]
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let query = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let dist = strsim::levenshtein(&query, &candidate.to_ascii_lowercase());
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, candidate));
                }
            } else {
                best = Some((dist, candidate));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// Replace all word-boundary occurrences of `needle` in `haystack` with `replacement`.
///
/// A word boundary is defined as: the character before the match (if any) is NOT
/// alphanumeric or underscore, AND the character after the match (if any) is NOT
/// alphanumeric or underscore. This prevents `net_price` from matching inside
/// `net_price_total` or `my_net_price`.
///
/// The matching is case-sensitive (fact names are identifiers).
#[must_use]
pub fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || needle.len() > haystack.len() {
        return haystack.to_string();
    }

    let h_bytes = haystack.as_bytes();
    let n_bytes = needle.as_bytes();
    let n_len = n_bytes.len();

    let mut result = String::with_capacity(haystack.len());
    let mut i = 0;

    while i + n_len <= h_bytes.len() {
        if &h_bytes[i..i + n_len] == n_bytes {
            let before_ok = i == 0 || is_word_boundary_char(h_bytes[i - 1]);
            let after_ok = i + n_len == h_bytes.len() || is_word_boundary_char(h_bytes[i + n_len]);
            if before_ok && after_ok {
                result.push_str(replacement);
                i += n_len;
                continue;
            }
        }
        result.push(haystack[i..].chars().next().unwrap());
        i += 1;
    }
    // Append remaining bytes that are shorter than needle
    if i < haystack.len() {
        result.push_str(&haystack[i..]);
    }
    result
}

/// Replace word-boundary occurrences of *any* needle in `needles` with `replacement`,
/// in a single left-to-right pass.
///
/// At each position the needles are tried in the given order — put the more specific
/// (e.g. qualified `alias.name`) needle first so it wins over a shorter one (`name`).
/// On a match the replacement is emitted and scanning resumes *after* the matched
/// needle: the inserted replacement text is never re-scanned.
///
/// This matters when the replacement itself contains one of the needles. For an
/// identity fact (`name` whose expression is the qualified column `alias.name`), two
/// sequential [`replace_word_boundary`] calls — qualified then unqualified — would
/// double-substitute (`alias.name` -> `(alias.name)` -> `(alias.(alias.name))`).
/// A single combined pass avoids that.
#[must_use]
pub fn replace_word_boundary_any(haystack: &str, needles: &[&str], replacement: &str) -> String {
    let h_bytes = haystack.as_bytes();
    let mut result = String::with_capacity(haystack.len());
    let mut i = 0;

    while i < h_bytes.len() {
        let mut matched = false;
        for &needle in needles {
            let n_bytes = needle.as_bytes();
            let n_len = n_bytes.len();
            if n_len == 0 || i + n_len > h_bytes.len() {
                continue;
            }
            if &h_bytes[i..i + n_len] == n_bytes {
                let before_ok = i == 0 || is_word_boundary_char(h_bytes[i - 1]);
                let after_ok =
                    i + n_len == h_bytes.len() || is_word_boundary_char(h_bytes[i + n_len]);
                if before_ok && after_ok {
                    result.push_str(replacement);
                    i += n_len;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            // Advance by a full UTF-8 char so `i` stays on a char boundary.
            let ch = haystack[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
    }

    result
}

/// Check if a byte is a word-boundary character (NOT alphanumeric or underscore).
#[must_use]
pub fn is_word_boundary_char(b: u8) -> bool {
    !b.is_ascii_alphanumeric() && b != b'_'
}

/// Wrap a closure in `catch_unwind`, converting panics to `Box<dyn Error>`.
///
/// Used at FFI boundaries to prevent Rust panics from unwinding through C++ frames
/// (which is undefined behavior). The closure must return `Result<T, Box<dyn Error>>`.
///
/// On panic, the payload is inspected for `&str` or `String` messages to produce
/// a descriptive error. Unknown payloads produce a generic "unknown cause" message.
pub fn catch_unwind_to_result<F, T>(f: F) -> Result<T, Box<dyn std::error::Error>>
where
    F: FnOnce() -> Result<T, Box<dyn std::error::Error>> + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                format!("internal error (panic): {s}")
            } else if let Some(s) = payload.downcast_ref::<String>() {
                format!("internal error (panic): {s}")
            } else {
                "internal error (panic): unknown cause".to_string()
            };
            Err(msg.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // -------------------------------------------------------------------
    // replace_word_boundary tests
    // -------------------------------------------------------------------

    #[test]
    fn replace_word_boundary_no_match() {
        let result = replace_word_boundary("SUM(total)", "net_price", "(x)");
        assert_eq!(result, "SUM(total)");
    }

    #[test]
    fn replace_word_boundary_exact_match_in_function() {
        let result =
            replace_word_boundary("SUM(net_price)", "net_price", "(price * (1 - discount))");
        assert_eq!(result, "SUM((price * (1 - discount)))");
    }

    #[test]
    fn replace_word_boundary_no_substring_match_suffix() {
        // "net_price" should NOT match in "net_price_total"
        let result = replace_word_boundary("SUM(net_price_total)", "net_price", "(x)");
        assert_eq!(result, "SUM(net_price_total)");
    }

    #[test]
    fn replace_word_boundary_no_substring_match_prefix() {
        // "net_price" should NOT match in "total_net_price_x"
        let result = replace_word_boundary("total_net_price_x + 1", "net_price", "(x)");
        assert_eq!(result, "total_net_price_x + 1");
    }

    #[test]
    fn replace_word_boundary_match_with_addition() {
        let result = replace_word_boundary("net_price + tax", "net_price", "(a + b)");
        assert_eq!(result, "(a + b) + tax");
    }

    #[test]
    fn replace_word_boundary_match_in_parens() {
        let result = replace_word_boundary("(net_price)", "net_price", "(a)");
        assert_eq!(result, "((a))");
    }

    #[test]
    fn replace_word_boundary_entire_string() {
        let result = replace_word_boundary("net_price", "net_price", "(a + b)");
        assert_eq!(result, "(a + b)");
    }

    #[test]
    fn replace_word_boundary_at_start() {
        let result = replace_word_boundary("net_price * 2", "net_price", "(x)");
        assert_eq!(result, "(x) * 2");
    }

    #[test]
    fn replace_word_boundary_at_end() {
        let result = replace_word_boundary("2 * net_price", "net_price", "(x)");
        assert_eq!(result, "2 * (x)");
    }

    #[test]
    fn replace_word_boundary_multiple_occurrences() {
        let result = replace_word_boundary("net_price + net_price", "net_price", "(x)");
        assert_eq!(result, "(x) + (x)");
    }

    #[test]
    fn replace_word_boundary_empty_needle() {
        let result = replace_word_boundary("abc", "", "x");
        assert_eq!(result, "abc");
    }

    // -------------------------------------------------------------------
    // replace_word_boundary_any tests
    // -------------------------------------------------------------------

    #[test]
    fn replace_any_qualified_wins_over_unqualified() {
        // Qualified needle is tried first; the unqualified `unit_price` inside the
        // emitted replacement must NOT be re-scanned (no double substitution).
        let result = replace_word_boundary_any(
            "s.unit_price",
            &["s.unit_price", "unit_price"],
            "(s.unit_price)",
        );
        assert_eq!(result, "(s.unit_price)");
    }

    #[test]
    fn replace_any_unqualified_fallback() {
        // When only the unqualified form appears, it still matches.
        let result =
            replace_word_boundary_any("SUM(unit_price)", &["s.unit_price", "unit_price"], "(x)");
        assert_eq!(result, "SUM((x))");
    }

    #[test]
    fn replace_any_inside_qualified_metric() {
        // Metric referencing an identity fact by its qualified column.
        let result = replace_word_boundary_any(
            "SUM(s.unit_price)",
            &["s.unit_price", "unit_price"],
            "(s.unit_price)",
        );
        assert_eq!(result, "SUM((s.unit_price))");
    }

    #[test]
    fn replace_any_no_substring_match() {
        let result =
            replace_word_boundary_any("unit_price_total", &["s.unit_price", "unit_price"], "(x)");
        assert_eq!(result, "unit_price_total");
    }

    #[test]
    fn replace_any_utf8_passthrough() {
        // Non-ASCII, non-matching content must not panic and must round-trip.
        let result = replace_word_boundary_any("héllo + unit_price", &["unit_price"], "(x)");
        assert_eq!(result, "héllo + (x)");
    }

    // -------------------------------------------------------------------
    // suggest_closest property tests
    // -------------------------------------------------------------------

    proptest! {
        /// Any suggestion returned by suggest_closest must be a member of the
        /// input `available` list. This prevents the function from inventing
        /// names that don't exist in the model.
        #[test]
        fn suggestion_is_always_valid_name(
            query in "[a-z_]{1,20}",
            names in prop::collection::vec("[a-z_]{1,20}", 1..20)
        ) {
            if let Some(suggestion) = suggest_closest(&query, &names) {
                prop_assert!(
                    names.contains(&suggestion),
                    "suggest_closest returned '{}' which is not in available names: {:?}",
                    suggestion,
                    names
                );
            }
        }

        /// An exact match (query == one of the available names) should always
        /// produce a suggestion, since edit distance is 0 which is within the
        /// threshold of 3.
        #[test]
        fn exact_match_always_suggests(
            name in "[a-z_]{1,20}",
            others in prop::collection::vec("[a-z_]{1,20}", 0..10)
        ) {
            let mut names = others;
            names.push(name.clone());
            let suggestion = suggest_closest(&name, &names);
            prop_assert!(
                suggestion.is_some(),
                "exact match '{}' should always produce a suggestion",
                name
            );
            prop_assert_eq!(
                suggestion.unwrap(),
                name,
                "exact match should suggest itself"
            );
        }

        /// When the available list is empty, suggest_closest must return None.
        #[test]
        fn empty_names_returns_none(
            query in "[a-z_]{1,20}"
        ) {
            let names: Vec<String> = vec![];
            prop_assert!(suggest_closest(&query, &names).is_none());
        }
    }
}
