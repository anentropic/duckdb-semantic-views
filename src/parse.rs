// Parse detection for `CREATE SEMANTIC VIEW` statements.
//
// This module provides two layers:
// 1. A pure detection function (`detect_create_semantic_view`) that is
//    testable under `cargo test` without the extension feature.
// 2. An FFI entry point (`sv_parse_rust`) that wraps the detection in
//    `catch_unwind` for panic safety, feature-gated on `extension`.

/// Not our statement -- return `DISPLAY_ORIGINAL_ERROR`.
pub const PARSE_NOT_OURS: u8 = 0;
/// Detected `CREATE SEMANTIC VIEW` -- return `PARSE_SUCCESSFUL`.
pub const PARSE_DETECTED: u8 = 1;

/// Detect whether a query is a `CREATE SEMANTIC VIEW` statement.
///
/// Handles case variations, leading/trailing whitespace, and trailing
/// semicolons (`DuckDB` inconsistently includes them per issue #18485).
///
/// This function is pure and allocation-free for the common case
/// (non-matching queries). It performs no heap allocation.
#[must_use]
pub fn detect_create_semantic_view(query: &str) -> u8 {
    let trimmed = query.trim();
    // Strip trailing semicolons -- DuckDB's `SplitQueries` re-appends `;`
    // to middle statements but not the last one (issue #18485).
    let trimmed = trimmed.trim_end_matches(';').trim();
    let prefix = "create semantic view";
    if trimmed.len() < prefix.len() {
        return PARSE_NOT_OURS;
    }
    // Compare only the prefix bytes, case-insensitively (ASCII SQL keywords).
    if trimmed.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
        PARSE_DETECTED
    } else {
        PARSE_NOT_OURS
    }
}

/// Parse a `CREATE SEMANTIC VIEW` DDL statement, extracting the view name and body.
///
/// Returns `(name, body)` where `name` is the view identifier and `body` is
/// everything between the first `(` and last `)`. Handles case variations,
/// leading/trailing whitespace, and trailing semicolons.
///
/// # Errors
///
/// Returns a descriptive error string if:
/// - The query is not a `CREATE SEMANTIC VIEW` statement
/// - The view name is missing
/// - The parenthesized body is missing
pub fn parse_ddl_text(query: &str) -> Result<(&str, &str), String> {
    let trimmed = query.trim();
    // Strip trailing semicolons (DuckDB issue #18485)
    let trimmed = trimmed.trim_end_matches(';').trim();

    let prefix = "create semantic view";
    if trimmed.len() < prefix.len()
        || !trimmed.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        return Err("Not a CREATE SEMANTIC VIEW statement".to_string());
    }

    // After the prefix, extract the view name
    let after_prefix = &trimmed[prefix.len()..];
    let after_prefix = after_prefix.trim_start();
    if after_prefix.is_empty() {
        return Err("Missing view name".to_string());
    }

    // View name ends at whitespace or '('
    let name_end = after_prefix
        .find(|c: char| c.is_whitespace() || c == '(')
        .unwrap_or(after_prefix.len());
    let name = &after_prefix[..name_end];
    if name.is_empty() {
        return Err("Missing view name".to_string());
    }

    // Find the opening paren
    let after_name = &after_prefix[name_end..];
    let open_paren_offset = after_name
        .find('(')
        .ok_or_else(|| "Expected '(' after view name".to_string())?;

    // Find the body: everything between first '(' and last ')' in the remaining text
    let from_open = &after_name[open_paren_offset..];
    let close_paren = from_open
        .rfind(')')
        .ok_or_else(|| "Expected ')' to close DDL body".to_string())?;

    // Body is between '(' and ')' (exclusive of both)
    let body = &from_open[1..close_paren];
    Ok((name, body))
}

/// Rewrite a `CREATE SEMANTIC VIEW` statement to a `create_semantic_view()` function call.
///
/// Transforms:
/// ```text
/// CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])
/// ```
/// Into:
/// ```text
/// SELECT * FROM create_semantic_view('sales', tables := [...], dimensions := [...])
/// ```
///
/// Single quotes in the view name are escaped (`'` -> `''`).
pub fn rewrite_ddl_to_function_call(query: &str) -> Result<String, String> {
    let (name, body) = parse_ddl_text(query)?;
    let safe_name = name.replace('\'', "''");
    Ok(format!(
        "SELECT * FROM create_semantic_view('{safe_name}', {body})"
    ))
}

/// FFI entry point called from C++ `sv_parse_stub`.
///
/// Wraps detection in `catch_unwind` for panic safety at the FFI boundary.
/// On any panic, returns `PARSE_NOT_OURS` (DuckDB shows its normal error).
///
/// # Safety
///
/// `query_ptr` must point to a valid byte sequence of `query_len` bytes.
/// The pointer must remain valid for the duration of this call.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_parse_rust(query_ptr: *const u8, query_len: usize) -> u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if query_ptr.is_null() || query_len == 0 {
            return PARSE_NOT_OURS;
        }
        // SAFETY: DuckDB query strings are always valid UTF-8 (ASCII SQL text).
        // Even if not, we only inspect ASCII prefix bytes.
        let query = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(query_ptr, query_len))
        };
        detect_create_semantic_view(query)
    }))
    .unwrap_or(PARSE_NOT_OURS)
}

/// Write a string into a raw byte buffer, null-terminated and truncated to `len - 1`.
///
/// # Safety
///
/// `buf` must point to a writable buffer of at least `len` bytes.
#[cfg(feature = "extension")]
unsafe fn write_to_buffer(buf: *mut u8, len: usize, s: &str) {
    if buf.is_null() || len == 0 {
        return;
    }
    let max_copy = len - 1; // reserve space for null terminator
    let copy_len = s.len().min(max_copy);
    std::ptr::copy_nonoverlapping(s.as_ptr(), buf, copy_len);
    *buf.add(copy_len) = 0; // null terminate
}

/// FFI entry point for DDL execution, called from C++ `sv_ddl_bind`.
///
/// Rewrites a `CREATE SEMANTIC VIEW` statement into a `create_semantic_view()`
/// function call and executes it on the provided connection.
///
/// On success: writes the view name to `name_out` (null-terminated), returns 0.
/// On failure: writes the error message to `error_out` (null-terminated), returns 1.
///
/// # Safety
///
/// - `query_ptr` must point to valid UTF-8 bytes of length `query_len`.
/// - `exec_conn` must be a valid, open `duckdb_connection`.
/// - `name_out` must point to a writable buffer of `name_out_len` bytes.
/// - `error_out` must point to a writable buffer of `error_out_len` bytes.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_execute_ddl_rust(
    query_ptr: *const u8,
    query_len: usize,
    exec_conn: libduckdb_sys::duckdb_connection,
    name_out: *mut u8,
    name_out_len: usize,
    error_out: *mut u8,
    error_out_len: usize,
) -> u8 {
    use libduckdb_sys as ffi;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<String, String> {
            if query_ptr.is_null() || query_len == 0 {
                return Err("Empty query".to_string());
            }
            // SAFETY: guaranteed valid UTF-8 by the caller (DuckDB query text)
            let query = unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(query_ptr, query_len))
            };

            // Rewrite DDL to function call
            let rewritten = rewrite_ddl_to_function_call(query)?;

            // Execute the rewritten SQL on the DDL connection
            let c_sql = std::ffi::CString::new(rewritten)
                .map_err(|_| "Rewritten SQL contains null byte".to_string())?;
            unsafe {
                let mut result: ffi::duckdb_result = std::mem::zeroed();
                let rc = ffi::duckdb_query(exec_conn, c_sql.as_ptr(), &mut result);
                if rc != ffi::DuckDBSuccess {
                    let err_ptr = ffi::duckdb_result_error(&mut result);
                    let err_msg = if err_ptr.is_null() {
                        "DDL execution failed (unknown error)".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(err_ptr)
                            .to_string_lossy()
                            .into_owned()
                    };
                    ffi::duckdb_destroy_result(&mut result);
                    return Err(err_msg);
                }
                ffi::duckdb_destroy_result(&mut result);
            }

            // Extract the view name from the original query
            let (name, _) = parse_ddl_text(query)?;
            Ok(name.to_string())
        },
    ));

    match result {
        Ok(Ok(name)) => {
            unsafe { write_to_buffer(name_out, name_out_len, &name) };
            0 // success
        }
        Ok(Err(err)) => {
            unsafe { write_to_buffer(error_out, error_out_len, &err) };
            1 // failure
        }
        Err(_panic) => {
            unsafe { write_to_buffer(error_out, error_out_len, "Internal panic in DDL execution") };
            1 // failure
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_detection() {
        assert_eq!(
            detect_create_semantic_view("CREATE SEMANTIC VIEW test (...)"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(
            detect_create_semantic_view("create semantic view test"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_create_semantic_view("Create Semantic View test"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_create_semantic_view("CREATE semantic VIEW test"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_leading_whitespace() {
        assert_eq!(
            detect_create_semantic_view("  CREATE SEMANTIC VIEW test"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_create_semantic_view("\n\tCREATE SEMANTIC VIEW test"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_trailing_semicolon() {
        assert_eq!(
            detect_create_semantic_view("CREATE SEMANTIC VIEW test;"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_create_semantic_view("CREATE SEMANTIC VIEW test ;"),
            PARSE_DETECTED
        );
        assert_eq!(
            detect_create_semantic_view("CREATE SEMANTIC VIEW test ;\n"),
            PARSE_DETECTED
        );
    }

    #[test]
    fn test_non_matching() {
        assert_eq!(detect_create_semantic_view("SELECT 1"), PARSE_NOT_OURS);
        assert_eq!(
            detect_create_semantic_view("CREATE TABLE test"),
            PARSE_NOT_OURS
        );
        assert_eq!(
            detect_create_semantic_view("CREATE VIEW test"),
            PARSE_NOT_OURS
        );
        assert_eq!(detect_create_semantic_view(""), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view(";"), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view("CREATE"), PARSE_NOT_OURS);
    }

    #[test]
    fn test_too_short() {
        assert_eq!(
            detect_create_semantic_view("create semantic vie"),
            PARSE_NOT_OURS
        );
    }

    #[test]
    fn test_exact_prefix_only() {
        // Exactly the prefix with nothing after -- still detected
        assert_eq!(
            detect_create_semantic_view("create semantic view"),
            PARSE_DETECTED
        );
    }

    // -----------------------------------------------------------------------
    // parse_ddl_text tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ddl_basic() {
        let (name, body) = parse_ddl_text("CREATE SEMANTIC VIEW sales (tables := [...])").unwrap();
        assert_eq!(name, "sales");
        assert_eq!(body, "tables := [...]");
    }

    #[test]
    fn test_parse_ddl_case_insensitive() {
        let (name, body) = parse_ddl_text("create semantic view My_View (a := 1)").unwrap();
        assert_eq!(name, "My_View");
        assert_eq!(body, "a := 1");
    }

    #[test]
    fn test_parse_ddl_whitespace_and_semicolon() {
        let (name, body) = parse_ddl_text("  CREATE SEMANTIC VIEW x (a := 1);").unwrap();
        assert_eq!(name, "x");
        assert_eq!(body, "a := 1");
    }

    #[test]
    fn test_parse_ddl_not_our_statement() {
        let err = parse_ddl_text("SELECT 1").unwrap_err();
        assert!(err.contains("Not a CREATE SEMANTIC VIEW"), "got: {err}");
    }

    #[test]
    fn test_parse_ddl_missing_name() {
        let err = parse_ddl_text("CREATE SEMANTIC VIEW").unwrap_err();
        assert!(err.contains("Missing view name"), "got: {err}");
    }

    #[test]
    fn test_parse_ddl_missing_parens() {
        let err = parse_ddl_text("CREATE SEMANTIC VIEW x").unwrap_err();
        assert!(err.contains("Expected '(' after view name"), "got: {err}");
    }

    #[test]
    fn test_parse_ddl_nested_parens() {
        // Nested parens in STRUCT literals -- rfind should find the last ')'
        let (name, body) = parse_ddl_text(
            "CREATE SEMANTIC VIEW v (tables := [{alias: 'a', table: 't'}], dimensions := [{name: 'x', expr: 'CAST(y AS INT)', source_table: 'a'}])"
        ).unwrap();
        assert_eq!(name, "v");
        assert!(body.starts_with("tables := ["));
        assert!(body.ends_with("'a'}]"));
    }

    #[test]
    fn test_parse_ddl_name_with_paren_adjacent() {
        // View name immediately followed by '(' with no space
        let (name, body) = parse_ddl_text("CREATE SEMANTIC VIEW myview(a := 1)").unwrap();
        assert_eq!(name, "myview");
        assert_eq!(body, "a := 1");
    }

    // -----------------------------------------------------------------------
    // rewrite_ddl_to_function_call tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rewrite_basic() {
        let sql = rewrite_ddl_to_function_call(
            "CREATE SEMANTIC VIEW sales (tables := [...], dimensions := [...])",
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM create_semantic_view('sales', tables := [...], dimensions := [...])"
        );
    }

    #[test]
    fn test_rewrite_escapes_single_quotes() {
        let sql = rewrite_ddl_to_function_call("CREATE SEMANTIC VIEW it's_a_view (tables := [])")
            .unwrap();
        assert!(sql.contains("'it''s_a_view'"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_preserves_body() {
        let sql = rewrite_ddl_to_function_call(
            "CREATE SEMANTIC VIEW v (tables := [{alias: 'sales', table: 'sales'}], dimensions := [{name: 'region', expr: 'region', source_table: 'sales'}], metrics := [{name: 'total', expr: 'SUM(amount)', source_table: 'sales'}])",
        )
        .unwrap();
        assert!(sql.starts_with("SELECT * FROM create_semantic_view('v', tables := ["));
        assert!(sql.contains("metrics := [{name: 'total'"));
    }

    #[test]
    fn test_rewrite_error_propagation() {
        let err = rewrite_ddl_to_function_call("SELECT 1").unwrap_err();
        assert!(err.contains("Not a CREATE SEMANTIC VIEW"), "got: {err}");
    }
}
